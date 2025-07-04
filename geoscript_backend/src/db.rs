use std::sync::OnceLock;

use sqlx::SqlitePool;

static DB_POOL: OnceLock<SqlitePool> = OnceLock::new();

pub async fn init_db_pool(db_url: &str) -> Result<(), sqlx::Error> {
  if DB_POOL.get().is_some() {
    return Ok(());
  }

  let pool = SqlitePool::connect(db_url).await?;
  let _ = DB_POOL.set(pool);
  Ok(())
}

pub fn get_db_pool() -> &'static SqlitePool {
  DB_POOL
    .get()
    .expect("Database pool has not been initialized")
}
