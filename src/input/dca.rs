use crate::constants::{MONO_FRAME_SIZE, SAMPLE_RATE_RAW};

use super::{codec::OpusDecoderState, error::DcaError, Codec, Container, Input, Metadata, Reader};
use audiopus::Channels;
use serde::{Deserialize, Serialize};
use std::{alloc::Layout, ffi::OsStr, mem};
use symphonia_core::{
    codecs::CodecParameters,
    io::{MediaSourceStream, SeekBuffered},
};
use tokio::{fs::File as TokioFile, io::AsyncReadExt};

use symphonia::core::{
    codecs::CODEC_TYPE_OPUS,
    errors::{self as symph_err, Result as SymphResult},
    formats::prelude::*,
    io::ReadBytes,
    meta::{Metadata as SymphMetadata, MetadataLog},
    probe::{Descriptor, Instantiate, QueryDescriptor},
};

/// Creates a streamed audio source from a DCA file.
/// Currently only accepts the [DCA1 format](https://github.com/bwmarrin/dca).
pub async fn dca<P: AsRef<OsStr>>(path: P) -> Result<Input, DcaError> {
    _dca(path.as_ref()).await
}

async fn _dca(path: &OsStr) -> Result<Input, DcaError> {
    let mut reader = TokioFile::open(path).await.map_err(DcaError::IoError)?;

    let mut header = [0u8; 4];

    // Read in the magic number to verify it's a DCA file.
    reader
        .read_exact(&mut header)
        .await
        .map_err(DcaError::IoError)?;

    if header != b"DCA1"[..] {
        return Err(DcaError::InvalidHeader);
    }

    let size = reader
        .read_i32_le()
        .await
        .map_err(|_| DcaError::InvalidHeader)?;

    // Sanity check
    if size < 2 {
        return Err(DcaError::InvalidSize(size));
    }

    let mut raw_json = Vec::with_capacity(size as usize);

    let mut json_reader = reader.take(size as u64);

    json_reader
        .read_to_end(&mut raw_json)
        .await
        .map_err(DcaError::IoError)?;

    let reader = json_reader.into_inner().into_std().await;

    let metadata: Metadata = serde_json::from_slice::<DcaMetadata>(raw_json.as_slice())
        .map_err(DcaError::InvalidMetadata)?
        .into();

    let stereo = metadata.channels == Some(2);

    Ok(Input::new(
        stereo,
        Reader::from_file(reader),
        Codec::Opus(OpusDecoderState::new().map_err(DcaError::Opus)?),
        Container::Dca {
            first_frame: (size as usize) + mem::size_of::<i32>() + header.len(),
        },
        Some(metadata),
    ))
}

impl QueryDescriptor for SymphDcaReader {
    fn query() -> &'static [Descriptor] {
        &[symphonia_core::support_format!(
            "dca",
            "DCA[0/1] Opus Wrapper",
            &["dca"],
            &[],
            &[b"DCA1"]
        )]
    }

    fn score(_context: &[u8]) -> u8 {
        255
    }
}

struct SeekAccel {
    frame_offsets: Vec<u64>,
    seek_index_fill_rate: u16,
    next_ts: u64,
}

impl SeekAccel {
    fn new(options: &FormatOptions, first_frame_index: u64) -> Self {
        let per_s = options.seek_index_fill_rate;
        let next_ts = (per_s as u64) * (SAMPLE_RATE_RAW as u64);

        Self {
            frame_offsets: vec![first_frame_index],
            seek_index_fill_rate: per_s,
            next_ts,
        }
    }

    fn update(&mut self, ts: u64, pos: u64) {
        if ts == self.next_ts {
            self.next_ts += (self.seek_index_fill_rate as u64) * (SAMPLE_RATE_RAW as u64);

            self.frame_offsets.push(pos);
        }
    }
}

/// DCA[0/1] Format reader for Symphonia
///
/// NOTE: DCA0 doesn't work right now since:
///
/// * No Magic bytes for it
/// * Symphonia doesn't yet use extension/MIME hints.
pub struct SymphDcaReader {
    source: MediaSourceStream,
    track: Option<Track>,
    metas: MetadataLog,
    seek_accel: SeekAccel,
    frame_pos: u64,
}

