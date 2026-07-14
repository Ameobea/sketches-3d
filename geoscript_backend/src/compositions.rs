use std::collections::HashMap;

use crate::{
  auth::User,
  render_thumbnail::render_thumbnail,
  tags::{COMPOSITION_TAGS, load_tags, load_tags_one, set_tags},
};
use axum::{
  Json,
  extract::{Extension, Path, Query, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Arguments, FromRow, Row, SqlitePool, sqlite::SqliteArguments};

use crate::server::APIError;
use axum::http::StatusCode;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transform3 {
  pub pos: [f64; 3],
  pub rot: [f64; 3],
  pub scale: [f64; 3],
}

/// 8 lowercase hex chars, matching the `lower(hex(randomblob(4)))` instance-id migration.
fn gen_instance_id() -> String {
  let mut b = [0u8; 4];
  getrandom::fill(&mut b).expect("getrandom failed");
  format!("{:02x}{:02x}{:02x}{:02x}", b[0], b[1], b[2], b[3])
}

/// A node placement: a `Transform3` plus a short id the editor uses to address gizmo
/// targets and undo by identity rather than array index (uniqueness is per node). `id`
/// is defaulted so trees from before the id migration deserialize and self-heal on load.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Instance {
  #[serde(flatten)]
  pub transform: Transform3,
  #[serde(default = "gen_instance_id")]
  pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeDef {
  pub id: String,
  pub name: String,
  pub source: String,
  /// Per-node placements; length >= 1. The single-copy case is `instances.len() == 1`.
  pub instances: Vec<Instance>,
  /// Opaque gizmo-value passthrough (populated by the editor; stored verbatim here).
  #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
  pub handles: serde_json::Map<String, serde_json::Value>,
  pub children: Vec<String>,
  #[serde(default, skip_serializing_if = "std::ops::Not::not")]
  pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TreeDef {
  pub version: u32,
  /// Id of the always-present `_root` compositor node.
  #[serde(rename = "rootId")]
  pub root_id: String,
  #[serde(rename = "globalsSource")]
  pub globals_source: String,
  pub nodes: HashMap<String, NodeDef>,
}

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
  /// Lives in the `composition_tags` join table rather than on the row; populated after the fetch.
  #[sqlx(skip)]
  pub tags: Vec<String>,
}

#[derive(Serialize)]
pub struct CompositionAndVersion {
  pub composition: Composition,
  pub version: CompositionVersion,
}

#[derive(Debug, FromRow, Serialize)]
pub struct CompositionVersion {
  pub id: i64,
  pub composition_id: i64,
  pub tree: sqlx::types::Json<TreeDef>,
  pub created_at: DateTime<Utc>,
  pub thumbnail_url: Option<String>,
  pub metadata: sqlx::types::Json<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
pub struct CreateComposition {
  pub title: String,
  pub description: String,
  pub tree: sqlx::types::Json<TreeDef>,
  pub is_shared: bool,
  pub metadata: sqlx::types::Json<serde_json::Map<String, serde_json::Value>>,
  #[serde(default)]
  pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct CreateCompositionVersion {
  pub tree: sqlx::types::Json<TreeDef>,
  pub metadata: sqlx::types::Json<serde_json::Map<String, serde_json::Value>>,
}

impl<'a> From<&'a CompositionVersion> for CreateCompositionVersion {
  fn from(other: &'a CompositionVersion) -> Self {
    Self {
      tree: other.tree.clone(),
      metadata: other.metadata.clone(),
    }
  }
}

#[derive(Debug, Deserialize)]
pub struct UpdateCompositionPatch {
  pub title: Option<String>,
  pub description: Option<String>,
  pub is_shared: Option<bool>,
  pub tags: Option<Vec<String>>,
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
    "INSERT INTO compositions (author_id, title, description, is_shared) VALUES (?, ?, ?, ?) \
     RETURNING id",
  )
  .bind(user.id)
  .bind(&payload.title)
  .bind(&payload.description)
  .bind(payload.is_shared as i32)
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

  let version_id = sqlx::query(
    "INSERT INTO composition_versions (composition_id, tree, metadata) VALUES (?, ?, ?) RETURNING \
     id",
  )
  .bind(id)
  .bind(&payload.tree)
  .bind(&payload.metadata)
  .fetch_one(&mut *tx)
  .await
  .map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to insert composition version: {err}"),
    )
  })?
  .try_get("id")
  .map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to get inserted composition version id: {err}"),
    )
  })?;

  set_tags(&mut tx, COMPOSITION_TAGS, id, &payload.tags).await?;

  tx.commit().await.map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to commit transaction: {err}"),
    )
  })?;

  render_thumbnail(pool.clone(), id, version_id);

  get_composition(
    State(pool),
    Path(id),
    Extension(Some(user)),
    Query(AdminTokenQuery { admin_token: None }),
  )
  .await
}

