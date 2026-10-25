use serde::Serialize;
use sqlx::FromRow;

use crate::{DbResult, Executor, now, unique};

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct PasskeyRow {
    pub id: i64,
    #[serde(skip)]
    pub user_id: i64,
    #[serde(skip)]
    pub credential_id: Vec<u8>,
    /// Serialized webauthn-rs `Passkey`.
    #[serde(skip)]
    pub passkey: String,
    pub name: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

pub async fn create(
    e: impl Executor<'_>,
    user_id: i64,
    credential_id: &[u8],
    passkey_json: &str,
    name: &str,
) -> DbResult<PasskeyRow> {
    sqlx::query_as(
        "INSERT INTO passkeys (user_id, credential_id, passkey, name, created_at)
         VALUES (?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(user_id)
    .bind(credential_id)
    .bind(passkey_json)
    .bind(name)
    .bind(now())
    .fetch_one(e)
    .await
    .map_err(unique("passkey"))
}

pub async fn for_user(e: impl Executor<'_>, user_id: i64) -> DbResult<Vec<PasskeyRow>> {
    Ok(
        sqlx::query_as("SELECT * FROM passkeys WHERE user_id = ? ORDER BY created_at")
            .bind(user_id)
            .fetch_all(e)
            .await?,
    )
}

pub async fn count_for_user(e: impl Executor<'_>, user_id: i64) -> DbResult<i64> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM passkeys WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(e)
            .await?,
    )
}

/// Store the updated passkey (signature counter, backup state) after a login.
pub async fn update_after_use(e: impl Executor<'_>, id: i64, passkey_json: &str) -> DbResult<()> {
    sqlx::query("UPDATE passkeys SET passkey = ?, last_used_at = ? WHERE id = ?")
        .bind(passkey_json)
        .bind(now())
        .bind(id)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn rename(e: impl Executor<'_>, user_id: i64, id: i64, name: &str) -> DbResult<bool> {
    let res = sqlx::query("UPDATE passkeys SET name = ? WHERE id = ? AND user_id = ?")
        .bind(name)
        .bind(id)
        .bind(user_id)
        .execute(e)
        .await?;
    Ok(res.rows_affected() == 1)
}

pub async fn delete(e: impl Executor<'_>, user_id: i64, id: i64) -> DbResult<bool> {
    let res = sqlx::query("DELETE FROM passkeys WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .execute(e)
        .await?;
    Ok(res.rows_affected() == 1)
}
