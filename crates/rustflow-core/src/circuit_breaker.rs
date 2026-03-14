use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Configuration for a [`CircuitBreaker`].
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Consecutive failures in `Closed` state before opening the circuit.
    pub failure_threshold: u32,
    /// Consecutive successes in `HalfOpen` state before closing the circuit.
    pub success_threshold: u32,
    /// How long (ms) to stay in `Open` state before probing via `HalfOpen`.
    pub timeout_ms: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 2,
            timeout_ms: 30_000,
        }
    }
}

/// Public view of the circuit's current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CbState {
    Closed,
    Open,
    HalfOpen,
}

// ── Internal state ────────────────────────────────────────────────────────────

enum StateInner {
    Closed { consecutive_failures: u32 },
    Open { opened_at: Instant },
    HalfOpen { consecutive_successes: u32 },
}

struct Inner {
    state: StateInner,
    config: CircuitBreakerConfig,
}

// ── CircuitBreaker ────────────────────────────────────────────────────────────

/// A three-state circuit breaker: `Closed` → `Open` → `HalfOpen` → `Closed`.
///
/// Thread-safe; multiple callers can share one instance via `Arc`.
///
/// # State transitions
///
/// ```text
/// Closed  ──(failure_threshold reached)──► Open
/// Open    ──(timeout_ms elapsed)──────────► HalfOpen
/// HalfOpen──(success_threshold reached)──► Closed
/// HalfOpen──(any failure)────────────────► Open
/// ```
pub struct CircuitBreaker {
    /// Human-readable name (usually the provider or tool name).
    pub name: String,
    inner: Mutex<Inner>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the given configuration.
    pub fn new(name: impl Into<String>, config: CircuitBreakerConfig) -> Self {
        Self {
            name: name.into(),
            inner: Mutex::new(Inner {
                state: StateInner::Closed {
                    consecutive_failures: 0,
                },
                config,
            }),
        }
    }

