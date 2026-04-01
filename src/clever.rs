use anyhow::{bail, Result};
use tracing::{debug, info};

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

    async fn put_json(&self, path: &str, body: &serde_json::Value) -> Result<()> {
        let url = format!("{}{}", self.org_base, path);
        debug!(url, "CC API PUT");
        let resp = self
            .http
            .put(&url)
            .header("Authorization", self.auth_header())
            .json(body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("CC API error {}: {}", status, body);
        }
        Ok(())
    }

    async fn post_empty(&self, path: &str) -> Result<()> {
        let url = format!("{}{}", self.org_base, path);
        debug!(url, "CC API POST");
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
        debug!(url, "CC API DELETE");
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

    /// Stoppe une application.
    ///
    /// CC redémarre automatiquement si minInstances ≥ 1.
    /// On passe donc d'abord minInstances à 0 via l'API de scalabilité,
    /// puis on supprime les instances en cours.
    pub async fn stop_app(&self, app_id: &str) -> Result<()> {
        info!(app_id, "Stopping application");

        // 1. Récupère la config actuelle pour préserver les flavors
        let app = self.get(&format!("/applications/{}", app_id)).await?;
        let inst = &app["instance"];
        let max_instances = inst["maxInstances"].as_i64().unwrap_or(1);
        let min_flavor = inst["minFlavor"]["name"].as_str().unwrap_or("nano");
        let max_flavor = inst["maxFlavor"]["name"].as_str().unwrap_or("nano");
        let homogeneous = inst["homogeneous"].as_bool().unwrap_or(false);

        // 2. minInstances → 0 pour empêcher le redémarrage automatique
        self.put_json(
            &format!("/applications/{}/scalability", app_id),
            &serde_json::json!({
                "minInstances": 0,
                "maxInstances": max_instances,
                "minFlavor": min_flavor,
                "maxFlavor": max_flavor,
                "homogeneous": homogeneous,
            }),
        ).await?;

        // 3. Supprime les instances en cours
        self.delete_req(&format!("/applications/{}/instances", app_id)).await?;

        info!(app_id, "Application stopped");
        Ok(())
    }

    /// Démarre une application.
    ///
    /// Remet minInstances à 1 (si elle était à 0 après un stop),
    /// puis déclenche un déploiement.
    pub async fn start_app(&self, app_id: &str) -> Result<()> {
        info!(app_id, "Starting application");

        // 1. Récupère la config actuelle
        let app = self.get(&format!("/applications/{}", app_id)).await?;
        let inst = &app["instance"];
        let max_instances = inst["maxInstances"].as_i64().unwrap_or(1).max(1);
        let min_flavor = inst["minFlavor"]["name"].as_str().unwrap_or("nano");
        let max_flavor = inst["maxFlavor"]["name"].as_str().unwrap_or("nano");
        let homogeneous = inst["homogeneous"].as_bool().unwrap_or(false);

        // 2. minInstances → 1 pour autoriser le démarrage
        self.put_json(
            &format!("/applications/{}/scalability", app_id),
            &serde_json::json!({
                "minInstances": 1,
                "maxInstances": max_instances,
                "minFlavor": min_flavor,
                "maxFlavor": max_flavor,
                "homogeneous": homogeneous,
            }),
        ).await?;

        // 3. Déclenche le déploiement
        self.post_empty(&format!("/applications/{}/instances", app_id)).await?;

        info!(app_id, "Application started");
        Ok(())
    }
}
