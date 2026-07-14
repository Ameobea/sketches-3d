use axum::{
  Extension, Json,
  extract::{Path, State},
};
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::{FromRow, Row, SqlitePool};
use tracing::error;
use url::Url;

use crate::{
  auth::User,
  render_material_thumbnail::render_material_thumbnail,
  server::APIError,
  tags::{MATERIAL_TAGS, load_tags, load_tags_one, set_tags},
};

const MATERIAL_SELECT: &str = "SELECT materials.id, materials.name, materials.description, \
                               materials.thumbnail_url, materials.material_definition, \
                               materials.owner_id, users.username as owner_name, \
                               materials.created_at, materials.is_shared FROM materials JOIN \
                               users ON materials.owner_id = users.id";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Material {
  id: i64,
  name: String,
  description: String,
  thumbnail_url: Option<Url>,
  material_definition: JsonValue,
  owner_id: i64,
  owner_name: String,
  created_at: DateTime<Utc>,
  is_shared: bool,
  tags: Vec<String>,
}

#[derive(FromRow, Debug)]
pub struct MaterialRow {
  id: i64,
  name: String,
  description: String,
  thumbnail_url: Option<String>,
  material_definition: String,
  owner_id: i64,
  owner_name: String,
  created_at: DateTime<Utc>,
  is_shared: bool,
}

impl MaterialRow {
  fn into_material(self, tags: Vec<String>) -> Result<Material, APIError> {
    let material_definition: JsonValue =
      serde_json::from_str(&self.material_definition).map_err(|err| {
        error!("invalid material definition JSON in db: {}: {err}", self.id);
        APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
      })?;

    let thumbnail_url = self
      .thumbnail_url
      .map(|url_str| {
        Url::parse(&url_str).map_err(|err| {
          error!("invalid thumbnail URL in db: {url_str}: {err}");
          APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
        })
      })
      .transpose()?;

    Ok(Material {
      id: self.id,
      name: self.name,
      description: self.description,
      thumbnail_url,
      material_definition,
      owner_id: self.owner_id,
      owner_name: self.owner_name,
      created_at: self.created_at,
      is_shared: self.is_shared,
      tags,
    })
  }
}

async fn fetch_material(pool: &SqlitePool, material_id: i64) -> Result<Material, APIError> {
  let row = sqlx::query_as::<_, MaterialRow>(sqlx::AssertSqlSafe(format!(
    "{MATERIAL_SELECT} WHERE materials.id = ?"
  )))
  .bind(material_id)
  .fetch_one(pool)
  .await
  .map_err(|err| {
    error!("Failed to fetch material {material_id}: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
  })?;

  let tags = load_tags_one(pool, MATERIAL_TAGS, material_id).await?;
  row.into_material(tags)
}

pub async fn list_materials(
  State(pool): State<SqlitePool>,
  Extension(user_opt): Extension<Option<User>>,
) -> Result<Json<Vec<Material>>, APIError> {
  let user_id = user_opt.as_ref().map_or(-1, |u| u.id);

  let rows = sqlx::query_as::<_, MaterialRow>(sqlx::AssertSqlSafe(format!(
    "{MATERIAL_SELECT} WHERE materials.is_shared OR materials.owner_id = ? ORDER BY \
     materials.created_at DESC"
  )))
  .bind(user_id)
  .fetch_all(&pool)
  .await
  .map_err(|err| {
    error!("Error fetching materials: {err}");
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      "Failed to fetch materials",
    )
  })?;

  let ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
  let mut tags_by_id = load_tags(&pool, MATERIAL_TAGS, &ids).await?;
  let materials: Result<Vec<Material>, APIError> = rows
    .into_iter()
    .map(|row| {
      let tags = tags_by_id.remove(&row.id).unwrap_or_default();
      row.into_material(tags)
    })
    .collect();
  Ok(Json(materials?))
}

#[derive(Deserialize)]
pub struct GetMaterialQuery {
  admin_token: Option<String>,
}

