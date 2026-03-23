//! Remote Gateway Query
//!
//! HTTP client that talks to the RAUTA admin server REST API.
//! Implements `GatewayQuery` trait so it can be used with the MCP handler.

use agent_api::query::GatewayQuery;
use agent_api::types::*;
use async_trait::async_trait;

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

    // Convenience methods for direct CLI use (without going through the trait)

    pub async fn get_status(&self) -> anyhow::Result<GatewaySnapshot> {
        self.snapshot().await
    }

    pub async fn get_routes(
        &self,
        method_filter: Option<&str>,
    ) -> anyhow::Result<Vec<RouteSnapshot>> {
        self.list_routes(method_filter, None).await
    }

    pub async fn get_route(&self, pattern: &str) -> anyhow::Result<Option<RouteSnapshot>> {
        GatewayQuery::get_route(self, pattern).await
    }

    pub async fn diagnose(&self, symptom: &str) -> anyhow::Result<Vec<Diagnosis>> {
        GatewayQuery::diagnose(self, symptom, None, None).await
    }
}

#[async_trait]
impl GatewayQuery for RemoteGatewayQuery {
    async fn snapshot(&self) -> anyhow::Result<GatewaySnapshot> {
        let url = format!("{}/api/v1/status", self.base_url);
        let resp = self.client.get(&url).send().await?.error_for_status()?;
        Ok(resp.json().await?)
    }

    async fn list_routes(
        &self,
        _method_filter: Option<&str>,
        _path_prefix: Option<&str>,
    ) -> anyhow::Result<Vec<RouteSnapshot>> {
        let url = format!("{}/api/v1/routes", self.base_url);
        let resp = self.client.get(&url).send().await?.error_for_status()?;
        Ok(resp.json().await?)
    }

    async fn get_route(&self, _pattern: &str) -> anyhow::Result<Option<RouteSnapshot>> {
        Ok(None)
    }

    async fn list_circuit_breakers(
        &self,
        _state_filter: Option<&str>,
    ) -> anyhow::Result<Vec<CircuitBreakerSnapshot>> {
        Ok(vec![])
    }

    async fn list_rate_limiters(
        &self,
        _route_filter: Option<&str>,
    ) -> anyhow::Result<Vec<RateLimiterSnapshot>> {
        Ok(vec![])
    }

    async fn list_listeners(&self) -> anyhow::Result<Vec<ListenerSnapshot>> {
        Ok(vec![])
    }

    async fn cache_stats(&self) -> anyhow::Result<Option<CacheStats>> {
        Ok(None)
    }

    async fn metrics_snapshot(
        &self,
        _metric_filter: Option<&str>,
    ) -> anyhow::Result<Vec<MetricSnapshot>> {
        Ok(vec![])
    }

    async fn diagnose(
        &self,
        symptom: &str,
        _route_filter: Option<&str>,
        _backend_filter: Option<&str>,
    ) -> anyhow::Result<Vec<Diagnosis>> {
        let url = format!("{}/api/v1/diagnose", self.base_url);
        let resp = self
            .client
            .post(&url)
            .query(&[("symptom", symptom)])
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json().await?)
    }

    async fn drain_backend(
        &self,
        _backend: &str,
        _timeout_secs: Option<u64>,
    ) -> anyhow::Result<()> {
        anyhow::bail!("drain_backend not yet implemented via remote query")
    }

    async fn undrain_backend(&self, _backend: &str) -> anyhow::Result<()> {
        anyhow::bail!("undrain_backend not yet implemented via remote query")
    }
}