pub async fn create_composition_version(
  State(pool): State<SqlitePool>,
  Extension(user): Extension<User>,
  Path(composition_id): Path<i64>,
  Json(payload): Json<CreateCompositionVersion>,
) -> Result<Json<CompositionVersion>, APIError> {
  let _comp_id: i64 =
    sqlx::query_scalar("SELECT id FROM compositions WHERE id = ? AND author_id = ?")
      .bind(composition_id)
      .bind(user.id)
      .fetch_one(&pool)
      .await
      .map_err(|err| match err {
        sqlx::Error::RowNotFound => APIError::new(
          StatusCode::NOT_FOUND,
          "Composition not found or you do not have permission to modify it",
        ),
        _ => APIError::new(
          StatusCode::INTERNAL_SERVER_ERROR,
          format!("Failed to check composition ownership: {err}"),
        ),
      })?;

  let latest_version = sqlx::query_as::<_, CompositionVersion>(
    "SELECT * FROM composition_versions WHERE composition_id = ? ORDER BY created_at DESC LIMIT 1",
  )
  .bind(composition_id)
  .fetch_optional(&pool)
  .await
  .map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to fetch latest composition version: {err}"),
    )
  })?;

  if let Some(latest) = latest_version {
    if CreateCompositionVersion::from(&latest) == payload {
      // avoid creating a new version if nothing has changed
      return Ok(Json(latest));
    }
  }

  let version = sqlx::query_as::<_, CompositionVersion>(
    "INSERT INTO composition_versions (composition_id, tree, metadata) VALUES (?, ?, ?) RETURNING \
     *",
  )
  .bind(composition_id)
  .bind(&payload.tree)
  .bind(&payload.metadata)
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

  render_thumbnail(pool, composition_id, version.id);

  Ok(Json(version))
}

pub async fn fork_composition(
  State(pool): State<SqlitePool>,
  Extension(user): Extension<User>,
  Path(composition_id): Path<i64>,
) -> Result<Json<CompositionAndVersion>, APIError> {
  let original_composition = sqlx::query_as::<_, Composition>(
    "SELECT c.*, u.username as author_username FROM compositions c JOIN users u ON c.author_id = \
     u.id WHERE c.id = ?",
  )
  .bind(composition_id)
  .fetch_one(&pool)
  .await
  .map_err(|err| match err {
    sqlx::Error::RowNotFound => APIError::new(
      StatusCode::NOT_FOUND,
      "Original composition not found or you do not have permission to access it",
    ),
    _ => APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to fetch original composition: {err}"),
    ),
  })?;

  if original_composition.author_id != user.id && !original_composition.is_shared {
    return Err(APIError::new(
      StatusCode::NOT_FOUND,
      "Original composition not found or you do not have permission to access it",
    ));
  }

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

  let version_id = sqlx::query(
    "INSERT INTO composition_versions (composition_id, tree, metadata) VALUES (?, ?, ?) RETURNING \
     id",
  )
  .bind(forked_composition_id)
  .bind(&latest_version.tree)
  .bind(&latest_version.metadata)
  .fetch_one(&mut *tx)
  .await
  .map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to insert forked composition version: {err}"),
    )
  })?
  .try_get::<i64, _>("id")
  .map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to get forked composition version id: {err}"),
    )
  })?;

  let mut forked_composition = sqlx::query_as::<_, Composition>(
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

  let original_tags = load_tags_one(&mut *tx, COMPOSITION_TAGS, composition_id).await?;
  forked_composition.tags = set_tags(
    &mut tx,
    COMPOSITION_TAGS,
    forked_composition_id,
    &original_tags,
  )
  .await?;

  tx.commit().await.map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to commit transaction: {err}"),
    )
  })?;

  render_thumbnail(pool, forked_composition_id, version_id);

  Ok(Json(CompositionAndVersion {
    composition: forked_composition,
    version: CompositionVersion {
      id: version_id,
      composition_id: forked_composition_id,
      tree: latest_version.tree,
      created_at: Utc::now(),
      thumbnail_url: None,
      metadata: latest_version.metadata,
    },
  }))
}