pub async fn get_material(
  axum_extra::extract::Query(GetMaterialQuery { admin_token }): axum_extra::extract::Query<
    GetMaterialQuery,
  >,
  State(pool): State<SqlitePool>,
  Path(material_id): Path<i64>,
  Extension(user_opt): Extension<Option<User>>,
) -> Result<Json<Material>, APIError> {
  let is_admin = admin_token.as_deref() == Some(crate::server::settings().admin_token.as_str());
  let where_clause = if is_admin {
    // dummy condition so placeholder count is equal between both queries
    "WHERE materials.id = ? AND ? != 0"
  } else {
    "WHERE materials.id = ? AND (materials.is_shared OR materials.owner_id = ?)"
  };

  let row = sqlx::query_as::<_, MaterialRow>(sqlx::AssertSqlSafe(format!(
    "{MATERIAL_SELECT} {where_clause}"
  )))
  .bind(material_id)
  .bind(user_opt.as_ref().map_or(-1, |u| u.id))
  .fetch_one(&pool)
  .await
  .map_err(|_err| APIError::new(StatusCode::NOT_FOUND, "Material not found"))?;

  let tags = load_tags_one(&pool, MATERIAL_TAGS, material_id).await?;
  Ok(Json(row.into_material(tags)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMaterialBody {
  name: String,
  #[serde(default)]
  description: String,
  material_definition: JsonValue,
  is_shared: Option<bool>,
  #[serde(default)]
  tags: Vec<String>,
}

pub async fn create_material(
  State(pool): State<SqlitePool>,
  Extension(user): Extension<User>,
  Json(body): Json<CreateMaterialBody>,
) -> Result<Json<Material>, APIError> {
  let material_definition_str =
    serde_json::to_string(&body.material_definition).map_err(|err| {
      error!("Failed to serialize material definition: {err}");
      APIError::new(StatusCode::BAD_REQUEST, "Invalid material definition")
    })?;

  let mut tx = pool.begin().await.map_err(|err| {
    error!("Failed to begin transaction: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
  })?;

  let material_id = sqlx::query_scalar::<_, i64>(
    r#"
    INSERT INTO materials (name, description, material_definition, owner_id, is_shared)
    VALUES (?, ?, ?, ?, ?)
    RETURNING id
    "#,
  )
  .bind(&body.name)
  .bind(&body.description)
  .bind(&material_definition_str)
  .bind(user.id)
  .bind(body.is_shared.unwrap_or(false))
  .fetch_one(&mut *tx)
  .await
  .map_err(|err| {
    error!("Error creating material in db: {err}");
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      "Failed to create material",
    )
  })?;

  set_tags(&mut tx, MATERIAL_TAGS, material_id, &body.tags).await?;

  tx.commit().await.map_err(|err| {
    error!("Failed to commit transaction: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
  })?;

  render_material_thumbnail(pool.clone(), material_id, material_definition_str);

  Ok(Json(fetch_material(&pool, material_id).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMaterialBody {
  name: Option<String>,
  description: Option<String>,
  material_definition: Option<JsonValue>,
  is_shared: Option<bool>,
  tags: Option<Vec<String>>,
}

pub async fn update_material(
  State(pool): State<SqlitePool>,
  Path(material_id): Path<i64>,
  Extension(user): Extension<User>,
  Json(body): Json<UpdateMaterialBody>,
) -> Result<Json<Material>, APIError> {
  let mut tx = pool.begin().await.map_err(|err| {
    error!("Failed to begin transaction: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
  })?;

  let owner_id = sqlx::query_scalar::<_, i64>("SELECT owner_id FROM materials WHERE id = ?")
    .bind(material_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|err| {
      error!("Error fetching material: {err}");
      APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
    })?
    .ok_or_else(|| APIError::new(StatusCode::NOT_FOUND, "Material not found"))?;

  if owner_id != user.id {
    return Err(APIError::new(
      StatusCode::FORBIDDEN,
      "You do not have permission to update this material",
    ));
  }

  let material_definition_str = body
    .material_definition
    .as_ref()
    .map(|def| {
      serde_json::to_string(def).map_err(|err| {
        error!("Failed to serialize material definition: {err}");
        APIError::new(StatusCode::BAD_REQUEST, "Invalid material definition")
      })
    })
    .transpose()?;

  sqlx::query(
    r#"
    UPDATE materials
    SET name = COALESCE(?, name),
        description = COALESCE(?, description),
        material_definition = COALESCE(?, material_definition),
        is_shared = COALESCE(?, is_shared)
    WHERE id = ?
    "#,
  )
  .bind(body.name)
  .bind(body.description)
  .bind(&material_definition_str)
  .bind(body.is_shared)
  .bind(material_id)
  .execute(&mut *tx)
  .await
  .map_err(|err| {
    error!("Failed to update material: {err}");
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      "Failed to update material",
    )
  })?;

  if let Some(tags) = &body.tags {
    set_tags(&mut tx, MATERIAL_TAGS, material_id, tags).await?;
  }

  tx.commit().await.map_err(|err| {
    error!("Failed to commit transaction: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
  })?;

  if let Some(def) = material_definition_str {
    render_material_thumbnail(pool.clone(), material_id, def);
  }

  Ok(Json(fetch_material(&pool, material_id).await?))
}

pub async fn delete_material(
  State(pool): State<SqlitePool>,
  Path(material_id): Path<i64>,
  Extension(user): Extension<User>,
) -> Result<StatusCode, APIError> {
  let mut tx = pool.begin().await.map_err(|err| {
    error!("Failed to begin transaction: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
  })?;

  let material = sqlx::query("SELECT owner_id FROM materials WHERE id = ?")
    .bind(material_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|err| {
      error!("Error fetching material for deletion: {err}");
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Failed to delete material",
      )
    })?
    .ok_or_else(|| APIError::new(StatusCode::NOT_FOUND, "Material not found"))?;

  let owner_id: i64 = material.get("owner_id");

  if owner_id != user.id {
    return Err(APIError::new(
      StatusCode::FORBIDDEN,
      "You do not have permission to delete this material",
    ));
  }

  sqlx::query("DELETE FROM materials WHERE id = ?")
    .bind(material_id)
    .execute(&mut *tx)
    .await
    .map_err(|err| {
      error!("Error deleting material: {err}");
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Failed to delete material",
      )
    })?;

  tx.commit().await.map_err(|err| {
    error!("Failed to commit transaction: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
  })?;

  Ok(StatusCode::NO_CONTENT)
}
