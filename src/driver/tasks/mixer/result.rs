use crate::{input::AudioStreamError, tracks::PlayError};
use std::sync::Arc;
use symphonia_core::errors::Error as SymphoniaError;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MixType {
    Passthrough(usize),
    MixedPcm(usize),
}

impl MixType {
    pub fn contains_audio(&self) -> bool {
        use MixType::*;

        match self {
            Passthrough(a) | MixedPcm(a) => *a != 0,
        }
    }
}

pub enum MixStatus {
    Live,
    Ended,
    Errored,
}

#[derive(Debug)]
pub enum InputReadyingError {
    Parsing(SymphoniaError),
    Creation(AudioStreamError),
    Seeking(SymphoniaError),
    Dropped,
    Waiting,
}

impl InputReadyingError {
    pub fn into_user(self) -> Option<PlayError> {
        match self {
            Self::Parsing(e) => Some(PlayError::Parse(Arc::new(e))),
            Self::Creation(e) => Some(PlayError::Create(Arc::new(e))),
            Self::Seeking(e) => Some(PlayError::Seek(Arc::new(e))),
            _ => None,
        }
    }
}
