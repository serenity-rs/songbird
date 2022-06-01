mod pool;
mod result;
mod util;

use pool::*;
use result::*;

use super::{disposal, error::Result, message::*};
use crate::{
    constants::*,
    driver::MixMode,
    events::EventStore,
    input::{Compose, Input, LiveInput, Metadata, Parsed},
    tracks::{
        Action,
        LoopState,
        PlayMode,
        ReadyState,
        TrackCommand,
        TrackHandle,
        TrackState,
        View,
    },
    Config,
};
use audiopus::{
    coder::Encoder as OpusEncoder,
    softclip::SoftClip,
    Application as CodingMode,
    Bitrate,
};
use discortp::{
    rtp::{MutableRtpPacket, RtpPacket},
    MutablePacket,
};
use flume::{Receiver, Sender, TryRecvError};
use rand::random;
use rubato::{FftFixedOut, Resampler};
use std::{
    io::Write,
    result::Result as StdResult,
    sync::Arc,
    time::{Duration, Instant},
};
use symphonia_core::{
    audio::{AudioBuffer, AudioBufferRef, Layout, SampleBuffer, Signal, SignalSpec},
    codecs::CODEC_TYPE_OPUS,
    conv::IntoSample,
    formats::SeekTo,
    sample::Sample,
    units::Time,
};
use tokio::runtime::Handle;
use tracing::{debug, error, instrument, warn};
use xsalsa20poly1305::TAG_SIZE;

pub struct Mixer {
    pub bitrate: Bitrate,
    pub config: Arc<Config>,
    pub conn_active: Option<MixerConnection>,
    pub content_prep_sequence: u64,
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
    thread_pool: BlockyTaskPool,
    pub ws: Option<Sender<WsMessage>>,

    pub tracks: Vec<InternalTrack>,
    track_handles: Vec<TrackHandle>,

    sample_buffer: SampleBuffer<f32>,
    symph_mix: AudioBuffer<f32>,
    resample_scratch: AudioBuffer<f32>,
}

