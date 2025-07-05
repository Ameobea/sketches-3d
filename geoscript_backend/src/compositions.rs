use crate::auth::User;
use axum::{
  Json,
  extract::{Extension, Path, Query, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Arguments, FromRow, Row, SqlitePool, sqlite::SqliteArguments};

use crate::server::APIError;
use axum::http::StatusCode;

#[derive(Debug, FromRow, Serialize)]
pub struct Composition {
  pub id: i64,
  pub author_id: i64,
  pub author_username: String,
  pub title: String,
  pub description: String,
  pub forked_from_id: Option<i64>,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
  pub is_shared: bool,
  pub is_featured: bool,
}

#[derive(Debug, FromRow, Serialize)]
pub struct CompositionVersion {
  pub id: i64,
  pub composition_id: i64,
  pub source_code: String,
  pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateComposition {
  pub title: String,
  pub description: String,
  pub source_code: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateCompositionVersion {
  pub source_code: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCompositionPatch {
  pub title: Option<String>,
  pub description: Option<String>,
  pub is_shared: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCompositionRequest {
  pub field_mask: Vec<String>,
  pub patch: UpdateCompositionPatch,
}

pub async fn create_composition(
  State(pool): State<SqlitePool>,
  Extension(user): Extension<User>,
  Json(payload): Json<CreateComposition>,
) -> Result<Json<Composition>, APIError> {
  let mut tx = pool.begin().await.map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to begin transaction: {err}"),
    )
  })?;

  let id: i64 = sqlx::query(
    "INSERT INTO compositions (author_id, title, description) VALUES (?, ?, ?) RETURNING id",
  )
  .bind(user.id)
  .bind(&payload.title)
  .bind(&payload.description)
  .fetch_one(&mut *tx)
  .await
  .map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to insert composition: {err}"),
    )
  })?
  .try_get("id")
  .map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to get inserted composition id: {err}"),
    )
  })?;

  sqlx::query("INSERT INTO composition_versions (composition_id, source_code) VALUES (?, ?)")
    .bind(id)
    .bind(&payload.source_code)
    .execute(&mut *tx)
    .await
    .map_err(|err| {
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Failed to insert composition version: {err}"),
      )
    })?;

  tx.commit().await.map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to commit transaction: {err}"),
    )
  })?;

  get_composition(State(pool), Path(id), Extension(Some(user))).await
}

pub async fn create_composition_version(
  State(pool): State<SqlitePool>,
  Extension(user): Extension<User>,
  Path(composition_id): Path<i64>,
  Json(payload): Json<CreateCompositionVersion>,
) -> Result<Json<CompositionVersion>, APIError> {
  let _composition =
    sqlx::query_as::<_, Composition>("SELECT * FROM compositions WHERE id = ? AND author_id = ?")
      .bind(composition_id)
      .bind(user.id)
      .fetch_one(&pool)
      .await
      .map_err(|err| match err {
        sqlx::Error::RowNotFound => APIError::new(
          StatusCode::NOT_FOUND,
          "Composition not found or you do not have permission to modify it".to_owned(),
        ),
        _ => APIError::new(
          StatusCode::INTERNAL_SERVER_ERROR,
          format!("Failed to fetch composition: {err}"),
        ),
      })?;

  let version = sqlx::query_as::<_, CompositionVersion>(
    "INSERT INTO composition_versions (composition_id, source_code) VALUES (?, ?) RETURNING *",
  )
  .bind(composition_id)
  .bind(payload.source_code)
  .fetch_one(&pool)
  .await
  .map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to insert composition version: {err}"),
    )
  })?;

  sqlx::query("UPDATE compositions SET updated_at = ? WHERE id = ?")
    .bind(Utc::now())
    .bind(composition_id)
    .execute(&pool)
    .await
    .map_err(|err| {
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Failed to update composition timestamp: {err}"),
      )
    })?;

  Ok(Json(version))
}

