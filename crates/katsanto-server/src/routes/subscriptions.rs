//! Subscriptions, the subscription feed, and live feed updates.

use std::{convert::Infallible, time::Duration};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::{Stream, StreamExt};
use katsanto_db::{
    feed, subscriptions, tags,
    videos::{self, ChannelMeta},
};
use katsanto_yt::model::ChannelTab;
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::BroadcastStream;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{api::is_channel_id, proxy::image_url};
use crate::{
    auth::CurrentUser,
    error::{AppError, AppResult, ErrorBody},
    feed_filter::{FeedFilter, FeedPage, WatchedFilter, load_page},
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list, subscribe))
        .routes(routes!(subscribed_ids))
        .routes(routes!(unsubscribe))
        .routes(routes!(unsubscribe_many))
        .routes(routes!(get_feed))
        .routes(routes!(events))
}

#[derive(Serialize, ToSchema)]
pub struct SubscriptionInfo {
    pub channel_id: String,
    pub name: String,
    pub handle: Option<String>,
    pub avatar_url: Option<String>,
    pub subscriber_count: Option<i64>,
    pub subscribed_at: i64,
    /// Newest upload seen.
    pub last_video_at: Option<i64>,
    /// Last refresh; `null` until the first one.
    pub fetched_at: Option<i64>,
    /// Why the last refresh failed, if it did.
    pub fetch_error: Option<String>,
    pub tag_ids: Vec<i64>,
}

impl SubscriptionInfo {
    fn new(s: subscriptions::Subscription, pairs: &[(String, i64)]) -> Self {
        Self {
            tag_ids: pairs
                .iter()
                .filter(|(c, _)| *c == s.channel_id)
                .map(|(_, t)| *t)
                .collect(),
            channel_id: s.channel_id,
            name: s.name,
            handle: s.handle,
            avatar_url: s.avatar_url.as_deref().map(image_url),
            subscriber_count: s.subscriber_count,
            subscribed_at: s.subscribed_at,
            last_video_at: s.last_video_at,
            fetched_at: s.fetched_at,
            fetch_error: s.fetch_error,
        }
    }
}

#[utoipa::path(get, path = "/subscriptions", tag = "subscriptions",
    responses((status = 200, body = Vec<SubscriptionInfo>)))]
async fn list(
    State(st): State<AppState>,
    me: CurrentUser,
) -> AppResult<Json<Vec<SubscriptionInfo>>> {
    let list = subscriptions::list(st.db.pool(), me.id()).await?;
    let pairs = tags::channel_tag_pairs(st.db.pool(), me.id()).await?;
    Ok(Json(
        list.into_iter()
            .map(|s| SubscriptionInfo::new(s, &pairs))
            .collect(),
    ))
}

/// Ids of subscribed channels (for subscribe buttons).
#[utoipa::path(get, path = "/subscriptions/ids", tag = "subscriptions",
    responses((status = 200, body = Vec<String>)))]
async fn subscribed_ids(
    State(st): State<AppState>,
    me: CurrentUser,
) -> AppResult<Json<Vec<String>>> {
    Ok(Json(
        subscriptions::channel_ids(st.db.pool(), me.id()).await?,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct Subscribe {
    pub channel_id: String,
}

/// Subscribe to a channel. Its latest uploads appear in the feed within moments.
#[utoipa::path(post, path = "/subscriptions", tag = "subscriptions", request_body = Subscribe,
    responses((status = 201, body = SubscriptionInfo), (status = 200, body = SubscriptionInfo, description = "Already subscribed"), (status = "4XX", body = ErrorBody)))]
async fn subscribe(
    State(st): State<AppState>,
    me: CurrentUser,
    Json(b): Json<Subscribe>,
) -> AppResult<(StatusCode, Json<SubscriptionInfo>)> {
    if !is_channel_id(&b.channel_id) {
        return Err(AppError::BadRequest("invalid channel id".into()));
    }
    // Validates the channel and gives its name and avatar right away.
    let page = st.channel(&b.channel_id, ChannelTab::Videos).await?;
    let c = &page.channel;
    videos::upsert_channel(
        st.db.pool(),
        &ChannelMeta {
            id: &c.id,
            name: &c.name,
            handle: c.handle.as_deref(),
            avatar_url: c.avatar_url.as_deref(),
            subscriber_count: c.subscriber_count.map(|n| n as i64),
        },
    )
    .await?;
    let created = subscriptions::subscribe(st.db.pool(), me.id(), &c.id).await?;
    if created {
        feed::schedule_now(st.db.pool(), &c.id).await?;
        st.refresh_wakeup.notify_one();
    }
    let sub = subscriptions::list(st.db.pool(), me.id())
        .await?
        .into_iter()
        .find(|s| s.channel_id == c.id)
        .ok_or_else(|| AppError::internal("subscription vanished"))?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    let pairs = tags::channel_tag_pairs(st.db.pool(), me.id()).await?;
    Ok((status, Json(SubscriptionInfo::new(sub, &pairs))))
}

#[utoipa::path(delete, path = "/subscriptions/{channel_id}", tag = "subscriptions",
    responses((status = 204), (status = 404, body = ErrorBody)))]
