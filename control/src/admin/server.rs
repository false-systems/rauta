//! Admin HTTP Server
//!
//! Serves management endpoints on port 9091 (configurable via RAUTA_ADMIN_PORT).
//! Separate from the proxy port (8080) — management traffic never competes with data plane.

use crate::admin::local_query::LocalGatewayQuery;
use agent_api::query::GatewayQuery;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{error, info};

/// Admin server that serves management REST API
pub struct AdminServer {
    query: Arc<LocalGatewayQuery>,
    bind_addr: SocketAddr,
}

impl AdminServer {
    pub fn new(query: LocalGatewayQuery, bind_addr: SocketAddr) -> Self {
        Self {
            query: Arc::new(query),
            bind_addr,
        }
    }

    /// Start the admin server (runs until cancelled)
    pub async fn serve(self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(self.bind_addr).await?;
        info!("Admin server listening on {}", self.bind_addr);

        loop {
            let (stream, _remote) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let query = Arc::clone(&self.query);

            tokio::spawn(async move {
                let service = service_fn(move |req| {
                    let query = Arc::clone(&query);
                    async move { handle_admin_request(req, query).await }
                });

                if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                    error!("Admin connection error: {}", e);
                }
            });
        }
    }
}

async fn handle_admin_request(
    req: Request<hyper::body::Incoming>,
    query: Arc<LocalGatewayQuery>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    let path = req.uri().path().to_string();
    let method = req.method().clone();

    let response = match (method.as_str(), path.as_str()) {
        ("GET", "/api/v1/status") => handle_status(&query).await,
        ("GET", "/api/v1/routes") => handle_list_routes(&query).await,
        ("GET", "/api/v1/cache") => handle_cache_stats(&query).await,
        ("POST", "/api/v1/diagnose") => {
            // Read symptom from query string or body
            let symptom = req
                .uri()
                .query()
                .and_then(|q| {
                    q.split('&')
                        .find(|p| p.starts_with("symptom="))
                        .map(|p| p.trim_start_matches("symptom=").to_string())
                })
                .unwrap_or_else(|| "degraded".to_string());

            handle_diagnose(&query, &symptom).await
        }
        ("GET", "/healthz") => json_response(StatusCode::OK, r#"{"status":"ok"}"#),
        _ => json_response(StatusCode::NOT_FOUND, r#"{"error":"not found"}"#),
    };

    Ok(response)
}

async fn handle_status(query: &LocalGatewayQuery) -> Response<BoxBody<Bytes, hyper::Error>> {
    match query.snapshot().await {
        Ok(snapshot) => match serde_json::to_string(&snapshot) {
            Ok(json) => json_response(StatusCode::OK, &json),
            Err(e) => json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &serde_json::json!({"error": format!("serialization failed: {}", e)}).to_string(),
            ),
        },
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &serde_json::json!({"error": e.to_string()}).to_string(),
        ),
    }
}

async fn handle_list_routes(query: &LocalGatewayQuery) -> Response<BoxBody<Bytes, hyper::Error>> {
    match query.list_routes(None, None).await {
        Ok(routes) => match serde_json::to_string(&routes) {
            Ok(json) => json_response(StatusCode::OK, &json),
            Err(e) => json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &serde_json::json!({"error": format!("serialization failed: {}", e)}).to_string(),
            ),
        },
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &serde_json::json!({"error": e.to_string()}).to_string(),
        ),
    }
}

async fn handle_cache_stats(query: &LocalGatewayQuery) -> Response<BoxBody<Bytes, hyper::Error>> {
    match query.cache_stats().await {
        Ok(stats) => match serde_json::to_string(&stats) {
            Ok(json) => json_response(StatusCode::OK, &json),
            Err(e) => json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &serde_json::json!({"error": format!("serialization failed: {}", e)}).to_string(),
            ),
        },
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &serde_json::json!({"error": e.to_string()}).to_string(),
        ),
    }
}

async fn handle_diagnose(
    query: &LocalGatewayQuery,
    symptom: &str,
) -> Response<BoxBody<Bytes, hyper::Error>> {
    match query.diagnose(symptom, None, None).await {
        Ok(diagnoses) => match serde_json::to_string(&diagnoses) {
            Ok(json) => json_response(StatusCode::OK, &json),
            Err(e) => json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &serde_json::json!({"error": format!("serialization failed: {}", e)}).to_string(),
            ),
        },
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &serde_json::json!({"error": e.to_string()}).to_string(),
        ),
    }
}

#[allow(clippy::unwrap_used)]
fn json_response(status: StatusCode, body: &str) -> Response<BoxBody<Bytes, hyper::Error>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(
            Full::new(Bytes::from(body.to_string()))
                .map_err(|never| match never {})
                .boxed(),
        )
        .unwrap()
}
