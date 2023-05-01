use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use discortp::rtp::{MutableRtpPacket, RtpPacket};
use flume::{Receiver, Sender, TryRecvError};
use nohash_hasher::{BuildNoHashHasher, IntMap, IsEnabled};
use once_cell::sync::Lazy;
use rand::random;
use tokio::runtime::Handle;

use crate::{constants::*, driver::tasks::error::Error as DriverError, Config};

use super::tasks::{
    message::{EventMessage, Interconnect, MixerMessage},
    mixer::Mixer,
};

#[cfg(test)]
use crate::driver::test_config::{OutputMessage, OutputMode, TickStyle};

/// The default shared scheduler instance.
///
/// This is built using the default calue of [`ScheduleMode`]. Users desiring
/// a custom strategy should avoid calling [`Config::default`].
///
/// [`Config::default`]: crate::Config::default
pub static DEFAULT_SCHEDULER: Lazy<Scheduler> = Lazy::new(Scheduler::default);

#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct TaskId(usize);

impl IsEnabled for TaskId {}

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

#[derive(Debug, Default)]
pub(crate) struct StatBlock {
    total: AtomicU64,
    live: AtomicU64,
}

#[derive(Debug, Default)]
pub(crate) struct LiveStatBlock {
    live: AtomicU64,
    last_ns: AtomicU64,
}

impl LiveStatBlock {
    fn has_room(&self, strategy: &ScheduleMode) -> bool {
        let curr_tasks = self.live.load(Ordering::Relaxed);
        curr_tasks < (strategy.task_limit() as u64)
    }
}

struct Core {
    mode: ScheduleMode,
    tasks: IntMap<TaskId, ParkedMixer>,
    // track taskids which are live to prevent their realloc? unlikely w u64 but still
    stats: Arc<StatBlock>,
    rx: Receiver<SchedulerMessage>,
    tx: Sender<SchedulerMessage>,
    next_id: TaskId,
    workers: Vec<LiveMixers>,
    to_cull: Vec<TaskId>,
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
            workers: Vec::with_capacity(16),
            to_cull: vec![],
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

                    // todo: store keepalive sends in another data structure so
                    // we don't check every task every 20ms.
                    let now = Instant::now();

                    for (id, task) in self.tasks.iter_mut() {
                        if task.tick_and_keepalive(now).is_err() {
                            self.to_cull.push(*id);
                        }

                        // #[cfg(test)]
                        // task.mixer.test_signal_empty_tick();
                    }
                },
                msg = self.rx.recv_async() => match msg {
                    Ok(SchedulerMessage::NewMixer(rx, ic, cfg)) => {
                        let mixer = ParkedMixer::new(rx, ic, cfg);
                        let id = self.next_id.incr();

                        mixer.spawn_forwarder(self.tx.clone(), id);
                        self.tasks.insert(id, mixer);
                        self.stats.total.fetch_add(1, Ordering::Relaxed);
                    },
                    Ok(SchedulerMessage::Demote(id, task)) => {
                        println!("CALL {id:?} was demoted!");
                        task.mixer.send_gateway_not_speaking();

                        task.spawn_forwarder(self.tx.clone(), id);
                        self.tasks.insert(id, task);
                    },
                    Ok(SchedulerMessage::Do(id, mix_msg)) => {
                        let now_live = mix_msg.is_mixer_now_live();
                        let task = self.tasks.get_mut(&id).unwrap();

                        match task.handle_message(mix_msg) {
                            Ok(false) if now_live => {
                                // Promote this task to a live mixer thread.
                                let mut task = self.tasks.remove(&id).unwrap();
                                let worker = self.fetch_worker();
                                if task.send_gateway_speaking().is_ok() {
                                    // TODO: put this task on a valid worker, kill old worker.
                                    worker.stats.live.fetch_add(1, Ordering::Relaxed);
                                    worker.tx.send((id, task))
                                        .expect("Worker thread unexpectedly died!");
                                    self.stats.live.fetch_add(1, Ordering::Relaxed);
                                }
                            },
                            Ok(false) => {},
                            Ok(true) | Err(_) => self.to_cull.push(id),
                        }
                    },
                    Ok(SchedulerMessage::Kill) | Err(_) => {
                        break;
                    },
                },
            }

            for id in self.to_cull.drain(..) {
                self.tasks.remove(&id);
            }
        }
    }

    fn fetch_worker(&mut self) -> &LiveMixers {
        // look through all workers.
        // if none found w/ space, add new.
        let idx = self
            .workers
            .iter()
            .position(|w| w.stats.has_room(&self.mode))
            .unwrap_or_else(|| {
                self.workers.push(LiveMixers::new(
                    self.mode.clone(),
                    self.tx.clone(),
                    self.stats.clone(),
                ));
                self.workers.len() - 1
            });

        &self.workers[idx]
    }

    fn spawn(mut self) {
        tokio::spawn(async move { self.run().await });
    }
}

