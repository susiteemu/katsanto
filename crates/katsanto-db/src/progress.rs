//! Per-user watch progress ("resume where you left off").

use serde::Serialize;
use sqlx::{FromRow, QueryBuilder, Sqlite};

use crate::{
    DbResult, Executor, now,
    videos::{VIDEO_ROW_COLUMNS, VideoRow},
};

/// A video counts as watched from this fraction on…
const FINISHED_FRACTION: f64 = 0.9;
/// …or when less than this much is left (end screens, credits).
const FINISHED_REMAINING_S: f64 = 30.0;
/// Below this, playback was just a peek and doesn't show up in "continue watching".
pub const MIN_RESUME_S: f64 = 30.0;

pub fn is_finished(position_s: f64, duration_s: f64) -> bool {
    duration_s > 0.0
        && (position_s >= duration_s * FINISHED_FRACTION
            || duration_s - position_s < FINISHED_REMAINING_S.min(duration_s * 0.5))
}

#[derive(Clone, Debug, FromRow, Serialize, PartialEq)]
pub struct Progress {
    pub video_id: String,
    pub position_s: f64,
    pub duration_s: f64,
    pub finished: bool,
    pub updated_at: i64,
}

/// Save a position. The video must be stored (see [`crate::videos`]).
pub async fn save(
    e: impl Executor<'_>,
    user_id: i64,
    video_id: &str,
    position_s: f64,
    duration_s: f64,
) -> DbResult<Progress> {
    Ok(sqlx::query_as(
        "INSERT INTO watch_progress (user_id, video_id, position_s, duration_s, finished, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(user_id, video_id) DO UPDATE SET
           position_s = excluded.position_s,
           duration_s = excluded.duration_s,
           finished = excluded.finished,
           updated_at = excluded.updated_at
         RETURNING video_id, position_s, duration_s, finished, updated_at",
    )
    .bind(user_id)
    .bind(video_id)
    .bind(position_s)
    .bind(duration_s)
    .bind(is_finished(position_s, duration_s))
    .bind(now())
    .fetch_one(e)
    .await?)
}

pub async fn get(e: impl Executor<'_>, user_id: i64, video_id: &str) -> DbResult<Option<Progress>> {
    Ok(sqlx::query_as(
        "SELECT video_id, position_s, duration_s, finished, updated_at
         FROM watch_progress WHERE user_id = ? AND video_id = ?",
    )
    .bind(user_id)
    .bind(video_id)
    .fetch_optional(e)
    .await?)
}

/// Progress for many videos at once (for cards). Unknown ids are skipped.
pub async fn for_videos(
    e: impl Executor<'_>,
    user_id: i64,
    video_ids: &[String],
) -> DbResult<Vec<Progress>> {
    if video_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut q: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT video_id, position_s, duration_s, finished, updated_at FROM watch_progress WHERE user_id = ",
    );
    q.push_bind(user_id).push(" AND video_id IN (");
    let mut ids = q.separated(", ");
    for id in video_ids {
        ids.push_bind(id);
    }
    ids.push_unseparated(")");
    Ok(q.build_query_as().fetch_all(e).await?)
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct InProgress {
    #[sqlx(flatten)]
    pub video: VideoRow,
    pub position_s: f64,
    pub duration_s_watched: f64,
    pub updated_at: i64,
}

/// Started but unfinished videos, most recent first.
pub async fn continue_watching(
    e: impl Executor<'_>,
    user_id: i64,
    limit: i64,
) -> DbResult<Vec<InProgress>> {
    let sql = format!(
        "SELECT {VIDEO_ROW_COLUMNS}, p.position_s, p.duration_s AS duration_s_watched, p.updated_at
         FROM watch_progress p
         JOIN videos v ON v.id = p.video_id
         JOIN channels c ON c.id = v.channel_id
         WHERE p.user_id = ? AND NOT p.finished AND p.position_s >= ?
         ORDER BY p.updated_at DESC LIMIT ?"
    );
    Ok(sqlx::query_as(&sql)
        .bind(user_id)
        .bind(MIN_RESUME_S)
        .bind(limit)
        .fetch_all(e)
        .await?)
}

pub async fn delete(e: impl Executor<'_>, user_id: i64, video_id: &str) -> DbResult<bool> {
    let res = sqlx::query("DELETE FROM watch_progress WHERE user_id = ? AND video_id = ?")
        .bind(user_id)
        .bind(video_id)
        .execute(e)
        .await?;
    Ok(res.rows_affected() == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finished_rule() {
        assert!(!is_finished(0.0, 600.0));
        assert!(!is_finished(500.0, 600.0));
        assert!(is_finished(540.0, 600.0), "90 %");
        assert!(is_finished(3580.0, 3600.0), "20 s left of an hour");
        assert!(!is_finished(30.0, 60.0), "short video: half left");
        assert!(is_finished(55.0, 60.0));
        assert!(!is_finished(10.0, 0.0), "unknown duration");
    }
}
