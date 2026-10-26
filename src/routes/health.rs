use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
pub struct Health {
    status: &'static str,
    database: &'static str,
    version: &'static str,
    commit: Option<String>,
}

pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    let db_ok = sqlx::query("SELECT 1").execute(&state.db).await.is_ok();
    let code = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        code,
        Json(Health {
            status: if db_ok { "ok" } else { "degraded" },
            database: if db_ok { "ok" } else { "unreachable" },
            version: env!("CARGO_PKG_VERSION"),
            // Railway expone el commit desplegado; en local no existe.
            commit: std::env::var("RAILWAY_GIT_COMMIT_SHA")
                .ok()
                .map(|sha| sha.chars().take(7).collect()),
        }),
    )
}
