mod metadata;
pub use self::metadata::*;

use std::sync::Arc;

use crate::constants::{SAMPLE_RATE, SAMPLE_RATE_RAW};

use std::{
    io::{Seek, SeekFrom},
    num::NonZero,
};

use symphonia::core::{
    self as symphonia_core,
    audio::sample::SampleFormat,
    codecs::{
        audio::{well_known::CODEC_ID_OPUS, AudioCodecParameters},
        CodecParameters,
    },
    common::FourCc,
    errors::{decode_error, seek_error, unsupported_error, Error, Result, SeekErrorKind},
    formats::{
        prelude::*,
        probe::{ProbeFormatData, ProbeableFormat, Score, Scoreable},
        TrackType,
    },
    io::*,
    meta::{
        well_known::METADATA_ID_VORBIS_COMMENT,
        Metadata,
        MetadataBuilder,
        MetadataInfo,
        MetadataLog,
        StandardTag,
        Tag,
    },
};

const DCA1_FORMAT_INFO: FormatInfo = FormatInfo {
    format: FormatId::new(FourCc::new(*b"DCA1")),
    short_name: "dca",
    long_name: "DCA[0/1] Opus Wrapper",
};

const DCA1_METADATA_INFO: MetadataInfo = MetadataInfo {
    metadata: METADATA_ID_VORBIS_COMMENT,
    short_name: "dca",
    long_name: "DCA[0/1] Opus Wrapper",
};

const DCA1_PROBE_INFO: &[ProbeFormatData] = &[symphonia::core::support_format!(
    DCA1_FORMAT_INFO,
    &["dca"],
    &[],
    &[b"DCA1"]
)];

impl ProbeableFormat<'_> for DcaReader<'_> {
    fn try_probe_new(
        mss: MediaSourceStream<'_>,
        opts: FormatOptions,
    ) -> Result<Box<dyn FormatReader + '_>> {
        Ok(Box::new(DcaReader::try_new(mss, &opts)?))
    }

    fn probe_data() -> &'static [ProbeFormatData] {
        DCA1_PROBE_INFO
    }
}

struct SeekAccel {
    frame_offsets: Vec<(Timestamp, u64)>,
    seek_index_fill_period_ms: u16,
    next_ts: Timestamp,
}

impl SeekAccel {
    fn new(options: &FormatOptions, first_frame_byte_pos: u64) -> Self {
        let per_ms = options.seek_index_fill_period_ms;
        let next_ts = Timestamp::new(((per_ms as i64) * (SAMPLE_RATE_RAW as i64)) / 1000);

        Self {
            frame_offsets: vec![(Timestamp::ZERO, first_frame_byte_pos)],
            seek_index_fill_period_ms: per_ms,
            next_ts,
        }
    }

    fn update(&mut self, ts: Timestamp, pos: u64) {
        if ts >= self.next_ts {
            let duration =
                ((self.seek_index_fill_period_ms as u64) * (SAMPLE_RATE_RAW as u64)) / 1000;

            self.next_ts = self.next_ts.saturating_add(duration.into());
            self.frame_offsets.push((ts, pos));
        }
    }

    fn get_seek_pos(&self, ts: Timestamp) -> (Timestamp, u64) {
        let index = self.frame_offsets.partition_point(|&(o_ts, _)| o_ts <= ts) - 1;
        self.frame_offsets[index]
    }
}

/// [DCA\[0/1\]](https://github.com/bwmarrin/dca) Format reader for Symphonia.
pub struct DcaReader<'s> {
    source: MediaSourceStream<'s>,
    media_info: MediaInfo,
    track: Option<Track>,
    metas: MetadataLog,
    seek_accel: SeekAccel,
    curr_ts: Timestamp,
    max_ts: Option<Timestamp>,
    held_packet: Option<Packet>,
}

