use super::message::*;
use crate::{
    events::CoreContext,
    model::{
        payload::{Heartbeat, Speaking},
        CloseCode as VoiceCloseCode, Event as GatewayEvent, FromPrimitive, SpeakingState,
    },
    ws::{Error as WsError, WsStream},
    ConnectionInfo,
};
use flume::Receiver;
use rand::{distr::Uniform, Rng};
use serenity_voice_model::payload::{
    DaveMlsInvalidCommitWelcome, DaveMlsKeyPackage, DaveMlsProposalsOperationType,
    DaveTransitionReady,
};
#[cfg(feature = "receive")]
use std::sync::Arc;
use std::{collections::HashMap, num::NonZeroU16, sync::Arc, time::Duration};
use tokio::{
    select,
    sync::RwLock,
    time::{sleep_until, Instant},
};
#[cfg(feature = "tungstenite")]
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tracing::{debug, info, instrument, trace, warn};

pub(crate) struct AuxNetwork {
    rx: Receiver<WsMessage>,
    ws_client: WsStream,
    dont_send: bool,

    ssrc: u32,
    heartbeat_interval: Duration,

    speaking: SpeakingState,
    last_heartbeat_nonce: Option<u64>,

    attempt_idx: usize,
    info: ConnectionInfo,

    dave_session: Arc<RwLock<Option<davey::DaveSession>>>,
    dave_protocol_version: Option<NonZeroU16>,
    dave_pending_transitions: HashMap<u16, u16>,
    dave_downgraded: bool,

    #[cfg(feature = "receive")]
    ssrc_signalling: Arc<SsrcTracker>,
}

impl AuxNetwork {
    pub(crate) fn new(
        evt_rx: Receiver<WsMessage>,
        ws_client: WsStream,
        ssrc: u32,
        heartbeat_interval: f64,
        attempt_idx: usize,
        info: ConnectionInfo,
        dave_session: Arc<RwLock<Option<davey::DaveSession>>>,
        dave_protocol_version: Option<NonZeroU16>,
        #[cfg(feature = "receive")] ssrc_signalling: Arc<SsrcTracker>,
    ) -> Self {
        Self {
            rx: evt_rx,
            ws_client,
            dont_send: false,

            ssrc,
            heartbeat_interval: Duration::from_secs_f64(heartbeat_interval / 1000.0),

            speaking: SpeakingState::empty(),
            last_heartbeat_nonce: None,

            attempt_idx,
            info,

            dave_session,
            dave_protocol_version,
            dave_pending_transitions: HashMap::new(),
            dave_downgraded: false,

            #[cfg(feature = "receive")]
            ssrc_signalling,
        }
    }

    #[instrument(skip(self))]
    async fn run(&mut self, interconnect: &mut Interconnect) {
        let mut next_heartbeat = Instant::now() + self.heartbeat_interval;

        loop {
            let mut ws_error = false;
            let mut should_reconnect = false;
            let mut ws_reason = None;

            let hb = sleep_until(next_heartbeat);

            select! {
                () = hb => {
                    ws_error = match self.send_heartbeat().await {
                        Err(e) => {
                            should_reconnect = ws_error_is_not_final(&e);
                            ws_reason = Some((&e).into());
                            true
                        },
                        _ => false,
                    };
                    next_heartbeat = self.next_heartbeat();
                }
                ws_msg = self.ws_client.recv_event_no_timeout(), if !self.dont_send => {
                    ws_error = match ws_msg {
                        Err(e) => {
                            should_reconnect = ws_error_is_not_final(&e);
                            ws_reason = Some((&e).into());
                            true
                        },
                        Ok(Some(msg)) => {
                            self.process_ws(interconnect, msg).await.expect("TODO");
                            false
                        },
                        _ => false,
                    };
                }
                inner_msg = self.rx.recv_async() => {
                    match inner_msg {
                        Ok(WsMessage::Ws(data)) => {
                            self.ws_client = *data;
                            next_heartbeat = self.next_heartbeat();
                            self.dont_send = false;
                        },
                        Ok(WsMessage::ReplaceInterconnect(i)) => {
                            *interconnect = i;
                        },
                        Ok(WsMessage::SetKeepalive(keepalive)) => {
                            self.heartbeat_interval = Duration::from_secs_f64(keepalive / 1000.0);
                            next_heartbeat = self.next_heartbeat();
                        },
                        Ok(WsMessage::Speaking(is_speaking)) => {
                            if self.speaking.contains(SpeakingState::MICROPHONE) != is_speaking && !self.dont_send {
                                self.speaking.set(SpeakingState::MICROPHONE, is_speaking);
                                info!("Changing to {:?}", self.speaking);

                                let ssu_status = self.ws_client
                                    .send_json(&GatewayEvent::from(Speaking {
                                        delay: Some(0),
                                        speaking: self.speaking,
                                        ssrc: self.ssrc,
                                        user_id: None,
                                    }))
                                    .await;

                                ws_error |= match ssu_status {
                                    Err(e) => {
                                        should_reconnect = ws_error_is_not_final(&e);
                                        ws_reason = Some((&e).into());
                                        true
                                    },
                                    _ => false,
                                }
                            }
                        },
                        Ok(WsMessage::Deliver(msg)) => {
                            self.process_ws(interconnect, msg).await.expect("TODO");
                        },
                        Err(flume::RecvError::Disconnected) => {
                            break;
                        },
                    }
                }
            }

            if ws_error {
                self.dont_send = true;

                if should_reconnect {
                    drop(interconnect.core.send(CoreMessage::Reconnect));
                } else {
                    drop(interconnect.core.send(CoreMessage::SignalWsClosure(
                        self.attempt_idx,
                        self.info.clone(),
                        ws_reason,
                    )));
                    break;
                }
            }
        }
    }

