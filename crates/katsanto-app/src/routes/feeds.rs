//! Tags on channels and saved feeds.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use katsanto_db::{Db, feeds, subscriptions, tags};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    auth::CurrentUser,
    error::{AppError, AppResult, ErrorBody},
    feed_filter::{FeedFilter, FeedPage, load_page},
    state::AppState,
};

const MAX_TAGS: usize = 100;
const MAX_FEEDS: usize = 50;
/// Tag colors the app offers; empty means the default.
const TAG_COLORS: &[&str] = &[
    "", "red", "orange", "amber", "green", "teal", "blue", "violet", "pink", "gray",
];

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_tags, create_tag))
        .routes(routes!(update_tag, delete_tag))
        .routes(routes!(tag_channels))
        .routes(routes!(set_channel_tags))
        .routes(routes!(list_feeds, create_feed))
        .routes(routes!(update_feed, delete_feed))
        .routes(routes!(reorder_feeds))
        .routes(routes!(feed_videos))
        .routes(routes!(preview))
}

// ---------- tags ----------

#[derive(Serialize, ToSchema)]
pub struct TagInfo {
    pub id: i64,
    pub name: String,
    /// One of the palette names (`red`, `blue`, …) or empty.
    pub color: String,
}

impl From<tags::Tag> for TagInfo {
    fn from(t: tags::Tag) -> Self {
        Self {
            id: t.id,
            name: t.name,
            color: t.color,
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct TagBody {
    pub name: String,
    #[serde(default)]
    pub color: String,
}

fn validate_tag(b: &TagBody) -> AppResult<String> {
    let name = b.name.trim();
    if name.is_empty() || name.chars().count() > 30 {
        return Err(AppError::BadRequest("Tag names are 1–30 characters".into()));
    }
    if !TAG_COLORS.contains(&b.color.as_str()) {
        return Err(AppError::BadRequest("unknown tag color".into()));
    }
    Ok(name.to_owned())
}

#[utoipa::path(get, path = "/tags", tag = "feeds", responses((status = 200, body = Vec<TagInfo>)))]
async fn list_tags(State(st): State<AppState>, me: CurrentUser) -> AppResult<Json<Vec<TagInfo>>> {
    let list = tags::list(st.db.pool(), me.id()).await?;
    Ok(Json(list.into_iter().map(Into::into).collect()))
}

#[utoipa::path(post, path = "/tags", tag = "feeds", request_body = TagBody,
    responses((status = 201, body = TagInfo), (status = "4XX", body = ErrorBody)))]
async fn create_tag(
    State(st): State<AppState>,
    me: CurrentUser,
    Json(b): Json<TagBody>,
) -> AppResult<(StatusCode, Json<TagInfo>)> {
    let name = validate_tag(&b)?;
    if tags::list(st.db.pool(), me.id()).await?.len() >= MAX_TAGS {
        return Err(AppError::Conflict(format!(
            "You can have at most {MAX_TAGS} tags"
        )));
    }
    let tag = tags::create(st.db.pool(), me.id(), &name, &b.color)
        .await
        .map_err(|_| AppError::Conflict(format!("You already have a tag named “{name}”")))?;
    Ok((StatusCode::CREATED, Json(tag.into())))
}

#[utoipa::path(put, path = "/tags/{id}", tag = "feeds", request_body = TagBody,
    responses((status = 204), (status = "4XX", body = ErrorBody)))]
