use super::{disposal, error::Result, input_creator, input_parser, message::*};
use crate::{
    constants::*,
    input::{AudioStreamError, Compose, Parsed, SymphInput},
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
use discortp::{
    rtp::{MutableRtpPacket, RtpPacket},
    MutablePacket,
};
use flume::{Receiver, Sender, TryRecvError};
use rand::random;
use rubato::{FftFixedOut, Resampler};
use spin_sleep::SpinSleeper;
use std::{result::Result as StdResult, time::Instant};
use symphonia_core::{
    audio::{AudioBuffer, AudioBufferRef, Layout, SampleBuffer, Signal},
    conv::IntoSample,
    errors::Error as SymphoniaError,
    sample::Sample,
};
#[cfg(not(feature = "tokio-02-marker"))]
use tokio::runtime::Handle;
#[cfg(feature = "tokio-02-marker")]
use tokio_compat::runtime::Handle;
use tracing::{debug, error, instrument};
use xsalsa20poly1305::TAG_SIZE;

pub struct Mixer {
    pub async_handle: Handle,
    pub bitrate: Bitrate,
    pub config: Config,
    pub conn_active: Option<MixerConnection>,
    pub content_prep_sequence: u64,
    pub creator: Sender<InputCreateMessage>,
    pub deadline: Instant,
    pub disposer: Sender<DisposalMessage>,
    pub encoder: OpusEncoder,
    pub interconnect: Interconnect,
    pub mix_rx: Receiver<MixerMessage>,
    pub muted: bool,
    pub packet: [u8; VOICE_PACKET_MAX],
    pub parser: Sender<InputParseMessage>,
    pub prevent_events: bool,
    pub silence_frames: u8,
    pub skip_sleep: bool,
    pub sleeper: SpinSleeper,
    pub soft_clip: SoftClip,
    pub ws: Option<Sender<WsMessage>>,

    pub tracks: Vec<Track>,
    full_inputs: Vec<InputState>,
    input_states: Vec<LocalInput>,
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
        let full_inputs = Vec::with_capacity(1.max(config.preallocated_tracks));
        let input_states = Vec::with_capacity(1.max(config.preallocated_tracks));

        // Create an object disposal thread here.
        let (disposer, disposal_rx) = flume::unbounded();
        std::thread::spawn(move || disposal::runner(disposal_rx));

        // Create input processing pipelines.
        let (parser, parser_rx) = flume::unbounded();
        let ic_remote = interconnect.clone();
        let config_remote = config.clone();
        std::thread::spawn(move || input_parser::runner(ic_remote, parser_rx, config_remote));

        let (creator, creator_rx) = flume::unbounded();
        let parser_remote = parser.clone();
        let ic_remote = interconnect.clone();
        async_handle.spawn(async move {
            input_creator::runner(ic_remote, creator_rx, parser_remote).await;
        });

        Self {
            async_handle,
            bitrate,
            config,
            conn_active: None,
            content_prep_sequence: 0,
            creator,
            deadline: Instant::now(),
            disposer,
            encoder,
            interconnect,
            mix_rx,
            muted: false,
            packet,
            parser,
            prevent_events: false,
            silence_frames: 0,
            skip_sleep: false,
            sleeper: Default::default(),
            soft_clip,
            ws: None,

            tracks,
            full_inputs,
            input_states,
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
                // todo!();
                // t.source.prep_with_handle(self.async_handle.clone());
                self.add_track(t)
            },
            SetTrack(t) => {
                self.tracks.clear();

                let mut out = self.fire_event(EventMessage::RemoveAllTracks);

                if let Some(mut t) = t {
                    todo!();
                    // t.source.prep_with_handle(self.async_handle.clone());

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

                let _ = self
                    .creator
                    .send(InputCreateMessage::ReplaceInterconnect(i.clone()));
                let _ = self
                    .parser
                    .send(InputParseMessage::ReplaceInterconnect(i.clone()));

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
        // TODO: make this an error?
        if let Some(source) = track.source.take() {
            // TODO: not kill the recreation function?
            let full_input = match source {
                a @ SymphInput::Lazy(_) => InputState::NotReady(a),
                SymphInput::Live(live, rec) => match live {
                    crate::input::LiveInput::Parsed(p) => InputState::Ready(p, rec),
                    other => InputState::NotReady(SymphInput::Live(other, rec)),
                },
            };

            self.full_inputs.push(full_input);

            let evts = track.events.take().unwrap_or_default();
            let state = track.state();
            let handle = track.handle.clone();

            self.tracks.push(track);

            self.input_states.push(LocalInput {
                inner_pos: 0,
                resampler: None,
            });

            self.interconnect
                .events
                .send(EventMessage::AddTrack(evts, state, handle))?;
        } else {
            println!("WTF no track?");
        }

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

        // FIXME: make choice of spin-sleep/imprecise sleep optional in next breaking.
        self.sleeper
            .sleep(self.deadline.saturating_duration_since(Instant::now()));
        self.deadline += TIMESTEP_LENGTH;
    }

    pub fn cycle(&mut self) -> Result<()> {
        // TODO: allow mixer config to be either stereo or mono.
        let mut mix_buffer = [0f32; STEREO_FRAME_SIZE];
        let mut symph_buffer = SampleBuffer::<f32>::new(
            MONO_FRAME_SIZE as u64,
            symphonia_core::audio::SignalSpec::new_with_layout(
                SAMPLE_RATE_RAW as u32,
                Layout::Stereo,
            ),
        );
        let mut symph_mix = symphonia_core::audio::AudioBuffer::<f32>::new(
            MONO_FRAME_SIZE as u64,
            symphonia_core::audio::SignalSpec::new_with_layout(
                SAMPLE_RATE_RAW as u32,
                Layout::Stereo,
            ),
        );
        let mut symph_scratch = symphonia_core::audio::AudioBuffer::<f32>::new(
            MONO_FRAME_SIZE as u64,
            symphonia_core::audio::SignalSpec::new_with_layout(
                SAMPLE_RATE_RAW as u32,
                Layout::Stereo,
            ),
        );

        symph_mix.render_reserved(Some(MONO_FRAME_SIZE));

        // Walk over all the audio files, combining into one audio frame according
        // to volume, play state, etc.
        let mut mix_len = {
            let mut rtp = MutableRtpPacket::new(&mut self.packet[..]).expect(
                "FATAL: Too few bytes in self.packet for RTP header.\
                    (Blame: VOICE_PACKET_MAX?)",
            );

            let payload = rtp.payload_mut();

            let out = mix_tracks(
                &mut payload[TAG_SIZE..],
                &mut symph_mix,
                &mut symph_scratch,
                &mut self.tracks,
                &mut self.full_inputs,
                &mut self.input_states,
                &self.interconnect,
                &self.creator,
                &self.parser,
                self.prevent_events,
            );

            symph_buffer.copy_interleaved_typed(&symph_mix);

            out
        };

        self.soft_clip.apply(&mut mix_buffer[..])?;

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
        // self.prep_and_send_packet(mix_buffer, mix_len)?;
        self.prep_and_send_packet(symph_buffer.samples(), mix_len)?;

        Ok(())
    }

    fn set_bitrate(&mut self, bitrate: Bitrate) -> Result<()> {
        self.encoder.set_bitrate(bitrate).map_err(Into::into)
    }

    #[inline]
    fn prep_and_send_packet(&mut self, buffer: &[f32], mix_len: MixType) -> Result<()> {
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

enum InputState {
    NotReady(SymphInput),
    Preparing(PreparingInfo),
    Ready(Parsed, Option<Box<dyn Compose>>),
}

struct PreparingInfo {
    time: Instant,
    callback: Receiver<MixerInputResultMessage>,
}

#[derive(Debug, Eq, PartialEq)]
enum MixType {
    Passthrough(usize),
    MixedPcm(usize),
}

impl MixType {
    fn contains_audio(&self) -> bool {
        use MixType::*;

        match self {
            Passthrough(a) | MixedPcm(a) => *a != 0,
        }
    }
}

struct LocalInput {
    inner_pos: usize,
    resampler: Option<FftFixedOut<f32>>,
}

enum MixStatus {
    Live,
    Ended,
    Errored,
}

#[inline]
fn mix_symph_indiv(
    symph_mix: &mut AudioBuffer<f32>,
    resample_scratch: &mut AudioBuffer<f32>,
    input: &mut Parsed,
    local_state: &mut LocalInput,
    volume: f32,
) -> (MixType, MixStatus) {
    let mut samples_written = 0;
    let mut buf_in_progress = false;
    let mut track_status = MixStatus::Live;

    println!("mixing!");

    resample_scratch.clear();

    while samples_written != MONO_FRAME_SIZE {
        println!("SAMPLES: {}/{}.", samples_written, MONO_FRAME_SIZE);
        // TODO: move out elsewhere? Try to init local state with default track?
        let source_packet = if local_state.inner_pos != 0 {
            // This is a deliberate unwrap:
            println!("Getting old packet.");
            input.decoder.last_decoded()
        } else {
            if let Ok(pkt) = input.format.next_packet() {
                println!(
                    "Getting new packet: want {}, saw {}.",
                    input.track_id,
                    pkt.track_id()
                );

                if pkt.track_id() != input.track_id {
                    println!("Skipping packet: not me.");
                    continue;
                }

                input
                    .decoder
                    .decode(&pkt)
                    .map_err(|e| {
                        track_status = MixStatus::Errored;
                        e
                    })
                    .ok()
            } else {
                // file end.
                println!("No packets left!");
                track_status = MixStatus::Ended;
                None
            }
        };

        // Cleanup: failed to get the next packet, but still have to convert and mix scratch.
        if source_packet.is_none() {
            println!("Cleaning up this file...");
            if buf_in_progress {
                println!("Flushing final buffer.");
                // fill up buf with zeroes, resample, mix
                let resampler = local_state.resampler.as_mut().unwrap();
                let in_len = resample_scratch.frames();
                let to_render = resampler.nbr_frames_needed().saturating_sub(in_len);

                if to_render != 0 {
                    resample_scratch.render_reserved(Some(to_render));
                    for plane in resample_scratch.planes_mut().planes() {
                        for val in &mut plane[in_len..] {
                            *val = 0.0f32;
                        }
                    }
                }

                // Luckily, we make use of the WHOLE input buffer here.
                let resampled = resampler
                    .process(resample_scratch.planes().planes())
                    .unwrap();

                // Calculate true end position using sample rate math
                let ratio = (resampled[0].len() as f32) / (resample_scratch.frames() as f32);
                let out_samples = (ratio * (in_len as f32)).round() as usize;

                // FIXME: actually mix.
                mix_resampled(&resampled, symph_mix, samples_written, volume);

                samples_written += out_samples;
            }

            break;
        }

        let source_packet = source_packet.unwrap();

        let in_rate = source_packet.spec().rate;

        if in_rate != SAMPLE_RATE_RAW as u32 {
            println!(
                "Sample rate mismatch: in {}, need {}",
                in_rate, SAMPLE_RATE_RAW
            );
            // NOTE: this should NEVER change in one stream.
            let chan_c = source_packet.spec().channels.count();
            let resampler = local_state.resampler.get_or_insert_with(|| {
                FftFixedOut::new(
                    in_rate as usize,
                    SAMPLE_RATE_RAW,
                    RESAMPLE_OUTPUT_FRAME_SIZE,
                    4,
                    chan_c,
                )
            });

            let inner_pos = local_state.inner_pos;
            let pkt_frames = source_packet.frames();

            if pkt_frames == 0 {
                continue;
            }

            let needed_in_frames = resampler.nbr_frames_needed();
            let available_frames = pkt_frames - inner_pos;

            let force_copy = buf_in_progress || needed_in_frames > available_frames;
            println!("Frame processing state: chan_c {}, inner_pos {}, pkt_frames {}, needed {}, available {}, force_copy {}.", chan_c, inner_pos, pkt_frames, needed_in_frames, available_frames, force_copy);
            let resampled = if (!force_copy) && matches!(source_packet, AudioBufferRef::F32(_)) {
                // This is the only case where we can pull off a straight resample...
                // I would really like if this could be a slice of slices,
                // but the technology just isn't there yet. And I don't feel like
                // writing unsafe transformations to do so.
                println!("Frame processing: no ***->f32 conv needed.");

                // NOTE: if let needed as if-let && {bool} is nightly only.
                if let AudioBufferRef::F32(s_pkt) = source_packet {
                    let refs: Vec<&[f32]> = s_pkt
                        .planes()
                        .planes()
                        .iter()
                        .map(|s| &s[inner_pos..][..needed_in_frames])
                        .collect();

                    local_state.inner_pos += needed_in_frames;
                    local_state.inner_pos %= pkt_frames;

                    resampler.process(&*refs).unwrap()
                } else {
                    unreachable!()
                }
            } else {
                // We either lack enough samples, or have the wrong data format, forcing
                // a conversion/copy into the buffer.
                // THIS IS (read beyond end or building buf)
                //  copy in to scratch
                //  update inner_pos
                //  if scratch full:
                //   tget = scratch
                //  else:
                //   inner_pos = 0
                //   continue (gets next packet).

                println!("Frame processing: cross-frame boundary and/or wrong format.");

                let old_scratch_len = resample_scratch.frames();
                let missing_frames = needed_in_frames - old_scratch_len;
                let frames_to_take = available_frames.min(missing_frames);

                resample_scratch.render_reserved(Some(frames_to_take));
                copy_into_resampler(
                    &source_packet,
                    resample_scratch,
                    inner_pos,
                    old_scratch_len,
                    frames_to_take,
                );

                local_state.inner_pos += frames_to_take;
                local_state.inner_pos %= pkt_frames;

                if resample_scratch.frames() != needed_in_frames {
                    // Not enough data to fill the resampler: fetch more.
                    buf_in_progress = true;
                    continue;
                } else {
                    let out = resampler
                        .process(resample_scratch.planes().planes())
                        .unwrap();
                    resample_scratch.clear();
                    buf_in_progress = false;
                    out
                }
            };

            let samples_marched = mix_resampled(&resampled, symph_mix, samples_written, volume);

            samples_written += samples_marched;
        } else {
            // No need to resample: mix as standard.
            let samples_marched = mix_over_ref(
                &source_packet,
                symph_mix,
                local_state.inner_pos,
                samples_written,
                volume,
            );

            samples_written += samples_marched;

            local_state.inner_pos += samples_marched;
            local_state.inner_pos %= source_packet.frames();
        }
    }

    println!("mixed! {}", samples_written);

    (MixType::MixedPcm(samples_written * 2), track_status)
}

#[inline]
fn mix_over_ref(
    source: &AudioBufferRef,
    target: &mut AudioBuffer<f32>,
    source_pos: usize,
    dest_pos: usize,
    volume: f32,
) -> usize {
    use AudioBufferRef::*;

    match source {
        U8(v) => mix_symph_buffer(v, target, source_pos, dest_pos, volume),
        U16(v) => mix_symph_buffer(v, target, source_pos, dest_pos, volume),
        U24(v) => mix_symph_buffer(v, target, source_pos, dest_pos, volume),
        U32(v) => mix_symph_buffer(v, target, source_pos, dest_pos, volume),
        S8(v) => mix_symph_buffer(v, target, source_pos, dest_pos, volume),
        S16(v) => mix_symph_buffer(v, target, source_pos, dest_pos, volume),
        S24(v) => mix_symph_buffer(v, target, source_pos, dest_pos, volume),
        S32(v) => mix_symph_buffer(v, target, source_pos, dest_pos, volume),
        F32(v) => mix_symph_buffer(v, target, source_pos, dest_pos, volume),
        F64(v) => mix_symph_buffer(v, target, source_pos, dest_pos, volume),
    }
}

#[inline]
fn mix_symph_buffer<S>(
    source: &AudioBuffer<S>,
    target: &mut AudioBuffer<f32>,
    source_pos: usize,
    dest_pos: usize,
    volume: f32,
) -> usize
where
    S: Sample + IntoSample<f32>,
{
    // mix in source_packet[inner_pos..] til end of EITHER buffer.
    // target.planes_mut()
    let src_usable = source.frames() - source_pos;
    let tgt_usable = target.frames() - dest_pos;

    let mix_ct = src_usable.min(tgt_usable);

    // FIXME: downmix if target is mono.
    for (d_plane, s_plane) in (&mut target.planes_mut().planes()[..])
        .iter_mut()
        .zip(source.planes().planes()[..].iter())
    {
        for (d, s) in d_plane[dest_pos..dest_pos + mix_ct]
            .iter_mut()
            .zip(s_plane[source_pos..source_pos + mix_ct].iter())
        {
            *d += volume * (*s).into_sample();
        }
    }

    mix_ct
}

#[inline]
fn mix_resampled(
    source: &Vec<Vec<f32>>,
    target: &mut AudioBuffer<f32>,
    dest_pos: usize,
    volume: f32,
) -> usize {
    let mix_ct = source[0].len();
    println!("Mixing {} samples into pos starting {}.", mix_ct, dest_pos);

    // FIXME: downmix if target is mono.
    for (d_plane, s_plane) in (&mut target.planes_mut().planes()[..])
        .iter_mut()
        .zip(source[..].iter())
    {
        for (d, s) in d_plane[dest_pos..dest_pos + mix_ct].iter_mut().zip(s_plane) {
            *d += volume * (*s);
        }
    }

    mix_ct
}

#[inline]
fn copy_into_resampler(
    source: &AudioBufferRef,
    target: &mut AudioBuffer<f32>,
    source_pos: usize,
    dest_pos: usize,
    len: usize,
) -> usize {
    use AudioBufferRef::*;

    match source {
        U8(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        U16(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        U24(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        U32(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        S8(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        S16(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        S24(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        S32(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        F32(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
        F64(v) => copy_symph_buffer(v, target, source_pos, dest_pos, len),
    }
}

#[inline]
fn copy_symph_buffer<S>(
    source: &AudioBuffer<S>,
    target: &mut AudioBuffer<f32>,
    source_pos: usize,
    dest_pos: usize,
    len: usize,
) -> usize
where
    S: Sample + IntoSample<f32>,
{
    for (d_plane, s_plane) in (&mut target.planes_mut().planes()[..])
        .iter_mut()
        .zip(source.planes().planes()[..].iter())
    {
        for (d, s) in d_plane[dest_pos..dest_pos + len]
            .iter_mut()
            .zip(s_plane[source_pos..source_pos + len].iter())
        {
            *d = (*s).into_sample();
        }
    }

    len
}

#[inline]
fn mix_tracks<'a>(
    opus_frame: &'a mut [u8],
    symph_mix: &mut AudioBuffer<f32>,
    symph_scratch: &mut AudioBuffer<f32>,
    tracks: &mut Vec<Track>,
    full_inputs: &mut Vec<InputState>,
    input_states: &mut Vec<LocalInput>,
    interconnect: &Interconnect,
    creator: &Sender<InputCreateMessage>,
    parser: &Sender<InputParseMessage>,
    prevent_events: bool,
) -> MixType {
    let mut len = 0;

    // Opus frame passthrough.
    // This requires that we have only one track, who has volume 1.0, and an
    // Opus codec type.
    let do_passthrough = tracks.len() == 1 && {
        let track = &tracks[0];
        (track.volume - 1.0).abs() < f32::EPSILON // && track.source.supports_passthrough()
    };

    println!(
        "lens {} {} {}",
        tracks.len(),
        full_inputs.len(),
        input_states.len()
    );

    for (((i, track), input), local_state) in tracks
        .iter_mut()
        .enumerate()
        .zip(full_inputs.iter_mut())
        .zip(input_states.iter_mut())
    {
        let vol = track.volume;
        // let stream = &mut track.source;

        if track.playing != PlayMode::Play {
            continue;
        }

        let input = match get_or_ready_input(input, creator, parser) {
            Ok(i) => i,
            Err(InputReadyingError::Waiting) => continue,
            // TODO: allow for retry in given time.
            Err(_) => {
                track.end();
                continue;
            },
        };

        // let (temp_len, opus_len) = if do_passthrough {
        //     (0, track.source.read_opus_frame(opus_frame).ok())
        // } else {
        //     (stream.mix(mix_buffer, vol), None)
        // };

        let (mix_type, status) = mix_symph_indiv(symph_mix, symph_scratch, input, local_state, vol);

        // FIXME: allow Ended to trigger a seek/loop/revisit in the same mix cycle?
        // This is a straight port of old logic, maybe we could combine with MixStatus::Ended.
        if mix_type.contains_audio() {
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

        match mix_type {
            MixType::MixedPcm(pcm_len) => {
                len = len.max(pcm_len);
            },
            a => return a,
        }
    }

    MixType::MixedPcm(len)
}

/// Readies the requested input state.
///
/// Returns the usable version of the audio if available, and whether the track should be deleted.
fn get_or_ready_input<'a>(
    input: &'a mut InputState,
    creator: &Sender<InputCreateMessage>,
    parser: &Sender<InputParseMessage>,
) -> StdResult<&'a mut Parsed, InputReadyingError> {
    use InputReadyingError::*;

    match input {
        InputState::NotReady(r) => {
            let (tx, rx) = flume::bounded(1);

            let mut state = InputState::Preparing(PreparingInfo {
                time: Instant::now(),
                callback: rx,
            });

            std::mem::swap(&mut state, input);

            match state {
                InputState::NotReady(a @ SymphInput::Lazy(_)) => {
                    let _ = creator.send(InputCreateMessage::Create(tx, a));
                },
                InputState::NotReady(SymphInput::Live(audio, rec)) => {
                    let _ = parser.send(InputParseMessage::Promote(tx, audio, rec));
                },
                _ => unreachable!(),
            }

            Err(Waiting)
        },
        InputState::Preparing(info) => {
            // Check this with a RefCell?

            match info.callback.try_recv() {
                Ok(MixerInputResultMessage::InputBuilt(parsed, rec)) => {
                    *input = InputState::Ready(parsed, rec);

                    if let InputState::Ready(ref mut parsed, _) = input {
                        Ok(parsed)
                    } else {
                        unreachable!()
                    }
                },
                Err(TryRecvError::Empty) => Err(Waiting),
                Ok(MixerInputResultMessage::InputCreateErr(e)) => Err(Creation(e)),
                Ok(MixerInputResultMessage::InputParseErr(e)) => Err(Parsing(e)),
                Err(TryRecvError::Disconnected) => Err(Dropped),
            }
        },
        InputState::Ready(parsed, _) => Ok(parsed),
    }
}

enum InputReadyingError {
    Parsing(SymphoniaError),
    Creation(AudioStreamError),
    Dropped,
    Waiting,
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
    let _ = mixer.creator.send(InputCreateMessage::Poison);
    let _ = mixer.parser.send(InputParseMessage::Poison);
}
