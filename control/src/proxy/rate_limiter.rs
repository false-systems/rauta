//! Lock-Free Token Bucket Rate Limiter
//!
//! Production-grade rate limiting using lock-free atomic token bucket:
//! - Configurable rate (requests per second)
//! - Burst capacity (max tokens in bucket)
//! - Per-route isolation
//! - **Lock-free**: tokens + timestamp packed into AtomicU64 with CAS
//!
//! Packed state layout (AtomicU64):
//! ```text
//! [63:32] tokens     (32 bits, 16.16 fixed-point)
//! [31:0]  last_refill_offset_ms (32 bits, millis since bucket creation)
//! ```
//!
//! Algorithm: https://en.wikipedia.org/wiki/Token_bucket
//!
//! Example:
//! ```rust,ignore
//! let limiter = RateLimiter::new();
//! limiter.configure_route("/api", 100.0, 200); // 100 rps, burst 200
//!
//! if limiter.check_rate_limit("/api") {
//!     // Process request
//! } else {
//!     // Return 429 Too Many Requests
//! }
//! ```

use arc_swap::ArcSwap;
use lazy_static::lazy_static;
use prometheus::{IntCounterVec, IntGaugeVec, Opts, Registry};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::warn;

lazy_static! {
    /// Global metrics registry for rate limiter
    static ref RATE_LIMITER_REGISTRY: Registry = Registry::new();

    /// Rate limit requests counter (allowed vs rejected)
    ///
    /// Labels:
    /// - route: The route pattern (e.g., "/api/users")
    /// - result: "allowed" or "rejected"
    ///
    /// Example PromQL queries:
    /// - Rate of rejected requests: `rate(rauta_rate_limit_requests_total{result="rejected"}[1m])`
    /// - Success rate by route: `rate(rauta_rate_limit_requests_total{result="allowed"}[1m]) / rate(rauta_rate_limit_requests_total[1m])`
    #[allow(clippy::expect_used)]
    static ref RATE_LIMIT_REQUESTS_TOTAL: IntCounterVec = {
        let opts = Opts::new(
            "rauta_rate_limit_requests_total",
            "Total number of requests processed by rate limiter (allowed or rejected)"
        );
        let counter = IntCounterVec::new(opts, &["route", "result"])
            .unwrap_or_else(|e| {
                eprintln!("WARN: Failed to create rauta_rate_limit_requests_total counter: {}", e);
                #[allow(clippy::expect_used)]
                {
                    IntCounterVec::new(
                        Opts::new("rauta_rate_limit_requests_total_fallback", "Fallback metric for rate limit requests"),
                        &["route", "result"]
                    ).expect("Fallback metric creation should never fail - if this panics, Prometheus is broken")
                }
            });
        if let Err(e) = RATE_LIMITER_REGISTRY.register(Box::new(counter.clone())) {
            eprintln!("WARN: Failed to register rauta_rate_limit_requests_total counter: {}", e);
        }
        counter
    };

    /// Current tokens available in each route's bucket
    ///
    /// Labels:
    /// - route: The route pattern (e.g., "/api/users")
    ///
    /// This gauge shows real-time token availability:
    /// - Value near capacity: Route is healthy, not rate limited
    /// - Value near 0: Route is experiencing high traffic
    /// - Value at 0: Route is actively rate limiting requests
    ///
    /// Example PromQL queries:
    /// - Routes near rate limit: `rauta_rate_limit_tokens_available < 10`
    /// - Token refill rate: `deriv(rauta_rate_limit_tokens_available[5m])`
    #[allow(clippy::expect_used)]
    static ref RATE_LIMIT_TOKENS_AVAILABLE: IntGaugeVec = {
        let opts = Opts::new(
            "rauta_rate_limit_tokens_available",
            "Current tokens available in the rate limiter bucket for each route"
        );
        let gauge = IntGaugeVec::new(opts, &["route"])
            .unwrap_or_else(|e| {
                eprintln!("WARN: Failed to create rauta_rate_limit_tokens_available gauge: {}", e);
                #[allow(clippy::expect_used)]
                {
                    IntGaugeVec::new(
                        Opts::new("rauta_rate_limit_tokens_available_fallback", "Fallback metric for rate limit tokens"),
                        &["route"]
                    ).expect("Fallback metric creation should never fail - if this panics, Prometheus is broken")
                }
            });
        if let Err(e) = RATE_LIMITER_REGISTRY.register(Box::new(gauge.clone())) {
            eprintln!("WARN: Failed to register rauta_rate_limit_tokens_available gauge: {}", e);
        }
        gauge
    };
}

