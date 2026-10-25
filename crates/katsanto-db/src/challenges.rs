//! Short-lived WebAuthn ceremony state.

use crate::{DbResult, Executor, now};

pub async fn put(
    e: impl Executor<'_>,
    id: &str,
    kind: &str,
    state: &str,
    ttl_s: i64,
) -> DbResult<()> {
    sqlx::query("INSERT INTO auth_challenges (id, kind, state, expires_at) VALUES (?, ?, ?, ?)")
        .bind(id)
        .bind(kind)
        .bind(state)
        .bind(now() + ttl_s)
        .execute(e)
        .await?;
    Ok(())
}

/// Remove and return a challenge's state. Each challenge can be used once.
pub async fn take(e: impl Executor<'_>, id: &str, kind: &str) -> DbResult<Option<String>> {
    Ok(sqlx::query_scalar(
        "DELETE FROM auth_challenges WHERE id = ? AND kind = ? AND expires_at > ? RETURNING state",
    )
    .bind(id)
    .bind(kind)
    .bind(now())
    .fetch_optional(e)
    .await?)
}

pub async fn delete_expired(e: impl Executor<'_>) -> DbResult<u64> {
    let res = sqlx::query("DELETE FROM auth_challenges WHERE expires_at <= ?")
        .bind(now())
        .execute(e)
        .await?;
    Ok(res.rows_affected())
}
