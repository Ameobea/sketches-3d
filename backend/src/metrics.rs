use foundations::telemetry::metrics::{metrics, Counter, HistogramBuilder, TimeHistogram};
use uuid::Uuid;

#[metrics]
pub mod http_server {
  /// Number of HTTP requests.
  pub fn requests_total(endpoint_name: &'static str) -> Counter;

  /// Number of successful HTTP requests.
  pub fn requests_success_total(endpoint_name: &'static str) -> Counter;

  /// Number of failed requests.
  pub fn requests_failed_total(endpoint_name: &'static str) -> Counter;

  /// Successful level completions
  #[ctor = HistogramBuilder {
    buckets: &[5., 7.5, 10., 12.5, 15., 17.5, 20., 22.5, 25., 27.5, 30., 32.5, 35., 37.5, 40., 42.5, 45., 47.5, 50., 52.5, 55., 57.5, 60., 62.5, 65., 67.5, 70., 72.5, 75., 77.5, 80., 82.5, 85., 87.5, 90.]
  }]
  pub fn level_completions(
    level_name: String,
    is_win: bool,
    user_id: Option<Uuid>,
  ) -> TimeHistogram;

  /// Level restarts
  #[ctor = HistogramBuilder {
    buckets: &[0.1, 0.5, 1., 2., 3., 4., 5., 7.5, 10., 12.5, 15., 17.5, 20., 22.5, 25., 27.5, 30., 32.5, 35., 37.5, 40., 42.5, 45., 47.5, 50., 52.5, 55., 57.5, 60., 62.5, 65., 67.5, 70., 72.5, 75., 77.5, 80., 82.5, 85., 87.5, 90.]
  }]
  pub fn level_restarts(level_name: String, user_id: Option<Uuid>) -> TimeHistogram;

  /// Portal travels
  pub fn portal_travel(level_name: String, user_id: Option<Uuid>) -> Counter;
}
