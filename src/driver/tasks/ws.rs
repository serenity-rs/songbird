use super::message::*;
use crate::{
    events::CoreContext,
    model::{
        payload::{Heartbeat, Speaking},
        CloseCode as VoiceCloseCode,
        Event as GatewayEvent,
        FromPrimitive,
        SpeakingState,
    },
    ws::{Error as WsError, WsStream},
    ConnectionInfo,
};
use flume::Receiver;
use rand::random;
use std::time::Duration;
use tokio::{
    select,
    time::{sleep_until, Instant},
};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tracing::{debug, info, instrument, trace, warn};

struct AuxNetwork {
    rx: Receiver<WsMessage>,
    ws_client: WsStream,
    dont_send: bool,

    ssrc: u32,
    heartbeat_interval: Duration,

    speaking: SpeakingState,
    last_heartbeat_nonce: Option<u64>,

    attempt_idx: usize,
    info: ConnectionInfo,
}

impl AuxNetwork {
    pub(crate) fn new(
        evt_rx: Receiver<WsMessage>,
        ws_client: WsStream,
        ssrc: u32,
        heartbeat_interval: f64,
        attempt_idx: usize,
        info: ConnectionInfo,
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
                _ = hb => {
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
                ws_msg = self.ws_client.recv_json_no_timeout(), if !self.dont_send => {
                    ws_error = match ws_msg {
                        Err(WsError::Json(e)) => {
                            debug!("Unexpected JSON {:?}.", e);
                            false
                        },
                        Err(e) => {
                            should_reconnect = ws_error_is_not_final(&e);
                            ws_reason = Some((&e).into());
                            true
                        },
                        Ok(Some(msg)) => {
                            self.process_ws(interconnect, msg);
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
        let nonce = random::<u64>();
        self.last_heartbeat_nonce = Some(nonce);

        trace!("Sent heartbeat {:?}", self.speaking);

        if !self.dont_send {
            self.ws_client
                .send_json(&GatewayEvent::from(Heartbeat { nonce }))
                .await?;
        }

        Ok(())
    }

    fn process_ws(&mut self, interconnect: &Interconnect, value: GatewayEvent) {
        match value {
            GatewayEvent::Speaking(ev) => {
                drop(interconnect.events.send(EventMessage::FireCoreEvent(
                    CoreContext::SpeakingStateUpdate(ev),
                )));
            },
            GatewayEvent::ClientConnect(ev) => {
                debug!("Received discontinued ClientConnect: {:?}", ev);
            },
            GatewayEvent::ClientDisconnect(ev) => {
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
            other => {
                trace!("Received other websocket data: {:?}", other);
            },
        }
    }
}

#[instrument(skip(interconnect, ws_client))]
pub(crate) async fn runner(
    mut interconnect: Interconnect,
    evt_rx: Receiver<WsMessage>,
    ws_client: WsStream,
    ssrc: u32,
    heartbeat_interval: f64,
    attempt_idx: usize,
    info: ConnectionInfo,
) {
    trace!("WS thread started.");
    let mut aux = AuxNetwork::new(
        evt_rx,
        ws_client,
        ssrc,
        heartbeat_interval,
        attempt_idx,
        info,
    );

    aux.run(&mut interconnect).await;
    trace!("WS thread finished.");
}

fn ws_error_is_not_final(err: &WsError) -> bool {
    match err {
        WsError::WsClosed(Some(frame)) => match frame.code {
            CloseCode::Library(l) =>
                if let Some(code) = VoiceCloseCode::from_u16(l) {
                    code.should_resume()
                } else {
                    true
                },
            _ => true,
        },
        e => {
            debug!("Error sending/receiving ws {:?}.", e);
            true
        },
    }
}
