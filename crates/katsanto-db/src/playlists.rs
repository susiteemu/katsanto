//! Watch lists.

use serde::Serialize;
use sqlx::FromRow;

use crate::{DbResult, Executor, now};

pub const WATCH_LATER: &str = "watch_later";
pub const CUSTOM: &str = "custom";

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct Playlist {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub remove_watched: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub item_count: i64,
    pub total_duration_s: i64,
    /// Thumbnail of the first video, for the list's cover.
    pub thumbnail_url: Option<String>,
}

const SELECT: &str = "SELECT p.id, p.kind, p.name, p.description, p.remove_watched, p.created_at, p.updated_at,
       (SELECT COUNT(*) FROM playlist_items i WHERE i.playlist_id = p.id) AS item_count,
       (SELECT COALESCE(SUM(v.duration_s), 0) FROM playlist_items i JOIN videos v ON v.id = i.video_id
         WHERE i.playlist_id = p.id) AS total_duration_s,
       (SELECT v.thumbnail_url FROM playlist_items i JOIN videos v ON v.id = i.video_id
         WHERE i.playlist_id = p.id ORDER BY i.position LIMIT 1) AS thumbnail_url
     FROM playlists p";

/// Watch later first, then the rest by name.
pub async fn list(e: impl Executor<'_>, user_id: i64) -> DbResult<Vec<Playlist>> {
    Ok(sqlx::query_as(&format!(
        "{SELECT} WHERE p.user_id = ? ORDER BY p.kind <> 'watch_later', p.name COLLATE NOCASE"
    ))
    .bind(user_id)
    .fetch_all(e)
    .await?)
}

pub async fn get(e: impl Executor<'_>, user_id: i64, id: i64) -> DbResult<Option<Playlist>> {
    Ok(
        sqlx::query_as(&format!("{SELECT} WHERE p.id = ? AND p.user_id = ?"))
            .bind(id)
            .bind(user_id)
            .fetch_optional(e)
            .await?,
    )
}

