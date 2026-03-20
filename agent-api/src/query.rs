//! Gateway Query Trait
//!
//! Defines the abstract interface for querying gateway state.
//! Two implementations:
//! 1. `LocalGatewayQuery` (in control crate) — reads from `Arc<Router>` directly
//! 2. `RemoteGatewayQuery` (in rauta-cli crate) — HTTP/Unix socket client

use crate::types::{
    CacheStats, CircuitBreakerSnapshot, Diagnosis, GatewaySnapshot, ListenerSnapshot,
    MetricSnapshot, RateLimiterSnapshot, RouteSnapshot,
};
use async_trait::async_trait;

/// Abstract interface for querying gateway state
///
/// All methods are read-only except `drain_backend` and `undrain_backend`.
#[async_trait]
pub trait GatewayQuery: Send + Sync {
    /// Get gateway status overview
    async fn snapshot(&self) -> anyhow::Result<GatewaySnapshot>;

    /// List all configured routes
    async fn list_routes(
        &self,
        method_filter: Option<&str>,
        path_prefix: Option<&str>,
    ) -> anyhow::Result<Vec<RouteSnapshot>>;

    /// Get a single route by pattern
    async fn get_route(&self, pattern: &str) -> anyhow::Result<Option<RouteSnapshot>>;

    /// List circuit breaker states
    async fn list_circuit_breakers(
        &self,
        state_filter: Option<&str>,
    ) -> anyhow::Result<Vec<CircuitBreakerSnapshot>>;

    /// List rate limiter states
    async fn list_rate_limiters(
        &self,
        route_filter: Option<&str>,
    ) -> anyhow::Result<Vec<RateLimiterSnapshot>>;

    /// List active listeners
    async fn list_listeners(&self) -> anyhow::Result<Vec<ListenerSnapshot>>;

    /// Get route cache statistics
    async fn cache_stats(&self) -> anyhow::Result<Option<CacheStats>>;

    /// Get metrics snapshot
    async fn metrics_snapshot(
        &self,
        metric_filter: Option<&str>,
    ) -> anyhow::Result<Vec<MetricSnapshot>>;

    /// Run diagnostics for a symptom
    async fn diagnose(
        &self,
        symptom: &str,
        route_filter: Option<&str>,
        backend_filter: Option<&str>,
    ) -> anyhow::Result<Vec<Diagnosis>>;

    /// Drain a backend (graceful removal)
    async fn drain_backend(&self, backend: &str, timeout_secs: Option<u64>) -> anyhow::Result<()>;

    /// Cancel drain for a backend
    async fn undrain_backend(&self, backend: &str) -> anyhow::Result<()>;
}
