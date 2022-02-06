use crate::input::AudioStreamError;
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
