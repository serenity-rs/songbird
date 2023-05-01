use std::error::Error;

use criterion::{
    black_box,
    criterion_group,
    criterion_main,
    BatchSize,
    Bencher,
    BenchmarkId,
    Criterion,
};
use flume::{Receiver, Sender, TryRecvError};
use songbird::{
    constants::*,
    driver::{
        bench_internals::{
            self,
            mixer::{state::InputState, Mixer},
            scheduler::*,
            task_message::*,
            CryptoState,
        },
        Bitrate,
    },
    input::{cached::Compressed, codecs::*, Input, RawAdapter},
    tracks,
    Config,
};
use std::{io::Cursor, net::UdpSocket, sync::Arc};
use tokio::runtime::{Handle, Runtime};
use xsalsa20poly1305::{KeyInit, XSalsa20Poly1305 as Cipher, KEY_SIZE};

// create a dummied task + interconnect.
// measure perf at varying numbers of sources (binary 1--64) without passthrough support.

type Listeners = (
    Receiver<CoreMessage>,
    Receiver<EventMessage>,
    Receiver<UdpRxMessage>,
);

type DummyMixer = (Mixer, Listeners);

fn dummied_mixer(handle: Handle, softclip: bool) -> DummyMixer {
    let (mix_tx, mix_rx) = flume::unbounded();
    let (core_tx, core_rx) = flume::unbounded();
    let (event_tx, event_rx) = flume::unbounded();

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

    let fake_conn = MixerConnection {
        cipher: Cipher::new_from_slice(&vec![0u8; KEY_SIZE]).unwrap(),
        crypto_state: CryptoState::Normal,
        udp_rx: udp_receiver_tx,
        udp_tx,
    };

    out.conn_active = Some(fake_conn);

    (out, (core_rx, event_rx, udp_receiver_rx))
}

fn mixer_float(num_tracks: usize, handle: Handle, softclip: bool) -> DummyMixer {
    let mut out = dummied_mixer(handle, softclip);

    let floats = utils::make_sine(10 * STEREO_FRAME_SIZE, true);

    for i in 0..num_tracks {
        let input: Input = RawAdapter::new(Cursor::new(floats.clone()), 48_000, 2).into();
        let promoted = match input {
            Input::Live(l, _) => l.promote(&CODEC_REGISTRY, &PROBE),
            _ => panic!("Failed to create a guaranteed source."),
        };
        let (handle, mut ctx) =
            bench_internals::track_context(Input::Live(promoted.unwrap(), None).into());
        out.0.add_track(ctx);
    }

    out
}

fn mixer_float_drop(num_tracks: usize, handle: Handle) -> DummyMixer {
    let mut out = dummied_mixer(handle, true);

    for i in 0..num_tracks {
        let floats = utils::make_sine((i / 5) * STEREO_FRAME_SIZE, true);
        let input: Input = RawAdapter::new(Cursor::new(floats.clone()), 48_000, 2).into();
        let promoted = match input {
            Input::Live(l, _) => l.promote(&CODEC_REGISTRY, &PROBE),
            _ => panic!("Failed to create a guaranteed source."),
        };
        let (handle, mut ctx) =
            bench_internals::track_context(Input::Live(promoted.unwrap(), None).into());
        out.0.add_track(ctx);
    }

    out
}

fn mixer_opus(handle: Handle) -> DummyMixer {
    // should add a single opus-based track.
    // make this fully loaded to prevent any perf cost there.
    let mut out = dummied_mixer(handle.clone(), false);

    let floats = utils::make_sine(6 * STEREO_FRAME_SIZE, true);

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
    let (handle, mut ctx) =
        bench_internals::track_context(Input::Live(promoted.unwrap(), None).into());

    out.0.add_track(ctx);

    out
}

