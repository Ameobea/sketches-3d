use std::time::Duration;

use axum::http::StatusCode;
use serde::Deserialize;
use uuid::Uuid;

use crate::{metrics, server::APIError};

pub async fn index() -> &'static str { "dream-backend up and running successfully!" }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayCompletedRequest {
  pub level_name: String,
  pub is_win: bool,
  pub time_seconds: f64,
  pub user_id: Option<Uuid>,
}

pub async fn play_completed(body: String) -> Result<(), APIError> {
  let deserializer = &mut serde_json::Deserializer::from_str(&body);
  let PlayCompletedRequest {
    level_name,
    is_win,
    time_seconds,
    user_id,
  } = serde_path_to_error::deserialize::<_, PlayCompletedRequest>(deserializer).map_err(|err| {
    error!("Error parsing request body: {err}");
    APIError {
      status: StatusCode::BAD_REQUEST,
      message: format!("Error parsing request body: {err}"),
    }
  })?;

  info!(
    "Level '{level_name}' completed; user_id={user_id:?}, is_win={is_win}, \
     time_seconds={time_seconds}"
  );

  let dur = Duration::from_secs_f64(time_seconds);
  let time_nanos = dur.as_nanos() as u64;
  metrics::http_server::level_completions(level_name, is_win, user_id).observe(time_nanos);

  Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LevelRestartedRequest {
  pub level_name: String,
  pub user_id: Option<Uuid>,
  pub time_seconds: f64,
}

pub async fn level_restarted(body: String) -> Result<(), APIError> {
  let deserializer = &mut serde_json::Deserializer::from_str(&body);
  let LevelRestartedRequest {
    level_name,
    user_id,
    time_seconds,
  } =
    serde_path_to_error::deserialize::<_, LevelRestartedRequest>(deserializer).map_err(|err| {
      error!("Error parsing request body: {err}");
      APIError {
        status: StatusCode::BAD_REQUEST,
        message: format!("Error parsing request body: {err}"),
      }
    })?;

  info!("Level '{level_name}' restarted after {time_seconds}s; user_id={user_id:?}");

  let dur = Duration::from_secs_f64(time_seconds);
  let time_nanos = dur.as_nanos() as u64;
  metrics::http_server::level_restarts(level_name, user_id).observe(time_nanos);

  Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PortalTravelRequest {
  pub destination_level_name: String,
  pub user_id: Option<Uuid>,
}

pub async fn portal_travel(body: String) -> Result<(), APIError> {
  let deserializer = &mut serde_json::Deserializer::from_str(&body);
  let PortalTravelRequest {
    destination_level_name,
    user_id,
  } = serde_path_to_error::deserialize::<_, PortalTravelRequest>(deserializer).map_err(|err| {
    error!("Error parsing request body: {err}");
    APIError {
      status: StatusCode::BAD_REQUEST,
      message: format!("Error parsing request body: {err}"),
    }
  })?;

  info!("Portal travel to '{destination_level_name}'; user_id={user_id:?}");

  metrics::http_server::portal_travel(destination_level_name, user_id).inc();

  Ok(())
}
