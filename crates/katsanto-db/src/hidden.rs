//! Videos a user hid ("not interested"); they never show up in feeds.

use crate::{DbResult, Executor, now};

pub async fn hide(e: impl Executor<'_>, user_id: i64, video_id: &str) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO hidden_videos (user_id, video_id, created_at) VALUES (?, ?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(video_id)
    .bind(now())
    .execute(e)
    .await?;
    Ok(())
}

pub async fn unhide(e: impl Executor<'_>, user_id: i64, video_id: &str) -> DbResult<bool> {
    let res = sqlx::query("DELETE FROM hidden_videos WHERE user_id = ? AND video_id = ?")
        .bind(user_id)
        .bind(video_id)
        .execute(e)
        .await?;
    Ok(res.rows_affected() == 1)
}
