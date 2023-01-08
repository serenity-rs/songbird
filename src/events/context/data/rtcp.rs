use discortp::rtcp::RtcpPacket;

use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
/// Telemetry/statistics packet, received from another stream (detailed in `packet`).
/// `payload_offset` contains the true payload location within the raw packet's `payload()`,
/// to allow manual decoding of `Rtcp` packet bodies.
pub struct RtcpData {
    /// Raw RTCP packet data.
    pub packet: Bytes,
    /// Byte index into the packet body (after headers) for where the payload begins.
    pub payload_offset: usize,
    /// Number of bytes at the end of the packet to discard.
    pub payload_end_pad: usize,
}

impl RtcpData {
    #[allow(missing_docs)]
    pub fn rtcp(&'_ self) -> RtcpPacket<'_> {
        RtcpPacket::new(&self.packet)
            .expect("FATAL: leaked illegally small RTP packet from UDP Rx task.")
    }
}