/// Export the rate limiter metrics registry (for global /metrics endpoint)
pub fn rate_limiter_registry() -> &'static Registry {
    &RATE_LIMITER_REGISTRY
}

// ============================================================================
// Fixed-point token representation (16.16)
// Upper 32 bits of packed u64 = tokens in 16.16 fixed-point
// Lower 32 bits = last refill offset in milliseconds from bucket creation
// ============================================================================

const FIXED_SHIFT: u32 = 16; // 16 fractional bits
const FIXED_ONE: u64 = 1 << FIXED_SHIFT; // 1.0 in fixed-point = 65536

#[inline]
fn float_to_fixed(f: f64) -> u64 {
    (f * FIXED_ONE as f64) as u64
}

#[inline]
fn fixed_to_float(fixed: u64) -> f64 {
    fixed as f64 / FIXED_ONE as f64
}

#[inline]
fn pack_bucket(tokens_fixed: u64, refill_offset_ms: u64) -> u64 {
    (tokens_fixed << 32) | (refill_offset_ms & 0xFFFF_FFFF)
}

#[inline]
fn unpack_tokens(packed: u64) -> u64 {
    packed >> 32
}

#[inline]
fn unpack_refill_offset(packed: u64) -> u64 {
    packed & 0xFFFF_FFFF
}

/// Lock-free token bucket for rate limiting
///
/// All state packed into a single `AtomicU64`:
/// - Upper 32 bits: tokens in 16.16 fixed-point
/// - Lower 32 bits: last refill offset (ms since creation)
///
/// `try_acquire()` does refill + consume in a single CAS.
#[derive(Debug)]
pub struct TokenBucket {
    /// Packed state: [63:32] tokens (16.16 fixed), [31:0] last_refill_offset_ms
    packed: AtomicU64,
    /// Maximum tokens (burst capacity) in 16.16 fixed-point
    capacity_fixed: u64,
    /// Refill rate in fixed-point tokens per millisecond
    refill_rate_per_ms_fixed: u64,
    /// When this bucket was created
    created_at: Instant,
    /// Original capacity as f64 (for available_tokens reporting)
    capacity_f64: f64,
}

impl TokenBucket {
    /// Create a new token bucket
    ///
    /// # Arguments
    /// * `rate` - Tokens per second (e.g., 100.0 = 100 requests/sec)
    /// * `burst` - Maximum burst capacity (tokens)
    pub fn new(rate: f64, burst: u64) -> Self {
        // Clamp burst to 16.16 fixed-point range (max ~65535 tokens in upper 32 bits)
        let clamped_burst = burst.min(65535);
        let capacity = clamped_burst as f64;
        let capacity_fixed = float_to_fixed(capacity);
        // Convert tokens/sec to tokens/ms in fixed-point
        let refill_rate_per_ms_fixed = float_to_fixed(rate / 1000.0);

        let initial = pack_bucket(capacity_fixed, 0);

        Self {
            packed: AtomicU64::new(initial),
            capacity_fixed,
            refill_rate_per_ms_fixed,
            created_at: Instant::now(),
            capacity_f64: capacity,
        }
    }

    /// Try to acquire a token
    ///
    /// Returns true if token acquired (request allowed), false otherwise (rate limited)
    pub fn try_acquire(&self) -> bool {
        self.try_acquire_n(1.0)
    }

