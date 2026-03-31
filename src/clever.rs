use anyhow::{bail, Result};
use tracing::debug;

use crate::config::AppConfig;

/// Client pour interagir avec l'API Clever Cloud.
///
/// Utilise l'API token (le plus simple pour une app interne à une orga).
/// Pour un usage multi-utilisateurs, basculer sur OAuth1 via clevercloud-sdk.
pub struct CleverClient {
    http: reqwest::Client,
    token: String,
    base_url: String,
}

impl CleverClient {
    pub fn new(cfg: &AppConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self {
            http,
            token: cfg.cc_api_token.clone(),
            base_url: "https://api-bridge.clever-cloud.com".to_string(),
        })
    }

    /// Démarre une application (POST /v2/organisations/{org}/applications/{app}/instances)
    pub async fn start_app(&self, org_id: &str, app_id: &str) -> Result<()> {
        let url = format!(
            "{}/v2/organisations/{}/applications/{}/instances",
            self.base_url, org_id, app_id
        );
        debug!(org_id, app_id, "Starting application");

        let resp = self.http
            .post(&url)
            .bearer_auth(&self.token)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("start_app failed: {} - {}", status, body);
        }
        Ok(())
    }

    /// Stoppe une application (DELETE /v2/organisations/{org}/applications/{app}/instances)
    pub async fn stop_app(&self, org_id: &str, app_id: &str) -> Result<()> {
        let url = format!(
            "{}/v2/organisations/{}/applications/{}/instances",
            self.base_url, org_id, app_id
        );
        debug!(org_id, app_id, "Stopping application");

        let resp = self.http
            .delete(&url)
            .bearer_auth(&self.token)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("stop_app failed: {} - {}", status, body);
        }
        Ok(())
    }

    /// Liste les organisations accessibles via le token
    pub async fn list_orgs(&self) -> Result<serde_json::Value> {
        let url = format!("{}/v2/self/organisations", self.base_url);

        let resp = self.http
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("list_orgs failed: {} - {}", status, body);
        }

        Ok(resp.json().await?)
    }

    /// Liste les applications d'une organisation
    pub async fn list_apps(&self, org_id: &str) -> Result<serde_json::Value> {
        let url = format!(
            "{}/v2/organisations/{}/applications",
            self.base_url, org_id
        );

        let resp = self.http
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("list_apps failed: {} - {}", status, body);
        }

        Ok(resp.json().await?)
    }
}
