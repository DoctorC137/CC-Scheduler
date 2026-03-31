use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Redirect, Response},
};
use serde::Deserialize;
use tracing::{error, warn};
use uuid::Uuid;

use crate::{
    api::AppState,
    clever::{cc_access_token, cc_request_token, CleverClient},
    models::User,
};

/// GET /auth/login — démarre le flow OAuth1 three-legged avec Clever Cloud
pub async fn login(State(state): State<AppState>) -> impl IntoResponse {
    let callback_url = format!("{}/auth/callback", state.base_url);

    match cc_request_token(&state.http, &callback_url).await {
        Ok((req_token, req_secret)) => {
            if let Err(e) = state.db.save_oauth_request(&req_token, &req_secret).await {
                error!("Failed to save oauth request token: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
            }
            let authorize_url = format!(
                "https://api.clever-cloud.com/oauth/authorize?oauth_token={}",
                req_token
            );
            Redirect::to(&authorize_url).into_response()
        }
        Err(e) => {
            error!("Failed to get CC request token: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to initiate authentication").into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct CallbackParams {
    oauth_token: String,
    oauth_verifier: String,
}

/// GET /auth/callback — CC redirige ici après autorisation de l'utilisateur
pub async fn callback(
    State(state): State<AppState>,
    Query(params): Query<CallbackParams>,
) -> impl IntoResponse {
    // Récupère et supprime le request_secret (one-shot)
    let req_secret = match state.db.get_and_delete_oauth_request(&params.oauth_token).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            warn!("OAuth request token inconnu ou expiré: {}", params.oauth_token);
            return (StatusCode::BAD_REQUEST, "oauth_token invalide ou expiré").into_response();
        }
        Err(e) => {
            error!("DB error lors du lookup oauth_request: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
        }
    };

    // Échange request token → access token
    let (access_token, access_secret) = match cc_access_token(
        &state.http,
        &params.oauth_token,
        &req_secret,
        &params.oauth_verifier,
    )
    .await
    {
        Ok(pair) => pair,
        Err(e) => {
            error!("Failed to get CC access token: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Authentication failed").into_response();
        }
    };

    // Récupère les infos de l'utilisateur via l'API CC
    let cc_client = CleverClient::new(access_token.clone(), access_secret.clone());
    let self_info = match cc_client.get_self().await {
        Ok(info) => info,
        Err(e) => {
            error!("Failed to get CC user info: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to retrieve user info").into_response();
        }
    };

    let cc_user_id = match self_info["id"].as_str() {
        Some(id) => id.to_string(),
        None => {
            error!("CC self response missing 'id' field: {:?}", self_info);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Invalid user info from CC").into_response();
        }
    };
    let cc_email = self_info["email"].as_str().map(|s| s.to_string());

    // Upsert user en DB (met à jour le token s'il a changé)
    let user = match state
        .db
        .upsert_user(&cc_user_id, cc_email.as_deref(), &access_token, &access_secret)
        .await
    {
        Ok(u) => u,
        Err(e) => {
            error!("Failed to upsert user: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
        }
    };

    // Crée une session 7j
    let session = match state.db.create_session(user.id).await {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to create session: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
        }
    };

    // Cookie HttpOnly + redirect vers l'app
    let cookie = format!(
        "session_id={}; HttpOnly; Path=/; SameSite=Lax; Max-Age=604800",
        session.id
    );
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::SET_COOKIE, cookie.parse().unwrap());
    headers.insert(header::LOCATION, "/".parse().unwrap());
    (StatusCode::FOUND, headers).into_response()
}

/// GET /auth/logout — invalide la session et redirige vers /auth/login
pub async fn logout(State(state): State<AppState>, req: Request<Body>) -> impl IntoResponse {
    if let Some(sid_str) = extract_cookie(&req, "session_id") {
        if let Ok(sid) = Uuid::parse_str(&sid_str) {
            let _ = state.db.delete_session(sid).await;
        }
    }
    let clear_cookie = "session_id=; HttpOnly; Path=/; SameSite=Lax; Max-Age=0";
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::SET_COOKIE, clear_cookie.parse().unwrap());
    headers.insert(header::LOCATION, "/auth/login".parse().unwrap());
    (StatusCode::FOUND, headers).into_response()
}

/// Middleware axum : vérifie la session sur toutes les routes sauf /auth/* et /health.
/// Injecte `Extension<User>` pour les handlers protégés.
pub async fn require_auth(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path();

    // Routes publiques
    if path.starts_with("/auth/") || path == "/health" {
        return next.run(req).await;
    }

    // Cherche le cookie de session
    let session_id = extract_cookie(&req, "session_id")
        .and_then(|s| Uuid::parse_str(&s).ok());

    if let Some(sid) = session_id {
        match state.db.get_session_user(sid).await {
            Ok(Some(user)) => {
                req.extensions_mut().insert(user);
                return next.run(req).await;
            }
            Ok(None) => {}
            Err(e) => error!("Session lookup error: {}", e),
        }
    }

    // Non authentifié — redirige le navigateur, retourne 401 pour les requêtes API
    let is_browser = req
        .headers()
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("text/html"))
        .unwrap_or(false);

    if is_browser {
        Redirect::to("/auth/login").into_response()
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Unauthorized" })),
        )
            .into_response()
    }
}

/// Extrait la valeur d'un cookie depuis le header Cookie.
pub fn extract_cookie(req: &Request<Body>, name: &str) -> Option<String> {
    req.headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            s.split(';')
                .map(|p| p.trim())
                .find(|p| p.starts_with(&format!("{}=", name)))
                .and_then(|p| p.splitn(2, '=').nth(1))
                .map(|v| v.to_string())
        })
}

/// Retourne les infos publiques de l'utilisateur connecté (pour le frontend)
pub async fn me(
    axum::Extension(user): axum::Extension<User>,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "id": user.cc_user_id,
        "email": user.cc_email,
    }))
}
