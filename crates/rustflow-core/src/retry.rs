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
