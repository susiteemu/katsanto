//! Background refresh of subscribed channels.
//!
//! Each subscribed channel is polled through its RSS feed (cheap, exact publish
//! times). New uploads are then looked up on the channel page for what RSS
//! lacks: duration, and whether it's a Short or a live stream. Channels are
//! shared: one refresh serves every subscriber.

use std::time::Duration;

use futures_util::{StreamExt, stream};
use katsanto_db::{
    feed::{self, Enrichment, FeedVideoMeta},
    now,
    videos::{self, ChannelMeta},
    websub,
};
use katsanto_yt::{
    YtError,
    model::{ChannelTab, VideoCard},
};
use rand::Rng;
use serde::Serialize;

use crate::{error::AppResult, state::AppState};

/// Channels refreshed at the same time.
const CONCURRENCY: usize = 4;
/// Upper bound on idle sleep, so schedule changes are picked up.
const MAX_IDLE: Duration = Duration::from_secs(60);
/// Videos the channel page doesn't list after this long are left unenriched.
const ENRICH_GIVE_UP_S: i64 = 24 * 3600;

/// Sent to connected browsers when a channel has new uploads.
#[derive(Clone, Debug, Serialize)]
pub struct FeedEvent {
    pub channel_id: String,
    pub video_ids: Vec<String>,
}

pub fn spawn(st: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match run_once(&st).await {
                Ok(0) => idle(&st).await,
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "refresh loop failed");
                    tokio::time::sleep(MAX_IDLE).await;
                }
            }
        }
    })
}

/// Sleep until the next channel is due or someone subscribes.
async fn idle(st: &AppState) {
    let wait = match feed::next_due_at(st.db.pool()).await {
        Ok(Some(t)) => Duration::from_secs((t - now()).clamp(1, MAX_IDLE.as_secs() as i64) as u64),
        _ => MAX_IDLE,
    };
    tokio::select! {
        _ = tokio::time::sleep(wait) => {}
        _ = st.refresh_wakeup.notified() => {}
    }
}

/// Refresh the channels that are due. Returns how many were processed.
pub async fn run_once(st: &AppState) -> AppResult<usize> {
    let due = feed::due_channels(st.db.pool(), (CONCURRENCY * 4) as i64).await?;
    let count = due.len();
    stream::iter(due)
        .for_each_concurrent(CONCURRENCY, |id| async move {
            if let Err(e) = refresh_channel(st, &id).await {
                tracing::warn!(channel = id, error = %e, "channel refresh failed");
            }
        })
        .await;
    Ok(count)
}

pub async fn refresh_channel(st: &AppState, channel_id: &str) -> AppResult<()> {
    let state = feed::refresh_state(st.db.pool(), channel_id).await?;
    let errors = state.as_ref().map_or(0, |s| s.fetch_errors);
    let fetched = match st.yt.channel_feed(channel_id).await {
        Ok(rss) => from_rss(st, channel_id, rss).await?,
        // YouTube's RSS feeds sometimes fail for hours, also for channels that are fine.
        // The channel page then provides the uploads, and decides whether the channel
        // is really gone.
        Err(rss_error) => match from_page(st, channel_id).await? {
            Ok(f) => {
                tracing::debug!(channel = channel_id, error = %rss_error, "RSS failed; used the channel page");
                f
            }
            Err(e) => {
                let retry = match e {
                    // Deleted or terminated channels: check rarely.
                    YtError::NotFound(_) => 24 * 3600,
                    _ => backoff_s(errors),
                };
                feed::mark_failed(st.db.pool(), channel_id, &e.to_string(), now() + retry).await?;
                return Ok(());
            }
        },
    };
    let Fetched { new_ids, newest } = fetched;
    let last_video_at = newest.or(state.and_then(|s| s.last_video_at));
    let mut interval = interval_s(last_video_at);
    // The hub announces uploads; polling is only the safety net then.
    if st.config.websub && websub::is_pushed(st.db.pool(), channel_id).await? {
        interval = interval.max(crate::websub::PUSHED_POLL_S);
    }
    feed::mark_fetched(st.db.pool(), channel_id, newest, now() + interval).await?;

    if !new_ids.is_empty() {
        tracing::debug!(channel = channel_id, new = new_ids.len(), "new uploads");
        // Nobody listening is fine.
        let _ = st.feed_events.send(FeedEvent {
            channel_id: channel_id.to_owned(),
            video_ids: new_ids,
        });
    }
    Ok(())
}

struct Fetched {
    new_ids: Vec<String>,
    /// Newest upload's publish time.
    newest: Option<i64>,
}

/// Store the uploads from the channel's RSS feed, then look up what RSS lacks.
async fn from_rss(
    st: &AppState,
    channel_id: &str,
    rss: katsanto_yt::model::ChannelFeed,
) -> AppResult<Fetched> {
    let mut conn = st.db.acquire().await?;
    if !rss.channel_name.is_empty() {
        videos::upsert_channel(
            &mut *conn,
            &ChannelMeta {
                id: channel_id,
                name: &rss.channel_name,
                handle: None,
                avatar_url: None,
                subscriber_count: None,
            },
        )
        .await?;
    }
    let mut new_ids = Vec::new();
    for v in &rss.videos {
        let is_new = feed::upsert_from_rss(
            &mut conn,
            &FeedVideoMeta {
                id: &v.id,
                channel_id,
                title: &v.title,
                thumbnail_url: v.thumbnail_url.as_deref(),
                published_at: v.published_at,
                view_count: v.view_count.map(|n| n as i64),
                is_short: v.is_short,
            },
        )
        .await?;
        if is_new {
            new_ids.push(v.id.clone());
        }
    }
    drop(conn);

    let pending = feed::unenriched(st.db.pool(), channel_id).await?;
    if !pending.is_empty()
        && let Err(e) = enrich(st, channel_id, pending, &rss.videos).await
    {
        // Durations can wait for the next refresh; the uploads are stored.
        tracing::info!(channel = channel_id, error = %e, "enrichment failed");
    }

    Ok(Fetched {
        new_ids,
        newest: rss.videos.iter().map(|v| v.published_at).max(),
    })
}

