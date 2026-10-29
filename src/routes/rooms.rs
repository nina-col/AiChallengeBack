//! El cuarto isométrico de cada lugar: se lee y se guarda entero, con revisión para que dos
//! pantallas no se pisen. Las reglas están en `crate::room`.

use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::checkins::tried_adjustments;
use crate::{
    AppState,
    auth::AuthUser,
    error::{AppError, AppResult},
    room::{self, LAYOUT_VERSION, ROOM_CATALOG, UNSUPPORTED},
};

#[derive(Serialize)]
pub struct Room {
    revision: i32,
    layout: Value,
}

/// 404 si el lugar no es de la persona o si todavía no tiene cuarto: la app arma uno por defecto.
pub async fn get_room(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Room>> {
    let room: Option<(i32, Value)> = sqlx::query_as(
        "SELECT r.revision, r.layout FROM rooms r JOIN spaces s ON s.id = r.space_id
         WHERE r.space_id = $1 AND s.user_id = $2",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?;
    let (revision, layout) = room.ok_or(AppError::NotFound)?;
    Ok(Json(Room { revision, layout }))
}

#[derive(Deserialize)]
pub struct SaveRoom {
    /// La revisión que se leyó; 0 para crear el cuarto.
    revision: i32,
    layout: Value,
}

#[derive(Serialize)]
pub struct Saved {
    revision: i32,
}

pub async fn put_room(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<SaveRoom>,
) -> AppResult<Json<Saved>> {
    let owned: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM spaces WHERE id = $1 AND user_id = $2)")
            .bind(id)
            .bind(user.id)
            .fetch_one(&state.db)
            .await?;
    if !owned {
        return Err(AppError::NotFound);
    }

    let owned_adjustments = tried_adjustments(&state.db, user.id).await?;
    let layout = match room::validate(&body.layout, ROOM_CATALOG.as_ref(), &owned_adjustments) {
        Ok(layout) => layout,
        // Un acomodo de una app más nueva no se toca: guardarlo con estas reglas perdería datos.
        Err(problems) if problems.iter().any(|p| p == UNSUPPORTED) => {
            return Err(AppError::UpdateApp);
        }
        Err(problems) => return Err(AppError::InvalidLayout(problems)),
    };

    // Se guarda normalizado: vuelto a serializar desde los tipos.
    let normalized = serde_json::to_value(&layout).expect("el layout se serializa");
    let saved: Option<i32> = sqlx::query_scalar(
        "INSERT INTO rooms (space_id, schema_version, layout)
         SELECT s.id, $2, $3 FROM spaces s WHERE s.id = $1 AND s.user_id = $4
         ON CONFLICT (space_id) DO UPDATE
            SET layout = EXCLUDED.layout, schema_version = EXCLUDED.schema_version,
                revision = rooms.revision + 1, updated_at = now()
          WHERE rooms.revision = $5
         RETURNING revision",
    )
    .bind(id)
    .bind(LAYOUT_VERSION as i16)
    .bind(&normalized)
    .bind(user.id)
    .bind(body.revision)
    .fetch_optional(&state.db)
    .await
    .map_err(too_big)?;
    if let Some(revision) = saved {
        return Ok(Json(Saved { revision }));
    }

    // Sin fila: el lugar ya no es de la persona (404) u otra pantalla guardó antes (409).
    let current: Option<i32> = sqlx::query_scalar(
        "SELECT r.revision FROM rooms r JOIN spaces s ON s.id = r.space_id
         WHERE r.space_id = $1 AND s.user_id = $2",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?;
    match current {
        Some(revision) => Err(AppError::Conflict { revision }),
        None => Err(AppError::NotFound),
    }
}

/// Postgres mide el JSON con espacios (`layout::text`), un poco más largo que el compacto que
/// revisa `validate`. Si solo pasa el límite por eso, se responde igual que un cuarto muy grande.
fn too_big(err: sqlx::Error) -> AppError {
    match &err {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23514") => {
            AppError::InvalidLayout(vec!["layout: too-large".into()])
        }
        _ => AppError::Database(err),
    }
}
