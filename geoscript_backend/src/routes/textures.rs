use axum::{
  Router, middleware,
  routing::{delete, get, patch, post},
};

use crate::{
  auth::{auth_middleware, maybe_auth_middleware},
  db::get_db_pool,
  server::instrument_handler,
  textures::{
    create_texture, create_texture_from_url, delete_texture, get_multiple_textures, get_texture,
    list_textures, update_texture,
  },
};

pub fn texture_routes() -> Router {
  let db_pool = get_db_pool().clone();

  let maybe_authed_routes = Router::new()
    .route("/", get(instrument_handler("list_textures", list_textures)))
    .route(
      "/multiple",
      get(instrument_handler(
        "get_multiple_textures",
        get_multiple_textures,
      )),
    )
    .route("/{id}", get(instrument_handler("get_texture", get_texture)))
    .route_layer(middleware::from_fn_with_state(
      db_pool.clone(),
      maybe_auth_middleware,
    ));

  let auth_routes = Router::new()
    .route(
      "/",
      post(instrument_handler("create_texture", create_texture)),
    )
    .route(
      "/from_url",
      post(instrument_handler(
        "create_texture_from_url",
        create_texture_from_url,
      )),
    )
    .route(
      "/{id}",
      patch(instrument_handler("update_texture", update_texture)),
    )
    .route(
      "/{id}",
      delete(instrument_handler("delete_texture", delete_texture)),
    )
    .route_layer(middleware::from_fn_with_state(
      db_pool.clone(),
      auth_middleware,
    ));

  maybe_authed_routes
    .merge(auth_routes)
    .with_state(db_pool.clone())
}
