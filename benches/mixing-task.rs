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
        DummyMixer,
        Listeners,
        MockScheduler,
    },
    input::{cached::Compressed, codecs::*, Input, RawAdapter},
    tracks,
    Config,
};
use std::{io::Cursor, net::UdpSocket, sync::Arc};
use tokio::runtime::{Handle, Runtime};
use xsalsa20poly1305::{KeyInit, XSalsa20Poly1305 as Cipher, KEY_SIZE};

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
                            vec![Mixer::test_with_float(*i, rt.handle().clone(), true)],
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
                            vec![Mixer::test_with_float(*i, rt.handle().clone(), false)],
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
                            vec![Mixer::test_with_float(*i, rt.handle().clone(), true)],
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
                                .map(|_| Mixer::test_with_float(*i, rt.handle().clone(), false))
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
                                .map(|_| Mixer::test_with_float(*i, rt.handle().clone(), false))
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
                    vec![Mixer::test_with_opus(rt.handle().clone())],
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
                    vec![Mixer::test_with_opus(rt.handle().clone())],
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
                                .map(|_| Mixer::test_with_opus(rt.handle().clone()))
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
                                .map(|_| Mixer::test_with_opus(rt.handle().clone()))
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
                    vec![Mixer::test_with_float_drop(15, rt.handle().clone())],
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
                        .map(|_| Mixer::test_with_opus(rt.handle().clone()))
                        .collect(),
                ))
            },
            |input| {
                black_box(input.0.core.remove_task(0));
            },
            BatchSize::SmallInput,
        )
    });

    c.bench_function("Live Mixer Thread Culling (Practical)", |b| {
        b.iter_batched_ref(
            || {
                black_box(MockScheduler::from_mixers(
                    None,
                    (0..N_MIXERS)
                        .map(|_| Mixer::test_with_opus(rt.handle().clone()))
                        .collect(),
                ))
            },
            |input| {
                black_box({
                    input.0.core.mark_for_cull(0);
                    input.0.core.mark_for_cull(1);
                    input.0.core.mark_for_cull(4);
                    input.0.core.demote_and_remove_mixers();
                });
            },
            BatchSize::SmallInput,
        )
    });

    c.bench_function("Live Mixer Thread Culling (Practical, NoDel)", |b| {
        b.iter_batched_ref(
            || {
                black_box(MockScheduler::from_mixers(
                    None,
                    (0..N_MIXERS)
                        .map(|_| Mixer::test_with_opus(rt.handle().clone()))
                        .collect(),
                ))
            },
            |input| {
                black_box(input.0.core.demote_and_remove_mixers());
            },
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(individual, no_passthrough, passthrough);
criterion_group!(multimix, no_passthrough_multimix, passthrough_multimix);
criterion_group!(deletions, culling, task_culling);
criterion_main!(individual, multimix, deletions);
