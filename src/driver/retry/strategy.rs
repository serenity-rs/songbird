use rand::random;
use std::time::Duration;

/// Logic used to determine how long to wait between retry attempts.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Strategy {
    /// The driver will wait for the same amount of time between each retry.
    Every(Duration),
    /// Exponential backoff waiting strategy, where the duration between
    /// attempts (approximately) doubles each time.
    Backoff(ExponentialBackoff),
}

impl Strategy {
    pub(crate) fn retry_in(&self, last_wait: Option<Duration>) -> Duration {
        match self {
            Self::Every(t) => *t,
            Self::Backoff(exp) => exp.retry_in(last_wait),
        }
    }
}

/// Exponential backoff waiting strategy.
///
/// Each attempt waits for twice the last delay plus/minus a
/// random jitter, clamped to a min and max value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExponentialBackoff {
    /// Minimum amount of time to wait between retries.
    ///
    /// *Defaults to 0.25s.*
    pub min: Duration,
    /// Maximum amount of time to wait between retries.
    ///
    /// This will be clamped to `>=` min.
    ///
    /// *Defaults to 10s.*
    pub max: Duration,
    /// Amount of uniform random jitter to apply to generated wait times.
    /// I.e., 0.1 will add +/-10% to generated intervals.
    ///
    /// This is restricted to within +/-100%.
    ///
    /// *Defaults to `0.1`.*
    pub jitter: f32,
}

impl Default for ExponentialBackoff {
    fn default() -> Self {
        Self {
            min: Duration::from_millis(250),
            max: Duration::from_secs(10),
            jitter: 0.1,
        }
    }
}

impl ExponentialBackoff {
    pub(crate) fn retry_in(&self, last_wait: Option<Duration>) -> Duration {
        let attempt = last_wait.map_or(self.min, |t| 2 * t);
        let perturb = (1.0 - (self.jitter * 2.0 * (random::<f32>() - 1.0))).clamp(0.0, 2.0);
        let mut target_time = attempt.mul_f32(perturb);

        // Now clamp target time into given range.
        let safe_max = if self.max < self.min {
            self.min
        } else {
            self.max
        };

        target_time = target_time.clamp(self.min, safe_max);

        target_time
    }
}
