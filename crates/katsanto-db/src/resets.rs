use sqlx::FromRow;

use crate::{DbResult, Executor, now};

#[derive(Clone, Debug, FromRow)]
pub struct ResetLink {
    pub id: i64,
    pub token_hash: Vec<u8>,
    pub user_id: i64,
    pub created_by: Option<i64>,
    pub created_at: i64,
    pub expires_at: i64,
    pub used_at: Option<i64>,
}

/// Create a reset link, invalidating any earlier unused links for the user.
pub async fn create(
    tx: &mut sqlx::SqliteConnection,
    token_hash: &[u8],
    user_id: i64,
    created_by: i64,
    expires_at: i64,
) -> DbResult<ResetLink> {
    sqlx::query("DELETE FROM reset_links WHERE user_id = ? AND used_at IS NULL")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    Ok(sqlx::query_as(
        "INSERT INTO reset_links (token_hash, user_id, created_by, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(token_hash)
    .bind(user_id)
    .bind(created_by)
    .bind(now())
    .bind(expires_at)
    .fetch_one(&mut *tx)
    .await?)
}

pub async fn find_valid(e: impl Executor<'_>, token_hash: &[u8]) -> DbResult<Option<ResetLink>> {
    Ok(sqlx::query_as(
        "SELECT * FROM reset_links WHERE token_hash = ? AND used_at IS NULL AND expires_at > ?",
    )
    .bind(token_hash)
    .bind(now())
    .fetch_optional(e)
    .await?)
}

pub async fn consume(e: impl Executor<'_>, id: i64) -> DbResult<bool> {
    let res = sqlx::query(
        "UPDATE reset_links SET used_at = ? WHERE id = ? AND used_at IS NULL AND expires_at > ?",
    )
    .bind(now())
    .bind(id)
    .bind(now())
    .execute(e)
    .await?;
    Ok(res.rows_affected() == 1)
}