pub async fn get_composition(
  State(pool): State<SqlitePool>,
  Path(composition_id): Path<i64>,
  Extension(user_opt): Extension<Option<User>>,
  Query(AdminTokenQuery { admin_token }): Query<AdminTokenQuery>,
) -> Result<Json<Composition>, APIError> {
  fn not_found() -> APIError {
    APIError::new(
      StatusCode::NOT_FOUND,
      "Composition not found or you do not have permission to access it",
    )
  }

  let is_admin = if let Some(token) = admin_token {
    token == crate::server::settings().admin_token
  } else {
    false
  };

  let mut composition = sqlx::query_as::<_, Composition>(
    "SELECT c.*, u.username as author_username FROM compositions c JOIN users u ON c.author_id = \
     u.id WHERE c.id = ?",
  )
  .bind(composition_id)
  .fetch_one(&pool)
  .await
  .map_err(|_| not_found())?;

  let visible = composition.is_shared
    || is_admin
    || user_opt.is_some_and(|user| user.id == composition.author_id);
  if !visible {
    return Err(not_found());
  }

  composition.tags = load_tags_one(&pool, COMPOSITION_TAGS, composition_id).await?;
  Ok(Json(composition))
}

pub async fn get_composition_history(
  State(pool): State<SqlitePool>,
  Path(composition_id): Path<i64>,
  Extension(user_opt): Extension<Option<User>>,
  Query(AdminTokenQuery { admin_token }): Query<AdminTokenQuery>,
) -> Result<Json<Vec<CompositionVersion>>, APIError> {
  // this makes sure that the user has access to the composition
  let _composition = get_composition(
    State(pool.clone()),
    Path(composition_id),
    Extension(user_opt),
    Query(AdminTokenQuery { admin_token }),
  )
  .await?
  .0;

  let versions = sqlx::query_as::<_, CompositionVersion>(
    "SELECT * FROM composition_versions WHERE composition_id = ? ORDER BY created_at DESC",
  )
  .bind(composition_id)
  .fetch_all(&pool)
  .await
  .map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to fetch composition versions: {err}"),
    )
  })?;

  Ok(Json(versions))
}

