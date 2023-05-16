use std::{num::NonZeroUsize, sync::Arc};

use flume::{Receiver, Sender};
use once_cell::sync::Lazy;

use crate::Config;

use super::tasks::message::{Interconnect, MixerMessage};

mod idle;
mod live;
mod stats;
mod task;

use idle::*;
pub use live::*;
pub use stats::*;
pub use task::*;

/// The default shared scheduler instance.
///
/// This is built using the default calue of [`ScheduleMode`]. Users desiring
/// a custom strategy should avoid calling [`Config::default`].
///
/// [`Config::default`]: crate::Config::default
pub static DEFAULT_SCHEDULER: Lazy<Scheduler> = Lazy::new(Scheduler::default);

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
    fn prealloc_size(&self) -> usize {
        match self {
            Self::MaxPerThread(n) => n.get(),
        }
    }

    fn task_limit(&self) -> usize {
        match self {
            Self::MaxPerThread(n) => n.get(),
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

/// A reference to a shared group of threads used for running idle and active
/// audio threads.
#[derive(Clone, Debug)]
pub struct Scheduler {
    inner: Arc<InnerScheduler>,
}

#[derive(Clone, Debug)]
struct InnerScheduler {
    tx: Sender<SchedulerMessage>,
    stats: Arc<StatBlock>,
}

// tricky part of the loop -- how do we let reconns barrel through as fast as possible?
// -- not hard, it's sent along the existing core -> mixer channel

impl Scheduler {
    /// Create a new mixer scheduler from the allocation strategy
    /// `mode`.
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
    pub fn total_tasks(&self) -> u64 {
        self.inner.stats.total_mixers()
    }

    /// Returns the total number of *active* calls scheduled and processing
    /// audio.
    pub fn live_tasks(&self) -> u64 {
        self.inner.stats.live_mixers()
    }

    /// Returns the total number of threads spawned to process live audio sessions.
    pub fn worker_threads(&self) -> u64 {
        self.inner.stats.worker_threads()
    }
}

impl Drop for InnerScheduler {
    fn drop(&mut self) {
        self.tx.send(SchedulerMessage::Kill);
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Scheduler::new(Default::default())
    }
}

#[allow(missing_docs)]
pub enum SchedulerMessage {
    NewMixer(Receiver<MixerMessage>, Interconnect, Config),
    Do(TaskId, MixerMessage),
    Demote(TaskId, ParkedMixer),
    Kill,
}
