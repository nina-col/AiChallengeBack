mod auth;
mod catalog;
mod config;
mod engine;
mod error;
mod routes;

use std::time::Duration;

use anyhow::Context;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "spaceease_api=info,tower_http=info".into()),
        )
        .init();

    let config = Config::from_env()?;

    // Un catálogo roto no debe llegar a producción: el healthcheck de Railway nunca pasaría.
    let problems = catalog::problems(&catalog::CATALOG);
    anyhow::ensure!(problems.is_empty(), "catalog.json inválido:\n{}", problems.join("\n"));

    let db = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.database_url)
        .await
        .context("no se pudo conectar a Postgres")?;
    sqlx::migrate!().run(&db).await.context("fallaron las migraciones")?;

    let app = routes::router(AppState { db });
    let listener = TcpListener::bind(("0.0.0.0", config.port)).await?;
    tracing::info!("escuchando en {}", listener.local_addr()?);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Railway manda SIGTERM al redesplegar; terminamos las peticiones en curso antes de salir.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sigterm) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sigterm.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
