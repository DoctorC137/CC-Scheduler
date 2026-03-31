use std::sync::Arc;
use axum::{
    extract::{Extension, Path, State},
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
    models::{CreateSchedule, UpdateSchedule, User},
    scheduler::SchedulerService,
};

#[derive(Clone)]
pub struct AppState {
    pub scheduler: Arc<SchedulerService>,
    pub db: Database,
    pub http: reqwest::Client,
    pub base_url: String,
}

pub fn build_router(
    scheduler: Arc<SchedulerService>,
    db: Database,
    http: reqwest::Client,
    base_url: String,
) -> Router {
    let state = AppState { scheduler, db, http, base_url };

    Router::new()
        // Frontend
        .route("/", get(index))
        // Auth
        .route("/auth/login", get(crate::auth::login))
        .route("/auth/callback", get(crate::auth::callback))
        .route("/auth/logout", get(crate::auth::logout))
        .route("/me", get(crate::auth::me))
        // Health
        .route("/health", get(health))
        // Schedules CRUD
        .route("/schedules", get(list_schedules))
        .route("/schedules", post(create_schedule))
        .route("/schedules/:id", get(get_schedule))
        .route("/schedules/:id", put(update_schedule))
        .route("/schedules/:id", delete(delete_schedule))
        .route("/schedules/:id/trigger/:action", post(trigger_now))
        // Clever Cloud API proxy
        .route("/orgs", get(list_orgs))
        .route("/orgs/:org_id/apps", get(list_cc_apps))
        // Middleware d'authentification (protège tout sauf /auth/* et /health)
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

async fn list_orgs(
    Extension(user): Extension<User>,
) -> Result<impl IntoResponse, AppError> {
    let cc = CleverClient::new(user.access_token, user.access_secret);
    let orgs = cc.list_orgs().await?;
    Ok(Json(orgs))
}

async fn list_cc_apps(
    Extension(user): Extension<User>,
    Path(org_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let cc = CleverClient::new(user.access_token, user.access_secret);
    let apps = cc.list_apps(&org_id).await?;
    Ok(Json(apps))
}

async fn list_schedules(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> Result<impl IntoResponse, AppError> {
    let schedules = state.db.list_schedules(user.id).await?;
    Ok(Json(schedules))
}

async fn create_schedule(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(payload): Json<CreateSchedule>,
) -> Result<impl IntoResponse, AppError> {
    let schedule = state.db.create_schedule(user.id, payload).await?;
    if schedule.enabled {
        state.scheduler.register(&schedule).await?;
    }
    Ok((StatusCode::CREATED, Json(schedule)))
}

async fn get_schedule(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let schedule = state.db.get_schedule(id, user.id).await?;
    Ok(Json(schedule))
}

async fn update_schedule(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateSchedule>,
) -> Result<impl IntoResponse, AppError> {
    let schedule = state.db.update_schedule(id, user.id, payload).await?;
    state.scheduler.reload_schedule(id).await?;
    Ok(Json(schedule))
}

async fn delete_schedule(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    state.db.delete_schedule(id, user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn trigger_now(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path((id, action)): Path<(Uuid, String)>,
) -> Result<impl IntoResponse, AppError> {
    let schedule = state.db.get_schedule(id, user.id).await?;
    let cc = CleverClient::new(user.access_token, user.access_secret);
    match action.as_str() {
        "stop" => cc.stop_app(&schedule.org_id, &schedule.app_id).await?,
        "start" => cc.start_app(&schedule.org_id, &schedule.app_id).await?,
        _ => return Err(AppError::BadRequest(format!("Unknown action: {}", action))),
    }
    Ok(Json(serde_json::json!({ "triggered": action })))
}
