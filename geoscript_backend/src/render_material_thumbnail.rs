use std::time::Instant;

use crate::{
  metrics::geoscript::{thumbnail_render_time, thumbnail_upload_time},
  object_storage::upload_file,
};
use sqlx::SqlitePool;
use uuid::Uuid;

/// Spawns a background task to render a thumbnail for the given material, upload it to
/// cloud storage, and save the thumbnail URL in the DB.
pub fn render_material_thumbnail(pool: SqlitePool, material_id: i64, material_definition: String) {
  tokio::spawn(async move {
    info!("Rendering thumbnail for material_id={material_id} ...");
    let url = format!(
      "http://localhost:5812/render_material/{}?admin_token={}",
      material_id,
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
    let thumbnail_path = format!("material_thumbnails/{}.avif", Uuid::new_v4());
    let thumbnail_url = match upload_file(&thumbnail_path, thumbnail_image).await {
      Ok(url) => url,
      Err(err) => {
        error!("Failed to upload thumbnail to cloud storage: {err:?}");
        return;
      },
    };
    thumbnail_upload_time().observe(start.elapsed().as_nanos() as u64);

    let maybe_current_def: Option<(String,)> =
      sqlx::query_as("SELECT material_definition FROM materials WHERE id = ?")
        .bind(material_id)
        .fetch_optional(&pool)
        .await
        .unwrap_or_else(|err| {
          error!("Failed to fetch material definition for race condition check: {err}");
          None
        });

    if let Some((current_def,)) = maybe_current_def {
      if current_def != material_definition {
        info!(
          "Race condition detected for material thumbnail rendering, aborting update. \
           material_id={material_id}"
        );
        return;
      }
    } else {
      // Material was deleted before we could update it.
      info!(
        "Material not found for thumbnail update, it was likely deleted. material_id={material_id}"
      );
      return;
    }

    let res = sqlx::query("UPDATE materials SET thumbnail_url = ? WHERE id = ?")
      .bind(&thumbnail_url)
      .bind(material_id)
      .execute(&pool)
      .await;
    if let Err(err) = res {
      error!("Failed to save thumbnail URL to DB: {err}");
      return;
    }

    info!(
      "Successfully saved thumbnail URL to DB; material_id={material_id}; \
       thumbnail_url={thumbnail_url}"
    );
  });
}
