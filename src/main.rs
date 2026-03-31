mod api;
mod config;
mod db;
mod error;
mod models;
mod scheduler;
mod clever;

use std::sync::Arc;
use anyhow::Result;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

use crate::{
    config::AppConfig,
    db::Database,
    scheduler::SchedulerService,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Init logging (CC_LOG_LEVEL ou RUST_LOG)
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    info!("Starting cc-scheduler");

    // Config depuis env vars (Clever Cloud les injecte automatiquement)
    let cfg = AppConfig::from_env()?;

    // Connexion DB (POSTGRESQL_ADDON_URI injectée par l'add-on CC)
    let db = Database::connect(&cfg.database_url).await?;
    db.migrate().await?;

    // Service scheduler
    let scheduler = Arc::new(SchedulerService::new(db.clone(), cfg.clone()).await?);
    scheduler.load_and_schedule_all().await?;

    // Lancement du serveur HTTP
    let app = api::build_router(scheduler.clone(), db.clone());

    let addr = format!("0.0.0.0:{}", cfg.port);
    info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
