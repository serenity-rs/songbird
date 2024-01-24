#![allow(missing_docs)]

#[cfg(feature = "receive")]
use super::UdpRxMessage;
use super::{Interconnect, TrackContext, WsMessage};

use crate::{
    driver::{Bitrate, Config, CryptoState},
    input::{AudioStreamError, Compose, Parsed},
};
use crypto_secretbox::XSalsa20Poly1305 as Cipher;
use flume::Sender;
use std::{net::UdpSocket, sync::Arc};
use symphonia_core::{errors::Error as SymphoniaError, formats::SeekedTo};

pub struct MixerConnection {
    pub cipher: Cipher,
    pub crypto_state: CryptoState,
    #[cfg(feature = "receive")]
    pub udp_rx: Sender<UdpRxMessage>,
    pub udp_tx: UdpSocket,
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

impl MixerMessage {
    pub fn is_mixer_maybe_live(&self) -> bool {
        matches!(
            self,
            Self::AddTrack(_) | Self::SetTrack(Some(_)) | Self::SetConn(..)
        )
    }
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
