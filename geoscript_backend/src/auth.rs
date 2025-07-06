use argon2::{
  Argon2,
  password_hash::{PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use axum::{
  Json,
  body::Body,
  extract::{Extension, State},
  http::{Request, StatusCode},
  middleware::Next,
  response::Response,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use tower_cookies::{Cookie, Cookies};

use crate::server::APIError;

#[derive(Debug, FromRow, Serialize, Clone)]
pub struct User {
  pub id: i64,
  pub username: String,
  #[serde(skip)]
  pub password_hash: String,
}

#[derive(Debug, Deserialize)]
pub struct Registration {
  pub username: String,
  pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct Login {
  pub username: String,
  pub password: String,
}

pub async fn register(
  State(pool): State<SqlitePool>,
  Json(registration): Json<Registration>,
) -> Result<Json<User>, APIError> {
  if registration.username.is_empty() || registration.password.is_empty() {
    return Err(APIError::new(
      StatusCode::BAD_REQUEST,
      "Username and password cannot be empty.",
    ));
  }

  if registration.password.len() < 6 {
    return Err(APIError::new(
      StatusCode::BAD_REQUEST,
      "Password must be at least 6 characters long, but you should really use more than that :)",
    ));
  }

  let salt = SaltString::generate(&mut OsRng);
  let hashed_password = Argon2::default()
    .hash_password(registration.password.as_bytes(), &salt)
    .map_err(|_| {
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Failed to hash password.",
      )
    })?
    .to_string();

  let user = sqlx::query_as::<_, User>(
    "INSERT INTO users (username, password_hash) VALUES (?, ?) RETURNING *",
  )
  .bind(registration.username)
  .bind(hashed_password)
  .fetch_one(&pool)
  .await
  .map_err(|err| match err {
    sqlx::Error::Database(err) if err.is_unique_violation() =>
      APIError::new(StatusCode::CONFLICT, "Username already exists."),
    _ => APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Failed to create user."),
  })?;

  Ok(Json(user))
}

pub async fn login(
  State(pool): State<SqlitePool>,
  cookies: Cookies,
  Json(login): Json<Login>,
) -> Result<Json<User>, APIError> {
  let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = ?")
    .bind(&login.username)
    .fetch_optional(&pool)
    .await
    .map_err(|_| APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch user."))?
    .ok_or_else(|| APIError::new(StatusCode::UNAUTHORIZED, "Invalid credentials."))?;

  let parsed_hash = argon2::PasswordHash::new(&user.password_hash).map_err(|_| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      "Failed to parse password hash.",
    )
  })?;

  Argon2::default()
    .verify_password(login.password.as_bytes(), &parsed_hash)
    .map_err(|_| APIError::new(StatusCode::UNAUTHORIZED, "Invalid credentials."))?;

  let session_id = create_session(&pool, user.id).await?;

  cookies.add(
    Cookie::build(("session_id", session_id))
      .http_only(true)
      .secure(true)
      .path("/")
      .same_site(tower_cookies::cookie::SameSite::Lax)
      .into(),
  );

  Ok(Json(user))
}

pub async fn me(Extension(user): Extension<User>) -> Json<User> { Json(user) }

pub async fn get_user(
  State(pool): State<SqlitePool>,
  axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<Json<User>, APIError> {
  let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
    .bind(id)
    .fetch_one(&pool)
    .await
    .map_err(|_| APIError::new(StatusCode::NOT_FOUND, "User not found."))?;

  Ok(Json(user))
}

pub async fn logout(
  cookies: Cookies,
  State(pool): State<SqlitePool>,
) -> Result<Json<()>, APIError> {
  let session_cookie = cookies
    .get("session_id")
    .ok_or_else(|| APIError::new(StatusCode::UNAUTHORIZED, "Not logged in."))?;

  sqlx::query("DELETE FROM sessions WHERE id = ?")
    .bind(session_cookie.value())
    .execute(&pool)
    .await
    .map_err(|_| {
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Failed to delete session.",
      )
    })?;

  cookies.remove(Cookie::from("session_id"));

  Ok(Json(()))
}

async fn create_session(pool: &SqlitePool, user_id: i64) -> Result<String, APIError> {
  let mut session_id_bytes = [0u8; 32];
  OsRng.fill_bytes(&mut session_id_bytes);
  let session_id = URL_SAFE_NO_PAD.encode(session_id_bytes);

  sqlx::query("INSERT INTO sessions (id, user_id, expires_at) VALUES (?, ?, ?)")
    .bind(&session_id)
    .bind(user_id)
    .bind(Utc::now() + Duration::days(7))
    .execute(pool)
    .await
    .map_err(|_| {
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Failed to create session.",
      )
    })?;

  Ok(session_id)
}

async fn get_user_from_session_cookie(
  session_id: &str,
  pool: &SqlitePool,
) -> Result<Option<User>, APIError> {
  sqlx::query_as::<_, User>(
    "SELECT u.* FROM users u JOIN sessions s ON u.id = s.user_id WHERE s.id = ? AND s.expires_at \
     > ?",
  )
  .bind(session_id)
  .bind(Utc::now())
  .fetch_optional(pool)
  .await
  .map_err(|_| APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch user."))
}

pub async fn auth_middleware(
  cookies: Cookies,
  State(pool): State<SqlitePool>,
  mut req: Request<Body>,
  next: Next,
) -> Result<Response, APIError> {
  let session_cookie = cookies
    .get("session_id")
    .ok_or_else(|| APIError::new(StatusCode::UNAUTHORIZED, "Not logged in."))?;

  let user = get_user_from_session_cookie(session_cookie.value(), &pool)
    .await?
    .ok_or_else(|| APIError::new(StatusCode::UNAUTHORIZED, "Invalid session."))?;

  req.extensions_mut().insert(user);

  Ok(next.run(req).await)
}

pub async fn maybe_auth_middleware(
  cookies: Cookies,
  State(pool): State<SqlitePool>,
  mut req: Request<Body>,
  next: Next,
) -> Result<Response, APIError> {
  let user_opt = if let Some(session_cookie) = cookies.get("session_id") {
    get_user_from_session_cookie(session_cookie.value(), &pool).await?
  } else {
    None
  };
  req.extensions_mut().insert(user_opt);
  Ok(next.run(req).await)
}

pub async fn cleanup_expired_sessions(pool: &SqlitePool) -> Result<(), APIError> {
  sqlx::query("DELETE FROM sessions WHERE expires_at < ?")
    .bind(Utc::now())
    .execute(pool)
    .await
    .map_err(|_| {
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Failed to clean up sessions.",
      )
    })?;

  Ok(())
}
