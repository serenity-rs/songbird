use crate::input::AudioStreamError;
use audiopus::error::Error as OpusError;
use serde_json::Error as JsonError;
use streamcatcher::CatcherError;
use symphonia_core::errors::Error as SymphError;
use tokio::task::JoinError;

/// Errors encountered using a [`Memory`] cached source.
///
/// [`Memory`]: super::Memory
#[derive(Debug)]
pub enum Error {
    /// The audio stream could not be created.
    Create(AudioStreamError),
    /// The audio stream failed to be created due to a panic in `spawn_blocking`.
    CreatePanicked,
    /// Streamcatcher's configuration was illegal, and the cache could not be created.
    Streamcatcher(CatcherError),
    /// The input stream had already been read (i.e., `Parsed`) and so the whole stream
    /// could not be used.
    StreamNotAtStart,
}

impl From<AudioStreamError> for Error {
    fn from(val: AudioStreamError) -> Self {
        Self::Create(val)
    }
}

impl From<CatcherError> for Error {
    fn from(val: CatcherError) -> Self {
        Self::Streamcatcher(val)
    }
}

impl From<JoinError> for Error {
    fn from(_val: JoinError) -> Self {
        Self::CreatePanicked
    }
}

/// Errors encountered using a [`Compressed`] or [`Decompressed`] cached source.
///
/// [`Compressed`]: super::Compressed
/// [`Decompressed`]: super::Decompressed
#[derive(Debug)]
pub enum CodecCacheError {
    /// The audio stream could not be created.
    Create(AudioStreamError),
    /// Symphonia failed to parse the container or decode the default stream.
    Parse(SymphError),
    /// The Opus encoder could not be created.
    Opus(OpusError),
    /// The file's metadata could not be converted to JSON.
    MetadataEncoding(JsonError),
    /// The input's metadata was too large after conversion to JSON to fit in a DCA file.
    MetadataTooLarge,
    /// The audio stream failed to be created due to a panic in `spawn_blocking`.
    CreatePanicked,
    /// The audio stream's channel count could not be determined.
    UnknownChannelCount,
    /// Streamcatcher's configuration was illegal, and the cache could not be created.
    Streamcatcher(CatcherError),
    /// The input stream had already been read (i.e., `Parsed`) and so the whole stream
    /// could not be used.
    StreamNotAtStart,
}

impl From<AudioStreamError> for CodecCacheError {
    fn from(val: AudioStreamError) -> Self {
        Self::Create(val)
    }
}

impl From<CatcherError> for CodecCacheError {
    fn from(val: CatcherError) -> Self {
        Self::Streamcatcher(val)
    }
}

impl From<JoinError> for CodecCacheError {
    fn from(_val: JoinError) -> Self {
        Self::CreatePanicked
    }
}

impl From<JsonError> for CodecCacheError {
    fn from(val: JsonError) -> Self {
        Self::MetadataEncoding(val)
    }
}

impl From<OpusError> for CodecCacheError {
    fn from(val: OpusError) -> Self {
        Self::Opus(val)
    }
}

impl From<SymphError> for CodecCacheError {
    fn from(val: SymphError) -> Self {
        Self::Parse(val)
    }
}
