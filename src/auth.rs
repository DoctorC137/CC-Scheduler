use axum::{
    body::Body,
    extract::State,
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{Html, IntoResponse, Json, Redirect, Response},
    Form,
};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha1::Sha1;

use crate::api::AppState;

type HmacSha1 = Hmac<Sha1>;

/// Calcule la valeur attendue du cookie de session à partir du mot de passe.
/// Utilise HMAC-SHA1 pour que le mot de passe ne soit jamais stocké ou exposé.
pub fn session_cookie_value(password: &str) -> String {
    let mut mac = HmacSha1::new_from_slice(password.as_bytes()).unwrap();
    mac.update(b"cc-scheduler-session-v1");
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// GET /auth/login — page de connexion
pub async fn login_page() -> impl IntoResponse {
    Html(LOGIN_HTML)
}

#[derive(Deserialize)]
pub struct LoginForm {
    password: String,
}

/// POST /auth/login — vérifie le mot de passe et pose le cookie de session
pub async fn login_submit(
    State(state): State<AppState>,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    if form.password == state.app_password {
        let cookie = format!(
            "session={}; HttpOnly; Path=/; SameSite=Lax; Max-Age=604800",
            state.session_value
        );
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(header::SET_COOKIE, cookie.parse().unwrap());
        headers.insert(header::LOCATION, "/".parse().unwrap());
        (StatusCode::FOUND, headers).into_response()
    } else {
        Redirect::to("/auth/login?error=1").into_response()
    }
}

/// GET /auth/logout — supprime le cookie et redirige vers le login
pub async fn logout() -> impl IntoResponse {
    let clear = "session=; HttpOnly; Path=/; SameSite=Lax; Max-Age=0";
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::SET_COOKIE, clear.parse().unwrap());
    headers.insert(header::LOCATION, "/auth/login".parse().unwrap());
    (StatusCode::FOUND, headers).into_response()
}

/// Middleware : protège toutes les routes sauf /auth/* et /health.
pub async fn require_auth(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path();

    if path.starts_with("/auth/") || path == "/health" {
        return next.run(req).await;
    }

    let cookie_ok = extract_cookie(&req, "session")
        .map(|v| v == state.session_value)
        .unwrap_or(false);

    if cookie_ok {
        next.run(req).await
    } else {
        let is_browser = req
            .headers()
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.contains("text/html"))
            .unwrap_or(false);

        if is_browser {
            Redirect::to("/auth/login").into_response()
        } else {
            (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "Unauthorized" })))
                .into_response()
        }
    }
}

fn extract_cookie(req: &Request<Body>, name: &str) -> Option<String> {
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

const LOGIN_HTML: &str = r#"<!DOCTYPE html>
<html lang="fr">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>CC Scheduler — Connexion</title>
  <style>
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
           background: #0f0f1a; color: #e0e0f0;
           display: flex; align-items: center; justify-content: center; height: 100vh; }
    .card { background: #13132a; border: 1px solid #1e1e3a; border-radius: 14px;
            padding: 36px 32px; width: 340px; }
    .logo { font-size: 15px; font-weight: 700; color: #a78bfa; margin-bottom: 6px; }
    .sub  { font-size: 12px; color: #50508a; margin-bottom: 28px; }
    label { display: block; font-size: 11px; color: #7070a8; margin-bottom: 5px; }
    input[type=password] {
      width: 100%; background: #0d0d22; border: 1px solid #1e1e3a; color: #d0d0f0;
      padding: 9px 12px; border-radius: 6px; font-size: 13px; outline: none;
      margin-bottom: 16px; transition: border .15s; }
    input[type=password]:focus { border-color: #6d28d9; }
    button { width: 100%; background: #4c1d95; color: #c4b5fd; border: none;
             padding: 10px; border-radius: 6px; cursor: pointer; font-size: 13px;
             font-weight: 500; transition: background .15s; }
    button:hover { background: #5b21b6; }
    .err { color: #f87171; font-size: 12px; margin-bottom: 14px; }
  </style>
</head>
<body>
  <div class="card">
    <div class="logo">CC Scheduler</div>
    <div class="sub">Gestion des horaires Clever Cloud</div>
    <form method="POST" action="/auth/login">
      <label>Mot de passe</label>
      <input type="password" name="password" autofocus placeholder="••••••••">
      <button type="submit">Se connecter</button>
    </form>
  </div>
  <script>
    // Affiche une erreur si ?error=1
    if (new URLSearchParams(location.search).get('error')) {
      const err = document.createElement('div');
      err.className = 'err';
      err.textContent = 'Mot de passe incorrect.';
      document.querySelector('form').prepend(err);
    }
  </script>
</body>
</html>"#;
