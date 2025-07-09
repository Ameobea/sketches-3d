use std::time::Instant;

use crate::{
  metrics::geoscript::{thumbnail_render_time, thumbnail_upload_time},
  object_storage::upload_file,
};
use sqlx::SqlitePool;
use uuid::Uuid;

/// Spawns a background task to render a thumbnail for the given composition version, upload it to
/// cloud storage, and save the thumbnail URL in the DB.
pub fn render_thumbnail(pool: SqlitePool, composition_id: i64, version_id: i64) {
  tokio::spawn(async move {
    info!("Rendering thumbnail for composition_id={composition_id}, version_id={version_id} ...");
    let url = format!(
      "http://localhost:5812/render/{composition_id}?admin_token={}&version_id={version_id}",
      crate::server::settings().admin_token
    );

    let start = Instant::now();
    let res = reqwest::get(&url).await;
    let res = match res {
      Ok(res) => res,
      Err(err) => {
        error!("Failed to request thumbnail render: {err}");
        return;
      },
    };
    if !res.status().is_success() {
      error!(
        "Thumbnail render request failed: {}; {}",
        res.status(),
        res
          .text()
          .await
          .unwrap_or_else(|_| "<failed to get body>".to_string())
      );
      return;
    }
    let thumbnail_image = match res.bytes().await {
      Ok(bytes) => bytes,
      Err(err) => {
        error!("Failed to get thumbnail image bytes: {err}");
        return;
      },
    };
    thumbnail_render_time().observe(start.elapsed().as_nanos() as u64);

    let start = Instant::now();
    let thumbnail_url = match upload_file(
      &format!("thumbnails/{}.avif", Uuid::new_v4()),
      thumbnail_image,
    )
    .await
    {
      Ok(url) => url,
      Err(err) => {
        error!("Failed to upload thumbnail to cloud storage: {err:?}");
        return;
      },
    };
    thumbnail_upload_time().observe(start.elapsed().as_nanos() as u64);

    let res = sqlx::query(
      "UPDATE composition_versions SET thumbnail_url = ? WHERE id = ? AND composition_id = ?",
    )
    .bind(&thumbnail_url)
    .bind(version_id)
    .bind(composition_id)
    .execute(&pool)
    .await;
    if let Err(err) = res {
      error!("Failed to save thumbnail URL to DB: {err}");
      return;
    }

    info!(
      "Successfully saved thumbnail URL to DB; comp_id={composition_id}, version_id={version_id}; \
       thumbnail_url={thumbnail_url}"
    );
  });
}