struct MockScheduler {
    core: LiveMixersCore,
    stats: Arc<StatBlock>,
    local: Arc<LiveStatBlock>,
    rx: Receiver<SchedulerMessage>,
    tx: Sender<(TaskId, ParkedMixer)>,
    id: TaskId,
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

fn no_passthrough(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("Float Input (No Passthrough)");

    for shift in 0..=6 {
        let track_count = 1 << shift;

        group.bench_with_input(
            BenchmarkId::new("Single Packet", track_count),
            &track_count,
            |b, i| {
                b.iter_batched_ref(
                    || {
                        black_box(MockScheduler::from_mixers(
                            None,
                            vec![mixer_float(*i, rt.handle().clone(), true)],
                        ))
                    },
                    |input| {
                        black_box(input.0.core.run_once());
                    },
                    BatchSize::SmallInput,
                )
            },
        );
        group.bench_with_input(
            BenchmarkId::new("Single Packet (No Soft-Clip)", track_count),
            &track_count,
            |b, i| {
                b.iter_batched_ref(
                    || {
                        black_box(MockScheduler::from_mixers(
                            None,
                            vec![mixer_float(*i, rt.handle().clone(), false)],
                        ))
                    },
                    |input| {
                        black_box(input.0.core.run_once());
                    },
                    BatchSize::SmallInput,
                )
            },
        );
        group.bench_with_input(
            BenchmarkId::new("n=5 Packets", track_count),
            &track_count,
            |b, i| {
                b.iter_batched_ref(
                    || {
                        black_box(MockScheduler::from_mixers(
                            None,
                            vec![mixer_float(*i, rt.handle().clone(), true)],
                        ))
                    },
                    |input| {
                        for i in 0..5 {
                            black_box(input.0.core.run_once());
                        }
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

fn no_passthrough_multimix(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    const N_MIXERS: usize = 16;
    let mut group = c.benchmark_group(format!("Float Input (No Passthrough, {N_MIXERS} mixers)"));

    for shift in 0..=2 {
        let track_count = 1 << shift;

        group.bench_with_input(
            BenchmarkId::new("Single Packet (No Soft-Clip)", track_count),
            &track_count,
            |b, i| {
                b.iter_batched_ref(
                    || {
                        black_box(MockScheduler::from_mixers(
                            None,
                            (0..N_MIXERS)
                                .map(|_| mixer_float(*i, rt.handle().clone(), false))
                                .collect(),
                        ))
                    },
                    |input| {
                        black_box(input.0.core.run_once());
                    },
                    BatchSize::SmallInput,
                )
            },
        );
        group.bench_with_input(
            BenchmarkId::new("n=5 Packets", track_count),
            &track_count,
            |b, i| {
                b.iter_batched_ref(
                    || {
                        black_box(MockScheduler::from_mixers(
                            None,
                            (0..N_MIXERS)
                                .map(|_| mixer_float(*i, rt.handle().clone(), false))
                                .collect(),
                        ))
                    },
                    |input| {
                        for i in 0..5 {
                            black_box(input.0.core.run_once());
                        }
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

fn passthrough(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("Opus Input (Passthrough)");

    group.bench_function("Single Packet", |b| {
        b.iter_batched_ref(
            || {
                black_box(MockScheduler::from_mixers(
                    None,
                    vec![mixer_opus(rt.handle().clone())],
                ))
            },
            |input| {
                black_box(input.0.core.run_once());
            },
            BatchSize::SmallInput,
        )
    });
    group.bench_function("n=5 Packets", |b| {
        b.iter_batched_ref(
            || {
                black_box(MockScheduler::from_mixers(
                    None,
                    vec![mixer_opus(rt.handle().clone())],
                ))
            },
            |input| {
                for i in 0..5 {
                    black_box(input.0.core.run_once());
                }
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

fn passthrough_multimix(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    const N_MIXERS: usize = 16;
    let mut group = c.benchmark_group(format!("Opus Input (Passthrough, {N_MIXERS} mixers)"));

    for shift in 0..=2 {
        let track_count = 1 << shift;

        group.bench_with_input(
            BenchmarkId::new("Single Packet (No Soft-Clip)", track_count),
            &track_count,
            |b, i| {
                b.iter_batched_ref(
                    || {
                        black_box(MockScheduler::from_mixers(
                            None,
                            (0..N_MIXERS)
                                .map(|_| mixer_opus(rt.handle().clone()))
                                .collect(),
                        ))
                    },
                    |input| {
                        black_box(input.0.core.run_once());
                    },
                    BatchSize::SmallInput,
                )
            },
        );
        group.bench_with_input(
            BenchmarkId::new("n=5 Packets", track_count),
            &track_count,
            |b, i| {
                b.iter_batched_ref(
                    || {
                        black_box(MockScheduler::from_mixers(
                            None,
                            (0..N_MIXERS)
                                .map(|_| mixer_opus(rt.handle().clone()))
                                .collect(),
                        ))
                    },
                    |input| {
                        for i in 0..5 {
                            black_box(input.0.core.run_once());
                        }
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

fn culling(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    c.bench_function("Worst-case Track Culling (15 tracks, 5 pkts)", |b| {
        b.iter_batched_ref(
            || {
                black_box(MockScheduler::from_mixers(
                    None,
                    vec![mixer_float_drop(15, rt.handle().clone())],
                ))
            },
            |input| {
                for i in 0..5 {
                    black_box(input.0.core.run_once());
                }
            },
            BatchSize::SmallInput,
        )
    });
}

fn task_culling(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    const N_MIXERS: usize = 8;

    c.bench_function("Live Mixer Thread Culling", |b| {
        b.iter_batched_ref(
            || {
                black_box(MockScheduler::from_mixers(
                    None,
                    (0..N_MIXERS)
                        .map(|_| mixer_opus(rt.handle().clone()))
                        .collect(),
                ))
            },
            |input| {
                black_box(input.0.core.remove_task(0));
            },
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(individual, no_passthrough, passthrough);
criterion_group!(multimix, no_passthrough_multimix, passthrough_multimix);
criterion_group!(deletions, culling, task_culling);
criterion_main!(individual, multimix, deletions);
