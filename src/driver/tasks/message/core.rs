#![allow(missing_docs)]

use crate::{
    driver::{connection::error::Error, Bitrate, Config},
    events::EventData,
    tracks::Track,
    ConnectionInfo,
};
use flume::Sender;

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum CoreMessage {
    ConnectWithResult(ConnectionInfo, Sender<Result<(), Error>>),
    Disconnect,
    SetTrack(Option<Track>),
    AddTrack(Track),
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
