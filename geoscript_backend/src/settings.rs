use foundations::{settings::settings, telemetry::settings::TelemetrySettings};
use serde_default_utils::*;

#[settings]
pub struct SqlSettings {
  pub db_url: String,
}

#[settings]
pub struct ServerSettings {
  /// Telemetry settings.
  pub telemetry: TelemetrySettings,

  /// Port that the HTTP server will listen on.
  #[serde(default = "default_u16::<5810>")]
  pub port: u16,

  pub sql: SqlSettings,
}
