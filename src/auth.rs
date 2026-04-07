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

// The session cookie value is HMAC-SHA1(password, domain-string).
// The password is never stored or sent over the wire; only its MAC is set as a cookie.
pub fn session_cookie_value(password: &str) -> String {
    let mut mac = HmacSha1::new_from_slice(password.as_bytes()).unwrap();
    mac.update(b"cc-scheduler-session-v1");
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

pub async fn login_page() -> impl IntoResponse {
    Html(LOGIN_HTML)
}

#[derive(Deserialize)]
pub struct LoginForm {
    password: String,
}

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

pub async fn logout() -> impl IntoResponse {
    let clear = "session=; HttpOnly; Path=/; SameSite=Lax; Max-Age=0";
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::SET_COOKIE, clear.parse().unwrap());
    headers.insert(header::LOCATION, "/auth/login".parse().unwrap());
    (StatusCode::FOUND, headers).into_response()
}

// Axum middleware: passes /auth/* and /health through unauthenticated;
// all other routes require a valid session cookie.
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

const LOGIN_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>CC Scheduler</title>
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap" rel="stylesheet">
  <style>
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body {
      font-family: 'Inter', -apple-system, sans-serif;
      background: #F4F6F9; color: #1A2033;
      display: flex; align-items: center; justify-content: center; height: 100vh;
      -webkit-font-smoothing: antialiased;
    }
    .card {
      background: #FFFFFF;
      border: 1px solid #E2E8F0;
      border-radius: 18px; padding: 36px 32px; width: 360px;
      box-shadow: 0 8px 40px rgba(11,15,30,.1);
    }
    .logo-row {
      display: flex; align-items: center; gap: 12px; margin-bottom: 28px;
    }
    .brand-name { font-size: 16px; font-weight: 700; color: #0B0F1E; }
    .brand-sub  { font-size: 11px; color: #94A3B8; margin-top: 2px; }
    label {
      display: block; font-size: 10px; font-weight: 600;
      color: #94A3B8; margin-bottom: 7px;
      text-transform: uppercase; letter-spacing: 0.8px;
    }
    input[type=password] {
      width: 100%; background: #F8FAFC;
      border: 1px solid #E2E8F0;
      color: #0B0F1E; padding: 10px 14px; border-radius: 9px;
      font-size: 14px; font-family: inherit; outline: none;
      margin-bottom: 18px; transition: border .15s, box-shadow .15s;
    }
    input[type=password]:focus {
      border-color: #CB1C42;
      box-shadow: 0 0 0 3px rgba(203,28,66,.08);
      background: #FFFFFF;
    }
    button {
      width: 100%; background: #CB1C42; color: #fff; border: none;
      padding: 11px; border-radius: 9px; cursor: pointer;
      font-size: 14px; font-weight: 600; font-family: inherit;
      transition: background .15s, box-shadow .15s;
    }
    button:hover { background: #E01E4A; box-shadow: 0 4px 16px rgba(203,28,66,.35); }
    .err {
      display: flex; align-items: center; gap: 8px;
      color: #DC2626; font-size: 12px; font-weight: 500;
      background: #FFF5F5;
      border: 1px solid #FECACA;
      border-radius: 8px; padding: 9px 12px; margin-bottom: 16px;
    }
  </style>
</head>
<body>
  <div class="card">
    <div class="logo-row">
      <svg width="40" height="40" viewBox="0 0 100 116" xmlns="http://www.w3.org/2000/svg">
        <polygon points="50,2  7,29  33,62  50,38"  fill="#F2926B"/>
        <polygon points="50,2  50,38 93,29"         fill="#E55C3A"/>
        <polygon points="7,29  7,87  33,62"         fill="#CC3A28"/>
        <polygon points="50,38 33,62 67,62"         fill="#C03030"/>
        <polygon points="50,38 67,62 93,29"         fill="#A82020"/>
        <polygon points="93,29 67,62 93,87"         fill="#8C1818"/>
        <polygon points="33,62 7,87  50,86"         fill="#721028"/>
        <polygon points="33,62 50,86 67,62"         fill="#600D20"/>
        <polygon points="67,62 50,86 93,87"         fill="#4E0A1A"/>
        <polygon points="7,87  50,114 50,86"        fill="#5C1024"/>
        <polygon points="50,86 50,114 93,87"        fill="#420B18"/>
      </svg>
      <div>
        <div class="brand-name">CC Scheduler</div>
        <div class="brand-sub">Clever Cloud</div>
      </div>
    </div>
    <form method="POST" action="/auth/login">
      <label>Password</label>
      <input type="password" name="password" autofocus placeholder="••••••••">
      <button type="submit">Sign in</button>
    </form>
  </div>
  <script>
    if (new URLSearchParams(location.search).get('error')) {
      const err = document.createElement('div');
      err.className = 'err';
      err.innerHTML = '<svg width="14" height="14" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg> Incorrect password.';
      document.querySelector('form').prepend(err);
    }
  </script>
</body>
</html>"##;
