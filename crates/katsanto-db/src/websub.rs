//! WebSub subscriptions at YouTube's hub (see the server's `websub` module).

use crate::{DbResult, Executor, now};

/// Subscribed channels without a push subscription, with one that expires within
/// `renew_before` seconds, or with a request the hub hasn't verified for `retry_after`.
pub async fn needing_subscription(
    e: impl Executor<'_>,
    renew_before: i64,
    retry_after: i64,
    limit: i64,
) -> DbResult<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT c.id FROM channels c LEFT JOIN websub w ON w.channel_id = c.id
         WHERE EXISTS (SELECT 1 FROM subscriptions s WHERE s.channel_id = c.id)
           AND (w.channel_id IS NULL
                OR (w.lease_expires_at IS NULL AND w.requested_at < ?1 - ?3)
                OR (w.lease_expires_at IS NOT NULL AND w.lease_expires_at < ?1 + ?2
                    AND w.requested_at < ?1 - ?3))
         ORDER BY w.requested_at IS NOT NULL, w.requested_at
         LIMIT ?4",
    )
    .bind(now())
    .bind(renew_before)
    .bind(retry_after)
    .bind(limit)
    .fetch_all(e)
    .await?)
}

/// Push subscriptions for channels nobody is subscribed to anymore.
pub async fn orphaned(e: impl Executor<'_>, limit: i64) -> DbResult<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT w.channel_id FROM websub w
         WHERE NOT EXISTS (SELECT 1 FROM subscriptions s WHERE s.channel_id = w.channel_id)
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(e)
    .await?)
}

/// Record a (re)subscription request; returns the channel's secret, which is created
/// once and kept, so notifications signed during a renewal still verify.
pub async fn requested(
    e: impl Executor<'_>,
    channel_id: &str,
    new_secret: &str,
) -> DbResult<String> {
    Ok(sqlx::query_scalar(
        "INSERT INTO websub (channel_id, secret, requested_at) VALUES (?, ?, ?)
         ON CONFLICT(channel_id) DO UPDATE SET requested_at = excluded.requested_at
         RETURNING secret",
    )
    .bind(channel_id)
    .bind(new_secret)
    .bind(now())
    .fetch_one(e)
    .await?)
}

pub struct Sub {
    pub secret: String,
    pub lease_expires_at: Option<i64>,
    pub wanted: bool,
}

/// The push subscription for a channel, and whether anyone still wants it.
pub async fn get(e: impl Executor<'_>, channel_id: &str) -> DbResult<Option<Sub>> {
    let row: Option<(String, Option<i64>, bool)> = sqlx::query_as(
        "SELECT w.secret, w.lease_expires_at,
                EXISTS (SELECT 1 FROM subscriptions s WHERE s.channel_id = w.channel_id)
         FROM websub w WHERE w.channel_id = ?",
    )
    .bind(channel_id)
    .fetch_optional(e)
    .await?;
    Ok(row.map(|(secret, lease_expires_at, wanted)| Sub {
        secret,
        lease_expires_at,
        wanted,
    }))
}

pub async fn verified(
    e: impl Executor<'_>,
    channel_id: &str,
    lease_expires_at: i64,
) -> DbResult<()> {
    sqlx::query("UPDATE websub SET lease_expires_at = ? WHERE channel_id = ?")
        .bind(lease_expires_at)
        .bind(channel_id)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn delete(e: impl Executor<'_>, channel_id: &str) -> DbResult<()> {
    sqlx::query("DELETE FROM websub WHERE channel_id = ?")
        .bind(channel_id)
        .execute(e)
        .await?;
    Ok(())
}

/// Whether the hub currently pushes this channel's uploads.
pub async fn is_pushed(e: impl Executor<'_>, channel_id: &str) -> DbResult<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM websub WHERE channel_id = ? AND lease_expires_at > ?)",
    )
    .bind(channel_id)
    .bind(now())
    .fetch_one(e)
    .await?)
}
