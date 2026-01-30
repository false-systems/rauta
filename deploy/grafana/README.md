# RAUTA Grafana Dashboard

Pre-built Grafana dashboard for monitoring RAUTA.

## Installation

1. Import `rauta-dashboard.json` into Grafana (Dashboards → Import)
2. Configure Prometheus data source pointing to RAUTA's `/metrics` endpoint

## Panels

### Traffic Overview
- **Request Rate** - Requests per second
- **Error Rate** - 5xx percentage with thresholds (green <1%, yellow 1-5%, red >5%)
- **Latency Percentiles** - p50, p95, p99

### Backend Health
- **Backend Health Status** - Healthy/Unhealthy per backend
- **Circuit Breaker State** - Closed/Open/Half-Open per backend
- **Health Check Probes** - Success/failure rate

### Connection Pool
- **Active Connections** - Current connections per backend
- **Connection Failures** - Failure rate per backend

### Rate Limiting
- **Rate Limit Decisions** - Allowed vs denied requests
- **Tokens Available** - Current bucket tokens per route

### Workers
- **Requests by Worker** - Load distribution across workers

## Endpoints

| Endpoint | Purpose |
|----------|---------|
| `/metrics` | Prometheus scrape target |
| `/healthz` | Kubernetes liveness/readiness |
| `/status` | JSON status summary |

## `/status` Response

```json
{
  "status": "ok",
  "uptime_seconds": 3600,
  "routes": 5
}
```

## Prometheus Scrape Config

```yaml
scrape_configs:
  - job_name: 'rauta'
    static_configs:
      - targets: ['rauta:9090']
```

## Key Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `http_requests_total` | Counter | Total requests (by method, path, status, worker) |
| `http_request_duration_seconds` | Histogram | Latency distribution |
| `circuit_breaker_state` | Gauge | 0=Closed, 1=Open, 2=Half-Open |
| `backend_health_status` | Gauge | 0=Unhealthy, 1=Healthy |
| `rate_limit_requests_total` | Counter | Rate limit decisions |
| `pool_connections_active` | Gauge | Active backend connections |