async fn update_tag(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(id): Path<i64>,
    Json(b): Json<TagBody>,
) -> AppResult<StatusCode> {
    let name = validate_tag(&b)?;
    let updated = tags::update(st.db.pool(), me.id(), id, &name, &b.color)
        .await
        .map_err(|_| AppError::Conflict(format!("You already have a tag named “{name}”")))?;
    if !updated {
        return Err(AppError::NotFound("tag".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Delete a tag. Feeds that used it simply stop matching on it.
#[utoipa::path(delete, path = "/tags/{id}", tag = "feeds",
    responses((status = 204), (status = 404, body = ErrorBody)))]
async fn delete_tag(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    if !tags::delete(st.db.pool(), me.id(), id).await? {
        return Err(AppError::NotFound("tag".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct TagChannels {
    pub channel_ids: Vec<String>,
    /// `true` adds the tag, `false` removes it.
    pub add: bool,
}

/// Add or remove a tag on several subscribed channels.
#[utoipa::path(post, path = "/tags/{id}/channels", tag = "feeds", request_body = TagChannels,
    responses((status = 204), (status = "4XX", body = ErrorBody)))]
async fn tag_channels(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(id): Path<i64>,
    Json(b): Json<TagChannels>,
) -> AppResult<StatusCode> {
    if tags::count_owned(st.db.pool(), me.id(), &[id]).await? == 0 {
        return Err(AppError::NotFound("tag".into()));
    }
    let subscribed = subscriptions::channel_ids(st.db.pool(), me.id()).await?;
    let channels: Vec<String> = b
        .channel_ids
        .into_iter()
        .filter(|c| subscribed.contains(c))
        .collect();
    let mut tx = st.db.begin().await?;
    tags::tag_channels(&mut tx, me.id(), id, &channels, b.add).await?;
    Db::commit(tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct ChannelTags {
    pub tag_ids: Vec<i64>,
}

/// Replace a subscribed channel's tags.
#[utoipa::path(put, path = "/subscriptions/{channel_id}/tags", tag = "feeds", request_body = ChannelTags,
    responses((status = 204), (status = "4XX", body = ErrorBody)))]
async fn set_channel_tags(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(channel_id): Path<String>,
    Json(b): Json<ChannelTags>,
) -> AppResult<StatusCode> {
    if !subscriptions::channel_ids(st.db.pool(), me.id())
        .await?
        .contains(&channel_id)
    {
        return Err(AppError::NotFound("subscription".into()));
    }
    let mut ids = b.tag_ids;
    ids.sort_unstable();
    ids.dedup();
    if tags::count_owned(st.db.pool(), me.id(), &ids).await? != ids.len() as i64 {
        return Err(AppError::BadRequest("unknown tag".into()));
    }
    let mut tx = st.db.begin().await?;
    tags::set_channel_tags(&mut tx, me.id(), &channel_id, &ids).await?;
    Db::commit(tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- feeds ----------

#[derive(Serialize, ToSchema)]
pub struct FeedInfo {
    pub id: i64,
    pub name: String,
    /// An emoji or short text shown with the name.
    pub icon: String,
    pub filter: FeedFilter,
}

impl TryFrom<feeds::Feed> for FeedInfo {
    type Error = AppError;
    fn try_from(f: feeds::Feed) -> AppResult<Self> {
        Ok(Self {
            filter: serde_json::from_str(&f.filter).unwrap_or_default(),
            id: f.id,
            name: f.name,
            icon: f.icon,
        })
    }
}

#[derive(Deserialize, ToSchema)]
pub struct FeedBody {
    pub name: String,
    #[serde(default)]
    pub icon: String,
    pub filter: FeedFilter,
}

fn validate_feed(b: FeedBody) -> AppResult<(String, String, String)> {
    let name = b.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 40 {
        return Err(AppError::BadRequest(
            "Feed names are 1–40 characters".into(),
        ));
    }
    let icon = b.icon.trim().to_owned();
    if icon.chars().count() > 4 {
        return Err(AppError::BadRequest("The icon is too long".into()));
    }
    let filter = serde_json::to_string(&b.filter.normalized()?).map_err(AppError::internal)?;
    Ok((name, icon, filter))
}

#[utoipa::path(get, path = "/feeds", tag = "feeds", responses((status = 200, body = Vec<FeedInfo>)))]
async fn list_feeds(State(st): State<AppState>, me: CurrentUser) -> AppResult<Json<Vec<FeedInfo>>> {
    let list = feeds::list(st.db.pool(), me.id()).await?;
    Ok(Json(
        list.into_iter()
            .map(FeedInfo::try_from)
            .collect::<AppResult<_>>()?,
    ))
}

#[utoipa::path(post, path = "/feeds", tag = "feeds", request_body = FeedBody,
    responses((status = 201, body = FeedInfo), (status = "4XX", body = ErrorBody)))]
async fn create_feed(
    State(st): State<AppState>,
    me: CurrentUser,
    Json(b): Json<FeedBody>,
) -> AppResult<(StatusCode, Json<FeedInfo>)> {
    let (name, icon, filter) = validate_feed(b)?;
    if feeds::list(st.db.pool(), me.id()).await?.len() >= MAX_FEEDS {
        return Err(AppError::Conflict(format!(
            "You can have at most {MAX_FEEDS} feeds"
        )));
    }
    let feed = feeds::create(st.db.pool(), me.id(), &name, &icon, &filter).await?;
    Ok((StatusCode::CREATED, Json(feed.try_into()?)))
}

#[utoipa::path(put, path = "/feeds/{id}", tag = "feeds", request_body = FeedBody,
    responses((status = 200, body = FeedInfo), (status = "4XX", body = ErrorBody)))]
async fn update_feed(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(id): Path<i64>,
    Json(b): Json<FeedBody>,
) -> AppResult<Json<FeedInfo>> {
    let (name, icon, filter) = validate_feed(b)?;
    let feed = feeds::update(st.db.pool(), me.id(), id, &name, &icon, &filter)
        .await?
        .ok_or_else(|| AppError::NotFound("feed".into()))?;
    Ok(Json(feed.try_into()?))
}

#[utoipa::path(delete, path = "/feeds/{id}", tag = "feeds",
    responses((status = 204), (status = 404, body = ErrorBody)))]
