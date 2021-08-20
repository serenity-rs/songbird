use crate::constants::*;
use audiopus::{Channels, Error as OpusError, SampleRate, coder::Decoder as OpusDecoder, TryFrom as AudTry};
use parking_lot::Mutex;
use std::{convert::TryFrom, sync::Arc};

use symphonia_core::{audio::{AsAudioBufferRef, AudioBuffer, AudioBufferRef, Layout, Signal, SignalSpec}, codecs::{CODEC_TYPE_OPUS, CodecParameters, CodecDescriptor, Decoder, DecoderOptions, FinalizeResult}, errors::{Result as SymphResult, decode_error}, formats::Packet};

#[derive(Clone, Debug)]
/// Inner state used to decode Opus input sources.
pub struct OpusDecoderState {
    /// Inner decoder used to convert opus frames into a stream of samples.
    pub decoder: Arc<Mutex<OpusDecoder>>,
    /// Controls whether this source allows direct Opus frame passthrough.
    /// Defaults to `true`.
    ///
    /// Enabling this flag is a promise from the programmer to the audio core
    /// that the source has been encoded at 48kHz, using 20ms long frames.
    /// If you cannot guarantee this, disable this flag (or else risk nasal demons)
    /// and bizarre audio behaviour.
    pub allow_passthrough: bool,
    pub(crate) current_frame: Vec<f32>,
    pub(crate) frame_pos: usize,
    pub(crate) should_reset: bool,
}

impl OpusDecoderState {
    /// Creates a new decoder, having stereo output at 48kHz.
    pub fn new() -> Result<Self, OpusError> {
        Ok(Self::from_decoder(OpusDecoder::new(
            SAMPLE_RATE,
            Channels::Stereo,
        )?))
    }

    /// Creates a new decoder pre-configured by the user.
    pub fn from_decoder(decoder: OpusDecoder) -> Self {
        Self {
            decoder: Arc::new(Mutex::new(decoder)),
            allow_passthrough: true,
            current_frame: Vec::with_capacity(STEREO_FRAME_SIZE),
            frame_pos: 0,
            should_reset: false,
        }
    }
}

/// Test wrapper around libopus for Symphonia
pub struct SymphOpusDecoder {
    inner: OpusDecoder,
    params: CodecParameters,
    buf: AudioBuffer<f32>,
    rawbuf: Vec<f32>,
}

impl Decoder for SymphOpusDecoder {
    fn try_new(
        params: &CodecParameters,
        _options: &DecoderOptions,
    ) -> SymphResult<Self> {
        // TODO: investigate how Symphonia wants me to specify the output format?
        let inner = OpusDecoder::new(SAMPLE_RATE, Channels::Stereo).unwrap();

        Ok(Self {
            inner,
            params: params.clone(),
            buf: AudioBuffer::new(
                MONO_FRAME_SIZE as u64,
                SignalSpec::new_with_layout(SAMPLE_RATE_RAW as u32, Layout::Stereo)
            ),
            rawbuf: vec![0.0f32; STEREO_FRAME_SIZE * 5],
        })
    }

    fn supported_codecs() -> &'static [symphonia::core::codecs::CodecDescriptor] {
        &[symphonia_core::support_codec!(CODEC_TYPE_OPUS, "opus", "libopus (1.3+, audiopus)")]
    }

    fn codec_params(&self) -> &symphonia::core::codecs::CodecParameters {
        &self.params
    }

    fn decode(
        &mut self,
        packet: &Packet,
    ) -> SymphResult<AudioBufferRef> {
        // println!("OPUS: {}", packet.buf().len());
        let s_ct = self.inner.decode_float(
            Some(&packet.buf()[..]),
            &mut self.rawbuf[..],
            false,
        ).unwrap();

        self.buf.clear();
        // self.buf.render_reserved(Some(self.rawbuf.len() / 2));
        self.buf.render_reserved(Some(s_ct));

        // Forcibly assuming stereo, for now.
        for ch in 0..2 {
            let iter = self.rawbuf.chunks_exact(2).map(|chunk| chunk[ch]);
            for (tgt, src) in self.buf.chan_mut(ch).iter_mut().zip(iter) {
                *tgt = src;
            }
        }

        Ok(self.buf.as_audio_buffer_ref())
    }

    fn reset(&mut self) {
        todo!()
    }

    fn finalize(&mut self) -> FinalizeResult {
        unimplemented!()
    }
}
