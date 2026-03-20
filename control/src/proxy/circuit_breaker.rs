//! Lock-Free Circuit Breaker Pattern Implementation
//!
//! Production-grade circuit breaker for backend health management:
//! - Three states: Closed, Open, Half-Open
//! - Configurable failure threshold and timeout
//! - Automatic recovery testing
//! - Per-backend isolation
//! - **Lock-free**: All state packed into a single AtomicU64 (CAS-based)
//!
//! Bit layout of packed state (AtomicU64):
//! ```text
//! [63:62] state          (2 bits)  — 0=Closed, 1=Open, 2=HalfOpen
//! [61:48] reserved       (14 bits) — future use
//! [47:32] failure_count  (16 bits) — consecutive failures (max 65535)
//! [31:16] success_count  (16 bits) — successes in Half-Open (max 65535)
//! [15:0]  half_open_reqs (16 bits) — requests allowed in Half-Open (max 65535)
//! ```
//!
//! Algorithm: https://martinfowler.com/bliki/CircuitBreaker.html
//!
//! Example:
//! ```rust,ignore
//! let breaker = CircuitBreaker::new(5, Duration::from_secs(30));
//!
//! // Record request result
//! breaker.record_success();
//! breaker.record_failure();
//!
//! // Check if requests allowed
//! if breaker.allow_request() {
//!     // Send request to backend
//! } else {
//!     // Backend is in Open state - fail fast
//! }
//! ```

use arc_swap::ArcSwap;
use lazy_static::lazy_static;
use prometheus::{IntCounterVec, IntGaugeVec, Opts, Registry};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::warn;

