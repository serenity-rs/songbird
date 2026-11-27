use super::*;
use bytes::Bytes;
use std::collections::VecDeque;
use tracing::trace;

/// How far ahead of the next sequence number to be played a packet may be stored.
///
/// Runs of padding-only packets (bandwidth probes) may be interleaved with the first audio
/// packets a user sends, each taking a sequence number: this must leave room for a second or so
/// of them.
const MAX_SLOTS_AHEAD: i16 = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredPacket {
    pub packet: Bytes,
    // We need to store this as it's possible that a user can change config modes.
    pub decrypted: bool,
}

/// One sequence number's place in the playout buffer.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Slot {
    /// Not received (yet).
    Empty,
    /// Received, but carrying no audio: played out as nothing at all.
    Skipped,
    Packet(StoredPacket),
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
    buffer: VecDeque<Slot>,
    playout_mode: PlayoutMode,
    next_seq: RtpSequence,
    current_timestamp: Option<RtpTimestamp>,
    consecutive_store_fails: usize,
}

impl PlayoutBuffer {
    pub fn new(capacity: usize, next_seq: RtpSequence) -> Self {
        Self {
            buffer: VecDeque::with_capacity(capacity),
            playout_mode: PlayoutMode::Fill,
            next_seq,
            current_timestamp: None,
            consecutive_store_fails: 0,
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
        // If the difference is *too big*, or in the past [also too big, in a way],
        // ignore the packet
        let pkt_seq = rtp.get_sequence().0;
        let desired_index = (pkt_seq - self.next_seq).0 as i16;

        // Similar concept to fetch_packet -- if there's a critical desync, and we're unwilling
        // to slot this packet into an empty/stuck buffer then behave as though this packet is the next
        // sequence number we're releasing.
        let err_threshold = i16::from(config.playout_buffer_length.get() * 5);
        let handling_desync = (self.buffer.is_empty()
            || self.consecutive_store_fails >= (err_threshold as usize))
            && desired_index >= err_threshold;

        if desired_index < 0 {
            trace!("Missed packet arrived late, discarding from playout.");
        } else if !handling_desync && desired_index >= MAX_SLOTS_AHEAD {
            trace!(
                "Packet arrived beyond playout max length({}): wanted slot {desired_index}.\
                ts {}, seq {}, next_out_seq {}",
                rtp.get_ssrc(),
                rtp.get_timestamp().0,
                rtp.get_sequence().0,
                self.next_seq,
            );
            self.consecutive_store_fails += 1;
        } else {
            let index = if handling_desync {
                self.buffer.clear();
                self.next_seq = pkt_seq;

                0
            } else {
                desired_index as usize
            };
            while self.buffer.len() <= index {
                self.buffer.push_back(Slot::Empty);
            }
            self.buffer[index] = Slot::Packet(packet);
            self.consecutive_store_fails = 0;
        }

        if self.buffer.len() >= usize::from(config.playout_buffer_length.get()) {
            self.playout_mode = PlayoutMode::Drain;
        }
    }

    /// Mark a sequence number as received but carrying no audio, so that playout passes
    /// over it rather than reporting a lost packet.
    pub fn skip_packet(&mut self, pkt_seq: RtpSequence) {
        let desired_index = (pkt_seq - self.next_seq).0 as i16;

        if (0..MAX_SLOTS_AHEAD).contains(&desired_index) {
            let index = desired_index as usize;
            while self.buffer.len() <= index {
                self.buffer.push_back(Slot::Empty);
            }
            self.buffer[index] = Slot::Skipped;
        }

        self.drop_leading_skips();
    }

    /// Skipped slots at the head of the buffer will never be played: release them now.
    fn drop_leading_skips(&mut self) {
        while self.buffer.front() == Some(&Slot::Skipped) {
            self.buffer.pop_front();
            self.next_seq += 1;
        }
    }

    pub fn fetch_packet(&mut self, config: &Config) -> PacketLookup {
        if self.playout_mode == PlayoutMode::Fill {
            return PacketLookup::Filling;
        }

        self.drop_leading_skips();

        let out = match self.buffer.pop_front() {
            Some(Slot::Packet(pkt)) => {
                let rtp = RtpPacket::new(&pkt.packet)
                    .expect("FATAL: earlier valid packet now invalid (fetch)");

                // The curr_ts captures the current playout point; we want to
                // be able to emit *all* packets with a smaller timestamp.
                // However, we need to handle this in a wrap-safe way.
                // ts_diff shows where the current time lies if we treat packet_ts
                // as 0, s.t. ts_diff >= 0 (equiv) packet_time <= curr_time.
                let curr_ts = self.current_timestamp.as_mut().unwrap();
                let pkt_ts = rtp.get_timestamp().0;
                let ts_diff = (*curr_ts - pkt_ts).0 as i32;

                // At least one client in the wild has seen unusual timestamp behaviour: the
                // first packet sent out in a run of audio may have an older timestamp.
                // This could be badly timestamped, or could conceivably be an orphaned packet
                // from a prior run, or e.g.:
                //  (n x RTP) -> [>100ms delay] -> (RTP) -> [long O(s) delay] -> (m x RTP)
                // This leaves us with two adjacent packets in the same playout with wildly varying
                // timestamps. We have a slightly tricky situation -- we need to preserve accurate
                // timing to correctly drain/refill/recreate very small pauses in audio, but don't
                // want to block indefinitely.
                //
                // We have a compromise here -- if an adjacent (Drain) packet has a ts gap
                // larger than it would take to go through multiple Fill/Drain cycles, then
                // treat its TS as the next expected value to avoid jamming the buffer and losing
                // later audio.
                let skip_after = i32::try_from(
                    usize::from(config.playout_buffer_length.get()) * 5 * MONO_FRAME_SIZE,
                )
                .unwrap_or((AUDIO_FRAME_RATE * 2 * MONO_FRAME_SIZE) as i32);

                if ts_diff >= 0 {
                    // At or before expected timestamp.
                    self.next_seq = (rtp.get_sequence() + 1).0;

                    PacketLookup::Packet(pkt)
                } else if ts_diff <= -skip_after {
                    // >5 playouts ahead.
                    self.next_seq = (rtp.get_sequence() + 1).0;
                    *curr_ts = pkt_ts;
                    PacketLookup::Packet(pkt)
                } else {
                    trace!("Witholding packet: ts_diff is {ts_diff}");
                    self.buffer.push_front(Slot::Packet(pkt));
                    self.playout_mode = PlayoutMode::Fill;
                    PacketLookup::Filling
                }
            },
            // Skipped slots never reach the head here: `drop_leading_skips` released them.
            Some(Slot::Empty | Slot::Skipped) => {
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
    let t_shift = MONO_FRAME_SIZE * usize::from(config.playout_buffer_length.get());
    (packet.get_timestamp() + (t_shift as u32)).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use discortp::rtp::MutableRtpPacket;

    const FIRST_SEQ: u16 = 1000;
    const FIRST_TS: u32 = 48_000;
    const RTP_HEADER_LEN: usize = 12;
    const PAYLOAD_LEN: usize = 8;

    fn packet(seq: u16, ts: u32) -> StoredPacket {
        let mut bytes = vec![0u8; RTP_HEADER_LEN + PAYLOAD_LEN];
        let mut rtp = MutableRtpPacket::new(&mut bytes[..]).unwrap();
        rtp.set_version(RTP_VERSION);
        rtp.set_payload_type(RTP_PROFILE_TYPE);
        rtp.set_sequence(seq.into());
        rtp.set_timestamp(ts.into());
        StoredPacket {
            packet: Bytes::from(bytes),
            decrypted: true,
        }
    }

    /// The sequence number and timestamp of the `n`th audio frame after the first.
    fn frame(n: u16) -> (u16, u32) {
        (
            FIRST_SEQ + n,
            FIRST_TS + u32::from(n) * MONO_FRAME_SIZE as u32,
        )
    }

    fn played_seq(lookup: &PacketLookup) -> Option<u16> {
        match lookup {
            PacketLookup::Packet(pkt) =>
                Some(RtpPacket::new(&pkt.packet).unwrap().get_sequence().0 .0),
            _ => None,
        }
    }

    fn buffer() -> PlayoutBuffer {
        PlayoutBuffer::new(16, Wrapping(FIRST_SEQ))
    }

    #[test]
    fn skipped_slots_are_not_reported_as_losses() {
        let config = Config::default();
        let mut buffer = buffer();
        let probes = 3;

        // Audio frame 0, then three padding-only packets, then audio frames at the following
        // sequence numbers whose timestamps carry on from frame 0.
        let (seq, ts) = frame(0);
        buffer.store_packet(packet(seq, ts), &config);
        for i in 1..=probes {
            buffer.skip_packet(Wrapping(FIRST_SEQ + i));
        }
        for n in 1..8 {
            let (_, ts) = frame(n);
            buffer.store_packet(packet(FIRST_SEQ + probes + n, ts), &config);
        }

        let mut played = vec![];
        for _ in 0..8 {
            let lookup = buffer.fetch_packet(&config);
            assert_ne!(lookup, PacketLookup::MissedPacket);
            played.extend(played_seq(&lookup));
        }

        let expected: Vec<u16> = std::iter::once(FIRST_SEQ)
            .chain((1..8).map(|n| FIRST_SEQ + probes + n))
            .collect();
        assert_eq!(played, expected);
    }

    #[test]
    fn leading_skips_do_not_take_a_slot() {
        let mut buffer = buffer();

        buffer.skip_packet(Wrapping(FIRST_SEQ));
        buffer.skip_packet(Wrapping(FIRST_SEQ + 1));

        assert!(buffer.buffer.is_empty());
        assert_eq!(buffer.next_seq(), Wrapping(FIRST_SEQ + 2));
    }
}