    fn next_heartbeat(&self) -> Instant {
        Instant::now() + self.heartbeat_interval
    }

    async fn send_heartbeat(&mut self) -> Result<(), WsError> {
        // Discord have suddenly, mysteriously, started rejecting
        // ints-as-strings. Keep JS happy here, I suppose...
        const JS_MAX_INT: u64 = (1u64 << 53) - 1;
        let nonce_range =
            Uniform::new(0, JS_MAX_INT).expect("uniform range is finite and nonempty");
        let nonce = rand::rng().sample(nonce_range);
        self.last_heartbeat_nonce = Some(nonce);

        trace!("Sent heartbeat {:?}", self.speaking);

        if !self.dont_send {
            self.ws_client
                .send_json(&GatewayEvent::from(Heartbeat { nonce }))
                .await?;
        }

        Ok(())
    }

    async fn process_ws(
        &mut self,
        interconnect: &Interconnect,
        value: GatewayEvent,
    ) -> Result<(), WsError> {
        match value {
            GatewayEvent::Speaking(ev) => {
                #[cfg(feature = "receive")]
                if let Some(user_id) = &ev.user_id {
                    self.ssrc_signalling.user_ssrc_map.insert(*user_id, ev.ssrc);
                }

                drop(interconnect.events.send(EventMessage::FireCoreEvent(
                    CoreContext::SpeakingStateUpdate(ev),
                )));
            },
            GatewayEvent::ClientConnect(ev) => {
                debug!("Received discontinued ClientConnect: {:?}", ev);
            },
            GatewayEvent::ClientDisconnect(ev) => {
                #[cfg(feature = "receive")]
                {
                    self.ssrc_signalling.disconnected_users.insert(ev.user_id);
                }

                drop(interconnect.events.send(EventMessage::FireCoreEvent(
                    CoreContext::ClientDisconnect(ev),
                )));
            },
            GatewayEvent::HeartbeatAck(ev) => {
                if let Some(nonce) = self.last_heartbeat_nonce.take() {
                    if ev.nonce == nonce {
                        trace!("Heartbeat ACK received.");
                    } else {
                        warn!(
                            "Heartbeat nonce mismatch! Expected {}, saw {}.",
                            nonce, ev.nonce
                        );
                    }
                }
            },
            GatewayEvent::DavePrepareTransition(ev) => {
                self.dave_pending_transitions
                    .insert(ev.transition_id, ev.protocol_version);

                if ev.transition_id == 0 {
                    self.execute_dave_transition(ev.transition_id).await;
                } else {
                    if ev.protocol_version == 0 {
                        if let Some(ref mut dave_session) = *self.dave_session.write().await {
                            dave_session.set_passthrough_mode(true, Some(120));
                        }

                        self.ws_client
                            .send_json(&GatewayEvent::DaveTransitionReady(DaveTransitionReady {
                                transition_id: ev.transition_id,
                                protocol_version: ev.protocol_version,
                            }))
                            .await?;
                    }
                }
            },
            GatewayEvent::DaveExecuteTransition(ev) => {
                self.execute_dave_transition(ev.transition_id).await;
            },
            GatewayEvent::DavePrepareEpoch(ev) if ev.epoch == 1 => {
                self.dave_protocol_version = NonZeroU16::new(ev.protocol_version);
                self.reinit_dave_session().await;
            },
            GatewayEvent::DaveMlsExternalSender(ev) => {
                if let Some(ref mut dave_session) = *self.dave_session.write().await {
                    dave_session
                        .set_external_sender(&ev.external_sender)
                        .expect("TODO");
                }
            },
            GatewayEvent::DaveMlsProposals(ev) => {
                let operation_type = match ev.operation_type {
                    DaveMlsProposalsOperationType::Append => davey::ProposalsOperationType::APPEND,
                    DaveMlsProposalsOperationType::Revoke => davey::ProposalsOperationType::REVOKE,
                };
                if let Some(ref mut dave_session) = *self.dave_session.write().await {
                    dave_session
                        .process_proposals(operation_type, &ev.proposals, None)
                        .expect("TODO");
                }
            },
            GatewayEvent::DaveMlsAnnounceCommitTransition(ev) => {
                let mut lock = self.dave_session.write().await;

                if let Some(ref mut dave_session) = *lock {
                    match dave_session.process_commit(&ev.commit_message) {
                        Ok(_) => {
                            if ev.transition_id != 0 {
                                self.dave_pending_transitions.insert(
                                    ev.transition_id,
                                    dave_session.protocol_version().into(),
                                );
                                self.ws_client
                                    .send_json(&GatewayEvent::DaveTransitionReady(
                                        DaveTransitionReady {
                                            transition_id: ev.transition_id,
                                            protocol_version: dave_session
                                                .protocol_version()
                                                .into(),
                                        },
                                    ))
                                    .await?;
                            }
                        },
                        Err(e) => {
                            warn!("MLS commit errored: {e:?}");
                            self.ws_client
                                .send_json(&GatewayEvent::DaveMlsInvalidCommitWelcome(
                                    DaveMlsInvalidCommitWelcome {
                                        transition_id: ev.transition_id,
                                    },
                                ))
                                .await?;
                            drop(lock);
                            self.reinit_dave_session().await;
                        },
                    }
                }
            },
            GatewayEvent::DaveMlsWelcome(ev) => {
                let mut lock = self.dave_session.write().await;

                if let Some(ref mut dave_session) = *lock {
                    match dave_session.process_welcome(&ev.welcome) {
                        Ok(_) => {},
                        Err(e) => {
                            warn!("MLS welcome errored: {e:?}");
                            self.ws_client
                                .send_json(&GatewayEvent::DaveMlsInvalidCommitWelcome(
                                    DaveMlsInvalidCommitWelcome {
                                        transition_id: ev.transition_id,
                                    },
                                ))
                                .await?;
                            drop(lock);
                            self.reinit_dave_session().await;
                        },
                    }
                }
            },
            other => {
                trace!("Received other websocket data: {:?}", other);
            },
        }

        Ok(())
    }