/// Store the uploads listed on the channel's Videos tab (the RSS fallback). Its cards
/// come with durations but only relative dates ("3 hours ago"), which become estimates.
/// The outer error is ours; the inner one YouTube's.
async fn from_page(st: &AppState, channel_id: &str) -> AppResult<Result<Fetched, YtError>> {
    let page = match st.yt.channel(channel_id, ChannelTab::Videos).await {
        Ok(p) => p,
        Err(e) => return Ok(Err(e)),
    };
    let info = &page.channel;
    let mut conn = st.db.acquire().await?;
    videos::upsert_channel(
        &mut *conn,
        &ChannelMeta {
            id: channel_id,
            name: &info.name,
            handle: info.handle.as_deref(),
            avatar_url: info.avatar_url.as_deref(),
            subscriber_count: info.subscriber_count.map(|n| n as i64),
        },
    )
    .await?;
    let mut new_ids = Vec::new();
    let mut newest = None;
    for v in page.videos.items.iter().filter(|v| !v.is_upcoming) {
        let Some(published_at) = v.published_at else {
            continue;
        };
        let is_new = feed::upsert_from_page(
            &mut conn,
            &FeedVideoMeta {
                id: &v.id,
                channel_id,
                title: &v.title,
                thumbnail_url: v.thumbnail_url.as_deref(),
                published_at,
                view_count: v.view_count.map(|n| n as i64),
                is_short: v.is_short,
            },
            v.duration_s.map(i64::from),
            v.is_live,
        )
        .await?;
        if is_new {
            new_ids.push(v.id.clone());
        }
        newest = newest.max(Some(published_at));
    }
    Ok(Ok(Fetched { new_ids, newest }))
}

/// Look up unenriched videos on the channel's tabs: Videos first, then Shorts
/// and Live for whatever is still missing.
async fn enrich(
    st: &AppState,
    channel_id: &str,
    mut pending: Vec<String>,
    rss: &[katsanto_yt::model::FeedVideo],
) -> AppResult<()> {
    for tab in [ChannelTab::Videos, ChannelTab::Shorts, ChannelTab::Live] {
        if pending.is_empty() {
            break;
        }
        // Not the cached `st.channel`: it may predate the uploads we're looking for.
        let page = st.yt.channel(channel_id, tab).await?;
        let info = &page.channel;
        if tab == ChannelTab::Videos {
            videos::upsert_channel(
                st.db.pool(),
                &ChannelMeta {
                    id: channel_id,
                    name: &info.name,
                    handle: info.handle.as_deref(),
                    avatar_url: info.avatar_url.as_deref(),
                    subscriber_count: info.subscriber_count.map(|n| n as i64),
                },
            )
            .await?;
        }
        let found: Vec<&VideoCard> = page
            .videos
            .items
            .iter()
            .filter(|v| pending.contains(&v.id))
            .collect();
        for v in &found {
            feed::enrich(
                st.db.pool(),
                &Enrichment {
                    id: &v.id,
                    duration_s: v.duration_s.map(i64::from),
                    thumbnail_url: v.thumbnail_url.as_deref(),
                    is_short: tab == ChannelTab::Shorts || v.is_short,
                    is_live: v.is_live || v.is_upcoming,
                },
            )
            .await?;
        }
        pending.retain(|id| !found.iter().any(|v| &v.id == id));
        let more_tabs = match tab {
            ChannelTab::Videos => info.has_shorts || info.has_live,
            ChannelTab::Shorts => info.has_live,
            ChannelTab::Live => false,
        };
        if !more_tabs {
            break;
        }
    }

    // Not on any tab (members-only, removed, …): stop looking once it's old enough.
    let give_up: Vec<String> = pending
        .into_iter()
        .filter(|id| {
            rss.iter()
                .find(|v| &v.id == id)
                .is_none_or(|v| now() - v.published_at > ENRICH_GIVE_UP_S)
        })
        .collect();
    feed::mark_enriched(st.db.pool(), &give_up).await?;
    Ok(())
}

/// Poll active channels often and dormant ones rarely, with jitter so
/// refreshes don't bunch up.
pub fn interval_s(last_video_at: Option<i64>) -> i64 {
    let age = last_video_at.map_or(i64::MAX, |t| now() - t);
    let base: i64 = match age {
        a if a < 2 * 86400 => 20 * 60,
        a if a < 14 * 86400 => 60 * 60,
        a if a < 90 * 86400 => 3 * 3600,
        _ => 6 * 3600,
    };
    let jitter = base / 6;
    base + rand::rng().random_range(-jitter..=jitter)
}

/// 10 min, 20 min, 40 min, … up to a day.
pub fn backoff_s(consecutive_errors: i64) -> i64 {
    (600_i64 << consecutive_errors.clamp(0, 10)).min(24 * 3600)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals_follow_activity() {
        let i = |days: i64| interval_s(Some(now() - days * 86400));
        assert!((1000..=1400).contains(&i(0)));
        assert!((3000..=4200).contains(&i(5)));
        assert!((9000..=12600).contains(&i(30)));
        assert!(interval_s(None) >= 18000);
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff_s(0), 600);
        assert_eq!(backoff_s(1), 1200);
        assert_eq!(backoff_s(3), 4800);
        assert_eq!(backoff_s(50), 24 * 3600);
    }
}
