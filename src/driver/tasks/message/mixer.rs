#![allow(missing_docs)]

use super::{Interconnect, TrackContext, UdpRxMessage, UdpTxMessage, WsMessage};

use crate::{
    driver::{Bitrate, Config, CryptoState},
    input::{AudioStreamError, Compose, Parsed},
};
use flume::Sender;
use std::sync::Arc;
use symphonia_core::{errors::Error as SymphoniaError, formats::SeekedTo};
use xsalsa20poly1305::XSalsa20Poly1305 as Cipher;

pub struct MixerConnection {
    pub cipher: Cipher,
    pub crypto_state: CryptoState,
    pub udp_rx: Sender<UdpRxMessage>,
    pub udp_tx: Sender<UdpTxMessage>,
}

pub enum MixerMessage {
    AddTrack(TrackContext),
    SetTrack(Option<TrackContext>),

    SetBitrate(Bitrate),
    SetConfig(Config),
    SetMute(bool),

    SetConn(MixerConnection, u32),
    Ws(Option<Sender<WsMessage>>),
    DropConn,

    ReplaceInterconnect(Interconnect),
    RebuildEncoder,

    Poison,
}

pub enum MixerInputResultMessage {
    CreateErr(Arc<AudioStreamError>),
    ParseErr(Arc<SymphoniaError>),
    Seek(
        Parsed,
        Option<Box<dyn Compose>>,
        Result<SeekedTo, Arc<SymphoniaError>>,
    ),
    Built(Parsed, Option<Box<dyn Compose>>),
}
