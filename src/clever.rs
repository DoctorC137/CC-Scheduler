use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use hmac::{Hmac, Mac};
use rand::Rng;
use sha1::Sha1;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

// Clever Cloud public OAuth1 consumer credentials (from clever-tools)
const CONSUMER_KEY: &str = "T5nFjKeHH4AIlEveuGhB5S3xg8T19e";
const CONSUMER_SECRET: &str = "MgVMqTr6fWlf2M0tkC2MXOnhfqBWDT";

type HmacSha1 = Hmac<Sha1>;

pub struct CleverClient {
    http: reqwest::Client,
    token: String,
    token_secret: String,
}

impl CleverClient {
    pub fn new(token: String, token_secret: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();
        Self { http, token, token_secret }
    }

    fn oauth_header(&self, method: &str, url: &str) -> String {
        build_oauth1_header(method, url, Some(&self.token), &self.token_secret, &[])
    }

    async fn get(&self, url: &str) -> Result<serde_json::Value> {
        let auth = self.oauth_header("GET", url);
        debug!(url, "CC API GET");
        let resp = self.http.get(url).header("Authorization", auth).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("CC API error {}: {}", status, body);
        }
        Ok(resp.json().await?)
    }

    async fn post_empty(&self, url: &str) -> Result<()> {
        let auth = self.oauth_header("POST", url);
        let resp = self.http.post(url).header("Authorization", auth).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("CC API error {}: {}", status, body);
        }
        Ok(())
    }

    async fn delete_req(&self, url: &str) -> Result<()> {
        let auth = self.oauth_header("DELETE", url);
        let resp = self.http.delete(url).header("Authorization", auth).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("CC API error {}: {}", status, body);
        }
        Ok(())
    }

    pub async fn get_self(&self) -> Result<serde_json::Value> {
        self.get("https://api.clever-cloud.com/v2/self").await
    }

    pub async fn list_orgs(&self) -> Result<serde_json::Value> {
        self.get("https://api.clever-cloud.com/v2/organisations").await
    }

    pub async fn list_apps(&self, org_id: &str) -> Result<serde_json::Value> {
        let url = format!(
            "https://api.clever-cloud.com/v2/organisations/{}/applications",
            org_id
        );
        self.get(&url).await
    }

    pub async fn start_app(&self, org_id: &str, app_id: &str) -> Result<()> {
        let url = format!(
            "https://api.clever-cloud.com/v2/organisations/{}/applications/{}/instances",
            org_id, app_id
        );
        debug!(org_id, app_id, "Starting application");
        self.post_empty(&url).await
    }

    pub async fn stop_app(&self, org_id: &str, app_id: &str) -> Result<()> {
        let url = format!(
            "https://api.clever-cloud.com/v2/organisations/{}/applications/{}/instances",
            org_id, app_id
        );
        debug!(org_id, app_id, "Stopping application");
        self.delete_req(&url).await
    }
}

/// OAuth1 three-legged — étape 1 : obtenir un request token auprès de CC
pub async fn cc_request_token(
    http: &reqwest::Client,
    callback_url: &str,
) -> Result<(String, String)> {
    let url = "https://api.clever-cloud.com/oauth/request_token";
    let auth = build_oauth1_header(
        "POST", url,
        None, "",   // pas de token user à cette étape
        &[("oauth_callback", callback_url)],
    );
    let resp = http.post(url).header("Authorization", auth).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("CC request_token error {}: {}", status, body);
    }
    parse_oauth_form_params(&resp.text().await?)
}

/// OAuth1 three-legged — étape 3 : échanger contre un access token
pub async fn cc_access_token(
    http: &reqwest::Client,
    request_token: &str,
    request_secret: &str,
    verifier: &str,
) -> Result<(String, String)> {
    let url = "https://api.clever-cloud.com/oauth/access_token";
    let auth = build_oauth1_header(
        "POST", url,
        Some(request_token), request_secret,
        &[("oauth_verifier", verifier)],
    );
    let resp = http.post(url).header("Authorization", auth).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("CC access_token error {}: {}", status, body);
    }
    parse_oauth_form_params(&resp.text().await?)
}

/// Parse une réponse URL-encoded OAuth : `oauth_token=X&oauth_token_secret=Y&...`
fn parse_oauth_form_params(body: &str) -> Result<(String, String)> {
    let mut token = None;
    let mut secret = None;
    for part in body.split('&') {
        let mut kv = part.splitn(2, '=');
        match (kv.next(), kv.next()) {
            (Some("oauth_token"), Some(v)) => token = Some(v.to_string()),
            (Some("oauth_token_secret"), Some(v)) => secret = Some(v.to_string()),
            _ => {}
        }
    }
    match (token, secret) {
        (Some(t), Some(s)) => Ok((t, s)),
        _ => bail!("OAuth response invalide (token ou secret manquant): {}", body),
    }
}

/// Construit un header Authorization OAuth1 signé HMAC-SHA1.
///
/// - `token`        : oauth_token (None pour l'étape request_token)
/// - `token_secret` : "" pour request_token, request_secret pour access_token
/// - `extra_params` : params supplémentaires inclus dans la signature
///                    ex: [("oauth_callback", url)] ou [("oauth_verifier", v)]
fn build_oauth1_header(
    method: &str,
    url: &str,
    token: Option<&str>,
    token_secret: &str,
    extra_params: &[(&str, &str)],
) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();

    let nonce: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    let mut params: Vec<(&str, String)> = vec![
        ("oauth_consumer_key", CONSUMER_KEY.to_string()),
        ("oauth_nonce", nonce),
        ("oauth_signature_method", "HMAC-SHA1".to_string()),
        ("oauth_timestamp", timestamp),
        ("oauth_version", "1.0".to_string()),
    ];

    if let Some(t) = token {
        params.push(("oauth_token", t.to_string()));
    }
    for (k, v) in extra_params {
        params.push((k, v.to_string()));
    }

    params.sort_by(|a, b| a.0.cmp(b.0));

    let param_str = params
        .iter()
        .map(|(k, v)| format!("{}={}", pct(k), pct(v)))
        .collect::<Vec<_>>()
        .join("&");

    let base = format!("{}&{}&{}", method, pct(url), pct(&param_str));
    let signing_key = format!("{}&{}", pct(CONSUMER_SECRET), pct(token_secret));

    let mut mac = HmacSha1::new_from_slice(signing_key.as_bytes()).unwrap();
    mac.update(base.as_bytes());
    let sig = B64.encode(mac.finalize().into_bytes());

    params.push(("oauth_signature", sig));
    params.sort_by(|a, b| a.0.cmp(b.0));

    let header_parts = params
        .iter()
        .map(|(k, v)| format!("{}=\"{}\"", k, pct(v)))
        .collect::<Vec<_>>()
        .join(", ");

    format!("OAuth {}", header_parts)
}

/// Percent-encode RFC 3986 : les caractères non-réservés (A-Z a-z 0-9 - _ . ~) ne sont PAS encodés.
fn pct(s: &str) -> String {
    static OAUTH1_SET: std::sync::OnceLock<percent_encoding::AsciiSet> = std::sync::OnceLock::new();
    let set = OAUTH1_SET.get_or_init(|| {
        percent_encoding::NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'_')
            .remove(b'.')
            .remove(b'~')
    });
    percent_encoding::utf8_percent_encode(s, set).to_string()
}
