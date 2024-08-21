use super::*;
use bytes::Bytes;
use std::collections::VecDeque;
use tracing::trace;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredPacket {
    pub packet: Bytes,
    // We need to store this as it's possible that a user can change config modes.
    pub decrypted: bool,
}

/// Determines whether an SSRC's packets should be decoded.
///
/// Playout requires us to keep an almost constant delay, to do so we build
/// a user's packet buffer up to the required length ([`Config::playout_buffer_length`])
/// ([`Self::Fill`]) and then emit packets on each tick ([`Self::Drain`]).
///
/// This gets a bit harder to reason about when users stop speaking. If a speech gap
/// lasts longer than the playout buffer, then we can simply swap from `Drain` -> `Fill`.
/// However, a genuine gap of `n` frames must lead to us reverting to `Fill` for `n` frames.
/// To compute this, we use the RTP timestamp of two `seq`-adjacent packets at playout: if the next
/// timestamp is too large, then we revert to `Fill`.
///
/// Small playout bursts also require care.
///
/// If timestamp info is incorrect, then in the worst case we eventually need to rebuffer if the delay
/// drains to zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlayoutMode {
    Fill,
    Drain,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PacketLookup {
    Packet(StoredPacket),
    MissedPacket,
    Filling,
}

#[derive(Debug)]
pub struct PlayoutBuffer {
    buffer: VecDeque<Option<StoredPacket>>,
    playout_mode: PlayoutMode,
    next_seq: RtpSequence,
    current_timestamp: Option<RtpTimestamp>,
}

impl PlayoutBuffer {
    pub fn new(capacity: usize, next_seq: RtpSequence) -> Self {
        Self {
            buffer: VecDeque::with_capacity(capacity),
            playout_mode: PlayoutMode::Fill,
            next_seq,
            current_timestamp: None,
        }
    }

    /// Slot a received RTP packet into the correct location in the playout buffer using
    /// its sequence number, subject to maximums.
    ///
    /// An out of bounds packet must create any remaining `None`s
    pub fn store_packet(&mut self, packet: StoredPacket, config: &Config) {
        let rtp = RtpPacket::new(&packet.packet)
            .expect("FATAL: earlier valid packet now invalid (store)");

        if self.current_timestamp.is_none() {
            self.current_timestamp = Some(reset_timeout(&rtp, config));
        }

        // compute index by taking wrapping difference between both seq numbers.
        // If the difference is *too big*, or in the past [also 'too big, in a way],
        // ignore the packet
        let desired_index = (rtp.get_sequence().0 - self.next_seq).0 as i16;

        if desired_index < 0 {
            trace!("Missed packet arrived late, discarding from playout.");
        } else if desired_index >= 64 {
            trace!("Packet arrived beyond playout max length: wanted slot {desired_index}.");
        } else {
            let index = desired_index as usize;
            while self.buffer.len() <= index {
                self.buffer.push_back(None);
            }
            self.buffer[index] = Some(packet);
        }

        if self.buffer.len() >= config.playout_buffer_length.get() {
            self.playout_mode = PlayoutMode::Drain;
        }
    }

    pub fn fetch_packet(&mut self) -> PacketLookup {
        if self.playout_mode == PlayoutMode::Fill {
            return PacketLookup::Filling;
        }

        let out = match self.buffer.pop_front() {
            Some(Some(pkt)) => {
                let rtp = RtpPacket::new(&pkt.packet)
                    .expect("FATAL: earlier valid packet now invalid (fetch)");

                // The curr_ts captures the current playout point; we want to
                // be able to emit *all* packets with a smaller timestamp.
                // However, we need to handle this in a wrap-safe way.
                // ts_diff shows where the current time lies if we treat packet_ts
                // as 0, s.t. ts_diff >= 0 (equiv) packet_time <= curr_time.
                let curr_ts = self.current_timestamp.unwrap();
                let ts_diff = (curr_ts - rtp.get_timestamp().0).0 as i32;

                if ts_diff >= 0 {
                    self.next_seq = (rtp.get_sequence() + 1).0;

                    PacketLookup::Packet(pkt)
                } else {
                    trace!("Witholding packet: ts_diff is {ts_diff}");
                    self.buffer.push_front(Some(pkt));
                    self.playout_mode = PlayoutMode::Fill;
                    PacketLookup::Filling
                }
            },
            Some(None) => {
                self.next_seq += 1;
                PacketLookup::MissedPacket
            },
            None => PacketLookup::Filling,
        };

        if self.buffer.is_empty() {
            self.playout_mode = PlayoutMode::Fill;
            self.current_timestamp = None;
        }

        if let Some(ts) = self.current_timestamp.as_mut() {
            *ts += &(MONO_FRAME_SIZE as u32);
        }

        out
    }

    pub fn next_seq(&self) -> RtpSequence {
        self.next_seq
    }
}

#[inline]
fn reset_timeout(packet: &RtpPacket<'_>, config: &Config) -> RtpTimestamp {
    let t_shift = MONO_FRAME_SIZE * config.playout_buffer_length.get();
    (packet.get_timestamp() + (t_shift as u32)).0
}
