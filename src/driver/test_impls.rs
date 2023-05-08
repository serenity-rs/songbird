#![allow(missing_docs)]

use crate::{
    constants::*,
    input::{
        cached::Compressed,
        codecs::{CODEC_REGISTRY, PROBE},
        RawAdapter,
    },
    test_utils,
};
use flume::{Receiver, Sender};
use std::{io::Cursor, net::UdpSocket, sync::Arc};
use tokio::runtime::Handle;
use xsalsa20poly1305::{KeyInit, XSalsa20Poly1305 as Cipher, KEY_SIZE};

use super::{
    scheduler::*,
    tasks::{message::*, mixer::Mixer},
    *,
};

// create a dummied task + interconnect.
// measure perf at varying numbers of sources (binary 1--64) without passthrough support.

#[cfg(feature = "receive")]
pub type Listeners = (
    Receiver<CoreMessage>,
    Receiver<EventMessage>,
    Receiver<UdpRxMessage>,
);

#[cfg(not(feature = "receive"))]
pub type Listeners = (Receiver<CoreMessage>, Receiver<EventMessage>);

pub type DummyMixer = (Mixer, Listeners);

impl Mixer {
    pub fn mock(handle: Handle, softclip: bool) -> DummyMixer {
        let (mix_tx, mix_rx) = flume::unbounded();
        let (core_tx, core_rx) = flume::unbounded();
        let (event_tx, event_rx) = flume::unbounded();

        #[cfg(feature = "receive")]
        let (udp_receiver_tx, udp_receiver_rx) = flume::unbounded();

        let ic = Interconnect {
            core: core_tx,
            events: event_tx,
            mixer: mix_tx,
        };

        // Scheduler must be created from a Tokio context...
        let (tx, rx) = flume::unbounded();
        handle.spawn_blocking(move || tx.send(Config::default().use_softclip(softclip)));
        let config = rx.recv().unwrap();

        let mut out = Mixer::new(mix_rx, handle, ic, config);

        let udp_tx = UdpSocket::bind("0.0.0.0:0").expect("Failed to create send port.");
        udp_tx
            .connect("127.0.0.1:5316")
            .expect("Failed to connect to local dest port.");

        #[cfg(feature = "receive")]
        let fake_conn = MixerConnection {
            cipher: Cipher::new_from_slice(&vec![0u8; KEY_SIZE]).unwrap(),
            crypto_state: CryptoState::Normal,
            udp_rx: udp_receiver_tx,
            udp_tx,
        };

        #[cfg(not(feature = "receive"))]
        let fake_conn = MixerConnection {
            cipher: Cipher::new_from_slice(&vec![0u8; KEY_SIZE]).unwrap(),
            crypto_state: CryptoState::Normal,
            udp_tx,
        };

        out.conn_active = Some(fake_conn);

        #[cfg(feature = "receive")]
        return (out, (core_rx, event_rx, udp_receiver_rx));

        #[cfg(not(feature = "receive"))]
        return (out, (core_rx, event_rx));
    }

    pub fn test_with_float(num_tracks: usize, handle: Handle, softclip: bool) -> DummyMixer {
        let mut out = Self::mock(handle, softclip);

        let floats = test_utils::make_sine(10 * STEREO_FRAME_SIZE, true);

        for _ in 0..num_tracks {
            let input: Input = RawAdapter::new(Cursor::new(floats.clone()), 48_000, 2).into();
            let promoted = match input {
                Input::Live(l, _) => l.promote(&CODEC_REGISTRY, &PROBE),
                _ => panic!("Failed to create a guaranteed source."),
            };
            let (_, ctx) = Track::from(Input::Live(promoted.unwrap(), None)).into_context();
            _ = out.0.add_track(ctx);
        }

        out
    }

    pub fn test_with_float_drop(num_tracks: usize, handle: Handle) -> DummyMixer {
        let mut out = Self::mock(handle, true);

        for i in 0..num_tracks {
            let floats = test_utils::make_sine((i / 5) * STEREO_FRAME_SIZE, true);
            let input: Input = RawAdapter::new(Cursor::new(floats.clone()), 48_000, 2).into();
            let promoted = match input {
                Input::Live(l, _) => l.promote(&CODEC_REGISTRY, &PROBE),
                _ => panic!("Failed to create a guaranteed source."),
            };
            let (_, ctx) = Track::from(Input::Live(promoted.unwrap(), None)).into_context();
            _ = out.0.add_track(ctx);
        }

        out
    }

    pub fn test_with_opus(handle: Handle) -> DummyMixer {
        // should add a single opus-based track.
        // make this fully loaded to prevent any perf cost there.
        let mut out = Self::mock(handle.clone(), false);

        let floats = test_utils::make_sine(6 * STEREO_FRAME_SIZE, true);

        let input: Input = RawAdapter::new(Cursor::new(floats), 48_000, 2).into();

        let mut src = handle.block_on(async move {
            Compressed::new(input, Bitrate::BitsPerSecond(128_000))
                .await
                .expect("These parameters are well-defined.")
        });

        src.raw.load_all();

        let promoted = match src.into() {
            Input::Live(l, _) => l.promote(&CODEC_REGISTRY, &PROBE),
            _ => panic!("Failed to create a guaranteed source."),
        };
        let (_, ctx) = Track::from(Input::Live(promoted.unwrap(), None)).into_context();

        _ = out.0.add_track(ctx);

        out
    }
}

pub struct MockScheduler {
    pub core: LiveMixersCore,
    pub stats: Arc<StatBlock>,
    pub local: Arc<LiveStatBlock>,
    pub rx: Receiver<SchedulerMessage>,
    pub tx: Sender<(TaskId, ParkedMixer)>,
    pub id: TaskId,
}

impl MockScheduler {
    pub fn new(mode: Option<ScheduleMode>) -> Self {
        let stats = Arc::new(StatBlock::default());
        let local = Arc::new(LiveStatBlock::default());

        let (task_tx, task_rx) = flume::unbounded();
        let (sched_tx, sched_rx) = flume::unbounded();

        let core = LiveMixersCore::new(
            mode.unwrap_or_default(),
            stats.clone(),
            local.clone(),
            task_rx,
            sched_tx,
        );
        Self {
            core,
            stats,
            local,
            rx: sched_rx,
            tx: task_tx,
            id: TaskId::new(),
        }
    }

    pub fn add_mixer_direct(&mut self, m: Mixer) {
        let id = self.id.incr();
        self.core.add_task_direct(m, id);
    }

    pub fn from_mixers(
        mode: Option<ScheduleMode>,
        mixers: Vec<DummyMixer>,
    ) -> (Self, Vec<Listeners>) {
        let mut out = Self::new(mode);
        let mut listeners = vec![];
        for (mixer, listener) in mixers {
            out.add_mixer_direct(mixer);
            listeners.push(listener);
        }
        (out, listeners)
    }
}
