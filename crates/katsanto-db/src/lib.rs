//! SQLite storage for Katsanto.
//!
//! Each module holds the queries for one table. Query functions take any SQLite
//! executor, so they work on the pool and inside transactions alike.

pub mod backup;
pub mod challenges;
pub mod feed;
pub mod feeds;
pub mod hidden;
pub mod invites;
pub mod passkeys;
pub mod playlists;
pub mod progress;
pub mod recovery;
pub mod resets;
pub mod sessions;
pub mod settings;
pub mod subscriptions;
pub mod tags;
pub mod users;
pub mod videos;
pub mod websub;

use std::{path::Path, str::FromStr, time::Duration};

use sqlx::{
    Sqlite, SqlitePool, Transaction,
    pool::PoolConnection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

pub use sqlx::{SqliteConnection as Conn, SqliteExecutor as Executor};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("already exists: {0}")]
    Conflict(&'static str),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error("migration failed: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

pub type DbResult<T> = Result<T, DbError>;

/// Map unique constraint violations to [`DbError::Conflict`].
pub(crate) fn unique(what: &'static str) -> impl Fn(sqlx::Error) -> DbError {
    move |e| match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => DbError::Conflict(what),
        _ => DbError::Sqlx(e),
    }
}

#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// Open (or create) the database file and run migrations.
    pub async fn open(path: &Path) -> DbResult<Self> {
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        Self::connect(opts, 8).await
    }

    /// Fresh in-memory database, for tests.
    pub async fn in_memory() -> DbResult<Self> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        // One connection: every connection to `:memory:` is a separate database.
        Self::connect(opts, 1).await
    }

    async fn connect(opts: SqliteConnectOptions, max: u32) -> DbResult<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(max)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn begin(&self) -> DbResult<Transaction<'static, Sqlite>> {
        Ok(self.pool.begin().await?)
    }

    pub async fn acquire(&self) -> DbResult<PoolConnection<Sqlite>> {
        Ok(self.pool.acquire().await?)
    }

    pub async fn commit(tx: Transaction<'static, Sqlite>) -> DbResult<()> {
        Ok(tx.commit().await?)
    }
}

pub fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}