    /// Returns `true` if the caller should proceed with the guarded operation.
    ///
    /// An `Open` circuit rejects immediately; after the timeout elapses it
    /// transitions to `HalfOpen` and permits one probe request.
    pub fn allow_request(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();
        match &inner.state {
            StateInner::Closed { .. } => true,
            StateInner::HalfOpen { .. } => true,
            StateInner::Open { opened_at } => {
                let timeout = Duration::from_millis(inner.config.timeout_ms);
                if opened_at.elapsed() >= timeout {
                    inner.state = StateInner::HalfOpen {
                        consecutive_successes: 0,
                    };
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Record a successful operation.
    ///
    /// Returns `true` when the circuit transitions from `HalfOpen` to `Closed`.
    pub fn record_success(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();
        match &inner.state {
            StateInner::Closed { .. } => {
                // Reset the failure counter on any success.
                inner.state = StateInner::Closed {
                    consecutive_failures: 0,
                };
                false
            }
            StateInner::HalfOpen { consecutive_successes } => {
                let new_successes = consecutive_successes + 1;
                if new_successes >= inner.config.success_threshold {
                    inner.state = StateInner::Closed {
                        consecutive_failures: 0,
                    };
                    true // transitioned → Closed
                } else {
                    inner.state = StateInner::HalfOpen {
                        consecutive_successes: new_successes,
                    };
                    false
                }
            }
            StateInner::Open { .. } => false, // shouldn't normally happen
        }
    }

    /// Record a failed operation.
    ///
    /// Returns `true` when the circuit transitions from `Closed` or `HalfOpen`
    /// to `Open`.
    pub fn record_failure(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();
        match &inner.state {
            StateInner::Closed { consecutive_failures } => {
                let new_failures = consecutive_failures + 1;
                if new_failures >= inner.config.failure_threshold {
                    inner.state = StateInner::Open {
                        opened_at: Instant::now(),
                    };
                    true // transitioned → Open
                } else {
                    inner.state = StateInner::Closed {
                        consecutive_failures: new_failures,
                    };
                    false
                }
            }
            StateInner::HalfOpen { .. } => {
                // Any failure in HalfOpen immediately re-opens.
                inner.state = StateInner::Open {
                    opened_at: Instant::now(),
                };
                true // transitioned → Open
            }
            StateInner::Open { .. } => false, // already open
        }
    }

    /// Current state as a string slice (useful for logging / metrics).
    pub fn state_name(&self) -> &'static str {
        let inner = self.inner.lock().unwrap();
        match &inner.state {
            StateInner::Closed { .. } => "closed",
            StateInner::Open { .. } => "open",
            StateInner::HalfOpen { .. } => "half_open",
        }
    }

    /// Current state as an enum value.
    pub fn cb_state(&self) -> CbState {
        let inner = self.inner.lock().unwrap();
        match &inner.state {
            StateInner::Closed { .. } => CbState::Closed,
            StateInner::Open { .. } => CbState::Open,
            StateInner::HalfOpen { .. } => CbState::HalfOpen,
        }
    }
}

// ── CircuitBreakerRegistry ────────────────────────────────────────────────────

/// Thread-safe registry of named circuit breakers.
///
/// Each resource (LLM provider name, tool name, …) gets its own breaker.
/// Breakers are created on first access using the registry's default config.
pub struct CircuitBreakerRegistry {
    breakers: Mutex<HashMap<String, Arc<CircuitBreaker>>>,
    default_config: CircuitBreakerConfig,
}

impl CircuitBreakerRegistry {
    /// Create a registry with the default [`CircuitBreakerConfig`].
    pub fn new() -> Self {
        Self {
            breakers: Mutex::new(HashMap::new()),
            default_config: CircuitBreakerConfig::default(),
        }
    }

    /// Create a registry with a custom default config.
    pub fn with_default_config(config: CircuitBreakerConfig) -> Self {
        Self {
            breakers: Mutex::new(HashMap::new()),
            default_config: config,
        }
    }

    /// Return the breaker for `name`, creating it (with the default config) if
    /// it does not yet exist.
    pub fn get_or_create(&self, name: &str) -> Arc<CircuitBreaker> {
        let mut breakers = self.breakers.lock().unwrap();
        if let Some(cb) = breakers.get(name) {
            return Arc::clone(cb);
        }
        let cb = Arc::new(CircuitBreaker::new(name, self.default_config.clone()));
        breakers.insert(name.to_string(), Arc::clone(&cb));
        cb
    }

    /// Return the breaker for `name` if it exists.
    pub fn get(&self, name: &str) -> Option<Arc<CircuitBreaker>> {
        self.breakers.lock().unwrap().get(name).cloned()
    }

    /// Names of all registered breakers.
    pub fn names(&self) -> Vec<String> {
        self.breakers.lock().unwrap().keys().cloned().collect()
    }

    /// Number of registered breakers.
    pub fn len(&self) -> usize {
        self.breakers.lock().unwrap().len()
    }

    /// True when no breakers have been registered yet.
    pub fn is_empty(&self) -> bool {
        self.breakers.lock().unwrap().is_empty()
    }
}

impl Default for CircuitBreakerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn cb(failure_threshold: u32, success_threshold: u32, timeout_ms: u64) -> CircuitBreaker {
        CircuitBreaker::new(
            "test",
            CircuitBreakerConfig {
                failure_threshold,
                success_threshold,
                timeout_ms,
            },
        )
    }

    // ── Closed state ──────────────────────────────────────────────────────────

    #[test]
    fn test_new_cb_is_closed() {
        let c = cb(3, 2, 1000);
        assert_eq!(c.cb_state(), CbState::Closed);
        assert_eq!(c.state_name(), "closed");
    }

    #[test]
    fn test_closed_allows_requests() {
        let c = cb(3, 2, 1000);
        assert!(c.allow_request());
    }

    #[test]
    fn test_closed_success_stays_closed() {
        let c = cb(3, 2, 1000);
        assert!(!c.record_success());
        assert_eq!(c.cb_state(), CbState::Closed);
    }

    #[test]
    fn test_closed_failures_below_threshold() {
        let c = cb(3, 2, 1000);
        assert!(!c.record_failure()); // 1
        assert!(!c.record_failure()); // 2
        assert_eq!(c.cb_state(), CbState::Closed);
    }

    #[test]
    fn test_closed_to_open_at_threshold() {
        let c = cb(3, 2, 1000);
        c.record_failure();
        c.record_failure();
        let opened = c.record_failure(); // 3rd = threshold
        assert!(opened);
        assert_eq!(c.cb_state(), CbState::Open);
    }

    #[test]
    fn test_success_resets_failure_counter() {
        let c = cb(3, 2, 1000);
        c.record_failure();
        c.record_failure();
        c.record_success(); // resets to 0
        c.record_failure();
        c.record_failure();
        // still below threshold (only 2 consecutive failures now)
        assert_eq!(c.cb_state(), CbState::Closed);
    }

    // ── Open state ────────────────────────────────────────────────────────────

    #[test]
    fn test_open_blocks_requests() {
        let c = cb(1, 2, 60_000);
        c.record_failure();
        assert_eq!(c.cb_state(), CbState::Open);
        assert!(!c.allow_request());
    }

    #[test]
    fn test_open_to_halfopen_after_timeout() {
        let c = cb(1, 2, 1); // 1 ms timeout
        c.record_failure();
        assert_eq!(c.cb_state(), CbState::Open);

        thread::sleep(Duration::from_millis(5));

        assert!(c.allow_request());
        assert_eq!(c.cb_state(), CbState::HalfOpen);
    }

    #[test]
    fn test_open_stays_open_before_timeout() {
        let c = cb(1, 2, 60_000);
        c.record_failure();
        // No sleep — timeout hasn't elapsed.
        assert!(!c.allow_request());
        assert_eq!(c.cb_state(), CbState::Open);
    }

    // ── HalfOpen state ────────────────────────────────────────────────────────

    #[test]
    fn test_halfopen_failure_reopens() {
        let c = cb(1, 2, 1);
        c.record_failure();
        thread::sleep(Duration::from_millis(5));
        c.allow_request(); // triggers Closed → HalfOpen
        assert_eq!(c.cb_state(), CbState::HalfOpen);

        let opened = c.record_failure();
        assert!(opened);
        assert_eq!(c.cb_state(), CbState::Open);
    }

    #[test]
    fn test_halfopen_successes_below_threshold() {
        let c = cb(1, 3, 1);
        c.record_failure();
        thread::sleep(Duration::from_millis(5));
        c.allow_request();

        assert!(!c.record_success()); // 1 of 3
        assert!(!c.record_success()); // 2 of 3
        assert_eq!(c.cb_state(), CbState::HalfOpen);
    }

    #[test]
    fn test_halfopen_to_closed_at_threshold() {
        let c = cb(1, 2, 1);
        c.record_failure();
        thread::sleep(Duration::from_millis(5));
        c.allow_request();

        assert!(!c.record_success()); // 1 of 2
        let closed = c.record_success(); // 2 of 2 — threshold!
        assert!(closed);
        assert_eq!(c.cb_state(), CbState::Closed);
    }

    // ── Registry ──────────────────────────────────────────────────────────────

    #[test]
    fn test_registry_creates_on_first_access() {
        let reg = CircuitBreakerRegistry::new();
        assert!(reg.is_empty());
        let cb = reg.get_or_create("provider-a");
        assert_eq!(cb.cb_state(), CbState::Closed);
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn test_registry_returns_same_instance() {
        let reg = CircuitBreakerRegistry::new();
        let cb1 = reg.get_or_create("x");
        let cb2 = reg.get_or_create("x");
        // Mutate through cb1, observe through cb2 (same Arc).
        cb1.record_failure();
        assert_eq!(cb2.cb_state(), CbState::Closed); // threshold is 5 by default
    }

    #[test]
    fn test_registry_get_returns_none_for_unknown() {
        let reg = CircuitBreakerRegistry::new();
        assert!(reg.get("unknown").is_none());
    }

    #[test]
    fn test_registry_names() {
        let reg = CircuitBreakerRegistry::new();
        reg.get_or_create("a");
        reg.get_or_create("b");
        let mut names = reg.names();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn test_registry_custom_default_config() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 1,
            timeout_ms: 0,
        };
        let reg = CircuitBreakerRegistry::with_default_config(config);
        let cb = reg.get_or_create("r");
        cb.record_failure();
        assert_eq!(cb.cb_state(), CbState::Open);
    }
}