lazy_static! {
    /// Global metrics registry for circuit breaker
    static ref CIRCUIT_BREAKER_REGISTRY: Registry = Registry::new();

    /// Circuit breaker state gauge (0=Closed, 1=Open, 2=HalfOpen)
    ///
    /// Labels:
    /// - backend: Backend identifier (e.g., "10.0.1.1:8080")
    ///
    /// State values:
    /// - 0 = Closed (healthy, requests allowed)
    /// - 1 = Open (unhealthy, requests rejected)
    /// - 2 = HalfOpen (testing recovery, limited requests allowed)
    ///
    /// Example PromQL queries:
    /// - Backends in Open state: `rauta_circuit_breaker_state{backend=~".*"} == 1`
    /// - Backends currently recovering: `rauta_circuit_breaker_state{backend=~".*"} == 2`
    #[allow(clippy::expect_used)]
    static ref CIRCUIT_BREAKER_STATE: IntGaugeVec = {
        let opts = Opts::new(
            "rauta_circuit_breaker_state",
            "Current state of circuit breaker (0=Closed, 1=Open, 2=HalfOpen)"
        );
        let gauge = IntGaugeVec::new(opts, &["backend"])
            .unwrap_or_else(|e| {
                eprintln!("WARN: Failed to create rauta_circuit_breaker_state gauge: {}", e);
                #[allow(clippy::expect_used)]
                {
                    IntGaugeVec::new(
                        Opts::new("rauta_circuit_breaker_state_fallback", "Fallback metric for circuit breaker state"),
                        &["backend"]
                    ).expect("Fallback metric creation should never fail - if this panics, Prometheus is broken")
                }
            });
        if let Err(e) = CIRCUIT_BREAKER_REGISTRY.register(Box::new(gauge.clone())) {
            eprintln!("WARN: Failed to register rauta_circuit_breaker_state gauge: {}", e);
        }
        gauge
    };

    /// Circuit breaker requests counter (allowed vs rejected)
    ///
    /// Labels:
    /// - backend: Backend identifier (e.g., "10.0.1.1:8080")
    /// - result: "allowed" or "rejected"
    ///
    /// Example PromQL queries:
    /// - Rate of rejected requests: `rate(rauta_circuit_breaker_requests_total{result="rejected"}[1m])`
    /// - Rejection ratio: `rate(rauta_circuit_breaker_requests_total{result="rejected"}[1m]) / rate(rauta_circuit_breaker_requests_total[1m])`
    #[allow(clippy::expect_used)]
    static ref CIRCUIT_BREAKER_REQUESTS_TOTAL: IntCounterVec = {
        let opts = Opts::new(
            "rauta_circuit_breaker_requests_total",
            "Total number of requests processed by circuit breaker (allowed or rejected)"
        );
        let counter = IntCounterVec::new(opts, &["backend", "result"])
            .unwrap_or_else(|e| {
                eprintln!("WARN: Failed to create rauta_circuit_breaker_requests_total counter: {}", e);
                #[allow(clippy::expect_used)]
                {
                    IntCounterVec::new(
                        Opts::new("rauta_circuit_breaker_requests_total_fallback", "Fallback metric for circuit breaker requests"),
                        &["backend", "result"]
                    ).expect("Fallback metric creation should never fail - if this panics, Prometheus is broken")
                }
            });
        if let Err(e) = CIRCUIT_BREAKER_REGISTRY.register(Box::new(counter.clone())) {
            eprintln!("WARN: Failed to register rauta_circuit_breaker_requests_total counter: {}", e);
        }
        counter
    };

    /// Circuit breaker failures counter
    ///
    /// Labels:
    /// - backend: Backend identifier (e.g., "10.0.1.1:8080")
    ///
    /// Tracks consecutive failures that lead to circuit opening.
    ///
    /// Example PromQL queries:
    /// - Failure rate by backend: `rate(rauta_circuit_breaker_failures_total[1m])`
    /// - Backends with high failure rates: `rate(rauta_circuit_breaker_failures_total[5m]) > 0.5`
    #[allow(clippy::expect_used)]
    static ref CIRCUIT_BREAKER_FAILURES_TOTAL: IntCounterVec = {
        let opts = Opts::new(
            "rauta_circuit_breaker_failures_total",
            "Total number of failures recorded by circuit breaker"
        );
        let counter = IntCounterVec::new(opts, &["backend"])
            .unwrap_or_else(|e| {
                eprintln!("WARN: Failed to create rauta_circuit_breaker_failures_total counter: {}", e);
                #[allow(clippy::expect_used)]
                {
                    IntCounterVec::new(
                        Opts::new("rauta_circuit_breaker_failures_total_fallback", "Fallback metric for circuit breaker failures"),
                        &["backend"]
                    ).expect("Fallback metric creation should never fail - if this panics, Prometheus is broken")
                }
            });
        if let Err(e) = CIRCUIT_BREAKER_REGISTRY.register(Box::new(counter.clone())) {
            eprintln!("WARN: Failed to register rauta_circuit_breaker_failures_total counter: {}", e);
        }
        counter
    };

    /// Circuit breaker state transitions counter
    ///
    /// Labels:
    /// - backend: Backend identifier (e.g., "10.0.1.1:8080")
    /// - from_state: Previous state (Closed/Open/HalfOpen)
    /// - to_state: New state (Closed/Open/HalfOpen)
    ///
    /// Tracks circuit breaker state changes for debugging and alerting.
    ///
    /// Example PromQL queries:
    /// - Circuits opening: `rate(rauta_circuit_breaker_transitions_total{to_state="Open"}[5m])`
    /// - Recovery attempts: `rauta_circuit_breaker_transitions_total{from_state="Open",to_state="HalfOpen"}`
    #[allow(clippy::expect_used)]
    static ref CIRCUIT_BREAKER_TRANSITIONS_TOTAL: IntCounterVec = {
        let opts = Opts::new(
            "rauta_circuit_breaker_transitions_total",
            "Total number of circuit breaker state transitions"
        );
        let counter = IntCounterVec::new(opts, &["backend", "from_state", "to_state"])
            .unwrap_or_else(|e| {
                eprintln!("WARN: Failed to create rauta_circuit_breaker_transitions_total counter: {}", e);
                #[allow(clippy::expect_used)]
                {
                    IntCounterVec::new(
                        Opts::new("rauta_circuit_breaker_transitions_total_fallback", "Fallback metric for circuit breaker transitions"),
                        &["backend", "from_state", "to_state"]
                    ).expect("Fallback metric creation should never fail - if this panics, Prometheus is broken")
                }
            });
        if let Err(e) = CIRCUIT_BREAKER_REGISTRY.register(Box::new(counter.clone())) {
            eprintln!("WARN: Failed to register rauta_circuit_breaker_transitions_total counter: {}", e);
        }
        counter
    };
}

/// Export the circuit breaker metrics registry (for global /metrics endpoint)
pub fn circuit_breaker_registry() -> &'static Registry {
    &CIRCUIT_BREAKER_REGISTRY
}

