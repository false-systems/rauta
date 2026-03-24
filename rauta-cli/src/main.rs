//! RAUTA CLI — Gateway management tool and kubectl plugin
//!
//! Supports three output formats:
//! - `table` (default): Human-readable table output
//! - `json`: Machine-readable JSON
//! - `agent`: Compact self-describing JSON with `_meta` and `_hints` for LLM consumption

mod output;
mod remote_query;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "rauta", version, about = "RAUTA gateway management CLI")]
struct Cli {
    /// Output format
    #[arg(long, default_value = "table", global = true)]
    format: OutputFormat,

    /// Admin server endpoint
    #[arg(
        long,
        default_value = "http://localhost:9091",
        global = true,
        env = "RAUTA_ADMIN_ENDPOINT"
    )]
    endpoint: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, ValueEnum)]
enum OutputFormat {
    Table,
    Json,
    Agent,
}

#[derive(Subcommand)]
enum Commands {
    /// Show gateway status overview
    Status,

    /// Route management
    Routes {
        #[command(subcommand)]
        action: RouteAction,
    },

    /// Backend management
    Backends {
        #[command(subcommand)]
        action: BackendAction,
    },

    /// Metrics queries
    Metrics {
        #[command(subcommand)]
        action: MetricsAction,
    },

    /// Run diagnostics
    Diagnose {
        /// Symptom to diagnose (e.g., "high-latency", "circuit-breaker-cascade")
        symptom: String,

        /// Filter by route pattern
        #[arg(long)]
        route: Option<String>,
    },

    /// Start MCP server over stdio (for Claude Code / Cursor integration)
    Mcp,
}

#[derive(Subcommand)]
enum RouteAction {
    /// List all routes
    List {
        /// Filter by HTTP method
        #[arg(long)]
        method: Option<String>,
    },
    /// Get route details
    Get {
        /// Route pattern
        pattern: String,
    },
}

#[derive(Subcommand)]
enum BackendAction {
    /// Show backend health
    Health {
        /// Filter by route
        #[arg(long)]
        route: Option<String>,
    },
    /// Drain a backend (graceful removal)
    Drain {
        /// Backend address (e.g., "10.0.1.5:8080")
        backend: String,
        /// Drain timeout in seconds
        #[arg(long, default_value = "30")]
        timeout: u64,
    },
    /// Cancel drain for a backend
    Undrain {
        /// Backend address
        backend: String,
    },
}

#[derive(Subcommand)]
enum MetricsAction {
    /// Snapshot of current metrics
    Snapshot,
    /// Query a specific metric
    Query {
        /// Metric name
        metric: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let client = remote_query::RemoteGatewayQuery::new(&cli.endpoint);

    match cli.command {
        Commands::Status => {
            let status = client.get_status().await?;
            output::render_status(&status, &cli.format);
        }
        Commands::Routes { action } => match action {
            RouteAction::List { method } => {
                let routes = client.get_routes(method.as_deref()).await?;
                output::render_routes(&routes, &cli.format);
            }
            RouteAction::Get { pattern } => {
                let route = client.get_route(&pattern).await?;
                output::render_route_detail(&route, &cli.format);
            }
        },
        Commands::Backends { action } => match action {
            BackendAction::Health { route: _ } => {
                let status = client.get_status().await?;
                output::render_status(&status, &cli.format);
            }
            BackendAction::Drain {
                backend,
                timeout: _,
            } => {
                println!("Draining backend {}...", backend);
            }
            BackendAction::Undrain { backend } => {
                println!("Undraining backend {}...", backend);
            }
        },
        Commands::Metrics { action } => match action {
            MetricsAction::Snapshot => {
                let status = client.get_status().await?;
                output::render_status(&status, &cli.format);
            }
            MetricsAction::Query { metric } => {
                println!("Querying metric: {}", metric);
            }
        },
        Commands::Diagnose { symptom, route: _ } => {
            let diagnoses = client.diagnose(&symptom).await?;
            output::render_diagnoses(&diagnoses, &cli.format);
        }
        Commands::Mcp => {
            // MCP server over stdio — stdout is the protocol channel, logs go to stderr
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_ansi(false)
                .with_env_filter(
                    tracing_subscriber::EnvFilter::from_default_env()
                        .add_directive(tracing::Level::INFO.into()),
                )
                .init();

            tracing::info!("Starting RAUTA MCP server (stdio transport)");
            tracing::info!("Admin endpoint: {}", cli.endpoint);

            let query: std::sync::Arc<dyn agent_api::query::GatewayQuery> =
                std::sync::Arc::new(remote_query::RemoteGatewayQuery::new(&cli.endpoint));
            let handler = mcp_server::handler::RautaMcpHandler::new(query);

            let service = rmcp::ServiceExt::serve(handler, rmcp::transport::stdio())
                .await
                .map_err(|e| anyhow::anyhow!("MCP serve error: {}", e))?;

            tracing::info!("MCP server running — waiting for client");
            service
                .waiting()
                .await
                .map_err(|e| anyhow::anyhow!("MCP wait error: {}", e))?;

            return Ok(());
        }
    }

    Ok(())
}
