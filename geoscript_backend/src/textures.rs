use axum::{
  Extension, Json,
  extract::{Path, State},
};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use tracing::error;
use url::Url;
use uuid::Uuid;

use crate::{
  auth::User,
  server::APIError,
  tags::{TEXTURE_TAGS, load_tags, load_tags_one, set_tags},
};

const TEXTURE_SELECT: &str =
  "SELECT textures.id, textures.name, textures.description, textures.thumbnail_url, textures.url, \
   textures.source_url, textures.owner_id, users.username as owner_name, textures.created_at, \
   textures.is_shared FROM textures JOIN users ON textures.owner_id = users.id";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Texture {
  id: i64,
  name: String,
  description: String,
  thumbnail_url: Url,
  url: Url,
  source_url: Option<String>,
  owner_id: i64,
  owner_name: String,
  created_at: DateTime<Utc>,
  is_shared: bool,
  tags: Vec<String>,
}

#[derive(FromRow, Debug)]
pub struct TextureRow {
  id: i64,
  name: String,
  description: String,
  thumbnail_url: String,
  url: String,
  source_url: Option<String>,
  owner_id: i64,
  owner_name: String,
  created_at: DateTime<Utc>,
  is_shared: bool,
}

impl TextureRow {
  fn into_texture(self, tags: Vec<String>) -> Result<Texture, APIError> {
    let parse = |url: &str| {
      Url::parse(url).map_err(|err| {
        error!("invalid URL in db: {url}: {err}");
        APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
      })
    };

    Ok(Texture {
      id: self.id,
      name: self.name,
      description: self.description,
      thumbnail_url: parse(&self.thumbnail_url)?,
      url: parse(&self.url)?,
      source_url: self.source_url,
      owner_id: self.owner_id,
      owner_name: self.owner_name,
      created_at: self.created_at,
      is_shared: self.is_shared,
      tags,
    })
  }
}

async fn attach_tags(pool: &SqlitePool, rows: Vec<TextureRow>) -> Result<Vec<Texture>, APIError> {
  let ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
  let mut tags_by_id = load_tags(pool, TEXTURE_TAGS, &ids).await?;
  rows
    .into_iter()
    .map(|row| {
      let tags = tags_by_id.remove(&row.id).unwrap_or_default();
      row.into_texture(tags)
    })
    .collect()
}

pub async fn list_textures(
  State(pool): State<SqlitePool>,
  Extension(user_opt): Extension<Option<User>>,
) -> Result<Json<Vec<Texture>>, APIError> {
  let user_id = user_opt.as_ref().map_or(-1, |u| u.id);

  let rows = sqlx::query_as::<_, TextureRow>(sqlx::AssertSqlSafe(format!(
    "{TEXTURE_SELECT} WHERE textures.is_shared OR textures.owner_id = ? ORDER BY \
     textures.created_at DESC"
  )))
  .bind(user_id)
  .fetch_all(&pool)
  .await
  .map_err(|err| {
    error!("Error fetching textures: {err}");
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      "Failed to fetch textures",
    )
  })?;

  Ok(Json(attach_tags(&pool, rows).await?))
}

pub async fn get_texture(
  State(pool): State<SqlitePool>,
  Path(texture_id): Path<i64>,
  Extension(user_opt): Extension<Option<User>>,
) -> Result<Json<Texture>, APIError> {
  fn not_found() -> APIError {
    APIError::new(
      StatusCode::NOT_FOUND,
      "Texture not found or no permission to view it",
    )
  }

  let texture_row = sqlx::query_as::<_, TextureRow>(sqlx::AssertSqlSafe(format!(
    "{TEXTURE_SELECT} WHERE textures.id = ?"
  )))
  .bind(texture_id)
  .fetch_one(&pool)
  .await
  .map_err(|err| match err {
    sqlx::Error::RowNotFound => not_found(),
    _ => {
      error!("Error fetching texture: {err}");
      APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch texture")
    },
  })?;

  if !texture_row.is_shared && user_opt.map_or(true, |user| user.id != texture_row.owner_id) {
    return Err(not_found());
  }

  let tags = load_tags_one(&pool, TEXTURE_TAGS, texture_id).await?;
  Ok(Json(texture_row.into_texture(tags)?))
}

#[derive(Deserialize)]
pub struct GetMultipleTexturesQuery {
  id: Vec<i64>,
  admin_token: Option<String>,
}