/// Convert CircuitState to string for metrics labels
#[inline]
fn state_to_str(state: CircuitState) -> &'static str {
    match state {
        CircuitState::Closed => "Closed",
        CircuitState::Open => "Open",
        CircuitState::HalfOpen => "HalfOpen",
    }
}

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation - requests allowed, failures counted
    Closed,
    /// Too many failures - requests blocked, wait for timeout
    Open,
    /// Testing recovery - limited requests allowed
    HalfOpen,
}

// ============================================================================
// Packed state bit layout for AtomicU64
// ============================================================================
const STATE_SHIFT: u32 = 62;
const STATE_MASK: u64 = 0x3 << STATE_SHIFT; // bits [63:62]
const FAILURE_SHIFT: u32 = 32;
const FAILURE_MASK: u64 = 0xFFFF << FAILURE_SHIFT; // bits [47:32]
const SUCCESS_SHIFT: u32 = 16;
const SUCCESS_MASK: u64 = 0xFFFF << SUCCESS_SHIFT; // bits [31:16]
const HALFOPEN_MASK: u64 = 0xFFFF; // bits [15:0]

const STATE_CLOSED: u64 = 0;
const STATE_OPEN: u64 = 1;
const STATE_HALFOPEN: u64 = 2;

#[inline]
fn pack_state(state: u64, failures: u64, successes: u64, half_open_reqs: u64) -> u64 {
    (state << STATE_SHIFT)
        | ((failures & 0xFFFF) << FAILURE_SHIFT)
        | ((successes & 0xFFFF) << SUCCESS_SHIFT)
        | (half_open_reqs & 0xFFFF)
}

#[inline]
fn unpack_state_field(packed: u64) -> u64 {
    (packed & STATE_MASK) >> STATE_SHIFT
}

#[inline]
fn unpack_failures(packed: u64) -> u64 {
    (packed & FAILURE_MASK) >> FAILURE_SHIFT
}

#[inline]
fn unpack_successes(packed: u64) -> u64 {
    (packed & SUCCESS_MASK) >> SUCCESS_SHIFT
}

#[inline]
fn unpack_half_open_reqs(packed: u64) -> u64 {
    packed & HALFOPEN_MASK
}

#[inline]
fn state_from_u64(val: u64) -> CircuitState {
    match val {
        STATE_OPEN => CircuitState::Open,
        STATE_HALFOPEN => CircuitState::HalfOpen,
        _ => CircuitState::Closed,
    }
}

#[inline]
fn state_to_u64(state: CircuitState) -> u64 {
    match state {
        CircuitState::Closed => STATE_CLOSED,
        CircuitState::Open => STATE_OPEN,
        CircuitState::HalfOpen => STATE_HALFOPEN,
    }
}

/// Lock-free circuit breaker for backend health management
///
/// All mutable state is packed into a single `AtomicU64`. State transitions use
/// CAS (compare-and-swap) loops to ensure correctness without locks.
/// A separate `AtomicU64` stores the last failure timestamp as microseconds since creation.
#[derive(Debug)]
pub struct CircuitBreaker {
    /// Packed state: [63:62] state, [47:32] failures, [31:16] successes, [15:0] half_open_reqs
    packed: AtomicU64,
    /// Last failure time as microseconds since breaker creation (+1 to distinguish from "never failed")
    last_failure_us: AtomicU64,
    /// When this breaker was created (reference point for last_failure_us)
    created_at: Instant,
    /// Failure threshold (consecutive failures to trip)
    failure_threshold: u32,
    /// Timeout before attempting Half-Open (Open → Half-Open)
    timeout: Duration,
    /// Max requests in Half-Open state
    half_open_max_requests: u32,
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    ///
    /// # Arguments
    /// * `failure_threshold` - Consecutive failures before opening circuit
    /// * `timeout` - Duration to wait before attempting Half-Open
    pub fn new(failure_threshold: u32, timeout: Duration) -> Self {
        Self {
            packed: AtomicU64::new(pack_state(STATE_CLOSED, 0, 0, 0)),
            last_failure_us: AtomicU64::new(0),
            created_at: Instant::now(),
            failure_threshold,
            timeout,
            half_open_max_requests: 3,
        }
    }