async fn unsubscribe(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(channel_id): Path<String>,
) -> AppResult<StatusCode> {
    if subscriptions::unsubscribe(st.db.pool(), me.id(), &[channel_id]).await? == 0 {
        return Err(AppError::NotFound("subscription".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct Unsubscribe {
    pub channel_ids: Vec<String>,
}

#[derive(Serialize, ToSchema)]
pub struct Removed {
    pub removed: u64,
}

/// Unsubscribe from several channels at once.
#[utoipa::path(post, path = "/subscriptions/remove", tag = "subscriptions", request_body = Unsubscribe,
    responses((status = 200, body = Removed)))]
async fn unsubscribe_many(
    State(st): State<AppState>,
    me: CurrentUser,
    Json(b): Json<Unsubscribe>,
) -> AppResult<Json<Removed>> {
    let removed = subscriptions::unsubscribe(st.db.pool(), me.id(), &b.channel_ids).await?;
    Ok(Json(Removed { removed }))
}

#[derive(Deserialize, IntoParams)]
pub struct FeedParams {
    /// From a previous page.
    cursor: Option<String>,
    /// Include Shorts (default false).
    #[serde(default)]
    shorts: bool,
    /// Include finished videos (default true).
    watched: Option<bool>,
}

/// Videos from subscribed channels, newest first.
#[utoipa::path(get, path = "/feed", tag = "subscriptions", params(FeedParams),
    responses((status = 200, body = FeedPage), (status = "4XX", body = ErrorBody)))]
async fn get_feed(
    State(st): State<AppState>,
    me: CurrentUser,
    Query(p): Query<FeedParams>,
) -> AppResult<Json<FeedPage>> {
    let filter = FeedFilter {
        shorts: p.shorts,
        watched: if p.watched.unwrap_or(true) {
            WatchedFilter::All
        } else {
            WatchedFilter::HideFinished
        },
        ..FeedFilter::default()
    };
    Ok(Json(
        load_page(&st, me.id(), &filter, p.cursor.as_deref()).await?,
    ))
}

/// Server-sent events. `feed` events announce new uploads from subscribed
/// channels: `{"channel_id": "UC…", "count": 2}`.
#[utoipa::path(get, path = "/events", tag = "subscriptions",
    responses((status = 200, content_type = "text/event-stream", body = String)))]
async fn events(
    State(st): State<AppState>,
    me: CurrentUser,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let user_id = me.id();
    let pool = st.db.pool().clone();
    let stream = BroadcastStream::new(st.feed_events.subscribe()).filter_map(move |ev| {
        let pool = pool.clone();
        async move {
            // Lagging receivers just miss events; the feed is reloaded on demand anyway.
            let ev = ev.ok()?;
            let subscribed = subscriptions::channel_ids(&pool, user_id).await.ok()?;
            subscribed.contains(&ev.channel_id).then(|| {
                let data =
                    serde_json::json!({ "channel_id": ev.channel_id, "count": ev.video_ids.len() });
                Ok(Event::default().event("feed").data(data.to_string()))
            })
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(25)))
}
