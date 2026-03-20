<p align="center">
  <br>
  <strong>R A U T A</strong>
  <br>
  <em>iron</em> — AI-native Kubernetes API gateway
  <br>
  <br>
  <a href="https://github.com/false-systems/rauta/actions/workflows/ci.yml"><img src="https://github.com/false-systems/rauta/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <img src="https://img.shields.io/badge/tests-220%2B-brightgreen" alt="Tests">
  <img src="https://img.shields.io/badge/rust-1.83%2B-f74c00" alt="Rust">
  <img src="https://img.shields.io/badge/license-Apache%202.0-blue" alt="License">
</p>

---

Rust Kubernetes Gateway API controller + L7 HTTP proxy. Lock-free hot path. AI agents are first-class operators.

```
cargo build --release -p control     # gateway
cargo build --release -p rauta-cli   # CLI
cargo test --workspace               # 220+ tests
```

## What it does

RAUTA sits between your clients and backends. It routes HTTP traffic using Kubernetes Gateway API resources, load-balances with Maglev consistent hashing, and lets AI agents query and operate it via MCP.

```
                  Client
                    │
          ┌─────── ▼ ───────┐
          │     RAUTA        │
          │                  │
          │  Route → Maglev  │──── :9091 Admin API ◄── AI Agent / CLI
          │  Filter → Forward│         │
          │  TLS ─ HTTP/2    │    MCP Tools (11)
          │                  │    Diagnostics Engine
          └──┬────┬────┬─────┘    Prometheus Metrics
             │    │    │
             ▼    ▼    ▼
          Backend Backend Backend
```

## Performance

The hot path (every request) uses **zero locks** for health checks and **zero heap allocations** for routing:

| Component | Before | After |
|-----------|--------|-------|
| Circuit breaker | 5 RwLocks | 1 AtomicU64 (CAS) |
| Rate limiter | 3 RwLocks | 1 AtomicU64 (CAS) |
| Health checks | 2 RwLocks | 1 ArcSwap load (~1ns) |
| Backend index tracking | HashSet (heap) | u32 bitmask (stack) |
| Hop-by-hop header check | to_lowercase() (heap) | eq_ignore_ascii_case (stack) |
| Route filter cloning | Deep clone | Arc::clone (~1ns) |

```
select_backend()            →  1 RwLock read (route table) + 1 atomic load (health)
circuit_breaker.allow()     →  1 atomic load
rate_limiter.try_acquire()  →  1 CAS loop
```

## Agent API

RAUTA is queryable and operable by AI agents. Three interfaces, same data:

**MCP Server** — 11 tools for Claude Code, Cursor, or any MCP client:
```
rauta_status              rauta_list_routes         rauta_get_route
rauta_list_circuit_breakers   rauta_list_rate_limiters   rauta_diagnose
rauta_drain_backend       rauta_undrain_backend     rauta_cache_stats
rauta_list_listeners      rauta_metrics_snapshot
```

**CLI** — human and machine output:
```bash
rauta status                          # table output (default)
rauta routes list --format=json       # machine-readable
rauta diagnose circuit-breaker-cascade --format=agent  # LLM-optimized
rauta backends drain 10.0.1.5:8080
```

**Admin REST API** — port 9091, separate from data plane:
```
GET  /api/v1/status
GET  /api/v1/routes
GET  /api/v1/cache
POST /api/v1/diagnose?symptom=circuit-breaker-cascade
```

## Diagnostics Engine

Deterministic Rust rules that explain gateway state. No LLM — pure structured reasoning.

| Rule | Detects | Severity |
|------|---------|----------|
| RAUTA-CB-001 | Circuit breaker cascade (2+ Open) | Critical |
| RAUTA-CB-002 | Single circuit breaker open | Warning |
| RAUTA-RL-001 | Rate limit exhausted | Warning |
| RAUTA-BE-001 | No healthy backends | Critical |
| RAUTA-BE-002 | All backends draining | Warning |
| RAUTA-CACHE-001 | Low cache hit rate (<50%) | Info |
| RAUTA-LISTEN-001 | Listener port conflict | Info |

Each diagnosis includes a causal chain, evidence, and actionable CLI commands.

## Gateway API

Full Kubernetes Gateway API v1 support:

```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: HTTPRoute
metadata:
  name: api
spec:
  parentRefs:
  - name: my-gateway
  rules:
  - matches:
    - path:
        type: PathPrefix
        value: /api/v1
    backendRefs:
    - name: api-service
      port: 8080
      weight: 90
    - name: api-canary
      port: 8080
      weight: 10
```

**Supported resources:** GatewayClass, Gateway, HTTPRoute, EndpointSlice, Secret (TLS)

**Routing features:** path prefix matching (radix tree), header matching (exact + regex), query parameter matching, method matching, weighted backends

**Filters:** request/response header modification, HTTP redirects (301/302), timeouts, retries with exponential backoff

**Load balancing:** Maglev consistent hashing — O(1) lookup, ~1/N disruption on backend changes, weighted distribution, sticky sessions via (client_ip, port) hash

## Workspace

```
rauta/
├── common/       no_std types — HttpMethod, Backend, Maglev, RouteKey
├── control/      gateway controller + HTTP proxy + admin server
├── agent-api/    GatewayQuery trait, typed snapshots, diagnostics engine
├── mcp-server/   MCP tool definitions for AI agents
└── rauta-cli/    CLI binary + kubectl-rauta plugin
```

## Quick start

```bash
# Standalone mode (no Kubernetes)
RAUTA_BACKEND_ADDR=127.0.0.1:9090 ./target/release/control

# Kubernetes mode
RAUTA_K8S_MODE=true ./target/release/control

# Admin API is always on :9091
curl localhost:9091/api/v1/status
```

## Configuration

| Variable | Default | What |
|----------|---------|------|
| `RAUTA_K8S_MODE` | `false` | Enable Kubernetes controllers |
| `RAUTA_BIND_ADDR` | `0.0.0.0:8080` | Proxy listen address |
| `RAUTA_BACKEND_ADDR` | — | Backend (standalone mode) |
| `RAUTA_ADMIN_PORT` | `9091` | Admin server port |
| `RAUTA_ADMIN_ENDPOINT` | `http://localhost:9091` | CLI target |
| `RAUTA_TLS_CERT` | — | TLS certificate path |
| `RAUTA_TLS_KEY` | — | TLS private key path |
| `RAUTA_GATEWAY_CLASS` | `rauta` | GatewayClass to watch |
| `RUST_LOG` | `info` | Log level |

## Development

```bash
cargo test --workspace               # all 220+ tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
make ci-local                         # full CI
```

Pre-commit and pre-push hooks run fmt, clippy, and tests automatically.

## Tech

[kube](https://kube.rs) ·
[hyper](https://hyper.rs) ·
[tokio](https://tokio.rs) ·
[rustls](https://github.com/rustls/rustls) ·
[matchit](https://github.com/ibraheemdev/matchit) ·
[arc-swap](https://github.com/vorner/arc-swap) ·
[jemalloc](https://jemalloc.net) ·
[prometheus](https://github.com/tikv/rust-prometheus)

## FALSE Systems

RAUTA is part of the [FALSE Systems](https://github.com/false-systems) tool family:

| Tool | Finnish | What |
|------|---------|------|
| **RAUTA** | iron | API gateway |
| **AHTI** | god of the sea | Causality engine |
| **POLKU** | path | Event transport |
| **KULTA** | gold | Progressive delivery |
| **TAPIO** | forest god | Infrastructure management |

---

Apache 2.0
