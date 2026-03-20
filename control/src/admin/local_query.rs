//! Local Gateway Query Implementation
//!
//! Reads directly from `Arc<Router>`, `Arc<CircuitBreakerManager>`, and `Arc<RateLimiter>`.
//! Used by the admin server and MCP server when running in-process.

use agent_api::query::GatewayQuery;
use agent_api::types::*;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Instant;

use crate::proxy::circuit_breaker::CircuitBreakerManager;
use crate::proxy::rate_limiter::RateLimiter;
use crate::proxy::router::Router;

/// Local query implementation that reads from shared gateway state
#[allow(dead_code)]
pub struct LocalGatewayQuery {
    router: Arc<Router>,
    circuit_breaker: Arc<CircuitBreakerManager>,
    rate_limiter: Arc<RateLimiter>,
    start_time: Instant,
}

impl LocalGatewayQuery {
    pub fn new(
        router: Arc<Router>,
        circuit_breaker: Arc<CircuitBreakerManager>,
        rate_limiter: Arc<RateLimiter>,
    ) -> Self {
        Self {
            router,
            circuit_breaker,
            rate_limiter,
            start_time: Instant::now(),
        }
    }
}

#[async_trait]
impl GatewayQuery for LocalGatewayQuery {
    async fn snapshot(&self) -> anyhow::Result<GatewaySnapshot> {
        let route_count = self.router.route_count();
        let uptime = self.start_time.elapsed().as_secs();

        Ok(GatewaySnapshot {
            status: "ok".to_string(),
            uptime_seconds: uptime,
            route_count,
            open_circuits: 0, // Would need snapshot from CircuitBreakerManager
            exhausted_rate_limiters: 0,
            listeners: vec![],
            cache_stats: Some(self.cache_stats_internal()),
        })
    }

    async fn list_routes(
        &self,
        _method_filter: Option<&str>,
        _path_prefix: Option<&str>,
    ) -> anyhow::Result<Vec<RouteSnapshot>> {
        // The router doesn't expose a full route list yet — placeholder
        Ok(vec![])
    }

    async fn get_route(&self, _pattern: &str) -> anyhow::Result<Option<RouteSnapshot>> {
        Ok(None)
    }

    async fn list_circuit_breakers(
        &self,
        _state_filter: Option<&str>,
    ) -> anyhow::Result<Vec<CircuitBreakerSnapshot>> {
        // CircuitBreakerManager doesn't expose iteration yet — placeholder
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
        Ok(Some(self.cache_stats_internal()))
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
        use agent_api::diagnostics::engine::{DiagnosticContext, DiagnosticsEngine};

        let snapshot = self.snapshot().await?;
        let ctx = DiagnosticContext {
            snapshot,
            routes: vec![],
            circuit_breakers: vec![],
            rate_limiters: vec![],
        };

        let engine = DiagnosticsEngine::with_builtin_rules();
        Ok(engine.diagnose_symptom(&ctx, symptom))
    }

    async fn drain_backend(
        &self,
        _backend: &str,
        _timeout_secs: Option<u64>,
    ) -> anyhow::Result<()> {
        anyhow::bail!("drain_backend not yet implemented via admin API")
    }

    async fn undrain_backend(&self, _backend: &str) -> anyhow::Result<()> {
        anyhow::bail!("undrain_backend not yet implemented via admin API")
    }
}

impl LocalGatewayQuery {
    fn cache_stats_internal(&self) -> CacheStats {
        let (hits, misses) = self.router.get_cache_stats();
        let size = self.router.get_cache_size();
        let total = hits + misses;
        let hit_rate = if total > 0 {
            hits as f64 / total as f64
        } else {
            0.0
        };

        CacheStats {
            hits,
            misses,
            size,
            hit_rate,
        }
    }
}
