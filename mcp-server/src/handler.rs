//! MCP Server Handler (rmcp)
//!
//! Implements `rmcp::ServerHandler` for RAUTA gateway tools.
//! Wraps any `GatewayQuery` implementation — `LocalGatewayQuery` for in-process
//! access, `RemoteGatewayQuery` for HTTP-based access.
//!
//! Usage:
//! ```rust,ignore
//! let handler = RautaMcpHandler::new(query);
//! rmcp::ServiceExt::serve(handler, rmcp::transport::stdio()).await?;
//! ```

use agent_api::query::GatewayQuery;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;

// ============================================================================
// Parameter types for MCP tools
// These use rmcp's schemars (1.x) for schema generation in tool definitions
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListRoutesParams {
    /// Filter by HTTP method (GET, POST, etc.)
    pub method: Option<String>,
    /// Filter by path prefix
    pub path_prefix: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetRouteParams {
    /// Route pattern to look up
    pub pattern: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListCircuitBreakersParams {
    /// Filter by state: Open, Closed, or HalfOpen
    pub state: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListRateLimitersParams {
    /// Filter by route pattern
    pub route: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiagnoseParams {
    /// Symptom to diagnose (e.g., "high-latency", "circuit-breaker-cascade")
    pub symptom: String,
    /// Filter by route pattern
    pub route: Option<String>,
    /// Filter by backend address
    pub backend: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DrainBackendParams {
    /// Backend address to drain (e.g., "10.0.1.5:8080")
    pub backend: String,
    /// Drain timeout in seconds (default: 30)
    pub timeout: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UndrainBackendParams {
    /// Backend address to undrain
    pub backend: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MetricsSnapshotParams {
    /// Filter by metric name
    pub metric: Option<String>,
}

// ============================================================================
// MCP Handler
// ============================================================================

/// RAUTA MCP server handler
///
/// Each `#[tool]` method maps to a `GatewayQuery` method.
/// rmcp generates JSON Schema from the parameter types and handles
/// JSON-RPC framing over stdio.
#[derive(Clone)]
pub struct RautaMcpHandler {
    query: Arc<dyn GatewayQuery>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl RautaMcpHandler {
    pub fn new(query: Arc<dyn GatewayQuery>) -> Self {
        Self {
            query,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Get gateway health overview: uptime, route count, open circuits, rate limiter status"
    )]
    async fn rauta_status(&self) -> Result<CallToolResult, McpError> {
        let snapshot = self
            .query
            .snapshot()
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let json = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "List all configured routes with backends, filters, and health status")]
    async fn rauta_list_routes(
        &self,
        Parameters(params): Parameters<ListRoutesParams>,
    ) -> Result<CallToolResult, McpError> {
        let routes = self
            .query
            .list_routes(params.method.as_deref(), params.path_prefix.as_deref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let json = serde_json::to_string_pretty(&routes)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Get detailed information about a single route")]
    async fn rauta_get_route(
        &self,
        Parameters(params): Parameters<GetRouteParams>,
    ) -> Result<CallToolResult, McpError> {
        let route = self
            .query
            .get_route(&params.pattern)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let json = serde_json::to_string_pretty(&route)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "List circuit breaker states for all backends")]
    async fn rauta_list_circuit_breakers(
        &self,
        Parameters(params): Parameters<ListCircuitBreakersParams>,
    ) -> Result<CallToolResult, McpError> {
        let breakers = self
            .query
            .list_circuit_breakers(params.state.as_deref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let json = serde_json::to_string_pretty(&breakers)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "List rate limiter states showing tokens available and capacity")]
    async fn rauta_list_rate_limiters(
        &self,
        Parameters(params): Parameters<ListRateLimitersParams>,
    ) -> Result<CallToolResult, McpError> {
        let limiters = self
            .query
            .list_rate_limiters(params.route.as_deref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let json = serde_json::to_string_pretty(&limiters)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Run diagnostic rules to detect gateway issues. Returns structured diagnoses with causal chains and suggested actions"
    )]
    async fn rauta_diagnose(
        &self,
        Parameters(params): Parameters<DiagnoseParams>,
    ) -> Result<CallToolResult, McpError> {
        let diagnoses = self
            .query
            .diagnose(
                &params.symptom,
                params.route.as_deref(),
                params.backend.as_deref(),
            )
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let json = serde_json::to_string_pretty(&diagnoses)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Get route cache statistics (hit rate, size)")]
    async fn rauta_cache_stats(&self) -> Result<CallToolResult, McpError> {
        let stats = self
            .query
            .cache_stats()
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let json = serde_json::to_string_pretty(&stats)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "List active network listeners with protocols and Gateway references")]
    async fn rauta_list_listeners(&self) -> Result<CallToolResult, McpError> {
        let listeners = self
            .query
            .list_listeners()
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let json = serde_json::to_string_pretty(&listeners)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Get Prometheus metrics as structured JSON")]
    async fn rauta_metrics_snapshot(
        &self,
        Parameters(params): Parameters<MetricsSnapshotParams>,
    ) -> Result<CallToolResult, McpError> {
        let metrics = self
            .query
            .metrics_snapshot(params.metric.as_deref())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let json = serde_json::to_string_pretty(&metrics)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Gracefully drain a backend, preventing new requests while allowing existing connections to finish"
    )]
    async fn rauta_drain_backend(
        &self,
        Parameters(params): Parameters<DrainBackendParams>,
    ) -> Result<CallToolResult, McpError> {
        self.query
            .drain_backend(&params.backend, params.timeout)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let json = serde_json::json!({"status": "draining", "backend": params.backend});
        Ok(CallToolResult::success(vec![Content::text(
            json.to_string(),
        )]))
    }

    #[tool(description = "Cancel drain for a backend, restoring it to active service")]
    async fn rauta_undrain_backend(
        &self,
        Parameters(params): Parameters<UndrainBackendParams>,
    ) -> Result<CallToolResult, McpError> {
        self.query
            .undrain_backend(&params.backend)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let json = serde_json::json!({"status": "active", "backend": params.backend});
        Ok(CallToolResult::success(vec![Content::text(
            json.to_string(),
        )]))
    }
}

#[tool_handler]
impl ServerHandler for RautaMcpHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "RAUTA AI-native Kubernetes API gateway. Query routes, backends, \
                 circuit breakers, rate limiters, and run diagnostics."
                .to_string(),
        )
    }
}
