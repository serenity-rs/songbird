#![allow(missing_docs)]

use super::{Interconnect, TrackContext, UdpRxMessage, UdpTxMessage, WsMessage};

use crate::{
    driver::{Bitrate, Config, CryptoState},
    input::{AudioStreamError, Compose, Parsed},
};
use flume::Sender;
use symphonia_core::{errors::Error as SymphoniaError, formats::SeekedTo};
use xsalsa20poly1305::XSalsa20Poly1305 as Cipher;

pub struct MixerConnection {
    pub cipher: Cipher,
    pub crypto_state: CryptoState,
    pub udp_rx: Sender<UdpRxMessage>,
    pub udp_tx: Sender<UdpTxMessage>,
}

impl Drop for MixerConnection {
    fn drop(&mut self) {
        drop(self.udp_rx.send(UdpRxMessage::Poison));
        drop(self.udp_tx.send(UdpTxMessage::Poison));
    }
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
    CreateErr(AudioStreamError),
    ParseErr(SymphoniaError),
    Seek(
        Parsed,
        Option<Box<dyn Compose>>,
        Result<SeekedTo, SymphoniaError>,
    ),
    Built(Parsed, Option<Box<dyn Compose>>),
}
