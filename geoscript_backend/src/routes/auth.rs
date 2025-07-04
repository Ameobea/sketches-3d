use crate::{
  auth::{auth_middleware, login, logout, me, register},
  db::get_db_pool,
  server::instrument_handler,
};
use axum::{
  Router, middleware,
  routing::{get, post},
};

pub fn auth_routes() -> Router {
  let protected_routes = Router::new()
    .route("/me", get(instrument_handler("me", me)))
    .route("/logout", post(instrument_handler("logout", logout)))
    .route_layer(middleware::from_fn_with_state(
      get_db_pool().clone(),
      auth_middleware,
    ));

  Router::new()
    .route("/register", post(instrument_handler("register", register)))
    .route("/login", post(instrument_handler("login", login)))
    .merge(protected_routes)
    .with_state(get_db_pool().clone())
}
