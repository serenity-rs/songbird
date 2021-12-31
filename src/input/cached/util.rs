use crate::{constants::*, input::Parsed};

use byteorder::{LittleEndian, WriteBytesExt};
use rubato::{FftFixedOut, Resampler};
use std::{
    io::{Read, Write},
    mem,
    ops::Range,
};
use symphonia_core::{
    audio::{AudioBuffer, AudioBufferRef, Signal},
    conv::IntoSample,
    sample::Sample,
};

const SAMPLE_LEN: usize = mem::size_of::<f32>();

/// Adapter for Symphonia sources into an interleaved f32 bytestream.
///
/// This will output `f32`s in LE byte order, matching the channel count
/// of the input.
pub struct ToAudioBytes {
    chan_count: usize,
    chan_limit: usize,
    parsed: Parsed,
    /// Position with parsed's last decoded frame.
    inner_pos: Range<usize>,
    resample: Option<ResampleState>,
    done: bool,

    interrupted_samples: Vec<f32>,
    interrupted_byte_pos: Range<usize>,
}

struct ResampleState {
    /// Used to hold outputs from resampling, *ready to be used*.
    resampled_data: Option<Vec<Vec<f32>>>,
    /// The actual resampler.
    resampler: FftFixedOut<f32>,
    /// Used to hold inputs to resampler across packet boundaries.
    scratch: AudioBuffer<f32>,
    /// The range of floats in `resampled_data` which have not yet
    /// been read.
    resample_pos: Range<usize>,
}

impl ToAudioBytes {
    pub fn new(parsed: Parsed, chan_limit: Option<usize>) -> Self {
        let track_info = parsed.decoder.codec_params();
        let sample_rate = track_info.sample_rate.unwrap_or(SAMPLE_RATE_RAW as u32);
        let layout = track_info
            .channel_layout
            .unwrap_or(symphonia_core::audio::Layout::Stereo);
        let chan_count = track_info.channels.map(|v| v.count()).unwrap_or(2);

        let chan_limit = chan_limit.unwrap_or(chan_count);

        let resample = if sample_rate != SAMPLE_RATE_RAW as u32 {
            let scratch = AudioBuffer::<f32>::new(
                MONO_FRAME_SIZE as u64,
                symphonia_core::audio::SignalSpec::new_with_layout(SAMPLE_RATE_RAW as u32, layout),
            );

            let resampler = FftFixedOut::new(
                sample_rate as usize,
                SAMPLE_RATE_RAW,
                RESAMPLE_OUTPUT_FRAME_SIZE,
                4,
                chan_count,
            );

            Some(ResampleState {
                resampled_data: None,
                resampler,
                scratch,
                resample_pos: 0..0,
            })
        } else {
            None
        };

        Self {
            chan_count,
            chan_limit,
            parsed,
            inner_pos: 0..0,
            resample,
            done: false,

            interrupted_samples: Vec::with_capacity(chan_count),
            interrupted_byte_pos: 0..0,
        }
    }

    pub fn num_channels(&self) -> usize {
        self.chan_count.min(self.chan_limit)
    }

    fn is_done(&self) -> bool {
        self.done
            && self.inner_pos.is_empty()
            && self
                .resample
                .as_ref()
                .map(|v| v.scratch.frames() == 0 && v.resample_pos.is_empty())
                .unwrap_or(true)
            && self.interrupted_byte_pos.is_empty()
    }
}