pub async fn fork_composition(
  State(pool): State<SqlitePool>,
  Extension(user): Extension<User>,
  Path(composition_id): Path<i64>,
) -> Result<Json<Composition>, APIError> {
  let original_composition = sqlx::query_as::<_, Composition>(
    "SELECT c.*, u.username as author_username FROM compositions c JOIN users u ON c.author_id = \
     u.id WHERE c.id = ?",
  )
  .bind(composition_id)
  .fetch_one(&pool)
  .await
  .map_err(|err| {
    APIError::new(
      StatusCode::NOT_FOUND,
      format!("Original composition not found: {err}"),
    )
  })?;

  let latest_version = sqlx::query_as::<_, CompositionVersion>(
    "SELECT * FROM composition_versions WHERE composition_id = ? ORDER BY created_at DESC LIMIT 1",
  )
  .bind(composition_id)
  .fetch_one(&pool)
  .await
  .map_err(|err| {
    APIError::new(
      StatusCode::NOT_FOUND,
      format!("No versions found for composition: {err}"),
    )
  })?;

  let mut tx = pool.begin().await.map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to begin transaction: {err}"),
    )
  })?;

  let forked_composition_id = sqlx::query(
    "INSERT INTO compositions (author_id, title, description, forked_from_id) VALUES (?, ?, ?, ?) \
     RETURNING id",
  )
  .bind(user.id)
  .bind(format!("{} (fork)", original_composition.title))
  .bind(&original_composition.description)
  .bind(original_composition.id)
  .fetch_one(&mut *tx)
  .await
  .map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to insert forked composition: {err}"),
    )
  })?
  .try_get::<i64, _>("id")
  .map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to get forked composition id: {err}"),
    )
  })?;

  sqlx::query("INSERT INTO composition_versions (composition_id, source_code) VALUES (?, ?)")
    .bind(forked_composition_id)
    .bind(&latest_version.source_code)
    .execute(&mut *tx)
    .await
    .map_err(|err| {
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Failed to insert forked composition version: {err}"),
      )
    })?;

  let forked_composition = sqlx::query_as::<_, Composition>(
    "SELECT c.*, u.username as author_username FROM compositions c JOIN users u ON c.author_id = \
     u.id WHERE c.id = ?",
  )
  .bind(forked_composition_id)
  .fetch_one(&mut *tx)
  .await
  .map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to fetch forked composition with username: {err}"),
    )
  })?;

  tx.commit().await.map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to commit transaction: {err}"),
    )
  })?;

  Ok(Json(forked_composition))
}

pub async fn get_composition(
  State(pool): State<SqlitePool>,
  Path(composition_id): Path<i64>,
  Extension(user_opt): Extension<Option<User>>,
) -> Result<Json<Composition>, APIError> {
  fn not_found() -> APIError {
    APIError::new(StatusCode::NOT_FOUND, "Composition not found".to_owned())
  }

  let composition = sqlx::query_as::<_, Composition>(
    "SELECT c.*, u.username as author_username FROM compositions c JOIN users u ON c.author_id = \
     u.id WHERE c.id = ?",
  )
  .bind(composition_id)
  .fetch_one(&pool)
  .await
  .map_err(|_| not_found())?;

  if composition.is_shared {
    return Ok(Json(composition));
  }

  let Some(user) = user_opt else {
    return Err(not_found());
  };

  if user.id != composition.author_id {
    return Err(not_found());
  }

  Ok(Json(composition))
}

pub async fn list_composition_versions(
  State(pool): State<SqlitePool>,
  Path(composition_id): Path<i64>,
  Extension(user_opt): Extension<Option<User>>,
) -> Result<Json<Vec<i64>>, APIError> {
  // makes sure that the user has access to the composition
  let _ = get_composition(
    State(pool.clone()),
    Path(composition_id),
    Extension(user_opt),
  )
  .await?;

  let versions = sqlx::query("SELECT id FROM composition_versions WHERE composition_id = ?")
    .bind(composition_id)
    .fetch_all(&pool)
    .await
    .map_err(|err| {
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Failed to fetch composition versions: {err}"),
      )
    })?
    .into_iter()
    .map(|row| row.get::<i64, _>("id"))
    .collect();

  Ok(Json(versions))
}

pub async fn get_composition_latest(
  State(pool): State<SqlitePool>,
  Path(composition_id): Path<i64>,
  Extension(user_opt): Extension<Option<User>>,
) -> Result<Json<CompositionVersion>, APIError> {
  // makes sure that the user has access to the composition
  let _ = get_composition(
    State(pool.clone()),
    Path(composition_id),
    Extension(user_opt),
  )
  .await?;

  let version = sqlx::query_as::<_, CompositionVersion>(
    "SELECT * FROM composition_versions WHERE composition_id = ? ORDER BY created_at DESC LIMIT 1",
  )
  .bind(composition_id)
  .fetch_one(&pool)
  .await
  .map_err(|err| {
    APIError::new(
      StatusCode::NOT_FOUND,
      format!("Failed to fetch latest version: {err}"),
    )
  })?;

  Ok(Json(version))
}

pub async fn get_composition_version(
  State(pool): State<SqlitePool>,
  Path((composition_id, version)): Path<(i64, i64)>,
  Extension(user_opt): Extension<Option<User>>,
) -> Result<Json<CompositionVersion>, APIError> {
  // makes sure that the user has access to the composition
  let _ = get_composition(
    State(pool.clone()),
    Path(composition_id),
    Extension(user_opt),
  )
  .await?;

  let version = sqlx::query_as::<_, CompositionVersion>(
    "SELECT * FROM composition_versions WHERE composition_id = ? AND id = ?",
  )
  .bind(composition_id)
  .bind(version)
  .fetch_one(&pool)
  .await
  .map_err(|err| {
    APIError::new(
      StatusCode::NOT_FOUND,
      format!("Failed to fetch composition version: {err}"),
    )
  })?;

  Ok(Json(version))
}

