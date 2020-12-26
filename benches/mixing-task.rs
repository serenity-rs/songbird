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
    driver::bench_internals::{mixer::Mixer, task_message::*, CryptoState},
    input::{cached::Compressed, Input},
    tracks,
    Bitrate,
};
use tokio::runtime::{Handle, Runtime};
use xsalsa20poly1305::{aead::NewAead, XSalsa20Poly1305 as Cipher, KEY_SIZE};

// create a dummied task + interconnect.
// measure perf at varying numbers of sources (binary 1--64) without passthrough support.

fn dummied_mixer(
    handle: Handle,
) -> (
    Mixer,
    (
        Receiver<CoreMessage>,
        Receiver<EventMessage>,
        Receiver<UdpRxMessage>,
        Receiver<UdpTxMessage>,
    ),
) {
    let (mix_tx, mix_rx) = flume::unbounded();
    let (core_tx, core_rx) = flume::unbounded();
    let (event_tx, event_rx) = flume::unbounded();

    let (udp_sender_tx, udp_sender_rx) = flume::unbounded();
    let (udp_receiver_tx, udp_receiver_rx) = flume::unbounded();

    let ic = Interconnect {
        core: core_tx,
        events: event_tx,
        mixer: mix_tx,
    };

    let mut out = Mixer::new(mix_rx, handle, ic, Default::default());

    let fake_conn = MixerConnection {
        cipher: Cipher::new_varkey(&vec![0u8; KEY_SIZE]).unwrap(),
        crypto_state: CryptoState::Normal,
        udp_rx: udp_receiver_tx,
        udp_tx: udp_sender_tx,
    };

    out.conn_active = Some(fake_conn);

    out.skip_sleep = true;

    (out, (core_rx, event_rx, udp_receiver_rx, udp_sender_rx))
}

fn mixer_float(
    num_tracks: usize,
    handle: Handle,
) -> (
    Mixer,
    (
        Receiver<CoreMessage>,
        Receiver<EventMessage>,
        Receiver<UdpRxMessage>,
        Receiver<UdpTxMessage>,
    ),
) {
    let mut out = dummied_mixer(handle);

    let floats = utils::make_sine(10 * STEREO_FRAME_SIZE, true);

    let mut tracks = vec![];
    for i in 0..num_tracks {
        let input = Input::float_pcm(true, floats.clone().into());
        tracks.push(tracks::create_player(input).0.into());
    }

    out.0.tracks = tracks;

    out
}

fn mixer_float_drop(
    num_tracks: usize,
    handle: Handle,
) -> (
    Mixer,
    (
        Receiver<CoreMessage>,
        Receiver<EventMessage>,
        Receiver<UdpRxMessage>,
        Receiver<UdpTxMessage>,
    ),
) {
    let mut out = dummied_mixer(handle);

    let mut tracks = vec![];
    for i in 0..num_tracks {
        let floats = utils::make_sine((i / 5) * STEREO_FRAME_SIZE, true);
        let input = Input::float_pcm(true, floats.clone().into());
        tracks.push(tracks::create_player(input).0.into());
    }

    out.0.tracks = tracks;

    out
}

fn mixer_opus(
    handle: Handle,
) -> (
    Mixer,
    (
        Receiver<CoreMessage>,
        Receiver<EventMessage>,
        Receiver<UdpRxMessage>,
        Receiver<UdpTxMessage>,
    ),
) {
    // should add a single opus-based track.
    // make this fully loaded to prevent any perf cost there.
    let mut out = dummied_mixer(handle);

    let floats = utils::make_sine(6 * STEREO_FRAME_SIZE, true);

    let mut tracks = vec![];

    let mut src = Compressed::new(
        Input::float_pcm(true, floats.clone().into()),
        Bitrate::BitsPerSecond(128_000),
    )
    .expect("These parameters are well-defined.");
    src.raw.load_all();

    tracks.push(tracks::create_player(src.into()).0.into());

    out.0.tracks = tracks;

    out
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
                    || black_box(mixer_float(*i, rt.handle().clone())),
                    |input| {
                        black_box(input.0.cycle());
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
                    || black_box(mixer_float(*i, rt.handle().clone())),
                    |input| {
                        for i in 0..5 {
                            black_box(input.0.cycle());
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
            || black_box(mixer_opus(rt.handle().clone())),
            |input| {
                black_box(input.0.cycle());
            },
            BatchSize::SmallInput,
        )
    });
    group.bench_function("n=5 Packets", |b| {
        b.iter_batched_ref(
            || black_box(mixer_opus(rt.handle().clone())),
            |input| {
                for i in 0..5 {
                    black_box(input.0.cycle());
                }
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

fn culling(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    c.bench_function("Worst-case Track Culling (15 tracks, 5 pkts)", |b| {
        b.iter_batched_ref(
            || black_box(mixer_float_drop(15, rt.handle().clone())),
            |input| {
                for i in 0..5 {
                    black_box(input.0.cycle());
                }
            },
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(benches, no_passthrough, passthrough, culling);
criterion_main!(benches);
