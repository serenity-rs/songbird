mod decode_sizes;
mod playout_buffer;
mod ssrc_state;

use self::{decode_sizes::*, playout_buffer::*, ssrc_state::*};

use super::message::*;
use crate::{
    constants::*,
    driver::CryptoMode,
    events::{context_data::VoiceTick, internal_data::*, CoreContext},
    Config,
};
use bytes::BytesMut;
use crypto_secretbox::XSalsa20Poly1305 as Cipher;
use discortp::{
    demux::{self, DemuxedMut},
    rtp::RtpPacket,
};
use flume::Receiver;
use std::{
    collections::{HashMap, HashSet},
    num::Wrapping,
    sync::Arc,
    time::Duration,
};
use tokio::{net::UdpSocket, select, time::Instant};
use tracing::{error, instrument, trace, warn};

type RtpSequence = Wrapping<u16>;
type RtpTimestamp = Wrapping<u32>;
type RtpSsrc = u32;

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
                () = tokio::time::sleep_until(playout_time) => {
                    let mut tick = VoiceTick {
                        speaking: HashMap::new(),
                        silent: HashSet::new(),
                    };

                    for (ssrc, state) in &mut self.decoder_map {
                        match state.get_voice_tick(&self.config) {
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
                () = tokio::time::sleep_until(cleanup_time) => {
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

                        _ = self.ssrc_signalling.disconnected_users.remove(&id);
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
