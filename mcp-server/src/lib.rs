//! RAUTA MCP Server
//!
//! Provides MCP (Model Context Protocol) tools for AI agents to query and manage
//! the RAUTA gateway. Uses the `agent-api` crate's `GatewayQuery` trait for
//! transport-agnostic access to gateway state.
//!
//! ## MCP Tools
//!
//! | Tool | Description |
//! |---|---|
//! | `rauta_status` | Health overview: uptime, route count, open circuits |
//! | `rauta_list_routes` | All routes with backends |
//! | `rauta_get_route` | Single route detail |
//! | `rauta_metrics_snapshot` | Prometheus metrics as JSON |
//! | `rauta_list_circuit_breakers` | Circuit breaker states |
//! | `rauta_list_rate_limiters` | Rate limiter state |
//! | `rauta_diagnose` | Run diagnostics |
//! | `rauta_cache_stats` | Route cache stats |
//! | `rauta_list_listeners` | Active listeners |
//! | `rauta_drain_backend` | Graceful drain (destructive) |
//! | `rauta_undrain_backend` | Cancel drain |
//!
//! ## Transports
//!
//! - **stdio**: For Claude Code / Cursor integration (`control --mcp-stdio`)
//! - **Streamable HTTP**: `POST /mcp` on admin port 9091 (future)
//!
//! ## Usage
//!
//! The MCP server wraps a `GatewayQuery` implementation. In-process, this is
//! `LocalGatewayQuery`. For remote access, `RemoteGatewayQuery` (from rauta-cli).

pub mod handler;
