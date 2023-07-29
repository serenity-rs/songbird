use super::{disposal, error::Result, message::*};
use crate::{
    constants::*,
    tracks::{PlayMode, Track},
    Config,
};
use audiopus::{
    coder::Encoder as OpusEncoder,
    softclip::SoftClip,
    Application as CodingMode,
    Bitrate,
    Channels,
};
use crypto_secretbox::SecretBox;
use discortp::{
    rtp::{MutableRtpPacket, RtpPacket},
    MutablePacket,
};
use flume::{Receiver, Sender, TryRecvError};
use rand::random;
use std::{convert::TryInto, time::Instant};
use tokio::runtime::Handle;
use tracing::{debug, error, instrument};

const TAG_SIZE: usize = SecretBox::<()>::TAG_SIZE;

pub struct Mixer {
    pub async_handle: Handle,
    pub bitrate: Bitrate,
    pub config: Config,
    pub conn_active: Option<MixerConnection>,
    pub deadline: Instant,
    pub disposer: Sender<DisposalMessage>,
    pub encoder: OpusEncoder,
    pub interconnect: Interconnect,
    pub mix_rx: Receiver<MixerMessage>,
    pub muted: bool,
    pub packet: [u8; VOICE_PACKET_MAX],
    pub prevent_events: bool,
    pub silence_frames: u8,
    pub skip_sleep: bool,
    pub soft_clip: SoftClip,
    pub tracks: Vec<Track>,
    pub ws: Option<Sender<WsMessage>>,
}

fn new_encoder(bitrate: Bitrate) -> Result<OpusEncoder> {
    let mut encoder = OpusEncoder::new(SAMPLE_RATE, Channels::Stereo, CodingMode::Audio)?;
    encoder.set_bitrate(bitrate)?;

    Ok(encoder)
}

impl Mixer {
    pub fn new(
        mix_rx: Receiver<MixerMessage>,
        async_handle: Handle,
        interconnect: Interconnect,
        config: Config,
    ) -> Self {
        let bitrate = DEFAULT_BITRATE;
        let encoder = new_encoder(bitrate)
            .expect("Failed to create encoder in mixing thread with known-good values.");
        let soft_clip = SoftClip::new(Channels::Stereo);

        let mut packet = [0u8; VOICE_PACKET_MAX];

        let mut rtp = MutableRtpPacket::new(&mut packet[..]).expect(
            "FATAL: Too few bytes in self.packet for RTP header.\
                (Blame: VOICE_PACKET_MAX?)",
        );
        rtp.set_version(RTP_VERSION);
        rtp.set_payload_type(RTP_PROFILE_TYPE);
        rtp.set_sequence(random::<u16>().into());
        rtp.set_timestamp(random::<u32>().into());

        let tracks = Vec::with_capacity(1.max(config.preallocated_tracks));

        // Create an object disposal thread here.
        let (disposer, disposal_rx) = flume::unbounded();
        std::thread::spawn(move || disposal::runner(disposal_rx));

        Self {
            async_handle,
            bitrate,
            config,
            conn_active: None,
            deadline: Instant::now(),
            disposer,
            encoder,
            interconnect,
            mix_rx,
            muted: false,
            packet,
            prevent_events: false,
            silence_frames: 0,
            skip_sleep: false,
            soft_clip,
            tracks,
            ws: None,
        }
    }

