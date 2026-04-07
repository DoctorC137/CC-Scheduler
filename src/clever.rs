use anyhow::{bail, Result};
use tracing::{debug, info};

/// Clever Cloud API client scoped to a single organisation.
/// All requests use a Biscuit service token (Bearer auth).
pub struct CleverClient {
    http: reqwest::Client,
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

    pub async fn get_org(&self) -> Result<serde_json::Value> {
        self.get("").await
    }

    pub async fn list_apps(&self) -> Result<serde_json::Value> {
        self.get("/applications").await
    }

    /// Stop an application.
    ///
    /// Clever Cloud automatically restarts any app whose `minInstances >= 1`,
    /// so we must set it to 0 before deleting running instances — otherwise
    /// the platform immediately schedules a new deployment.
    pub async fn stop_app(&self, app_id: &str) -> Result<()> {
        info!(app_id, "Stopping application");

        let app = self.get(&format!("/applications/{}", app_id)).await?;
        let inst = &app["instance"];
        let max_instances = inst["maxInstances"].as_i64().unwrap_or(1);
        let min_flavor = inst["minFlavor"]["name"].as_str().unwrap_or("nano");
        let max_flavor = inst["maxFlavor"]["name"].as_str().unwrap_or("nano");
        let homogeneous = inst["homogeneous"].as_bool().unwrap_or(false);

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

        self.delete_req(&format!("/applications/{}/instances", app_id)).await?;

        info!(app_id, "Application stopped");
        Ok(())
    }

    /// Start an application.
    ///
    /// Re-enables auto-restart by setting `minInstances` back to 1,
    /// then triggers a new deployment.
    pub async fn start_app(&self, app_id: &str) -> Result<()> {
        info!(app_id, "Starting application");

        let app = self.get(&format!("/applications/{}", app_id)).await?;
        let inst = &app["instance"];
        let max_instances = inst["maxInstances"].as_i64().unwrap_or(1).max(1);
        let min_flavor = inst["minFlavor"]["name"].as_str().unwrap_or("nano");
        let max_flavor = inst["maxFlavor"]["name"].as_str().unwrap_or("nano");
        let homogeneous = inst["homogeneous"].as_bool().unwrap_or(false);

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

    #[tokio::test]
    async fn stop_app_correct_sequence() {
        let server = MockServer::start().await;

        Mock::given(method("GET")).and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(app_json(2)))
            .expect(1).mount(&server).await;
        Mock::given(method("PUT")).and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1).mount(&server).await;
        Mock::given(method("DELETE")).and(path("/applications/app-1/instances"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1).mount(&server).await;

        client(&server.uri()).stop_app("app-1").await.unwrap();

        let reqs = server.received_requests().await.unwrap();
        assert_eq!(reqs[0].method.as_str(), "GET");
        assert_eq!(reqs[1].method.as_str(), "PUT");
        assert_eq!(reqs[2].method.as_str(), "DELETE");
    }

