use axum::{
  Router,
  body::Bytes,
  http::{HeaderMap, StatusCode, header},
  response::IntoResponse,
  routing::post,
};

use crate::server::{APIError, instrument_handler};

const THUMBNAIL_GENERATOR_URL: &str = "http://localhost:5812/render_transient";
const AUTH_HEADER: &str = "x-cli-token";

pub fn render_routes() -> Router {
  Router::new().route(
    "/transient",
    post(instrument_handler("render_transient", render_transient)),
  )
}

async fn render_transient(headers: HeaderMap, body: Bytes) -> Result<impl IntoResponse, APIError> {
  let provided = headers
    .get(AUTH_HEADER)
    .and_then(|v| v.to_str().ok())
    .unwrap_or("");
  let expected = &crate::server::settings().cli_token;
  if expected.is_empty() || provided != expected.as_str() {
    return Err(APIError::new(StatusCode::UNAUTHORIZED, "Invalid CLI token"));
  }

  let client = reqwest::Client::builder()
    .timeout(std::time::Duration::from_secs(15 * 60))
    .build()
    .map_err(|err| {
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("http client: {err}"),
      )
    })?;

  let upstream = client
    .post(THUMBNAIL_GENERATOR_URL)
    .header(header::CONTENT_TYPE, "application/json")
    .body(body)
    .send()
    .await
    .map_err(|err| {
      APIError::new(
        StatusCode::BAD_GATEWAY,
        format!("Failed to reach thumbnail_generator: {err}"),
      )
    })?;

  let status = upstream.status();
  let content_type = upstream
    .headers()
    .get(header::CONTENT_TYPE)
    .cloned()
    .unwrap_or_else(|| header::HeaderValue::from_static("application/octet-stream"));
  let body_bytes = upstream
    .bytes()
    .await
    .map_err(|err| APIError::new(StatusCode::BAD_GATEWAY, format!("Upstream body: {err}")))?;

  let axum_status = StatusCode::from_u16(status.as_u16())
    .map_err(|_| APIError::new(StatusCode::BAD_GATEWAY, "Invalid upstream status"))?;

  let mut resp = (axum_status, body_bytes).into_response();
  resp
    .headers_mut()
    .insert(header::CONTENT_TYPE, content_type);
  Ok(resp)
}