    fn run(&mut self) {
        let mut events_failure = false;
        let mut conn_failure = false;

        'runner: loop {
            if self.conn_active.is_some() {
                loop {
                    match self.mix_rx.try_recv() {
                        Ok(m) => {
                            let (events, conn, should_exit) = self.handle_message(m);
                            events_failure |= events;
                            conn_failure |= conn;

                            if should_exit {
                                break 'runner;
                            }
                        },

                        Err(TryRecvError::Disconnected) => {
                            break 'runner;
                        },

                        Err(TryRecvError::Empty) => {
                            break;
                        },
                    };
                }

                // The above action may have invalidated the connection; need to re-check!
                if self.conn_active.is_some() {
                    if let Err(e) = self.cycle().and_then(|_| self.audio_commands_events()) {
                        events_failure |= e.should_trigger_interconnect_rebuild();
                        conn_failure |= e.should_trigger_connect();

                        debug!("Mixer thread cycle: {:?}", e);
                    }
                }
            } else {
                match self.mix_rx.recv() {
                    Ok(m) => {
                        let (events, conn, should_exit) = self.handle_message(m);
                        events_failure |= events;
                        conn_failure |= conn;

                        if should_exit {
                            break 'runner;
                        }
                    },
                    Err(_) => {
                        break 'runner;
                    },
                }
            }

            // event failure? rebuild interconnect.
            // ws or udp failure? full connect
            // (soft reconnect is covered by the ws task.)
            //
            // in both cases, send failure is fatal,
            // but will only occur on disconnect.
            // expecting this is fairly noisy, so exit silently.
            if events_failure {
                self.prevent_events = true;
                let sent = self
                    .interconnect
                    .core
                    .send(CoreMessage::RebuildInterconnect);
                events_failure = false;

                if sent.is_err() {
                    break;
                }
            }

            if conn_failure {
                self.conn_active = None;
                let sent = self.interconnect.core.send(CoreMessage::FullReconnect);
                conn_failure = false;

                if sent.is_err() {
                    break;
                }
            }
        }
    }

    #[inline]
    fn handle_message(&mut self, msg: MixerMessage) -> (bool, bool, bool) {
        let mut events_failure = false;
        let mut conn_failure = false;
        let mut should_exit = false;

        use MixerMessage::*;

        let error = match msg {
            AddTrack(mut t) => {
                t.source.prep_with_handle(self.async_handle.clone());
                self.add_track(t)
            },
            SetTrack(t) => {
                self.tracks.clear();

                let mut out = self.fire_event(EventMessage::RemoveAllTracks);

                if let Some(mut t) = t {
                    t.source.prep_with_handle(self.async_handle.clone());

                    // Do this unconditionally: this affects local state infallibly,
                    // with the event installation being the remote part.
                    if let Err(e) = self.add_track(t) {
                        out = Err(e);
                    }
                }

                out
            },
            SetBitrate(b) => {
                self.bitrate = b;
                if let Err(e) = self.set_bitrate(b) {
                    error!("Failed to update bitrate {:?}", e);
                }
                Ok(())
            },
            SetMute(m) => {
                self.muted = m;
                Ok(())
            },
            SetConn(conn, ssrc) => {
                self.conn_active = Some(conn);
                let mut rtp = MutableRtpPacket::new(&mut self.packet[..]).expect(
                    "Too few bytes in self.packet for RTP header.\
                        (Blame: VOICE_PACKET_MAX?)",
                );
                rtp.set_ssrc(ssrc);
                rtp.set_sequence(random::<u16>().into());
                rtp.set_timestamp(random::<u32>().into());
                self.deadline = Instant::now();
                Ok(())
            },
            DropConn => {
                self.conn_active = None;
                Ok(())
            },
            ReplaceInterconnect(i) => {
                self.prevent_events = false;
                if let Some(ws) = &self.ws {
                    conn_failure |= ws.send(WsMessage::ReplaceInterconnect(i.clone())).is_err();
                }
                if let Some(conn) = &self.conn_active {
                    conn_failure |= conn
                        .udp_rx
                        .send(UdpRxMessage::ReplaceInterconnect(i.clone()))
                        .is_err();
                }
                self.interconnect = i;

                self.rebuild_tracks()
            },
            SetConfig(new_config) => {
                self.config = new_config.clone();

                if self.tracks.capacity() < self.config.preallocated_tracks {
                    self.tracks
                        .reserve(self.config.preallocated_tracks - self.tracks.len());
                }

                if let Some(conn) = &self.conn_active {
                    conn_failure |= conn
                        .udp_rx
                        .send(UdpRxMessage::SetConfig(new_config))
                        .is_err();
                }

                Ok(())
            },
            RebuildEncoder => match new_encoder(self.bitrate) {
                Ok(encoder) => {
                    self.encoder = encoder;
                    Ok(())
                },
                Err(e) => {
                    error!("Failed to rebuild encoder. Resetting bitrate. {:?}", e);
                    self.bitrate = DEFAULT_BITRATE;
                    self.encoder = new_encoder(self.bitrate)
                        .expect("Failed fallback rebuild of OpusEncoder with safe inputs.");
                    Ok(())
                },
            },
            Ws(new_ws_handle) => {
                self.ws = new_ws_handle;
                Ok(())
            },
            Poison => {
                should_exit = true;
                Ok(())
            },
        };

        if let Err(e) = error {
            events_failure |= e.should_trigger_interconnect_rebuild();
            conn_failure |= e.should_trigger_connect();
        }

        (events_failure, conn_failure, should_exit)
    }

    #[inline]
    fn fire_event(&self, event: EventMessage) -> Result<()> {
        // As this task is responsible for noticing the potential death of an event context,
        // it's responsible for not forcibly recreating said context repeatedly.
        if !self.prevent_events {
            self.interconnect.events.send(event)?;
            Ok(())
        } else {
            Ok(())
        }
    }

    #[inline]
    fn add_track(&mut self, mut track: Track) -> Result<()> {
        let evts = track.events.take().unwrap_or_default();
        let state = track.state();
        let handle = track.handle.clone();

        self.tracks.push(track);

        self.interconnect
            .events
            .send(EventMessage::AddTrack(evts, state, handle))?;

        Ok(())
    }

    // rebuilds the event thread's view of each track, in event of a full rebuild.
    #[inline]
    fn rebuild_tracks(&mut self) -> Result<()> {
        for track in self.tracks.iter_mut() {
            let evts = track.events.take().unwrap_or_default();
            let state = track.state();
            let handle = track.handle.clone();

            self.interconnect
                .events
                .send(EventMessage::AddTrack(evts, state, handle))?;
        }

        Ok(())
    }

    #[inline]
    fn audio_commands_events(&mut self) -> Result<()> {
        // Apply user commands.
        for (i, track) in self.tracks.iter_mut().enumerate() {
            // This causes fallible event system changes,
            // but if the event thread has died then we'll certainly
            // detect that on the tick later.
            // Changes to play state etc. MUST all be handled.
            track.process_commands(i, &self.interconnect);
        }

        // TODO: do without vec?
        let mut i = 0;
        let mut to_remove = Vec::with_capacity(self.tracks.len());
        while i < self.tracks.len() {
            let track = self
                .tracks
                .get_mut(i)
                .expect("Tried to remove an illegal track index.");

            if track.playing.is_done() {
                let p_state = track.playing();
                let to_drop = self.tracks.swap_remove(i);
                to_remove.push(i);
                self.fire_event(EventMessage::ChangeState(
                    i,
                    TrackStateChange::Mode(p_state),
                ))?;
                let _ = self.disposer.send(DisposalMessage::Track(to_drop));
            } else {
                i += 1;
            }
        }

        // Tick
        self.fire_event(EventMessage::Tick)?;

        // Then do removals.
        for i in &to_remove[..] {
            self.fire_event(EventMessage::RemoveTrack(*i))?;
        }

        Ok(())
    }

    #[inline]
    fn march_deadline(&mut self) {
        if self.skip_sleep {
            return;
        }

        std::thread::sleep(self.deadline.saturating_duration_since(Instant::now()));
        self.deadline += TIMESTEP_LENGTH;
    }

    pub fn cycle(&mut self) -> Result<()> {
        let mut mix_buffer = [0f32; STEREO_FRAME_SIZE];

        // Walk over all the audio files, combining into one audio frame according
        // to volume, play state, etc.
        let mut mix_len = {
            let mut rtp = MutableRtpPacket::new(&mut self.packet[..]).expect(
                "FATAL: Too few bytes in self.packet for RTP header.\
                    (Blame: VOICE_PACKET_MAX?)",
            );

            let payload = rtp.payload_mut();

            // self.mix_tracks(&mut payload[TAG_SIZE..], &mut mix_buffer)
            mix_tracks(
                &mut payload[TAG_SIZE..],
                &mut mix_buffer,
                &mut self.tracks,
                &self.interconnect,
                self.prevent_events,
            )
        };

        self.soft_clip.apply((&mut mix_buffer[..]).try_into()?)?;

        if self.muted {
            mix_len = MixType::MixedPcm(0);
        }

        if mix_len == MixType::MixedPcm(0) {
            if self.silence_frames > 0 {
                self.silence_frames -= 1;

                // Explicit "Silence" frame.
                let mut rtp = MutableRtpPacket::new(&mut self.packet[..]).expect(
                    "FATAL: Too few bytes in self.packet for RTP header.\
                        (Blame: VOICE_PACKET_MAX?)",
                );

                let payload = rtp.payload_mut();

                (&mut payload[TAG_SIZE..TAG_SIZE + SILENT_FRAME.len()])
                    .copy_from_slice(&SILENT_FRAME[..]);

                mix_len = MixType::Passthrough(SILENT_FRAME.len());
            } else {
                // Per official guidelines, send 5x silence BEFORE we stop speaking.
                if let Some(ws) = &self.ws {
                    // NOTE: this should prevent a catastrophic thread pileup.
                    // A full reconnect might cause an inner closed connection.
                    // It's safer to leave the central task to clean this up and
                    // pass the mixer a new channel.
                    let _ = ws.send(WsMessage::Speaking(false));
                }

                self.march_deadline();

                return Ok(());
            }
        } else {
            self.silence_frames = 5;
        }

        if let Some(ws) = &self.ws {
            ws.send(WsMessage::Speaking(true))?;
        }

        self.march_deadline();
        self.prep_and_send_packet(mix_buffer, mix_len)?;

        Ok(())
    }

    fn set_bitrate(&mut self, bitrate: Bitrate) -> Result<()> {
        self.encoder.set_bitrate(bitrate).map_err(Into::into)
    }

    #[inline]
    fn prep_and_send_packet(&mut self, buffer: [f32; 1920], mix_len: MixType) -> Result<()> {
        let conn = self
            .conn_active
            .as_mut()
            .expect("Shouldn't be mixing packets without access to a cipher + UDP dest.");

        let index = {
            let mut rtp = MutableRtpPacket::new(&mut self.packet[..]).expect(
                "FATAL: Too few bytes in self.packet for RTP header.\
                    (Blame: VOICE_PACKET_MAX?)",
            );

            let payload = rtp.payload_mut();
            let crypto_mode = conn.crypto_state.kind();

            let payload_len = match mix_len {
                MixType::Passthrough(opus_len) => opus_len,
                MixType::MixedPcm(_samples) => {
                    let total_payload_space = payload.len() - crypto_mode.payload_suffix_len();
                    self.encoder.encode_float(
                        &buffer[..STEREO_FRAME_SIZE],
                        &mut payload[TAG_SIZE..total_payload_space],
                    )?
                },
            };

            let final_payload_size = conn
                .crypto_state
                .write_packet_nonce(&mut rtp, TAG_SIZE + payload_len);

            conn.crypto_state.kind().encrypt_in_place(
                &mut rtp,
                &conn.cipher,
                final_payload_size,
            )?;

            RtpPacket::minimum_packet_size() + final_payload_size
        };

        // TODO: This is dog slow, don't do this.
        // Can we replace this with a shared ring buffer + semaphore?
        // i.e., do something like double/triple buffering in graphics.
        conn.udp_tx
            .send(UdpTxMessage::Packet(self.packet[..index].to_vec()))?;

        let mut rtp = MutableRtpPacket::new(&mut self.packet[..]).expect(
            "FATAL: Too few bytes in self.packet for RTP header.\
                (Blame: VOICE_PACKET_MAX?)",
        );
        rtp.set_sequence(rtp.get_sequence() + 1);
        rtp.set_timestamp(rtp.get_timestamp() + MONO_FRAME_SIZE as u32);

        Ok(())
    }
}