    /// Try to acquire N tokens
    ///
    /// Refills and consumes tokens in a single CAS loop.
    pub fn try_acquire_n(&self, n: f64) -> bool {
        if n <= 0.0 {
            return true;
        }

        let n_fixed = float_to_fixed(n);
        // Mask to u32 range to handle wrapping after ~49.7 days uptime
        let now_ms = (self.created_at.elapsed().as_millis() as u32) as u64;

        loop {
            let current = self.packed.load(Ordering::Acquire);
            let old_tokens = unpack_tokens(current);
            let old_refill_ms = unpack_refill_offset(current);

            // Wrapping subtraction handles 32-bit timestamp overflow (~49.7 days)
            let elapsed_ms = (now_ms as u32).wrapping_sub(old_refill_ms as u32) as u64;
            let tokens_to_add = elapsed_ms * self.refill_rate_per_ms_fixed;
            let new_tokens = (old_tokens + tokens_to_add).min(self.capacity_fixed);

            // Try to consume
            if new_tokens >= n_fixed {
                let after_consume = new_tokens - n_fixed;
                let new_packed = pack_bucket(after_consume, now_ms);
                if self
                    .packed
                    .compare_exchange_weak(current, new_packed, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    return true;
                }
                // CAS failed, retry
            } else {
                // Not enough tokens — still update refill timestamp for freshness
                let new_packed = pack_bucket(new_tokens, now_ms);
                let _ = self.packed.compare_exchange_weak(
                    current,
                    new_packed,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                return false;
            }
        }
    }

    /// Get current token count (for testing/metrics)
    pub fn available_tokens(&self) -> f64 {
        let now_ms = (self.created_at.elapsed().as_millis() as u32) as u64;
        let current = self.packed.load(Ordering::Acquire);
        let old_tokens = unpack_tokens(current);
        let old_refill_ms = unpack_refill_offset(current);

        let elapsed_ms = (now_ms as u32).wrapping_sub(old_refill_ms as u32) as u64;
        let tokens_to_add = elapsed_ms * self.refill_rate_per_ms_fixed;
        let tokens = (old_tokens + tokens_to_add).min(self.capacity_fixed);

        fixed_to_float(tokens).min(self.capacity_f64)
    }

    /// Get burst capacity
    pub fn capacity(&self) -> f64 {
        self.capacity_f64
    }

    /// Get refill rate in tokens per second
    pub fn refill_rate(&self) -> f64 {
        fixed_to_float(self.refill_rate_per_ms_fixed) * 1000.0
    }

    /// Reset bucket to full capacity (for testing)
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn reset(&self) {
        let now_ms = self.created_at.elapsed().as_millis() as u64;
        self.packed
            .store(pack_bucket(self.capacity_fixed, now_ms), Ordering::Release);
    }
}

/// Per-route rate limiter
///
/// Uses `ArcSwap` for lock-free read access to the bucket map on the hot path.
pub struct RateLimiter {
    /// Route -> TokenBucket mapping (lock-free reads via ArcSwap)
    buckets: ArcSwap<HashMap<String, Arc<TokenBucket>>>,
    /// Serializes writes (new bucket creation)
    write_lock: std::sync::Mutex<()>,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new() -> Self {
        Self {
            buckets: ArcSwap::from_pointee(HashMap::new()),
            write_lock: std::sync::Mutex::new(()),
        }
    }

    /// Configure rate limit for a route
    ///
    /// # Arguments
    /// * `route` - Route pattern (e.g., "/api")
    /// * `rate` - Requests per second
    /// * `burst` - Burst capacity
    pub fn configure_route(&self, route: &str, rate: f64, burst: u64) {
        let _guard = self.write_lock.lock().unwrap_or_else(|poisoned| {
            warn!("RateLimiter write_lock poisoned, recovering");
            poisoned.into_inner()
        });

        let bucket = Arc::new(TokenBucket::new(rate, burst));
        let snapshot = self.buckets.load();
        let mut new_map = (**snapshot).clone();
        new_map.insert(route.to_string(), bucket);
        self.buckets.store(Arc::new(new_map));
    }

    /// Check if request is allowed (within rate limit)
    ///
    /// Returns true if allowed, false if rate limited
    pub fn check_rate_limit(&self, route: &str) -> bool {
        // Lock-free read (single atomic load)
        let snapshot = self.buckets.load();

        if let Some(bucket) = snapshot.get(route) {
            let allowed = bucket.try_acquire();

            // Record metrics
            let result = if allowed { "allowed" } else { "rejected" };
            RATE_LIMIT_REQUESTS_TOTAL
                .with_label_values(&[route, result])
                .inc();

            // Update tokens available gauge
            let tokens = bucket.available_tokens();
            RATE_LIMIT_TOKENS_AVAILABLE
                .with_label_values(&[route])
                .set(tokens as i64);

            allowed
        } else {
            // No rate limit configured for this route - allow by default
            RATE_LIMIT_REQUESTS_TOTAL
                .with_label_values(&[route, "allowed"])
                .inc();

            true
        }
    }

    /// Remove rate limit configuration for a route
    #[allow(dead_code)]
    pub fn remove_route(&self, route: &str) {
        let _guard = self.write_lock.lock().unwrap_or_else(|poisoned| {
            warn!("RateLimiter write_lock poisoned, recovering");
            poisoned.into_inner()
        });

        let snapshot = self.buckets.load();
        let mut new_map = (**snapshot).clone();
        new_map.remove(route);
        self.buckets.store(Arc::new(new_map));
    }

    /// Get available tokens for a route (for testing/metrics)
    #[allow(dead_code)]
    pub fn available_tokens(&self, route: &str) -> Option<f64> {
        let snapshot = self.buckets.load();
        snapshot.get(route).map(|bucket| bucket.available_tokens())
    }