pub async fn list_composition_versions(
  State(pool): State<SqlitePool>,
  Path(composition_id): Path<i64>,
  Extension(user_opt): Extension<Option<User>>,
  Query(AdminTokenQuery { admin_token }): Query<AdminTokenQuery>,
) -> Result<Json<Vec<i64>>, APIError> {
  // makes sure that the user has access to the composition
  let _ = get_composition(
    State(pool.clone()),
    Path(composition_id),
    Extension(user_opt),
    Query(AdminTokenQuery { admin_token }),
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

#[derive(Deserialize)]
pub struct AdminTokenQuery {
  admin_token: Option<String>,
}

pub async fn get_composition_latest(
  State(pool): State<SqlitePool>,
  Path(composition_id): Path<i64>,
  Extension(user_opt): Extension<Option<User>>,
  Query(AdminTokenQuery { admin_token }): Query<AdminTokenQuery>,
) -> Result<Json<CompositionVersion>, APIError> {
  // makes sure that the user has access to the composition
  let _ = get_composition(
    State(pool.clone()),
    Path(composition_id),
    Extension(user_opt),
    Query(AdminTokenQuery { admin_token }),
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
  Query(AdminTokenQuery { admin_token }): Query<AdminTokenQuery>,
) -> Result<Json<CompositionVersion>, APIError> {
  // makes sure that the user has access to the composition
  let _ = get_composition(
    State(pool.clone()),
    Path(composition_id),
    Extension(user_opt),
    Query(AdminTokenQuery { admin_token }),
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

async fn list_compositions(
  pool: &SqlitePool,
  author_id: Option<i64>,
  is_shared: Option<bool>,
  is_featured: Option<bool>,
  limit: Option<i64>,
  offset: Option<usize>,
  include_code: bool,
) -> Result<Vec<PublicComposition>, APIError> {
  #[derive(Debug, FromRow)]
  struct PublicCompositionRow {
    #[sqlx(flatten)]
    comp: Composition,
    latest_id: i64,
    latest_composition_id: i64,
    latest_tree: sqlx::types::Json<TreeDef>,
    latest_created_at: DateTime<Utc>,
    latest_thumbnail_url: Option<String>,
    latest_metadata: sqlx::types::Json<serde_json::Map<String, serde_json::Value>>,
  }

  let mut where_conditions = Vec::new();
  if let Some(author_id) = author_id {
    where_conditions.push(format!("c.author_id = {author_id}"));
  }
  if let Some(is_shared) = is_shared {
    where_conditions.push(format!("c.is_shared = {}", is_shared as i32));
  }
  if is_featured.unwrap_or(false) {
    where_conditions.push("c.is_featured = 1".to_string());
  }

  let where_clause = if where_conditions.is_empty() {
    "".to_string()
  } else {
    format!("WHERE {}", where_conditions.join(" AND "))
  };

  let query_str = format!(
    "
SELECT
  c.*,
  u.username as author_username,
  cv.id as latest_id,
  cv.composition_id as latest_composition_id,
  {},
  cv.created_at as latest_created_at,
  cv.thumbnail_url as latest_thumbnail_url,
  cv.metadata as latest_metadata
FROM compositions c
JOIN users u ON c.author_id = u.id
JOIN (
  SELECT
    *,
    ROW_NUMBER() OVER(PARTITION BY composition_id ORDER BY created_at DESC) as rn
  FROM composition_versions
) cv ON c.id = cv.composition_id AND cv.rn = 1
{where_clause}
ORDER BY c.updated_at DESC
{}",
    if include_code {
      "cv.tree as latest_tree"
    } else {
      // Stub TreeDef containing only an empty `_root`. Used as a placeholder when the
      // caller didn't ask for the actual code.
      "'{\"version\":1,\"rootId\":\"_\",\"globalsSource\":\"\",\"nodes\":{\"_\":{\"id\":\"_\",\"\
       name\":\"_root\",\"source\":\"\",\"instances\":[{\"pos\":[0,0,0],\"rot\":[0,0,0],\"scale\":\
       [1,1,1]}],\"children\":[]}}}' as latest_tree"
    },
    match (limit, offset) {
      (Some(limit), Some(offset)) => format!("LIMIT {limit} OFFSET {offset}"),
      (Some(limit), None) => format!("LIMIT {limit}"),
      (None, Some(offset)) => format!("LIMIT 9999999999 OFFSET {offset}"),
      (None, None) => String::new(),
    }
  );

  let mut query = sqlx::query_as::<_, PublicCompositionRow>(sqlx::AssertSqlSafe(query_str));
  if let Some(limit) = limit {
    query = query.bind(limit);
  }

  let rows = query.fetch_all(pool).await.map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to list compositions: {err}"),
    )
  })?;

  let ids: Vec<i64> = rows.iter().map(|row| row.comp.id).collect();
  let mut tags_by_id = load_tags(pool, COMPOSITION_TAGS, &ids).await?;

  let result = rows
    .into_iter()
    .map(|row| {
      let latest = CompositionVersion {
        id: row.latest_id,
        composition_id: row.latest_composition_id,
        tree: row.latest_tree,
        created_at: row.latest_created_at,
        thumbnail_url: row.latest_thumbnail_url,
        metadata: row.latest_metadata,
      };
      let mut comp = row.comp;
      comp.tags = tags_by_id.remove(&comp.id).unwrap_or_default();
      PublicComposition { comp, latest }
    })
    .collect();

  Ok(result)
}

#[derive(Deserialize)]
pub struct ListMyCompositionsQuery {
  pub include_code: Option<bool>,
}

pub async fn list_my_compositions(
  State(pool): State<SqlitePool>,
  Extension(user): Extension<User>,
  Query(query): Query<ListMyCompositionsQuery>,
) -> Result<Json<Vec<PublicComposition>>, APIError> {
  let result = list_compositions(
    &pool,
    Some(user.id),
    None,
    None,
    None,
    None,
    query.include_code.unwrap_or(false),
  )
  .await?;
  Ok(Json(result))
}

#[derive(Deserialize)]
pub struct ListPublicCompositionsQuery {
  featured_only: Option<bool>,
  count: Option<usize>,
  offset: Option<usize>,
  pub include_code: Option<bool>,
  pub user_id: Option<i64>,
}

#[derive(Serialize)]
pub struct PublicComposition {
  pub comp: Composition,
  pub latest: CompositionVersion,
}

pub async fn list_public_compositions(
  State(pool): State<SqlitePool>,
  Query(ListPublicCompositionsQuery {
    featured_only,
    count,
    offset,
    include_code,
    user_id,
  }): Query<ListPublicCompositionsQuery>,
) -> Result<Json<Vec<PublicComposition>>, APIError> {
  let result = list_compositions(
    &pool,
    user_id,
    Some(true),
    featured_only,
    Some(count.unwrap_or(100).min(100) as i64),
    offset,
    include_code.unwrap_or(false),
  )
  .await?;
  Ok(Json(result))
}

pub async fn update_composition(
  State(pool): State<SqlitePool>,
  Extension(user): Extension<User>,
  Path(composition_id): Path<i64>,
  Json(payload): Json<UpdateCompositionRequest>,
) -> Result<Json<Composition>, APIError> {
  let allowed_fields = &["title", "description", "is_shared", "tags"];
  let mut set_clauses = Vec::with_capacity(payload.field_mask.len());
  let mut args = SqliteArguments::default();
  let mut new_tags: Option<&[String]> = None;

  for field in &payload.field_mask {
    if !allowed_fields.contains(&field.as_str()) {
      return Err(APIError::new(
        StatusCode::BAD_REQUEST,
        format!("Field '{field}' cannot be updated"),
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
      // not a column on `compositions`; lives in the `composition_tags` join table
      "tags" => new_tags = payload.patch.tags.as_deref(),
      _ => unreachable!("Unexpected field in field_mask: {field}"),
    }
  }

  if set_clauses.is_empty() && new_tags.is_none() {
    return Err(APIError::new(
      StatusCode::BAD_REQUEST,
      "No valid fields to update".to_owned(),
    ));
  }

  let mut tx = pool.begin().await.map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to begin transaction: {err}"),
    )
  })?;

  sqlx::query_scalar::<_, i64>("SELECT id FROM compositions WHERE id = ? AND author_id = ?")
    .bind(composition_id)
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|err| {
      APIError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Failed to check composition ownership: {err}"),
      )
    })?
    .ok_or_else(|| {
      APIError::new(
        StatusCode::NOT_FOUND,
        "Composition not found or you do not have permission to modify it".to_owned(),
      )
    })?;

  if !set_clauses.is_empty() {
    let set_clause = set_clauses.join(", ");
    let query = format!("UPDATE compositions SET {set_clause} WHERE id = ?;");
    args.add(composition_id).unwrap();
    sqlx::query_with(sqlx::AssertSqlSafe(query), args)
      .execute(&mut *tx)
      .await
      .map_err(|err| {
        APIError::new(
          StatusCode::INTERNAL_SERVER_ERROR,
          format!("Failed to update composition: {err}"),
        )
      })?;
  }

  if let Some(tags) = new_tags {
    set_tags(&mut tx, COMPOSITION_TAGS, composition_id, tags).await?;
  }

  tx.commit().await.map_err(|err| {
    APIError::new(
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to commit transaction: {err}"),
    )
  })?;

  get_composition(
    State(pool.clone()),
    Path(composition_id),
    Extension(Some(user.clone())),
    Query(AdminTokenQuery { admin_token: None }),
  )
  .await
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

