use super::*;
use crate::{
    constants::*,
    driver::{
        tasks::error::{Error, Result},
        CryptoMode,
        DecodeMode,
    },
    events::context_data::{RtpData, VoiceData},
    Config,
};
use audiopus::{
    coder::Decoder as OpusDecoder,
    error::{Error as OpusError, ErrorCode},
    packet::Packet as OpusPacket,
    Channels,
};
use discortp::{
    rtp::{RtpExtensionPacket, RtpPacket},
    Packet,
    PacketSize,
};
use std::{convert::TryInto, time::Duration};
use tokio::time::Instant;
use tracing::{error, warn};

#[derive(Debug)]
pub struct SsrcState {
    playout_buffer: PlayoutBuffer,
    decoder: OpusDecoder,
    decode_size: PacketDecodeSize,
    pub(crate) prune_time: Instant,
    pub(crate) disconnected: bool,
}

impl SsrcState {
    pub fn new(pkt: &RtpPacket<'_>, config: &Config) -> Self {
        let playout_capacity = config.playout_buffer_length.get() + config.playout_spike_length;

        Self {
            playout_buffer: PlayoutBuffer::new(playout_capacity, pkt.get_sequence().0),
            decoder: OpusDecoder::new(SAMPLE_RATE, Channels::Stereo)
                .expect("Failed to create new Opus decoder for source."),
            decode_size: PacketDecodeSize::TwentyMillis,
            prune_time: Instant::now() + config.decode_state_timeout,
            disconnected: false,
        }
    }

    pub fn store_packet(&mut self, packet: StoredPacket, config: &Config) {
        self.playout_buffer.store_packet(packet, config);
    }

    pub fn refresh_timer(&mut self, state_timeout: Duration) {
        if !self.disconnected {
            self.prune_time = Instant::now() + state_timeout;
        }
    }

    pub fn get_voice_tick(&mut self, config: &Config) -> Result<Option<VoiceData>> {
        // Acquire a packet from the playout buffer:
        // Update nexts, lasts...
        // different cases: null packet who we want to decode as a miss, and packet who we must ignore temporarily.
        let m_pkt = self.playout_buffer.fetch_packet();
        let pkt = match m_pkt {
            PacketLookup::Packet(StoredPacket { packet, decrypted }) => Some((packet, decrypted)),
            PacketLookup::MissedPacket => None,
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
            let missed_packets = new_seq.saturating_sub(self.playout_buffer.next_seq().0);

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
                let missing_frame: Option<OpusPacket<'_>> = None;
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
