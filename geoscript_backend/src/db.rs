use tokio::sync::OnceCell;

use sqlx::{
  Sqlite, SqlitePool, Transaction,
  sqlite::{SqliteConnectOptions, SqliteJournalMode},
};

static DB_POOL: OnceCell<SqlitePool> = OnceCell::const_new();

pub async fn init_db_pool(db_url: &str) -> Result<&'static SqlitePool, sqlx::Error> {
  DB_POOL
    .get_or_try_init(move || async move {
      // sqlx stopped defaulting SQLite to WAL in 0.9; without pinning it, a database restored
      // from `.backup` (which always writes a fresh file in rollback-journal mode) silently
      // stays there, where readers block writers.
      let opts = db_url
        .parse::<SqliteConnectOptions>()?
        .journal_mode(SqliteJournalMode::Wal);
      let pool = SqlitePool::connect_with(opts).await?;
      sqlx::migrate!("./migrations").run(&pool).await?;
      Ok(pool)
    })
    .await
}

pub fn get_db_pool() -> &'static SqlitePool {
  DB_POOL
    .get()
    .expect("Database pool has not been initialized")
}

/// SQLite does not invoke the busy handler when a deferred transaction upgrades from a read to a
/// write; it fails with `SQLITE_BUSY` immediately to avoid deadlocking. Any transaction that will
/// write must therefore take the write lock up front so that contention waits out `busy_timeout`
/// instead of erroring.
pub async fn begin_write(pool: &SqlitePool) -> Result<Transaction<'static, Sqlite>, sqlx::Error> {
  pool.begin_with("BEGIN IMMEDIATE").await
}
