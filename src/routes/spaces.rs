use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::checkins::count;
use crate::{
    AppState,
    auth::AuthUser,
    catalog::{CATALOG, LocalizedAdjustment},
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

/// "Este lugar cambió": el motor solo toma como de aquí lo que se pruebe desde ahora. No borra nada.
pub async fn reset_space(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let updated = sqlx::query("UPDATE spaces SET reset_at = now() WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if updated == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, sqlx::FromRow)]
pub struct PlaceRef {
    id: Uuid,
    name: String,
    kind: String,
    reset_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct StimulusCount {
    stimulus: String,
    label: &'static str,
    times: u32,
}

#[derive(sqlx::FromRow)]
struct TriedRow {
    adjustment_id: String,
    stimulus: Option<String>,
    helped: i64,
    trials: i64,
    last_tried: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct TriedHere {
    adjustment: LocalizedAdjustment<'static>,
    /// El estímulo con el que más se probó aquí (para "Usarlo otra vez").
    stimulus: Option<String>,
    helped: u32,
    /// Pruebas terminadas aquí, sea cual sea el resultado.
    trials: u32,
    last_tried: Option<DateTime<Utc>>,
}

/// Lo que se sabe de un lugar. Cuenta pruebas, no resultados, y los números son conteos reales:
/// lo que no sirvió aquí también es algo que se sabe.
#[derive(Serialize)]
pub struct PlaceMemory {
    space: PlaceRef,
    /// Pruebas terminadas aquí desde siempre (incluye las comparaciones sin ajuste). Solo sube.
    trials: u32,
    /// Las de después de "Este lugar cambió"; igual a `trials` si nunca se reinició.
    trials_since_reset: u32,
    /// Ajustes distintos probados aquí: "cosas que sabes".
    known: u32,
    /// Lo que suele molestar aquí: registros de este lugar donde se marcó "sí".
    stimuli: Vec<StimulusCount>,
    /// Cada ajuste probado aquí (en cualquier tarea), del más reciente al más antiguo.
    tried: Vec<TriedHere>,
    /// "No puedo hacerlo aquí".
    blocked: Vec<LocalizedAdjustment<'static>>,
}

pub async fn memory(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<PlaceMemory>> {
    let space: PlaceRef = sqlx::query_as(
        "SELECT id, name, kind, reset_at FROM spaces WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let (trials, trials_since_reset, known): (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*),
                count(*) FILTER (WHERE $3::timestamptz IS NULL OR completed_at > $3),
                count(DISTINCT adjustment_id) FILTER (WHERE adjustment_id <> 'no_change')
         FROM checkins WHERE space_id = $1 AND user_id = $2 AND status = 'done'",
    )
    .bind(id)
    .bind(user.id)
    .bind(space.reset_at)
    .fetch_one(&state.db)
    .await?;

    let stimuli: Vec<(String, i64)> = sqlx::query_as(
        "SELECT o.stimulus, count(DISTINCT o.checkin_id)
         FROM observations o JOIN checkins c ON c.id = o.checkin_id
         WHERE c.space_id = $1 AND c.user_id = $2 AND o.response = 'yes'
         GROUP BY o.stimulus
         ORDER BY count(DISTINCT o.checkin_id) DESC, o.stimulus",
    )
    .bind(id)
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    let tried: Vec<TriedRow> = sqlx::query_as(
        "SELECT adjustment_id,
                mode() WITHIN GROUP (ORDER BY focus_stimulus) AS stimulus,
                count(*) FILTER (WHERE outcome = 'helped') AS helped,
                count(*) AS trials,
                max(completed_at) AS last_tried
         FROM checkins
         WHERE space_id = $1 AND user_id = $2 AND status = 'done'
           AND adjustment_id IS NOT NULL AND adjustment_id <> 'no_change'
         GROUP BY adjustment_id
         ORDER BY max(completed_at) DESC NULLS LAST, adjustment_id",
    )
    .bind(id)
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    let blocked: Vec<String> = sqlx::query_scalar(
        "SELECT adjustment_id FROM space_blocks WHERE space_id = $1 ORDER BY created_at",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    let locale = user.locale;
    Ok(Json(PlaceMemory {
        space,
        trials: count(trials),
        trials_since_reset: count(trials_since_reset),
        known: count(known),
        stimuli: stimuli
            .into_iter()
            .filter_map(|(stimulus, times)| {
                let label = CATALOG.stimulus(&stimulus)?.label.get(locale);
                Some(StimulusCount {
                    stimulus,
                    label,
                    times: count(times),
                })
            })
            .collect(),
        tried: tried
            .into_iter()
            .filter_map(|r| {
                Some(TriedHere {
                    adjustment: CATALOG.adjustment(&r.adjustment_id)?.localized(locale),
                    stimulus: r.stimulus,
                    helped: count(r.helped),
                    trials: count(r.trials),
                    last_tried: r.last_tried,
                })
            })
            .collect(),
        blocked: blocked
            .iter()
            .filter_map(|id| Some(CATALOG.adjustment(id)?.localized(locale)))
            .collect(),
    }))
}
