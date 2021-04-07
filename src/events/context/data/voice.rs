use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
/// Opus audio packet, received from another stream (detailed in `packet`).
/// `payload_offset` contains the true payload location within the raw packet's `payload()`,
/// if extensions or raw packet data are required.
/// If `audio.len() == 0`, then this packet arrived out-of-order.
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
