use serde::Serialize;
use sqlx::FromRow;

use crate::{DbResult, Executor, now, unique};

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    #[serde(skip)]
    pub webauthn_id: Vec<u8>,
    #[serde(skip)]
    pub password_hash: Option<String>,
    pub is_admin: bool,
    pub disabled: bool,
    #[serde(skip)]
    pub settings: String,
    pub created_at: i64,
    pub last_login_at: Option<i64>,
}

/// User with credential counts, for the admin list.
#[derive(Clone, Debug, FromRow, Serialize)]
pub struct UserSummary {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
    pub disabled: bool,
    pub created_at: i64,
    pub last_login_at: Option<i64>,
    pub has_password: bool,
    pub passkey_count: i64,
}

pub struct NewUser<'a> {
    pub username: &'a str,
    pub webauthn_id: &'a [u8],
    pub password_hash: Option<&'a str>,
    pub is_admin: bool,
}

pub async fn create(e: impl Executor<'_>, u: NewUser<'_>) -> DbResult<User> {
    sqlx::query_as(
        "INSERT INTO users (username, webauthn_id, password_hash, is_admin, created_at)
         VALUES (?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(u.username)
    .bind(u.webauthn_id)
    .bind(u.password_hash)
    .bind(u.is_admin)
    .bind(now())
    .fetch_one(e)
    .await
    .map_err(unique("username"))
}

pub async fn count(e: impl Executor<'_>) -> DbResult<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(e)
        .await?)
}

pub async fn by_id(e: impl Executor<'_>, id: i64) -> DbResult<Option<User>> {
    Ok(sqlx::query_as("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(e)
        .await?)
}

pub async fn by_username(e: impl Executor<'_>, username: &str) -> DbResult<Option<User>> {
    Ok(sqlx::query_as("SELECT * FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(e)
        .await?)
}

pub async fn by_webauthn_id(e: impl Executor<'_>, webauthn_id: &[u8]) -> DbResult<Option<User>> {
    Ok(sqlx::query_as("SELECT * FROM users WHERE webauthn_id = ?")
        .bind(webauthn_id)
        .fetch_optional(e)
        .await?)
}

pub async fn list(e: impl Executor<'_>) -> DbResult<Vec<UserSummary>> {
    Ok(sqlx::query_as(
        "SELECT u.id, u.username, u.is_admin, u.disabled, u.created_at, u.last_login_at,
                u.password_hash IS NOT NULL AS has_password,
                (SELECT COUNT(*) FROM passkeys p WHERE p.user_id = u.id) AS passkey_count
         FROM users u ORDER BY u.username",
    )
    .fetch_all(e)
    .await?)
}

pub async fn set_password(e: impl Executor<'_>, id: i64, hash: Option<&str>) -> DbResult<()> {
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(hash)
        .bind(id)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn set_disabled(e: impl Executor<'_>, id: i64, disabled: bool) -> DbResult<()> {
    sqlx::query("UPDATE users SET disabled = ? WHERE id = ?")
        .bind(disabled)
        .bind(id)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn set_admin(e: impl Executor<'_>, id: i64, is_admin: bool) -> DbResult<()> {
    sqlx::query("UPDATE users SET is_admin = ? WHERE id = ?")
        .bind(is_admin)
        .bind(id)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn set_settings(e: impl Executor<'_>, id: i64, settings: &str) -> DbResult<()> {
    sqlx::query("UPDATE users SET settings = ? WHERE id = ?")
        .bind(settings)
        .bind(id)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn touch_login(e: impl Executor<'_>, id: i64) -> DbResult<()> {
    sqlx::query("UPDATE users SET last_login_at = ? WHERE id = ?")
        .bind(now())
        .bind(id)
        .execute(e)
        .await?;
    Ok(())
}

pub async fn count_active_admins(e: impl Executor<'_>) -> DbResult<i64> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE is_admin AND NOT disabled")
            .fetch_one(e)
            .await?,
    )
}

pub async fn delete(e: impl Executor<'_>, id: i64) -> DbResult<()> {
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id)
        .execute(e)
        .await?;
    Ok(())
}
