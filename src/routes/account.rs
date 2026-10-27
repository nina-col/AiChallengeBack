use axum::{Json, extract::State, http::StatusCode};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{AuthUser, hash_device_key, valid_device_key},
    error::{AppError, AppResult},
};

/// Versión vigente del aviso de privacidad (GET /privacy). Subirla si cambia lo que se guarda.
pub const CONSENT_VERSION: i16 = 1;

#[derive(Deserialize)]
pub struct DeviceLogin {
    device_key: String,
    #[serde(default)]
    locale: Option<String>,
    /// Versión del aviso que la persona aceptó; obligatoria para crear la cuenta.
    #[serde(default)]
    consent_version: Option<i16>,
}

#[derive(Serialize)]
pub struct DeviceSession {
    user_id: Uuid,
    created: bool,
}

/// Crea la cuenta del dispositivo la primera vez; las siguientes solo confirma que existe.
pub async fn login_device(
    State(state): State<AppState>,
    Json(body): Json<DeviceLogin>,
) -> AppResult<(StatusCode, Json<DeviceSession>)> {
    if !valid_device_key(&body.device_key) {
        return Err(AppError::BadRequest("device_key inválida".into()));
    }
    let locale = parse_locale(body.locale.as_deref())?;
    let key_hash = hash_device_key(&body.device_key);

    if let Some(user_id) = existing_user(&state, &key_hash).await? {
        return Ok((
            StatusCode::OK,
            Json(DeviceSession {
                user_id,
                created: false,
            }),
        ));
    }
    // Guardar registros sobre el entorno de trabajo es dato sensible: sin consentimiento expreso
    // no se crea la cuenta (LFPDPPP art. 8; Apple 5.1.1).
    if body.consent_version != Some(CONSENT_VERSION) {
        return Err(AppError::BadRequest(format!(
            "falta aceptar el aviso de privacidad (consent_version = {CONSENT_VERSION})"
        )));
    }

    let created: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO users (key_hash, locale, consent_version, consented_at) VALUES ($1, $2, $3, now())
         ON CONFLICT (key_hash) DO NOTHING RETURNING id",
    )
    .bind(&key_hash)
    .bind(locale)
    .bind(CONSENT_VERSION)
    .fetch_optional(&state.db)
    .await?;

    match created {
        Some(user_id) => Ok((
            StatusCode::CREATED,
            Json(DeviceSession {
                user_id,
                created: true,
            }),
        )),
        // Dos peticiones a la vez con la misma llave: la otra ganó.
        None => {
            let user_id = existing_user(&state, &key_hash)
                .await?
                .ok_or(AppError::NotFound)?;
            Ok((
                StatusCode::OK,
                Json(DeviceSession {
                    user_id,
                    created: false,
                }),
            ))
        }
    }
}

async fn existing_user(state: &AppState, key_hash: &[u8]) -> AppResult<Option<Uuid>> {
    Ok(
        sqlx::query_scalar("SELECT id FROM users WHERE key_hash = $1")
            .bind(key_hash)
            .fetch_optional(&state.db)
            .await?,
    )
}

#[derive(Serialize, sqlx::FromRow)]
pub struct Me {
    id: Uuid,
    locale: String,
    created_at: DateTime<Utc>,
}

pub async fn get_me(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Me>> {
    let me = sqlx::query_as("SELECT id, locale, created_at FROM users WHERE id = $1")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(me))
}

#[derive(Deserialize)]
pub struct UpdateMe {
    locale: String,
}

pub async fn update_me(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<UpdateMe>,
) -> AppResult<Json<Me>> {
    let locale = parse_locale(Some(&body.locale))?;
    let me = sqlx::query_as(
        "UPDATE users SET locale = $2 WHERE id = $1 RETURNING id, locale, created_at",
    )
    .bind(user.id)
    .bind(locale)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(me))
}

/// Borra la cuenta y, en cascada, todos sus espacios, registros y observaciones.
pub async fn delete_me(State(state): State<AppState>, user: AuthUser) -> AppResult<StatusCode> {
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user.id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn parse_locale(locale: Option<&str>) -> AppResult<&'static str> {
    match locale {
        None | Some("es") => Ok("es"),
        Some("en") => Ok("en"),
        Some(_) => Err(AppError::BadRequest("locale debe ser 'es' o 'en'".into())),
    }
}
