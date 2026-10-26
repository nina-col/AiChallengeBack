mod account;
mod checkins;
mod health;
mod scan;
mod spaces;

use axum::{
    Router,
    response::Html,
    routing::{delete, get, post},
};
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer, trace::TraceLayer};

use crate::AppState;

/// Una foto reducida (~150 KB) y 10 s de audio WAV (~320 KB) caben con mucho margen.
const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

pub fn router(state: AppState) -> Router {
    let v1 = Router::new()
        .route("/auth/device", post(account::login_device))
        .route(
            "/me",
            get(account::get_me).patch(account::update_me).delete(account::delete_me),
        )
        .route("/spaces", get(spaces::list_spaces).post(spaces::create_space))
        .route("/spaces/{id}", delete(spaces::delete_space))
        .route("/catalog", get(checkins::get_catalog))
        .route("/checkins", get(checkins::list_checkins).post(checkins::create_checkin))
        .route("/checkins/{id}", get(checkins::get_checkin))
        .route("/checkins/{id}/decline", post(checkins::decline))
        .route("/checkins/{id}/accept", post(checkins::accept))
        .route("/checkins/{id}/outcome", post(checkins::record_outcome))
        .route("/checkins/{id}/abandon", post(checkins::abandon))
        .route("/playbook", get(checkins::playbook))
        .route("/scan/photo", post(scan::scan_photo));

    Router::new()
        .route("/health", get(health::health))
        .route("/privacy", get(privacy))
        .nest("/v1", v1)
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Aviso de privacidad (versión `account::CONSENT_VERSION`). Lo enlaza la app y App Store Connect.
async fn privacy() -> Html<&'static str> {
    Html(include_str!("../../static/privacy.html"))
}