    #[tokio::test]
    async fn stop_app_sets_min_instances_zero() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(app_json(2)))
            .mount(&server).await;
        Mock::given(method("PUT")).and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200)).mount(&server).await;
        Mock::given(method("DELETE")).and(path("/applications/app-1/instances"))
            .respond_with(ResponseTemplate::new(204)).mount(&server).await;

        client(&server.uri()).stop_app("app-1").await.unwrap();

        let reqs = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&reqs[1].body).unwrap();
        assert_eq!(body["minInstances"], 0);
    }

    #[tokio::test]
    async fn stop_app_preserves_max_instances() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(app_json(5)))
            .mount(&server).await;
        Mock::given(method("PUT")).and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200)).mount(&server).await;
        Mock::given(method("DELETE")).and(path("/applications/app-1/instances"))
            .respond_with(ResponseTemplate::new(204)).mount(&server).await;

        client(&server.uri()).stop_app("app-1").await.unwrap();

        let reqs = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&reqs[1].body).unwrap();
        assert_eq!(body["maxInstances"], 5);
    }

    #[tokio::test]
    async fn start_app_correct_sequence() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(app_json(2)))
            .expect(1).mount(&server).await;
        Mock::given(method("PUT")).and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1).mount(&server).await;
        Mock::given(method("POST")).and(path("/applications/app-1/instances"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1).mount(&server).await;

        client(&server.uri()).start_app("app-1").await.unwrap();

        let reqs = server.received_requests().await.unwrap();
        assert_eq!(reqs[0].method.as_str(), "GET");
        assert_eq!(reqs[1].method.as_str(), "PUT");
        assert_eq!(reqs[2].method.as_str(), "POST");
    }

    #[tokio::test]
    async fn start_app_sets_min_instances_one() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(app_json(2)))
            .mount(&server).await;
        Mock::given(method("PUT")).and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200)).mount(&server).await;
        Mock::given(method("POST")).and(path("/applications/app-1/instances"))
            .respond_with(ResponseTemplate::new(200)).mount(&server).await;

        client(&server.uri()).start_app("app-1").await.unwrap();

        let reqs = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&reqs[1].body).unwrap();
        assert_eq!(body["minInstances"], 1);
    }

    #[tokio::test]
    async fn start_app_ensures_at_least_one_max_instance() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(app_json(0)))
            .mount(&server).await;
        Mock::given(method("PUT")).and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(200)).mount(&server).await;
        Mock::given(method("POST")).and(path("/applications/app-1/instances"))
            .respond_with(ResponseTemplate::new(200)).mount(&server).await;

        client(&server.uri()).start_app("app-1").await.unwrap();

        let reqs = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&reqs[1].body).unwrap();
        assert_eq!(body["maxInstances"], 1);
    }

    #[tokio::test]
    async fn stop_app_propagates_api_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
            .mount(&server).await;

        let result = client(&server.uri()).stop_app("app-1").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("403"));
    }

    #[tokio::test]
    async fn start_app_propagates_api_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/applications/app-1"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&server).await;

        let result = client(&server.uri()).start_app("app-1").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("500"));
    }

    // Real integration tests — require CC_ORG_ID and CC_SERVICE_TOKEN in env (or .env).
    // Run with: cargo test -- --ignored --test-threads=1

    fn real_client() -> Option<(CleverClient, String)> {
        dotenvy::dotenv().ok();
        let org_id = std::env::var("CC_ORG_ID").ok()?;
        let token = std::env::var("CC_SERVICE_TOKEN").ok()?;
        let base = format!("https://api.clever-cloud.com/v2/organisations/{}", org_id);
        Some((CleverClient { http: reqwest::Client::new(), org_base: base, token }, org_id))
    }

    const TEST_APP_ID: &str = "app_0575225d-7864-435d-80eb-39f2a78299d3";

    #[tokio::test]
    #[ignore = "requires real Clever Cloud credentials"]
    async fn integration_stop_sets_min_instances_zero() {
        let (cc, _) = real_client().expect("CC_ORG_ID and CC_SERVICE_TOKEN required");
        cc.stop_app(TEST_APP_ID).await.expect("stop_app failed");
        let app = cc.get(&format!("/applications/{}", TEST_APP_ID)).await.unwrap();
        assert_eq!(app["instance"]["minInstances"].as_i64().unwrap_or(-1), 0);
    }

    #[tokio::test]
    #[ignore = "requires real Clever Cloud credentials"]
    async fn integration_start_sets_min_instances_one() {
        let (cc, _) = real_client().expect("CC_ORG_ID and CC_SERVICE_TOKEN required");
        cc.start_app(TEST_APP_ID).await.expect("start_app failed");
        let app = cc.get(&format!("/applications/{}", TEST_APP_ID)).await.unwrap();
        assert!(app["instance"]["minInstances"].as_i64().unwrap_or(-1) >= 1);
    }

    #[tokio::test]
    #[ignore = "requires real Clever Cloud credentials"]
    async fn integration_full_stop_start_cycle() {
        let (cc, _) = real_client().expect("CC_ORG_ID and CC_SERVICE_TOKEN required");

        cc.stop_app(TEST_APP_ID).await.expect("stop_app failed");
        let app = cc.get(&format!("/applications/{}", TEST_APP_ID)).await.unwrap();
        assert_eq!(app["instance"]["minInstances"].as_i64().unwrap_or(-1), 0);

        cc.start_app(TEST_APP_ID).await.expect("start_app failed");
        let app = cc.get(&format!("/applications/{}", TEST_APP_ID)).await.unwrap();
        assert!(app["instance"]["minInstances"].as_i64().unwrap_or(-1) >= 1);
    }
}
