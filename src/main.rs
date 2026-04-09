mod api;
mod auth;
mod config;
mod db;
mod error;
mod models;
mod scheduler;
mod clever;

use std::{net::SocketAddr, sync::Arc};
use anyhow::Result;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

use crate::{
    api::AppState,
    auth::session_cookie_value,
    clever::CleverClient,
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

    let cc = Arc::new(CleverClient::new(&cfg.cc_org_id, &cfg.cc_service_token));

    let org_name = match cc.get_org().await {
        Ok(org) => org["name"].as_str().unwrap_or(&cfg.cc_org_id).to_string(),
        Err(e) => {
            tracing::warn!("Could not fetch org name: {} — using org_id as fallback", e);
            cfg.cc_org_id.clone()
        }
    };
    info!("Managing organisation: {} ({})", org_name, cfg.cc_org_id);

    let scheduler = Arc::new(SchedulerService::new(db.clone(), cc.clone()).await?);
    scheduler.load_and_schedule_all().await?;

    let session_value = session_cookie_value(&cfg.app_password);

    if cfg.trusted_proxy_ips.is_empty() {
        info!("CC_REVERSE_PROXY_IPS not set — no IP restriction (local/dev mode)");
    } else {
        info!("Trusted proxy IPs: {:?}", cfg.trusted_proxy_ips);
    }

    let state = AppState {
        scheduler,
        db,
        cc,
        org_id: cfg.cc_org_id.clone(),
        org_name,
        session_value,
        app_password: cfg.app_password.clone(),
        trusted_proxy_ips: cfg.trusted_proxy_ips.clone(),
    };

    let app = api::build_router(state);
    let addr = format!("0.0.0.0:{}", cfg.port);
    info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;

    Ok(())
}
