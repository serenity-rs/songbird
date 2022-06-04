//! Raw audio input data streams and sources.
//!
//! [`Input`]s in Songbird are based on [symphonia], which provides demuxing,
//! decoding and management of synchronous byte sources (i.e., any items which
//! `impl` [`Read`]).
//!
//! Songbird adds support for the Opus codec to symphonia via [`OpusDecoder`],
//! the [DCA1] file format via [`DcaReader`], and a simple PCM adapter via [`RawReader`];
//! the [format] and [codec registries] in [`codecs`] install these on top of those
//! enabled in your `Cargo.toml` when you include symphonia.
//!
//! ## Common sources
//! * Any owned byte slice: `&'static [u8]`, `Bytes`, or `Vec<u8>`,
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
//! [`OpusDecoder`]: codecs::OpusDecoder
//! [`DcaReader`]: codecs::DcaReader
//! [`RawReader`]: codecs::RawReader
//! [format]: static@codecs::PROBE
//! [codec registries]: static@codecs::CODEC_REGISTRY

mod adapters;
mod audiostream;
pub mod codecs;
mod compose;
mod error;
mod live_input;
mod metadata;
mod parsed;
mod sources;
pub mod utils;

pub use self::{
    adapters::*,
    audiostream::*,
    compose::*,
    error::*,
    live_input::*,
    metadata::*,
    parsed::*,
    sources::*,
};

pub use symphonia_core as core;

use std::{error::Error, io::Cursor};
use symphonia_core::{codecs::CodecRegistry, probe::Probe};
use tokio::runtime::Handle as TokioHandle;

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

    /// Tries to get any information about this audio stream acquired during parsing.
    ///
    /// Only exists when this input is both [`Self::Live`] and has been fully parsed.
    /// In general, you probably want to use [`Self::aux_metadata`].
    pub fn metadata(&mut self) -> Result<Metadata, MetadataError> {
        if let Self::Live(live, _) = self {
            live.metadata()
        } else {
            Err(MetadataError::NotLive)
        }
    }

    #[allow(missing_docs)]
    pub fn make_live(self, handle: TokioHandle) -> Result<Self, AudioStreamError> {
        use Input::*;

        let out = match self {
            Lazy(mut lazy) => {
                let (created, lazy) = if lazy.should_create_async() {
                    let (tx, rx) = flume::bounded(1);
                    handle.spawn(async move {
                        let out = lazy.create_async().await;
                        let _ = tx.send_async((out, lazy));
                    });
                    rx.recv().map_err(|_| {
                        let err_msg: Box<dyn Error + Send + Sync> =
                            "async Input create handler panicked".into();
                        AudioStreamError::Fail(err_msg)
                    })?
                } else {
                    (lazy.create(), lazy)
                };

                Live(LiveInput::Raw(created?), Some(lazy))
            },
            other => other,
        };

        Ok(out)
    }

    #[allow(missing_docs)]
    pub async fn make_live_async(self) -> Result<Self, AudioStreamError> {
        use Input::*;

        let out = match self {
            Lazy(mut lazy) => {
                let (created, lazy) = if lazy.should_create_async() {
                    (lazy.create_async().await, lazy)
                } else {
                    tokio::task::spawn_blocking(move || (lazy.create(), lazy))
                        .await
                        .map_err(|_| {
                            let err_msg: Box<dyn Error + Send + Sync> =
                                "synchronous Input create handler panicked".into();
                            AudioStreamError::Fail(err_msg)
                        })?
                };

                Live(LiveInput::Raw(created?), Some(lazy))
            },
            other => other,
        };

        Ok(out)
    }

    #[allow(missing_docs)]
    pub fn make_playable(
        self,
        codecs: &CodecRegistry,
        probe: &Probe,
        handle: TokioHandle,
    ) -> Result<Self, MakePlayableError> {
        let out = self.make_live(handle)?;
        match out {
            Self::Lazy(_) => unreachable!(),
            Self::Live(input, lazy) => {
                let promoted = input.promote(codecs, probe)?;
                Ok(Self::Live(promoted, lazy))
            },
        }
    }

    #[allow(missing_docs)]
    pub fn is_playable(&self) -> bool {
        match self {
            Self::Live(input, _) => input.is_playable(),
            _ => false,
        }
    }

    #[allow(missing_docs)]
    pub fn parsed(&self) -> Option<&Parsed> {
        if let Self::Live(input, _) = self {
            input.parsed()
        } else {
            None
        }
    }

    #[allow(missing_docs)]
    pub fn parsed_mut(&mut self) -> Option<&mut Parsed> {
        if let Self::Live(input, _) = self {
            input.parsed_mut()
        } else {
            None
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
