use std::collections::{HashMap, HashSet};

use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
#[allow(missing_docs)]
/// Opus audio packet, received from another stream (detailed in `packet`).
/// `payload_offset` contains the true payload location within the raw packet's `payload()`,
/// if extensions or raw packet data are required.
///
/// Valid audio data (`Some(audio)` where `audio.len >= 0`) contains up to 20ms of 16-bit stereo PCM audio
/// at 48kHz, using native endianness. Songbird will not send audio for silent regions, these should
/// be inferred using [`SpeakingUpdate`]s (and filled in by the user if required using arrays of zeroes).
///
/// If `audio.len() == 0`, then this packet arrived out-of-order. If `None`, songbird was not configured
/// to decode received packets.
///
/// [`SpeakingUpdate`]: crate::events::CoreEvent::SpeakingUpdate
pub struct VoiceTick {
    pub speaking: HashMap<u32, VoiceData>,

    pub silent: HashSet<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
#[allow(missing_docs)]
pub struct VoiceData {
    pub packet: Option<RtpData>,
    pub decoded_voice: Vec<i16>,
}
