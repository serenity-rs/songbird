#![allow(missing_docs)]

pub(crate) mod disposal;
pub mod error;
mod events;
pub mod message;
pub mod mixer;
pub(crate) mod udp_rx;
pub(crate) mod udp_tx;
pub(crate) mod ws;

use super::connection::{error::Error as ConnectionError, Connection};
use crate::{events::CoreContext, Config};
use flume::{Receiver, RecvError, Sender};
use message::*;
#[cfg(not(feature = "tokio-02-marker"))]
use tokio::{runtime::Handle, spawn};
#[cfg(feature = "tokio-02-marker")]
use tokio_compat::{runtime::Handle, spawn};
use tracing::{error, instrument, trace};

pub(crate) fn start(config: Config, rx: Receiver<CoreMessage>, tx: Sender<CoreMessage>) {
    spawn(async move {
        trace!("Driver started.");
        runner(config, rx, tx).await;
        trace!("Driver finished.");
    });
}

fn start_internals(core: Sender<CoreMessage>, config: Config) -> Interconnect {
    let (evt_tx, evt_rx) = flume::unbounded();
    let (mix_tx, mix_rx) = flume::unbounded();

    let interconnect = Interconnect {
        core,
        events: evt_tx,
        mixer: mix_tx,
    };

    let ic = interconnect.clone();
    spawn(async move {
        trace!("Event processor started.");
        events::runner(ic, evt_rx).await;
        trace!("Event processor finished.");
    });

    let ic = interconnect.clone();
    let handle = Handle::current();
    std::thread::spawn(move || {
        trace!("Mixer started.");
        mixer::runner(ic, mix_rx, handle, config);
        trace!("Mixer finished.");
    });

    interconnect
}

#[instrument(skip(rx, tx))]
async fn runner(mut config: Config, rx: Receiver<CoreMessage>, tx: Sender<CoreMessage>) {
    let mut next_config: Option<Config> = None;
    let mut connection = None;
    let mut interconnect = start_internals(tx, config.clone());

    loop {
        match rx.recv_async().await {
            Ok(CoreMessage::ConnectWithResult(info, tx)) => {
                config = if let Some(new_config) = next_config.take() {
                    let _ = interconnect
                        .mixer
                        .send(MixerMessage::SetConfig(new_config.clone()));
                    new_config
                } else {
                    config
                };

                connection = match Connection::new(info, &interconnect, &config).await {
                    Ok(connection) => {
                        // Other side may not be listening: this is fine.
                        let _ = tx.send(Ok(()));

                        let _ = interconnect
                            .events
                            .send(EventMessage::FireCoreEvent(CoreContext::DriverConnect));

                        Some(connection)
                    },
                    Err(why) => {
                        // See above.
                        let _ = tx.send(Err(why));

                        let _ = interconnect.events.send(EventMessage::FireCoreEvent(
                            CoreContext::DriverConnectFailed,
                        ));

                        None
                    },
                };
            },
            Ok(CoreMessage::Disconnect) => {
                connection = None;
                let _ = interconnect.mixer.send(MixerMessage::DropConn);
                let _ = interconnect.mixer.send(MixerMessage::RebuildEncoder);
            },
            Ok(CoreMessage::SetTrack(s)) => {
                let _ = interconnect.mixer.send(MixerMessage::SetTrack(s));
            },
            Ok(CoreMessage::AddTrack(s)) => {
                let _ = interconnect.mixer.send(MixerMessage::AddTrack(s));
            },
            Ok(CoreMessage::SetBitrate(b)) => {
                let _ = interconnect.mixer.send(MixerMessage::SetBitrate(b));
            },
            Ok(CoreMessage::SetConfig(mut new_config)) => {
                next_config = Some(new_config.clone());

                new_config.make_safe(&config, connection.is_some());

                let _ = interconnect.mixer.send(MixerMessage::SetConfig(new_config));
            },
            Ok(CoreMessage::AddEvent(evt)) => {
                let _ = interconnect.events.send(EventMessage::AddGlobalEvent(evt));
            },
            Ok(CoreMessage::RemoveGlobalEvents) => {
                let _ = interconnect.events.send(EventMessage::RemoveGlobalEvents);
            },
            Ok(CoreMessage::Mute(m)) => {
                let _ = interconnect.mixer.send(MixerMessage::SetMute(m));
            },
            Ok(CoreMessage::Reconnect) => {
                if let Some(mut conn) = connection.take() {
                    // try once: if interconnect, try again.
                    // if still issue, full connect.
                    let info = conn.info.clone();

                    let full_connect = match conn.reconnect().await {
                        Ok(()) => {
                            connection = Some(conn);
                            false
                        },
                        Err(ConnectionError::InterconnectFailure(_)) => {
                            interconnect.restart_volatile_internals();

                            match conn.reconnect().await {
                                Ok(()) => {
                                    connection = Some(conn);
                                    false
                                },
                                _ => true,
                            }
                        },
                        _ => true,
                    };

                    if full_connect {
                        connection = Connection::new(info, &interconnect, &config)
                            .await
                            .map_err(|e| {
                                error!("Catastrophic connection failure. Stopping. {:?}", e);
                                let _ = interconnect.events.send(EventMessage::FireCoreEvent(
                                    CoreContext::DriverReconnectFailed,
                                ));
                                e
                            })
                            .ok();
                    }

                    if connection.is_some() {
                        let _ = interconnect
                            .events
                            .send(EventMessage::FireCoreEvent(CoreContext::DriverReconnect));
                    }
                }
            },
            Ok(CoreMessage::FullReconnect) =>
                if let Some(conn) = connection.take() {
                    let info = conn.info.clone();

                    connection = Connection::new(info, &interconnect, &config)
                        .await
                        .map_err(|e| {
                            error!("Catastrophic connection failure. Stopping. {:?}", e);
                            let _ = interconnect.events.send(EventMessage::FireCoreEvent(
                                CoreContext::DriverReconnectFailed,
                            ));
                            e
                        })
                        .ok();

                    if connection.is_some() {
                        let _ = interconnect
                            .events
                            .send(EventMessage::FireCoreEvent(CoreContext::DriverReconnect));
                    }
                },
            Ok(CoreMessage::RebuildInterconnect) => {
                interconnect.restart_volatile_internals();
            },
            Err(RecvError::Disconnected) | Ok(CoreMessage::Poison) => {
                break;
            },
        }
    }

    trace!("Main thread exited");
    interconnect.poison_all();
}
