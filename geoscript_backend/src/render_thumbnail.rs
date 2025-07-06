use sqlx::SqlitePool;

/// Spawns a background task to render a thumbnail for the given composition version, upload it to
/// cloud storage, and save the thumbnail URL in the DB.
pub fn render_thumbnail(pool: &'static SqlitePool, composition_id: i64, version_id: i64) {
  tokio::spawn(async move {
    let url = format!(
      "http://localhost:5812/render/4?admin_token={}&version_id={version_id}",
      crate::server::settings().admin_token
    );

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

    // TODO: upload to cloud storage
    let thumbnail_url = "TEMP.png".to_owned();

    let res = sqlx::query(
      "UPDATE composition_versions SET thumbnail_url = ? WHERE id = ? AND composition_id = ?",
    )
    .bind(&thumbnail_url)
    .bind(version_id)
    .bind(composition_id)
    .execute(pool)
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
