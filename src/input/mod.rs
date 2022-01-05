//! Raw audio input data streams and sources.
//!
//! [`Input`] is handled in Songbird by combining metadata with:
//!  * A 48kHz audio bytestream, via [`Reader`],
//!  * A [`Container`] describing the framing mechanism of the bytestream,
//!  * A [`Codec`], defining the format of audio frames.
//!
//! When used as a [`Read`], the output bytestream will be a floating-point
//! PCM stream at 48kHz, matching the channel count of the input source.
//!
//! ## Opus frame passthrough.
//! Some sources, such as [`Compressed`] or the output of [`dca`], support
//! direct frame passthrough to the driver. This lets you directly send the
//! audio data you have *without decoding, re-encoding, or mixing*. In many
//! cases, this can greatly reduce the processing/compute cost of the driver.
//!
//! This functionality requires that:
//!  * only one track is active (including paused tracks),
//!  * that track's input supports direct Opus frame reads,
//!  * its [`Input`] [meets the promises described herein](codec/struct.OpusDecoderState.html#structfield.allow_passthrough),
//!  * and that track's volume is set to `1.0`.
//!
//! [`Input`]: Input
//! [`Reader`]: reader::Reader
//! [`Container`]: Container
//! [`Codec`]: Codec
//! [`Read`]: https://doc.rust-lang.org/std/io/trait.Read.html
//! [`Compressed`]: cached::Compressed
//! [`dca`]: dca()

mod adapter;
pub mod cached;
mod child;
mod compose;
mod dca;
mod error;
mod file;
mod http;
mod metadata;
mod opus;
pub mod registry;
pub mod utils;
mod ytdl;

pub use self::{
    adapter::*,
    child::*,
    compose::*,
    dca::DcaReader,
    error::*,
    file::*,
    http::*,
    metadata::AuxMetadata,
    opus::*,
    ytdl::*,
};

/// TODO: explain the role of symph.
pub use symphonia_core as core;

use std::io::Result as IoResult;
use symphonia_core::{
    codecs::{CodecRegistry, Decoder},
    errors::Error as SymphError,
    formats::FormatReader,
    io::{MediaSource, MediaSourceStream},
    probe::{Hint, Probe, ProbedMetadata},
};

/// Test text hello.
///
/// questions: how to merge with track management? how to handle metadata user-side?
pub enum Input {
    /// Probably want to define a trait so that people can spit out an
    /// input, maybe having an async context? This way I can:
    /// * permanantly sunset restartables: a great idea when people remember them,
    ///   but it'll be better if I can make their lazy mode the default.
    Lazy(Box<dyn Compose>),
    /// Account for tyhe case that someone proceses their stream entirely locally?
    /// FIXME: Rename me to Live.
    Live(LiveInput, Option<Box<dyn Compose>>),
}

#[allow(missing_docs)]
pub enum LiveInput {
    Raw(AudioStream<Box<dyn MediaSource>>),
    Wrapped(AudioStream<MediaSourceStream>),
    Parsed(Parsed),
}

#[allow(missing_docs)]
impl LiveInput {
    pub fn promote(self, codecs: &CodecRegistry, probe: &Probe) -> Result<Self, SymphError> {
        let mut out = self;

        if let LiveInput::Raw(r) = out {
            // TODO: allow passing in of MSS options?
            let mss = MediaSourceStream::new(r.input, Default::default());
            out = LiveInput::Wrapped(AudioStream {
                input: mss,
                hint: r.hint,
            });
        }

        if let LiveInput::Wrapped(w) = out {
            let hint = w.hint.unwrap_or_default();
            let input = w.input;

            let probe_data =
                probe.format(&hint, input, &Default::default(), &Default::default())?;
            let format = probe_data.format;
            let meta = probe_data.metadata;

            let mut default_track_id = format.default_track().map(|track| track.id);
            let mut decoder: Option<Box<dyn Decoder>> = None;

            // Take default track (if it exists), take first track to be found otherwise.
            for track in format.tracks() {
                if decoder.is_some() {
                    break;
                }

                if default_track_id.is_some() && Some(track.id) != default_track_id {
                    continue;
                }

                let this_decoder = codecs.make(&track.codec_params, &Default::default())?;

                decoder = Some(this_decoder);
                default_track_id = Some(track.id);
            }

            let track_id = default_track_id.unwrap();

            let p = Parsed {
                format,
                decoder: decoder.unwrap(),
                track_id,
                meta,
            };

            out = LiveInput::Parsed(p);
        }

        Ok(out)
    }
}

#[allow(missing_docs)]
pub struct AudioStream<T: Send> {
    pub input: T,
    pub hint: Option<Hint>,
}

#[allow(missing_docs)]
pub struct Parsed {
    pub format: Box<dyn FormatReader>,
    pub decoder: Box<dyn Decoder>,
    pub track_id: u32,
    pub meta: ProbedMetadata,
}
