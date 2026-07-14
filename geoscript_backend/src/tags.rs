use std::collections::HashMap;

use axum::http::StatusCode;
use sqlx::{Executor, Sqlite, SqliteConnection};
use tracing::error;

use crate::server::APIError;

const MAX_TAG_LEN: usize = 64;
const MAX_TAGS_PER_ENTITY: usize = 24;

#[derive(Clone, Copy)]
pub struct TagJoin {
  table: &'static str,
  id_col: &'static str,
}

pub const TEXTURE_TAGS: TagJoin = TagJoin {
  table: "texture_tags",
  id_col: "texture_id",
};
pub const MATERIAL_TAGS: TagJoin = TagJoin {
  table: "material_tags",
  id_col: "material_id",
};
pub const COMPOSITION_TAGS: TagJoin = TagJoin {
  table: "composition_tags",
  id_col: "composition_id",
};

fn internal_err(err: sqlx::Error, ctx: &str) -> APIError {
  error!("{ctx}: {err}");
  APIError::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
}

/// Tags are identified by their text, so they're canonicalized (lowercased, whitespace collapsed)
/// before hitting the `UNIQUE` column; without this `Metal` and `metal ` would be distinct tags.
pub fn normalize_tags(tags: &[String]) -> Result<Vec<String>, APIError> {
  let mut out: Vec<String> = Vec::new();
  for raw in tags {
    let tag = raw
      .split_whitespace()
      .collect::<Vec<_>>()
      .join(" ")
      .to_lowercase();
    if tag.is_empty() {
      continue;
    }
    if tag.len() > MAX_TAG_LEN {
      return Err(APIError::new(
        StatusCode::BAD_REQUEST,
        format!("Tag exceeds {MAX_TAG_LEN} chars: {tag}"),
      ));
    }
    if !out.contains(&tag) {
      out.push(tag);
    }
  }

  if out.len() > MAX_TAGS_PER_ENTITY {
    return Err(APIError::new(
      StatusCode::BAD_REQUEST,
      format!("Too many tags; max is {MAX_TAGS_PER_ENTITY}"),
    ));
  }

  out.sort();
  Ok(out)
}

/// Replaces the entity's tags wholesale, creating any that don't exist yet. Returns the normalized
/// tags as stored.
pub async fn set_tags(
  conn: &mut SqliteConnection,
  join: TagJoin,
  entity_id: i64,
  tags: &[String],
) -> Result<Vec<String>, APIError> {
  let tags = normalize_tags(tags)?;

  let sql = format!("DELETE FROM {} WHERE {} = ?", join.table, join.id_col);
  sqlx::query(sqlx::AssertSqlSafe(sql))
    .bind(entity_id)
    .execute(&mut *conn)
    .await
    .map_err(|err| internal_err(err, "Error clearing tags"))?;

  for tag in &tags {
    sqlx::query("INSERT OR IGNORE INTO tags (tag) VALUES (?)")
      .bind(tag)
      .execute(&mut *conn)
      .await
      .map_err(|err| internal_err(err, "Error creating tag"))?;

    let sql = format!(
      "INSERT INTO {} ({}, tag_id) SELECT ?, id FROM tags WHERE tag = ?",
      join.table, join.id_col
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
      .bind(entity_id)
      .bind(tag)
      .execute(&mut *conn)
      .await
      .map_err(|err| internal_err(err, "Error linking tag"))?;
  }

  Ok(tags)
}

pub async fn load_tags<'e, E: Executor<'e, Database = Sqlite>>(
  executor: E,
  join: TagJoin,
  entity_ids: &[i64],
) -> Result<HashMap<i64, Vec<String>>, APIError> {
  let mut out: HashMap<i64, Vec<String>> = entity_ids.iter().map(|&id| (id, Vec::new())).collect();
  if entity_ids.is_empty() {
    return Ok(out);
  }

  let ids = entity_ids
    .iter()
    .map(|id| id.to_string())
    .collect::<Vec<_>>()
    .join(",");
  let sql = format!(
    "SELECT j.{id_col}, tags.tag FROM {table} j JOIN tags ON j.tag_id = tags.id WHERE j.{id_col} \
     IN ({ids}) ORDER BY tags.tag",
    table = join.table,
    id_col = join.id_col
  );

  let rows: Vec<(i64, String)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
    .fetch_all(executor)
    .await
    .map_err(|err| internal_err(err, "Error loading tags"))?;

  for (entity_id, tag) in rows {
    out.entry(entity_id).or_default().push(tag);
  }
  Ok(out)
}

pub async fn load_tags_one<'e, E: Executor<'e, Database = Sqlite>>(
  executor: E,
  join: TagJoin,
  entity_id: i64,
) -> Result<Vec<String>, APIError> {
  Ok(
    load_tags(executor, join, &[entity_id])
      .await?
      .remove(&entity_id)
      .unwrap_or_default(),
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn normalization_dedups_and_canonicalizes() {
    let tags = [
      "  Metal ".to_owned(),
      "metal".to_owned(),
      "Wet   Stone".to_owned(),
      "".to_owned(),
    ];
    assert_eq!(normalize_tags(&tags).unwrap(), vec!["metal", "wet stone"]);
  }

  #[test]
  fn oversized_tags_rejected() {
    assert!(normalize_tags(&["x".repeat(MAX_TAG_LEN + 1)]).is_err());
    let many: Vec<String> = (0..MAX_TAGS_PER_ENTITY + 1)
      .map(|i| i.to_string())
      .collect();
    assert!(normalize_tags(&many).is_err());
  }
}
