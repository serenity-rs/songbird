#![allow(missing_docs)]

use crate::{
    driver::{connection::error::Error, Bitrate, Config},
    events::{context_data::DisconnectReason, EventData},
    tracks::{Track, TrackCommand, TrackHandle},
    ConnectionInfo,
};
use flume::{Receiver, Sender};

pub enum CoreMessage {
    ConnectWithResult(ConnectionInfo, Sender<Result<(), Error>>),
    RetryConnect(usize),
    SignalWsClosure(usize, ConnectionInfo, Option<DisconnectReason>),
    Disconnect,
    SetTrack(Option<TrackContext>),
    AddTrack(TrackContext),
    SetBitrate(Bitrate),
    AddEvent(EventData),
    RemoveGlobalEvents,
    SetConfig(Config),
    Mute(bool),
    Reconnect,
    FullReconnect,
    RebuildInterconnect,
    Poison,
}

pub struct TrackContext {
    pub track: Track,
    pub handle: TrackHandle,
    pub receiver: Receiver<TrackCommand>,
}
