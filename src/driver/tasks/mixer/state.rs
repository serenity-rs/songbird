use crate::{
    driver::tasks::message::*,
    input::{Compose, Input, LiveInput, Metadata, Parsed},
    tracks::ReadyState,
};
use flume::Receiver;
use rubato::FftFixedOut;
use std::time::Instant;
use symphonia_core::formats::SeekTo;

pub enum InputState {
    NotReady(Input),
    Preparing(PreparingInfo),
    Ready(Parsed, Option<Box<dyn Compose>>),
}

impl InputState {
    pub fn metadata(&mut self) -> Option<Metadata> {
        if let Self::Ready(parsed, _) = self {
            Some(parsed.into())
        } else {
            None
        }
    }
}

impl From<Input> for InputState {
    fn from(val: Input) -> Self {
        match val {
            a @ Input::Lazy(_) => InputState::NotReady(a),
            Input::Live(live, rec) => match live {
                LiveInput::Parsed(p) => InputState::Ready(p, rec),
                other => InputState::NotReady(Input::Live(other, rec)),
            },
        }
    }
}

impl From<&InputState> for ReadyState {
    fn from(val: &InputState) -> Self {
        use InputState::*;

        match val {
            NotReady(_) => Self::Uninitialised,
            Preparing(_) => Self::Preparing,
            Ready(_, _) => Self::Playable,
        }
    }
}

pub struct PreparingInfo {
    #[allow(dead_code)]
    pub time: Instant,
    pub queued_seek: Option<SeekTo>,
    pub callback: Receiver<MixerInputResultMessage>,
}

pub struct DecodeState {
    pub inner_pos: usize,
    pub resampler: Option<(usize, FftFixedOut<f32>, Vec<Vec<f32>>)>,
    pub passthrough: Passthrough,
}

impl DecodeState {
    pub fn reset(&mut self) {
        self.inner_pos = 0;
        self.resampler = None;
    }
}

impl Default for DecodeState {
    fn default() -> Self {
        Self {
            inner_pos: 0,
            resampler: None,
            passthrough: Passthrough::Inactive,
        }
    }
}

/// Simple state to manage decoder resets etc.
///
/// Inactive->Active transitions should trigger a reset.
///
/// Block should be used if a source contains known-bad packets:
/// it's unlikely that packet sizes will vary, but if they do then
/// we can't passthrough (and every attempt will trigger a codec reset,
/// which probably won't sound too smooth).
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum Passthrough {
    Active,
    Inactive,
    Block,
}
