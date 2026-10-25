//! Saved feeds. The filter is opaque JSON here; the server defines and compiles it.

use serde::Serialize;
use sqlx::FromRow;

use crate::{DbResult, Executor, now};

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct Feed {
    pub id: i64,
    pub name: String,
    pub icon: String,
    pub position: i64,
    pub filter: String,
}

const COLUMNS: &str = "id, name, icon, position, filter";

pub async fn list(e: impl Executor<'_>, user_id: i64) -> DbResult<Vec<Feed>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM feeds WHERE user_id = ? ORDER BY position, id"
    ))
    .bind(user_id)
    .fetch_all(e)
    .await?)
}

pub async fn get(e: impl Executor<'_>, user_id: i64, id: i64) -> DbResult<Option<Feed>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM feeds WHERE id = ? AND user_id = ?"
    ))
    .bind(id)
    .bind(user_id)
    .fetch_optional(e)
    .await?)
}

/// New feeds go last.
pub async fn create(
    e: impl Executor<'_>,
    user_id: i64,
    name: &str,
    icon: &str,
    filter: &str,
) -> DbResult<Feed> {
    Ok(sqlx::query_as(&format!(
        "INSERT INTO feeds (user_id, name, icon, position, filter, created_at, updated_at)
         VALUES (?, ?, ?, (SELECT COALESCE(MAX(position), -1) + 1 FROM feeds WHERE user_id = ?), ?, ?, ?)
         RETURNING {COLUMNS}"
    ))
    .bind(user_id)
    .bind(name)
    .bind(icon)
    .bind(user_id)
    .bind(filter)
    .bind(now())
    .bind(now())
    .fetch_one(e)
    .await?)
}

pub async fn update(
    e: impl Executor<'_>,
    user_id: i64,
    id: i64,
    name: &str,
    icon: &str,
    filter: &str,
) -> DbResult<Option<Feed>> {
    Ok(sqlx::query_as(&format!(
        "UPDATE feeds SET name = ?, icon = ?, filter = ?, updated_at = ?
         WHERE id = ? AND user_id = ? RETURNING {COLUMNS}"
    ))
    .bind(name)
    .bind(icon)
    .bind(filter)
    .bind(now())
    .bind(id)
    .bind(user_id)
    .fetch_optional(e)
    .await?)
}

pub async fn delete(e: impl Executor<'_>, user_id: i64, id: i64) -> DbResult<bool> {
    let res = sqlx::query("DELETE FROM feeds WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .execute(e)
        .await?;
    Ok(res.rows_affected() == 1)
}

/// Set the order: `ids[i]` gets position `i`. Unknown ids are ignored.
pub async fn reorder(conn: &mut crate::Conn, user_id: i64, ids: &[i64]) -> DbResult<()> {
    for (pos, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE feeds SET position = ? WHERE id = ? AND user_id = ?")
            .bind(pos as i64)
            .bind(id)
            .bind(user_id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}
