//! Raw audio input data streams and sources.
//!
//! [`Input`]s in Songbird are based on [symphonia], which provides demuxing,
//! decoding and management of synchronous byte sources (i.e., any items which
//! `impl` [`Read`]).
//!
//! Songbird adds support for the Opus codec to symphonia via [`OpusDecoder`],
//! the [DCA1] file format via [`DcaReader`], and a simple PCM adapter via [`RawReader`];
//! the format and codec registries in [`registry::*`] install these on top of those
//! enabled in your `Cargo.toml` when you include symphonia.
//!
//! ## Common sources
//! * [`File`] offers a lazy way to open local audio files,
//! * [`HttpRequest`] streams a given file from a URL using the reqwest HTTP library,
//! * [`YoutubeDl`] uses `yt-dlp` (or any other `youtube-dl`-like program) to scrape
//!   a target URL for a usable audio stream, before opening an [`HttpRequest`].
//!
//! ## Adapters
//! Songbird includes several adapters to make developing your own inputs easier:
//! * [`cached::*`], which allow seeking and shared caching of an input stream (storing
//!   it in memory in a variety of formats),
//! * [`ChildContainer`] for managing audio given by a process chain,
//! * [`RawAdapter`], for feeding in a synchronous `f32`-PCM stream, and
//! * [`AsyncAdapterStream`], for passing bytes from an `AsyncRead` (`+ AsyncSeek`) stream
//!   into the mixer.
//!
//! ## Opus frame passthrough.
//! Some sources, such as [`Compressed`] or any WebM/Opus/DCA file, support
//! direct frame passthrough to the driver. This lets you directly send the
//! audio data you have *without decoding, re-encoding, or mixing*. In many
//! cases, this can greatly reduce the CPU cost required by the driver.
//!
//! This functionality requires that:
//!  * only one track is active (including paused tracks),
//!  * that track's input supports direct Opus frame reads,
//!  * this input's frames are all sized to 20ms.
//!  * and that track's volume is set to `1.0`.
//!
//! [`Input`]s which are almost suitable but which have **any** illegal frames will be
//! blocked from passthrough to prevent glitches such as repeated encoder frame gaps.
//!
//! [symphonia]: https://docs.rs/symphonia
//! [`Read`]: https://doc.rust-lang.org/std/io/trait.Read.html
//! [`Compressed`]: cached::Compressed
//! [DCA1]: https://github.com/bwmarrin/dca
//! [`registry::*`]: registry
//! [`cached::*`]: cached

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
mod raw;
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
    raw::*,
    ytdl::*,
};

pub use symphonia_core as core;

use std::io::{Cursor, Result as IoResult};
use symphonia_core::{
    codecs::{CodecRegistry, Decoder},
    errors::Error as SymphError,
    formats::FormatReader,
    io::{MediaSource, MediaSourceStream},
    probe::{Hint, Probe, ProbedMetadata},
};

/// A possibly lazy audio source.
///
/// This can be created from a wide variety of sources:
/// * TODO: enumerate -- in-memory via AsRef<[u8]>,
/// * Files, HTTP sources, ...
///
/// # Example
///
/// TODO: show use of diff sources?
pub enum Input {
    /// A byte source which is not yet initialised.
    ///
    /// When a parent track is either played or explicitly readied, the inner [`Compose`]
    /// is used to create an [`Input::Live`].
    ///
    /// [`Compose`]: Compose
    /// [`Input::Live`]: Input::Live
    Lazy(
        /// A trait object which can be used to (re)create a usable byte stream.
        Box<dyn Compose>,
    ),
    /// An initialised byte source.
    ///
    /// This contains a raw byte stream, the lazy initialiser that was used,
    /// as well as any symphonia-specific format data and/or hints.
    Live(
        /// The byte source, plus symphonia-specific data.
        LiveInput,
        /// The struct used to initialise this source, if available.
        ///
        /// This is used to recreate the stream when a source does not support
        /// backward seeking, if present.
        Option<Box<dyn Compose>>,
    ),
}

impl Input {
    /// Requests auxiliary metadata which can be accessed without parsing the file.
    ///
    /// This method will never be called by songbird but allows, for instance, access to metadata
    /// which might only be visible to a web crawler, e.g., uploader or source URL.
    ///
    /// This requires that the [`Input`] has a [`Compose`] available to use, otherwise it
    /// will always fail with [`AudioStreamError::Unsupported`].
    pub async fn aux_metadata(&mut self) -> Result<AuxMetadata, AudioStreamError> {
        match self {
            Self::Lazy(ref mut composer) => composer.aux_metadata().await,
            Self::Live(_, Some(ref mut composer)) => composer.aux_metadata().await,
            _ => Err(AudioStreamError::Unsupported),
        }
    }
}

impl<T: AsRef<[u8]> + Send + Sync + 'static> From<T> for Input {
    fn from(val: T) -> Self {
        let raw_src = LiveInput::Raw(AudioStream {
            input: Box::new(Cursor::new(val)),
            hint: None,
        });

        Input::Live(raw_src, None)
    }
}

/// An initialised audio source.
///
/// This type's variants reflect files at different stages of readiness for use by
/// symphonia. [`Parsed`] file streams are ready for playback.
///
/// [`Parsed`]: Self::Parsed
pub enum LiveInput {
    /// An unread, raw file stream.
    Raw(AudioStream<Box<dyn MediaSource>>),
    /// An unread file which has been wrapped with a large read-ahead buffer.
    Wrapped(AudioStream<MediaSourceStream>),
    /// An audio file which has had its headers parsed and decoder state built.
    Parsed(Parsed),
}

impl LiveInput {
    /// Converts this audio source into a [`Parsed`] object using the supplied format and codec
    /// registries.
    ///
    /// Where applicable, this will convert [`Raw`] -> [`Wrapped`] -> [`Parsed`], and will
    /// play the default track (or the first encountered track if this is not available) if a
    /// container holds multiple audio streams.
    ///
    /// *This is a blocking operation. Symphonia uses standard library I/O (e.g., [`Read`], [`Seek`]).
    /// If you wish to use this from an async task, you must do so within `spawn_blocking`.*
    ///
    /// [`Parsed`]: Self::Parsed
    /// [`Raw`]: Self::Raw
    /// [`Wrapped`]: Self::Wrapped
    /// [`Read`]: https://doc.rust-lang.org/std/io/trait.Read.html
    /// [`Seek`]: https://doc.rust-lang.org/std/io/trait.Seek.html
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

/// An unread byte stream for an audio file.
pub struct AudioStream<T: Send> {
    /// The wrapped file stream.
    ///
    /// An input stream *must not* have been read into past the start of the
    /// audio container's header.
    pub input: T,
    /// Extension and MIME type information which may help guide format selection.
    pub hint: Option<Hint>,
}

/// An audio file which has had its headers parsed and decoder state built.
pub struct Parsed {
    /// Audio packet, seeking, and state access for all tracks in a file.
    ///
    /// This may be used to access packets one at a time from the input file.
    /// Additionally, this exposes container-level and per track metadata which
    /// have been extracted.
    pub format: Box<dyn FormatReader>,
    /// Decoder state for the chosen track.
    pub decoder: Box<dyn Decoder>,
    /// The chosen track's ID.
    ///
    /// This is required to identify the correct packet stream inside the container.
    pub track_id: u32,
    /// Metadata extracted by symphonia while detecting a file's format.
    ///
    /// Typically, this detects metadata *outside* the file's core format (i.e.,
    /// ID3 tags in MP3 and WAV files).
    pub meta: ProbedMetadata,
}
