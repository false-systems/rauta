//! Output rendering for different formats (table, json, agent)

use agent_api::types::{Diagnosis, GatewaySnapshot, RouteSnapshot};
use comfy_table::{Cell, Table};

use crate::OutputFormat;

pub fn render_status(snapshot: &GatewaySnapshot, format: &OutputFormat) {
    match format {
        OutputFormat::Table => {
            let mut table = Table::new();
            table.set_header(vec!["Field", "Value"]);
            table.add_row(vec!["Status", &snapshot.status]);
            table.add_row(vec!["Uptime", &format_duration(snapshot.uptime_seconds)]);
            table.add_row(vec!["Routes", &snapshot.route_count.to_string()]);
            table.add_row(vec!["Open Circuits", &snapshot.open_circuits.to_string()]);
            table.add_row(vec![
                "Exhausted Rate Limiters",
                &snapshot.exhausted_rate_limiters.to_string(),
            ]);

            if let Some(cache) = &snapshot.cache_stats {
                table.add_row(vec![
                    "Cache Hit Rate",
                    &format!("{:.1}%", cache.hit_rate * 100.0),
                ]);
            }

            println!("{table}");
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(snapshot).unwrap_or_default();
            println!("{json}");
        }
        OutputFormat::Agent => {
            let agent_output = serde_json::json!({
                "_meta": {
                    "tool": "rauta",
                    "command": "status",
                    "format": "agent"
                },
                "_hints": {
                    "summary": format!("Gateway {} with {} routes, {} open circuits",
                        snapshot.status, snapshot.route_count, snapshot.open_circuits),
                    "actions": if snapshot.open_circuits > 0 {
                        vec!["Run `rauta diagnose circuit-breaker-cascade` for details"]
                    } else {
                        vec![]
                    }
                },
                "data": snapshot
            });
            println!(
                "{}",
                serde_json::to_string(&agent_output).unwrap_or_default()
            );
        }
    }
}

pub fn render_routes(routes: &[RouteSnapshot], format: &OutputFormat) {
    match format {
        OutputFormat::Table => {
            if routes.is_empty() {
                println!("No routes configured");
                return;
            }

            let mut table = Table::new();
            table.set_header(vec!["Method", "Pattern", "Backends", "Filters"]);

            for route in routes {
                let mut filters = Vec::new();
                if route.has_request_filters {
                    filters.push("req-headers");
                }
                if route.has_response_filters {
                    filters.push("resp-headers");
                }
                if route.has_redirect {
                    filters.push("redirect");
                }
                if route.has_timeout {
                    filters.push("timeout");
                }
                if route.has_retry {
                    filters.push("retry");
                }

                table.add_row(vec![
                    Cell::new(&route.method),
                    Cell::new(&route.pattern),
                    Cell::new(route.backends.len()),
                    Cell::new(if filters.is_empty() {
                        "-".to_string()
                    } else {
                        filters.join(", ")
                    }),
                ]);
            }

            println!("{table}");
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(routes).unwrap_or_default();
            println!("{json}");
        }
        OutputFormat::Agent => {
            let agent_output = serde_json::json!({
                "_meta": { "tool": "rauta", "command": "routes list", "format": "agent" },
                "_hints": {
                    "summary": format!("{} routes configured", routes.len()),
                },
                "data": routes
            });
            println!(
                "{}",
                serde_json::to_string(&agent_output).unwrap_or_default()
            );
        }
    }
}

pub fn render_route_detail(route: &Option<RouteSnapshot>, format: &OutputFormat) {
    match route {
        Some(route) => match format {
            OutputFormat::Table => {
                println!("Route: {} {}", route.method, route.pattern);
                println!("Backends: {}", route.backends.len());
                for backend in &route.backends {
                    println!(
                        "  {}:{} (weight={}, draining={})",
                        backend.address, backend.port, backend.weight, backend.is_draining
                    );
                }
            }
            OutputFormat::Json | OutputFormat::Agent => {
                let json = serde_json::to_string_pretty(route).unwrap_or_default();
                println!("{json}");
            }
        },
        None => println!("Route not found"),
    }
}

pub fn render_diagnoses(diagnoses: &[Diagnosis], format: &OutputFormat) {
    match format {
        OutputFormat::Table => {
            if diagnoses.is_empty() {
                println!("No issues detected");
                return;
            }

            for diagnosis in diagnoses {
                println!(
                    "[{:?}] {} ({})",
                    diagnosis.severity, diagnosis.rule_id, diagnosis.symptom
                );
                for cause in &diagnosis.causal_chain {
                    println!("  Cause: {}", cause);
                }
                for evidence in &diagnosis.evidence {
                    println!("  Evidence: {}", evidence);
                }
                for action in &diagnosis.suggested_actions {
                    if let Some(cmd) = &action.cli_command {
                        println!("  Action: {} ({})", action.description, cmd);
                    } else {
                        println!("  Action: {}", action.description);
                    }
                }
                println!();
            }
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(diagnoses).unwrap_or_default();
            println!("{json}");
        }
        OutputFormat::Agent => {
            let critical_count = diagnoses
                .iter()
                .filter(|d| d.severity == agent_api::types::Severity::Critical)
                .count();
            let agent_output = serde_json::json!({
                "_meta": { "tool": "rauta", "command": "diagnose", "format": "agent" },
                "_hints": {
                    "summary": format!("{} issues found ({} critical)", diagnoses.len(), critical_count),
                    "urgent": critical_count > 0,
                },
                "data": diagnoses
            });
            println!(
                "{}",
                serde_json::to_string(&agent_output).unwrap_or_default()
            );
        }
    }
}

fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!(
            "{}h {}m {}s",
            seconds / 3600,
            (seconds % 3600) / 60,
            seconds % 60
        )
    }
}