impl<'s> DcaReader<'s> {
    fn try_new(mut source: MediaSourceStream<'s>, options: &FormatOptions) -> Result<Self> {
        // Read in the magic number to verify it's a DCA file.
        let magic = source.read_quad_bytes()?;

        let read_meta = match &magic {
            b"DCA1" => true,
            _ if &magic[..3] == b"DCA" => {
                return unsupported_error("unsupported DCA version");
            },
            _ => {
                source.seek_buffered_rel(-4);
                false
            },
        };

        let mut codec_params = AudioCodecParameters::new();

        codec_params
            .for_codec(CODEC_ID_OPUS)
            .with_max_frames_per_packet(1)
            .with_sample_rate(SAMPLE_RATE_RAW as u32)
            .with_sample_format(SampleFormat::F32);

        let mut metas = MetadataLog::default();

        if read_meta {
            let size = source.read_u32()?;

            // Sanity check
            if (size as i32) < 2 {
                return decode_error("missing DCA1 metadata block");
            }

            let raw_json = source.read_boxed_slice_exact(size as usize)?;

            let metadata: DcaMetadata = serde_json::from_slice::<DcaMetadata>(&raw_json)
                .map_err(|_| Error::DecodeError("malformed DCA1 metadata block"))?;

            let mut revision = MetadataBuilder::new(DCA1_METADATA_INFO);

            if let Some(info) = metadata.info {
                if let Some(t) = info.title {
                    let t = Arc::new(t);
                    revision.add_tag(Tag::new_from_parts(
                        "title",
                        t.clone(),
                        Some(StandardTag::TrackTitle(t)),
                    ));
                }
                if let Some(t) = info.album {
                    let t = Arc::new(t);
                    revision.add_tag(Tag::new_from_parts(
                        "album",
                        t.clone(),
                        Some(StandardTag::Album(t)),
                    ));
                }
                if let Some(t) = info.artist {
                    let t = Arc::new(t);
                    revision.add_tag(Tag::new_from_parts(
                        "artist",
                        t.clone(),
                        Some(StandardTag::Artist(t)),
                    ));
                }
                if let Some(t) = info.genre {
                    let t = Arc::new(t);
                    revision.add_tag(Tag::new_from_parts(
                        "genre",
                        t.clone(),
                        Some(StandardTag::Genre(t)),
                    ));
                }
                if let Some(t) = info.comments {
                    let t = Arc::new(t);
                    revision.add_tag(Tag::new_from_parts(
                        "comments",
                        t.clone(),
                        Some(StandardTag::Comment(t)),
                    ));
                }
                if let Some(_t) = info.cover {
                    // TODO: Add visual, figure out MIME types.
                }
            }

            if let Some(origin) = metadata.origin {
                if let Some(t) = origin.url {
                    let t = Arc::new(t);
                    revision.add_tag(Tag::new_from_parts(
                        "url",
                        t.clone(),
                        Some(StandardTag::Url(t)),
                    ));
                }
            }

            metas.push(revision.build());
        }

        let bytes_read = source.pos();

        let mut track = Track::new(0);
        track
            .with_codec_params(CodecParameters::Audio(codec_params))
            .with_time_base(TimeBase::new(
                NonZero::new(1).unwrap(),
                NonZero::new(SAMPLE_RATE_RAW as u32).unwrap(),
            ));

        Ok(Self {
            source,
            media_info: MediaInfo::from_track(&track),
            track: Some(track),
            metas,
            seek_accel: SeekAccel::new(options, bytes_read),
            curr_ts: Timestamp::ZERO,
            max_ts: None,
            held_packet: None,
        })
    }
}

impl Scoreable for DcaReader<'_> {
    fn score(mut src: ScopedStream<&mut MediaSourceStream<'_>>) -> Result<Score> {
        let magic = src.read_quad_bytes()?;

        match &magic {
            b"DCA1" => Ok(Score::Supported(255)),
            _ => Ok(Score::Unsupported),
        }
    }
}

