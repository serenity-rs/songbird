use crate::constants::*;
use audiopus::{
    coder::{Decoder as AudiopusDecoder, GenericCtl},
    Channels,
    Error as OpusError,
};
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

/// Test wrapper around libopus for Symphonia
pub struct OpusDecoder {
    inner: AudiopusDecoder,
    params: CodecParameters,
    buf: AudioBuffer<f32>,
    rawbuf: Vec<f32>,
}

impl OpusDecoder {
    fn decode_inner(&mut self, packet: &Packet) -> SymphResult<()> {
        let pkt = if packet.buf().is_empty() {
            None
        } else {
            Some(packet.buf())
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

impl Decoder for OpusDecoder {
    fn try_new(params: &CodecParameters, _options: &DecoderOptions) -> SymphResult<Self> {
        let inner = AudiopusDecoder::new(SAMPLE_RATE, Channels::Stereo).unwrap();

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
