use crate::{
  auth::{auth_middleware, maybe_auth_middleware},
  compositions::{
    create_composition, create_composition_version, delete_composition, fork_composition,
    get_composition, get_composition_history, get_composition_latest, get_composition_version,
    list_composition_versions, list_my_compositions, list_public_compositions, update_composition,
  },
  db::get_db_pool,
  server::instrument_handler,
};
use axum::{
  Router, middleware,
  routing::{delete, get, patch, post},
};

pub fn compositions_routes() -> Router {
  let db_pool = get_db_pool().clone();
  let protected_routes = Router::new()
    .route(
      "/my",
      get(instrument_handler(
        "list_my_compositions",
        list_my_compositions,
      )),
    )
    .route(
      "/",
      post(instrument_handler("create_composition", create_composition)),
    )
    .route(
      "/{id}",
      patch(instrument_handler("update_composition", update_composition)),
    )
    .route(
      "/{id}",
      delete(instrument_handler("delete_composition", delete_composition)),
    )
    .route(
      "/{id}/fork",
      post(instrument_handler("fork_composition", fork_composition)),
    )
    .route(
      "/{id}/versions",
      post(instrument_handler(
        "create_composition_version",
        create_composition_version,
      )),
    )
    .route_layer(middleware::from_fn_with_state(
      db_pool.clone(),
      auth_middleware,
    ));

  let maybe_authed_routes = Router::new()
    .route(
      "/{id}",
      get(instrument_handler("get_composition", get_composition)),
    )
    .route(
      "/{id}/history",
      get(instrument_handler(
        "get_composition_history",
        get_composition_history,
      )),
    )
    .route(
      "/{id}/versions",
      get(instrument_handler(
        "list_composition_versions",
        list_composition_versions,
      )),
    )
    .route(
      "/{id}/latest",
      get(instrument_handler(
        "get_composition_latest",
        get_composition_latest,
      )),
    )
    .route(
      "/{id}/version/{version}",
      get(instrument_handler(
        "get_composition_version",
        get_composition_version,
      )),
    )
    .route_layer(middleware::from_fn_with_state(
      db_pool.clone(),
      maybe_auth_middleware,
    ));

  Router::new()
    .route(
      "/",
      get(instrument_handler(
        "list_public_compositions",
        list_public_compositions,
      )),
    )
    .merge(protected_routes)
    .merge(maybe_authed_routes)
    .with_state(db_pool)
}