impl FormatReader for DcaReader<'_> {
    fn format_info(&self) -> &FormatInfo {
        &DCA1_FORMAT_INFO
    }

    fn media_info(&self) -> &MediaInfo {
        &self.media_info
    }

    fn metadata(&mut self) -> Metadata<'_> {
        self.metas.metadata()
    }

    fn seek(&mut self, _mode: SeekMode, to: SeekTo) -> Result<SeekedTo> {
        let can_backseek = self.source.is_seekable();

        let Some(track) = &self.track else {
            return seek_error(SeekErrorKind::Unseekable);
        };

        let Some(CodecParameters::Audio(codec_params)) = &track.codec_params else {
            return seek_error(SeekErrorKind::Unseekable);
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
                        return seek_error(SeekErrorKind::Unseekable);
                    }
                } else {
                    return seek_error(SeekErrorKind::Unseekable);
                },
            SeekTo::Timestamp { ts, .. } => ts,
        };

        if let Some(max_ts) = self.max_ts {
            if ts > max_ts {
                return seek_error(SeekErrorKind::OutOfRange);
            }
        }

        let backseek_needed = self.curr_ts > ts;

        if backseek_needed && !can_backseek {
            return seek_error(SeekErrorKind::ForwardOnly);
        }

        let (accel_seek_ts, accel_seek_pos) = self.seek_accel.get_seek_pos(ts);

        if backseek_needed || accel_seek_pos > self.source.pos() {
            self.source.seek(SeekFrom::Start(accel_seek_pos))?;
            self.curr_ts = accel_seek_ts;
        }

        while let Ok(Some(pkt)) = self.next_packet() {
            let pts = pkt.pts;
            let dur = pkt.dur;
            let Some(end) = pts.checked_add(dur) else {
                return seek_error(SeekErrorKind::OutOfRange);
            };
            let track_id = pkt.track_id;

            if (pts..end).contains(&ts) {
                self.held_packet = Some(pkt);
                return Ok(SeekedTo {
                    track_id,
                    required_ts: ts,
                    actual_ts: pts,
                });
            }
        }

        seek_error(SeekErrorKind::OutOfRange)
    }

    fn tracks(&self) -> &[Track] {
        // DCA tracks can hold only one track by design.
        // Of course, a zero-length file is technically allowed,
        // in which case no track.
        self.track.as_slice()
    }

    fn default_track(&self, track_type: TrackType) -> Option<&Track> {
        match track_type {
            TrackType::Audio => self.track.as_ref(),
            _ => None,
        }
    }

    fn next_packet(&mut self) -> Result<Option<Packet>> {
        if let Some(pkt) = self.held_packet.take() {
            return Ok(Some(pkt));
        }

        let frame_pos = self.source.pos();

        let p_len = match self.source.read_u16() {
            Ok(len) => len as i16,
            Err(eof) => {
                self.max_ts = Some(self.curr_ts);
                return Err(eof.into());
            },
        };

        if p_len < 0 {
            return decode_error("DCA frame header had a negative length.");
        }

        let buf = self.source.read_boxed_slice_exact(p_len as usize)?;

        if buf.is_empty() || buf.len() > i32::MAX as usize {
            return decode_error("Packet was not a valid Opus packet: too large for opus2.");
        }

        let sample_ct = opus2::packet::get_nb_samples(&buf, SAMPLE_RATE).or_else(|_| {
            decode_error("Packet was not a valid Opus packet: couldn't read sample count.")
        })? as u64;

        let duration = sample_ct.into();

        let out = Packet::new(0, self.curr_ts, duration, buf);

        self.seek_accel.update(self.curr_ts, frame_pos);

        let Some(new_ts) = self.curr_ts.checked_add(duration) else {
            return decode_error("Packet was not a valid Opus packet: timestamp overflowed.");
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

#[cfg(test)]
mod tests {
    use crate::{
        constants::test_data::FILE_DCA_TARGET,
        input::{input_tests::*, File},
    };

    // NOTE: this covers youtube audio in a non-copyright-violating way, since
    // those depend on an HttpRequest internally anyhow.
    #[tokio::test]
    #[ntest::timeout(10_000)]
    async fn dca_track_plays() {
        track_plays_passthrough(|| File::new(FILE_DCA_TARGET)).await;
    }

    #[tokio::test]
    #[ntest::timeout(10_000)]
    async fn dca_forward_seek_correct() {
        forward_seek_correct(|| File::new(FILE_DCA_TARGET)).await;
    }

    #[tokio::test]
    #[ntest::timeout(10_000)]
    async fn dca_backward_seek_correct() {
        backward_seek_correct(|| File::new(FILE_DCA_TARGET)).await;
    }

    #[tokio::test]
    #[ntest::timeout(10_000)]
    async fn opus_passthrough_when_other_tracks_paused() {
        track_plays_passthrough_when_is_only_active(|| File::new(FILE_DCA_TARGET)).await;
    }
}