    /// Snapshot all rate limiter buckets for the admin API/CLI/MCP
    pub fn snapshot_all(&self) -> Vec<agent_api::types::RateLimiterSnapshot> {
        let snapshot = self.buckets.load();
        snapshot
            .iter()
            .map(|(route, bucket)| agent_api::types::RateLimiterSnapshot {
                route: route.clone(),
                tokens_available: bucket.available_tokens(),
                capacity: bucket.capacity(),
                refill_rate: bucket.refill_rate(),
            })
            .collect()
    }

    /// Count buckets with zero tokens (actively rate limiting)
    pub fn exhausted_count(&self) -> usize {
        let snapshot = self.buckets.load();
        snapshot
            .values()
            .filter(|b| b.available_tokens() <= 0.0)
            .count()
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_token_bucket_allows_requests_under_limit() {
        let bucket = TokenBucket::new(10.0, 10); // 10 rps, burst 10

        // Should allow first 10 requests (burst capacity)
        for i in 1..=10 {
            assert!(
                bucket.try_acquire(),
                "Request {} should be allowed (within burst)",
                i
            );
        }

        // Should deny 11th request (bucket empty)
        assert!(
            !bucket.try_acquire(),
            "Request 11 should be denied (bucket empty)"
        );
    }

    #[test]
    fn test_token_bucket_refills_over_time() {
        let bucket = TokenBucket::new(100.0, 10); // 100 rps, burst 10

        // Drain bucket
        for _ in 0..10 {
            bucket.try_acquire();
        }

        // Bucket should be empty
        assert!(!bucket.try_acquire(), "Bucket should be empty");

        // Wait 100ms (should refill 10 tokens at 100 rps)
        thread::sleep(Duration::from_millis(100));

        // Should have ~10 tokens now
        assert!(
            bucket.try_acquire(),
            "Bucket should have refilled after 100ms"
        );
    }

    #[test]
    fn test_token_bucket_burst_capacity() {
        let bucket = TokenBucket::new(10.0, 50); // 10 rps, burst 50

        // Should allow 50 requests immediately (burst)
        for i in 1..=50 {
            assert!(
                bucket.try_acquire(),
                "Request {} should be allowed (within burst of 50)",
                i
            );
        }

        // 51st should fail
        assert!(
            !bucket.try_acquire(),
            "Request 51 should be denied (burst exceeded)"
        );
    }

    #[test]
    fn test_token_bucket_refill_rate_accuracy() {
        let bucket = TokenBucket::new(1000.0, 10); // 1000 rps = 1 token per ms

        // Drain bucket
        for _ in 0..10 {
            bucket.try_acquire();
        }

        // Wait 5ms (should refill ~5 tokens)
        thread::sleep(Duration::from_millis(5));

        // Should allow ~5 requests
        let mut allowed = 0;
        for _ in 0..10 {
            if bucket.try_acquire() {
                allowed += 1;
            }
        }

        // Should have allowed 4-7 requests (accounting for timing variance and system load)
        assert!(
            (4..=7).contains(&allowed),
            "Should allow 4-7 requests after 5ms, got {}",
            allowed
        );
    }

    #[test]
    fn test_token_bucket_weighted_acquire() {
        let bucket = TokenBucket::new(10.0, 10);

        // Acquire 5 tokens
        assert!(bucket.try_acquire_n(5.0), "Should acquire 5 tokens");

        // Acquire 3 more
        assert!(bucket.try_acquire_n(3.0), "Should acquire 3 more tokens");

        // Try to acquire 5 more (only 2 left)
        assert!(
            !bucket.try_acquire_n(5.0),
            "Should fail to acquire 5 tokens (only 2 left)"
        );

        // Should still be able to acquire 2
        assert!(
            bucket.try_acquire_n(2.0),
            "Should acquire remaining 2 tokens"
        );
    }

    #[test]
    fn test_rate_limiter_configure_and_check() {
        let limiter = RateLimiter::new();

        // Configure /api route
        limiter.configure_route("/api", 10.0, 10);

        // Should allow first 10 requests
        for i in 1..=10 {
            assert!(
                limiter.check_rate_limit("/api"),
                "Request {} should be allowed",
                i
            );
        }

        // 11th should fail
        assert!(
            !limiter.check_rate_limit("/api"),
            "Request 11 should be rate limited"
        );
    }

    #[test]
    fn test_rate_limiter_per_route_isolation() {
        let limiter = RateLimiter::new();

        // Configure different limits for different routes
        limiter.configure_route("/api", 10.0, 10);
        limiter.configure_route("/admin", 5.0, 5);

        // Drain /api
        for _ in 0..10 {
            limiter.check_rate_limit("/api");
        }

        // /api should be rate limited
        assert!(
            !limiter.check_rate_limit("/api"),
            "/api should be rate limited"
        );

        // /admin should still be available
        assert!(
            limiter.check_rate_limit("/admin"),
            "/admin should still be available"
        );
    }

    #[test]
    fn test_rate_limiter_no_config_allows_all() {
        let limiter = RateLimiter::new();

        // No rate limit configured - should allow all
        for _ in 0..1000 {
            assert!(
                limiter.check_rate_limit("/unconfigured"),
                "Unconfigured routes should allow all requests"
            );
        }
    }

    #[test]
    fn test_rate_limiter_remove_route() {
        let limiter = RateLimiter::new();

        // Configure and drain
        limiter.configure_route("/api", 10.0, 10);
        for _ in 0..10 {
            limiter.check_rate_limit("/api");
        }

        // Should be rate limited
        assert!(!limiter.check_rate_limit("/api"));

        // Remove configuration
        limiter.remove_route("/api");

        // Should now allow (no config = allow all)
        assert!(
            limiter.check_rate_limit("/api"),
            "Should allow after removing rate limit config"
        );
    }

    #[test]
    fn test_rate_limiter_concurrent_access() {
        use std::sync::Arc;

        let limiter = Arc::new(RateLimiter::new());
        limiter.configure_route("/api", 100.0, 100);

        // Spawn 10 threads, each trying 20 requests
        let handles: Vec<_> = (0..10)
            .map(|_| {
                let limiter = Arc::clone(&limiter);
                thread::spawn(move || {
                    let mut allowed = 0;
                    for _ in 0..20 {
                        if limiter.check_rate_limit("/api") {
                            allowed += 1;
                        }
                    }
                    allowed
                })
            })
            .collect();

        // Collect results
        let total_allowed: u32 = handles.into_iter().map(|h| h.join().unwrap()).sum();

        // Should allow approximately 100 requests (burst capacity).
        // With CAS-based atomics under high contention, slight variance is possible
        // due to time-based refill between CAS retries. Allow ±1 tolerance.
        assert!(
            (99..=101).contains(&total_allowed),
            "Should allow ~100 requests across all threads, got {}",
            total_allowed
        );
    }

    #[test]
    fn test_rate_limiter_metrics_recorded() {
        let limiter = RateLimiter::new();
        limiter.configure_route("/test", 10.0, 10);

        // Allow 5 requests
        for _ in 0..5 {
            assert!(limiter.check_rate_limit("/test"));
        }

        // Check metrics are accessible (they're recorded to RATE_LIMITER_REGISTRY)
        let metrics = crate::proxy::rate_limiter::rate_limiter_registry().gather();

        // Should have metrics for rauta_rate_limit_requests_total
        let has_request_metric = metrics
            .iter()
            .any(|family| family.name() == "rauta_rate_limit_requests_total");
        assert!(
            has_request_metric,
            "Should have rauta_rate_limit_requests_total metric"
        );

        // Should have metrics for rauta_rate_limit_tokens_available
        let has_tokens_metric = metrics
            .iter()
            .any(|family| family.name() == "rauta_rate_limit_tokens_available");
        assert!(
            has_tokens_metric,
            "Should have rauta_rate_limit_tokens_available metric"
        );
    }

    #[test]
    fn test_token_bucket_available_tokens() {
        let bucket = TokenBucket::new(10.0, 20);

        // Full bucket
        assert!(
            (bucket.available_tokens() - 20.0).abs() < 0.01,
            "Should start with 20 tokens"
        );

        // Acquire 5
        bucket.try_acquire_n(5.0);
        assert!(
            (bucket.available_tokens() - 15.0).abs() < 0.01,
            "Should have 15 tokens after acquiring 5"
        );

        // Acquire 10 more
        bucket.try_acquire_n(10.0);
        assert!(
            (bucket.available_tokens() - 5.0).abs() < 0.01,
            "Should have 5 tokens after acquiring 15 total"
        );
    }

    #[test]
    fn test_fixed_point_roundtrip() {
        // Verify fixed-point conversion accuracy
        assert!((fixed_to_float(float_to_fixed(1.0)) - 1.0).abs() < 0.001);
        assert!((fixed_to_float(float_to_fixed(100.0)) - 100.0).abs() < 0.01);
        assert!((fixed_to_float(float_to_fixed(0.5)) - 0.5).abs() < 0.001);

        // Verify packing roundtrip
        let tokens = float_to_fixed(42.5);
        let offset = 12345u64;
        let packed = pack_bucket(tokens, offset);
        assert_eq!(unpack_tokens(packed), tokens);
        assert_eq!(unpack_refill_offset(packed), offset);
    }
}
