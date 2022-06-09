//! Configuration for connection retries.

mod strategy;

pub use self::strategy::*;

use std::time::Duration;

/// Configuration to be used for retrying driver connection attempts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Retry {
    /// Strategy used to determine how long to wait between retry attempts.
    ///
    /// *Defaults to an [`ExponentialBackoff`] from 0.25s
    /// to 10s, with a jitter of `0.1`.*
    ///
    /// [`ExponentialBackoff`]: Strategy::Backoff
    pub strategy: Strategy,
    /// The maximum number of retries to attempt.
    ///
    /// `None` will attempt an infinite number of retries,
    /// while `Some(0)` will attempt to connect *once* (no retries).
    ///
    /// *Defaults to `Some(5)`.*
    pub retry_limit: Option<usize>,
}

impl Default for Retry {
    fn default() -> Self {
        Self {
            strategy: Strategy::Backoff(ExponentialBackoff::default()),
            retry_limit: Some(5),
        }
    }
}

impl Retry {
    pub(crate) fn retry_in(
        &self,
        last_wait: Option<Duration>,
        attempts: usize,
    ) -> Option<Duration> {
        if self.retry_limit.map_or(true, |a| attempts < a) {
            Some(self.strategy.retry_in(last_wait))
        } else {
            None
        }
    }
}
