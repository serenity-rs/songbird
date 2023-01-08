use super::{
    error::{Error, Result},
    message::*,
    Config,
};
use crate::{
    constants::*,
    driver::{CryptoMode, DecodeMode},
    events::{
        context_data::{RtpData, VoiceData, VoiceTick},
        internal_data::*,
        CoreContext,
    },
};
use audiopus::{
    coder::Decoder as OpusDecoder,
    error::{Error as OpusError, ErrorCode},
    packet::Packet as OpusPacket,
    Channels,
};
use bytes::{Bytes, BytesMut};
use discortp::{
    demux::{self, DemuxedMut},
    rtp::{RtpExtensionPacket, RtpPacket},
    Packet,
    PacketSize,
};
use flume::Receiver;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    convert::TryInto,
    sync::Arc,
    time::Duration,
};
use tokio::{net::UdpSocket, select, time::Instant};
use tracing::{error, instrument, trace, warn};
use xsalsa20poly1305::XSalsa20Poly1305 as Cipher;

#[derive(Clone, Debug, Eq, PartialEq)]
struct StoredPacket {
    packet: Bytes,
    // We need to store this as it's possible that a user can change config modes.
    decrypted: bool,
}

#[derive(Debug)]
struct SsrcState {
    playout_buffer: VecDeque<Option<StoredPacket>>,
    playout_mode: PlayoutMode,
    decoder: OpusDecoder,
    next_seq: RtpSequence,
    decode_size: PacketDecodeSize,
    prune_time: Instant,
    disconnected: bool,
    current_timestamp: Option<RtpTimestamp>,
}

// Store `current_time'? I.e., if queue is empty then force-set to

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PacketDecodeSize {
    /// Minimum frame size on Discord.
    TwentyMillis,
    /// Hybrid packet, sent by Firefox web client.
    ///
    /// Likely 20ms frame + 10ms frame.
    ThirtyMillis,
    /// Next largest frame size.
    FortyMillis,
    /// Maximum Opus frame size.
    SixtyMillis,
    /// Maximum Opus packet size: 120ms.
    Max,
}

impl PacketDecodeSize {
    fn bump_up(self) -> Self {
        match self {
            Self::TwentyMillis => Self::ThirtyMillis,
            Self::ThirtyMillis => Self::FortyMillis,
            Self::FortyMillis => Self::SixtyMillis,
            Self::SixtyMillis | Self::Max => Self::Max,
        }
    }

    fn can_bump_up(self) -> bool {
        self != Self::Max
    }

    fn len(self) -> usize {
        match self {
            Self::TwentyMillis => STEREO_FRAME_SIZE,
            Self::ThirtyMillis => (STEREO_FRAME_SIZE / 2) * 3,
            Self::FortyMillis => 2 * STEREO_FRAME_SIZE,
            Self::SixtyMillis => 3 * STEREO_FRAME_SIZE,
            Self::Max => 6 * STEREO_FRAME_SIZE,
        }
    }
}

type RtpSequence = u16;
type RtpTimestamp = u32;
type RtpSsrc = u32;

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
enum PacketLookup {
    Packet(StoredPacket),
    MissedPacket,
    Filling,
}

impl SsrcState {
    fn new(pkt: &RtpPacket<'_>, config: &Config) -> Self {
        let playout_capacity = config.playout_buffer_length.get() + config.playout_spike_length;
        Self {
            playout_buffer: VecDeque::with_capacity(playout_capacity),
            playout_mode: PlayoutMode::Fill,
            decoder: OpusDecoder::new(SAMPLE_RATE, Channels::Stereo)
                .expect("Failed to create new Opus decoder for source."),
            next_seq: pkt.get_sequence().into(),
            decode_size: PacketDecodeSize::TwentyMillis,
            prune_time: Instant::now() + config.decode_state_timeout,
            disconnected: false,

            current_timestamp: Some(reset_timeout(pkt, config)),
        }
    }

