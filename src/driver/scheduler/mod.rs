use std::{
    num::NonZeroUsize,
    sync::{atomic::AtomicU64, Arc},
    time::Instant,
};

use flume::{Receiver, Sender};
use once_cell::sync::Lazy;

pub static DEFAULT_SCHEDULER: Lazy<Scheduler> = Lazy::new(Scheduler::default);

#[non_exhaustive]
pub enum ScheduleMode {
    MaxPerThread(NonZeroUsize),
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

#[derive(Debug, Default)]
struct StatBlock {
    total: AtomicU64,
    live: AtomicU64,
}

struct Core {
    mode: ScheduleMode,
    tasks: Vec<ParkedMixer>,
    stats: Arc<StatBlock>,
    rx: Receiver<SchedulerMessage>,
    tx: Sender<SchedulerMessage>,
}

impl Core {
    fn new(mode: ScheduleMode) -> (Self, Sender<SchedulerMessage>) {
        let (tx, rx) = flume::unbounded();

        let stats = Default::default();
        let tasks = Vec::with_capacity(128);

        let out = Self {
            mode,
            tasks,
            stats,
            rx,
            tx: tx.clone(),
        };

        (out, tx)
    }

    async fn run(self) {
    	// await timer events for all parked mixers
    	// spawn any NewMixers into tasks which handle easy changes
    	// and translate play events etc into a request to be made 'live'.
    	// also signal evt context of each thread every 20ms.
    }

    fn spawn(self) {
        tokio::spawn(self.run());
    }
}

pub enum SchedulerMessage {
    NewMixer(Box<Mixer>),
    PromoteMixer(u64),
    Kill,
}

#[derive(Clone, Debug)]
pub struct Scheduler {
    tx: Sender<SchedulerMessage>,
    stats: Arc<StatBlock>,
}

impl Scheduler {
    pub fn new(mode: ScheduleMode) -> Self {
        let (core, tx) = Core::new(mode);

        let stats = core.stats.clone();
        core.spawn();

        Self { tx, stats }
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Scheduler::new(Default::default())
    }
}

// TEMP
pub struct Mixer;

pub struct ParkedMixer {
    mixer: Box<Mixer>,
    rtp_sequence: u16,
    rtp_timestamp: u32,
    park_time: Instant,
}

pub struct LiveMixers {
    packets: Vec<Vec<u8>>,
    tasks: Vec<Mixer>,
    mode: ScheduleMode,
}
