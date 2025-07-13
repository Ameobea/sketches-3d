#![feature(once_cell_try)]

use std::{sync::Arc, time::Duration};

use foundations::{
  cli::{Arg, ArgAction, Cli},
  telemetry::{self, tokio_runtime_metrics::record_runtime_metrics_sample, TelemetryConfig},
  BootstrapResult,
};
use server::start_server;
use settings::ServerSettings;

use crate::{auth::cleanup_expired_sessions, db::init_db_pool};

#[macro_use]
extern crate tracing;

pub mod auth;
pub mod compositions;
pub mod db;
pub mod metrics;
pub mod object_storage;
pub mod render_thumbnail;
pub mod routes;
pub mod server;
pub mod settings;
pub mod textures;

async fn start() -> BootstrapResult<()> {
  let service_info = foundations::service_info!();

  let cli = Cli::<ServerSettings>::new(&service_info, vec![Arg::new("dry-run")
    .long("dry-run")
    .action(ArgAction::SetTrue)
    .help("Validate or generate config without running the server")])?;

  if cli.arg_matches.get_flag("dry-run") || cli.arg_matches.get_one::<String>("generate").is_some()
  {
    return Ok(());
  }

  let tele_serv_fut = telemetry::init(TelemetryConfig {
    custom_server_routes: Vec::new(),
    service_info: &service_info,
    settings: &cli.settings.telemetry,
  })?;
  if let Some(tele_serv_addr) = tele_serv_fut.server_addr() {
    info!("Telemetry server is listening on http://{tele_serv_addr}");
    tokio::task::spawn(tele_serv_fut);
  }

  let pool = init_db_pool(&cli.settings.sql.db_url).await?;

  tokio::task::spawn(async move {
    loop {
      if let Err(err) = cleanup_expired_sessions(&pool).await {
        error!("Failed to clean up expired sessions: {err:?}");
      }
      tokio::time::sleep(Duration::from_secs(600)).await;
    }
  });

  start_server(&cli.settings).await?;

  unreachable!("Server should not exit unless an error occurs")
}

fn main() -> BootstrapResult<()> {
  tracing_subscriber::fmt::fmt().init();

  let rt = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()?;

  let handle = rt.handle();
  foundations::telemetry::tokio_runtime_metrics::register_runtime(
    Some(Arc::from("geotoy")),
    None,
    handle,
  );
  info!("Registered tokio runtime metrics");

  rt.spawn(async move {
    // if we record metrics before configuring the metrics in `start()`, it will cause the name
    // configuration to be lost and make the names `undefined`.
    tokio::time::sleep(Duration::from_millis(500)).await;

    loop {
      record_runtime_metrics_sample();

      tokio::time::sleep(Duration::from_millis(500)).await;
    }
  });

  rt.block_on(start())
}
