use super::{disposal, error::Result, message::*};
use crate::{
    constants::*,
    input::Parsed,
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
use std::{borrow::Borrow, collections::HashMap, time::Instant};
use symphonia_core::{
    audio::{AudioBuffer, AudioBufferRef, Layout, SampleBuffer, Signal},
    conv::IntoSample,
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
    pub sleeper: SpinSleeper,
    pub soft_clip: SoftClip,
    pub tracks: Vec<Track>,
    pub ws: Option<Sender<WsMessage>>,

    pub symph_formats: Vec<Box<dyn symphonia_core::formats::FormatReader>>,
    pub symph_decoders: Vec<HashMap<u32, Box<dyn symphonia_core::codecs::Decoder>>>,
    pub symph_resamplers: Vec<HashMap<u32, rubato::FftFixedOut<f32>>>,
    pub symph_inner_frame_pos: Vec<HashMap<u32, usize>>,
    // Should the above hashmaps be stuck together?
    // Do we just make the simplifying assumption and use only the default track?
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
            sleeper: Default::default(),
            soft_clip,
            tracks,
            ws: None,

            symph_formats: vec![],
            symph_decoders: vec![],
            symph_resamplers: vec![],
            symph_inner_frame_pos: vec![],
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

            SymphTrack(s) => {
                match s {
                    crate::input::SymphInput::Lazy(_) => todo!(),
                    crate::input::SymphInput::Raw(_) => todo!(),
                    crate::input::SymphInput::Wrapped(wrapped) => {
                        // FIXME: clean this up and let people config their own probes etc.
                        // FIXME: offer reasonable default (lazy-static) which includes these already.
                        let mut reg = symphonia::core::codecs::CodecRegistry::new();
                        symphonia::default::register_enabled_codecs(&mut reg);
                        reg.register_all::<crate::input::codec::SymphOpusDecoder>();

                        let probe = symphonia::default::get_probe();

                        let mut probe = symphonia::core::probe::Probe::default();
                        probe.register_all::<crate::input::SymphDcaReader>();
                        symphonia::default::register_enabled_formats(&mut probe);

                        // TODO: figure out various methods to maybe pass a hint in, too.
                        let mut hint = symphonia::core::probe::Hint::new();

                        let p_ta = std::time::Instant::now();
                        let f =
                            probe.format(&hint, wrapped, &Default::default(), &Default::default());
                        let d1 = p_ta.elapsed();

                        // TODO: find a way to pass init/track errors back out to calling code.
                        match f {
                            Ok(pr) => {
                                let mut formatter = pr.format;

                                let mut tracks = HashMap::new();

                                for track in formatter.tracks() {
                                    match reg.make(&track.codec_params, &Default::default()) {
                                        Ok(mut txer) => {
                                            tracks.insert(track.id, txer);
                                        },
                                        Err(e) => {
                                            println!("\t\tMake error: {:?}", e);
                                        },
                                    }
                                }

                                self.symph_formats.push(formatter);
                                self.symph_decoders.push(tracks);
                                self.symph_resamplers.push(HashMap::default());
                            },
                            Err(e) => {
                                println!("Symph error: {:?}", e);
                            },
                        }
                        println!(
                            "init time probe format {}, make decoders {}",
                            d1.as_nanos(),
                            p_ta.elapsed().as_nanos()
                        );

                        Ok(())
                    },
                    crate::input::SymphInput::Parsed(_, _) => todo!(),
                }
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

        symph_mix.render_reserved(Some(MONO_FRAME_SIZE));

        // Walk over all the audio files, combining into one audio frame according
        // to volume, play state, etc.
        let mut mix_len = {
            let mut rtp = MutableRtpPacket::new(&mut self.packet[..]).expect(
                "FATAL: Too few bytes in self.packet for RTP header.\
                    (Blame: VOICE_PACKET_MAX?)",
            );

            let payload = rtp.payload_mut();

            // self.mix_tracks(&mut payload[TAG_SIZE..], &mut mix_buffer)
            // mix_tracks(
            //     &mut payload[TAG_SIZE..],
            //     &mut mix_buffer,
            //     &mut self.tracks,
            //     &self.interconnect,
            //     self.prevent_events,
            // )

            let mut occup = 0;

            for ((format, dec_map), resample_map) in self
                .symph_formats
                .iter_mut()
                .zip(self.symph_decoders.iter_mut())
                .zip(self.symph_resamplers.iter_mut())
            {
                // FIXME: assumes only one track per input.
                // FIXME: has a big issue with frame size: need to handle too big AND too small!
                if let Ok(pkt) = format.next_packet() {
                    if let Some(txer) = dec_map.get_mut(&pkt.track_id()) {
                        match txer.decode(&pkt) {
                            Ok(bytes) => {
                                match bytes {
                                    symphonia_core::audio::AudioBufferRef::F32(frame) => {
                                        println!(
                                            "pkt len {} samples @ {}Hz",
                                            frame.frames(),
                                            frame.spec().rate
                                        );

                                        occup = occup.max(frame.frames() * 2);

                                        let mut d_planes = symph_mix.planes_mut();
                                        let s_planes = frame.planes();

                                        // NOTE: this is garbage. just for understanding relative costs...
                                        if frame.spec().rate != 48000 {
                                            let chan_c = frame.spec().channels.count();

                                            let rs = resample_map
                                                .entry(pkt.track_id())
                                                .or_insert_with(|| {
                                                    rubato::FftFixedOut::new(
                                                        frame.spec().rate as usize,
                                                        SAMPLE_RATE_RAW,
                                                        RESAMPLE_OUTPUT_FRAME_SIZE,
                                                        1,
                                                        chan_c,
                                                    )
                                                });

                                            let t = Instant::now();
                                            let rs_planes = rs.process(s_planes.planes()).unwrap();
                                            println!(
                                                "Resample cost: {}ns, {} samples",
                                                t.elapsed().as_nanos(),
                                                rs_planes[0].len()
                                            );

                                            for (d_plane, s_plane) in (&mut d_planes.planes()[..])
                                                .iter_mut()
                                                .zip(rs_planes.iter())
                                            {
                                                for (d, s) in d_plane.iter_mut().zip(s_plane.iter())
                                                {
                                                    *d += s;
                                                }
                                            }
                                        } else {
                                            // FIXME: downmix
                                            for (d_plane, s_plane) in (&mut d_planes.planes()[..])
                                                .iter_mut()
                                                .zip(s_planes.planes()[..].iter())
                                            {
                                                for (d, s) in d_plane.iter_mut().zip(s_plane.iter())
                                                {
                                                    *d += s;
                                                }
                                            }
                                        }
                                    },
                                    _ => {
                                        eprintln!("non float frame!");
                                    },
                                }
                            },
                            Err(e) => {
                                eprintln!("SYMPH PKT ERROR: {:?}", e);
                            },
                        }
                    }
                }
            }

            symph_buffer.copy_interleaved_typed(&symph_mix);

            MixType::MixedPcm(occup)
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

#[derive(Debug, Eq, PartialEq)]
enum MixType {
    Passthrough(usize),
    MixedPcm(usize),
}

struct LocalInput {
    inner_pos: usize,
    resampler: Option<FftFixedOut<f32>>,
    chosen_track: Option<u32>,
}

#[inline]
fn mix_symph() -> MixType {
    todo!()
}

enum MixStatus {
    Live,
    Ended,
    Errored,
}

#[inline]
fn mix_symph_indiv(
    symph_buffer: &mut SampleBuffer<f32>,
    symph_mix: &mut AudioBuffer<f32>,
    resample_scratch: &mut AudioBuffer<f32>,
    input: &mut Parsed,
    local_state: &mut LocalInput,
    volume: f32,
) -> (MixType, MixStatus) {
    let mut samples_written = 0;
    let mut buf_in_progress = false;
    let mut track_status = MixStatus::Live;

    resample_scratch.clear();

    while samples_written != MONO_FRAME_SIZE {
        // FIXME: re-engimeer for one default track.
        let my_decoder = input.decoders.get_mut(&0).unwrap();

        let source_packet = if local_state.inner_pos != 0 {
            // This is a deliberate unwrap:
            my_decoder.last_decoded()
        } else {
            // TODO: move out elsewhere? Try to init local state with default track?
            let default_track = input.format.default_track().map(|t| t.id);

            if let Ok(pkt) = input.format.next_packet() {
                let my_track = local_state
                    .chosen_track
                    .get_or_insert_with(|| default_track.unwrap_or_else(|| pkt.track_id()));

                if pkt.track_id() != *my_track {
                    continue;
                }

                my_decoder
                    .decode(&pkt)
                    .map_err(|e| {
                        track_status = MixStatus::Errored;
                        e
                    })
                    .ok()
            } else {
                // file end.
                track_status = MixStatus::Ended;
                None
            }
        };

        if source_packet.is_none() {
            if buf_in_progress {
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

                // refactor
                // NOTE: if let needed as if-let && {bool} is nightly only.
                if let AudioBufferRef::F32(s_pkt) = source_packet {
                    let refs: Vec<&[f32]> = s_pkt
                        .planes()
                        .planes()
                        .iter()
                        .map(|s| &s[inner_pos..])
                        .collect();

                    local_state.inner_pos += needed_in_frames;
                    local_state.inner_pos %= pkt_frames;

                    resampler.process(&*refs).unwrap();
                } else {
                    unreachable!();
                }

                // FIXME: calc true end pos using sampel rate math?

                // FIXME: actually mix.
            }

            break;
        }

        let source_packet = source_packet.unwrap();

        let in_rate = source_packet.spec().rate;

        if in_rate != SAMPLE_RATE_RAW as u32 {
            // NOTE: this should NEVER change in one stream.
            let chan_c = source_packet.spec().channels.count();
            let resampler = local_state.resampler.get_or_insert_with(|| {
                FftFixedOut::new(
                    in_rate as usize,
                    SAMPLE_RATE_RAW,
                    RESAMPLE_OUTPUT_FRAME_SIZE,
                    1,
                    chan_c,
                )
            });

            let inner_pos = local_state.inner_pos;
            let pkt_frames = source_packet.frames();
            let needed_in_frames = resampler.nbr_frames_needed();
            let available_frames = pkt_frames - inner_pos;

            let force_copy = buf_in_progress || needed_in_frames > available_frames;
            let bytes = if (!force_copy) && matches!(source_packet, AudioBufferRef::F32(_)) {
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
                        .map(|s| &s[inner_pos..])
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

                // FIXME: RENDER_RESERVED ACCORDING TO SIZE.
                // FIXME: DO THE COPY.

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

            samples_written += bytes[0].len();

            // FIXME: mix in from bytes.
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
fn copy_into_resampler(
    source: &AudioBufferRef,
    target: &mut AudioBuffer<f32>,
    source_pos: usize,
    dest_pos: usize,
) -> usize {
    use AudioBufferRef::*;

    match source {
        U8(v) => copy_symph_buffer(v, target, source_pos, dest_pos),
        U16(v) => copy_symph_buffer(v, target, source_pos, dest_pos),
        U24(v) => copy_symph_buffer(v, target, source_pos, dest_pos),
        U32(v) => copy_symph_buffer(v, target, source_pos, dest_pos),
        S8(v) => copy_symph_buffer(v, target, source_pos, dest_pos),
        S16(v) => copy_symph_buffer(v, target, source_pos, dest_pos),
        S24(v) => copy_symph_buffer(v, target, source_pos, dest_pos),
        S32(v) => copy_symph_buffer(v, target, source_pos, dest_pos),
        F32(v) => copy_symph_buffer(v, target, source_pos, dest_pos),
        F64(v) => copy_symph_buffer(v, target, source_pos, dest_pos),
    }
}

#[inline]
fn copy_symph_buffer<S>(
    source: &AudioBuffer<S>,
    target: &mut AudioBuffer<f32>,
    source_pos: usize,
    dest_pos: usize,
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
            *d = (*s).into_sample();
        }
    }

    mix_ct
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
