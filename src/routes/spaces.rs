use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    AppState,
    auth::AuthUser,
    error::{AppError, AppResult},
};

const MAX_SPACES: i64 = 20;

#[derive(Serialize, sqlx::FromRow)]
pub struct Space {
    id: Uuid,
    name: String,
    kind: String,
    created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct NewSpace {
    name: String,
    kind: String,
}

pub async fn list_spaces(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<Vec<Space>>> {
    let spaces = sqlx::query_as(
        "SELECT id, name, kind, created_at FROM spaces WHERE user_id = $1 ORDER BY created_at",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(spaces))
}

pub async fn create_space(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<NewSpace>,
) -> AppResult<(StatusCode, Json<Space>)> {
    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(AppError::BadRequest(
            "el nombre debe tener entre 1 y 60 caracteres".into(),
        ));
    }
    if !matches!(body.kind.as_str(), "office" | "home" | "other") {
        return Err(AppError::BadRequest(
            "kind debe ser office, home u other".into(),
        ));
    }

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM spaces WHERE user_id = $1")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    if count >= MAX_SPACES {
        return Err(AppError::BadRequest(format!(
            "máximo {MAX_SPACES} espacios"
        )));
    }

    let space = sqlx::query_as(
        "INSERT INTO spaces (user_id, name, kind) VALUES ($1, $2, $3)
         RETURNING id, name, kind, created_at",
    )
    .bind(user.id)
    .bind(name)
    .bind(&body.kind)
    .fetch_one(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(space)))
}

pub async fn delete_space(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let deleted = sqlx::query("DELETE FROM spaces WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
