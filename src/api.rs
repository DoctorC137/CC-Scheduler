use std::sync::Arc;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{delete, get, post, put},
    Json, Router,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

use crate::{
    db::Database,
    error::AppError,
    models::{CreateSchedule, UpdateSchedule},
    scheduler::SchedulerService,
};

#[derive(Clone)]
pub struct AppState {
    pub scheduler: Arc<SchedulerService>,
    pub db: Database,
}

pub fn build_router(scheduler: Arc<SchedulerService>, db: Database) -> Router {
    let state = AppState { scheduler, db };

    Router::new()
        // Frontend
        .route("/", get(index))
        // Health check
        .route("/health", get(health))
        // Schedules CRUD
        .route("/schedules", get(list_schedules))
        .route("/schedules", post(create_schedule))
        .route("/schedules/:id", get(get_schedule))
        .route("/schedules/:id", put(update_schedule))
        .route("/schedules/:id", delete(delete_schedule))
        // Actions manuelles
        .route("/schedules/:id/trigger/:action", post(trigger_now))
        // Clever Cloud API proxy
        .route("/orgs", get(list_orgs))
        .route("/orgs/:org_id/apps", get(list_cc_apps))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn index() -> impl IntoResponse {
    Html(include_str!("frontend.html"))
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn list_orgs(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let orgs = state.scheduler.cc.list_orgs().await?;
    Ok(Json(orgs))
}

async fn list_schedules(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let schedules = state.db.list_schedules().await?;
    Ok(Json(schedules))
}

async fn create_schedule(
    State(state): State<AppState>,
    Json(payload): Json<CreateSchedule>,
) -> Result<impl IntoResponse, AppError> {
    let schedule = state.db.create_schedule(payload).await?;

    // Enregistre immédiatement dans le cron scheduler si activé
    if schedule.enabled {
        state.scheduler.register(&schedule).await?;
    }

    Ok((StatusCode::CREATED, Json(schedule)))
}

async fn get_schedule(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let schedule = state.db.get_schedule(id).await?;
    Ok(Json(schedule))
}

async fn update_schedule(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateSchedule>,
) -> Result<impl IntoResponse, AppError> {
    let schedule = state.db.update_schedule(id, payload).await?;

    // Recharge le schedule dans le cron
    state.scheduler.reload_schedule(id).await?;

    Ok(Json(schedule))
}

async fn delete_schedule(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    state.db.delete_schedule(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Déclenche une action (start|stop) immédiatement, sans attendre le cron.
async fn trigger_now(
    State(state): State<AppState>,
    Path((id, action)): Path<(Uuid, String)>,
) -> Result<impl IntoResponse, AppError> {
    let schedule = state.db.get_schedule(id).await?;

    match action.as_str() {
        "stop" => state.scheduler.cc.stop_app(&schedule.org_id, &schedule.app_id).await?,
        "start" => state.scheduler.cc.start_app(&schedule.org_id, &schedule.app_id).await?,
        _ => return Err(AppError::BadRequest(format!("Unknown action: {}", action))),
    }

    Ok(Json(serde_json::json!({ "triggered": action })))
}

/// Retourne la liste des apps CC d'une organisation (pour le UI de sélection).
async fn list_cc_apps(
    State(state): State<AppState>,
    Path(org_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let apps = state.scheduler.cc.list_apps(&org_id).await?;
    Ok(Json(apps))
}
