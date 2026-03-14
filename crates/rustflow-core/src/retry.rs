use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Retry policy for a step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[derive(Default)]
pub enum RetryPolicy {
    /// Do not retry on failure.
    #[default]
    None,

    /// Retry a fixed number of times with a constant interval.
    Fixed {
        /// Maximum number of retry attempts.
        max_retries: u32,
        /// Duration to wait between attempts (in milliseconds).
        interval_ms: u64,
    },

    /// Retry with exponentially increasing backoff.
    Exponential {
        /// Maximum number of retry attempts.
        max_retries: u32,
        /// Initial wait duration (in milliseconds).
        initial_interval_ms: u64,
        /// Multiplier applied to the interval on each attempt.
        multiplier: f64,
        /// Maximum interval cap (in milliseconds).
        max_interval_ms: u64,
    },
}

impl RetryPolicy {
    /// Returns the maximum number of retries, or 0 if not retrying.
    pub fn max_retries(&self) -> u32 {
        match self {
            RetryPolicy::None => 0,
            RetryPolicy::Fixed { max_retries, .. } => *max_retries,
            RetryPolicy::Exponential { max_retries, .. } => *max_retries,
        }
    }

    /// Computes the wait duration for a given attempt number (0-indexed).
    pub fn backoff(&self, attempt: u32) -> Duration {
        match self {
            RetryPolicy::None => Duration::ZERO,
            RetryPolicy::Fixed { interval_ms, .. } => Duration::from_millis(*interval_ms),
            RetryPolicy::Exponential {
                initial_interval_ms,
                multiplier,
                max_interval_ms,
                ..
            } => {
                let ms = (*initial_interval_ms as f64) * multiplier.powi(attempt as i32);
                let ms = ms.min(*max_interval_ms as f64) as u64;
                Duration::from_millis(ms)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_none_max_retries() {
        assert_eq!(RetryPolicy::None.max_retries(), 0);
    }

    #[test]
    fn test_none_backoff() {
        assert_eq!(RetryPolicy::None.backoff(0), Duration::ZERO);
        assert_eq!(RetryPolicy::None.backoff(5), Duration::ZERO);
    }

    #[test]
    fn test_fixed_max_retries() {
        let policy = RetryPolicy::Fixed {
            max_retries: 3,
            interval_ms: 1000,
        };
        assert_eq!(policy.max_retries(), 3);
    }

    #[test]
    fn test_fixed_backoff_constant() {
        let policy = RetryPolicy::Fixed {
            max_retries: 3,
            interval_ms: 500,
        };
        assert_eq!(policy.backoff(0), Duration::from_millis(500));
        assert_eq!(policy.backoff(1), Duration::from_millis(500));
        assert_eq!(policy.backoff(10), Duration::from_millis(500));
    }

    #[test]
    fn test_exponential_max_retries() {
        let policy = RetryPolicy::Exponential {
            max_retries: 5,
            initial_interval_ms: 100,
            multiplier: 2.0,
            max_interval_ms: 10000,
        };
        assert_eq!(policy.max_retries(), 5);
    }

    #[test]
    fn test_exponential_backoff_growth() {
        let policy = RetryPolicy::Exponential {
            max_retries: 5,
            initial_interval_ms: 100,
            multiplier: 2.0,
            max_interval_ms: 10000,
        };
        assert_eq!(policy.backoff(0), Duration::from_millis(100));
        assert_eq!(policy.backoff(1), Duration::from_millis(200));
        assert_eq!(policy.backoff(2), Duration::from_millis(400));
        assert_eq!(policy.backoff(3), Duration::from_millis(800));
    }

    #[test]
    fn test_exponential_backoff_capped() {
        let policy = RetryPolicy::Exponential {
            max_retries: 10,
            initial_interval_ms: 1000,
            multiplier: 3.0,
            max_interval_ms: 5000,
        };
        assert_eq!(policy.backoff(0), Duration::from_millis(1000));
        assert_eq!(policy.backoff(1), Duration::from_millis(3000));
        // 1000 * 3^2 = 9000 -> capped to 5000
        assert_eq!(policy.backoff(2), Duration::from_millis(5000));
        assert_eq!(policy.backoff(10), Duration::from_millis(5000));
    }

    #[test]
    fn test_default_is_none() {
        let policy = RetryPolicy::default();
        assert_eq!(policy, RetryPolicy::None);
    }

    #[test]
    fn test_serde_roundtrip_none() {
        let policy = RetryPolicy::None;
        let json = serde_json::to_string(&policy).unwrap();
        let deserialized: RetryPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, RetryPolicy::None);
    }

    #[test]
    fn test_serde_roundtrip_fixed() {
        let policy = RetryPolicy::Fixed {
            max_retries: 3,
            interval_ms: 1000,
        };
        let json = serde_json::to_string(&policy).unwrap();
        let deserialized: RetryPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, policy);
    }

    #[test]
    fn test_serde_roundtrip_exponential() {
        let policy = RetryPolicy::Exponential {
            max_retries: 5,
            initial_interval_ms: 100,
            multiplier: 2.0,
            max_interval_ms: 10000,
        };
        let json = serde_json::to_string(&policy).unwrap();
        let deserialized: RetryPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, policy);
    }
}