    fn refresh_timer(&mut self, state_timeout: Duration) {
        if !self.disconnected {
            self.prune_time = Instant::now() + state_timeout;
        }
    }

    /// Slot a received RTP packet into the correct location in the playout buffer using
    /// its sequence number, subject to maximums.
    ///
    /// An out of bounds packet must create any remaining `None`s
    fn store_packet(&mut self, packet: StoredPacket, config: &Config) {
        let rtp = RtpPacket::new(&packet.packet)
            .expect("FATAL: earlier valid packet now invalid (store)");

        if self.current_timestamp.is_none() {
            self.current_timestamp = Some(reset_timeout(&rtp, config));
        }

        // i32 has full range of u16 in each direction.
        let desired_index = (rtp.get_sequence().0 .0 as i32) - (self.next_seq as i32);

        if desired_index < 0 {
            trace!("Missed packet arrived late, discarding from playout.");
        } else if desired_index >= 64 {
            trace!("Packet arrived beyond playout max length.");
        } else {
            let index = desired_index as usize;
            while self.playout_buffer.len() <= index {
                self.playout_buffer.push_back(None);
            }
            self.playout_buffer[index] = Some(packet);
        }

        if self.playout_buffer.len() >= config.playout_buffer_length.get() {
            self.playout_mode = PlayoutMode::Drain;
        }
    }

    fn fetch_packet(&mut self) -> PacketLookup {
        if self.playout_mode == PlayoutMode::Fill {
            return PacketLookup::Filling;
        }

        // TODO: unset timestamp if queue is drained.
        let out = match self.playout_buffer.pop_front() {
            Some(Some(pkt)) => {
                let rtp = RtpPacket::new(&pkt.packet)
                    .expect("FATAL: earlier valid packet now invalid (fetch)");

                let curr_ts = self.current_timestamp.unwrap();
                let ts_diff = curr_ts.wrapping_sub(rtp.get_timestamp().0 .0) as i32;

                if ts_diff <= 0 {
                    PacketLookup::Packet(pkt)
                } else {
                    trace!("Witholding packet: ts_diff is {ts_diff}");
                    self.playout_buffer.push_front(Some(pkt));
                    self.playout_mode = PlayoutMode::Fill;
                    PacketLookup::Filling
                }
            },
            Some(None) => PacketLookup::MissedPacket,
            None => PacketLookup::Filling,
        };

        if self.playout_buffer.is_empty() {
            self.playout_mode = PlayoutMode::Fill;
            self.current_timestamp = None;
        }

        if let Some(ts) = self.current_timestamp.as_mut() {
            *ts = ts.wrapping_add(MONO_FRAME_SIZE as u32);
        }

        out
    }

