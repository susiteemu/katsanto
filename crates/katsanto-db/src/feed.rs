//! The subscription feed and the refresh bookkeeping behind it.

use serde::Serialize;
use sqlx::{FromRow, QueryBuilder, Sqlite};

use crate::{DbResult, Executor, now};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Watched {
    #[default]
    All,
    /// Leave out finished videos.
    HideFinished,
    /// Only videos never started.
    OnlyUnwatched,
    /// Only started, unfinished videos.
    InProgress,
}

/// The SQL-side part of a feed filter (title matching happens in the server).
#[derive(Clone, Debug, Default)]
pub struct FeedQuery {
    pub include_shorts: bool,
    pub include_live: bool,
    pub watched: Watched,
    /// Restrict to channels with any of these tags, or in `channels`. Both empty: all subscriptions.
    pub tag_ids: Vec<i64>,
    pub channels: Vec<String>,
    pub exclude_channels: Vec<String>,
    /// Videos of unknown duration are kept.
    pub min_duration_s: Option<i64>,
    pub max_duration_s: Option<i64>,
    pub published_after: Option<i64>,
    /// Continue after this `(published_at, video_id)`.
    pub after: Option<(i64, String)>,
    pub limit: i64,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct FeedRow {
    pub id: String,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub duration_s: Option<i64>,
    pub published_at: i64,
    pub view_count: Option<i64>,
    pub is_short: bool,
    pub is_live: bool,
    pub channel_id: String,
    pub channel_name: String,
    pub channel_avatar_url: Option<String>,
    pub position_s: Option<f64>,
    pub watched_duration_s: Option<f64>,
    pub finished: Option<bool>,
    pub progress_updated_at: Option<i64>,
}

/// Videos from the user's subscriptions, newest first.
pub async fn feed(e: impl Executor<'_>, user_id: i64, q: &FeedQuery) -> DbResult<Vec<FeedRow>> {
    let mut b: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT v.id, v.title, v.thumbnail_url, v.duration_s, v.published_at, v.view_count,
                v.is_short, v.is_live, v.channel_id, c.name AS channel_name,
                c.avatar_url AS channel_avatar_url, p.position_s,
                p.duration_s AS watched_duration_s, p.finished, p.updated_at AS progress_updated_at
         FROM videos v
         JOIN subscriptions s ON s.channel_id = v.channel_id AND s.user_id = ",
    );
    b.push_bind(user_id);
    b.push(" JOIN channels c ON c.id = v.channel_id LEFT JOIN watch_progress p ON p.video_id = v.id AND p.user_id = ");
    b.push_bind(user_id);
    b.push(" WHERE v.published_at IS NOT NULL AND NOT EXISTS (SELECT 1 FROM hidden_videos h WHERE h.video_id = v.id AND h.user_id = ");
    b.push_bind(user_id).push(")");
    if !q.include_shorts {
        b.push(" AND NOT v.is_short");
    }
    if !q.include_live {
        b.push(" AND NOT v.is_live");
    }
    match q.watched {
        Watched::All => {}
        Watched::HideFinished => {
            b.push(" AND p.finished IS NOT 1");
        }
        Watched::OnlyUnwatched => {
            b.push(" AND p.video_id IS NULL");
        }
        Watched::InProgress => {
            b.push(" AND p.finished = 0");
        }
    }
    if !q.tag_ids.is_empty() || !q.channels.is_empty() {
        b.push(" AND (v.channel_id IN (SELECT ct.channel_id FROM channel_tags ct JOIN tags t ON t.id = ct.tag_id WHERE t.user_id = ");
        b.push_bind(user_id).push(" AND ct.tag_id IN (");
        push_list(
            &mut b,
            q.tag_ids
                .iter()
                .copied()
                .chain(q.tag_ids.is_empty().then_some(-1)),
        );
        b.push(")) OR v.channel_id IN (");
        push_list(
            &mut b,
            q.channels
                .iter()
                .cloned()
                .chain(q.channels.is_empty().then(String::new)),
        );
        b.push("))");
    }
    if !q.exclude_channels.is_empty() {
        b.push(" AND v.channel_id NOT IN (");
        push_list(&mut b, q.exclude_channels.iter().cloned());
        b.push(")");
    }
    if let Some(min) = q.min_duration_s {
        b.push(" AND (v.duration_s IS NULL OR v.duration_s >= ")
            .push_bind(min)
            .push(")");
    }
    if let Some(max) = q.max_duration_s {
        b.push(" AND (v.duration_s IS NULL OR v.duration_s <= ")
            .push_bind(max)
            .push(")");
    }
    if let Some(t) = q.published_after {
        b.push(" AND v.published_at >= ").push_bind(t);
    }
    if let Some((t, id)) = &q.after {
        b.push(" AND (v.published_at, v.id) < (")
            .push_bind(*t)
            .push(", ")
            .push_bind(id)
            .push(")");
    }
    b.push(" ORDER BY v.published_at DESC, v.id DESC LIMIT ")
        .push_bind(q.limit);
    Ok(b.build_query_as().fetch_all(e).await?)
}

