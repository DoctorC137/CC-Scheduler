use anyhow::{bail, Result};
use tracing::debug;

/// Client Clever Cloud utilisant un service token Biscuit (Bearer auth).
/// Toutes les opérations sont scoped à l'organisation configurée.
pub struct CleverClient {
    http: reqwest::Client,
    /// https://api.clever-cloud.com/v2/organisations/{org_id}
    org_base: String,
    token: String,
}

impl CleverClient {
    pub fn new(org_id: &str, token: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();
        Self {
            http,
            org_base: format!(
                "https://api.clever-cloud.com/v2/organisations/{}",
                org_id
            ),
            token: token.to_string(),
        }
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.token)
    }

    async fn get(&self, path: &str) -> Result<serde_json::Value> {
        let url = format!("{}{}", self.org_base, path);
        debug!(url, "CC API GET");
        let resp = self
            .http
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("CC API error {}: {}", status, body);
        }
        Ok(resp.json().await?)
    }

    async fn post_empty(&self, path: &str) -> Result<()> {
        let url = format!("{}{}", self.org_base, path);
        let resp = self
            .http
            .post(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("CC API error {}: {}", status, body);
        }
        Ok(())
    }

    async fn delete_req(&self, path: &str) -> Result<()> {
        let url = format!("{}{}", self.org_base, path);
        let resp = self
            .http
            .delete(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("CC API error {}: {}", status, body);
        }
        Ok(())
    }

    /// Infos de l'organisation (nom, etc.)
    pub async fn get_org(&self) -> Result<serde_json::Value> {
        self.get("").await
    }

    /// Liste des applications de l'organisation
    pub async fn list_apps(&self) -> Result<serde_json::Value> {
        self.get("/applications").await
    }

    /// Démarre une application
    pub async fn start_app(&self, app_id: &str) -> Result<()> {
        let path = format!("/applications/{}/instances", app_id);
        debug!(app_id, "Starting application");
        self.post_empty(&path).await
    }

    /// Stoppe une application
    pub async fn stop_app(&self, app_id: &str) -> Result<()> {
        let path = format!("/applications/{}/instances", app_id);
        debug!(app_id, "Stopping application");
        self.delete_req(&path).await
    }
}