    async fn reinit_dave_session(&mut self) {
        if let Some(dave_protocol_version) = self.dave_protocol_version {
            let key_package = if let Some(ref mut dave_session) = *self.dave_session.write().await {
                dave_session
                    .reinit(
                        dave_protocol_version,
                        self.info.user_id.0.into(),
                        self.info.channel_id.expect("TODO").0.into(),
                        None,
                    )
                    .expect("TODO");
                dave_session.create_key_package().expect("TODO")
            } else {
                let mut dave_session = davey::DaveSession::new(
                    dave_protocol_version,
                    self.info.user_id.0.into(),
                    self.info.channel_id.expect("TODO").0.into(),
                    None,
                )
                .expect("TODO");
                let key_package = dave_session.create_key_package().expect("TODO");

                *self.dave_session.write().await = Some(dave_session);

                key_package
            };

            self.ws_client
                .send_binary(&GatewayEvent::DaveMlsKeyPackage(DaveMlsKeyPackage {
                    key_package,
                }))
                .await
                .expect("TODO");
        } else if let Some(ref mut dave_session) = *self.dave_session.write().await {
            dave_session.reset().expect("TODO");
            dave_session.set_passthrough_mode(true, Some(10));
        }
    }

    async fn execute_dave_transition(&mut self, transition_id: u16) {
        let Some(new_version) = self.dave_pending_transitions.get(&transition_id) else {
            warn!("Received DaveExecuteTransition for unknown transition ID {transition_id}");
            return;
        };
        let old_version = if let Some(ref dave_session) = *self.dave_session.read().await {
            dave_session.protocol_version().into()
        } else {
            0u16
        };

        if old_version != *new_version && *new_version == 0 {
            self.dave_downgraded = true;
        } else if transition_id > 0 && self.dave_downgraded {
            self.dave_downgraded = false;

            if let Some(ref mut dave_session) = *self.dave_session.write().await {
                dave_session.set_passthrough_mode(true, Some(10));
            }
        }

        self.dave_pending_transitions.remove(&transition_id);
    }
}

#[instrument(skip(interconnect, aux))]
pub(crate) async fn runner(mut interconnect: Interconnect, mut aux: AuxNetwork) {
    trace!("WS thread started.");
    aux.run(&mut interconnect).await;
    trace!("WS thread finished.");
}

fn ws_error_is_not_final(err: &WsError) -> bool {
    match err {
        #[cfg(feature = "tungstenite")]
        WsError::WsClosed(Some(frame)) => match frame.code {
            CloseCode::Library(l) => {
                if let Some(code) = VoiceCloseCode::from_u16(l) {
                    code.should_resume()
                } else {
                    true
                }
            },
            _ => true,
        },
        #[cfg(feature = "tws")]
        WsError::WsClosed(Some(code)) => match (*code).into() {
            code @ 4000..=4999_u16 => {
                if let Some(code) = VoiceCloseCode::from_u16(code) {
                    code.should_resume()
                } else {
                    true
                }
            },
            _ => true,
        },
        e => {
            debug!("Error sending/receiving ws {:?}.", e);
            true
        },
    }
}
