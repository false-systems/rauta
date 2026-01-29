# RAUTA Development Guide

Complete guide for developing RAUTA on Linux and macOS.

## Overview

RAUTA is a pure Rust userspace Gateway API controller. No eBPF, no kernel modules - just fast HTTP proxying.

## Quick Setup

### One-Command Setup (Recommended)

```bash
./scripts/setup.sh
```

This auto-detects your platform and installs the right tools.

---

## Development Setup

### Requirements

- Rust 1.83+
- OpenSSL development headers (for TLS)

### Linux

```bash
# Ubuntu/Debian
sudo apt-get install build-essential pkg-config libssl-dev

# Fedora/RHEL
sudo dnf install gcc pkg-config openssl-devel

# Arch Linux
sudo pacman -S base-devel openssl
```

### macOS

```bash
# Xcode command line tools
xcode-select --install

# OpenSSL (via Homebrew)
brew install openssl
```

---

## Daily Development

### Build

```bash
# Build control plane
cargo build -p control

# Build with release optimizations
cargo build --release -p control
```

### Test

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_router_maglev

# Run tests with output
cargo test -- --nocapture
```

### Format & Lint

```bash
# Format code
cargo fmt

# Lint (treat warnings as errors)
cargo clippy -- -D warnings
```

---

## TDD Workflow

**RED → GREEN → REFACTOR**

```bash
# 1. Write test (RED)
vim control/src/proxy/router.rs

# 2. Run test (should fail)
cargo test test_new_feature
# ❌ test_new_feature ... FAILED

# 3. Implement feature (GREEN)
vim control/src/proxy/router.rs

# 4. Run test again
cargo test test_new_feature
# ✅ test_new_feature ... ok

# 5. Refactor if needed
# Tests stay green!
```

---

## Project Structure

```
rauta/
├── common/           # Shared types (HttpMethod, Backend, Maglev)
│   └── src/lib.rs
├── control/          # Main controller
│   └── src/
│       ├── main.rs           # Entry point
│       ├── apis/gateway/     # K8s controllers
│       └── proxy/            # HTTP proxy
└── deploy/           # Kubernetes manifests
```

---

## Running Locally

### Standalone Mode (No Kubernetes)

```bash
# Start a backend server
python3 -m http.server 9090 &

# Run RAUTA
RAUTA_BACKEND_ADDR=127.0.0.1:9090 \
RAUTA_BIND_ADDR=127.0.0.1:8080 \
cargo run -p control

# Test
curl http://localhost:8080/
```

### Kubernetes Mode

```bash
# Create kind cluster
kind create cluster --name rauta-dev

# Install Gateway API CRDs
kubectl apply -f https://github.com/kubernetes-sigs/gateway-api/releases/download/v1.1.0/standard-install.yaml

# Run controller
RAUTA_K8S_MODE=true cargo run -p control
```

---

## Docker

### Build Image

```bash
./docker/build.sh
```

### Run Integration Tests

```bash
./docker/test.sh
```

### Docker Compose

```bash
# Start services
docker-compose -f docker/docker-compose.yml up -d

# View logs
docker-compose -f docker/docker-compose.yml logs -f

# Stop
docker-compose -f docker/docker-compose.yml down
```

---

## Performance Testing

### Load Test with wrk

```bash
# Start RAUTA + backend
docker-compose -f docker/docker-compose.prod.yml up -d

# Run load test
wrk -t4 -c100 -d10s http://localhost:8080/

# With latency histogram
wrk -t12 -c400 -d30s --latency http://localhost:8080/
```

### Expected Performance

- Throughput: 50-100k req/s (containerized)
- Latency p99: 1-5ms

---

## VS Code Integration

### Recommended Extensions

- rust-analyzer
- Even Better TOML
- Error Lens

### Tasks

Press `Cmd+Shift+B` (macOS) or `Ctrl+Shift+B` (Linux) to run build tasks.

---

## Troubleshooting

### OpenSSL Not Found

```bash
# macOS
export OPENSSL_DIR=$(brew --prefix openssl)

# Linux
sudo apt-get install libssl-dev  # Ubuntu/Debian
sudo dnf install openssl-devel   # Fedora
```

### rust-analyzer Shows Errors

```bash
# Reload VS Code window
Cmd+Shift+P → "Developer: Reload Window"
```

### Slow Builds

```bash
# Use debug builds for development
cargo build  # Fast

# Use release builds for testing
cargo build --release  # Slow but optimized
```

---

## FAQ

**Q: Which platform is better for development?**
A: Both Linux and macOS work great. It's pure Rust - no platform-specific code.

**Q: Do I need Docker?**
A: Only for integration tests. Daily development is just `cargo build` and `cargo test`.

**Q: What about Windows?**
A: Should work with WSL2. Not tested on native Windows.

---

## Next Steps

1. **Run setup**: `./scripts/setup.sh`
2. **Build project**: `cargo build -p control`
3. **Run tests**: `cargo test`
4. **Start developing**: Edit → Build → Test

**Happy hacking!**