    fn process(&mut self, config: &Config) -> Result<Option<VoiceData>> {
        // Acquire a packet from the playout buffer:
        // Update nexts, lasts...
        // different cases: null packet who we want to decode as a miss, and packet who we must ignore temporarily.
        let m_pkt = self.fetch_packet();
        let pkt = match m_pkt {
            PacketLookup::Packet(StoredPacket { packet, decrypted }) => {
                let rtp = RtpPacket::new(&packet)
                    .expect("FATAL: earlier valid packet now invalid (fetch)");
                self.next_seq = rtp.get_sequence().0 .0 + 1;

                Some((packet, decrypted))
            },
            PacketLookup::MissedPacket => {
                self.next_seq += 1;

                None
            },
            PacketLookup::Filling => return Ok(None),
        };

        let mut out = VoiceData {
            packet: None,
            decoded_voice: None,
        };

        let should_decode = config.decode_mode == DecodeMode::Decode;

        if let Some((packet, decrypted)) = pkt {
            let rtp = RtpPacket::new(&packet).unwrap();
            let extensions = rtp.get_extension() != 0;

            let payload = rtp.payload();
            let payload_offset = CryptoMode::payload_prefix_len();
            let payload_end_pad = payload.len() - config.crypto_mode.payload_suffix_len();

            // We still need to compute missed packets here in case of long loss chains or similar.
            // This occurs due to the fallback in 'store_packet' (i.e., empty buffer and massive seq difference).
            // Normal losses should be handled by the below `else` branch.
            let new_seq: u16 = rtp.get_sequence().into();
            let missed_packets = new_seq.saturating_sub(self.next_seq);

            // TODO: maybe hand over audio and extension indices alongside packet?
            let (audio, _packet_size) = self.scan_and_decode(
                &payload[payload_offset..payload_end_pad],
                extensions,
                missed_packets,
                should_decode && decrypted,
            )?;

            let rtp_data = RtpData {
                packet,
                payload_offset,
                payload_end_pad,
            };

            out.packet = Some(rtp_data);
            out.decoded_voice = audio;
        } else if should_decode {
            let mut audio = vec![0; self.decode_size.len()];
            let dest_samples = (&mut audio[..])
                .try_into()
                .expect("Decode logic will cap decode buffer size at i32::MAX.");
            let len = self.decoder.decode(None, dest_samples, false)?;
            audio.truncate(2 * len);

            out.decoded_voice = Some(audio);
        }

        Ok(Some(out))
    }

    fn scan_and_decode(
        &mut self,
        data: &[u8],
        extension: bool,
        missed_packets: u16,
        decode: bool,
    ) -> Result<(Option<Vec<i16>>, usize)> {
        let start = if extension {
            RtpExtensionPacket::new(data)
                .map(|pkt| pkt.packet_size())
                .ok_or_else(|| {
                    error!("Extension packet indicated, but insufficient space.");
                    Error::IllegalVoicePacket
                })
        } else {
            Ok(0)
        }?;

        let pkt = if decode {
            let mut out = vec![0; self.decode_size.len()];

            for _ in 0..missed_packets {
                let missing_frame: Option<OpusPacket> = None;
                let dest_samples = (&mut out[..])
                    .try_into()
                    .expect("Decode logic will cap decode buffer size at i32::MAX.");
                if let Err(e) = self.decoder.decode(missing_frame, dest_samples, false) {
                    warn!("Issue while decoding for missed packet: {:?}.", e);
                }
            }

            // In general, we should expect 20 ms frames.
            // However, Discord occasionally like to surprise us with something bigger.
            // This is *sender-dependent behaviour*.
            //
            // This should scan up to find the "correct" size that a source is using,
            // and then remember that.
            loop {
                let tried_audio_len = self.decoder.decode(
                    Some(data[start..].try_into()?),
                    (&mut out[..]).try_into()?,
                    false,
                );
                match tried_audio_len {
                    Ok(audio_len) => {
                        // Decoding to stereo: audio_len refers to sample count irrespective of channel count.
                        // => multiply by number of channels.
                        out.truncate(2 * audio_len);

                        break;
                    },
                    Err(OpusError::Opus(ErrorCode::BufferTooSmall)) => {
                        if self.decode_size.can_bump_up() {
                            self.decode_size = self.decode_size.bump_up();
                            out = vec![0; self.decode_size.len()];
                        } else {
                            error!("Received packet larger than Opus standard maximum,");
                            return Err(Error::IllegalVoicePacket);
                        }
                    },
                    Err(e) => {
                        error!("Failed to decode received packet: {:?}.", e);
                        return Err(e.into());
                    },
                }
            }

            Some(out)
        } else {
            None
        };

        Ok((pkt, data.len() - start))
    }
}

struct UdpRx {
    cipher: Cipher,
    decoder_map: HashMap<RtpSsrc, SsrcState>,
    config: Config,
    rx: Receiver<UdpRxMessage>,
    ssrc_signalling: Arc<SsrcTracker>,
    udp_socket: UdpSocket,
}

