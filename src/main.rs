mod api;
mod auth;
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
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    info!("Starting cc-scheduler");

    let cfg = AppConfig::from_env()?;

    let db = Database::connect(&cfg.database_url).await?;
    db.migrate().await?;

    let scheduler = Arc::new(SchedulerService::new(db.clone()).await?);
    scheduler.load_and_schedule_all().await?;

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let app = api::build_router(scheduler, db, http, cfg.base_url.clone());

    let addr = format!("0.0.0.0:{}", cfg.port);
    info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
