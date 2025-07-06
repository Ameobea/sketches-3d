use std::{
  future::Future,
  marker::PhantomData,
  pin::Pin,
  sync::OnceLock,
  task::{Context, Poll},
};

use axum::{
  extract::Request,
  handler::Handler,
  http::StatusCode,
  response::{IntoResponse, Response},
};
use foundations::BootstrapResult;
use tower_cookies::CookieManagerLayer;
use tower_http::{
  cors,
  trace::{DefaultMakeSpan, DefaultOnResponse},
};
use tracing::Level;

use crate::{metrics::geoscript, routes, settings::ServerSettings};

#[derive(Clone)]
pub(crate) struct InstrumentedHandler<H, S> {
  pub endpoint_name: &'static str,
  pub handler: H,
  pub state: PhantomData<S>,
}

#[pin_project::pin_project]
pub(crate) struct InstrumentedHandlerFuture<F> {
  #[pin]
  inner: F,
  endpoint_name: &'static str,
}

impl<F: Unpin + Future<Output = Response>> Future for InstrumentedHandlerFuture<F> {
  type Output = Response;

  fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    let endpoint_name = self.endpoint_name;
    let this = self.project();
    let inner = this.inner;
    let inner_poll = inner.poll(cx);

    match inner_poll {
      Poll::Ready(res) => {
        geoscript::requests_total(endpoint_name).inc();

        if res.status().is_success() {
          geoscript::requests_success_total(endpoint_name).inc();
        } else {
          geoscript::requests_failed_total(endpoint_name).inc();
        }
        Poll::Ready(res)
      },
      Poll::Pending => Poll::Pending,
    }
  }
}

impl<T, H: Handler<T, S>, S: Clone + Send + Sync + 'static> Handler<T, S>
  for InstrumentedHandler<H, S>
where
  H::Future: Unpin,
{
  type Future = InstrumentedHandlerFuture<H::Future>;

  fn call(self, req: Request, state: S) -> Self::Future {
    let res_future = self.handler.call(req, state);
    InstrumentedHandlerFuture {
      inner: res_future,
      endpoint_name: self.endpoint_name,
    }
  }
}

pub(crate) fn instrument_handler<
  T: 'static,
  H: Handler<T, S> + 'static,
  S: Clone + Send + 'static,
>(
  endpoint_name: &'static str,
  handler: H,
) -> InstrumentedHandler<H, S> {
  InstrumentedHandler {
    endpoint_name,
    handler,
    state: PhantomData,
  }
}

#[derive(Debug)]
pub struct APIError {
  pub status: StatusCode,
  pub message: String,
}

impl APIError {
  pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
    APIError {
      status,
      message: message.into(),
    }
  }
}

impl IntoResponse for APIError {
  fn into_response(self) -> Response { (self.status, self.message).into_response() }
}

static SETTINGS: OnceLock<ServerSettings> = OnceLock::new();

pub fn settings() -> &'static ServerSettings {
  SETTINGS
    .get()
    .expect("Server settings have not been initialized")
}

pub async fn start_server(settings: &ServerSettings) -> BootstrapResult<()> {
  SETTINGS.set(settings.clone()).unwrap();

  let mut router = routes::app_routes();

  router = router
    .layer(CookieManagerLayer::new())
    .layer(
      tower_http::cors::CorsLayer::new()
        .allow_origin(cors::AllowOrigin::list([
          "http://localhost:4800".parse().unwrap(),
          "https://3d.ameo.design".parse().unwrap(),
        ]))
        .allow_credentials(true)
        .allow_headers(cors::AllowHeaders::list([
          axum::http::header::CONTENT_TYPE,
          axum::http::header::AUTHORIZATION,
          axum::http::header::ACCEPT,
          axum::http::header::ORIGIN,
          axum::http::header::COOKIE,
        ]))
        .allow_methods(cors::AllowMethods::list([
          axum::http::Method::GET,
          axum::http::Method::POST,
          axum::http::Method::PATCH,
          axum::http::Method::DELETE,
          axum::http::Method::OPTIONS,
          axum::http::Method::PUT,
          axum::http::Method::HEAD,
          axum::http::Method::TRACE,
          axum::http::Method::CONNECT,
        ])),
    )
    .layer(
      tower_http::trace::TraceLayer::new_for_http()
        .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
        .on_response(DefaultOnResponse::default().level(Level::INFO)),
    );

  let addr = format!("0.0.0.0:{}", settings.port);
  info!("Server is listening on http://{addr}");
  let listener = tokio::net::TcpListener::bind(addr).await?;
  axum::serve(listener, router).await?;
  Ok(())
}