#[cfg(test)]
mod tests {
  use super::*;

  const WITH_ID: &str = r#"{"version":1,"rootId":"r","globalsSource":"","nodes":{"r":{"id":"r","name":"_root","source":"","instances":[{"pos":[0,0,0],"rot":[0,0,0],"scale":[1,1,1],"id":"deadbeef"}],"children":[]}}}"#;
  const NO_ID: &str = r#"{"version":1,"rootId":"r","globalsSource":"","nodes":{"r":{"id":"r","name":"_root","source":"","instances":[{"pos":[1.5,0,0],"rot":[0,0,0],"scale":[1,1,1]}],"children":[]}}}"#;

  #[test]
  fn instance_id_preserved_and_flat_on_round_trip() {
    let tree: TreeDef = serde_json::from_str(WITH_ID).unwrap();
    assert_eq!(tree.nodes["r"].instances[0].id, "deadbeef");
    let out = serde_json::to_value(&tree).unwrap();
    let inst = &out["nodes"]["r"]["instances"][0];
    assert_eq!(inst["id"], "deadbeef");
    assert!(
      inst.get("transform").is_none(),
      "id/pos must sit flat, not nested"
    );
    assert_eq!(inst["pos"][0].as_f64(), Some(0.0));
  }

  #[test]
  fn missing_instance_id_is_backfilled() {
    let tree: TreeDef = serde_json::from_str(NO_ID).unwrap();
    let inst = &tree.nodes["r"].instances[0];
    assert_eq!(inst.transform.pos[0], 1.5);
    assert_eq!(inst.id.len(), 8);
    assert!(inst.id.chars().all(|c| c.is_ascii_hexdigit()));
  }
}
