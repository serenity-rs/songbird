use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
/// Opus audio packet, received from another stream (detailed in `packet`).
/// `payload_offset` contains the true payload location within the raw packet's `payload()`,
/// if extensions or raw packet data are required.
///
/// Valid audio data (`Some(audio)` where `audio.len >= 0`) contains up to 20ms of 16-bit mono PCM audio
/// at 48kHz, using native endianness. Songbird will not send audio for silent regions, these should
/// be inferred using [`SpeakingUpdate`]s (and filled in by the user if required using arrays of zeroes).
///
/// If `audio.len() == 0`, then this packet arrived out-of-order. If `None`, songbird was not configured
/// to decode received packets.
///
/// [`SpeakingUpdate`]: crate::events::CoreEvent::SpeakingUpdate
pub struct VoiceData<'a> {
    /// Decoded audio from this packet.
    pub audio: &'a Option<Vec<i16>>,
    /// Raw RTP packet data.
    ///
    /// Includes the SSRC (i.e., sender) of this packet.
    pub packet: &'a Rtp,
    /// Byte index into the packet body (after headers) for where the payload begins.
    pub payload_offset: usize,
    /// Number of bytes at the end of the packet to discard.
    pub payload_end_pad: usize,
}
