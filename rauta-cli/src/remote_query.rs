//! Remote Gateway Query
//!
//! HTTP client that talks to the RAUTA admin server REST API.

use agent_api::types::{Diagnosis, GatewaySnapshot, RouteSnapshot};

pub struct RemoteGatewayQuery {
    base_url: String,
    client: reqwest::Client,
}

impl RemoteGatewayQuery {
    pub fn new(endpoint: &str) -> Self {
        Self {
            base_url: endpoint.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub async fn get_status(&self) -> anyhow::Result<GatewaySnapshot> {
        let url = format!("{}/api/v1/status", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let snapshot: GatewaySnapshot = resp.json().await?;
        Ok(snapshot)
    }

    pub async fn get_routes(
        &self,
        _method_filter: Option<&str>,
    ) -> anyhow::Result<Vec<RouteSnapshot>> {
        let url = format!("{}/api/v1/routes", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let routes: Vec<RouteSnapshot> = resp.json().await?;
        Ok(routes)
    }

    pub async fn get_route(&self, _pattern: &str) -> anyhow::Result<Option<RouteSnapshot>> {
        // Placeholder — admin server doesn't have this endpoint yet
        Ok(None)
    }

    pub async fn diagnose(&self, symptom: &str) -> anyhow::Result<Vec<Diagnosis>> {
        let url = format!("{}/api/v1/diagnose?symptom={}", self.base_url, symptom);
        let resp = self.client.post(&url).send().await?;
        let diagnoses: Vec<Diagnosis> = resp.json().await?;
        Ok(diagnoses)
    }
}
