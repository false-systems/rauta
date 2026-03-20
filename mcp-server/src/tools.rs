//! MCP Tool Definitions
//!
//! Each tool corresponds to a `GatewayQuery` method. Tool definitions include
//! JSON Schema for parameters and return types (via `schemars`).

use agent_api::query::GatewayQuery;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// MCP tool definition
#[derive(Debug, Clone, Serialize)]
pub struct McpToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: serde_json::Value,
}

/// Parameters for rauta_list_routes tool
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListRoutesParams {
    /// Filter by HTTP method (GET, POST, etc.)
    pub method: Option<String>,
    /// Filter by path prefix
    pub path_prefix: Option<String>,
}

/// Parameters for rauta_get_route tool
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetRouteParams {
    /// Route pattern to look up
    pub pattern: String,
}

/// Parameters for rauta_list_circuit_breakers tool
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListCircuitBreakersParams {
    /// Filter by state (Open, Closed, HalfOpen)
    pub state: Option<String>,
}

/// Parameters for rauta_list_rate_limiters tool
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListRateLimitersParams {
    /// Filter by route pattern
    pub route: Option<String>,
}

/// Parameters for rauta_diagnose tool
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiagnoseParams {
    /// Symptom to diagnose (e.g., "high-latency", "circuit-breaker-cascade")
    pub symptom: String,
    /// Filter by route pattern
    pub route: Option<String>,
    /// Filter by backend address
    pub backend: Option<String>,
}

/// Parameters for rauta_drain_backend tool
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DrainBackendParams {
    /// Backend address to drain (e.g., "10.0.1.5:8080")
    pub backend: String,
    /// Drain timeout in seconds (default: 30)
    pub timeout: Option<u64>,
}

/// Parameters for rauta_undrain_backend tool
#[derive(Debug, Deserialize, JsonSchema)]
pub struct UndrainBackendParams {
    /// Backend address to undrain
    pub backend: String,
}

/// Parameters for rauta_metrics_snapshot tool
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MetricsSnapshotParams {
    /// Filter by metric name
    pub metric: Option<String>,
}

/// MCP tool executor backed by a GatewayQuery implementation
pub struct McpToolExecutor {
    query: Arc<dyn GatewayQuery>,
}

impl McpToolExecutor {
    pub fn new(query: Arc<dyn GatewayQuery>) -> Self {
        Self { query }
    }

