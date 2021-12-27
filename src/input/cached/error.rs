#![allow(missing_docs)]

use crate::input::AudioStreamError;
use audiopus::error::Error as OpusError;
use serde_json::Error as JsonError;
use streamcatcher::CatcherError;
use symphonia_core::errors::Error as SymphError;
use tokio::task::JoinError;

#[derive(Debug)]
pub enum Error {
    Create(AudioStreamError),
    CreatePanicked,
    Streamcatcher(CatcherError),
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
    fn from(val: JoinError) -> Self {
        Self::CreatePanicked
    }
}

#[derive(Debug)]
pub enum CocdecCacheError {
    Create(AudioStreamError),
    Parse(SymphError),
    Opus(OpusError),
    MetadataEncoding(JsonError),
    MetadataTooLarge,
    CreatePanicked,
    Streamcatcher(CatcherError),
    StreamNotAtStart,
}

impl From<AudioStreamError> for CocdecCacheError {
    fn from(val: AudioStreamError) -> Self {
        Self::Create(val)
    }
}

impl From<CatcherError> for CocdecCacheError {
    fn from(val: CatcherError) -> Self {
        Self::Streamcatcher(val)
    }
}

impl From<JoinError> for CocdecCacheError {
    fn from(val: JoinError) -> Self {
        Self::CreatePanicked
    }
}

impl From<JsonError> for CocdecCacheError {
    fn from(val: JsonError) -> Self {
        Self::MetadataEncoding(val)
    }
}

impl From<OpusError> for CocdecCacheError {
    fn from(val: OpusError) -> Self {
        Self::Opus(val)
    }
}

impl From<SymphError> for CocdecCacheError {
    fn from(val: SymphError) -> Self {
        Self::Parse(val)
    }
}