pub(crate) enum SchedulerMessage {
    NewMixer(Receiver<MixerMessage>, Interconnect, Config),
    Do(TaskId, MixerMessage),
    Demote(TaskId, ParkedMixer),
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
        self.tx
            .send(SchedulerMessage::NewMixer(rx, ic, config.clone()))
            .unwrap();
    }

    /// Returns the total number of calls (idle and active) scheduled.
    pub fn total_tasks(&self) -> u64 {
        self.stats.total.load(Ordering::Relaxed)
    }

    /// Returns the total number of *active* calls scheduled and processing
    /// audio.
    pub fn live_tasks(&self) -> u64 {
        self.stats.live.load(Ordering::Relaxed)
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Scheduler::new(Default::default())
    }
}

pub struct ParkedMixer {
    mixer: Box<Mixer>,
    ssrc: u32,
    rtp_sequence: u16,
    rtp_timestamp: u32,
    park_time: Instant,
}

impl ParkedMixer {
    pub fn new(mix_rx: Receiver<MixerMessage>, interconnect: Interconnect, config: Config) -> Self {
        Self {
            mixer: Box::new(Mixer::new(mix_rx, Handle::current(), interconnect, config)),
            ssrc: 0,
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
                if exit {
                    break;
                }
            }
        });
    }

    /// Returns whether the mixer should exit and be cleaned up.
    fn handle_message(&mut self, msg: MixerMessage) -> Result<bool, ()> {
        match msg {
            MixerMessage::SetConn(conn, ssrc) => {
                // Overridden because
                self.ssrc = ssrc;
                self.rtp_sequence = random::<u16>();
                self.rtp_timestamp = random::<u32>();
                self.mixer.conn_active = Some(conn);
                self.mixer.update_keepalive(ssrc);

                Ok(false)
            },
            MixerMessage::Ws(ws) => {
                // Overridden so that we don't mistakenly tell Discord we're speaking.
                self.mixer.ws = ws;
                self.send_gateway_not_speaking();

                Ok(false)
            },
            msg => {
                let (events_failure, conn_failure, should_exit) =
                    self.mixer.handle_message(msg, &mut []);

                self.mixer
                    .do_rebuilds(events_failure, conn_failure)
                    .map_err(|_| ())
                    .map(|_| should_exit)
            },
        }
    }

    fn tick_and_keepalive(&mut self, now: Instant) -> Result<(), ()> {
        let mut events_failure = self.mixer.fire_event(EventMessage::Tick).is_err();

        let ka_err = self
            .mixer
            .check_and_send_keepalive(Some(now))
            .or_else(DriverError::disarm_would_block);

        let conn_failure = if let Err(e) = ka_err {
            events_failure |= e.should_trigger_interconnect_rebuild();
            e.should_trigger_connect()
        } else {
            false
        };

        self.mixer
            .do_rebuilds(events_failure, conn_failure)
            .map_err(|_| ())
    }

    fn send_gateway_speaking(&mut self) -> Result<(), ()> {
        if let Err(e) = self.mixer.send_gateway_speaking() {
            self.mixer
                .do_rebuilds(
                    e.should_trigger_interconnect_rebuild(),
                    e.should_trigger_connect(),
                )
                .map_err(|_| ())
        } else {
            Ok(())
        }
    }

    fn send_gateway_not_speaking(&self) {
        self.mixer.send_gateway_not_speaking()
    }
}

const PACKETS_PER_BLOCK: usize = 16;

pub struct LiveMixersCore {
    packets: Vec<Box<[u8]>>,
    packet_lens: Vec<usize>,
    tasks: Vec<Box<Mixer>>,
    ids: Vec<TaskId>,
    to_cull: Vec<bool>,

    deadline: Instant,
    start_of_work: Option<Instant>,