pub async fn list_my_compositions(
  State(pool): State<SqlitePool>,
  Extension(user): Extension<User>,
) -> Result<Json<Vec<Composition>>, APIError> {
  let compositions = sqlx::query_as::<_, Composition>(
    "SELECT c.*, u.username as author_username FROM compositions c JOIN users u ON c.author_id = \
     u.id WHERE c.author_id = ? ORDER BY c.updated_at DESC",
  )
  .bind(user.id)
  .fetch_all(&pool)
  .await
  .map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to list compositions: {err}"),
    )
  })?;

  Ok(Json(compositions))
}

#[derive(Deserialize)]
pub struct ListPublicCompositionsQuery {
  featured_only: Option<bool>,
  count: Option<usize>,
}

pub async fn list_public_compositions(
  State(pool): State<SqlitePool>,
  Query(ListPublicCompositionsQuery {
    featured_only,
    count,
  }): Query<ListPublicCompositionsQuery>,
) -> Result<Json<Vec<Composition>>, APIError> {
  let query = format!(
    "SELECT c.*, u.username as author_username FROM compositions c JOIN users u ON c.author_id = \
     u.id WHERE c.is_shared = 1 {} ORDER BY c.updated_at DESC LIMIT ?",
    if featured_only.unwrap_or(false) {
      "AND c.is_featured = 1"
    } else {
      ""
    },
  );

  let compositions = sqlx::query_as::<_, Composition>(&query)
    .bind(count.unwrap_or(100).min(100) as i64)
    .fetch_all(&pool)
    .await
    .map_err(|err| {
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Failed to list public compositions: {err}"),
      )
    })?;

  Ok(Json(compositions))
}

pub async fn update_composition(
  State(pool): State<SqlitePool>,
  Extension(user): Extension<User>,
  Path(composition_id): Path<i64>,
  Json(payload): Json<UpdateCompositionRequest>,
) -> Result<Json<Composition>, APIError> {
  let allowed_fields = &["title", "description", "is_shared"];
  let mut set_clauses = Vec::with_capacity(payload.field_mask.len());
  let mut args = SqliteArguments::default();

  for field in &payload.field_mask {
    if !allowed_fields.contains(&field.as_str()) {
      return Err(APIError::new(
        StatusCode::BAD_REQUEST,
        format!("Field '{{field}}' cannot be updated"),
      ));
    }
    match field.as_str() {
      "title" =>
        if let Some(ref v) = payload.patch.title {
          set_clauses.push("title = ?".to_owned());
          args.add(v.clone()).unwrap();
        },
      "description" =>
        if let Some(ref v) = payload.patch.description {
          set_clauses.push("description = ?".to_owned());
          args.add(v.clone()).unwrap();
        },
      "is_shared" =>
        if let Some(v) = payload.patch.is_shared {
          set_clauses.push("is_shared = ?".to_owned());
          args.add(v as i32).unwrap();
        },
      _ => unreachable!("Unexpected field in field_mask: {field}"),
    }
  }

  if set_clauses.is_empty() {
    return Err(APIError::new(
      StatusCode::BAD_REQUEST,
      "No valid fields to update".to_owned(),
    ));
  }

  let set_clause = set_clauses.join(", ");
  let query =
    format!("UPDATE compositions SET {set_clause} WHERE id = ? AND author_id = ? RETURNING *");
  args.add(composition_id).unwrap();
  args.add(user.id).unwrap();

  let updated = sqlx::query_as_with::<_, Composition, _>(&query, args)
    .fetch_one(&pool)
    .await
    .map_err(|err| {
      APIError::new(
        StatusCode::NOT_FOUND,
        format!("Failed to update composition: {err}"),
      )
    })?;

  Ok(Json(updated))
}

pub async fn delete_composition(
  State(pool): State<SqlitePool>,
  Extension(user): Extension<User>,
  Path(composition_id): Path<i64>,
) -> Result<StatusCode, APIError> {
  let result = sqlx::query("DELETE FROM compositions WHERE id = ? AND author_id = ?")
    .bind(composition_id)
    .bind(user.id)
    .execute(&pool)
    .await
    .map_err(|err| {
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Failed to delete composition: {err}"),
      )
    })?;

  if result.rows_affected() == 0 {
    return Err(APIError::new(
      StatusCode::NOT_FOUND,
      "Composition not found or you do not have permission to delete it".to_owned(),
    ));
  }

  Ok(StatusCode::NO_CONTENT)
}