    /// Check if request should be allowed
    ///
    /// Returns true if request allowed, false if circuit is Open.
    /// Uses CAS loop for lock-free state transitions.
    pub fn allow_request(&self) -> bool {
        loop {
            let current = self.packed.load(Ordering::Acquire);
            let state = unpack_state_field(current);

            match state {
                STATE_CLOSED => return true,
                STATE_OPEN => {
                    // Check if timeout has elapsed for Open → HalfOpen transition
                    if self.should_attempt_reset() {
                        // CAS: transition Open → HalfOpen, immediately count this as first request
                        let new = pack_state(STATE_HALFOPEN, 0, 0, 1);
                        if self
                            .packed
                            .compare_exchange_weak(
                                current,
                                new,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            )
                            .is_ok()
                        {
                            return true;
                        }
                        // CAS failed — another thread transitioned first, retry
                        continue;
                    }
                    return false;
                }
                STATE_HALFOPEN => {
                    // Allow limited requests for testing
                    let half_open_reqs = unpack_half_open_reqs(current);
                    if half_open_reqs < self.half_open_max_requests as u64 {
                        let failures = unpack_failures(current);
                        let successes = unpack_successes(current);
                        let new =
                            pack_state(STATE_HALFOPEN, failures, successes, half_open_reqs + 1);
                        if self
                            .packed
                            .compare_exchange_weak(
                                current,
                                new,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            )
                            .is_ok()
                        {
                            return true;
                        }
                        // CAS failed, retry
                        continue;
                    }
                    return false;
                }
                _ => return false,
            }
        }
    }

