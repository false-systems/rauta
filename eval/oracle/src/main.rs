//! RAUTA Oracle — Ground Truth Test Binary
//!
//! Validates RAUTA's external behavior against its contract.
//! Connects to a live gateway instance via HTTP and asserts correctness.
//!
//! Run: cargo test --manifest-path eval/oracle/Cargo.toml -- --nocapture
//! Run one: cargo test --manifest-path eval/oracle/Cargo.toml -- case_007 --nocapture
//!
//! Required: RAUTA must be running with at least one route configured.
//!
//! Environment:
//!   RAUTA_PROXY_ENDPOINT   (default: http://localhost:8080)
//!   RAUTA_ADMIN_ENDPOINT   (default: http://localhost:9091)

fn main() {
    eprintln!("rauta-oracle is a test binary — run with:");
    eprintln!("  cargo test --manifest-path eval/oracle/Cargo.toml -- --nocapture");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use std::time::Duration;
    use tokio::time::sleep;

    // ====================================================================
    // Configuration
    // ====================================================================

    /// Proxy endpoint (data plane)
    fn proxy_base() -> String {
        std::env::var("RAUTA_PROXY_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:8080".into())
    }

    /// Admin endpoint (management plane)
    fn admin_base() -> String {
        std::env::var("RAUTA_ADMIN_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:9091".into())
    }

    // Settlement times — how long to wait for async state to propagate
    #[allow(dead_code)] // Used as system matures
    const ROUTE_SETTLE_MS: u64 = 200;
    #[allow(dead_code)]
    const HEALTH_SETTLE_MS: u64 = 500;
    const METRICS_SETTLE_MS: u64 = 100;

    // ====================================================================
    // Helpers
    // ====================================================================

    fn http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("Failed to create HTTP client")
    }

    async fn admin_get(path: &str) -> reqwest::Response {
        let url = format!("{}{}", admin_base(), path);
        http_client()
            .get(&url)
            .send()
            .await
            .unwrap_or_else(|e| panic!("Admin GET {} failed: {}", url, e))
    }

    async fn admin_get_json(path: &str) -> Value {
        let resp = admin_get(path).await;
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .unwrap_or_else(|e| panic!("Failed to read admin response body: {}", e));
        assert!(
            status == 200,
            "Admin GET {} returned {} (expected 200): {}",
            path,
            status,
            body
        );
        serde_json::from_str(&body)
            .unwrap_or_else(|e| panic!("Admin response is not valid JSON: {} — body: {}", e, body))
    }

    async fn admin_post_json(path: &str) -> Value {
        let url = format!("{}{}", admin_base(), path);
        let resp = http_client()
            .post(&url)
            .send()
            .await
            .unwrap_or_else(|e| panic!("Admin POST {} failed: {}", url, e));
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .unwrap_or_else(|e| panic!("Failed to read admin response body: {}", e));
        assert!(
            status == 200,
            "Admin POST {} returned {} (expected 200): {}",
            path,
            status,
            body
        );
        serde_json::from_str(&body)
            .unwrap_or_else(|e| panic!("Admin response is not valid JSON: {} — body: {}", e, body))
    }

    async fn proxy_get(path: &str) -> reqwest::Response {
        let url = format!("{}{}", proxy_base(), path);
        http_client()
            .get(&url)
            .send()
            .await
            .unwrap_or_else(|e| panic!("Proxy GET {} failed: {}", url, e))
    }

    async fn proxy_request(
        method: reqwest::Method,
        path: &str,
        body: Option<&str>,
    ) -> reqwest::Response {
        let url = format!("{}{}", proxy_base(), path);
        let mut builder = http_client().request(method, &url);
        if let Some(b) = body {
            builder = builder.body(b.to_string());
        }
        builder
            .send()
            .await
            .unwrap_or_else(|e| panic!("Proxy request {} failed: {}", url, e))
    }

    // ====================================================================
    // ADMIN API HEALTH (001-003)
    //
    // Validates that the management plane is reachable and returns
    // correctly structured responses. These are the most fundamental
    // tests — if the admin API is down, nothing else matters.
    // ====================================================================

    #[tokio::test]
    async fn case_001_admin_healthz_returns_200() {
        let resp = admin_get("/healthz").await;
        assert_eq!(resp.status().as_u16(), 200, "Admin /healthz must return 200");
    }

    #[tokio::test]
    async fn case_002_admin_status_returns_valid_json() {
        let status = admin_get_json("/api/v1/status").await;

        // Required fields
        assert!(status.get("status").is_some(), "status field missing");
        assert!(
            status.get("uptime_seconds").is_some(),
            "uptime_seconds field missing"
        );
        assert!(
            status.get("route_count").is_some(),
            "route_count field missing"
        );
        assert!(
            status.get("open_circuits").is_some(),
            "open_circuits field missing"
        );
        assert!(
            status.get("exhausted_rate_limiters").is_some(),
            "exhausted_rate_limiters field missing"
        );

        // Status must be "ok"
        assert_eq!(
            status["status"].as_str().unwrap(),
            "ok",
            "Gateway status should be 'ok'"
        );

        // Uptime must parse as a number
        assert!(
            status["uptime_seconds"].as_u64().is_some(),
            "Uptime should be a valid integer"
        );
    }

    #[tokio::test]
    async fn case_003_admin_status_has_cache_stats() {
        let status = admin_get_json("/api/v1/status").await;

        let cache = status
            .get("cache_stats")
            .expect("cache_stats field missing");
        assert!(cache.get("hits").is_some(), "cache_stats.hits missing");
        assert!(cache.get("misses").is_some(), "cache_stats.misses missing");
        assert!(cache.get("size").is_some(), "cache_stats.size missing");
        assert!(
            cache.get("hit_rate").is_some(),
            "cache_stats.hit_rate missing"
        );
    }

    // ====================================================================
    // PROXY HEALTH (004-006)
    //
    // Validates the data plane responds correctly to standard requests.
    // ====================================================================

    #[tokio::test]
    async fn case_004_proxy_healthz_returns_200() {
        let resp = proxy_get("/healthz").await;
        assert_eq!(
            resp.status().as_u16(),
            200,
            "Proxy /healthz must return 200"
        );
    }

    #[tokio::test]
    async fn case_005_proxy_metrics_returns_prometheus() {
        // Generate some traffic first so metrics have data
        let _ = proxy_get("/healthz").await;
        sleep(Duration::from_millis(METRICS_SETTLE_MS)).await;

        let resp = proxy_get("/metrics").await;
        assert_eq!(
            resp.status().as_u16(),
            200,
            "Proxy /metrics must return 200"
        );

        let body = resp.text().await.unwrap();
        // Must contain Prometheus-format metrics (HELP/TYPE headers)
        assert!(
            body.contains("# HELP") || body.contains("# TYPE"),
            "Metrics endpoint must return Prometheus-formatted metrics"
        );
    }

    #[tokio::test]
    async fn case_006_proxy_status_returns_json() {
        let resp = proxy_get("/status").await;
        assert_eq!(
            resp.status().as_u16(),
            200,
            "Proxy /status must return 200"
        );

        let body: Value = resp.json().await.unwrap();
        assert!(body.get("status").is_some(), "status field missing");
        assert!(body.get("routes").is_some(), "routes field missing");
    }

    // ====================================================================
    // ROUTE LISTING (007-009)
    //
    // Validates the admin API returns real route data.
    // ====================================================================

    #[tokio::test]
    async fn case_007_admin_routes_returns_array() {
        let routes = admin_get_json("/api/v1/routes").await;
        assert!(routes.is_array(), "GET /api/v1/routes must return a JSON array");
    }

    #[tokio::test]
    async fn case_008_admin_routes_match_status_count() {
        let status = admin_get_json("/api/v1/status").await;
        let routes = admin_get_json("/api/v1/routes").await;

        let status_count = status["route_count"].as_u64().unwrap();
        let routes_count = routes.as_array().unwrap().len() as u64;

        assert_eq!(
            status_count, routes_count,
            "status.route_count ({}) must match routes array length ({})",
            status_count, routes_count
        );
    }

    #[tokio::test]
    async fn case_009_routes_have_required_fields() {
        let routes = admin_get_json("/api/v1/routes").await;
        let routes_arr = routes.as_array().unwrap();

        // Skip if no routes configured (standalone mode with no backend)
        if routes_arr.is_empty() {
            eprintln!("SKIP: No routes configured — configure RAUTA_BACKEND_ADDR to test route fields");
            return;
        }

        for route in routes_arr {
            assert!(
                route.get("pattern").is_some(),
                "Route missing 'pattern' field: {:?}",
                route
            );
            assert!(
                route.get("method").is_some(),
                "Route missing 'method' field: {:?}",
                route
            );
            assert!(
                route.get("backends").is_some(),
                "Route missing 'backends' field: {:?}",
                route
            );
            assert!(
                route["backends"].is_array(),
                "Route 'backends' must be an array: {:?}",
                route
            );
        }
    }

    // ====================================================================
    // CIRCUIT BREAKER INTROSPECTION (010-011)
    //
    // Validates circuit breaker state is queryable via admin API.
    // ====================================================================

    #[tokio::test]
    async fn case_010_circuit_breakers_initially_empty_or_closed() {
        let status = admin_get_json("/api/v1/status").await;
        let open = status["open_circuits"].as_u64().unwrap();

        // On a fresh instance, no circuits should be open
        assert_eq!(
            open, 0,
            "Fresh gateway should have 0 open circuits, got {}",
            open
        );
    }

    #[tokio::test]
    async fn case_011_rate_limiters_initially_not_exhausted() {
        let status = admin_get_json("/api/v1/status").await;
        let exhausted = status["exhausted_rate_limiters"].as_u64().unwrap();

        assert_eq!(
            exhausted, 0,
            "Fresh gateway should have 0 exhausted rate limiters, got {}",
            exhausted
        );
    }

    // ====================================================================
    // DIAGNOSTICS (012-013)
    //
    // Validates the diagnostics engine runs and returns structured results.
    // ====================================================================

    #[tokio::test]
    async fn case_012_diagnose_returns_array() {
        let diagnoses =
            admin_post_json("/api/v1/diagnose?symptom=circuit-breaker-cascade").await;
        assert!(
            diagnoses.is_array(),
            "POST /api/v1/diagnose must return a JSON array"
        );
    }

    #[tokio::test]
    async fn case_013_diagnose_healthy_gateway_has_no_critical() {
        let diagnoses = admin_post_json("/api/v1/diagnose?symptom=degraded").await;
        let arr = diagnoses.as_array().unwrap();

        let critical_count = arr
            .iter()
            .filter(|d| d["severity"].as_str() == Some("critical"))
            .count();

        assert_eq!(
            critical_count, 0,
            "Healthy gateway should have 0 critical diagnoses, got {}",
            critical_count
        );
    }

    // ====================================================================
    // PROXY ROUTING (014-016)
    //
    // Validates the proxy correctly routes requests and returns proper
    // error codes for unrouted paths.
    // ====================================================================

    #[tokio::test]
    async fn case_014_unrouted_path_returns_error() {
        let resp = proxy_get("/this-path-definitely-does-not-exist-12345").await;
        let status = resp.status().as_u16();
        // In standalone mode with a catch-all "/" route, unrouted paths hit the backend.
        // If the backend is down, this returns 502. If no route exists, 404.
        // Either way, it should NOT return 200 (that would mean false routing).
        assert_ne!(
            status, 200,
            "Unrouted/unreachable path must not return 200 — got 200"
        );
    }

    #[tokio::test]
    async fn case_015_metrics_not_recorded_for_metrics_endpoint() {
        // Hit /metrics twice, then check that the metrics endpoint itself
        // doesn't appear in the request count metrics
        proxy_get("/metrics").await;
        proxy_get("/metrics").await;
        sleep(Duration::from_millis(METRICS_SETTLE_MS)).await;

        let resp = proxy_get("/metrics").await;
        let body = resp.text().await.unwrap();

        // /metrics requests should NOT be counted in http_requests_total
        // (they're excluded from metrics recording in handle_request)
        let metrics_path_counted = body
            .lines()
            .any(|l| l.contains("http_requests_total") && l.contains("/metrics"));

        assert!(
            !metrics_path_counted,
            "Requests to /metrics should not be recorded in http_requests_total"
        );
    }

    #[tokio::test]
    async fn case_016_healthz_not_recorded_in_metrics() {
        proxy_get("/healthz").await;
        sleep(Duration::from_millis(METRICS_SETTLE_MS)).await;

        let resp = proxy_get("/metrics").await;
        let body = resp.text().await.unwrap();

        let healthz_counted = body
            .lines()
            .any(|l| l.contains("http_requests_total") && l.contains("/healthz"));

        assert!(
            !healthz_counted,
            "Requests to /healthz should not be recorded in http_requests_total"
        );
    }

    // ====================================================================
    // RESILIENCE (017-019)
    //
    // Validates the proxy handles malformed/adversarial input correctly
    // without crashing or leaking errors.
    // ====================================================================

    #[tokio::test]
    async fn case_017_admin_unknown_endpoint_returns_404() {
        let resp = admin_get("/api/v1/nonexistent").await;
        assert_eq!(
            resp.status().as_u16(),
            404,
            "Unknown admin endpoint must return 404"
        );
    }

    #[tokio::test]
    async fn case_018_proxy_survives_empty_post() {
        let resp =
            proxy_request(reqwest::Method::POST, "/some-path", Some("")).await;
        // Should return 404 (no route) or some other code — NOT crash
        let status = resp.status().as_u16();
        assert!(
            status >= 200 && status < 600,
            "Proxy should return valid HTTP status for empty POST, got {}",
            status
        );
    }

    #[tokio::test]
    async fn case_019_admin_diagnose_with_empty_symptom_returns_array() {
        let diagnoses = admin_post_json("/api/v1/diagnose?symptom=").await;
        assert!(
            diagnoses.is_array(),
            "Diagnose with empty symptom should still return array"
        );
    }

    // ====================================================================
    // ADMIN/PROXY ISOLATION (020-021)
    //
    // Validates that admin and proxy are on separate ports and don't
    // interfere with each other.
    // ====================================================================

    #[tokio::test]
    async fn case_020_admin_and_proxy_are_different_ports() {
        let proxy = proxy_base();
        let admin = admin_base();
        assert_ne!(
            proxy, admin,
            "Proxy and admin must be on different endpoints"
        );
    }

    #[tokio::test]
    async fn case_021_proxy_does_not_serve_admin_api() {
        // The proxy should NOT serve /api/v1/status — that's admin-only
        let resp = proxy_get("/api/v1/status").await;
        let status = resp.status().as_u16();
        // Should be 404 (no route) not 200 (admin response)
        assert_ne!(
            status, 200,
            "Proxy must not serve admin API endpoints — got 200 for /api/v1/status"
        );
    }
}