#[derive(Debug, Eq, PartialEq)]
enum MixType {
    Passthrough(usize),
    MixedPcm(usize),
}

#[inline]
fn mix_tracks<'a>(
    opus_frame: &'a mut [u8],
    mix_buffer: &mut [f32; STEREO_FRAME_SIZE],
    tracks: &mut Vec<Track>,
    interconnect: &Interconnect,
    prevent_events: bool,
) -> MixType {
    let mut len = 0;

    // Opus frame passthrough.
    // This requires that we have only one track, who has volume 1.0, and an
    // Opus codec type.
    let do_passthrough = tracks.len() == 1 && {
        let track = &tracks[0];
        (track.volume - 1.0).abs() < f32::EPSILON && track.source.supports_passthrough()
    };

    for (i, track) in tracks.iter_mut().enumerate() {
        let vol = track.volume;
        let stream = &mut track.source;

        if track.playing != PlayMode::Play {
            continue;
        }

        let (temp_len, opus_len) = if do_passthrough {
            (0, track.source.read_opus_frame(opus_frame).ok())
        } else {
            (stream.mix(mix_buffer, vol), None)
        };

        len = len.max(temp_len);
        if temp_len > 0 || opus_len.is_some() {
            track.step_frame();
        } else if track.do_loop() {
            if let Ok(time) = track.seek_time(Default::default()) {
                // have to reproduce self.fire_event here
                // to circumvent the borrow checker's lack of knowledge.
                //
                // In event of error, one of the later event calls will
                // trigger the event thread rebuild: it is more prudent that
                // the mixer works as normal right now.
                if !prevent_events {
                    let _ = interconnect.events.send(EventMessage::ChangeState(
                        i,
                        TrackStateChange::Position(time),
                    ));
                    let _ = interconnect.events.send(EventMessage::ChangeState(
                        i,
                        TrackStateChange::Loops(track.loops, false),
                    ));
                }
            }
        } else {
            track.end();
        }

        if let Some(opus_len) = opus_len {
            return MixType::Passthrough(opus_len);
        }
    }

    MixType::MixedPcm(len)
}

/// The mixing thread is a synchronous context due to its compute-bound nature.
///
/// We pass in an async handle for the benefit of some Input classes (e.g., restartables)
/// who need to run their restart code elsewhere and return blank data until such time.
#[instrument(skip(interconnect, mix_rx, async_handle))]
pub(crate) fn runner(
    interconnect: Interconnect,
    mix_rx: Receiver<MixerMessage>,
    async_handle: Handle,
    config: Config,
) {
    let mut mixer = Mixer::new(mix_rx, async_handle, interconnect, config);

    mixer.run();

    let _ = mixer.disposer.send(DisposalMessage::Poison);
}
