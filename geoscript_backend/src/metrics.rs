use foundations::telemetry::metrics::{metrics, Counter};

#[metrics]
pub mod geoscript {
  /// Number of HTTP requests.
  pub fn requests_total(endpoint_name: &'static str) -> Counter;

  /// Number of successful HTTP requests.
  pub fn requests_success_total(endpoint_name: &'static str) -> Counter;

  /// Number of failed requests.
  pub fn requests_failed_total(endpoint_name: &'static str) -> Counter;
}
