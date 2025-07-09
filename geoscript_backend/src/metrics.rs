use foundations::telemetry::metrics::{metrics, Counter, HistogramBuilder, TimeHistogram};

#[metrics]
pub mod geoscript {
  /// Number of HTTP requests.
  pub fn requests_total(endpoint_name: &'static str) -> Counter;

  /// Number of successful HTTP requests.
  pub fn requests_success_total(endpoint_name: &'static str) -> Counter;

  /// Number of failed requests.
  pub fn requests_failed_total(endpoint_name: &'static str) -> Counter;

  /// Distribution of time taken to render thumbnails
  #[ctor = HistogramBuilder {
    buckets: &[0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 15.0, 20.0, 30.0, 60.0, 120.0, 300.0, 600.0],
  }]
  pub fn thumbnail_render_time() -> TimeHistogram;

  /// Distribution of time taken to upload thumbnails to object storage
  #[ctor = HistogramBuilder {
    buckets: &[0.01, 0.05, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 5.0, 7.5, 10.0]
  }]
  pub fn thumbnail_upload_time() -> TimeHistogram;
}