async fn delete_feed(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    if !feeds::delete(st.db.pool(), me.id(), id).await? {
        return Err(AppError::NotFound("feed".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct FeedOrder {
    /// Feed ids in the new order.
    pub ids: Vec<i64>,
}

#[utoipa::path(put, path = "/feeds/order", tag = "feeds", request_body = FeedOrder,
    responses((status = 204)))]
async fn reorder_feeds(
    State(st): State<AppState>,
    me: CurrentUser,
    Json(b): Json<FeedOrder>,
) -> AppResult<StatusCode> {
    let mut tx = st.db.begin().await?;
    feeds::reorder(&mut tx, me.id(), &b.ids).await?;
    Db::commit(tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, IntoParams)]
pub struct CursorParam {
    cursor: Option<String>,
}

#[utoipa::path(get, path = "/feeds/{id}/videos", tag = "feeds", params(CursorParam),
    responses((status = 200, body = FeedPage), (status = "4XX", body = ErrorBody)))]
async fn feed_videos(
    State(st): State<AppState>,
    me: CurrentUser,
    Path(id): Path<i64>,
    Query(q): Query<CursorParam>,
) -> AppResult<Json<FeedPage>> {
    let feed: FeedInfo = feeds::get(st.db.pool(), me.id(), id)
        .await?
        .ok_or_else(|| AppError::NotFound("feed".into()))?
        .try_into()?;
    Ok(Json(
        load_page(&st, me.id(), &feed.filter, q.cursor.as_deref()).await?,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct Preview {
    pub filter: FeedFilter,
    pub cursor: Option<String>,
}

/// Videos an unsaved filter would show (feed editor preview, quick filters).
#[utoipa::path(post, path = "/feeds/preview", tag = "feeds", request_body = Preview,
    responses((status = 200, body = FeedPage), (status = "4XX", body = ErrorBody)))]
async fn preview(
    State(st): State<AppState>,
    me: CurrentUser,
    Json(b): Json<Preview>,
) -> AppResult<Json<FeedPage>> {
    let filter = b.filter.normalized()?;
    Ok(Json(
        load_page(&st, me.id(), &filter, b.cursor.as_deref()).await?,
    ))
}
