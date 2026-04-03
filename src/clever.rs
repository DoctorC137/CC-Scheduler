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
            &format!("/applications/{}", app_id),
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
            &format!("/applications/{}", app_id),
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

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn app_json(max_instances: i64) -> serde_json::Value {
        serde_json::json!({
            "instance": {
                "maxInstances": max_instances,
                "minFlavor": {"name": "nano"},
                "maxFlavor": {"name": "nano"},
                "homogeneous": false
            }
        })
    }

    fn client(base_url: &str) -> CleverClient {
        CleverClient {
            http: reqwest::Client::new(),
            org_base: base_url.to_string(),
            token: "test-token".to_string(),
        }
    }

    // ── stop_app ──────────────────────────────────────────────────────────────

    /// stop_app doit : GET app → PUT scalability(minInstances=0) → DELETE instances
    #[tokio::test]
    async fn stop_app_correct_sequence() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(app_json(2)))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/applications/app-1/scalability"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("DELETE"))
            .and(path("/applications/app-1/instances"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        client(&server.uri()).stop_app("app-1").await.unwrap();

        let reqs = server.received_requests().await.unwrap();
        assert_eq!(reqs.len(), 3, "attendu GET + PUT + DELETE");
        assert_eq!(reqs[0].method.as_str(), "GET");
        assert_eq!(reqs[1].method.as_str(), "PUT");
        assert_eq!(reqs[2].method.as_str(), "DELETE");
    }

    /// stop_app doit envoyer minInstances=0 dans le PUT
    #[tokio::test]
    async fn stop_app_sets_min_instances_zero() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(app_json(2)))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/applications/app-1/scalability"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/applications/app-1/instances"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        client(&server.uri()).stop_app("app-1").await.unwrap();

        let reqs = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&reqs[1].body).unwrap();
        assert_eq!(body["minInstances"], 0, "stop_app doit passer minInstances=0");
    }

    /// stop_app doit préserver maxInstances de la config originale
    #[tokio::test]
    async fn stop_app_preserves_max_instances() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(app_json(5)))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/applications/app-1/scalability"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/applications/app-1/instances"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        client(&server.uri()).stop_app("app-1").await.unwrap();

        let reqs = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&reqs[1].body).unwrap();
        assert_eq!(body["maxInstances"], 5, "maxInstances doit être préservé depuis la config originale");
    }

    // ── start_app ─────────────────────────────────────────────────────────────

    /// start_app doit : GET app → PUT scalability(minInstances=1) → POST instances
    #[tokio::test]
    async fn start_app_correct_sequence() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(app_json(2)))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/applications/app-1/scalability"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/applications/app-1/instances"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        client(&server.uri()).start_app("app-1").await.unwrap();

        let reqs = server.received_requests().await.unwrap();
        assert_eq!(reqs.len(), 3, "attendu GET + PUT + POST");
        assert_eq!(reqs[0].method.as_str(), "GET");
        assert_eq!(reqs[1].method.as_str(), "PUT");
        assert_eq!(reqs[2].method.as_str(), "POST");
    }

    /// start_app doit envoyer minInstances=1 dans le PUT
    #[tokio::test]
    async fn start_app_sets_min_instances_one() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(app_json(2)))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/applications/app-1/scalability"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/applications/app-1/instances"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        client(&server.uri()).start_app("app-1").await.unwrap();

        let reqs = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&reqs[1].body).unwrap();
        assert_eq!(body["minInstances"], 1, "start_app doit passer minInstances=1");
    }

    /// start_app doit garantir maxInstances >= 1 même si la config retourne 0
    #[tokio::test]
    async fn start_app_ensures_at_least_one_max_instance() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(app_json(0)))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/applications/app-1/scalability"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/applications/app-1/instances"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        client(&server.uri()).start_app("app-1").await.unwrap();

        let reqs = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&reqs[1].body).unwrap();
        assert_eq!(
            body["maxInstances"], 1,
            "maxInstances doit être au moins 1 pour permettre le démarrage"
        );
    }

    // ── erreurs API ───────────────────────────────────────────────────────────

    /// stop_app doit retourner une erreur si l'API retourne 4xx/5xx
    #[tokio::test]
    async fn stop_app_propagates_api_error() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
            .mount(&server)
            .await;

        let result = client(&server.uri()).stop_app("app-1").await;
        assert!(result.is_err(), "stop_app doit propager l'erreur API");
        assert!(result.unwrap_err().to_string().contains("403"));
    }

    /// start_app doit retourner une erreur si l'API retourne 4xx/5xx
    #[tokio::test]
    async fn start_app_propagates_api_error() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&server)
            .await;

        let result = client(&server.uri()).start_app("app-1").await;
        assert!(result.is_err(), "start_app doit propager l'erreur API");
        assert!(result.unwrap_err().to_string().contains("500"));
    }

    // ── tests réels contre l'API Clever Cloud ─────────────────────────────────
    // Requièrent CC_ORG_ID et CC_SERVICE_TOKEN dans l'environnement (ou .env).
    // Lancés avec : cargo test -- --ignored

    fn real_client() -> Option<(CleverClient, String)> {
        dotenvy::dotenv().ok();
        let org_id = std::env::var("CC_ORG_ID").ok()?;
        let token = std::env::var("CC_SERVICE_TOKEN").ok()?;
        let base = format!("https://api.clever-cloud.com/v2/organisations/{}", org_id);
        Some((CleverClient { http: reqwest::Client::new(), org_base: base, token }, org_id))
    }

    /// App de test (ne pas utiliser le CC-Scheduler lui-même).
    /// "Deploiement Test Vinext"
    const TEST_APP_ID: &str = "app_0575225d-7864-435d-80eb-39f2a78299d3";

    #[tokio::test]
    #[ignore = "requiert credentials Clever Cloud réels"]
    async fn integration_stop_sets_min_instances_zero() {
        let (cc, _) = real_client().expect("CC_ORG_ID et CC_SERVICE_TOKEN requis");

        cc.stop_app(TEST_APP_ID).await
            .expect("stop_app a échoué sur l'API réelle");

        // Vérifie que minInstances est bien 0 après le stop
        let app = cc.get(&format!("/applications/{}", TEST_APP_ID)).await.unwrap();
        let min = app["instance"]["minInstances"].as_i64().unwrap_or(-1);
        assert_eq!(min, 0, "minInstances doit être 0 après stop_app");
    }

    #[tokio::test]
    #[ignore = "requiert credentials Clever Cloud réels"]
    async fn integration_start_sets_min_instances_one() {
        let (cc, _) = real_client().expect("CC_ORG_ID et CC_SERVICE_TOKEN requis");

        cc.start_app(TEST_APP_ID).await
            .expect("start_app a échoué sur l'API réelle");

        // Vérifie que minInstances est bien >= 1 après le start
        let app = cc.get(&format!("/applications/{}", TEST_APP_ID)).await.unwrap();
        let min = app["instance"]["minInstances"].as_i64().unwrap_or(-1);
        assert!(min >= 1, "minInstances doit être >= 1 après start_app, got {}", min);
    }

    #[tokio::test]
    #[ignore = "requiert credentials Clever Cloud réels"]
    async fn integration_full_stop_start_cycle() {
        let (cc, _) = real_client().expect("CC_ORG_ID et CC_SERVICE_TOKEN requis");

        // Stop
        cc.stop_app(TEST_APP_ID).await.expect("stop_app a échoué");
        let app = cc.get(&format!("/applications/{}", TEST_APP_ID)).await.unwrap();
        let min_after_stop = app["instance"]["minInstances"].as_i64().unwrap_or(-1);
        assert_eq!(min_after_stop, 0, "minInstances doit être 0 après stop");

        // Start
        cc.start_app(TEST_APP_ID).await.expect("start_app a échoué");
        let app = cc.get(&format!("/applications/{}", TEST_APP_ID)).await.unwrap();
        let min_after_start = app["instance"]["minInstances"].as_i64().unwrap_or(-1);
        assert!(min_after_start >= 1, "minInstances doit être >= 1 après start, got {}", min_after_start);
    }
}
