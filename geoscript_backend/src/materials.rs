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

use crate::{auth::User, render_material_thumbnail::render_material_thumbnail, server::APIError};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Material {
  id: i64,
  name: String,
  thumbnail_url: Option<Url>,
  material_definition: JsonValue,
  owner_id: i64,
  owner_name: String,
  created_at: DateTime<Utc>,
  is_shared: bool,
}

#[derive(FromRow, Debug)]
pub struct MaterialRow {
  id: i64,
  name: String,
  thumbnail_url: Option<String>,
  material_definition: String,
  owner_id: i64,
  owner_name: String,
  created_at: DateTime<Utc>,
  is_shared: bool,
}

impl TryFrom<MaterialRow> for Material {
  type Error = APIError;

  fn try_from(row: MaterialRow) -> Result<Self, Self::Error> {
    let material_definition: JsonValue =
      serde_json::from_str(&row.material_definition).map_err(|err| {
        error!("invalid material definition JSON in db: {}: {err}", row.id);
        APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
      })?;

    let thumbnail_url = row
      .thumbnail_url
      .map(|url_str| {
        Url::parse(&url_str).map_err(|err| {
          error!("invalid thumbnail URL in db: {}: {err}", url_str);
          APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
        })
      })
      .transpose()?;

    Ok(Material {
      id: row.id,
      name: row.name,
      thumbnail_url,
      material_definition,
      owner_id: row.owner_id,
      owner_name: row.owner_name,
      created_at: row.created_at,
      is_shared: row.is_shared,
    })
  }
}

pub async fn list_materials(
  State(pool): State<SqlitePool>,
  Extension(user_opt): Extension<Option<User>>,
) -> Result<Json<Vec<Material>>, APIError> {
  let user_id = user_opt.as_ref().map_or(-1, |u| u.id);

  let materials = sqlx::query_as::<_, MaterialRow>(
    r#"
    SELECT materials.id, materials.name, materials.thumbnail_url, materials.material_definition, materials.owner_id, users.username as owner_name, materials.created_at, materials.is_shared
    FROM materials
    JOIN users ON materials.owner_id = users.id
    WHERE materials.is_shared OR materials.owner_id = ?
    ORDER BY materials.created_at DESC
    "#,
  )
  .bind(user_id)
  .fetch_all(&pool)
  .await
  .map_err(|err| {
    error!("Error fetching materials: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch materials")
  })?;

  let materials: Result<Vec<Material>, APIError> =
    materials.into_iter().map(|row| row.try_into()).collect();
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
  let query = if admin_token.as_ref().map(|t| t.as_str())
    == Some(crate::server::settings().admin_token.as_str())
  {
    r#"
    SELECT materials.id, materials.name, materials.thumbnail_url, materials.material_definition, materials.owner_id, users.username as owner_name, materials.created_at, materials.is_shared
    FROM materials
    JOIN users ON materials.owner_id = users.id
    WHERE materials.id = ?
    -- dummy condition so placeholder count is equal between both queries
    AND ? != 0
    "#
  } else {
    r#"
    SELECT materials.id, materials.name, materials.thumbnail_url, materials.material_definition, materials.owner_id, users.username as owner_name, materials.created_at, materials.is_shared
    FROM materials
    JOIN users ON materials.owner_id = users.id
    WHERE materials.id = ? AND (materials.is_shared OR materials.owner_id = ?)
    "#
  };

  let material_row = sqlx::query_as::<_, MaterialRow>(query)
    .bind(material_id)
    .bind(user_opt.as_ref().map_or(-1, |u| u.id))
    .fetch_one(&pool)
    .await
    .map_err(|_err| APIError::new(StatusCode::NOT_FOUND, "Material not found"))?;

  Ok(Json(material_row.try_into()?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMaterialBody {
  name: String,
  material_definition: JsonValue,
  is_shared: Option<bool>,
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

  let material_id = sqlx::query_scalar::<_, i64>(
    r#"
        INSERT INTO materials (name, material_definition, owner_id, is_shared)
        VALUES (?, ?, ?, ?)
        RETURNING id
        "#,
  )
  .bind(&body.name)
  .bind(&material_definition_str)
  .bind(user.id)
  .bind(body.is_shared.unwrap_or(false))
  .fetch_one(&pool)
  .await
  .map_err(|err| {
    error!("Error creating material in db: {err}");
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      "Failed to create material",
    )
  })?;

  render_material_thumbnail(pool.clone(), material_id, material_definition_str);

  let material_row = sqlx::query_as::<_, MaterialRow>(
    r#"
        SELECT materials.id, materials.name, materials.thumbnail_url, materials.material_definition, materials.owner_id, users.username as owner_name, materials.created_at, materials.is_shared
        FROM materials
        JOIN users ON materials.owner_id = users.id
        WHERE materials.id = ?
        "#,
  )
  .bind(material_id)
  .fetch_one(&pool)
  .await
  .map_err(|err| {
    error!("Failed to fetch newly created material: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
  })?;

  Ok(Json(material_row.try_into()?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMaterialBody {
  name: Option<String>,
  material_definition: Option<JsonValue>,
  is_shared: Option<bool>,
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

  let material_row = sqlx::query_as::<_, MaterialRow>(
    r#"
        SELECT materials.id, materials.name, materials.thumbnail_url, materials.material_definition, materials.owner_id, users.username as owner_name, materials.created_at, materials.is_shared
        FROM materials
        JOIN users ON materials.owner_id = users.id
        WHERE materials.id = ?
        "#,
  )
  .bind(material_id)
  .fetch_one(&mut *tx)
  .await
  .map_err(|_| APIError::new(StatusCode::NOT_FOUND, "Material not found"))?;

  if material_row.owner_id != user.id {
    return Err(APIError::new(
      StatusCode::FORBIDDEN,
      "You do not have permission to update this material",
    ));
  }

  let name = body.name.unwrap_or(material_row.name);
  let is_shared = body.is_shared.unwrap_or(material_row.is_shared);
  let material_definition_updated = body.material_definition.is_some();
  let material_definition_str = if let Some(def) = &body.material_definition {
    serde_json::to_string(def).map_err(|err| {
      error!("Failed to serialize material definition: {err}");
      APIError::new(StatusCode::BAD_REQUEST, "Invalid material definition")
    })?
  } else {
    material_row.material_definition
  };

  sqlx::query(
    r#"
        UPDATE materials
        SET name = ?, material_definition = ?, is_shared = ?
        WHERE id = ?
        "#,
  )
  .bind(name)
  .bind(&material_definition_str)
  .bind(is_shared)
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

  tx.commit().await.map_err(|err| {
    error!("Failed to commit transaction: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
  })?;

  if material_definition_updated {
    render_material_thumbnail(pool.clone(), material_id, material_definition_str);
  }

  let updated_material_row = sqlx::query_as::<_, MaterialRow>(
    r#"
        SELECT materials.id, materials.name, materials.thumbnail_url, materials.material_definition, materials.owner_id, users.username as owner_name, materials.created_at, materials.is_shared
        FROM materials
        JOIN users ON materials.owner_id = users.id
        WHERE materials.id = ?
        "#,
  )
  .bind(material_id)
  .fetch_one(&pool)
  .await
  .map_err(|err| {
    error!("Failed to fetch updated material: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
  })?;

  Ok(Json(updated_material_row.try_into()?))
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