    mode: ScheduleMode,
    stats: Arc<LiveStatBlock>,
    global_stats: Arc<StatBlock>,
    rx: Receiver<(TaskId, ParkedMixer)>,
    tx: Sender<SchedulerMessage>,

    #[cfg(feature = "internals")]
    pub skip_sleep: bool,
}

impl LiveMixersCore {
    fn new(
        mode: ScheduleMode,
        global_stats: Arc<StatBlock>,
        stats: Arc<LiveStatBlock>,
        rx: Receiver<(TaskId, ParkedMixer)>,
        tx: Sender<SchedulerMessage>,
    ) -> Self {
        let to_prealloc = mode.prealloc_size();

        // TODO: benchmark 2048-byte size slices (wrt lookup/indexing cost.)
        // TODO: allow for different packet block-alloc sizes
        let packets = vec![packet_block(PACKETS_PER_BLOCK)];

        Self {
            packets,
            packet_lens: Vec::with_capacity(to_prealloc),
            tasks: Vec::with_capacity(to_prealloc),
            ids: Vec::with_capacity(to_prealloc),
            to_cull: Vec::with_capacity(to_prealloc),

            deadline: Instant::now(),
            start_of_work: None,

            mode,
            stats,
            global_stats,
            rx,
            tx,

            #[cfg(feature = "internals")]
            skip_sleep: false,
        }
    }

    #[inline]
    fn run(&mut self) {
        while self.run_once() {}
    }

    #[inline]
    pub fn run_once(&mut self) -> bool {
        // Check for new tasks.
        if self.handle_scheduler_msgs().is_err() {
            return false;
        }

        // Receive commands for each task.
        self.handle_task_msgs();

        // Move any idle calls back to the global pool.
        self.demote_and_remove_mixers();

        for ((packet, packet_len), mixer) in self
            .packets
            .iter_mut()
            .flat_map(|v| v.chunks_exact_mut(VOICE_PACKET_MAX))
            .zip(self.packet_lens.iter_mut())
            .zip(self.tasks.iter_mut())
        {
            match mixer.mix_and_build_packet(packet) {
                Ok(written_sz) => *packet_len = written_sz,
                Err(e) => {
                    *packet_len = 0;
                    // TODO: let culling happen here too (?)
                    _ = mixer.do_rebuilds(
                        e.should_trigger_interconnect_rebuild(),
                        e.should_trigger_connect(),
                    );
                },
            }
        }

        // TODO dealloc blocks every... 1 min?

        let end_of_work = Instant::now();

        if let Some(start_of_work) = self.start_of_work {
            let work: Duration = end_of_work - start_of_work;
            self.stats
                .last_ns
                .store(work.as_nanos() as u64, Ordering::Relaxed);
        }

        // Wait till the right time to send this packet:
        // usually a 20ms tick, in test modes this is either a finite number of runs or user input.
        self.march_deadline();

        // Send all.
        self.start_of_work = Some(Instant::now());
        for ((packet, packet_len), mixer) in self
            .packets
            .iter_mut()
            .flat_map(|v| v.chunks_exact_mut(VOICE_PACKET_MAX))
            .zip(self.packet_lens.iter())
            .zip(self.tasks.iter())
        {
            if *packet_len > 0 {
                mixer.send_packet(&packet[..*packet_len]);
            }
            #[cfg(test)]
            if *packet_len == 0 {
                mixer.test_signal_empty_tick();
            }
            advance_rtp_counters(packet);
        }

        for mixer in self.tasks.iter_mut() {
            mixer.audio_commands_events();
            mixer.check_and_send_keepalive(self.start_of_work);
        }

        true
    }

    #[cfg(test)]
    fn _march_deadline(&mut self) {
        // For testing, assume all will have same tick style.
        // Only count 'remaining loops' on one of the nodes.
        let mixer = self.tasks.get_mut(0).map(|m| {
            let style = m.config.tick_style.clone();
            (m, style)
        });

        match mixer {
            None | Some((_, TickStyle::Timed)) => {
                std::thread::sleep(self.deadline.saturating_duration_since(Instant::now()));
                self.deadline += TIMESTEP_LENGTH;
            },
            Some((m, TickStyle::UntimedWithExecLimit(rx))) => {
                if m.remaining_loops.is_none() {
                    if let Ok(new_val) = rx.recv() {
                        m.remaining_loops = Some(new_val.wrapping_sub(1));
                    }
                }

                if let Some(cnt) = m.remaining_loops.as_mut() {
                    if *cnt == 0 {
                        m.remaining_loops = None;
                    } else {
                        *cnt = cnt.wrapping_sub(1);
                    }
                }
            },
        }
    }

