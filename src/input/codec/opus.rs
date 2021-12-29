use crate::constants::*;
use audiopus::{
    coder::{Decoder as OpusDecoder, GenericCtl},
    Channels,
    Error as OpusError,
    SampleRate,
    TryFrom as AudTry,
};
use parking_lot::Mutex;
use std::{convert::TryFrom, sync::Arc};
use symphonia_core::{
    audio::{AsAudioBufferRef, AudioBuffer, AudioBufferRef, Layout, Signal, SignalSpec},
    codecs::{
        CodecDescriptor,
        CodecParameters,
        Decoder,
        DecoderOptions,
        FinalizeResult,
        CODEC_TYPE_OPUS,
    },
    errors::{decode_error, Result as SymphResult},
    formats::Packet,
};

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

impl SymphOpusDecoder {
    fn decode_inner(&mut self, packet: &Packet) -> SymphResult<()> {
        let pkt = if packet.buf().len() == 0 {
            None
        } else {
            Some(&packet.buf()[..])
        };

        let s_ct = loop {
            match self.inner.decode_float(pkt, &mut self.rawbuf[..], false) {
                Ok(v) => break v,
                Err(OpusError::Opus(audiopus::ErrorCode::BufferTooSmall)) => {
                    // double the buffer size
                    // correct behav would be to mirror the decoder logic in the udp_rx set.
                    self.rawbuf.resize(self.rawbuf.len() * 2, 0.0);
                    self.buf = AudioBuffer::new(
                        self.rawbuf.len() as u64 / 2,
                        SignalSpec::new_with_layout(SAMPLE_RATE_RAW as u32, Layout::Stereo),
                    );
                },
                Err(e) => {
                    tracing::error!("Opus decode error: {:?}", e);
                    return decode_error("desc");
                },
            }
        };

        self.buf.clear();
        self.buf.render_reserved(Some(s_ct));

        // Forcibly assuming stereo, for now.
        for ch in 0..2 {
            let iter = self.rawbuf.chunks_exact(2).map(|chunk| chunk[ch]);
            for (tgt, src) in self.buf.chan_mut(ch).iter_mut().zip(iter) {
                *tgt = src;
            }
        }

        Ok(())
    }
}

impl Decoder for SymphOpusDecoder {
    fn try_new(params: &CodecParameters, _options: &DecoderOptions) -> SymphResult<Self> {
        // TODO: investigate how Symphonia wants me to specify the output format?
        let inner = OpusDecoder::new(SAMPLE_RATE, Channels::Stereo).unwrap();

        Ok(Self {
            inner,
            params: params.clone(),
            buf: AudioBuffer::new(
                MONO_FRAME_SIZE as u64,
                SignalSpec::new_with_layout(SAMPLE_RATE_RAW as u32, Layout::Stereo),
            ),
            rawbuf: vec![0.0f32; STEREO_FRAME_SIZE],
        })
    }

    fn supported_codecs() -> &'static [symphonia::core::codecs::CodecDescriptor] {
        &[symphonia_core::support_codec!(
            CODEC_TYPE_OPUS,
            "opus",
            "libopus (1.3+, audiopus)"
        )]
    }

    fn codec_params(&self) -> &symphonia::core::codecs::CodecParameters {
        &self.params
    }

    fn decode(&mut self, packet: &Packet) -> SymphResult<AudioBufferRef<'_>> {
        if let Err(e) = self.decode_inner(packet) {
            self.buf.clear();
            Err(e)
        } else {
            Ok(self.buf.as_audio_buffer_ref())
        }
    }

    fn reset(&mut self) {
        let _ = self.inner.reset_state();
    }

    fn finalize(&mut self) -> FinalizeResult {
        Default::default()
    }

    fn last_decoded(&self) -> AudioBufferRef {
        self.buf.as_audio_buffer_ref()
    }
}