/// Push `a, b, c` as bind parameters.
fn push_list<'a, T>(b: &mut QueryBuilder<'a, Sqlite>, items: impl Iterator<Item = T>)
where
    T: 'a + Send + sqlx::Encode<'a, Sqlite> + sqlx::Type<Sqlite>,
{
    let mut sep = b.separated(", ");
    for item in items {
        sep.push_bind(item);
    }
}

/// Subscribed channels due for a refresh, oldest schedule first.
pub async fn due_channels(e: impl Executor<'_>, limit: i64) -> DbResult<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT c.id FROM channels c
         WHERE c.next_fetch_at <= ? AND EXISTS (SELECT 1 FROM subscriptions s WHERE s.channel_id = c.id)
         ORDER BY c.next_fetch_at LIMIT ?",
    )
    .bind(now())
    .bind(limit)
    .fetch_all(e)
    .await?)
}

/// When the earliest subscribed channel is due (for sleeping until then).
pub async fn next_due_at(e: impl Executor<'_>) -> DbResult<Option<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT MIN(c.next_fetch_at) FROM channels c
         WHERE EXISTS (SELECT 1 FROM subscriptions s WHERE s.channel_id = c.id)",
    )
    .fetch_one(e)
    .await?)
}

pub async fn schedule_now(e: impl Executor<'_>, channel_id: &str) -> DbResult<()> {
    sqlx::query("UPDATE channels SET next_fetch_at = 0 WHERE id = ?")
        .bind(channel_id)
        .execute(e)
        .await?;
    Ok(())
}

#[derive(Clone, Debug, FromRow)]
pub struct RefreshState {
    pub last_video_at: Option<i64>,
    pub fetch_errors: i64,
}

pub async fn refresh_state(
    e: impl Executor<'_>,
    channel_id: &str,
) -> DbResult<Option<RefreshState>> {
    Ok(
        sqlx::query_as("SELECT last_video_at, fetch_errors FROM channels WHERE id = ?")
            .bind(channel_id)
            .fetch_optional(e)
            .await?,
    )
}

pub async fn mark_fetched(
    e: impl Executor<'_>,
    channel_id: &str,
    last_video_at: Option<i64>,
    next_fetch_at: i64,
) -> DbResult<()> {
    sqlx::query(
        "UPDATE channels SET fetched_at = ?, next_fetch_at = ?, fetch_errors = 0, fetch_error = NULL,
           last_video_at = MAX(COALESCE(last_video_at, 0), COALESCE(?, 0))
         WHERE id = ?",
    )
    .bind(now())
    .bind(next_fetch_at)
    .bind(last_video_at)
    .bind(channel_id)
    .execute(e)
    .await?;
    Ok(())
}

pub async fn mark_failed(
    e: impl Executor<'_>,
    channel_id: &str,
    error: &str,
    next_fetch_at: i64,
) -> DbResult<()> {
    sqlx::query(
        "UPDATE channels SET fetched_at = ?, next_fetch_at = ?, fetch_errors = fetch_errors + 1, fetch_error = ?
         WHERE id = ?",
    )
    .bind(now())
    .bind(next_fetch_at)
    .bind(error)
    .bind(channel_id)
    .execute(e)
    .await?;
    Ok(())
}

pub struct FeedVideoMeta<'a> {
    pub id: &'a str,
    pub channel_id: &'a str,
    pub title: &'a str,
    pub thumbnail_url: Option<&'a str>,
    pub published_at: i64,
    pub view_count: Option<i64>,
    pub is_short: bool,
}

