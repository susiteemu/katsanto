use serde::Serialize;
use sqlx::{FromRow, QueryBuilder, Sqlite};

use crate::{DbResult, Executor, now};

/// Subscribe; the channel must be stored. Returns false if already subscribed.
pub async fn subscribe(e: impl Executor<'_>, user_id: i64, channel_id: &str) -> DbResult<bool> {
    let res = sqlx::query(
        "INSERT INTO subscriptions (user_id, channel_id, created_at) VALUES (?, ?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(channel_id)
    .bind(now())
    .execute(e)
    .await?;
    Ok(res.rows_affected() == 1)
}

/// Unsubscribe from the given channels; returns how many subscriptions were removed.
pub async fn unsubscribe(
    e: impl Executor<'_>,
    user_id: i64,
    channel_ids: &[String],
) -> DbResult<u64> {
    if channel_ids.is_empty() {
        return Ok(0);
    }
    let mut q: QueryBuilder<Sqlite> =
        QueryBuilder::new("DELETE FROM subscriptions WHERE user_id = ");
    q.push_bind(user_id).push(" AND channel_id IN (");
    let mut ids = q.separated(", ");
    for id in channel_ids {
        ids.push_bind(id);
    }
    ids.push_unseparated(")");
    Ok(q.build().execute(e).await?.rows_affected())
}

pub async fn channel_ids(e: impl Executor<'_>, user_id: i64) -> DbResult<Vec<String>> {
    Ok(
        sqlx::query_scalar("SELECT channel_id FROM subscriptions WHERE user_id = ?")
            .bind(user_id)
            .fetch_all(e)
            .await?,
    )
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct Subscription {
    pub channel_id: String,
    pub name: String,
    pub handle: Option<String>,
    pub avatar_url: Option<String>,
    pub subscriber_count: Option<i64>,
    pub subscribed_at: i64,
    pub last_video_at: Option<i64>,
    pub fetched_at: Option<i64>,
    pub fetch_error: Option<String>,
}

pub async fn list(e: impl Executor<'_>, user_id: i64) -> DbResult<Vec<Subscription>> {
    Ok(sqlx::query_as(
        "SELECT c.id AS channel_id, c.name, c.handle, c.avatar_url, c.subscriber_count,
                s.created_at AS subscribed_at, c.last_video_at, c.fetched_at, c.fetch_error
         FROM subscriptions s JOIN channels c ON c.id = s.channel_id
         WHERE s.user_id = ? ORDER BY c.name COLLATE NOCASE",
    )
    .bind(user_id)
    .fetch_all(e)
    .await?)
}
