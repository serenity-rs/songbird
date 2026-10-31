use std::{
    io::{Seek, SeekFrom},
    num::NonZero,
};
use symphonia::core::{
    self as symphonia_core,
    audio::{
        layouts::{CHANNEL_LAYOUT_MONO, CHANNEL_LAYOUT_STEREO},
        sample::SampleFormat,
    },
    codecs::{
        audio::{well_known::CODEC_ID_PCM_F32LE, AudioCodecParameters},
        CodecParameters,
    },
    common::FourCc,
    errors::{self as symph_err, Result as SymphResult, SeekErrorKind},
    formats::{
        prelude::*,
        probe::{ProbeFormatData, ProbeableFormat, Score, Scoreable},
        TrackType,
    },
    io::{MediaSource, MediaSourceStream, ReadBytes, SeekBuffered, *},
    meta::{Metadata as SymphMetadata, MetadataLog},
};

const RAW_FORMAT_INFO: FormatInfo = FormatInfo {
    format: FormatId::new(FourCc::new(*b"SBRA")),
    short_name: "raw",
    long_name: "Raw arbitrary-length f32 audio container.",
};

const RAW_PROBE_INFO: &[ProbeFormatData] = &[symphonia::core::support_format!(
    RAW_FORMAT_INFO,
    &["rawf32"],
    &[],
    &[b"SbirdRaw"]
)];

impl ProbeableFormat<'_> for RawReader<'_> {
    fn try_probe_new(
        mss: MediaSourceStream<'_>,
        opts: FormatOptions,
    ) -> SymphResult<Box<dyn FormatReader + '_>> {
        Ok(Box::new(RawReader::try_new(mss, opts)?))
    }

    fn probe_data() -> &'static [ProbeFormatData] {
        RAW_PROBE_INFO
    }
}

/// Symphonia support for a simple container for raw f32-PCM data of unknown duration.
///
/// Contained files have a simple header:
/// * the 8-byte signature `b"SbirdRaw"`,
/// * the sample rate, as a little-endian `u32`,
/// * the channel count, as a little-endian `u32`.
///
/// The remainder of the file is interleaved little-endian `f32` samples.
pub struct RawReader<'s> {
    source: MediaSourceStream<'s>,
    media_info: MediaInfo,
    track: Track,
    meta: MetadataLog,
    curr_ts: Timestamp,
    max_ts: Option<Timestamp>,
}

impl<'s> RawReader<'s> {
    fn try_new(mut source: MediaSourceStream<'s>, _options: FormatOptions) -> SymphResult<Self> {
        let mut magic = [0u8; 8];
        ReadBytes::read_buf_exact(&mut source, &mut magic[..])?;

        if &magic != b"SbirdRaw" {
            source.seek_buffered_rel(-(magic.len() as isize));
            return symph_err::decode_error("rawf32: illegal magic byte sequence.");
        }

        let sample_rate = source.read_u32()?;
        if sample_rate == 0 {
            return symph_err::decode_error("rawf32: sample rate is zero.");
        }

        let n_chans = source.read_u32()?;

        let chans = match n_chans {
            1 => CHANNEL_LAYOUT_MONO,
            2 => CHANNEL_LAYOUT_STEREO,
            _ =>
                return symph_err::decode_error(
                    "rawf32: channel layout is not stereo or mono for fmt_pcm",
                ),
        };

        let mut codec_params = AudioCodecParameters::new();

        codec_params
            .for_codec(CODEC_ID_PCM_F32LE)
            .with_bits_per_coded_sample((std::mem::size_of::<f32>() as u32) * 8)
            .with_bits_per_sample((std::mem::size_of::<f32>() as u32) * 8)
            .with_sample_rate(sample_rate)
            .with_sample_format(SampleFormat::F32)
            .with_max_frames_per_packet(sample_rate as u64 / 50)
            .with_channels(chans);

        let mut track = Track::new(0);
        track
            .with_codec_params(CodecParameters::Audio(codec_params))
            .with_time_base(TimeBase::new(
                NonZero::new(1).unwrap(),
                NonZero::new(sample_rate).unwrap(),
            ));

        Ok(Self {
            source,
            media_info: MediaInfo::from_track(&track),
            track,
            meta: MetadataLog::default(),
            curr_ts: Timestamp::ZERO,
            max_ts: None,
        })
    }
}

