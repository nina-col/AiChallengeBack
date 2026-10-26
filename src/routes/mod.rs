mod health;

use axum::{Router, routing::get};
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer, trace::TraceLayer};

use crate::AppState;

/// Una foto reducida (~150 KB) y 10 s de audio WAV (~320 KB) caben con mucho margen.
const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health))
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
