use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use hmac::{Hmac, Mac};
use rand::Rng;
use sha1::Sha1;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

use crate::config::AppConfig;

// Clever Cloud / clever-tools public OAuth1 consumer credentials
const CONSUMER_KEY: &str = "T5nFjKeHH4AIlEveuGhB5S3xg8T19e";
const CONSUMER_SECRET: &str = "MgVMqTr6fWlf2M0tkC2MXOnhfqBWDT";

type HmacSha1 = Hmac<Sha1>;

pub struct CleverClient {
    http: reqwest::Client,
    token: String,
    token_secret: String,
}

impl CleverClient {
    pub fn new(cfg: &AppConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self {
            http,
            token: cfg.cc_oauth_token.clone(),
            token_secret: cfg.cc_oauth_secret.clone(),
        })
    }

    /// Construit le header Authorization OAuth1 pour une requête GET
    fn oauth1_header(&self, method: &str, url: &str) -> String {
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

        let mut params = vec![
            ("oauth_consumer_key", CONSUMER_KEY.to_string()),
            ("oauth_nonce", nonce.clone()),
            ("oauth_signature_method", "HMAC-SHA1".to_string()),
            ("oauth_timestamp", timestamp.clone()),
            ("oauth_token", self.token.clone()),
            ("oauth_version", "1.0".to_string()),
        ];

        // Tri alphabétique requis par OAuth1
        params.sort_by(|a, b| a.0.cmp(b.0));

        let param_str = params
            .iter()
            .map(|(k, v)| format!("{}={}", pct(k), pct(v)))
            .collect::<Vec<_>>()
            .join("&");

        let base = format!("{}&{}&{}", method, pct(url), pct(&param_str));
        let signing_key = format!("{}&{}", pct(CONSUMER_SECRET), pct(&self.token_secret));

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

    async fn get(&self, url: &str) -> Result<serde_json::Value> {
        let auth = self.oauth1_header("GET", url);
        debug!(url, "CC API GET");

        let resp = self.http
            .get(url)
            .header("Authorization", auth)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("CC API error {}: {}", status, body);
        }

        Ok(resp.json().await?)
    }

    async fn post_empty(&self, url: &str) -> Result<()> {
        let auth = self.oauth1_header("POST", url);

        let resp = self.http
            .post(url)
            .header("Authorization", auth)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("CC API error {}: {}", status, body);
        }
        Ok(())
    }

    async fn delete_req(&self, url: &str) -> Result<()> {
        let auth = self.oauth1_header("DELETE", url);

        let resp = self.http
            .delete(url)
            .header("Authorization", auth)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("CC API error {}: {}", status, body);
        }
        Ok(())
    }

    /// Liste les organisations accessibles
    pub async fn list_orgs(&self) -> Result<serde_json::Value> {
        self.get("https://api.clever-cloud.com/v2/organisations").await
    }

    /// Liste les applications d'une organisation
    pub async fn list_apps(&self, org_id: &str) -> Result<serde_json::Value> {
        let url = format!(
            "https://api.clever-cloud.com/v2/organisations/{}/applications",
            org_id
        );
        self.get(&url).await
    }

    /// Démarre une application
    pub async fn start_app(&self, org_id: &str, app_id: &str) -> Result<()> {
        let url = format!(
            "https://api.clever-cloud.com/v2/organisations/{}/applications/{}/instances",
            org_id, app_id
        );
        debug!(org_id, app_id, "Starting application");
        self.post_empty(&url).await
    }

    /// Stoppe une application
    pub async fn stop_app(&self, org_id: &str, app_id: &str) -> Result<()> {
        let url = format!(
            "https://api.clever-cloud.com/v2/organisations/{}/applications/{}/instances",
            org_id, app_id
        );
        debug!(org_id, app_id, "Stopping application");
        self.delete_req(&url).await
    }
}

/// Percent-encode selon RFC 3986 (requis par OAuth1)
/// Les caractères non-réservés (A-Z a-z 0-9 - _ . ~) ne doivent PAS être encodés.
fn pct(s: &str) -> String {
    // NON_ALPHANUMERIC encode tout sauf les alphanumériques.
    // On retire les 4 caractères non-réservés restants : - _ . ~
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
