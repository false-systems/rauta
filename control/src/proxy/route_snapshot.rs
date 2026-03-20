//! RCU Route Snapshot — Lock-Free Routing Data
//!
//! Immutable snapshots shared via `ArcSwap`. Readers get an `Arc` via a single
//! atomic load (~1ns). Writers clone-modify-swap under a serializing `Mutex`.
//!
//! Two separate snapshots for different update frequencies:
//! - `RouteData`: routes + prefix_router — changes only on K8s reconciliation
//! - `HealthData`: backend_health + draining_backends — changes per response

use common::Backend;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Backend draining state
#[derive(Debug, Clone)]
pub struct BackendDraining {
    pub deadline: Instant,
}

impl BackendDraining {
    pub fn new(drain_timeout: Duration) -> Self {
        Self {
            deadline: Instant::now() + drain_timeout,
        }
    }

    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.deadline
    }
}

/// Backend health statistics for passive health checking
#[derive(Debug, Clone)]
pub struct BackendHealth {
    window: VecDeque<bool>,
}

const WINDOW_SIZE: usize = 100;

impl Default for BackendHealth {
    fn default() -> Self {
        Self::new()
    }
}

impl BackendHealth {
    pub fn new() -> Self {
        Self {
            window: VecDeque::with_capacity(WINDOW_SIZE),
        }
    }

    pub fn is_healthy(&self) -> bool {
        let total = self.window.len();
        if total == 0 {
            return true;
        }
        let error_count = self.window.iter().filter(|&&is_error| is_error).count();
        let error_rate = error_count as f64 / total as f64;
        error_rate <= 0.5
    }

    pub fn record_response(&mut self, status_code: u16) {
        let is_error = (500..600).contains(&status_code);
        if self.window.len() == WINDOW_SIZE {
            self.window.pop_front();
        }
        self.window.push_back(is_error);
    }
}

/// Mutable health state — updated per response via ArcSwap clone-modify-swap.
/// Small enough that cloning is cheap (~a few KB for typical deployments).
#[derive(Clone)]
pub struct HealthData {
    pub backend_health: HashMap<Backend, BackendHealth>,
    pub draining_backends: HashMap<Backend, BackendDraining>,
}

impl HealthData {
    pub fn new() -> Self {
        Self {
            backend_health: HashMap::new(),
            draining_backends: HashMap::new(),
        }
    }
}

impl Default for HealthData {
    fn default() -> Self {
        Self::new()
    }
}
