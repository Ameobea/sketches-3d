use std::{
  marker::PhantomData,
  pin::Pin,
  task::{Context, Poll},
};

use axum::{
  extract::Request,
  handler::Handler,
  http::StatusCode,
  response::{IntoResponse, Response},
  Router,
};
use foundations::BootstrapResult;
use tower_http::{
  cors,
  trace::{DefaultMakeSpan, DefaultOnResponse},
};
use tracing::Level;

use crate::{metrics::geoscript, routes, settings::ServerSettings};

#[derive(Clone)]
struct InstrumentedHandler<H, S> {
  pub endpoint_name: &'static str,
  pub handler: H,
  pub state: PhantomData<S>,
}

#[pin_project::pin_project]
struct InstrumentedHandlerFuture<F> {
  #[pin]
  inner: F,
  endpoint_name: &'static str,
}

impl<F: Unpin + Future<Output = Response>> Future for InstrumentedHandlerFuture<F> {
  type Output = Response;

  fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    geoscript::requests_total(self.endpoint_name).inc();

    let endpoint_name = self.endpoint_name;
    let this = self.project();
    let inner = this.inner;
    let inner_poll = inner.poll(cx);

    match inner_poll {
      Poll::Ready(res) => {
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

fn instrument_handler<T: 'static, H: Handler<T, S> + 'static, S: Clone + Send + 'static>(
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

impl IntoResponse for APIError {
  fn into_response(self) -> Response { (self.status, self.message).into_response() }
}

pub async fn start_server(settings: &ServerSettings) -> BootstrapResult<()> {
  let mut router = Router::new(); //TODO

  router = router
    .layer(
      tower_http::cors::CorsLayer::new()
        .allow_origin(cors::AllowOrigin::list([
          "http://localhost:4800".parse().unwrap(),
          "https://3d.ameo.design".parse().unwrap(),
        ]))
        // .allow_credentials(true)
        .allow_headers(cors::Any)
        .allow_methods(cors::Any),
    )
    .layer(
      tower_http::trace::TraceLayer::new_for_http()
        .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
        .on_response(DefaultOnResponse::default().level(Level::INFO)),
    );

  let addr = format!("0.0.0.0:{}", settings.port);
  info!("Server is listening on http://{}", addr);
  let listener = tokio::net::TcpListener::bind(addr).await?;
  axum::serve(listener, router).await?;
  Ok(())
}
