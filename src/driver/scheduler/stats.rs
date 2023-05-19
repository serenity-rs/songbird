use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use super::{ParkedMixer, ScheduleMode, RESCHEDULE_THRESHOLD};

/// Statistics shared by an entire `Scheduler`.
#[derive(Debug, Default)]
pub struct StatBlock {
    total: AtomicU64,
    live: AtomicU64,
    threads: AtomicU64,
}

#[allow(missing_docs)]
impl StatBlock {
    #[inline]
    pub fn total_mixers(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn live_mixers(&self) -> u64 {
        self.live.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn worker_threads(&self) -> u64 {
        self.threads.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn add_idle_mixer(&self) {
        self.total.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn remove_idle_mixer(&self) {
        self.total.fetch_sub(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn move_mixer_to_live(&self) {
        self.live.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn move_mixer_to_idle(&self) {
        self.live.fetch_sub(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn remove_live_mixer(&self) {
        self.move_mixer_to_idle();
        self.remove_idle_mixer();
    }

    #[inline]
    pub fn add_worker(&self) {
        self.threads.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn remove_worker(&self) {
        self.threads.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Runtime statistics for an individual worker.
///
/// Individual statistics are measured atomically -- the worker thread
/// may have been cleaned up, or its mixer count may not match the
/// count when [`Self::last_compute_cost_ns`] was set.
#[derive(Debug, Default)]
pub struct LiveStatBlock {
    live: AtomicU64,
    last_ns: AtomicU64,
}

impl LiveStatBlock {
    /// Returns the number of mixer tasks scheduled on this worker thread.
    #[inline]
    pub fn live_mixers(&self) -> u64 {
        self.live.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn add_mixer(&self) {
        self.live.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn remove_mixer(&self) {
        self.live.fetch_sub(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn store_compute_cost(&self, work: Duration) {
        self.last_ns
            .store(work.as_nanos() as u64, Ordering::Relaxed);
    }

    /// Returns the number of nanoseconds required to process all worker threads'
    /// packet transmission, mixing, encoding, and encryption in the last tick.
    #[inline]
    pub fn last_compute_cost_ns(&self) -> u64 {
        self.last_ns.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn has_room(&self, strategy: &ScheduleMode, task: &ParkedMixer) -> bool {
        let task_room = strategy
            .task_limit()
            .map(|limit| self.live_mixers() < limit as u64)
            .unwrap_or(true);

        let exec_room = task
            .last_cost
            .map(|cost| cost.as_nanos() as u64 + self.last_compute_cost_ns() < RESCHEDULE_THRESHOLD)
            .unwrap_or(true);

        println!("{task_room} {exec_room}");

        task_room && exec_room
    }
}