pub async fn get_multiple_textures(
  axum_extra::extract::Query(GetMultipleTexturesQuery {
    id: ids,
    admin_token,
  }): axum_extra::extract::Query<GetMultipleTexturesQuery>,
  State(pool): State<SqlitePool>,
  Extension(user_opt): Extension<Option<User>>,
) -> Result<Json<Vec<Texture>>, APIError> {
  let user_id = user_opt.as_ref().map_or(-1, |u| u.id);

  let ids_str = ids
    .iter()
    .map(|id| id.to_string())
    .collect::<Vec<_>>()
    .join(", ");
  let is_admin = admin_token.as_deref() == Some(crate::server::settings().admin_token.as_str());
  let query = if is_admin {
    format!("{TEXTURE_SELECT} WHERE textures.id IN ({ids_str})")
  } else {
    format!(
      "{TEXTURE_SELECT} WHERE textures.id IN ({ids_str}) AND (textures.is_shared OR \
       textures.owner_id = ?)"
    )
  };

  let rows = sqlx::query_as::<_, TextureRow>(sqlx::AssertSqlSafe(query))
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .map_err(|err| {
      error!("Error fetching textures: {err}");
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Failed to fetch textures",
      )
    })?;

  Ok(Json(attach_tags(&pool, rows).await?))
}

async fn create_texture_inner(
  pool: SqlitePool,
  client: &reqwest::Client,
  user: User,
  meta: CreateTextureQuery,
  texture_data: Bytes,
  source_url: Option<String>,
) -> Result<Json<Texture>, APIError> {
  let client_clone = client.clone();
  let body_clone = texture_data.clone();
  let (thumbnail_res, avif_res) = tokio::try_join!(
    async move {
      let res = client_clone
        .post("http://localhost:5812/thumbnail")
        .header("Content-Type", "image/png")
        .body(body_clone)
        .send()
        .await
        .map_err(|err| {
          error!("Error generating thumbnail: {err}");
          APIError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to generate thumbnail",
          )
        })?;
      let status = res.status();
      if !status.is_success() {
        let error_body = res
          .text()
          .await
          .unwrap_or_else(|_| "<failed to get body>".to_string());
        error!("Error generating thumbnail: {status}; {error_body}");
        return Err(APIError::new(
          StatusCode::INTERNAL_SERVER_ERROR,
          "Failed to generate thumbnail",
        ));
      }
      let bytes = res.bytes().await.map_err(|err| {
        error!("Failed to read thumbnail response: {err}");
        APIError::new(
          StatusCode::INTERNAL_SERVER_ERROR,
          "Failed to read thumbnail response",
        )
      })?;
      Ok(bytes)
    },
    async move {
      let res = client
        .post("http://localhost:5812/convert-to-avif")
        .header("Content-Type", "image/png")
        .body(texture_data)
        .send()
        .await
        .map_err(|err| {
          error!("Error generating AVIF: {err}");
          APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Failed to generate AVIF")
        })?;
      let status = res.status();
      if !status.is_success() {
        let error_body = res
          .text()
          .await
          .unwrap_or_else(|_| "<failed to get body>".to_string());
        error!("Error generating AVIF: {status}; {error_body}");
        return Err(APIError::new(
          StatusCode::INTERNAL_SERVER_ERROR,
          "Failed to generate AVIF",
        ));
      }
      let bytes = res.bytes().await.map_err(|err| {
        error!("Failed to read AVIF response: {err}");
        APIError::new(
          StatusCode::INTERNAL_SERVER_ERROR,
          "Failed to read AVIF response",
        )
      })?;
      Ok(bytes)
    }
  )?;

  let thumbnail_bytes = thumbnail_res;
  let body = avif_res;

  let texture_uuid = Uuid::new_v4().to_string();
  let texture_path = format!("textures/{}/{}.avif", user.id, texture_uuid);
  let thumbnail_path = format!("textures/{}/{}/thumbnail.avif", user.id, texture_uuid);

  let (texture_url_res, thumbnail_url_res) = tokio::join!(
    crate::object_storage::upload_file(&texture_path, body),
    crate::object_storage::upload_file(&thumbnail_path, thumbnail_bytes.into())
  );

  let texture_url = texture_url_res?;
  let thumbnail_url = thumbnail_url_res?;

  let mut tx = pool.begin().await.map_err(|err| {
    error!("Failed to begin transaction: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
  })?;

  let texture_id = sqlx::query_scalar::<_, i64>(
    r#"
    INSERT INTO textures (name, description, thumbnail_url, url, owner_id, is_shared, source_url)
    VALUES (?, ?, ?, ?, ?, ?, ?)
    RETURNING id
    "#,
  )
  .bind(&meta.name)
  .bind(meta.description.unwrap_or_default())
  .bind(&thumbnail_url)
  .bind(&texture_url)
  .bind(user.id)
  .bind(meta.is_shared.unwrap_or(false))
  .bind(source_url)
  .fetch_one(&mut *tx)
  .await
  .map_err(|err| {
    error!("Error creating texture in db: {err}");
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      "Failed to create texture",
    )
  })?;

  set_tags(&mut tx, TEXTURE_TAGS, texture_id, &meta.tag).await?;

  tx.commit().await.map_err(|err| {
    error!("Failed to commit transaction: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
  })?;

  get_texture(State(pool), Path(texture_id), Extension(Some(user))).await
}