impl FormatReader for SymphDcaReader {
    fn try_new(mut source: MediaSourceStream, options: &FormatOptions) -> SymphResult<Self> {
        // Read in the magic number to verify it's a DCA file.
        let magic = source.read_quad_bytes()?;

        let read_meta = match &magic {
            b"DCA1" => true,
            _ if &magic[..3] == b"DCA" => {
                return symph_err::unsupported_error("unsupported DCA version");
            },
            _ => {
                source.seek_buffered_rel(-4);
                false
            },
        };

        let mut codec_params = CodecParameters::new();

        codec_params
            .for_codec(CODEC_TYPE_OPUS)
            .with_max_frames_per_packet(1)
            .with_sample_rate(SAMPLE_RATE_RAW as u32)
            .with_time_base(TimeBase::new(1, SAMPLE_RATE_RAW as u32))
            .with_sample_format(symphonia_core::sample::SampleFormat::F32);

        let stereo = if read_meta {
            let size = source.read_u32()?;

            // Sanity check
            if (size as i32) < 2 {
                return symph_err::decode_error("missing DCA1 metadata block");
            }

            let raw_json = source.read_boxed_slice_exact(size as usize)?;

            let metadata: SymphResult<DcaMetadata> =
                serde_json::from_slice::<DcaMetadata>(&raw_json)
                    .map_err(|_| symph_err::Error::DecodeError("malformed DCA1 metadata block"));

            let metadata: Metadata = metadata?.into();

            metadata.channels == Some(2)
        } else {
            true
        };

        // codec_params.with_channels((if stereo {symphonia::core::audio::Layout::Stereo} else {symphonia::core::audio::Layout::Mono}).into());

        let metas = MetadataLog::default();

        let bytes_read = source.pos();

        Ok(Self {
            source,
            track: Some(Track {
                id: 0,
                language: None,
                codec_params,
            }),
            metas,
            seek_accel: SeekAccel::new(options, bytes_read),
            frame_pos: 0,
        })
    }

    fn cues(&self) -> &[Cue] {
        // No cues in DCA...
        &[]
    }

    fn metadata(&mut self) -> SymphMetadata<'_> {
        self.metas.metadata()
    }

    fn seek(&mut self, mode: SeekMode, to: SeekTo) -> SymphResult<SeekedTo> {
        todo!()
    }

    fn tracks(&self) -> &[symphonia_core::formats::Track] {
        // DCA tracks can hold only one track by design.
        // Of course, a zero-length file is technically allowed,
        // in which case no track.
        if let Some(track) = self.track.as_ref() {
            std::slice::from_ref(track)
        } else {
            &[]
        }
    }

    fn default_track(&self) -> Option<&symphonia_core::formats::Track> {
        self.track.as_ref()
    }

    fn next_packet(&mut self) -> SymphResult<Packet> {
        let frame_pos = self.source.pos();

        let p_len = self.source.read_u16()? as i16;

        if p_len < 0 {
            return symph_err::decode_error("");
        }

        // FIXME: may not be 20ms frames.
        // Will be on my files, at least.
        let ts = (MONO_FRAME_SIZE as u64) * self.frame_pos;
        let out = Packet::new_from_boxed_slice(
            0,
            ts,
            MONO_FRAME_SIZE as u64,
            self.source.read_boxed_slice_exact(p_len as usize)?,
        );

        self.frame_pos += 1;

        self.seek_accel.update(ts, frame_pos);

        Ok(out)
    }

    fn into_inner(self: Box<Self>) -> MediaSourceStream {
        self.source
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DcaMetadata {
    pub dca: DcaInfo,
    pub opus: Opus,
    pub info: Option<Info>,
    pub origin: Option<Origin>,
    pub extra: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DcaInfo {
    pub version: u64,
    pub tool: Tool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Tool {
    pub name: String,
    pub version: String,
    pub url: Option<String>,
    pub author: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Opus {
    pub mode: String,
    pub sample_rate: u32,
    pub frame_size: u64,
    pub abr: Option<u64>,
    pub vbr: bool,
    pub channels: u8,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Info {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub cover: Option<String>,
    pub comments: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Origin {
    pub source: Option<String>,
    pub abr: Option<u64>,
    pub channels: Option<u8>,
    pub encoding: Option<String>,
    pub url: Option<String>,
}

impl From<DcaMetadata> for Metadata {
    fn from(mut d: DcaMetadata) -> Self {
        let (track, artist) = d
            .info
            .take()
            .map(|mut m| (m.title.take(), m.artist.take()))
            .unwrap_or_else(|| (None, None));

        let channels = Some(d.opus.channels);
        let sample_rate = Some(d.opus.sample_rate);

        Self {
            track,
            artist,

            channels,
            sample_rate,

            ..Default::default()
        }
    }
}