    #[cfg(not(test))]
    #[inline(always)]
    #[allow(clippy::inline_always)] // Justified, this is a very very hot path
    fn _march_deadline(&mut self) {
        std::thread::sleep(self.deadline.saturating_duration_since(Instant::now()));
        self.deadline += TIMESTEP_LENGTH;
    }

    #[inline]
    fn march_deadline(&mut self) {
        #[cfg(feature = "internals")]
        if self.skip_sleep {
            return;
        }

        self._march_deadline();
    }

    #[inline]
    fn handle_scheduler_msgs(&mut self) -> Result<(), ()> {
        let mut activation_time = None;
        loop {
            match self.rx.try_recv() {
                Ok((id, task)) => {
                    println!("CALL {id:?} was promoted!");
                    self.add_task(
                        task,
                        id,
                        *activation_time.get_or_insert_with(|| (self.deadline - TIMESTEP_LENGTH)),
                    );
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Err(()),
            }
        }

        Ok(())
    }

    #[inline]
    fn handle_task_msgs(&mut self) {
        for (i, (packet, mixer)) in self
            .packets
            .iter_mut()
            .flat_map(|v| v.chunks_exact_mut(VOICE_PACKET_MAX))
            .zip(self.tasks.iter_mut())
            .enumerate()
        {
            let mut events_failure = false;
            let mut conn_failure = false;

            let fatal = loop {
                match mixer.mix_rx.try_recv() {
                    Ok(m) => {
                        let (events, conn, should_exit) = mixer.handle_message(m, packet);
                        events_failure |= events;
                        conn_failure |= conn;

                        if should_exit {
                            break true;
                        }
                    },
                    Err(TryRecvError::Disconnected) => {
                        break true;
                    },

                    Err(TryRecvError::Empty) => {
                        break false;
                    },
                }
            };

            if fatal || mixer.do_rebuilds(events_failure, conn_failure).is_err() {
                // this is not zipped in because it is *not* needed most ticks.
                self.to_cull[i] = true;
            }
        }
    }

    #[inline]
    fn demote_and_remove_mixers(&mut self) {
        let mut i = 0;
        while i < self.tasks.len() {
            #[cfg(test)]
            let force_conn = self.tasks[i].config.override_connection.is_some();
            #[cfg(not(test))]
            let force_conn = false;

            if self.to_cull[i]
                || (self.tasks[i].tracks.is_empty() && self.tasks[i].silence_frames == 0)
                || !(self.tasks[i].conn_active.is_some() || force_conn)
            {
                if let Some((id, parked)) = self.remove_task(i) {
                    let _ = self.tx.send(SchedulerMessage::Demote(id, parked));
                } else {
                    self.global_stats.total.fetch_sub(1, Ordering::Relaxed);
                }

                self.stats.live.fetch_sub(1, Ordering::Relaxed);
                self.global_stats.live.fetch_sub(1, Ordering::Relaxed);
            } else {
                i += 1;
            }
        }

        // TODO: test whether asserts speed this up.
    }

    #[inline]
    fn add_task(&mut self, task: ParkedMixer, id: TaskId, activation_time: Instant) {
        let idx = self.ids.len();

        let elapsed = task.park_time - activation_time;

        let samples_f64 = elapsed.as_secs_f64() * (SAMPLE_RATE_RAW as f64);
        let mod_samples = (samples_f64 as u64) as u32;
        let rtp_timestamp = task.rtp_timestamp.wrapping_add(mod_samples);

        self.ids.push(id);
        self.tasks.push(task.mixer);
        self.packet_lens.push(0);
        self.to_cull.push(false);

        let block_size = self.mode.prealloc_size();
        let block = idx / block_size;
        let inner_idx = idx % block_size;

        while self.packets.len() <= block {
            self.packets.push(packet_block(PACKETS_PER_BLOCK));
        }
        let packet = &mut self.packets[block][inner_idx * VOICE_PACKET_MAX..][..VOICE_PACKET_MAX];

        let mut rtp = MutableRtpPacket::new(packet).expect(
            "FATAL: Too few bytes in self.packet for RTP header.\
                (Blame: VOICE_PACKET_MAX?)",
        );
        rtp.set_ssrc(task.ssrc);
        rtp.set_timestamp(rtp_timestamp.into());
        rtp.set_sequence(task.rtp_sequence.into());
    }

    #[inline]
    fn remove_task(&mut self, i: usize) -> Option<(TaskId, ParkedMixer)> {
        // TO REMOVE:
        // swap-remove on all relevant stores.
        // simulate swap-remove on buffer contents:
        //  move important packet header fields from end into i
        let end = self.tasks.len() - 1;

        let id = self.ids.swap_remove(i);
        let _len = self.packet_lens.swap_remove(i);
        let mixer = self.tasks.swap_remove(i);
        let alive = !self.to_cull.swap_remove(i);

        let block_size = self.mode.prealloc_size();
        let block = i / block_size;
        let inner_idx = i % block_size;

        // TODO: consider replacing this with a memcpy (10B).
        //  issue -- tricky to get &muts over both packet bodies
        let replacement = (end > i).then(|| {
            let end_block = end / block_size;
            let end_inner = end % block_size;

            let end_packet =
                &mut self.packets[end_block][end_inner * VOICE_PACKET_MAX..][..VOICE_PACKET_MAX];

            let rtp = RtpPacket::new(end_packet).expect(
                "FATAL: Too few bytes in self.packet for RTP header.\
                    (Blame: VOICE_PACKET_MAX?)",
            );

            (rtp.get_sequence(), rtp.get_timestamp(), rtp.get_ssrc())
        });

        let packet = &mut self.packets[block][inner_idx * VOICE_PACKET_MAX..][..VOICE_PACKET_MAX];
        let mut rtp = MutableRtpPacket::new(packet).expect(
            "FATAL: Too few bytes in self.packet for RTP header.\
                (Blame: VOICE_PACKET_MAX?)",
        );
        let ssrc = rtp.get_ssrc();
        let rtp_timestamp = rtp.get_timestamp().into();
        let rtp_sequence = rtp.get_sequence().into();

        if let Some((seq, ts, ssrc)) = replacement {
            rtp.set_sequence(seq);
            rtp.set_timestamp(ts);
            rtp.set_ssrc(ssrc);
        }

        alive.then(move || {
            let park_time = Instant::now();

            (
                id,
                ParkedMixer {
                    mixer,
                    ssrc,
                    rtp_sequence,
                    rtp_timestamp,
                    park_time,
                },
            )
        })
    }

    fn spawn(mut self) {
        std::thread::spawn(move || {
            self.run();
        });
    }
}

#[inline]
fn packet_block(n_packets: usize) -> Box<[u8]> {
    let mut packets = vec![0u8; VOICE_PACKET_MAX * n_packets].into_boxed_slice();

    for packet in packets.chunks_exact_mut(VOICE_PACKET_MAX) {
        let mut rtp = MutableRtpPacket::new(packet).expect(
            "FATAL: Too few bytes in self.packet for RTP header.\
                (Blame: VOICE_PACKET_MAX?)",
        );
        rtp.set_version(RTP_VERSION);
        rtp.set_payload_type(RTP_PROFILE_TYPE);
    }

    packets
}

#[inline]
fn advance_rtp_counters(packet: &mut [u8]) {
    let mut rtp = MutableRtpPacket::new(packet).expect(
        "FATAL: Too few bytes in self.packet for RTP header.\
            (Blame: VOICE_PACKET_MAX?)",
    );
    rtp.set_sequence(rtp.get_sequence() + 1);
    rtp.set_timestamp(rtp.get_timestamp() + MONO_FRAME_SIZE as u32);
}

pub struct LiveMixers {
    stats: Arc<LiveStatBlock>,
    tx: Sender<(TaskId, ParkedMixer)>,
}

impl LiveMixers {
    pub(crate) fn new(
        mode: ScheduleMode,
        sched_tx: Sender<SchedulerMessage>,
        global_stats: Arc<StatBlock>,
    ) -> Self {
        let stats = Arc::new(LiveStatBlock::default());
        let (live_tx, live_rx) = flume::unbounded();

        let core = LiveMixersCore::new(mode, global_stats, stats.clone(), live_rx, sched_tx);
        core.spawn();

        Self { stats, tx: live_tx }
    }
}