#[derive(Deserialize)]
pub struct CreateTextureQuery {
  name: String,
  description: Option<String>,
  is_shared: Option<bool>,
  #[serde(default)]
  tag: Vec<String>,
}

pub async fn create_texture(
  State(pool): State<SqlitePool>,
  axum_extra::extract::Query(meta): axum_extra::extract::Query<CreateTextureQuery>,
  Extension(user): Extension<User>,
  body: Bytes,
) -> Result<Json<Texture>, APIError> {
  let client = reqwest::Client::new();

  create_texture_inner(pool, &client, user, meta, body, None).await
}

#[derive(Deserialize)]
pub struct CreateTextureFromURLBody {
  url: String,
}

pub async fn create_texture_from_url(
  State(pool): State<SqlitePool>,
  axum_extra::extract::Query(meta): axum_extra::extract::Query<CreateTextureQuery>,
  Extension(user): Extension<User>,
  Json(CreateTextureFromURLBody { url }): Json<CreateTextureFromURLBody>,
) -> Result<Json<Texture>, APIError> {
  let client = reqwest::Client::new();

  let res = client
    .get(&url)
    .timeout(std::time::Duration::from_secs(30))
    .send()
    .await
    .map_err(|err| {
      error!("Error fetching texture from URL: {err}");
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Failed to fetch texture from URL: {err}"),
      )
    })?;

  let status = res.status();
  if !status.is_success() {
    let error_body = res
      .text()
      .await
      .unwrap_or_else(|err| format!("Error reading response body: {err}"));
    error!("Error fetching texture from URL: {status}; {error_body}");
    return Err(APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to fetch texture from URL: {status}; {error_body}"),
    ));
  }
  let texture_data = res.bytes().await.map_err(|err| {
    error!("Failed to read texture response: {err}");
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to read texture response: {err}"),
    )
  })?;

  create_texture_inner(pool, &client, user, meta, texture_data, Some(url)).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTextureBody {
  name: Option<String>,
  description: Option<String>,
  is_shared: Option<bool>,
  tags: Option<Vec<String>>,
}

async fn owned_texture(
  conn: &mut sqlx::SqliteConnection,
  texture_id: i64,
  user: &User,
) -> Result<(), APIError> {
  let owner_id = sqlx::query_scalar::<_, i64>("SELECT owner_id FROM textures WHERE id = ?")
    .bind(texture_id)
    .fetch_optional(conn)
    .await
    .map_err(|err| {
      error!("Error fetching texture: {err}");
      APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
    })?
    .ok_or_else(|| APIError::new(StatusCode::NOT_FOUND, "Texture not found"))?;

  if owner_id != user.id {
    return Err(APIError::new(
      StatusCode::FORBIDDEN,
      "You do not have permission to modify this texture",
    ));
  }
  Ok(())
}

pub async fn update_texture(
  State(pool): State<SqlitePool>,
  Path(texture_id): Path<i64>,
  Extension(user): Extension<User>,
  Json(body): Json<UpdateTextureBody>,
) -> Result<Json<Texture>, APIError> {
  let mut tx = pool.begin().await.map_err(|err| {
    error!("Failed to begin transaction: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
  })?;

  owned_texture(&mut tx, texture_id, &user).await?;

  sqlx::query(
    r#"
    UPDATE textures
    SET name = COALESCE(?, name),
        description = COALESCE(?, description),
        is_shared = COALESCE(?, is_shared)
    WHERE id = ?
    "#,
  )
  .bind(body.name)
  .bind(body.description)
  .bind(body.is_shared)
  .bind(texture_id)
  .execute(&mut *tx)
  .await
  .map_err(|err| {
    error!("Failed to update texture: {err}");
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      "Failed to update texture",
    )
  })?;

  if let Some(tags) = &body.tags {
    set_tags(&mut tx, TEXTURE_TAGS, texture_id, tags).await?;
  }

  tx.commit().await.map_err(|err| {
    error!("Failed to commit transaction: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
  })?;

  get_texture(State(pool), Path(texture_id), Extension(Some(user))).await
}

pub async fn delete_texture(
  State(pool): State<SqlitePool>,
  Path(texture_id): Path<i64>,
  Extension(user): Extension<User>,
) -> Result<StatusCode, APIError> {
  let mut tx = pool.begin().await.map_err(|err| {
    error!("Failed to begin transaction: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
  })?;

  owned_texture(&mut tx, texture_id, &user).await?;

  sqlx::query("DELETE FROM textures WHERE id = ?")
    .bind(texture_id)
    .execute(&mut *tx)
    .await
    .map_err(|err| {
      error!("Error deleting texture: {err}");
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Failed to delete texture",
      )
    })?;

  tx.commit().await.map_err(|err| {
    error!("Failed to commit transaction: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
  })?;

  Ok(StatusCode::NO_CONTENT)
}
