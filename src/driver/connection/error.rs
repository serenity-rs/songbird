//! Connection errors and convenience types.

use crate::{
    driver::tasks::{error::Recipient, message::*},
    ws::Error as WsError,
};
use flume::SendError;
use serde_json::Error as JsonError;
use std::{error::Error as StdError, fmt, io::Error as IoError};
use tokio::time::error::Elapsed;
use xsalsa20poly1305::aead::Error as CryptoError;

/// Errors encountered while connecting to a Discord voice server over the driver.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The driver hung up an internal signaller, either due to another connection attempt
    /// or a crash.
    AttemptDiscarded,
    /// An error occurred during [en/de]cryption of voice packets or key generation.
    Crypto(CryptoError),
    /// Server did not return the expected crypto mode during negotiation.
    CryptoModeInvalid,
    /// Selected crypto mode was not offered by server.
    CryptoModeUnavailable,
    /// An indicator that an endpoint URL was invalid.
    EndpointUrl,
    /// Discord hello/ready handshake was violated.
    ExpectedHandshake,
    /// Discord failed to correctly respond to IP discovery.
    IllegalDiscoveryResponse,
    /// Could not parse Discord's view of our IP.
    IllegalIp,
    /// Miscellaneous I/O error.
    Io(IoError),
    /// JSON (de)serialization error.
    Json(JsonError),
    /// Failed to message other background tasks after connection establishment.
    InterconnectFailure(Recipient),
    /// Error communicating with gateway server over WebSocket.
    Ws(WsError),
    /// Connection attempt timed out.
    TimedOut,
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

impl From<JsonError> for Error {
    fn from(e: JsonError) -> Error {
        Error::Json(e)
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

impl From<WsError> for Error {
    fn from(e: WsError) -> Error {
        Error::Ws(e)
    }
}

impl From<Elapsed> for Error {
    fn from(_e: Elapsed) -> Error {
        Error::TimedOut
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to connect to Discord RTP server: ")?;
        match self {
            Self::AttemptDiscarded => write!(f, "connection attempt was aborted/discarded"),
            Self::Crypto(e) => e.fmt(f),
            Self::CryptoModeInvalid => write!(f, "server changed negotiated encryption mode"),
            Self::CryptoModeUnavailable => write!(f, "server did not offer chosen encryption mode"),
            Self::EndpointUrl => write!(f, "endpoint URL received from gateway was invalid"),
            Self::ExpectedHandshake => write!(f, "voice initialisation protocol was violated"),
            Self::IllegalDiscoveryResponse =>
                write!(f, "IP discovery/NAT punching response was invalid"),
            Self::IllegalIp => write!(f, "IP discovery/NAT punching response had bad IP value"),
            Self::Io(e) => e.fmt(f),
            Self::Json(e) => e.fmt(f),
            Self::InterconnectFailure(e) => write!(f, "failed to contact other task ({:?})", e),
            Self::Ws(e) => write!(f, "websocket issue ({:?}).", e),
            Self::TimedOut => write!(f, "connection attempt timed out"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Error::AttemptDiscarded
            | Error::CryptoModeInvalid
            | Error::CryptoModeUnavailable
            | Error::EndpointUrl
            | Error::ExpectedHandshake
            | Error::IllegalDiscoveryResponse
            | Error::IllegalIp
            | Error::InterconnectFailure(_)
            | Error::Ws(_)
            | Error::TimedOut => None,
            Error::Crypto(e) => e.source(),
            Error::Io(e) => e.source(),
            Error::Json(e) => e.source(),
        }
    }
}

/// Convenience type for Discord voice/driver connection error handling.
pub type Result<T> = std::result::Result<T, Error>;
