//! User-defined tags on channels ("Programming", "Entertainment", …).

use serde::Serialize;
use sqlx::{FromRow, QueryBuilder, Sqlite};

use crate::{DbResult, Executor, now, unique};

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub color: String,
}

pub async fn list(e: impl Executor<'_>, user_id: i64) -> DbResult<Vec<Tag>> {
    Ok(sqlx::query_as(
        "SELECT id, name, color FROM tags WHERE user_id = ? ORDER BY name COLLATE NOCASE",
    )
    .bind(user_id)
    .fetch_all(e)
    .await?)
}

pub async fn create(e: impl Executor<'_>, user_id: i64, name: &str, color: &str) -> DbResult<Tag> {
    sqlx::query_as(
        "INSERT INTO tags (user_id, name, color, created_at) VALUES (?, ?, ?, ?)
         RETURNING id, name, color",
    )
    .bind(user_id)
    .bind(name)
    .bind(color)
    .bind(now())
    .fetch_one(e)
    .await
    .map_err(unique("tag"))
}

/// Rename/recolor. Returns false if the tag isn't the user's.
pub async fn update(
    e: impl Executor<'_>,
    user_id: i64,
    id: i64,
    name: &str,
    color: &str,
) -> DbResult<bool> {
    let res = sqlx::query("UPDATE tags SET name = ?, color = ? WHERE id = ? AND user_id = ?")
        .bind(name)
        .bind(color)
        .bind(id)
        .bind(user_id)
        .execute(e)
        .await
        .map_err(unique("tag"))?;
    Ok(res.rows_affected() == 1)
}

pub async fn delete(e: impl Executor<'_>, user_id: i64, id: i64) -> DbResult<bool> {
    let res = sqlx::query("DELETE FROM tags WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .execute(e)
        .await?;
    Ok(res.rows_affected() == 1)
}

/// How many of `tag_ids` belong to the user (to validate input).
pub async fn count_owned(e: impl Executor<'_>, user_id: i64, tag_ids: &[i64]) -> DbResult<i64> {
    if tag_ids.is_empty() {
        return Ok(0);
    }
    let mut q: QueryBuilder<Sqlite> =
        QueryBuilder::new("SELECT COUNT(*) FROM tags WHERE user_id = ");
    q.push_bind(user_id).push(" AND id IN (");
    let mut sep = q.separated(", ");
    for id in tag_ids {
        sep.push_bind(*id);
    }
    sep.push_unseparated(")");
    Ok(q.build_query_scalar().fetch_one(e).await?)
}

/// Replace the user's tags on a channel.
pub async fn set_channel_tags(
    conn: &mut crate::Conn,
    user_id: i64,
    channel_id: &str,
    tag_ids: &[i64],
) -> DbResult<()> {
    sqlx::query(
        "DELETE FROM channel_tags WHERE channel_id = ?
         AND tag_id IN (SELECT id FROM tags WHERE user_id = ?)",
    )
    .bind(channel_id)
    .bind(user_id)
    .execute(&mut *conn)
    .await?;
    for id in tag_ids {
        sqlx::query(
            "INSERT INTO channel_tags (tag_id, channel_id)
             SELECT id, ? FROM tags WHERE id = ? AND user_id = ? ON CONFLICT DO NOTHING",
        )
        .bind(channel_id)
        .bind(id)
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Add or remove one tag on many channels (bulk tagging).
pub async fn tag_channels(
    conn: &mut crate::Conn,
    user_id: i64,
    tag_id: i64,
    channel_ids: &[String],
    add: bool,
) -> DbResult<()> {
    for channel_id in channel_ids {
        let sql = if add {
            "INSERT INTO channel_tags (tag_id, channel_id)
             SELECT id, ? FROM tags WHERE id = ? AND user_id = ? ON CONFLICT DO NOTHING"
        } else {
            "DELETE FROM channel_tags WHERE channel_id = ?
             AND tag_id = (SELECT id FROM tags WHERE id = ? AND user_id = ?)"
        };
        sqlx::query(sql)
            .bind(channel_id)
            .bind(tag_id)
            .bind(user_id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// `(channel_id, tag_id)` pairs for all of the user's tags.
pub async fn channel_tag_pairs(e: impl Executor<'_>, user_id: i64) -> DbResult<Vec<(String, i64)>> {
    Ok(sqlx::query_as(
        "SELECT ct.channel_id, ct.tag_id FROM channel_tags ct JOIN tags t ON t.id = ct.tag_id
         WHERE t.user_id = ?",
    )
    .bind(user_id)
    .fetch_all(e)
    .await?)
}
