use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("falta una llave de dispositivo válida")]
    Unauthorized,
    #[error("no encontrado")]
    NotFound,
    #[error("{0}")]
    BadRequest(String),
    #[error("el análisis con IA no está disponible ahora; sigue con las preguntas")]
    Unavailable,
    #[error("llegaste al límite de análisis de hoy; sigue con las preguntas")]
    TooManyRequests,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

pub type AppResult<T> = Result<T, AppError>;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            AppError::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "ai_unavailable"),
            AppError::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, "daily_limit"),
            AppError::Database(err) => {
                tracing::error!(error = %err, "error de base de datos");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
        };
        // Los detalles de la base de datos se quedan en los logs, nunca en la respuesta.
        let message = match &self {
            AppError::Database(_) => "error interno".to_owned(),
            other => other.to_string(),
        };
        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}
