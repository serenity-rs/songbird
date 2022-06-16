use crate::tracks::ReadyState;
use symphonia_core::errors::Error as SymphError;

use super::*;

pub struct InternalTrack {
    pub(crate) playing: PlayMode,
    pub(crate) volume: f32,
    pub(crate) input: InputState,
    pub(crate) mix_state: DecodeState,
    pub(crate) position: Duration,
    pub(crate) play_time: Duration,
    pub(crate) commands: Receiver<TrackCommand>,
    pub(crate) loops: LoopState,
}

impl<'a> InternalTrack {
    pub(crate) fn decompose_track(
        val: TrackContext,
    ) -> (Self, EventStore, TrackState, TrackHandle) {
        let TrackContext {
            handle,
            track,
            receiver,
        } = val;
        let out = InternalTrack {
            playing: track.playing,
            volume: track.volume,
            input: InputState::from(track.input),
            mix_state: DecodeState::default(),
            position: Duration::default(),
            play_time: Duration::default(),
            commands: receiver,
            loops: track.loops,
        };

        let state = out.state();

        (out, track.events, state, handle)
    }

    pub(crate) fn state(&self) -> TrackState {
        let ready = (&self.input).into();

        TrackState {
            playing: self.playing.clone(),
            volume: self.volume,
            position: self.position,
            play_time: self.play_time,
            loops: self.loops,
            ready,
        }
    }

    pub(crate) fn view(&'a mut self) -> View<'a> {
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

    pub(crate) fn process_commands(&mut self, index: usize, ic: &Interconnect) -> Action {
        // Note: disconnection and an empty channel are both valid,
        // and should allow the audio object to keep running as intended.

        // We also need to export a target seek point to the mixer, if known.
        let mut action = Action::default();

        // Note that interconnect failures are not currently errors.
        // In correct operation, the event thread should never panic,
        // but it receiving status updates is secondary do actually
        // doing the work.
        while let Ok(cmd) = self.commands.try_recv() {
            match cmd {
                TrackCommand::Play => {
                    self.playing.change_to(PlayMode::Play);
                    drop(ic.events.send(EventMessage::ChangeState(
                        index,
                        TrackStateChange::Mode(self.playing.clone()),
                    )));
                },
                TrackCommand::Pause => {
                    self.playing.change_to(PlayMode::Pause);
                    drop(ic.events.send(EventMessage::ChangeState(
                        index,
                        TrackStateChange::Mode(self.playing.clone()),
                    )));
                },
                TrackCommand::Stop => {
                    self.playing.change_to(PlayMode::Stop);
                    drop(ic.events.send(EventMessage::ChangeState(
                        index,
                        TrackStateChange::Mode(self.playing.clone()),
                    )));
                },
                TrackCommand::Volume(vol) => {
                    self.volume = vol;
                    drop(ic.events.send(EventMessage::ChangeState(
                        index,
                        TrackStateChange::Volume(self.volume),
                    )));
                },
                TrackCommand::Seek(time) => action.seek_point = Some(time),
                TrackCommand::AddEvent(evt) => {
                    drop(ic.events.send(EventMessage::AddTrackEvent(index, evt)));
                },
                TrackCommand::Do(func) => {
                    if let Some(indiv_action) = func(self.view()) {
                        action.combine(indiv_action);
                    }

                    drop(ic.events.send(EventMessage::ChangeState(
                        index,
                        TrackStateChange::Total(self.state()),
                    )));
                },
                TrackCommand::Request(tx) => {
                    drop(tx.send(self.state()));
                },
                TrackCommand::Loop(loops) => {
                    self.loops = loops;
                    drop(ic.events.send(EventMessage::ChangeState(
                        index,
                        TrackStateChange::Loops(self.loops, true),
                    )));
                },
                TrackCommand::MakePlayable => action.make_playable = true,
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

    /// Readies the requested input state.
    ///
    /// Returns the usable version of the audio if available, and whether the track should be deleted.
    pub(crate) fn get_or_ready_input(
        &'a mut self,
        id: usize,
        interconnect: &Interconnect,
        pool: &BlockyTaskPool,
        config: &Arc<Config>,
        prevent_events: bool,
    ) -> StdResult<(&'a mut Parsed, &'a mut DecodeState), InputReadyingError> {
        let input = &mut self.input;
        let local = &mut self.mix_state;

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

                if !prevent_events {
                    drop(interconnect.events.send(EventMessage::ChangeState(
                        id,
                        TrackStateChange::Ready(ReadyState::Preparing),
                    )));
                }

                Err(InputReadyingError::Waiting)
            },
            InputState::Preparing(info) => {
                let queued_seek = info.queued_seek.take();

                let orig_out = match info.callback.try_recv() {
                    Ok(MixerInputResultMessage::Built(parsed, rec)) => {
                        *input = InputState::Ready(parsed, rec);
                        local.reset();

                        // TODO: set position to the true track position here?

                        if !prevent_events {
                            drop(interconnect.events.send(EventMessage::ChangeState(
                                id,
                                TrackStateChange::Ready(ReadyState::Playable),
                            )));
                        }

                        if let InputState::Ready(ref mut parsed, _) = input {
                            Ok(parsed)
                        } else {
                            unreachable!()
                        }
                    },
                    Ok(MixerInputResultMessage::Seek(parsed, rec, seek_res)) => {
                        match seek_res {
                            Ok(pos) =>
                                if let Some(time_base) = parsed.decoder.codec_params().time_base {
                                    // modify track.
                                    let new_time = time_base.calc_time(pos.actual_ts);
                                    let time_in_float = new_time.seconds as f64 + new_time.frac;
                                    self.position =
                                        std::time::Duration::from_secs_f64(time_in_float);

                                    if !prevent_events {
                                        drop(interconnect.events.send(EventMessage::ChangeState(
                                            id,
                                            TrackStateChange::Position(self.position),
                                        )));

                                        drop(interconnect.events.send(EventMessage::ChangeState(
                                            id,
                                            TrackStateChange::Ready(ReadyState::Playable),
                                        )));
                                    }

                                    local.reset();
                                    *input = InputState::Ready(parsed, rec);

                                    if let InputState::Ready(ref mut parsed, _) = input {
                                        Ok(parsed)
                                    } else {
                                        unreachable!()
                                    }
                                } else {
                                    Err(InputReadyingError::Seeking(SymphError::Unsupported(
                                        "Track had no recorded time base.",
                                    )))
                                },
                            Err(e) => Err(InputReadyingError::Seeking(e)),
                        }
                    },
                    Ok(MixerInputResultMessage::CreateErr(e)) =>
                        Err(InputReadyingError::Creation(e)),
                    Ok(MixerInputResultMessage::ParseErr(e)) => Err(InputReadyingError::Parsing(e)),
                    Err(TryRecvError::Disconnected) => Err(InputReadyingError::Dropped),
                    Err(TryRecvError::Empty) => Err(InputReadyingError::Waiting),
                };

                let orig_out = orig_out.map(|a| (a, &mut self.mix_state));

                match (orig_out, queued_seek) {
                    (Ok(v), Some(_time)) => {
                        warn!("Track was given seek command while busy: handling not impl'd yet.");
                        Ok(v)
                    },
                    (a, _) => a,
                }
            },
            InputState::Ready(parsed, _) => Ok((parsed, &mut self.mix_state)),
        }
    }
}
