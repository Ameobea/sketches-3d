use bytes::Bytes;
use reqwest::StatusCode;

use crate::server::APIError;

pub async fn upload_file(path: &str, content: Bytes) -> Result<String, APIError> {
  let settings = crate::server::settings();
  let client = reqwest::Client::new();
  let url = format!(
    "https://storage.bunnycdn.com/{}/{path}",
    settings.bunny_storage.storage_zone_name
  );

  let res = client
    .put(&url)
    .header("AccessKey", &settings.bunny_storage.access_key)
    .header("content-type", "application/octet-stream")
    .body(content)
    .send()
    .await
    .map_err(|err| {
      error!("Error uploading to bunny storage: {err}");
      APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Failed to upload file")
    })?;

  if !res.status().is_success() {
    error!(
      "Error uploading to bunny storage: {}; {}",
      res.status(),
      res
        .text()
        .await
        .unwrap_or_else(|_| "<failed to get body>".to_string())
    );
    return Err(APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      "Failed to upload file",
    ));
  }

  Ok(format!(
    "{}/{}",
    settings.bunny_storage.public_endpoint, path
  ))
}