impl Read for ToAudioBytes {
    fn read(&mut self, mut buf: &mut [u8]) -> std::io::Result<usize> {
        // NOTE: this is disturbingly similar to the mixer code, but different enough that we can't
        // just reuse it freely.
        let orig_sz = buf.len();
        let num_chans = self.num_channels();

        while buf.len() != 0 && !self.is_done() {
            // Work to clear interrupted channel floats.
            while buf.len() != 0 && !self.interrupted_byte_pos.is_empty() {
                let index_of_first_f32 = self.interrupted_byte_pos.start / SAMPLE_LEN;
                let f32_inner_pos = self.interrupted_byte_pos.start % SAMPLE_LEN;
                let f32_bytes_remaining = SAMPLE_LEN - f32_inner_pos;
                let to_write = f32_bytes_remaining.min(buf.len());

                let bytes = self.interrupted_samples[index_of_first_f32].to_le_bytes();
                let written = buf.write(&bytes[f32_inner_pos..][..to_write])?;
                self.interrupted_byte_pos.start += written;
            }

            // Clear out already produced resampled floats.
            if let Some(resample) = self.resample.as_mut() {
                if let Some(data) = resample.resampled_data.as_mut() {
                    if buf.len() != 0 && !resample.resample_pos.is_empty() {
                        let bytes_advanced = write_resample_buffer(
                            &data,
                            buf,
                            &mut resample.resample_pos,
                            &mut self.interrupted_samples,
                            &mut self.interrupted_byte_pos,
                            num_chans,
                        );

                        buf = &mut buf[bytes_advanced..];
                    }
                }

                if resample.resample_pos.is_empty() {
                    resample.resampled_data = None;
                } else {
                    continue;
                }
            }

            // Now work with new packets.
            let source_packet = if !self.inner_pos.is_empty() {
                Some(self.parsed.decoder.last_decoded())
            } else if let Ok(pkt) = self.parsed.format.next_packet() {
                if pkt.track_id() != self.parsed.track_id {
                    continue;
                }

                self.parsed
                    .decoder
                    .decode(&pkt)
                    .map(|pkt| {
                        self.inner_pos = 0..pkt.frames();
                        pkt
                    })
                    .ok()
            } else {
                // EOF.
                None
            };

            if source_packet.is_none() {
                self.done = true;

                if let Some(resample) = self.resample.as_mut() {
                    if resample.scratch.frames() != 0 {
                        let resampler = &mut resample.resampler;
                        let in_len = resample.scratch.frames();
                        let to_render = resampler.nbr_frames_needed().saturating_sub(in_len);

                        if to_render != 0 {
                            resample.scratch.render_reserved(Some(to_render));
                            for plane in resample.scratch.planes_mut().planes() {
                                for val in &mut plane[in_len..] {
                                    *val = 0.0f32;
                                }
                            }
                        }

                        // Luckily, we make use of the WHOLE input buffer here.
                        let resampled = resampler
                            .process(resample.scratch.planes().planes())
                            .unwrap();

                        // Calculate true end position using sample rate math
                        let ratio =
                            (resampled[0].len() as f32) / (resample.scratch.frames() as f32);
                        let out_samples = (ratio * (in_len as f32)).round() as usize;

                        resample.resampled_data = Some(resampled);

                        resample.scratch.clear();
                        resample.resample_pos = 0..out_samples;
                    }
                }

                // Now go back and make use of the buffer.
                // We have to do this here because we can't make any guarantees about
                // the read site having enough space to hold all samples etc.
                continue;
            }

            let source_packet = source_packet.unwrap();

            if let Some(resample) = self.resample.as_mut() {
                // Do a resample using the newest packet.
                let pkt_frames = source_packet.frames();

                if pkt_frames == 0 {
                    continue;
                }

                let needed_in_frames = resample.resampler.nbr_frames_needed();
                let available_frames = self.inner_pos.len();

                let force_copy =
                    resample.scratch.frames() != 0 || needed_in_frames > available_frames;

                let resampled = if (!force_copy) && matches!(source_packet, AudioBufferRef::F32(_))
                {
                    // This is the only case where we can pull off a straight resample...
                    // I.e., skip scratch.

                    // NOTE: if let needed as if-let && {bool} is nightly only.
                    if let AudioBufferRef::F32(s_pkt) = source_packet {
                        let refs: Vec<&[f32]> = s_pkt
                            .planes()
                            .planes()
                            .iter()
                            .map(|s| &s[self.inner_pos.start..][..needed_in_frames])
                            .collect();

                        self.inner_pos.start += needed_in_frames;

                        resample.resampler.process(&*refs).unwrap()
                    } else {
                        unreachable!()
                    }
                } else {
                    // We either lack enough samples, or have the wrong data format, forcing
                    // a conversion/copy into scratch.

                    let old_scratch_len = resample.scratch.frames();
                    let missing_frames = needed_in_frames - old_scratch_len;
                    let frames_to_take = available_frames.min(missing_frames);

                    resample.scratch.render_reserved(Some(frames_to_take));
                    copy_into_resampler(
                        &source_packet,
                        &mut resample.scratch,
                        self.inner_pos.start,
                        old_scratch_len,
                        frames_to_take,
                    );

                    self.inner_pos.start += frames_to_take;

                    if resample.scratch.frames() != needed_in_frames {
                        continue;
                    } else {
                        let out = resample
                            .resampler
                            .process(resample.scratch.planes().planes())
                            .unwrap();
                        resample.scratch.clear();
                        out
                    }
                };

                resample.resample_pos = 0..resampled[0].len();
                resample.resampled_data = Some(resampled);
            } else {
                // Newest packet may be used straight away: just convert format
                // to ensure it's f32.
                let bytes_advanced = write_out(
                    &source_packet,
                    buf,
                    &mut self.inner_pos,
                    &mut self.interrupted_samples,
                    &mut self.interrupted_byte_pos,
                    num_chans,
                );

                buf = &mut buf[bytes_advanced * SAMPLE_LEN..];
            }
        }
        Ok(orig_sz - buf.len())
    }
}

