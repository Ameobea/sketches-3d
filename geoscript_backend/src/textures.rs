use axum::{
  extract::{Path, Query, State},
  Extension, Json,
};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use tracing::error;
use url::Url;
use uuid::Uuid;

use crate::{auth::User, server::APIError};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Texture {
  id: i64,
  name: String,
  thumbnail_url: Url,
  url: Url,
  owner_id: i64,
  owner_name: String,
  created_at: DateTime<Utc>,
  is_shared: bool,
}

#[derive(FromRow, Debug)]
pub struct TextureRow {
  id: i64,
  name: String,
  thumbnail_url: String,
  url: String,
  owner_id: i64,
  owner_name: String,
  created_at: DateTime<Utc>,
  is_shared: bool,
}

impl TryFrom<TextureRow> for Texture {
  type Error = APIError;

  fn try_from(row: TextureRow) -> Result<Self, Self::Error> {
    Ok(Texture {
      id: row.id,
      name: row.name,
      thumbnail_url: Url::parse(&row.thumbnail_url).map_err(|err| {
        error!("invalid thumbnail URL in db: {}: {err}", row.thumbnail_url);
        APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
      })?,
      url: Url::parse(&row.url).map_err(|err| {
        error!("invalid URL in db: {}: {err}", row.url);
        APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
      })?,
      owner_id: row.owner_id,
      owner_name: row.owner_name,
      created_at: row.created_at,
      is_shared: row.is_shared,
    })
  }
}

pub async fn list_textures(
  State(pool): State<SqlitePool>,
  Extension(user_opt): Extension<Option<User>>,
) -> Result<Json<Vec<Texture>>, APIError> {
  let user_id = user_opt.as_ref().map_or(-1, |u| u.id);

  let textures = sqlx::query_as::<_, TextureRow>(
    r#"
    SELECT textures.id, textures.name, textures.thumbnail_url, textures.url, textures.owner_id, users.username as owner_name, textures.created_at, textures.is_shared
    FROM textures
    JOIN users ON textures.owner_id = users.id
    WHERE textures.is_shared OR textures.owner_id = ?
    ORDER BY textures.created_at DESC
    "#,
  )
  .bind(user_id)
  .fetch_all(&pool)
  .await
  .map_err(|err| {
    error!("Error fetching textures: {err}");
    APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch textures")
  })?;

  let textures: Result<Vec<Texture>, APIError> =
    textures.into_iter().map(|row| row.try_into()).collect();
  Ok(Json(textures?))
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

  let texture_row = sqlx::query_as::<_, TextureRow>(
    r#"
    SELECT textures.id, textures.name, textures.thumbnail_url, textures.url, textures.owner_id, users.username as owner_name, textures.created_at, textures.is_shared
    FROM textures
    JOIN users ON textures.owner_id = users.id
    WHERE textures.id = ?
    "#,
  )
  .bind(texture_id)
  .fetch_one(&pool)
  .await
  .map_err(|err| match err {
    sqlx::Error::RowNotFound => not_found(),
    _ => {
      error!("Error fetching texture: {err}");
      APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch texture")
    }
  })?;

  if texture_row.is_shared {
    return Ok(Json(texture_row.try_into()?));
  }

  let Some(user) = user_opt else {
    return Err(not_found());
  };
  if user.id != texture_row.owner_id {
    return Err(not_found());
  }

  Ok(Json(texture_row.try_into()?))
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
  let query = if admin_token.as_ref().map(|t| t.as_str())
    == Some(crate::server::settings().admin_token.as_str())
  {
    format!(
      r#"
    SELECT textures.id, textures.name, textures.thumbnail_url, textures.url, textures.owner_id, users.username as owner_name, textures.created_at, textures.is_shared
    FROM textures
    JOIN users ON textures.owner_id = users.id
    WHERE textures.id IN ({ids_str})
    "#
    )
  } else {
    format!(
      r#"
    SELECT textures.id, textures.name, textures.thumbnail_url, textures.url, textures.owner_id, users.username as owner_name, textures.created_at, textures.is_shared
    FROM textures
    JOIN users ON textures.owner_id = users.id
    WHERE textures.id IN ({ids_str}) AND (textures.is_shared OR textures.owner_id = ?)
    "#
    )
  };
  let textures = sqlx::query_as::<_, TextureRow>(&query)
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

  let textures: Result<Vec<Texture>, APIError> =
    textures.into_iter().map(|row| row.try_into()).collect();
  Ok(Json(textures?))
}

async fn create_texture_inner(
  pool: SqlitePool,
  client: &reqwest::Client,
  user: User,
  name: &str,
  texture_data: Bytes,
  is_shared: Option<bool>,
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

  let texture_id = sqlx::query_scalar::<_, i64>(
    r#"
    INSERT INTO textures (name, thumbnail_url, url, owner_id, is_shared, source_url)
    VALUES (?, ?, ?, ?, ?, ?)
    RETURNING id
    "#,
  )
  .bind(name)
  .bind(&thumbnail_url)
  .bind(&texture_url)
  .bind(user.id)
  .bind(is_shared.unwrap_or(false))
  .bind(source_url)
  .fetch_one(&pool)
  .await
  .map_err(|err| {
    error!("Error creating texture in db: {err}");
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      "Failed to create texture",
    )
  })?;

  get_texture(State(pool), Path(texture_id), Extension(Some(user))).await
}

#[derive(Deserialize)]
pub struct CreateTextureQuery {
  name: String,
  is_shared: Option<bool>,
}

pub async fn create_texture(
  State(pool): State<SqlitePool>,
  Query(CreateTextureQuery { name, is_shared }): Query<CreateTextureQuery>,
  Extension(user): Extension<User>,
  body: Bytes,
) -> Result<Json<Texture>, APIError> {
  let client = reqwest::Client::new();

  create_texture_inner(pool, &client, user, &name, body, is_shared, None).await
}

#[derive(Deserialize)]
pub struct CreateTextureFromURLBody {
  url: String,
}

pub async fn create_texture_from_url(
  State(pool): State<SqlitePool>,
  Query(CreateTextureQuery { name, is_shared }): Query<CreateTextureQuery>,
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

  create_texture_inner(
    pool,
    &client,
    user,
    &name,
    texture_data,
    is_shared,
    Some(url),
  )
  .await
}
