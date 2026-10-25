use serde::Serialize;
use sqlx::FromRow;

use crate::{DbResult, Executor, now};

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct Session {
    pub id: i64,
    #[serde(skip)]
    pub token_hash: Vec<u8>,
    #[serde(skip)]
    pub user_id: i64,
    pub user_agent: String,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub expires_at: i64,
    #[serde(skip)]
    pub reauth_at: Option<i64>,
}

pub async fn create(
    e: impl Executor<'_>,
    token_hash: &[u8],
    user_id: i64,
    user_agent: &str,
    ttl_s: i64,
) -> DbResult<Session> {
    let now = now();
    Ok(sqlx::query_as(
        "INSERT INTO sessions (token_hash, user_id, user_agent, created_at, last_seen_at, expires_at, reauth_at)
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(token_hash)
    .bind(user_id)
    .bind(user_agent)
    .bind(now)
    .bind(now)
    .bind(now + ttl_s)
    // Logging in counts as re-authentication.
    .bind(now)
    .fetch_one(e)
    .await?)
}

/// A live session for an enabled user.
pub async fn find_active(e: impl Executor<'_>, token_hash: &[u8]) -> DbResult<Option<Session>> {
    Ok(sqlx::query_as(
        "SELECT s.* FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = ? AND s.expires_at > ? AND NOT u.disabled",
    )
    .bind(token_hash)
    .bind(now())
    .fetch_optional(e)
    .await?)
}

/// Sliding expiry: extend the session and record activity.
pub async fn touch(e: impl Executor<'_>, id: i64, ttl_s: i64) -> DbResult<()> {
    let now = now();
    sqlx::query("UPDATE sessions SET last_seen_at = ?, expires_at = ? WHERE id = ?")
        .bind(now)
        .bind(now + ttl_s)
        .bind(id)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn mark_reauth(e: impl Executor<'_>, id: i64) -> DbResult<()> {
    sqlx::query("UPDATE sessions SET reauth_at = ? WHERE id = ?")
        .bind(now())
        .bind(id)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn for_user(e: impl Executor<'_>, user_id: i64) -> DbResult<Vec<Session>> {
    Ok(sqlx::query_as(
        "SELECT * FROM sessions WHERE user_id = ? AND expires_at > ? ORDER BY last_seen_at DESC",
    )
    .bind(user_id)
    .bind(now())
    .fetch_all(e)
    .await?)
}

pub async fn delete(e: impl Executor<'_>, user_id: i64, id: i64) -> DbResult<bool> {
    let res = sqlx::query("DELETE FROM sessions WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .execute(e)
        .await?;
    Ok(res.rows_affected() == 1)
}

/// Delete all of a user's sessions, optionally keeping one (the current).
pub async fn delete_all(e: impl Executor<'_>, user_id: i64, keep: Option<i64>) -> DbResult<u64> {
    let res = sqlx::query("DELETE FROM sessions WHERE user_id = ? AND id IS NOT ?")
        .bind(user_id)
        .bind(keep)
        .execute(e)
        .await?;
    Ok(res.rows_affected())
}

pub async fn delete_expired(e: impl Executor<'_>) -> DbResult<u64> {
    let res = sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
        .bind(now())
        .execute(e)
        .await?;
    Ok(res.rows_affected())
}