    /// Record successful request
    pub fn record_success(&self) {
        loop {
            let current = self.packed.load(Ordering::Acquire);
            let state = unpack_state_field(current);

            match state {
                STATE_CLOSED => {
                    // Reset failure count on success
                    let new = pack_state(STATE_CLOSED, 0, 0, 0);
                    if self
                        .packed
                        .compare_exchange_weak(current, new, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                    {
                        return;
                    }
                    // CAS failed, retry
                }
                STATE_HALFOPEN => {
                    let successes = unpack_successes(current) + 1;
                    let half_open_reqs = unpack_half_open_reqs(current);

                    if successes >= self.half_open_max_requests as u64 {
                        // Enough successes — close the circuit
                        let new = pack_state(STATE_CLOSED, 0, 0, 0);
                        if self
                            .packed
                            .compare_exchange_weak(
                                current,
                                new,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            )
                            .is_ok()
                        {
                            // Clear last failure time
                            self.last_failure_us.store(0, Ordering::Release);
                            return;
                        }
                    } else {
                        let failures = unpack_failures(current);
                        let new = pack_state(STATE_HALFOPEN, failures, successes, half_open_reqs);
                        if self
                            .packed
                            .compare_exchange_weak(
                                current,
                                new,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            )
                            .is_ok()
                        {
                            return;
                        }
                    }
                    // CAS failed, retry
                }
                _ => return, // Ignore successes in Open state
            }
        }
    }

    /// Record failed request
    pub fn record_failure(&self) {
        // Update last failure time (use micros for sub-millisecond precision, +1 to distinguish from "never failed")
        let elapsed_us = self.created_at.elapsed().as_micros() as u64 + 1;
        self.last_failure_us.store(elapsed_us, Ordering::Release);

        loop {
            let current = self.packed.load(Ordering::Acquire);
            let state = unpack_state_field(current);

            match state {
                STATE_CLOSED => {
                    let failures = unpack_failures(current) + 1;
                    if failures >= self.failure_threshold as u64 {
                        // Trip the circuit
                        let new = pack_state(STATE_OPEN, failures, 0, 0);
                        if self
                            .packed
                            .compare_exchange_weak(
                                current,
                                new,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            )
                            .is_ok()
                        {
                            return;
                        }
                    } else {
                        let new = pack_state(STATE_CLOSED, failures, 0, 0);
                        if self
                            .packed
                            .compare_exchange_weak(
                                current,
                                new,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            )
                            .is_ok()
                        {
                            return;
                        }
                    }
                    // CAS failed, retry
                }
                STATE_HALFOPEN => {
                    // Any failure in Half-Open immediately reopens circuit
                    let new = pack_state(STATE_OPEN, 0, 0, 0);
                    if self
                        .packed
                        .compare_exchange_weak(current, new, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                    {
                        return;
                    }
                    // CAS failed, retry
                }
                STATE_OPEN => {
                    // Already open, just update timestamp (done above)
                    return;
                }
                _ => return,
            }
        }
    }

    /// Get current circuit state
    pub fn state(&self) -> CircuitState {
        let packed = self.packed.load(Ordering::Acquire);
        state_from_u64(unpack_state_field(packed))
    }

    /// Get current failure count
    #[allow(dead_code)]
    pub fn failure_count(&self) -> u32 {
        let packed = self.packed.load(Ordering::Acquire);
        unpack_failures(packed) as u32
    }

    /// Check if circuit should attempt reset (Open → Half-Open)
    fn should_attempt_reset(&self) -> bool {
        let last_failure_us = self.last_failure_us.load(Ordering::Acquire);
        if last_failure_us == 0 {
            return false;
        }
        // last_failure_us is stored as (elapsed_micros + 1), so subtract 1 to recover actual value
        let last_failure_duration = Duration::from_micros(last_failure_us - 1);
        let elapsed_since_creation = self.created_at.elapsed();
        if elapsed_since_creation > last_failure_duration {
            let time_since_failure = elapsed_since_creation - last_failure_duration;
            time_since_failure >= self.timeout
        } else {
            false
        }
    }

    /// Reset circuit to Closed state (for testing)
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn reset(&self) {
        self.packed
            .store(pack_state(STATE_CLOSED, 0, 0, 0), Ordering::Release);
        self.last_failure_us.store(0, Ordering::Release);
    }
}

/// Per-backend circuit breaker manager
///
/// Uses `ArcSwap` for lock-free read access to the breaker map on the hot path.
/// New breakers are created via a serializing `Mutex` (only on first access per backend).
pub struct CircuitBreakerManager {
    /// Backend ID -> CircuitBreaker mapping (lock-free reads via ArcSwap)
    breakers: ArcSwap<HashMap<String, Arc<CircuitBreaker>>>,
    /// Serializes writes (new breaker creation) — never held on hot path
    write_lock: std::sync::Mutex<()>,
    /// Default failure threshold
    default_failure_threshold: u32,
    /// Default timeout
    default_timeout: Duration,
}

impl CircuitBreakerManager {
    /// Create a new circuit breaker manager
    ///
    /// # Arguments
    /// * `failure_threshold` - Default consecutive failures before opening
    /// * `timeout` - Default duration before attempting Half-Open
    pub fn new(failure_threshold: u32, timeout: Duration) -> Self {
        Self {
            breakers: ArcSwap::from_pointee(HashMap::new()),
            write_lock: std::sync::Mutex::new(()),
            default_failure_threshold: failure_threshold,
            default_timeout: timeout,
        }
    }

    /// Get or create circuit breaker for backend
    pub fn get_breaker(&self, backend_id: &str) -> Arc<CircuitBreaker> {
        // Fast path: lock-free read (single atomic load, ~1ns)
        let snapshot = self.breakers.load();
        if let Some(breaker) = snapshot.get(backend_id) {
            return Arc::clone(breaker);
        }

        // Slow path: create new breaker (serialized, but only on first access per backend)
        let _guard = self.write_lock.lock().unwrap_or_else(|poisoned| {
            warn!("CircuitBreakerManager write_lock poisoned, recovering");
            poisoned.into_inner()
        });

        // Double-check after acquiring write lock
        let snapshot = self.breakers.load();
        if let Some(breaker) = snapshot.get(backend_id) {
            return Arc::clone(breaker);
        }

        let breaker = Arc::new(CircuitBreaker::new(
            self.default_failure_threshold,
            self.default_timeout,
        ));

        // Clone-on-write: clone the map, insert, and atomically swap
        let mut new_map = (**snapshot).clone();
        new_map.insert(backend_id.to_string(), Arc::clone(&breaker));
        self.breakers.store(Arc::new(new_map));

        breaker
    }

    /// Check if backend allows requests
    pub fn allow_request(&self, backend_id: &str) -> bool {
        let breaker = self.get_breaker(backend_id);
        let old_state = breaker.state();
        let allowed = breaker.allow_request();
        let new_state = breaker.state();

        // Record metrics
        let result = if allowed { "allowed" } else { "rejected" };
        CIRCUIT_BREAKER_REQUESTS_TOTAL
            .with_label_values(&[backend_id, result])
            .inc();

        // Update state gauge
        let state_value = state_to_u64(new_state) as i64;
        CIRCUIT_BREAKER_STATE
            .with_label_values(&[backend_id])
            .set(state_value);

        // Record state transition if changed
        if old_state != new_state {
            CIRCUIT_BREAKER_TRANSITIONS_TOTAL
                .with_label_values(&[backend_id, state_to_str(old_state), state_to_str(new_state)])
                .inc();
        }

        allowed
    }

    /// Record successful request
    pub fn record_success(&self, backend_id: &str) {
        let breaker = self.get_breaker(backend_id);
        let old_state = breaker.state();
        breaker.record_success();
        let new_state = breaker.state();

        // Update state gauge
        let state_value = state_to_u64(new_state) as i64;
        CIRCUIT_BREAKER_STATE
            .with_label_values(&[backend_id])
            .set(state_value);

        // Record state transition if changed
        if old_state != new_state {
            CIRCUIT_BREAKER_TRANSITIONS_TOTAL
                .with_label_values(&[backend_id, state_to_str(old_state), state_to_str(new_state)])
                .inc();
        }
    }

    /// Record failed request
    pub fn record_failure(&self, backend_id: &str) {
        let breaker = self.get_breaker(backend_id);
        let old_state = breaker.state();
        breaker.record_failure();
        let new_state = breaker.state();

        // Increment failure counter
        CIRCUIT_BREAKER_FAILURES_TOTAL
            .with_label_values(&[backend_id])
            .inc();

        // Update state gauge
        let state_value = state_to_u64(new_state) as i64;
        CIRCUIT_BREAKER_STATE
            .with_label_values(&[backend_id])
            .set(state_value);

        // Record state transition if changed
        if old_state != new_state {
            CIRCUIT_BREAKER_TRANSITIONS_TOTAL
                .with_label_values(&[backend_id, state_to_str(old_state), state_to_str(new_state)])
                .inc();
        }
    }

    /// Get backend circuit state
    #[allow(dead_code)]
    pub fn get_state(&self, backend_id: &str) -> Option<CircuitState> {
        let snapshot = self.breakers.load();
        snapshot.get(backend_id).map(|b| b.state())
    }

    /// Remove circuit breaker for backend
    #[allow(dead_code)]
    pub fn remove_backend(&self, backend_id: &str) {
        let _guard = self.write_lock.lock().unwrap_or_else(|poisoned| {
            warn!("CircuitBreakerManager write_lock poisoned, recovering");
            poisoned.into_inner()
        });

        let snapshot = self.breakers.load();
        let mut new_map = (**snapshot).clone();
        new_map.remove(backend_id);
        self.breakers.store(Arc::new(new_map));
    }
}

impl Default for CircuitBreakerManager {
    fn default() -> Self {
        Self::new(5, Duration::from_secs(30))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_circuit_breaker_starts_closed() {
        let breaker = CircuitBreaker::new(5, Duration::from_secs(30));

        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.allow_request());
    }

    #[test]
    fn test_circuit_breaker_opens_after_threshold() {
        let breaker = CircuitBreaker::new(3, Duration::from_secs(30));

        // Record 3 failures (threshold)
        for i in 1..=3 {
            breaker.record_failure();

            if i < 3 {
                assert_eq!(
                    breaker.state(),
                    CircuitState::Closed,
                    "Circuit should stay Closed until threshold"
                );
            } else {
                assert_eq!(
                    breaker.state(),
                    CircuitState::Open,
                    "Circuit should Open at threshold"
                );
            }
        }

        // Requests should be blocked
        assert!(
            !breaker.allow_request(),
            "Requests should be blocked in Open state"
        );
    }

    #[test]
    fn test_circuit_breaker_resets_failure_count_on_success() {
        let breaker = CircuitBreaker::new(5, Duration::from_secs(30));

        // Record 2 failures
        breaker.record_failure();
        breaker.record_failure();
        assert_eq!(breaker.failure_count(), 2);

        // Success resets count
        breaker.record_success();
        assert_eq!(breaker.failure_count(), 0);
        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_half_open_after_timeout() {
        let breaker = CircuitBreaker::new(3, Duration::from_millis(100));

        // Open the circuit
        for _ in 0..3 {
            breaker.record_failure();
        }
        assert_eq!(breaker.state(), CircuitState::Open);

        // Wait for timeout
        thread::sleep(Duration::from_millis(150));

        // First request should transition to Half-Open
        assert!(
            breaker.allow_request(),
            "First request after timeout should be allowed (Half-Open)"
        );
        assert_eq!(breaker.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn test_circuit_breaker_half_open_closes_on_success() {
        let breaker = CircuitBreaker::new(3, Duration::from_millis(100));

        // Open the circuit
        for _ in 0..3 {
            breaker.record_failure();
        }

        // Wait for timeout and transition to Half-Open
        thread::sleep(Duration::from_millis(150));
        breaker.allow_request();
        assert_eq!(breaker.state(), CircuitState::HalfOpen);

        // Record 3 successful test requests (half_open_max_requests = 3)
        breaker.record_success();
        breaker.record_success();
        breaker.record_success();

        // Circuit should close
        assert_eq!(
            breaker.state(),
            CircuitState::Closed,
            "Circuit should close after successful Half-Open tests"
        );
        assert_eq!(breaker.failure_count(), 0);
    }

    #[test]
    fn test_circuit_breaker_half_open_reopens_on_failure() {
        let breaker = CircuitBreaker::new(3, Duration::from_millis(100));

        // Open the circuit
        for _ in 0..3 {
            breaker.record_failure();
        }

        // Wait for timeout and transition to Half-Open
        thread::sleep(Duration::from_millis(150));
        breaker.allow_request();
        assert_eq!(breaker.state(), CircuitState::HalfOpen);

        // Any failure in Half-Open reopens circuit
        breaker.record_failure();
        assert_eq!(
            breaker.state(),
            CircuitState::Open,
            "Circuit should reopen on Half-Open failure"
        );
    }

    #[test]
    fn test_circuit_breaker_half_open_limited_requests() {
        let breaker = CircuitBreaker::new(3, Duration::from_millis(100));

        // Open the circuit
        for _ in 0..3 {
            breaker.record_failure();
        }

        // Wait for timeout
        thread::sleep(Duration::from_millis(150));

        // Half-Open allows limited requests (max 3)
        assert!(breaker.allow_request(), "Request 1 should be allowed");
        assert!(breaker.allow_request(), "Request 2 should be allowed");
        assert!(breaker.allow_request(), "Request 3 should be allowed");
        assert!(
            !breaker.allow_request(),
            "Request 4 should be blocked (limit reached)"
        );
    }

    #[test]
    fn test_circuit_breaker_manager_per_backend_isolation() {
        let manager = CircuitBreakerManager::new(3, Duration::from_secs(30));

        // Create backend-2 first (simulate a successful request)
        manager.record_success("backend-2");

        // Fail backend-1
        for _ in 0..3 {
            manager.record_failure("backend-1");
        }

        // backend-1 should be Open
        assert_eq!(
            manager.get_state("backend-1"),
            Some(CircuitState::Open),
            "backend-1 should be Open"
        );

        // backend-2 should still be Closed (isolated from backend-1)
        assert_eq!(
            manager.get_state("backend-2"),
            Some(CircuitState::Closed),
            "backend-2 should be Closed (isolated from backend-1)"
        );
        assert!(
            manager.allow_request("backend-2"),
            "backend-2 should allow requests"
        );
    }

    #[test]
    fn test_circuit_breaker_manager_create_on_demand() {
        let manager = CircuitBreakerManager::new(3, Duration::from_secs(30));

        // get_state on nonexistent backend returns None
        assert_eq!(manager.get_state("new-backend"), None);

        // get_breaker creates a new breaker
        let breaker = manager.get_breaker("new-backend");
        assert_eq!(breaker.state(), CircuitState::Closed);

        // Second access reuses breaker
        manager.record_failure("new-backend");
        assert_eq!(manager.get_breaker("new-backend").failure_count(), 1);
    }

    #[test]
    fn test_circuit_breaker_manager_remove_backend() {
        let manager = CircuitBreakerManager::new(3, Duration::from_secs(30));

        // Create breaker for backend
        manager.record_failure("backend-1");
        assert_eq!(manager.get_state("backend-1"), Some(CircuitState::Closed));
        assert_eq!(manager.get_breaker("backend-1").failure_count(), 1);

        // Remove backend
        manager.remove_backend("backend-1");
        assert_eq!(
            manager.get_state("backend-1"),
            None,
            "get_state should return None after removal"
        );

        // get_breaker creates a fresh breaker
        let new_breaker = manager.get_breaker("backend-1");
        assert_eq!(new_breaker.state(), CircuitState::Closed);
        assert_eq!(
            new_breaker.failure_count(),
            0,
            "New breaker should have zero failures"
        );
    }

    #[test]
    fn test_circuit_breaker_concurrent_access() {
        use std::sync::Arc;

        let breaker = Arc::new(CircuitBreaker::new(10, Duration::from_secs(30)));

        // Spawn 5 threads, each recording 2 failures
        let handles: Vec<_> = (0..5)
            .map(|_| {
                let breaker = Arc::clone(&breaker);
                thread::spawn(move || {
                    breaker.record_failure();
                    breaker.record_failure();
                })
            })
            .collect();

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        // Total 10 failures (exactly at threshold)
        assert_eq!(
            breaker.state(),
            CircuitState::Open,
            "Circuit should Open after 10 concurrent failures"
        );
    }

    #[test]
    fn test_circuit_breaker_metrics_recorded() {
        let manager = CircuitBreakerManager::new(3, Duration::from_secs(30));

        // Allow request (should be allowed initially)
        assert!(manager.allow_request("backend1"));

        // Record some failures to trigger state change
        manager.record_failure("backend1");
        manager.record_failure("backend1");
        manager.record_failure("backend1");

        // Circuit should now be Open
        assert_eq!(manager.get_state("backend1"), Some(CircuitState::Open));

        // Check metrics are accessible
        let metrics = crate::proxy::circuit_breaker::circuit_breaker_registry().gather();

        // Should have rauta_circuit_breaker_state metric
        let has_state_metric = metrics
            .iter()
            .any(|family| family.name() == "rauta_circuit_breaker_state");
        assert!(
            has_state_metric,
            "Should have rauta_circuit_breaker_state metric"
        );

        // Should have rauta_circuit_breaker_requests_total metric
        let has_requests_metric = metrics
            .iter()
            .any(|family| family.name() == "rauta_circuit_breaker_requests_total");
        assert!(
            has_requests_metric,
            "Should have rauta_circuit_breaker_requests_total metric"
        );

        // Should have rauta_circuit_breaker_failures_total metric
        let has_failures_metric = metrics
            .iter()
            .any(|family| family.name() == "rauta_circuit_breaker_failures_total");
        assert!(
            has_failures_metric,
            "Should have rauta_circuit_breaker_failures_total metric"
        );

        // Should have rauta_circuit_breaker_transitions_total metric
        let has_transitions_metric = metrics
            .iter()
            .any(|family| family.name() == "rauta_circuit_breaker_transitions_total");
        assert!(
            has_transitions_metric,
            "Should have rauta_circuit_breaker_transitions_total metric"
        );
    }

    #[test]
    fn test_circuit_breaker_state_transitions_full_cycle() {
        let breaker = CircuitBreaker::new(2, Duration::from_millis(100));

        // 1. Start Closed
        assert_eq!(breaker.state(), CircuitState::Closed);

        // 2. Fail twice → Open
        breaker.record_failure();
        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Open);

        // 3. Wait for timeout → Half-Open
        thread::sleep(Duration::from_millis(150));
        breaker.allow_request();
        assert_eq!(breaker.state(), CircuitState::HalfOpen);

        // 4. Succeed 3 times → Closed
        breaker.record_success();
        breaker.record_success();
        breaker.record_success();
        assert_eq!(breaker.state(), CircuitState::Closed);

        // 5. Verify failure count reset
        assert_eq!(breaker.failure_count(), 0);
    }

    #[test]
    fn test_packed_state_roundtrip() {
        // Verify packing/unpacking is correct
        let packed = pack_state(STATE_OPEN, 42, 7, 3);
        assert_eq!(unpack_state_field(packed), STATE_OPEN);
        assert_eq!(unpack_failures(packed), 42);
        assert_eq!(unpack_successes(packed), 7);
        assert_eq!(unpack_half_open_reqs(packed), 3);

        let packed2 = pack_state(STATE_CLOSED, 0, 0, 0);
        assert_eq!(unpack_state_field(packed2), STATE_CLOSED);
        assert_eq!(unpack_failures(packed2), 0);

        let packed3 = pack_state(STATE_HALFOPEN, 65535, 65535, 65535);
        assert_eq!(unpack_state_field(packed3), STATE_HALFOPEN);
        assert_eq!(unpack_failures(packed3), 65535);
        assert_eq!(unpack_successes(packed3), 65535);
        assert_eq!(unpack_half_open_reqs(packed3), 65535);
    }
}
