# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What Is This

RAUTA ("iron" in Finnish) is an AI-native Kubernetes Gateway API controller with an L7 HTTP proxy, written in Rust. It's a learning project evolving into an AI-native API gateway with agent-queryable management, diagnostics engine, and eBPF observability.

## Build & Test Commands

```bash
# Build
cargo build -p control              # Debug build (fast)
cargo build --release -p control     # Release build (slow, LTO enabled)

# Test
cargo test --workspace               # All tests
cargo test TEST_NAME -- --nocapture   # Single test with output

# Lint & format
cargo fmt --all                       # Format
cargo fmt --all -- --check            # Check format (CI)
cargo clippy --all-targets --all-features -- -D warnings  # Lint (strict)

# Full CI locally (recommended before pushing)
make ci-local
```

Both `make` and `just` are available. The justfile has `just test-one TEST` for running a single test with output.

## Architecture

**Workspace layout:**
- `common` — no_std shared types (HttpMethod, Backend, Maglev, RouteKey)
- `control` — main controller binary (proxy + K8s controllers + admin server)
- `agent-api` — shared types, GatewayQuery trait, diagnostics engine
- `mcp-server` — MCP tool definitions for AI agent integration
- `rauta-cli` — CLI binary + kubectl plugin

**Two subsystems in `control/src/`:**

1. **`apis/gateway/`** — Kubernetes controllers (kube-rs reconcilers). Watch GatewayClass, Gateway, HTTPRoute, EndpointSlice, and Secret resources. Push routing config into the Router.

2. **`proxy/`** — HTTP proxy engine. Router uses a `matchit` radix tree for path matching, then Maglev consistent hashing for backend selection. Request flow: Listener → Router → Filters → Forwarder → Backend.

**Key data flow:** K8s reconcilers translate Gateway API resources into `Router` entries. The Router is shared via `Arc<Router>` between controllers and the proxy server.

## Rust Rules (Enforced)

1. **No `.unwrap()` in production code** — Use `?`, `safe_read()`/`safe_write()`, or `.ok_or_else()`. Tests may use `.unwrap()`.
2. **No `println!`** — Use `tracing::{info, warn, error, debug}`.
3. **No string enums** — Use proper Rust enums with `#[repr(u8)]` where appropriate.
4. **No TODOs or stubs** — Complete implementations only.
5. **Safe lock helpers are mandatory** for `RwLock`/`Mutex` — use `safe_read(&lock)` / `safe_write(&lock)` instead of `.read().unwrap()` / `.write().unwrap()`. These recover from lock poisoning. Defined in modules that use them.
6. **Clippy lints** in `control/Cargo.toml` warn on `unwrap_used`, `expect_used`, and `panic` in non-test code.

## TDD Workflow

Always RED → GREEN → REFACTOR. Write a failing test first, implement minimally, then clean up. Tests use `#[tokio::test]` for async.

## Common Extension Points

**Adding a filter:** Add variant to `FilterAction` in `filters.rs` → implement in `apply_request_filters()`/`apply_response_filters()` → parse from HTTPRoute in `http_route.rs` → add tests.

**Adding a match condition:** Add to `RouteMatch` → update `matches_request()` in `router.rs` → parse in `http_route.rs` → add tests.

**Adding a metric:** Register in `metrics.rs` → instrument in code path → test.

## Environment Variables

Key config: `RAUTA_K8S_MODE` (enable K8s mode), `RAUTA_BIND_ADDR` (listen address), `RAUTA_BACKEND_ADDR` (standalone backend), `RAUTA_ADMIN_PORT` (admin server port, default 9091), `RAUTA_ADMIN_ENDPOINT` (CLI admin endpoint), `RUST_LOG` (log level).

## Verification Before Commit

```bash
make ci-local   # runs fmt check, clippy, cargo check, tests
```
