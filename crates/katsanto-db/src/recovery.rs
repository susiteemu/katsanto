use crate::{DbResult, Executor, now};

/// Replace all of a user's recovery codes. Callers pass hashes, never codes.
pub async fn replace(
    tx: &mut sqlx::SqliteConnection,
    user_id: i64,
    code_hashes: &[Vec<u8>],
) -> DbResult<()> {
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = ?")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    for hash in code_hashes {
        sqlx::query("INSERT INTO recovery_codes (user_id, code_hash) VALUES (?, ?)")
            .bind(user_id)
            .bind(hash)
            .execute(&mut *tx)
            .await?;
    }
    Ok(())
}

/// Mark a code as used. Returns false if it does not exist or was already used.
pub async fn consume(e: impl Executor<'_>, user_id: i64, code_hash: &[u8]) -> DbResult<bool> {
    let res = sqlx::query(
        "UPDATE recovery_codes SET used_at = ?
         WHERE user_id = ? AND code_hash = ? AND used_at IS NULL",
    )
    .bind(now())
    .bind(user_id)
    .bind(code_hash)
    .execute(e)
    .await?;
    Ok(res.rows_affected() == 1)
}

pub async fn remaining(e: impl Executor<'_>, user_id: i64) -> DbResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT COUNT(*) FROM recovery_codes WHERE user_id = ? AND used_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(e)
    .await?)
}