#[inline]
fn write_out(
    source: &AudioBufferRef,
    target: &mut [u8],
    source_pos: &mut Range<usize>,
    spillover: &mut Vec<f32>,
    spill_range: &mut Range<usize>,
    num_chans: usize,
) -> usize {
    use AudioBufferRef::*;

    match source {
        U8(v) => write_symph_buffer(v, target, source_pos, spillover, spill_range, num_chans),
        U16(v) => write_symph_buffer(v, target, source_pos, spillover, spill_range, num_chans),
        U24(v) => write_symph_buffer(v, target, source_pos, spillover, spill_range, num_chans),
        U32(v) => write_symph_buffer(v, target, source_pos, spillover, spill_range, num_chans),
        S8(v) => write_symph_buffer(v, target, source_pos, spillover, spill_range, num_chans),
        S16(v) => write_symph_buffer(v, target, source_pos, spillover, spill_range, num_chans),
        S24(v) => write_symph_buffer(v, target, source_pos, spillover, spill_range, num_chans),
        S32(v) => write_symph_buffer(v, target, source_pos, spillover, spill_range, num_chans),
        F32(v) => write_symph_buffer(v, target, source_pos, spillover, spill_range, num_chans),
        F64(v) => write_symph_buffer(v, target, source_pos, spillover, spill_range, num_chans),
    }
}

#[inline]
fn write_symph_buffer<S>(
    source: &AudioBuffer<S>,
    buf: &mut [u8],
    source_pos: &mut Range<usize>,
    spillover: &mut Vec<f32>,
    spill_range: &mut Range<usize>,
    num_chans: usize,
) -> usize
where
    S: Sample + IntoSample<f32>,
{
    let float_space = buf.len() / SAMPLE_LEN;
    let interleaved_space = float_space / num_chans;
    let non_contiguous_end = (float_space % num_chans) != 0;

    let remaining = source_pos.len();
    let to_write = remaining.min(interleaved_space);
    let need_spill = non_contiguous_end && to_write < remaining;

    let samples_used = to_write + if need_spill { 1 } else { 0 };
    let last_sample = source_pos.start + to_write;

    if need_spill {
        spillover.clear();
        *spill_range = 0..num_chans * SAMPLE_LEN;
    }

    for (i, plane) in (&source.planes().planes()[..num_chans])
        .into_iter()
        .enumerate()
    {
        for (j, sample) in (&plane[source_pos.start..][..to_write])
            .into_iter()
            .enumerate()
        {
            // write this into the correct slot of buf.
            let addr = ((j * num_chans) + i) * SAMPLE_LEN;
            (&mut buf[addr..][..SAMPLE_LEN])
                .write_f32::<LittleEndian>((*sample).into_sample())
                .expect("Address known to exist by length checks.");
        }

        if need_spill {
            spillover.push(plane[last_sample].into_sample());
        }
    }

    source_pos.start += samples_used;

    to_write * num_chans * SAMPLE_LEN
}

#[inline]
fn write_resample_buffer(
    source: &Vec<Vec<f32>>,
    buf: &mut [u8],
    source_pos: &mut Range<usize>,
    spillover: &mut Vec<f32>,
    spill_range: &mut Range<usize>,
    num_chans: usize,
) -> usize {
    let float_space = buf.len() / SAMPLE_LEN;
    let interleaved_space = float_space / num_chans;
    let non_contiguous_end = (float_space % num_chans) != 0;

    let remaining = source_pos.len();
    let to_write = remaining.min(interleaved_space);
    let need_spill = non_contiguous_end && to_write < remaining;

    let samples_used = to_write + if need_spill { 1 } else { 0 };
    let last_sample = source_pos.start + to_write;

    if need_spill {
        spillover.clear();
        *spill_range = 0..num_chans * SAMPLE_LEN;
    }

    for (i, plane) in (&source[..num_chans]).into_iter().enumerate() {
        for (j, sample) in (&plane[source_pos.start..][..to_write])
            .into_iter()
            .enumerate()
        {
            // write this into the correct slot of buf.
            let addr = ((j * num_chans) + i) * SAMPLE_LEN;
            (&mut buf[addr..][..SAMPLE_LEN])
                .write_f32::<LittleEndian>(*sample)
                .expect("Address well-formed according to bounds checks.");
        }

        if need_spill {
            spillover.push(plane[last_sample]);
        }
    }

    source_pos.start += samples_used;

    let out = to_write * num_chans * SAMPLE_LEN;
    out
}

// these two are exact copies of the driver code...
#[inline]
fn copy_into_resampler(
    source: &AudioBufferRef,
    target: &mut AudioBuffer<f32>,
    source_pos: usize,
    dest_pos: usize,
    len: usize,
) -> usize {
    use AudioBufferRef::*;

    match source {
        U8(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        U16(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        U24(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        U32(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        S8(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        S16(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        S24(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        S32(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        F32(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        F64(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
    }
}

#[inline]
fn copy_symph_buffer<S>(
    source: &AudioBuffer<S>,
    target: &mut AudioBuffer<f32>,
    source_pos: usize,
    dest_pos: usize,
    len: usize,
) -> usize
where
    S: Sample + IntoSample<f32>,
{
    for (d_plane, s_plane) in (&mut target.planes_mut().planes()[..])
        .iter_mut()
        .zip(source.planes().planes()[..].iter())
    {
        for (d, s) in d_plane[dest_pos..dest_pos + len]
            .iter_mut()
            .zip(s_plane[source_pos..source_pos + len].iter())
        {
            *d = (*s).into_sample();
        }
    }

    len
}
