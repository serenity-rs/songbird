use super::message::*;
use crate::{
    events::{EventStore, GlobalEvents, TrackEvent},
    tracks::{ReadyState, TrackHandle, TrackState},
};
use flume::Receiver;
use tracing::{debug, info, instrument, trace};

#[instrument(skip(_interconnect, evt_rx))]
pub(crate) async fn runner(_interconnect: Interconnect, evt_rx: Receiver<EventMessage>) {
    let mut global = GlobalEvents::default();

    let mut events: Vec<EventStore> = vec![];
    let mut states: Vec<TrackState> = vec![];
    let mut handles: Vec<TrackHandle> = vec![];

    loop {
        match evt_rx.recv_async().await {
            Ok(EventMessage::AddGlobalEvent(data)) => {
                info!("Global event added.");
                global.add_event(data);
            },
            Ok(EventMessage::AddTrackEvent(i, data)) => {
                info!("Adding event to track {}.", i);

                let event_store = events
                    .get_mut(i)
                    .expect("Event thread was given an illegal store index for AddTrackEvent.");
                let state = states
                    .get_mut(i)
                    .expect("Event thread was given an illegal state index for AddTrackEvent.");

                event_store.add_event(data, state.position);
            },
            Ok(EventMessage::FireCoreEvent(ctx)) => {
                let ctx = ctx.to_user_context();
                let evt = ctx
                    .to_core_event()
                    .expect("Event thread was passed a non-core event in FireCoreEvent.");

                trace!("Firing core event {:?}.", evt);

                global.fire_core_event(evt, ctx).await;
            },
            Ok(EventMessage::RemoveGlobalEvents) => {
                global.remove_handlers();
            },
            Ok(EventMessage::AddTrack(store, state, handle)) => {
                events.push(store);
                states.push(state);
                handles.push(handle);

                info!("Event state for track {} added", events.len());
            },
            Ok(EventMessage::ChangeState(i, change)) => {
                let max_states = states.len();
                debug!(
                    "Changing state for track {} of {}: {:?}",
                    i, max_states, change
                );

                let state = states
                    .get_mut(i)
                    .expect("Event thread was given an illegal state index for ChangeState.");

                match change {
                    TrackStateChange::Mode(mut mode) => {
                        std::mem::swap(&mut state.playing, &mut mode);
                        if state.playing != mode {
                            global.fire_track_event(state.playing.as_track_event(), i);
                            if let Some(other_evts) = state.playing.also_fired_track_events() {
                                for evt in other_evts {
                                    global.fire_track_event(evt, i);
                                }
                            }
                        }
                    },
                    TrackStateChange::Volume(vol) => {
                        state.volume = vol;
                    },
                    TrackStateChange::Position(pos) => {
                        // Currently, only Tick should fire time events.
                        state.position = pos;
                    },
                    TrackStateChange::Loops(loops, user_set) => {
                        state.loops = loops;
                        if !user_set {
                            global.fire_track_event(TrackEvent::Loop, i);
                        }
                    },
                    TrackStateChange::Total(new) => {
                        // Massive, unprecedented state changes.
                        *state = new;
                    },
                    TrackStateChange::Ready(ready_state) => {
                        state.ready = ready_state;

                        match ready_state {
                            ReadyState::Playable => {
                                global.fire_track_event(TrackEvent::Playable, i);
                            },
                            ReadyState::Preparing => {
                                global.fire_track_event(TrackEvent::Preparing, i);
                            },
                            ReadyState::Uninitialised => {},
                        }
                    },
                }
            },
            Ok(EventMessage::RemoveTrack(i)) => {
                info!("Event state for track {} of {} removed.", i, events.len());

                events.swap_remove(i);
                states.swap_remove(i);
                handles.swap_remove(i);
            },
            Ok(EventMessage::RemoveAllTracks) => {
                info!("Event state for all tracks removed.");

                events.clear();
                states.clear();
                handles.clear();
            },
            Ok(EventMessage::Tick) => {
                // NOTE: this should fire saved up blocks of state change evts.
                global.tick(&mut events, &mut states, &mut handles).await;
            },
            Err(_) | Ok(EventMessage::Poison) => {
                break;
            },
        }
    }

    trace!("Event thread exited.");
}