impl UdpRx {
    #[instrument(skip(self))]
    async fn run(&mut self, interconnect: &mut Interconnect) {
        let mut cleanup_time = Instant::now();
        let mut playout_time = Instant::now() + TIMESTEP_LENGTH;
        let mut byte_dest: Option<BytesMut> = None;

        loop {
            if byte_dest.is_none() {
                byte_dest = Some(BytesMut::zeroed(VOICE_PACKET_MAX));
            }

            select! {
                Ok((len, _addr)) = self.udp_socket.recv_from(byte_dest.as_mut().unwrap()) => {
                    let mut pkt = byte_dest.take().unwrap();
                    pkt.truncate(len);

                    self.process_udp_message(interconnect, pkt);
                },
                msg = self.rx.recv_async() => {
                    match msg {
                        Ok(UdpRxMessage::ReplaceInterconnect(i)) => {
                            *interconnect = i;
                        },
                        Ok(UdpRxMessage::SetConfig(c)) => {
                            self.config = c;
                        },
                        Err(flume::RecvError::Disconnected) => break,
                    }
                },
                _ = tokio::time::sleep_until(playout_time) => {
                    let mut tick = VoiceTick {
                        speaking: HashMap::new(),
                        silent: HashSet::new(),
                    };

                    for (ssrc, state) in &mut self.decoder_map {
                        match state.process(&self.config) {
                            Ok(Some(data)) => {
                                tick.speaking.insert(*ssrc, data);
                            },
                            Ok(None) => {
                                if !state.disconnected {
                                    tick.silent.insert(*ssrc);
                                }
                            },
                            Err(e) => {
                                warn!("Decode error for SSRC {ssrc}: {e:?}");
                                tick.silent.insert(*ssrc);
                            },
                        }
                    }

                    playout_time += TIMESTEP_LENGTH;

                    drop(interconnect.events.send(EventMessage::FireCoreEvent(CoreContext::VoiceTick(tick))));
                },
                _ = tokio::time::sleep_until(cleanup_time) => {
                    // periodic cleanup.
                    let now = Instant::now();

                    // check ssrc map to see if the WS task has informed us of any disconnects.
                    loop {
                        // This is structured in an odd way to prevent deadlocks.
                        // while-let seemed to keep the dashmap iter() alive for block scope, rather than
                        // just the initialiser.
                        let id = {
                            if let Some(id) = self.ssrc_signalling.disconnected_users.iter().next().map(|v| *v.key()) {
                                id
                            } else {
                                break;
                            }
                        };

                        let _ = self.ssrc_signalling.disconnected_users.remove(&id);
                        if let Some((_, ssrc)) = self.ssrc_signalling.user_ssrc_map.remove(&id) {
                            if let Some(state) = self.decoder_map.get_mut(&ssrc) {
                                // don't cleanup immediately: leave for later cycle
                                // this is key with reorder/jitter buffers where we may
                                // still need to decode post disconnect for ~0.2s.
                                state.prune_time = now + Duration::from_secs(1);
                                state.disconnected = true;
                            }
                        }
                    }

                    // now remove all dead ssrcs.
                    self.decoder_map.retain(|_, v| v.prune_time > now);

                    cleanup_time = now + Duration::from_secs(5);
                },
            }
        }
    }

