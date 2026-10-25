//! Cached video and channel metadata.

use serde::Serialize;
use sqlx::FromRow;

use crate::{DbResult, Executor, now};

pub struct ChannelMeta<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub handle: Option<&'a str>,
    pub avatar_url: Option<&'a str>,
    pub subscriber_count: Option<i64>,
}

/// Insert or update a channel. Missing optional fields keep their stored values,
/// so a sparse source (e.g. the player response) never erases richer data.
pub async fn upsert_channel(e: impl Executor<'_>, c: &ChannelMeta<'_>) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO channels (id, name, handle, avatar_url, subscriber_count, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           handle = COALESCE(excluded.handle, channels.handle),
           avatar_url = COALESCE(excluded.avatar_url, channels.avatar_url),
           subscriber_count = COALESCE(excluded.subscriber_count, channels.subscriber_count),
           updated_at = excluded.updated_at",
    )
    .bind(c.id)
    .bind(c.name)
    .bind(c.handle)
    .bind(c.avatar_url)
    .bind(c.subscriber_count)
    .bind(now())
    .execute(e)
    .await?;
    Ok(())
}

pub struct VideoMeta<'a> {
    pub id: &'a str,
    pub channel_id: &'a str,
    pub title: &'a str,
    pub thumbnail_url: Option<&'a str>,
    pub duration_s: Option<i64>,
    pub published_at: Option<i64>,
    pub is_short: bool,
    pub is_live: bool,
}

/// Insert or update a video; the channel must exist. Like channels, missing
/// optional fields keep stored values.
pub async fn upsert_video(e: impl Executor<'_>, v: &VideoMeta<'_>) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO videos (id, channel_id, title, thumbnail_url, duration_s, published_at, is_short, is_live, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
           channel_id = excluded.channel_id,
           title = excluded.title,
           thumbnail_url = COALESCE(excluded.thumbnail_url, videos.thumbnail_url),
           duration_s = COALESCE(excluded.duration_s, videos.duration_s),
           published_at = COALESCE(excluded.published_at, videos.published_at),
           is_short = excluded.is_short OR videos.is_short,
           is_live = excluded.is_live,
           updated_at = excluded.updated_at",
    )
    .bind(v.id)
    .bind(v.channel_id)
    .bind(v.title)
    .bind(v.thumbnail_url)
    .bind(v.duration_s)
    .bind(v.published_at)
    .bind(v.is_short)
    .bind(v.is_live)
    .bind(now())
    .execute(e)
    .await?;
    Ok(())
}

pub async fn duration(e: impl Executor<'_>, id: &str) -> DbResult<Option<i64>> {
    Ok(
        sqlx::query_scalar::<_, Option<i64>>("SELECT duration_s FROM videos WHERE id = ?")
            .bind(id)
            .fetch_optional(e)
            .await?
            .flatten(),
    )
}

pub async fn exists(e: impl Executor<'_>, id: &str) -> DbResult<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM videos WHERE id = ?)")
            .bind(id)
            .fetch_one(e)
            .await?,
    )
}

/// A stored video with its channel, as shown on cards.
#[derive(Clone, Debug, FromRow, Serialize)]
pub struct VideoRow {
    pub id: String,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub duration_s: Option<i64>,
    pub published_at: Option<i64>,
    pub is_short: bool,
    pub is_live: bool,
    pub channel_id: String,
    pub channel_name: String,
    pub channel_avatar_url: Option<String>,
}

pub(crate) const VIDEO_ROW_COLUMNS: &str = "v.id, v.title, v.thumbnail_url, v.duration_s, v.published_at,
     v.is_short, v.is_live, v.channel_id, c.name AS channel_name, c.avatar_url AS channel_avatar_url";
