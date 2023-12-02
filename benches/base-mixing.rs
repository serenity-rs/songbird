use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use songbird::{
    constants::*,
    driver::{
        bench_internals::mixer::{mix_logic, state::DecodeState},
        MixMode,
    },
    input::{codecs::*, Input, LiveInput, Parsed},
    test_utils as utils,
};
use std::io::Cursor;
use symphonia_core::audio::{AudioBuffer, Layout, SampleBuffer, Signal, SignalSpec};

pub fn mix_one_frame(c: &mut Criterion) {
    let floats = utils::make_sine(1 * STEREO_FRAME_SIZE, true);

    let symph_layout = MixMode::Stereo.into();

    let mut symph_mix = AudioBuffer::<f32>::new(
        MONO_FRAME_SIZE as u64,
        symphonia_core::audio::SignalSpec::new_with_layout(SAMPLE_RATE_RAW as u32, symph_layout),
    );
    let mut resample_scratch = AudioBuffer::<f32>::new(
        MONO_FRAME_SIZE as u64,
        SignalSpec::new_with_layout(SAMPLE_RATE_RAW as u32, Layout::Stereo),
    );

    let mut group = c.benchmark_group("Stereo Target");

    for (pres, hz) in [("", 48_000), (" (Resample)", 44_100)] {
        group.bench_with_input(
            BenchmarkId::new(format!("Stereo Source{}", pres), hz),
            &hz,
            |b, i| {
                b.iter_batched_ref(
                    || black_box(make_src(&floats, 2, *i)),
                    |(ref mut input, ref mut local_input)| {
                        symph_mix.clear();
                        symph_mix.render_reserved(Some(MONO_FRAME_SIZE));
                        resample_scratch.clear();

                        black_box(mix_logic::mix_symph_indiv(
                            &mut symph_mix,
                            &mut resample_scratch,
                            input,
                            local_input,
                            black_box(1.0),
                            None,
                        ));
                    },
                    BatchSize::SmallInput,
                )
            },
        );

        group.bench_with_input(
            BenchmarkId::new(format!("Mono Source{}", pres), hz),
            &hz,
            |b, i| {
                b.iter_batched_ref(
                    || black_box(make_src(&floats, 1, *i)),
                    |(ref mut input, ref mut local_input)| {
                        symph_mix.clear();
                        symph_mix.render_reserved(Some(MONO_FRAME_SIZE));
                        resample_scratch.clear();

                        black_box(mix_logic::mix_symph_indiv(
                            &mut symph_mix,
                            &mut resample_scratch,
                            input,
                            local_input,
                            black_box(1.0),
                            None,
                        ));
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

fn make_src(src: &Vec<u8>, chans: u32, hz: u32) -> (Parsed, DecodeState) {
    let local_input = Default::default();

    let adapted: Input =
        songbird::input::RawAdapter::new(Cursor::new(src.clone()), hz, chans).into();
    let promoted = match adapted {
        Input::Live(l, _) => l.promote(get_codec_registry(), get_probe()),
        _ => panic!("Failed to create a guaranteed source."),
    };
    let parsed = match promoted {
        Ok(LiveInput::Parsed(parsed)) => parsed,
        Err(e) => panic!("AR {:?}", e),
        _ => panic!("Failed to create a guaranteed source."),
    };

    (parsed, local_input)
}

criterion_group!(benches, mix_one_frame);
criterion_main!(benches);