    /// List all available MCP tools with their schemas
    pub fn list_tools(&self) -> Vec<McpToolDef> {
        vec![
            McpToolDef {
                name: "rauta_status",
                description: "Get gateway health overview: uptime, route count, open circuits, rate limiter status",
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            },
            McpToolDef {
                name: "rauta_list_routes",
                description: "List all configured routes with backends, filters, and health status",
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "method": {"type": "string", "description": "Filter by HTTP method"},
                        "path_prefix": {"type": "string", "description": "Filter by path prefix"}
                    }
                }),
            },
            McpToolDef {
                name: "rauta_get_route",
                description: "Get detailed information about a single route",
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {"type": "string", "description": "Route pattern to look up"}
                    },
                    "required": ["pattern"]
                }),
            },
            McpToolDef {
                name: "rauta_list_circuit_breakers",
                description: "List circuit breaker states for all backends",
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "state": {"type": "string", "description": "Filter by state (Open, Closed, HalfOpen)"}
                    }
                }),
            },
            McpToolDef {
                name: "rauta_list_rate_limiters",
                description: "List rate limiter states showing tokens available and capacity",
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "route": {"type": "string", "description": "Filter by route pattern"}
                    }
                }),
            },
            McpToolDef {
                name: "rauta_diagnose",
                description: "Run diagnostic rules to detect gateway issues. Returns structured diagnoses with causal chains and suggested actions",
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "symptom": {"type": "string", "description": "Symptom to diagnose"},
                        "route": {"type": "string", "description": "Filter by route"},
                        "backend": {"type": "string", "description": "Filter by backend"}
                    },
                    "required": ["symptom"]
                }),
            },
            McpToolDef {
                name: "rauta_cache_stats",
                description: "Get route cache statistics (hit rate, size)",
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            },
            McpToolDef {
                name: "rauta_list_listeners",
                description: "List active network listeners with protocols and Gateway references",
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            },
            McpToolDef {
                name: "rauta_drain_backend",
                description: "Gracefully drain a backend, preventing new requests while allowing existing connections to finish",
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "backend": {"type": "string", "description": "Backend address (e.g., 10.0.1.5:8080)"},
                        "timeout": {"type": "integer", "description": "Drain timeout in seconds (default: 30)"}
                    },
                    "required": ["backend"]
                }),
            },
            McpToolDef {
                name: "rauta_undrain_backend",
                description: "Cancel drain for a backend, restoring it to active service",
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "backend": {"type": "string", "description": "Backend address to undrain"}
                    },
                    "required": ["backend"]
                }),
            },
            McpToolDef {
                name: "rauta_metrics_snapshot",
                description: "Get Prometheus metrics as structured JSON",
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "metric": {"type": "string", "description": "Filter by metric name"}
                    }
                }),
            },
        ]
    }

    /// Execute an MCP tool by name with JSON parameters
    pub async fn call_tool(
        &self,
        name: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        match name {
            "rauta_status" => {
                let snapshot = self.query.snapshot().await.map_err(|e| e.to_string())?;
                serde_json::to_value(snapshot).map_err(|e| e.to_string())
            }
            "rauta_list_routes" => {
                let p: ListRoutesParams =
                    serde_json::from_value(params).map_err(|e| e.to_string())?;
                let routes = self
                    .query
                    .list_routes(p.method.as_deref(), p.path_prefix.as_deref())
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(routes).map_err(|e| e.to_string())
            }
            "rauta_get_route" => {
                let p: GetRouteParams =
                    serde_json::from_value(params).map_err(|e| e.to_string())?;
                let route = self
                    .query
                    .get_route(&p.pattern)
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(route).map_err(|e| e.to_string())
            }
            "rauta_list_circuit_breakers" => {
                let p: ListCircuitBreakersParams =
                    serde_json::from_value(params).map_err(|e| e.to_string())?;
                let cbs = self
                    .query
                    .list_circuit_breakers(p.state.as_deref())
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(cbs).map_err(|e| e.to_string())
            }
            "rauta_list_rate_limiters" => {
                let p: ListRateLimitersParams =
                    serde_json::from_value(params).map_err(|e| e.to_string())?;
                let rls = self
                    .query
                    .list_rate_limiters(p.route.as_deref())
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(rls).map_err(|e| e.to_string())
            }
            "rauta_diagnose" => {
                let p: DiagnoseParams =
                    serde_json::from_value(params).map_err(|e| e.to_string())?;
                let diagnoses = self
                    .query
                    .diagnose(&p.symptom, p.route.as_deref(), p.backend.as_deref())
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(diagnoses).map_err(|e| e.to_string())
            }
            "rauta_cache_stats" => {
                let stats = self.query.cache_stats().await.map_err(|e| e.to_string())?;
                serde_json::to_value(stats).map_err(|e| e.to_string())
            }
            "rauta_list_listeners" => {
                let listeners = self
                    .query
                    .list_listeners()
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(listeners).map_err(|e| e.to_string())
            }
            "rauta_drain_backend" => {
                let p: DrainBackendParams =
                    serde_json::from_value(params).map_err(|e| e.to_string())?;
                self.query
                    .drain_backend(&p.backend, p.timeout)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::json!({"status": "draining", "backend": p.backend}))
            }
            "rauta_undrain_backend" => {
                let p: UndrainBackendParams =
                    serde_json::from_value(params).map_err(|e| e.to_string())?;
                self.query
                    .undrain_backend(&p.backend)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::json!({"status": "active", "backend": p.backend}))
            }
            "rauta_metrics_snapshot" => {
                let p: MetricsSnapshotParams =
                    serde_json::from_value(params).map_err(|e| e.to_string())?;
                let metrics = self
                    .query
                    .metrics_snapshot(p.metric.as_deref())
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(metrics).map_err(|e| e.to_string())
            }
            _ => Err(format!("Unknown tool: {}", name)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_api::types::*;

    #[test]
    fn test_tool_list_has_all_tools() {
        // Use a mock query (we just need the tool definitions, not actual execution)
        struct MockQuery;

        #[async_trait::async_trait]
        impl GatewayQuery for MockQuery {
            async fn snapshot(&self) -> anyhow::Result<GatewaySnapshot> {
                unimplemented!()
            }
            async fn list_routes(
                &self,
                _: Option<&str>,
                _: Option<&str>,
            ) -> anyhow::Result<Vec<RouteSnapshot>> {
                unimplemented!()
            }
            async fn get_route(&self, _: &str) -> anyhow::Result<Option<RouteSnapshot>> {
                unimplemented!()
            }
            async fn list_circuit_breakers(
                &self,
                _: Option<&str>,
            ) -> anyhow::Result<Vec<CircuitBreakerSnapshot>> {
                unimplemented!()
            }
            async fn list_rate_limiters(
                &self,
                _: Option<&str>,
            ) -> anyhow::Result<Vec<RateLimiterSnapshot>> {
                unimplemented!()
            }
            async fn list_listeners(&self) -> anyhow::Result<Vec<ListenerSnapshot>> {
                unimplemented!()
            }
            async fn cache_stats(&self) -> anyhow::Result<Option<CacheStats>> {
                unimplemented!()
            }
            async fn metrics_snapshot(
                &self,
                _: Option<&str>,
            ) -> anyhow::Result<Vec<MetricSnapshot>> {
                unimplemented!()
            }
            async fn diagnose(
                &self,
                _: &str,
                _: Option<&str>,
                _: Option<&str>,
            ) -> anyhow::Result<Vec<Diagnosis>> {
                unimplemented!()
            }
            async fn drain_backend(&self, _: &str, _: Option<u64>) -> anyhow::Result<()> {
                unimplemented!()
            }
            async fn undrain_backend(&self, _: &str) -> anyhow::Result<()> {
                unimplemented!()
            }
        }

        let executor = McpToolExecutor::new(Arc::new(MockQuery));
        let tools = executor.list_tools();

        assert_eq!(tools.len(), 11, "Should have 11 MCP tools");

        let tool_names: Vec<&str> = tools.iter().map(|t| t.name).collect();
        assert!(tool_names.contains(&"rauta_status"));
        assert!(tool_names.contains(&"rauta_list_routes"));
        assert!(tool_names.contains(&"rauta_diagnose"));
        assert!(tool_names.contains(&"rauta_drain_backend"));
        assert!(tool_names.contains(&"rauta_undrain_backend"));
        assert!(tool_names.contains(&"rauta_metrics_snapshot"));
    }
}
