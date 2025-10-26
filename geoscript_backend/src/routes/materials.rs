use axum::{
  Router, middleware,
  routing::{delete, get, post, put},
};

use crate::{
  auth::{auth_middleware, maybe_auth_middleware},
  db::get_db_pool,
  materials,
  server::instrument_handler,
};

pub fn materials_routes() -> Router {
  let db_pool = get_db_pool().clone();

  let protected_routes = Router::new()
    .route(
      "/",
      post(instrument_handler(
        "create_material",
        materials::create_material,
      )),
    )
    .route(
      "/{id}",
      put(instrument_handler(
        "update_material",
        materials::update_material,
      )),
    )
    .route(
      "/{id}",
      delete(instrument_handler(
        "delete_material",
        materials::delete_material,
      )),
    )
    .route_layer(middleware::from_fn_with_state(
      db_pool.clone(),
      auth_middleware,
    ));

  let maybe_authed_routes = Router::new()
    .route(
      "/",
      get(instrument_handler(
        "list_materials",
        materials::list_materials,
      )),
    )
    .route(
      "/{id}",
      get(instrument_handler("get_material", materials::get_material)),
    )
    .route_layer(middleware::from_fn_with_state(
      db_pool.clone(),
      maybe_auth_middleware,
    ));

  Router::new()
    .merge(protected_routes)
    .merge(maybe_authed_routes)
    .with_state(db_pool)
}
