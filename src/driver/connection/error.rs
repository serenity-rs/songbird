//! Connection errors and convenience types.

use crate::{
    driver::tasks::{error::Recipient, message::*},
    ws::Error as WsError,
};
use crypto_secretbox::{cipher::InvalidLength, Error as CryptoError};
use flume::SendError;
use serde_json::Error as JsonError;
use std::{error::Error as StdError, fmt, io::Error as IoError};
use tokio::time::error::Elapsed;

/// Errors encountered while connecting to a Discord voice server over the driver.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The driver hung up an internal signaller, either due to another connection attempt
    /// or a crash.
    AttemptDiscarded,
    /// An error occurred during [en/de]cryption of voice packets or key generation.
    Crypto(CryptoError),
    /// Invalid length error while generating crypto keys
    InvalidLength(InvalidLength),
    /// Server did not return the expected crypto mode during negotiation.
    CryptoModeInvalid,
    /// Selected crypto mode was not offered by server.
    CryptoModeUnavailable,
    /// An indicator that an endpoint URL was invalid.
    EndpointUrl,
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

impl From<InvalidLength> for Error {
    fn from(value: InvalidLength) -> Self {
        Error::InvalidLength(value)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to connect to Discord RTP server: ")?;
        use Error::*;
        match self {
            AttemptDiscarded => write!(f, "connection attempt was aborted/discarded"),
            Crypto(e) => e.fmt(f),
            InvalidLength(e) => e.fmt(f),
            CryptoModeInvalid => write!(f, "server changed negotiated encryption mode"),
            CryptoModeUnavailable => write!(f, "server did not offer chosen encryption mode"),
            EndpointUrl => write!(f, "endpoint URL received from gateway was invalid"),
            IllegalDiscoveryResponse => write!(f, "IP discovery/NAT punching response was invalid"),
            IllegalIp => write!(f, "IP discovery/NAT punching response had bad IP value"),
            Io(e) => e.fmt(f),
            Json(e) => e.fmt(f),
            InterconnectFailure(e) => write!(f, "failed to contact other task ({:?})", e),
            Ws(e) => write!(f, "websocket issue ({:?}).", e),
            TimedOut => write!(f, "connection attempt timed out"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Error::AttemptDiscarded => None,
            Error::Crypto(e) => e.source(),
            Error::InvalidLength(v) => v.source(),
            Error::CryptoModeInvalid => None,
            Error::CryptoModeUnavailable => None,
            Error::EndpointUrl => None,
            Error::IllegalDiscoveryResponse => None,
            Error::IllegalIp => None,
            Error::Io(e) => e.source(),
            Error::Json(e) => e.source(),
            Error::InterconnectFailure(_) => None,
            Error::Ws(_) => None,
            Error::TimedOut => None,
        }
    }
}

/// Convenience type for Discord voice/driver connection error handling.
pub type Result<T> = std::result::Result<T, Error>;