/// The user's Watch later list, created on first use.
pub async fn ensure_watch_later(conn: &mut crate::Conn, user_id: i64) -> DbResult<i64> {
    sqlx::query(
        "INSERT INTO playlists (user_id, kind, name, remove_watched, created_at, updated_at)
         VALUES (?, 'watch_later', 'Watch later', 1, ?, ?) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(now())
    .bind(now())
    .execute(&mut *conn)
    .await?;
    Ok(
        sqlx::query_scalar("SELECT id FROM playlists WHERE user_id = ? AND kind = 'watch_later'")
            .bind(user_id)
            .fetch_one(&mut *conn)
            .await?,
    )
}

pub async fn create(
    e: impl Executor<'_>,
    user_id: i64,
    name: &str,
    description: &str,
) -> DbResult<i64> {
    Ok(sqlx::query_scalar(
        "INSERT INTO playlists (user_id, kind, name, description, created_at, updated_at)
         VALUES (?, 'custom', ?, ?, ?, ?) RETURNING id",
    )
    .bind(user_id)
    .bind(name)
    .bind(description)
    .bind(now())
    .bind(now())
    .fetch_one(e)
    .await?)
}

/// Returns false if the list isn't the user's.
pub async fn update(
    e: impl Executor<'_>,
    user_id: i64,
    id: i64,
    name: &str,
    description: &str,
    remove_watched: bool,
) -> DbResult<bool> {
    let res = sqlx::query(
        "UPDATE playlists SET name = ?, description = ?, remove_watched = ?, updated_at = ?
         WHERE id = ? AND user_id = ?",
    )
    .bind(name)
    .bind(description)
    .bind(remove_watched)
    .bind(now())
    .bind(id)
    .bind(user_id)
    .execute(e)
    .await?;
    Ok(res.rows_affected() == 1)
}

/// Delete a custom list (Watch later can only be emptied).
pub async fn delete(e: impl Executor<'_>, user_id: i64, id: i64) -> DbResult<bool> {
    let res = sqlx::query("DELETE FROM playlists WHERE id = ? AND user_id = ? AND kind = 'custom'")
        .bind(id)
        .bind(user_id)
        .execute(e)
        .await?;
    Ok(res.rows_affected() == 1)
}

async fn touch(e: impl Executor<'_>, id: i64) -> DbResult<()> {
    sqlx::query("UPDATE playlists SET updated_at = ? WHERE id = ?")
        .bind(now())
        .bind(id)
        .execute(e)
        .await?;
    Ok(())
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct Item {
    pub video_id: String,
    pub position: f64,
    pub added_at: i64,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub duration_s: Option<i64>,
    pub published_at: Option<i64>,
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

/// A list's videos in order, with the user's progress.
pub async fn items(e: impl Executor<'_>, user_id: i64, playlist_id: i64) -> DbResult<Vec<Item>> {
    Ok(sqlx::query_as(
        "SELECT i.video_id, i.position, i.added_at, v.title, v.thumbnail_url, v.duration_s, v.published_at,
                v.is_short, v.is_live, v.channel_id, c.name AS channel_name, c.avatar_url AS channel_avatar_url,
                p.position_s, p.duration_s AS watched_duration_s, p.finished, p.updated_at AS progress_updated_at
         FROM playlist_items i
         JOIN videos v ON v.id = i.video_id
         JOIN channels c ON c.id = v.channel_id
         LEFT JOIN watch_progress p ON p.video_id = i.video_id AND p.user_id = ?
         WHERE i.playlist_id = ?
         ORDER BY i.position",
    )
    .bind(user_id)
    .bind(playlist_id)
    .fetch_all(e)
    .await?)
}

/// Append a video (it must be stored). Returns false if it was already in the list.
pub async fn add(conn: &mut crate::Conn, playlist_id: i64, video_id: &str) -> DbResult<bool> {
    let res = sqlx::query(
        "INSERT INTO playlist_items (playlist_id, video_id, position, added_at)
         VALUES (?, ?, (SELECT COALESCE(MAX(position), 0) + 1 FROM playlist_items WHERE playlist_id = ?), ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(playlist_id)
    .bind(video_id)
    .bind(playlist_id)
    .bind(now())
    .execute(&mut *conn)
    .await?;
    touch(&mut *conn, playlist_id).await?;
    Ok(res.rows_affected() == 1)
}

pub async fn remove(e: impl Executor<'_>, playlist_id: i64, video_id: &str) -> DbResult<bool> {
    let res = sqlx::query("DELETE FROM playlist_items WHERE playlist_id = ? AND video_id = ?")
        .bind(playlist_id)
        .bind(video_id)
        .execute(e)
        .await?;
    Ok(res.rows_affected() == 1)
}

/// Move a video right after `after` (or to the top when `None`). Returns false
/// if either video isn't in the list.
pub async fn move_item(
    conn: &mut crate::Conn,
    playlist_id: i64,
    video_id: &str,
    after: Option<&str>,
) -> DbResult<bool> {
    let positions: Vec<(String, f64)> = sqlx::query_as(
        "SELECT video_id, position FROM playlist_items WHERE playlist_id = ? ORDER BY position",
    )
    .bind(playlist_id)
    .fetch_all(&mut *conn)
    .await?;
    if !positions.iter().any(|(id, _)| id == video_id) {
        return Ok(false);
    }
    let rest: Vec<&(String, f64)> = positions.iter().filter(|(id, _)| id != video_id).collect();
    let idx = match after {
        None => 0,
        Some(a) => match rest.iter().position(|(id, _)| id == a) {
            Some(i) => i + 1,
            None => return Ok(false),
        },
    };
    let before = idx.checked_sub(1).map(|i| rest[i].1);
    let next = rest.get(idx).map(|r| r.1);
    let pos = match (before, next) {
        (Some(b), Some(n)) => (b + n) / 2.0,
        (Some(b), None) => b + 1.0,
        (None, Some(n)) => n - 1.0,
        (None, None) => 1.0,
    };
    // Floats run out of room after ~50 halvings between the same neighbours: renumber.
    if before.is_some_and(|b| pos <= b) || next.is_some_and(|n| pos >= n) {
        let mut order: Vec<&str> = rest.iter().map(|(id, _)| id.as_str()).collect();
        order.insert(idx, video_id);
        for (i, id) in order.iter().enumerate() {
            sqlx::query(
                "UPDATE playlist_items SET position = ? WHERE playlist_id = ? AND video_id = ?",
            )
            .bind((i + 1) as f64)
            .bind(playlist_id)
            .bind(id)
            .execute(&mut *conn)
            .await?;
        }
    } else {
        sqlx::query(
            "UPDATE playlist_items SET position = ? WHERE playlist_id = ? AND video_id = ?",
        )
        .bind(pos)
        .bind(playlist_id)
        .bind(video_id)
        .execute(&mut *conn)
        .await?;
    }
    touch(&mut *conn, playlist_id).await?;
    Ok(true)
}

/// Ids of the user's lists that contain the video.
pub async fn containing(e: impl Executor<'_>, user_id: i64, video_id: &str) -> DbResult<Vec<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT i.playlist_id FROM playlist_items i JOIN playlists p ON p.id = i.playlist_id
         WHERE p.user_id = ? AND i.video_id = ?",
    )
    .bind(user_id)
    .bind(video_id)
    .fetch_all(e)
    .await?)
}

/// After a video is finished: drop it from the user's lists that remove watched videos.
pub async fn remove_watched(e: impl Executor<'_>, user_id: i64, video_id: &str) -> DbResult<u64> {
    let res = sqlx::query(
        "DELETE FROM playlist_items WHERE video_id = ?
         AND playlist_id IN (SELECT id FROM playlists WHERE user_id = ? AND remove_watched)",
    )
    .bind(video_id)
    .bind(user_id)
    .execute(e)
    .await?;
    Ok(res.rows_affected())
}