impl Scoreable for RawReader<'_> {
    fn score(mut src: ScopedStream<&mut MediaSourceStream<'_>>) -> SymphResult<Score> {
        let mut magic = [0u8; 8];
        ReadBytes::read_buf_exact(&mut src, &mut magic[..])?;

        match &magic {
            b"SbirdRaw" => Ok(Score::Supported(255)),
            _ => Ok(Score::Unsupported),
        }
    }
}

impl FormatReader for RawReader<'_> {
    fn format_info(&self) -> &FormatInfo {
        &RAW_FORMAT_INFO
    }

    fn media_info(&self) -> &MediaInfo {
        &self.media_info
    }

    fn metadata(&mut self) -> SymphMetadata<'_> {
        self.meta.metadata()
    }

    fn seek(&mut self, _mode: SeekMode, to: SeekTo) -> SymphResult<SeekedTo> {
        let can_backseek = self.source.is_seekable();

        let track = &self.track;

        let Some(CodecParameters::Audio(codec_params)) = &track.codec_params else {
            return symph_err::seek_error(SeekErrorKind::Unseekable);
        };

        let rate = codec_params.sample_rate;

        let ts = match to {
            SeekTo::Time { time, .. } =>
                if let Some(rate) = rate.and_then(NonZero::new) {
                    if let Some(ts) =
                        TimeBase::new(NonZero::new(1).unwrap(), rate).calc_timestamp(time)
                    {
                        ts
                    } else {
                        return symph_err::seek_error(SeekErrorKind::Unseekable);
                    }
                } else {
                    return symph_err::seek_error(SeekErrorKind::Unseekable);
                },
            SeekTo::Timestamp { ts, .. } => ts,
        };

        if ts.is_negative() {
            // TODO: Is this the correct error type?
            return symph_err::seek_error(SeekErrorKind::OutOfRange);
        }

        if let Some(max_ts) = self.max_ts {
            if ts > max_ts {
                return symph_err::seek_error(SeekErrorKind::OutOfRange);
            }
        }

        let backseek_needed = self.curr_ts > ts;

        if backseek_needed && !can_backseek {
            return symph_err::seek_error(SeekErrorKind::ForwardOnly);
        }

        let chan_count = codec_params
            .channels
            .as_ref()
            .expect("Channel count is built into format.")
            .count() as u64;

        let seek_pos = 16 + (std::mem::size_of::<f32>() as u64) * (ts.get() as u64 * chan_count);

        self.source.seek(SeekFrom::Start(seek_pos))?;
        self.curr_ts = ts;

        Ok(SeekedTo {
            track_id: track.id,
            required_ts: ts,
            actual_ts: ts,
        })
    }

    fn tracks(&self) -> &[Track] {
        std::slice::from_ref(&self.track)
    }

    fn default_track(&self, track_type: TrackType) -> Option<&Track> {
        match track_type {
            TrackType::Audio => Some(&self.track),
            _ => None,
        }
    }

    fn next_packet(&mut self) -> SymphResult<Option<Packet>> {
        let track = &self.track;

        let Some(CodecParameters::Audio(codec_params)) = &track.codec_params else {
            // This should be unreachable
            return symph_err::decode_error("not and audio coded");
        };

        let rate = codec_params
            .sample_rate
            .expect("Sample rate is built into format.") as usize;

        let chan_count = codec_params
            .channels
            .as_ref()
            .expect("Channel count is built into format.")
            .count();

        let sample_unit = std::mem::size_of::<f32>() * chan_count;

        // Aim for 20ms (50Hz).
        let buf = self.source.read_boxed_slice((rate / 50) * sample_unit)?;

        let sample_ct = (buf.len() / sample_unit) as u64;
        let duration = sample_ct.into();

        let out = Packet::new(0, self.curr_ts, duration, buf);

        let Some(new_ts) = self.curr_ts.checked_add(duration) else {
            return symph_err::decode_error(
                "Packet was not a valid Opus packet: timestamp overflowed.",
            );
        };
        self.curr_ts = new_ts;

        Ok(Some(out))
    }

    fn into_inner<'s>(self: Box<Self>) -> MediaSourceStream<'s>
    where
        Self: 's,
    {
        self.source
    }
}
