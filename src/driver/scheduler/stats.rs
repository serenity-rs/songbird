use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use super::ScheduleMode;

#[allow(missing_docs)]
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

#[allow(missing_docs)]
#[derive(Debug, Default)]
pub struct LiveStatBlock {
    live: AtomicU64,
    last_ns: AtomicU64,
}

#[allow(missing_docs)]
impl LiveStatBlock {
    #[inline]
    pub fn live_mixers(&self) -> u64 {
        self.live.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn add_mixer(&self) {
        self.live.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn remove_mixer(&self) {
        self.live.fetch_sub(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn store_compute_cost(&self, work: Duration) {
        self.last_ns
            .store(work.as_nanos() as u64, Ordering::Relaxed);
    }

    #[inline]
    pub fn last_compute_cost_ns(&self) -> u64 {
        self.last_ns.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn has_room(&self, strategy: &ScheduleMode) -> bool {
        self.live_mixers() < (strategy.task_limit() as u64)
    }
}
