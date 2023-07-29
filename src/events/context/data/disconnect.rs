use crate::{
    error::ConnectionError,
    id::*,
    model::{CloseCode as VoiceCloseCode, FromPrimitive},
    ws::Error as WsError,
};
use async_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

/// Voice connection details gathered at termination or failure.
///
/// In the event of a failure, this event data is gathered after
/// a reconnection strategy has exhausted all of its attempts.
#[derive(Debug)]
#[non_exhaustive]
pub struct DisconnectData<'a> {
    /// The location that a voice connection was terminated.
    pub kind: DisconnectKind,
    /// The cause of any connection failure.
    ///
    /// If `None`, then this disconnect was requested by the user in some way
    /// (i.e., leaving or changing voice channels).
    pub reason: Option<DisconnectReason>,
    /// ID of the voice channel being joined, if it is known.
    ///
    /// If this is available, then this can be used to reconnect/renew
    /// a voice session via thew gateway.
    pub channel_id: Option<ChannelId>,
    /// ID of the target voice channel's parent guild.
    pub guild_id: GuildId,
    /// Unique string describing this session for validation/authentication purposes.
    pub session_id: &'a str,
}

/// The location that a voice connection was terminated.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum DisconnectKind {
    /// The voice driver failed to connect to the server.
    ///
    /// This requires explicit handling at the gateway level
    /// to either reconnect or fully disconnect.
    Connect,
    /// The voice driver failed to reconnect to the server.
    ///
    /// This requires explicit handling at the gateway level
    /// to either reconnect or fully disconnect.
    Reconnect,
    /// The voice connection was terminated mid-session by either
    /// the user or Discord.
    ///
    /// If `reason == None`, then this disconnection is either
    /// a full disconnect or a user-requested channel change.
    /// Otherwise, this is likely a session expiry (requiring user
    /// handling to fully disconnect/reconnect).
    Runtime,
}

/// The reason that a voice connection failed.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DisconnectReason {
    /// This (re)connection attempt was dropped due to another request.
    AttemptDiscarded,
    /// Songbird had an internal error.
    ///
    /// This should never happen; if this is ever seen, raise an issue with logs.
    Internal,
    /// A host-specific I/O error caused the fault; this is likely transient, and
    /// should be retried some time later.
    Io,
    /// Songbird and Discord disagreed on the protocol used to establish a
    /// voice connection.
    ///
    /// This should never happen; if this is ever seen, raise an issue with logs.
    ProtocolViolation,
    /// A voice connection was not established in the specified time.
    TimedOut,
    /// The Websocket connection was closed by Discord.
    ///
    /// This typically indicates that the voice session has expired,
    /// and a new one needs to be requested via the gateway.
    WsClosed(Option<VoiceCloseCode>),
}

impl From<&ConnectionError> for DisconnectReason {
    fn from(e: &ConnectionError) -> Self {
        use ConnectionError::*;

        match e {
            AttemptDiscarded => Self::AttemptDiscarded,
            CryptoModeInvalid
            | CryptoModeUnavailable
            | EndpointUrl
            | IllegalDiscoveryResponse
            | IllegalIp
            | Json(_) => Self::ProtocolViolation,
            Io(_) => Self::Io,
            Crypto(_) | InterconnectFailure(_) | InvalidLength(_) => Self::Internal,
            Ws(ws) => ws.into(),
            TimedOut => Self::TimedOut,
        }
    }
}

impl From<&WsError> for DisconnectReason {
    fn from(e: &WsError) -> Self {
        Self::WsClosed(match e {
            WsError::WsClosed(Some(frame)) => match frame.code {
                CloseCode::Library(l) => VoiceCloseCode::from_u16(l),
                _ => None,
            },
            _ => None,
        })
    }
}
