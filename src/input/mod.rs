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
mod dca;
pub mod error;
mod http;
mod metadata;
mod opus;
pub mod registry;
pub mod utils;
mod ytdl;

pub use self::{
    adapter::*,
    child::*,
    dca::DcaReader,
    http::*,
    metadata::Metadata,
    opus::*,
    ytdl::*,
};

/// TODO: explain the role of symph.
pub use symphonia_core as core;

use std::{
    error::Error as StdError,
    fmt::Display,
    io::Result as IoResult,
    path::Path,
    result::Result as StdResult,
    time::Duration,
};
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
    pub fn promote(self, codecs: &CodecRegistry, probe: &Probe) -> StdResult<Self, SymphError> {
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

// TODO: add an optional mechanism to query lightweight metadata?
// i.e., w/o instantiating track.
#[allow(missing_docs)]
#[async_trait::async_trait]
pub trait Compose: Send {
    /// Create a source synchronously.
    fn create(
        &mut self,
    ) -> std::result::Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError>;
    /// Create a source asynchronously.
    async fn create_async(
        &mut self,
    ) -> std::result::Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError>;
    /// Hmm.
    fn should_create_async(&self) -> bool;
}

#[allow(missing_docs)]
pub struct File<P: AsRef<Path>> {
    path: P,
}

#[allow(missing_docs)]
impl<P: AsRef<Path>> File<P> {
    pub fn new(path: P) -> Self {
        Self { path }
    }
}

impl<P: AsRef<Path> + Send + Sync + 'static> From<File<P>> for Input {
    fn from(val: File<P>) -> Self {
        Input::Lazy(Box::new(val))
    }
}

#[async_trait::async_trait]
impl<P: AsRef<Path> + Send + Sync> Compose for File<P> {
    fn create(
        &mut self,
    ) -> std::result::Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        let err: Box<dyn StdError + Send + Sync> =
            "Files should be created asynchronously.".to_string().into();
        Err(AudioStreamError::Fail(err))
    }

    async fn create_async(
        &mut self,
    ) -> std::result::Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        let file = tokio::fs::File::open(&self.path)
            .await
            .map_err(|io| AudioStreamError::Fail(Box::new(io)))?;

        let input = Box::new(file.into_std().await);

        let mut hint = Hint::default();
        if let Some(ext) = self.path.as_ref().extension().and_then(|s| s.to_str()) {
            hint.with_extension(ext);
        }

        Ok(AudioStream {
            input,
            hint: Some(hint),
        })
    }

    fn should_create_async(&self) -> bool {
        true
    }
}

#[allow(missing_docs)]
pub struct AudioStream<T: Send> {
    pub input: T,
    pub hint: Option<Hint>,
}

#[allow(missing_docs)]
#[non_exhaustive]
#[derive(Debug)]
pub enum AudioStreamError {
    RetryIn(Duration),
    Fail(Box<dyn StdError + Send>),
}

impl Display for AudioStreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("failed to create audio -- ")?;
        match self {
            Self::RetryIn(t) => f.write_fmt(format_args!("retry in {:.2}s", t.as_secs_f32())),
            Self::Fail(why) => f.write_fmt(format_args!("{}", why)),
        }
    }
}

impl StdError for AudioStreamError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        None
    }

    fn cause(&self) -> Option<&dyn StdError> {
        self.source()
    }
}

#[allow(missing_docs)]
pub struct Parsed {
    pub format: Box<dyn FormatReader>,
    pub decoder: Box<dyn Decoder>,
    pub track_id: u32,
    pub meta: ProbedMetadata,
}
