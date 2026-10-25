use serde::Serialize;
use sqlx::FromRow;

use crate::{DbResult, Executor, now};

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct Invite {
    pub id: i64,
    #[serde(skip)]
    pub token_hash: Vec<u8>,
    pub note: String,
    pub created_by: Option<i64>,
    pub created_at: i64,
    pub expires_at: i64,
    pub used_by: Option<i64>,
    pub used_at: Option<i64>,
}

pub async fn create(
    e: impl Executor<'_>,
    token_hash: &[u8],
    note: &str,
    created_by: i64,
    expires_at: i64,
) -> DbResult<Invite> {
    Ok(sqlx::query_as(
        "INSERT INTO invites (token_hash, note, created_by, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(token_hash)
    .bind(note)
    .bind(created_by)
    .bind(now())
    .bind(expires_at)
    .fetch_one(e)
    .await?)
}

/// An unused, unexpired invite.
pub async fn find_valid(e: impl Executor<'_>, token_hash: &[u8]) -> DbResult<Option<Invite>> {
    Ok(sqlx::query_as(
        "SELECT * FROM invites WHERE token_hash = ? AND used_at IS NULL AND expires_at > ?",
    )
    .bind(token_hash)
    .bind(now())
    .fetch_optional(e)
    .await?)
}

/// Atomically mark an invite used. Returns false if someone else used it first.
pub async fn consume(e: impl Executor<'_>, id: i64, used_by: i64) -> DbResult<bool> {
    let res = sqlx::query(
        "UPDATE invites SET used_by = ?, used_at = ?
         WHERE id = ? AND used_at IS NULL AND expires_at > ?",
    )
    .bind(used_by)
    .bind(now())
    .bind(id)
    .bind(now())
    .execute(e)
    .await?;
    Ok(res.rows_affected() == 1)
}

pub async fn list(e: impl Executor<'_>) -> DbResult<Vec<Invite>> {
    Ok(
        sqlx::query_as("SELECT * FROM invites ORDER BY created_at DESC")
            .fetch_all(e)
            .await?,
    )
}

pub async fn delete(e: impl Executor<'_>, id: i64) -> DbResult<bool> {
    let res = sqlx::query("DELETE FROM invites WHERE id = ?")
        .bind(id)
        .execute(e)
        .await?;
    Ok(res.rows_affected() == 1)
}
