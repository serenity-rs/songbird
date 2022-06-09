use crate::{
    constants::OPUS_PASSTHROUGH_STRIKE_LIMIT,
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
        match val {
            InputState::NotReady(_) => Self::Uninitialised,
            InputState::Preparing(_) => Self::Preparing,
            InputState::Ready(_, _) => Self::Playable,
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
    pub passthrough_violations: u8,
}

impl DecodeState {
    pub fn reset(&mut self) {
        self.inner_pos = 0;
        self.resampler = None;
    }

    pub fn record_and_check_passthrough_strike_final(&mut self, fatal: bool) -> bool {
        self.passthrough_violations = self.passthrough_violations.saturating_add(1);
        let blocked = fatal || self.passthrough_violations > OPUS_PASSTHROUGH_STRIKE_LIMIT;
        if blocked {
            self.passthrough = Passthrough::Block;
        }
        blocked
    }
}

impl Default for DecodeState {
    fn default() -> Self {
        Self {
            inner_pos: 0,
            resampler: None,
            passthrough: Passthrough::Inactive,
            passthrough_violations: 0,
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
