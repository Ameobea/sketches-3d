use std::time::Duration;

use foundations::{
  BootstrapResult,
  cli::{Arg, ArgAction, Cli},
  telemetry::{self, TelemetryConfig, tokio_runtime_metrics::record_runtime_metrics_sample},
};
use server::start_server;
use settings::ServerSettings;

use crate::db::init_db_pool;

#[macro_use]
extern crate tracing;

pub mod db;
pub mod metrics;
pub mod routes;
pub mod server;
pub mod settings;

async fn start() -> BootstrapResult<()> {
  let service_info = foundations::service_info!();

  // Parse command line arguments. Add additional command line option that allows checking
  // the config without running the server.
  let cli = Cli::<ServerSettings>::new(&service_info, vec![
    Arg::new("dry-run")
      .long("dry-run")
      .action(ArgAction::SetTrue)
      .help("Validate or generate config without running the server"),
  ])?;

  // Exit if we just want to check the config.
  if cli.arg_matches.get_flag("dry-run") || cli.arg_matches.get_one::<String>("generate").is_some()
  {
    return Ok(());
  }

  // Initialize telemetry with the settings obtained from the config.
  let tele_serv_fut = telemetry::init(TelemetryConfig {
    custom_server_routes: Vec::new(),
    service_info: &service_info,
    settings: &cli.settings.telemetry,
  })?;
  if let Some(tele_serv_addr) = tele_serv_fut.server_addr() {
    info!("Telemetry server is listening on http://{}", tele_serv_addr);
    tokio::task::spawn(tele_serv_fut);
  }

  init_db_pool(&cli.settings.sql.db_url).await?;

  start_server(&cli.settings).await?;

  unreachable!("Server should not exit unless an error occurs")
}

fn main() -> BootstrapResult<()> {
  tracing_subscriber::fmt::fmt().init();

  let rt = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()?;

  let handle = rt.handle();
  foundations::telemetry::tokio_runtime_metrics::register_runtime(None, None, handle);
  info!("Registered tokio runtime metrics");

  rt.spawn(async move {
    loop {
      record_runtime_metrics_sample();

      // record metrics roughly twice a second
      tokio::time::sleep(Duration::from_millis(500)).await;
    }
  });

  rt.block_on(start())
}
