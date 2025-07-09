use foundations::{settings::settings, telemetry::settings::TelemetrySettings};
use serde_default_utils::*;

#[settings]
pub struct SqlSettings {
  pub db_url: String,
}

#[settings]
pub struct ObjectStorageSettings {
  pub endpoint: String,
  pub bucket_name: String,
  pub access_key: String,
  pub secret_access_key: String,
  pub public_endpoint: String,
}

#[settings]
pub struct BunnyStorageSettings {
  pub access_key: String,
  pub storage_zone_name: String,
  pub public_endpoint: String,
}

#[settings]
pub struct ServerSettings {
  /// Telemetry settings.
  pub telemetry: TelemetrySettings,

  /// Port that the HTTP server will listen on.
  #[serde(default = "default_u16::<5810>")]
  pub port: u16,
  pub admin_token: String,

  pub sql: SqlSettings,

  pub object_storage: ObjectStorageSettings,
  pub bunny_storage: BunnyStorageSettings,
}
