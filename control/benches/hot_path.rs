//! Hot path benchmarks — measures the critical per-request operations
//!
//! Run: cargo bench -p control --bench hot_path
//!
//! Benchmarks:
//! - Circuit breaker allow_request (AtomicU64 CAS)
//! - Rate limiter try_acquire (AtomicU64 CAS)
//! - Router select_backend (route lookup + Maglev + health check via ArcSwap)

#![allow(clippy::unwrap_used)]

use common::{Backend, HttpMethod};
use control::proxy::circuit_breaker::CircuitBreakerManager;
use control::proxy::rate_limiter::RateLimiter;
use control::proxy::router::Router;
use std::hint::black_box;
use std::time::{Duration, Instant};

fn main() {
    println!("RAUTA Hot Path Benchmarks");
    println!("========================\n");

    bench_circuit_breaker();
    bench_rate_limiter();
    bench_router_select();
}

fn bench_circuit_breaker() {
    let manager = CircuitBreakerManager::new(5, Duration::from_secs(30));

    // Warm up — create a breaker
    manager.record_success("bench-backend");

    let iterations = 1_000_000;
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(manager.allow_request("bench-backend"));
    }
    let elapsed = start.elapsed();

    let ns_per_op = elapsed.as_nanos() / iterations as u128;
    let ops_per_sec = iterations as f64 / elapsed.as_secs_f64();
    println!(
        "circuit_breaker.allow_request: {}ns/op ({:.0} ops/sec)",
        ns_per_op, ops_per_sec
    );
}

fn bench_rate_limiter() {
    let limiter = RateLimiter::new();
    limiter.configure_route("/bench", 1_000_000.0, 1_000_000);

    let iterations = 1_000_000;
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(limiter.check_rate_limit("/bench"));
    }
    let elapsed = start.elapsed();

    let ns_per_op = elapsed.as_nanos() / iterations as u128;
    let ops_per_sec = iterations as f64 / elapsed.as_secs_f64();
    println!(
        "rate_limiter.check_rate_limit: {}ns/op ({:.0} ops/sec)",
        ns_per_op, ops_per_sec
    );
}

fn bench_router_select() {
    let router = Router::new();
    let backend = Backend::new(0x7f000001, 8080, 100);
    router
        .add_route(HttpMethod::GET, "/api/v1/users", vec![backend])
        .unwrap();

    let iterations = 1_000_000;
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(router.select_backend(HttpMethod::GET, "/api/v1/users", None, None));
    }
    let elapsed = start.elapsed();

    let ns_per_op = elapsed.as_nanos() / iterations as u128;
    let ops_per_sec = iterations as f64 / elapsed.as_secs_f64();
    println!(
        "router.select_backend:        {}ns/op ({:.0} ops/sec)",
        ns_per_op, ops_per_sec
    );
}