    fn process_udp_message(&mut self, interconnect: &Interconnect, mut packet: BytesMut) {
        // NOTE: errors here (and in general for UDP) are not fatal to the connection.
        // Panics should be avoided due to adversarial nature of rx'd packets,
        // but correct handling should not prompt a reconnect.
        //
        // For simplicity, if the event task fails then we nominate the mixing thread
        // to rebuild their context etc. (hence, the `let _ =` statements.), as it will
        // try to make contact every 20ms.
        let crypto_mode = self.config.crypto_mode;

        match demux::demux_mut(packet.as_mut()) {
            DemuxedMut::Rtp(mut rtp) => {
                if !rtp_valid(&rtp.to_immutable()) {
                    error!("Illegal RTP message received.");
                    return;
                }

                let packet_data = if self.config.decode_mode.should_decrypt() {
                    let out = crypto_mode
                        .decrypt_in_place(&mut rtp, &self.cipher)
                        .map(|(s, t)| (s, t, true));

                    if let Err(e) = out {
                        warn!("RTP decryption failed: {:?}", e);
                    }

                    out.ok()
                } else {
                    None
                };

                // TODO: slot packet into Bytes here.
                // move deocde processing AWAY from here.
                // send raw pkt to event ctx as Bytes + tag [Rtp/Rtcp].

                let rtp = rtp.to_immutable();
                let (rtp_body_start, rtp_body_tail, decrypted) = packet_data.unwrap_or_else(|| {
                    (
                        CryptoMode::payload_prefix_len(),
                        crypto_mode.payload_suffix_len(),
                        false,
                    )
                });

                let entry = self
                    .decoder_map
                    .entry(rtp.get_ssrc())
                    .or_insert_with(|| SsrcState::new(&rtp, &self.config));

                // Only do this on RTP, rather than RTCP -- this pins decoder state liveness
                // to *speech* rather than just presence.
                entry.refresh_timer(self.config.decode_state_timeout);

                let store_pkt = StoredPacket {
                    packet: packet.freeze(),
                    decrypted,
                };
                let packet = store_pkt.packet.clone();
                entry.store_packet(store_pkt, &self.config);

                drop(interconnect.events.send(EventMessage::FireCoreEvent(
                    CoreContext::RtpPacket(InternalRtpPacket {
                        packet,
                        payload_offset: rtp_body_start,
                        payload_end_pad: rtp_body_tail,
                    }),
                )));
            },
            DemuxedMut::Rtcp(mut rtcp) => {
                let packet_data = if self.config.decode_mode.should_decrypt() {
                    let out = crypto_mode.decrypt_in_place(&mut rtcp, &self.cipher);

                    if let Err(e) = out {
                        warn!("RTCP decryption failed: {:?}", e);
                    }

                    out.ok()
                } else {
                    None
                };

                let (start, tail) = packet_data.unwrap_or_else(|| {
                    (
                        CryptoMode::payload_prefix_len(),
                        crypto_mode.payload_suffix_len(),
                    )
                });

                drop(interconnect.events.send(EventMessage::FireCoreEvent(
                    CoreContext::RtcpPacket(InternalRtcpPacket {
                        packet: packet.freeze(),
                        payload_offset: start,
                        payload_end_pad: tail,
                    }),
                )));
            },
            DemuxedMut::FailedParse(t) => {
                warn!("Failed to parse message of type {:?}.", t);
            },
            DemuxedMut::TooSmall => {
                warn!("Illegal UDP packet from voice server.");
            },
        }
    }
}

#[instrument(skip(interconnect, rx, cipher))]
pub(crate) async fn runner(
    mut interconnect: Interconnect,
    rx: Receiver<UdpRxMessage>,
    cipher: Cipher,
    config: Config,
    udp_socket: UdpSocket,
    ssrc_signalling: Arc<SsrcTracker>,
) {
    trace!("UDP receive handle started.");

    let mut state = UdpRx {
        cipher,
        decoder_map: HashMap::new(),
        config,
        rx,
        ssrc_signalling,
        udp_socket,
    };

    state.run(&mut interconnect).await;

    trace!("UDP receive handle stopped.");
}

#[inline]
fn rtp_valid(packet: &RtpPacket<'_>) -> bool {
    packet.get_version() == RTP_VERSION && packet.get_payload_type() == RTP_PROFILE_TYPE
}

#[inline]
fn reset_timeout(packet: &RtpPacket<'_>, config: &Config) -> RtpTimestamp {
    let t_shift = MONO_FRAME_SIZE * config.playout_buffer_length.get();
    (packet.get_timestamp() - (t_shift as u32)).0 .0
}
