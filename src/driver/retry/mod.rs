//! Configuration for connection retries.

mod strategy;

pub use self::strategy::*;

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
            strategy: Strategy::Backoff(Default::default()),
            retry_limit: Some(5),
        }
    }
}
