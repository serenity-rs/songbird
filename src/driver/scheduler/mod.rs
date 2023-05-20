use std::{error::Error as StdError, fmt::Display, num::NonZeroUsize, sync::Arc};

use flume::{Receiver, RecvError, Sender};
use once_cell::sync::Lazy;

use crate::{constants::TIMESTEP_LENGTH, Config};

use super::tasks::message::{Interconnect, MixerMessage};

mod idle;
mod live;
mod stats;
mod task;

use idle::*;
pub use live::*;
pub use stats::*;
pub use task::*;

/// A soft maximum of 90% of the 20ms budget to account for variance in execution time.
const RESCHEDULE_THRESHOLD: u64 = ((TIMESTEP_LENGTH.subsec_nanos() as u64) * 9) / 10;

/// The default shared scheduler instance.
///
/// This is built using the default calue of [`ScheduleMode`]. Users desiring
/// a custom strategy should avoid calling [`Config::default`].
///
/// [`Config::default`]: crate::Config::default
pub static DEFAULT_SCHEDULER: Lazy<Scheduler> = Lazy::new(Scheduler::default);

/// A reference to a shared group of threads used for running idle and active
/// audio threads.
#[derive(Clone, Debug)]
pub struct Scheduler {
    inner: Arc<InnerScheduler>,
}

/// Inner contents of a [`Scheduler`] instance.
///
/// This is an `Arc` around `Arc`'d contents so that we can make use of the
/// drop check on `Scheduler` to cleanup resources.
#[derive(Clone, Debug)]
struct InnerScheduler {
    tx: Sender<SchedulerMessage>,
    stats: Arc<StatBlock>,
}

impl Scheduler {
    /// Create a new mixer scheduler from the allocation strategy `mode`.
    #[must_use]
    pub fn new(mode: ScheduleMode) -> Self {
        let (core, tx) = Idle::new(mode);

        let stats = core.stats.clone();
        core.spawn();

        let inner = Arc::new(InnerScheduler { tx, stats });

        Self { inner }
    }

    pub(crate) fn new_mixer(&self, config: &Config, ic: Interconnect, rx: Receiver<MixerMessage>) {
        self.inner
            .tx
            .send(SchedulerMessage::NewMixer(rx, ic, config.clone()))
            .unwrap();
    }

    /// Returns the total number of calls (idle and active) scheduled.
    #[must_use]
    pub fn total_tasks(&self) -> u64 {
        self.inner.stats.total_mixers()
    }

    /// Returns the total number of *active* calls scheduled and processing
    /// audio.
    #[must_use]
    pub fn live_tasks(&self) -> u64 {
        self.inner.stats.live_mixers()
    }

    /// Returns the total number of threads spawned to process live audio sessions.
    #[must_use]
    pub fn worker_threads(&self) -> u64 {
        self.inner.stats.worker_threads()
    }

    /// Request a list of handles to statistics for currently live workers.
    pub async fn worker_thread_stats(&self) -> Result<Vec<Arc<LiveStatBlock>>, Error> {
        let (tx, rx) = flume::bounded(1);
        _ = self.inner.tx.send(SchedulerMessage::GetStats(tx));

        rx.recv_async().await.map_err(Error::from)
    }

    /// Request a list of handles to statistics for currently live workers with a blocking call.
    pub fn worker_thread_stats_blocking(&self) -> Result<Vec<Arc<LiveStatBlock>>, Error> {
        let (tx, rx) = flume::bounded(1);
        _ = self.inner.tx.send(SchedulerMessage::GetStats(tx));

        rx.recv().map_err(Error::from)
    }
}

impl Drop for InnerScheduler {
    fn drop(&mut self) {
        _ = self.tx.send(SchedulerMessage::Kill);
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Scheduler::new(ScheduleMode::default())
    }
}

/// Strategies for mapping live mixer tasks to individual threads.
///
/// Defaults to `MaxPerThread(16)`.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum ScheduleMode {
    /// Allows at most `n` tasks to run per thread.
    MaxPerThread(NonZeroUsize),
}

impl ScheduleMode {
    /// Returns the number of `Mixer`s that a scheduler should preallocate
    /// resources for.
    fn prealloc_size(&self) -> usize {
        match self {
            Self::MaxPerThread(n) => n.get(),
        }
    }

    /// Returns the maximum number of concurrent mixers that a scheduler is
    /// allowed to place on a single thread.
    ///
    /// Future scheduling modes may choose to limit *only* on execution cost.
    #[allow(clippy::unnecessary_wraps)]
    fn task_limit(&self) -> Option<usize> {
        match self {
            Self::MaxPerThread(n) => Some(n.get()),
        }
    }
}

const DEFAULT_MIXERS_PER_THREAD: NonZeroUsize = match NonZeroUsize::new(16) {
    Some(v) => v,
    None => [][0],
};

impl Default for ScheduleMode {
    fn default() -> Self {
        Self::MaxPerThread(DEFAULT_MIXERS_PER_THREAD)
    }
}

/// Control messages for a scheduler.
pub enum SchedulerMessage {
    /// Build a new `Mixer` as part of the initialisation of a `Driver`.
    NewMixer(Receiver<MixerMessage>, Interconnect, Config),
    /// Forward a command for
    Do(TaskId, MixerMessage),
    /// Return a `Mixer` from a worker back to the idle pool.
    Demote(TaskId, ParkedMixer),
    /// Move an expensive `Mixer` to another thread in the worker pool.
    Overspill(WorkerId, TaskId, ParkedMixer),
    /// Request a copy of all per-worker statistics.
    GetStats(Sender<Vec<Arc<LiveStatBlock>>>),
    /// Cleanup once all `Scheduler` handles are dropped.
    Kill,
}

/// Errors encountered when communicating with the internals of a [`Scheduler`].
///
/// [`Scheduler`]: crate::driver::Scheduler
#[non_exhaustive]
#[derive(Debug)]
pub enum Error {
    /// The scheduler exited or crashed while awating the request.
    Disconnected,
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disconnected => f.write_str("the scheduler terminated mid-request"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        None
    }
}

impl From<RecvError> for Error {
    fn from(_: RecvError) -> Self {
        Self::Disconnected
    }
}