/// Store a video from the channel's RSS feed: its publish time is exact, so it
/// wins over an estimate. Returns true if the video is new.
pub async fn upsert_from_rss(conn: &mut crate::Conn, v: &FeedVideoMeta<'_>) -> DbResult<bool> {
    let existed = crate::videos::exists(&mut *conn, v.id).await?;
    sqlx::query(
        "INSERT INTO videos (id, channel_id, title, thumbnail_url, published_at, view_count, is_short, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
           title = excluded.title,
           thumbnail_url = COALESCE(videos.thumbnail_url, excluded.thumbnail_url),
           published_at = excluded.published_at,
           view_count = excluded.view_count,
           is_short = videos.is_short OR excluded.is_short,
           updated_at = excluded.updated_at",
    )
    .bind(v.id)
    .bind(v.channel_id)
    .bind(v.title)
    .bind(v.thumbnail_url)
    .bind(v.published_at)
    .bind(v.view_count)
    .bind(v.is_short)
    .bind(now())
    .execute(&mut *conn)
    .await?;
    Ok(!existed)
}

/// Store a video found on the channel page (when RSS is unavailable). Its publish
/// time is an estimate, so an exact one already stored is kept; duration and type
/// come along, so it needs no enrichment. Returns true if the video is new.
pub async fn upsert_from_page(
    conn: &mut crate::Conn,
    v: &FeedVideoMeta<'_>,
    duration_s: Option<i64>,
    is_live: bool,
) -> DbResult<bool> {
    let existed = crate::videos::exists(&mut *conn, v.id).await?;
    sqlx::query(
        "INSERT INTO videos (id, channel_id, title, thumbnail_url, published_at, view_count, is_short,
                             is_live, duration_s, enriched, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?)
         ON CONFLICT(id) DO UPDATE SET
           title = excluded.title,
           thumbnail_url = COALESCE(videos.thumbnail_url, excluded.thumbnail_url),
           published_at = COALESCE(videos.published_at, excluded.published_at),
           view_count = COALESCE(excluded.view_count, videos.view_count),
           is_short = videos.is_short OR excluded.is_short,
           is_live = excluded.is_live,
           duration_s = COALESCE(excluded.duration_s, videos.duration_s),
           enriched = 1,
           updated_at = excluded.updated_at",
    )
    .bind(v.id)
    .bind(v.channel_id)
    .bind(v.title)
    .bind(v.thumbnail_url)
    .bind(v.published_at)
    .bind(v.view_count)
    .bind(v.is_short)
    .bind(is_live)
    .bind(duration_s)
    .bind(now())
    .execute(&mut *conn)
    .await?;
    Ok(!existed)
}

pub async fn unenriched(e: impl Executor<'_>, channel_id: &str) -> DbResult<Vec<String>> {
    Ok(
        sqlx::query_scalar("SELECT id FROM videos WHERE channel_id = ? AND NOT enriched")
            .bind(channel_id)
            .fetch_all(e)
            .await?,
    )
}

pub struct Enrichment<'a> {
    pub id: &'a str,
    pub duration_s: Option<i64>,
    pub thumbnail_url: Option<&'a str>,
    pub is_short: bool,
    pub is_live: bool,
}

/// Fill in what only the channel page knows. Unknown videos are ignored.
pub async fn enrich(e: impl Executor<'_>, v: &Enrichment<'_>) -> DbResult<()> {
    sqlx::query(
        "UPDATE videos SET duration_s = COALESCE(?, duration_s), is_short = ?, is_live = ?,
           thumbnail_url = COALESCE(?, thumbnail_url), enriched = 1, updated_at = ?
         WHERE id = ?",
    )
    .bind(v.duration_s)
    .bind(v.is_short)
    .bind(v.is_live)
    .bind(v.thumbnail_url)
    .bind(now())
    .bind(v.id)
    .execute(e)
    .await?;
    Ok(())
}

/// Give up on enriching videos the channel page didn't list (e.g. members-only),
/// so they aren't looked up again on every refresh.
pub async fn mark_enriched(e: impl Executor<'_>, ids: &[String]) -> DbResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut q: QueryBuilder<Sqlite> =
        QueryBuilder::new("UPDATE videos SET enriched = 1 WHERE id IN (");
    let mut sep = q.separated(", ");
    for id in ids {
        sep.push_bind(id);
    }
    sep.push_unseparated(")");
    q.build().execute(e).await?;
    Ok(())
}