fn new_encoder(bitrate: Bitrate, mix_mode: MixMode) -> Result<OpusEncoder> {
    let mut encoder = OpusEncoder::new(SAMPLE_RATE, mix_mode.to_opus(), CodingMode::Audio)?;
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
        let encoder = new_encoder(bitrate, config.mix_mode)
            .expect("Failed to create encoder in mixing thread with known-good values.");
        let soft_clip = SoftClip::new(config.mix_mode.to_opus());

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
        let track_handles = Vec::with_capacity(1.max(config.preallocated_tracks));

        // Create an object disposal thread here.
        let (disposer, disposal_rx) = flume::unbounded();
        std::thread::spawn(move || disposal::runner(disposal_rx));

        let thread_pool = BlockyTaskPool::new(async_handle);

        let symph_layout = config.mix_mode.symph_layout();

        let config = config.into();

        let sample_buffer = SampleBuffer::<f32>::new(
            MONO_FRAME_SIZE as u64,
            symphonia_core::audio::SignalSpec::new_with_layout(
                SAMPLE_RATE_RAW as u32,
                symph_layout,
            ),
        );
        let symph_mix = AudioBuffer::<f32>::new(
            MONO_FRAME_SIZE as u64,
            symphonia_core::audio::SignalSpec::new_with_layout(
                SAMPLE_RATE_RAW as u32,
                symph_layout,
            ),
        );
        let resample_scratch = AudioBuffer::<f32>::new(
            MONO_FRAME_SIZE as u64,
            SignalSpec::new_with_layout(SAMPLE_RATE_RAW as u32, Layout::Stereo),
        );

        Self {
            bitrate,
            config,
            conn_active: None,
            content_prep_sequence: 0,
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
            thread_pool,
            ws: None,

            tracks,
            track_handles,

            sample_buffer,
            symph_mix,
            resample_scratch,
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
            AddTrack(t) => self.add_track(t),
            SetTrack(t) => {
                self.tracks.clear();

                let mut out = self.fire_event(EventMessage::RemoveAllTracks);

                if let Some(t) = t {
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
                if new_config.mix_mode != self.config.mix_mode {
                    self.soft_clip = SoftClip::new(new_config.mix_mode.to_opus());
                    if let Ok(enc) = new_encoder(self.bitrate, new_config.mix_mode) {
                        self.encoder = enc;
                    } else {
                        self.bitrate = DEFAULT_BITRATE;
                        self.encoder = new_encoder(self.bitrate, new_config.mix_mode)
                            .expect("Failed fallback rebuild of OpusEncoder with safe inputs.");
                    }

                    let sl = new_config.mix_mode.symph_layout();
                    self.sample_buffer = SampleBuffer::<f32>::new(
                        MONO_FRAME_SIZE as u64,
                        SignalSpec::new_with_layout(SAMPLE_RATE_RAW as u32, sl),
                    );
                    self.symph_mix = AudioBuffer::<f32>::new(
                        MONO_FRAME_SIZE as u64,
                        SignalSpec::new_with_layout(SAMPLE_RATE_RAW as u32, sl),
                    );
                }

                self.config = Arc::new(new_config.clone());

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
            RebuildEncoder => match new_encoder(self.bitrate, self.config.mix_mode) {
                Ok(encoder) => {
                    self.encoder = encoder;
                    Ok(())
                },
                Err(e) => {
                    error!("Failed to rebuild encoder. Resetting bitrate. {:?}", e);
                    self.bitrate = DEFAULT_BITRATE;
                    self.encoder = new_encoder(self.bitrate, self.config.mix_mode)
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
    pub fn add_track(&mut self, track: TrackContext) -> Result<()> {
        let (track, evts, state, handle) = InternalTrack::decompose_track(track);
        self.tracks.push(track);
        self.track_handles.push(handle.clone());
        self.interconnect
            .events
            .send(EventMessage::AddTrack(evts, state, handle))?;

        Ok(())
    }

    // rebuilds the event thread's view of each track, in event of a full rebuild.
    #[inline]
    fn rebuild_tracks(&mut self) -> Result<()> {
        for (track, handle) in self.tracks.iter().zip(self.track_handles.iter()) {
            let evts = Default::default();
            let state = track.state();
            let handle = handle.clone();

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
            let action = track.process_commands(i, &self.interconnect);

            if let Some(time) = action.seek_point {
                let full_input = &mut track.input;
                let time = Time::from(time.as_secs_f64());
                let mut ts = SeekTo::Time {
                    time,
                    track_id: None,
                };
                let (tx, rx) = flume::bounded(1);

                let queued_seek = if matches!(full_input, InputState::Preparing(_)) {
                    Some(util::copy_seek_to(&ts))
                } else {
                    None
                };

                let mut new_state = InputState::Preparing(PreparingInfo {
                    time: Instant::now(),
                    callback: rx,
                    queued_seek,
                });

                std::mem::swap(full_input, &mut new_state);

                match new_state {
                    InputState::Ready(p, r) => {
                        if let SeekTo::Time { time: _, track_id } = &mut ts {
                            *track_id = Some(p.track_id);
                        }

                        self.thread_pool
                            .seek(tx, p, r, ts, true, self.config.clone());
                    },
                    InputState::Preparing(old_prep) => {
                        // Annoying case: we need to mem_swap for the other two cases,
                        // but here we don't want to.
                        // new_state contains the old request now, so we want to move its
                        // callback and time *back* into self.full_inputs[i].
                        if let InputState::Preparing(new_prep) = full_input {
                            new_prep.callback = old_prep.callback;
                            new_prep.time = old_prep.time;
                        } else {
                            unreachable!()
                        }
                    },
                    InputState::NotReady(lazy) =>
                        self.thread_pool
                            .create(tx, lazy, Some(ts), self.config.clone()),
                }
            }

            if action.make_playable {
                let _ = get_or_ready_input(
                    track,
                    i,
                    &self.interconnect,
                    &self.thread_pool,
                    &self.config,
                    self.prevent_events,
                );
            }
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
                let p_state = track.playing;
                let to_drop = self.tracks.swap_remove(i);
                let _ = self
                    .disposer
                    .send(DisposalMessage::Track(Box::new(to_drop)));
                let to_drop = self.track_handles.swap_remove(i);
                let _ = self.disposer.send(DisposalMessage::Handle(to_drop));

                to_remove.push(i);
                self.fire_event(EventMessage::ChangeState(
                    i,
                    TrackStateChange::Mode(p_state),
                ))?;
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

        self.symph_mix.clear();
        self.symph_mix.render_reserved(Some(MONO_FRAME_SIZE));
        self.resample_scratch.clear();

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
                &mut self.symph_mix,
                &mut self.resample_scratch,
                &mut self.tracks,
                &mut self.track_handles,
                &self.interconnect,
                &self.thread_pool,
                &self.config,
                self.prevent_events,
            );

            self.sample_buffer.copy_interleaved_typed(&self.symph_mix);

            out
        };

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

            if let MixType::MixedPcm(n) = mix_len {
                // to apply soft_clip, we need this to be in a normal f32 buffer.
                // unfortunately, SampleBuffer does not expose a `.samples_mut()`.
                // hence, an extra copy...
                let samples_to_copy = self.config.mix_mode.channels() * n;

                (&mut mix_buffer[..samples_to_copy])
                    .copy_from_slice(&self.sample_buffer.samples()[..samples_to_copy]);

                self.soft_clip.apply(
                    (&mut mix_buffer[..])
                        .try_into()
                        .expect("Mix buffer is known to have a valid sample count (softclip)."),
                )?;
            }
        }

        if let Some(ws) = &self.ws {
            ws.send(WsMessage::Speaking(true))?;
        }

        self.march_deadline();
        self.prep_and_send_packet(&mix_buffer, mix_len)?;

        if matches!(mix_len, MixType::MixedPcm(a) if a > 0) {
            for plane in self.symph_mix.planes_mut().planes() {
                plane.fill(0.0);
            }
        }

        Ok(())
    }

    fn set_bitrate(&mut self, bitrate: Bitrate) -> Result<()> {
        self.encoder.set_bitrate(bitrate).map_err(Into::into)
    }

    #[inline]
    fn prep_and_send_packet(&mut self, buffer: &[f32; 1920], mix_len: MixType) -> Result<()> {
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
                        &buffer[..self.config.mix_mode.sample_count_in_frame()],
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

pub enum InputState {
    NotReady(Input),
    Preparing(PreparingInfo),
    Ready(Parsed, Option<Box<dyn Compose>>),
}

impl InputState {
    fn metadata(&mut self) -> Option<Metadata> {
        if let Self::Ready(parsed, _) = self {
            Some(parsed.into())
        } else {
            None
        }
    }
}

impl From<Input> for InputState {
    fn from(val: Input) -> Self {
        match val {
            a @ Input::Lazy(_) => InputState::NotReady(a),
            Input::Live(live, rec) => match live {
                LiveInput::Parsed(p) => InputState::Ready(p, rec),
                other => InputState::NotReady(Input::Live(other, rec)),
            },
        }
    }
}

impl From<&InputState> for ReadyState {
    fn from(val: &InputState) -> Self {
        use InputState::*;

        match val {
            NotReady(_) => Self::Uninitialised,
            Preparing(_) => Self::Preparing,
            Ready(_, _) => Self::Playable,
        }
    }
}

pub struct PreparingInfo {
    #[allow(dead_code)]
    time: Instant,
    queued_seek: Option<SeekTo>,
    callback: Receiver<MixerInputResultMessage>,
}

pub struct MixState {
    inner_pos: usize,
    resampler: Option<(usize, FftFixedOut<f32>, Vec<Vec<f32>>)>,
    passthrough: Passthrough,
}

impl MixState {
    fn reset(&mut self) {
        self.inner_pos = 0;
        self.resampler = None;
    }
}

impl Default for MixState {
    fn default() -> Self {
        Self {
            inner_pos: 0,
            resampler: None,
            passthrough: Passthrough::Inactive,
        }
    }
}

#[inline]
pub fn mix_symph_indiv(
    symph_mix: &mut AudioBuffer<f32>,
    resample_scratch: &mut AudioBuffer<f32>,
    input: &mut Parsed,
    local_state: &mut MixState,
    volume: f32,
    mut opus_slot: Option<&mut [u8]>,
) -> (MixType, MixStatus) {
    let mut samples_written = 0;
    let mut buf_in_progress = false;
    let mut track_status = MixStatus::Live;
    let codec_type = input.decoder.codec_params().codec;

    resample_scratch.clear();

    while samples_written != MONO_FRAME_SIZE {
        let source_packet = if local_state.inner_pos != 0 {
            Some(input.decoder.last_decoded())
        } else if let Ok(pkt) = input.format.next_packet() {
            if pkt.track_id() != input.track_id {
                continue;
            }

            let buf = pkt.buf();

            // Opus packet passthrough special case.
            if codec_type == CODEC_TYPE_OPUS && local_state.passthrough != Passthrough::Block {
                if let Some(slot) = opus_slot.as_mut() {
                    let sample_ct = buf
                        .try_into()
                        .and_then(|buf| audiopus::packet::nb_samples(buf, SAMPLE_RATE));

                    match sample_ct {
                        Ok(MONO_FRAME_SIZE) if buf.len() <= slot.len() => {
                            slot.write_all(buf).expect(
                                "Bounds check performed, and failure will block passthrough.",
                            );

                            return (MixType::Passthrough(buf.len()), MixStatus::Live);
                        },
                        _ => {
                            local_state.passthrough = Passthrough::Block;
                        },
                    }
                }
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
            track_status = MixStatus::Ended;
            None
        };

        // Cleanup: failed to get the next packet, but still have to convert and mix scratch.
        if source_packet.is_none() {
            if buf_in_progress {
                // fill up buf with zeroes, resample, mix
                let (chan_c, resampler, rs_out_buf) = local_state.resampler.as_mut().unwrap();
                let in_len = resample_scratch.frames();
                let to_render = resampler.input_frames_next().saturating_sub(in_len);

                if to_render != 0 {
                    resample_scratch.render_reserved(Some(to_render));
                    for plane in resample_scratch.planes_mut().planes() {
                        for val in &mut plane[in_len..] {
                            *val = 0.0f32;
                        }
                    }
                }

                // Luckily, we make use of the WHOLE input buffer here.
                resampler
                    .process_into_buffer(
                        &resample_scratch.planes().planes()[..*chan_c],
                        rs_out_buf,
                        None,
                    )
                    .unwrap();

                // Calculate true end position using sample rate math
                let ratio = (rs_out_buf[0].len() as f32) / (resample_scratch.frames() as f32);
                let out_samples = (ratio * (in_len as f32)).round() as usize;

                mix_resampled(rs_out_buf, symph_mix, samples_written, volume);

                samples_written += out_samples;
            }

            break;
        }

        let source_packet = source_packet.unwrap();

        let in_rate = source_packet.spec().rate;

        if in_rate != SAMPLE_RATE_RAW as u32 {
            // NOTE: this should NEVER change in one stream.
            let chan_c = source_packet.spec().channels.count();
            let (_, resampler, rs_out_buf) = local_state.resampler.get_or_insert_with(|| {
                // TODO: integ. error handling here.
                let resampler = FftFixedOut::new(
                    in_rate as usize,
                    SAMPLE_RATE_RAW,
                    RESAMPLE_OUTPUT_FRAME_SIZE,
                    4,
                    chan_c,
                )
                .expect("Failed to create resampler.");
                let out_buf = resampler.output_buffer_allocate();

                (chan_c, resampler, out_buf)
            });

            let inner_pos = local_state.inner_pos;
            let pkt_frames = source_packet.frames();

            if pkt_frames == 0 {
                continue;
            }

            let needed_in_frames = resampler.input_frames_next();
            let available_frames = pkt_frames - inner_pos;

            let force_copy = buf_in_progress || needed_in_frames > available_frames;
            // println!("Frame processing state: chan_c {}, inner_pos {}, pkt_frames {}, needed {}, available {}, force_copy {}.", chan_c, inner_pos, pkt_frames, needed_in_frames, available_frames, force_copy);
            if (!force_copy) && matches!(source_packet, AudioBufferRef::F32(_)) {
                // This is the only case where we can pull off a straight resample...
                // I would really like if this could be a slice of slices,
                // but the technology just isn't there yet. And I don't feel like
                // writing unsafe transformations to do so.

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

                    resampler
                        .process_into_buffer(&*refs, rs_out_buf, None)
                        .unwrap()
                } else {
                    unreachable!()
                }
            } else {
                // We either lack enough samples, or have the wrong data format, forcing
                // a conversion/copy into the buffer.
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
                    resampler
                        .process_into_buffer(
                            &resample_scratch.planes().planes()[..chan_c],
                            rs_out_buf,
                            None,
                        )
                        .unwrap();
                    resample_scratch.clear();
                    buf_in_progress = false;
                }
            };

            let samples_marched = mix_resampled(rs_out_buf, symph_mix, samples_written, volume);

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

    (MixType::MixedPcm(samples_written), track_status)
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
    let src_usable = source.frames() - source_pos;
    let tgt_usable = target.frames() - dest_pos;

    let mix_ct = src_usable.min(tgt_usable);

    let target_chans = target.spec().channels.count();
    let target_mono = target_chans == 1;
    let source_chans = source.spec().channels.count();
    let source_mono = source_chans == 1;

    let source_planes = source.planes();
    let source_raw_planes = source_planes.planes();

    if source_mono {
        let source_plane = source_raw_planes[0];
        for d_plane in (&mut *target.planes_mut().planes()).iter_mut() {
            for (d, s) in d_plane[dest_pos..dest_pos + mix_ct]
                .iter_mut()
                .zip(source_plane[source_pos..source_pos + mix_ct].iter())
            {
                *d += volume * (*s).into_sample();
            }
        }
    } else if target_mono {
        let vol_adj = 1.0 / (source_chans as f32);
        let mut t_planes = target.planes_mut();
        let d_plane = &mut t_planes.planes()[0];
        for s_plane in source_raw_planes[..].iter() {
            for (d, s) in d_plane[dest_pos..dest_pos + mix_ct]
                .iter_mut()
                .zip(s_plane[source_pos..source_pos + mix_ct].iter())
            {
                *d += volume * vol_adj * (*s).into_sample();
            }
        }
    } else {
        for (d_plane, s_plane) in (&mut *target.planes_mut().planes())
            .iter_mut()
            .zip(source_raw_planes[..].iter())
        {
            for (d, s) in d_plane[dest_pos..dest_pos + mix_ct]
                .iter_mut()
                .zip(s_plane[source_pos..source_pos + mix_ct].iter())
            {
                *d += volume * (*s).into_sample();
            }
        }
    }

    mix_ct
}

#[inline]
fn mix_resampled(
    source: &[Vec<f32>],
    target: &mut AudioBuffer<f32>,
    dest_pos: usize,
    volume: f32,
) -> usize {
    let mix_ct = source[0].len();

    let target_chans = target.spec().channels.count();
    let target_mono = target_chans == 1;
    let source_chans = source.len();
    let source_mono = source_chans == 1;

    if source_mono {
        let source_plane = &source[0];
        for d_plane in (&mut *target.planes_mut().planes()).iter_mut() {
            for (d, s) in d_plane[dest_pos..dest_pos + mix_ct]
                .iter_mut()
                .zip(source_plane)
            {
                *d += volume * s;
            }
        }
    } else if target_mono {
        let vol_adj = 1.0 / (source_chans as f32);
        let mut t_planes = target.planes_mut();
        let d_plane = &mut t_planes.planes()[0];
        for s_plane in source[..].iter() {
            for (d, s) in d_plane[dest_pos..dest_pos + mix_ct].iter_mut().zip(s_plane) {
                *d += volume * vol_adj * s;
            }
        }
    } else {
        for (d_plane, s_plane) in (&mut *target.planes_mut().planes())
            .iter_mut()
            .zip(source[..].iter())
        {
            for (d, s) in d_plane[dest_pos..dest_pos + mix_ct].iter_mut().zip(s_plane) {
                *d += volume * (*s);
            }
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
    for (d_plane, s_plane) in (&mut *target.planes_mut().planes())
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

// TODO: make on &mut self?
#[inline]
#[allow(clippy::too_many_arguments)]
fn mix_tracks<'a>(
    opus_frame: &'a mut [u8],
    symph_mix: &mut AudioBuffer<f32>,
    symph_scratch: &mut AudioBuffer<f32>,
    tracks: &mut Vec<InternalTrack>,
    handles: &mut [TrackHandle],
    interconnect: &Interconnect,
    thread_pool: &BlockyTaskPool,
    config: &Arc<Config>,
    prevent_events: bool,
) -> MixType {
    let mut len = 0;

    // Opus frame passthrough.
    // This requires that we have only one track, who has volume 1.0, and an
    // Opus codec type (verified internally).
    let do_passthrough = tracks.len() == 1 && {
        let track = &tracks[0];
        (track.volume - 1.0).abs() < f32::EPSILON
    };

    for (i, track) in tracks.iter_mut().enumerate() {
        let vol = track.volume;

        if track.playing != PlayMode::Play {
            continue;
        }

        let input = get_or_ready_input(track, i, interconnect, thread_pool, config, prevent_events);

        let (input, mix_state) = match input {
            Ok(i) => i,
            Err(InputReadyingError::Waiting) => continue,
            // TODO: allow for retry in given time.
            Err(_e) => {
                track.end();
                continue;
            },
        };

        let opus_slot = if do_passthrough {
            Some(&mut *opus_frame)
        } else {
            None
        };

        let (mix_type, _status) =
            mix_symph_indiv(symph_mix, symph_scratch, input, mix_state, vol, opus_slot);

        let return_here = match mix_type {
            MixType::MixedPcm(pcm_len) => {
                len = len.max(pcm_len);
                false
            },
            _ => {
                if mix_state.passthrough == Passthrough::Inactive {
                    input.decoder.reset();
                }
                mix_state.passthrough = Passthrough::Active;
                true
            },
        };

        // FIXME: allow Ended to trigger a seek/loop/revisit in the same mix cycle?
        // This is a straight port of old logic, maybe we could combine with MixStatus::Ended.
        if mix_type.contains_audio() {
            track.step_frame();
        } else if track.do_loop() {
            let _ = handles[i].seek_time(Default::default());
            if !prevent_events {
                // position update is sent out later, when the seek concludes.
                let _ = interconnect.events.send(EventMessage::ChangeState(
                    i,
                    TrackStateChange::Loops(track.loops, false),
                ));
            }
        } else {
            track.end();
        }

        // This needs to happen here due to borrow checker shenanigans.
        if return_here {
            return mix_type;
        }
    }

    MixType::MixedPcm(len)
}

// TODO: make on &mut self?
/// Readies the requested input state.
///
/// Returns the usable version of the audio if available, and whether the track should be deleted.
#[allow(clippy::too_many_arguments)]
fn get_or_ready_input<'a>(
    track: &'a mut InternalTrack,
    id: usize,
    interconnect: &Interconnect,
    pool: &BlockyTaskPool,
    config: &Arc<Config>,
    prevent_events: bool,
) -> StdResult<(&'a mut Parsed, &'a mut MixState), InputReadyingError> {
    use InputReadyingError::*;

    let input = &mut track.input;
    let local = &mut track.mix_state;

    match input {
        InputState::NotReady(_) => {
            let (tx, rx) = flume::bounded(1);

            let mut state = InputState::Preparing(PreparingInfo {
                time: Instant::now(),
                queued_seek: None,
                callback: rx,
            });

            std::mem::swap(&mut state, input);

            match state {
                InputState::NotReady(a @ Input::Lazy(_)) => {
                    pool.create(tx, a, None, config.clone());
                },
                InputState::NotReady(Input::Live(audio, rec)) => {
                    pool.parse(config.clone(), tx, audio, rec, None);
                },
                _ => unreachable!(),
            }

            Err(Waiting)
        },
        InputState::Preparing(info) => {
            let queued_seek = info.queued_seek.take();

            let orig_out = match info.callback.try_recv() {
                Ok(MixerInputResultMessage::Built(parsed, rec)) => {
                    *input = InputState::Ready(parsed, rec);
                    local.reset();

                    if let InputState::Ready(ref mut parsed, _) = input {
                        Ok(parsed)
                    } else {
                        unreachable!()
                    }
                },
                Ok(MixerInputResultMessage::Seek(parsed, rec, seek_res)) => {
                    match seek_res {
                        Ok(pos) => {
                            let time_base =
                                if let Some(tb) = parsed.decoder.codec_params().time_base {
                                    tb
                                } else {
                                    // Probably fire an Unsupported.
                                    todo!()
                                };
                            // modify track.
                            let new_time = time_base.calc_time(pos.actual_ts);
                            let time_in_float = new_time.seconds as f64 + new_time.frac;
                            track.position = std::time::Duration::from_secs_f64(time_in_float);

                            if !prevent_events {
                                let _ = interconnect.events.send(EventMessage::ChangeState(
                                    id,
                                    TrackStateChange::Position(track.position),
                                ));
                            }

                            local.reset();
                            *input = InputState::Ready(parsed, rec);

                            if let InputState::Ready(ref mut parsed, _) = input {
                                Ok(parsed)
                            } else {
                                unreachable!()
                            }
                        },
                        Err(e) => Err(Seeking(e)),
                    }
                },
                Err(TryRecvError::Empty) => Err(Waiting),
                Ok(MixerInputResultMessage::CreateErr(e)) => Err(Creation(e)),
                Ok(MixerInputResultMessage::ParseErr(e)) => Err(Parsing(e)),
                Err(TryRecvError::Disconnected) => Err(Dropped),
            };

            let orig_out = orig_out.map(|a| (a, &mut track.mix_state));

            match (orig_out, queued_seek) {
                (Ok(v), Some(_time)) => {
                    warn!("Track was given seek command while busy: handling not impl'd yet.");
                    Ok(v)
                },
                (a, _) => a,
            }
        },
        InputState::Ready(parsed, _) => Ok((parsed, &mut track.mix_state)),
    }
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

/// Simple state to manage decoder resets etc.
///
/// Inactive->Active transitions should trigger a reset.
///
/// Block should be used if a source contains known-bad packets:
/// it's unlikely that packet sizes will vary, but if they do then
/// we can't passthrough (and every attempt will trigger a codec reset,
/// which probably won't sound too smooth).
#[derive(Clone, Copy, Eq, PartialEq)]
enum Passthrough {
    Active,
    Inactive,
    Block,
}

pub struct InternalTrack {
    playing: PlayMode,
    volume: f32,
    input: InputState,
    mix_state: MixState,
    position: Duration,
    play_time: Duration,
    commands: Receiver<TrackCommand>,
    loops: LoopState,
}

impl<'a> InternalTrack {
    fn decompose_track(val: TrackContext) -> (Self, EventStore, TrackState, TrackHandle) {
        let TrackContext {
            handle,
            track,
            receiver,
        } = val;
        let out = InternalTrack {
            playing: track.playing,
            volume: track.volume,
            input: InputState::from(track.input),
            mix_state: Default::default(),
            position: Default::default(),
            play_time: Default::default(),
            commands: receiver,
            loops: track.loops,
        };

        let state = out.state();

        (out, track.events, state, handle)
    }

    fn state(&self) -> TrackState {
        TrackState {
            playing: self.playing,
            volume: self.volume,
            position: self.position,
            play_time: self.play_time,
            loops: self.loops,
        }
    }

    fn view(&'a mut self) -> View<'a> {
        let ready = (&self.input).into();

        View {
            position: &self.position,
            play_time: &self.play_time,
            volume: &mut self.volume,
            meta: self.input.metadata(),
            ready,
            playing: &mut self.playing,
            loops: &mut self.loops,
        }
    }

    fn process_commands(&mut self, index: usize, ic: &Interconnect) -> Action {
        // Note: disconnection and an empty channel are both valid,
        // and should allow the audio object to keep running as intended.

        // We also need to export a target seek point to the mixer, if known.
        let mut action = Action::default();

        // Note that interconnect failures are not currently errors.
        // In correct operation, the event thread should never panic,
        // but it receiving status updates is secondary do actually
        // doing the work.
        loop {
            match self.commands.try_recv() {
                Ok(cmd) => {
                    use TrackCommand::*;
                    match cmd {
                        Play => {
                            self.playing.change_to(PlayMode::Play);
                            let _ = ic.events.send(EventMessage::ChangeState(
                                index,
                                TrackStateChange::Mode(self.playing),
                            ));
                        },
                        Pause => {
                            self.playing.change_to(PlayMode::Pause);
                            let _ = ic.events.send(EventMessage::ChangeState(
                                index,
                                TrackStateChange::Mode(self.playing),
                            ));
                        },
                        Stop => {
                            self.playing.change_to(PlayMode::Stop);
                            let _ = ic.events.send(EventMessage::ChangeState(
                                index,
                                TrackStateChange::Mode(self.playing),
                            ));
                        },
                        Volume(vol) => {
                            self.volume = vol;
                            let _ = ic.events.send(EventMessage::ChangeState(
                                index,
                                TrackStateChange::Volume(self.volume),
                            ));
                        },
                        Seek(time) => action.seek_point = Some(time),
                        AddEvent(evt) => {
                            let _ = ic.events.send(EventMessage::AddTrackEvent(index, evt));
                        },
                        Do(func) => {
                            if let Some(indiv_action) = func(self.view()) {
                                action.combine(indiv_action);
                            }

                            let _ = ic.events.send(EventMessage::ChangeState(
                                index,
                                TrackStateChange::Total(self.state()),
                            ));
                        },
                        Request(tx) => {
                            let _ = tx.send(self.state());
                        },
                        Loop(loops) => {
                            self.loops = loops;
                            let _ = ic.events.send(EventMessage::ChangeState(
                                index,
                                TrackStateChange::Loops(self.loops, true),
                            ));
                        },
                        MakePlayable => action.make_playable = true,
                    }
                },
                Err(TryRecvError::Disconnected) => {
                    // this branch will never be visited.
                    break;
                },
                Err(TryRecvError::Empty) => {
                    break;
                },
            }
        }

        action
    }

    pub(crate) fn do_loop(&mut self) -> bool {
        match self.loops {
            LoopState::Infinite => true,
            LoopState::Finite(0) => false,
            LoopState::Finite(ref mut n) => {
                *n -= 1;
                true
            },
        }
    }

    /// Steps playback location forward by one frame.
    pub(crate) fn step_frame(&mut self) {
        self.position += TIMESTEP_LENGTH;
        self.play_time += TIMESTEP_LENGTH;
    }

    pub(crate) fn end(&mut self) -> &mut Self {
        self.playing.change_to(PlayMode::End);

        self
    }
}
