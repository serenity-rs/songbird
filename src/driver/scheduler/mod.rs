use std::{
    num::NonZeroUsize,
    sync::{atomic::AtomicU64, Arc},
    time::Instant, collections::HashMap,
};

use flume::{Receiver, Sender};
use nohash_hasher::{IntMap, IsEnabled, BuildNoHashHasher};
use once_cell::sync::Lazy;
use rand::random;
use tokio::runtime::Handle;

use crate::{Config, constants::TIMESTEP_LENGTH};

use super::tasks::{message::{Interconnect, MixerMessage}, mixer::Mixer};

/// The default shared scheduler instance.
///
/// This is built using the default calue of [`ScheduleMode`]. Users desiring
/// a custom strategy should avoid calling [`Config::default`].
///
/// [`Config::default`]: crate::Config::default
pub static DEFAULT_SCHEDULER: Lazy<Scheduler> = Lazy::new(Scheduler::default);

#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct TaskId(usize);

impl IsEnabled for TaskId { }

impl TaskId {
    fn incr(&mut self) -> Self {
        let out = *self;
        self.0 = self.0.wrapping_add(1);
        out
    }
}

/// Strategies for mapping live mixer tasks to individual threads.
///
/// Defaults to `MaxPerThread(16)`.
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
    tasks: IntMap<TaskId, ParkedMixer>,
    // track taskids which are live to prevent their realloc? unlikely w u64 but still
    stats: Arc<StatBlock>,
    rx: Receiver<SchedulerMessage>,
    tx: Sender<SchedulerMessage>,
    next_id: TaskId,
}

impl Core {
    fn new(mode: ScheduleMode) -> (Self, Sender<SchedulerMessage>) {
        let (tx, rx) = flume::unbounded();

        let stats = Default::default();
        let tasks = HashMap::with_capacity_and_hasher(128, BuildNoHashHasher::default());

        // TODO: include heap of keepalive sending times?
        let out = Self {
            mode,
            tasks,
            stats,
            rx,
            tx: tx.clone(),
            next_id: TaskId(0),
        };

        (out, tx)
    }

    async fn run(&mut self) {
        let mut interval = tokio::time::interval(TIMESTEP_LENGTH);
        // await timer events for all parked mixers
        // spawn any NewMixers into tasks which handle easy changes
        // and translate play events etc into a request to be made 'live'.
        // also signal evt context of each thread every 20ms.
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // notify all evt threads.
                    // send any keepalive packets?
                    for task in self.tasks.values() {
                        // the existing "do_events_keepalives" (or w/e) fn covers this.
                        // TODO: integrate with error handling logic.
                        task.mixer.interconnect.tick();
                    }
                },
                msg = self.rx.recv_async() => match msg {
                    Ok(SchedulerMessage::NewMixer(rx, ic, cfg)) => {
                        let remote_tx = self.tx.clone();

                        let mixer = ParkedMixer::new(rx, ic, cfg);
                        let id = self.next_id.incr();

                        mixer.spawn_forwarder(self.tx.clone(), id);
                        self.tasks.insert(id, mixer);
                    },
                    Ok(SchedulerMessage::Do(id, mix_msg)) => {
                        let now_live = mix_msg.is_mixer_now_live();
                        // TODO: call mixer's normal reaction.

                        // hand the msg to the mixer, do the thing.
                        

                        if now_live {
                            let task = self.tasks.remove(&id).unwrap();
                            let elapsed = task.park_time.elapsed();
                            // TODO: promote thread.
                            // TODO: bump up rtp_timestamp according to elapsed time.
                        }

                        todo!()
                    },
                    Ok(SchedulerMessage::Kill) | Err(_) => {
                        break;
                    },
                },
            }
        }
    }

    fn spawn(self) {
        tokio::spawn(self.run());
    }
}

pub(crate) enum SchedulerMessage {
    NewMixer(Receiver<MixerMessage>, Interconnect, Config),
    Do(TaskId, MixerMessage),
    Kill,
}

/// A reference to a shared group of threads used for running idle and active
/// audio threads.
#[derive(Clone, Debug)]
pub struct Scheduler {
    tx: Sender<SchedulerMessage>,
    stats: Arc<StatBlock>,
}

// tricky part of the loop -- how do we let reconns barrel through as fast as possible?
// -- not hard, it's sent along the existing core -> mixer channel

impl Scheduler {
    /// Create a new mixer scheduler from the allocation strategy
    /// `mode`.
    pub fn new(mode: ScheduleMode) -> Self {
        let (core, tx) = Core::new(mode);

        let stats = core.stats.clone();
        core.spawn();

        Self { tx, stats }
    }

    pub(crate) fn new_mixer(&self, config: &Config, ic: Interconnect, rx: Receiver<MixerMessage>) {
        self.tx.send(SchedulerMessage::NewMixer(rx, ic, config.clone()))
            .unwrap();
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Scheduler::new(Default::default())
    }
}

pub struct ParkedMixer {
    mixer: Box<Mixer>,
    rtp_sequence: u16,
    rtp_timestamp: u32,
    park_time: Instant,
}

impl ParkedMixer {
    pub fn new(
        mix_rx: Receiver<MixerMessage>,
        interconnect: Interconnect,
        config: Config,
    ) -> Self {
        Self {
            mixer: Box::new(Mixer::new(mix_rx, Handle::current(), interconnect, config)),
            rtp_sequence: random::<u16>(),
            rtp_timestamp: random::<u32>(),
            park_time: Instant::now(),
        }
    }

    fn spawn_forwarder(&self, tx: Sender<SchedulerMessage>, id: TaskId) {
        let remote_rx = self.mixer.mix_rx.clone();
        tokio::spawn(async move {
            while let Ok(msg) = remote_rx.recv() {
                let exit = msg.is_mixer_now_live();
                tx.send_async(SchedulerMessage::Do(id, msg)).await;
                if exit { break; }
            }
        });
    }
}

pub struct LiveMixers {
    packets: Vec<Vec<u8>>,
    tasks: Vec<Mixer>,
    mode: ScheduleMode,
}
