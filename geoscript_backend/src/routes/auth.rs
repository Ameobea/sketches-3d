use crate::{
  auth::{auth_middleware, get_user, login, logout, me, register},
  db::get_db_pool,
  server::instrument_handler,
};
use axum::{
  middleware,
  routing::{get, post},
  Router,
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
    .route("/user/{id}", get(instrument_handler("get_user", get_user)))
    .merge(protected_routes)
    .with_state(get_db_pool().clone())
}
