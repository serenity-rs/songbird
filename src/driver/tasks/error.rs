use super::message::*;
use crate::ws::Error as WsError;
use audiopus::Error as OpusError;
use flume::SendError;
use std::io::Error as IoError;
use xsalsa20poly1305::aead::Error as CryptoError;

#[derive(Debug)]
pub enum Recipient {
    AuxNetwork,
    Event,
    Mixer,
    #[cfg(feature = "receive")]
    UdpRx,
    UdpTx,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    Crypto(CryptoError),
    #[cfg(feature = "receive")]
    /// Received an illegal voice packet on the voice UDP socket.
    IllegalVoicePacket,
    InterconnectFailure(Recipient),
    Io(IoError),
    Opus(OpusError),
    Ws(WsError),
}

impl Error {
    pub(crate) fn should_trigger_connect(&self) -> bool {
        match self {
            Error::InterconnectFailure(Recipient::AuxNetwork | Recipient::UdpTx) => true,
            #[cfg(feature = "receive")]
            Error::InterconnectFailure(Recipient::UdpRx) => true,
            _ => false,
        }
    }

    pub(crate) fn should_trigger_interconnect_rebuild(&self) -> bool {
        matches!(self, Error::InterconnectFailure(Recipient::Event))
    }
}

impl From<CryptoError> for Error {
    fn from(e: CryptoError) -> Self {
        Error::Crypto(e)
    }
}

impl From<IoError> for Error {
    fn from(e: IoError) -> Error {
        Error::Io(e)
    }
}

impl From<OpusError> for Error {
    fn from(e: OpusError) -> Error {
        Error::Opus(e)
    }
}

impl From<SendError<WsMessage>> for Error {
    fn from(_e: SendError<WsMessage>) -> Error {
        Error::InterconnectFailure(Recipient::AuxNetwork)
    }
}

impl From<SendError<EventMessage>> for Error {
    fn from(_e: SendError<EventMessage>) -> Error {
        Error::InterconnectFailure(Recipient::Event)
    }
}

impl From<SendError<MixerMessage>> for Error {
    fn from(_e: SendError<MixerMessage>) -> Error {
        Error::InterconnectFailure(Recipient::Mixer)
    }
}

#[cfg(feature = "receive")]
impl From<SendError<UdpRxMessage>> for Error {
    fn from(_e: SendError<UdpRxMessage>) -> Error {
        Error::InterconnectFailure(Recipient::UdpRx)
    }
}

impl From<SendError<UdpTxMessage>> for Error {
    fn from(_e: SendError<UdpTxMessage>) -> Error {
        Error::InterconnectFailure(Recipient::UdpTx)
    }
}

impl From<WsError> for Error {
    fn from(e: WsError) -> Error {
        Error::Ws(e)
    }
}
