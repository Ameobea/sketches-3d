use tokio::sync::OnceCell;

use sqlx::SqlitePool;

static DB_POOL: OnceCell<SqlitePool> = OnceCell::const_new();

pub async fn init_db_pool(db_url: &str) -> Result<&'static SqlitePool, sqlx::Error> {
  DB_POOL
    .get_or_try_init(move || async move {
      let pool = SqlitePool::connect(db_url).await?;
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
