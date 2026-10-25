//! Instance-wide settings editable from the admin UI, stored as JSON values.

use serde::{Serialize, de::DeserializeOwned};

use crate::{DbResult, Executor};

pub async fn get<T: DeserializeOwned>(e: impl Executor<'_>, key: &str) -> DbResult<Option<T>> {
    let raw: Option<String> =
        sqlx::query_scalar("SELECT value FROM instance_settings WHERE key = ?")
            .bind(key)
            .fetch_optional(e)
            .await?;
    Ok(raw.and_then(|v| match serde_json::from_str(&v) {
        Ok(v) => Some(v),
        Err(err) => {
            tracing::warn!(key, %err, "ignoring invalid instance setting");
            None
        }
    }))
}

pub async fn set<T: Serialize>(e: impl Executor<'_>, key: &str, value: &T) -> DbResult<()> {
    let json = serde_json::to_string(value).expect("serializable setting");
    sqlx::query(
        "INSERT INTO instance_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(json)
    .execute(e)
    .await?;
    Ok(())
}
