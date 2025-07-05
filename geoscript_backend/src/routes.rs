use axum::Router;

use self::{auth::auth_routes, compositions::compositions_routes};

pub mod auth;
pub mod compositions;

pub fn app_routes() -> Router {
  Router::new()
    .nest("/users", auth_routes())
    .nest("/compositions", compositions_routes())
}
