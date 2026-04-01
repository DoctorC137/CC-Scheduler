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
    auth::require_auth,
    clever::CleverClient,
    db::Database,
    error::AppError,
    models::{CreateSchedule, UpdateSchedule},
    scheduler::SchedulerService,
};

#[derive(Clone)]
pub struct AppState {
    pub scheduler: Arc<SchedulerService>,
    pub db: Database,
    pub cc: Arc<CleverClient>,
    pub org_id: String,
    pub org_name: String,
    pub session_value: String,
    pub app_password: String,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/auth/login", get(crate::auth::login_page).post(crate::auth::login_submit))
        .route("/auth/logout", get(crate::auth::logout))
        .route("/health", get(health))
        .route("/orgs", get(list_orgs))
        .route("/orgs/:org_id/apps", get(list_cc_apps))
        .route("/schedules", get(list_schedules))
        .route("/schedules", post(create_schedule))
        .route("/schedules/:id", get(get_schedule))
        .route("/schedules/:id", put(update_schedule))
        .route("/schedules/:id", delete(delete_schedule))
        .route("/schedules/:id/trigger/:action", post(trigger_now))
        .layer(axum::middleware::from_fn_with_state(state.clone(), require_auth))
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

/// Retourne l'organisation configurée (toujours une seule avec les service tokens)
async fn list_orgs(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!([{
        "id": state.org_id,
        "name": state.org_name,
    }]))
}

async fn list_cc_apps(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let apps = state.cc.list_apps().await?;
    Ok(Json(apps))
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

async fn trigger_now(
    State(state): State<AppState>,
    Path((id, action)): Path<(Uuid, String)>,
) -> Result<impl IntoResponse, AppError> {
    let schedule = state.db.get_schedule(id).await?;
    match action.as_str() {
        "stop" => state.cc.stop_app(&schedule.app_id).await?,
        "start" => state.cc.start_app(&schedule.app_id).await?,
        _ => return Err(AppError::BadRequest(format!("Unknown action: {}", action))),
    }
    Ok(Json(serde_json::json!({ "triggered": action })))
}
