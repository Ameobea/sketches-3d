use axum::Router;

use self::{
  auth::auth_routes, compositions::compositions_routes, materials::materials_routes,
  textures::texture_routes,
};

pub mod auth;
pub mod compositions;
pub mod materials;
pub mod textures;

pub fn app_routes() -> Router {
  Router::new()
    .nest("/users", auth_routes())
    .nest("/compositions", compositions_routes())
    .nest("/textures", texture_routes())
    .nest("/materials", materials_routes())
}
