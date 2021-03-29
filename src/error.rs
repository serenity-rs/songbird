//! Driver and gateway error handling.

#[cfg(feature = "serenity")]
use futures::channel::mpsc::TrySendError;
#[cfg(feature = "serenity")]
use serenity::gateway::InterMessage;
#[cfg(feature = "gateway-core")]
use std::{error::Error, fmt};
#[cfg(feature = "twilight")]
use twilight_gateway::shard::CommandError;

#[cfg(feature = "gateway-core")]
#[derive(Debug)]
#[non_exhaustive]
/// Error returned when a manager or call handler is
/// unable to send messages over Discord's gateway.
pub enum JoinError {
    /// Request to join was dropped, cancelled, or replaced.
    Dropped,
    /// No available gateway connection was provided to send
    /// voice state update messages.
    NoSender,
    /// Tried to leave a [`Call`] which was not found.
    ///
    /// [`Call`]: crate::Call
    NoCall,
    /// Connection details were not received from Discord in the
    /// time given in [the `Call`'s configuration].
    ///
    /// This can occur if a message is lost by the Discord client
    /// between restarts, or if Discord's gateway believes that
    /// this bot is still in the channel it attempts to join.
    ///
    /// *Users should `leave` the server on the gateway before
    /// re-attempting connection.*
    ///
    /// [the `Call`'s configuration]: crate::Config
    TimedOut,
    #[cfg(feature = "driver-core")]
    /// The driver failed to establish a voice connection.
    ///
    /// *Users should `leave` the server on the gateway before
    /// re-attempting connection.*
    Driver(ConnectionError),
    #[cfg(feature = "serenity")]
    /// Serenity-specific WebSocket send error.
    Serenity(TrySendError<InterMessage>),
    #[cfg(feature = "twilight")]
    /// Twilight-specific WebSocket send error.
    Twilight(CommandError),
}

#[cfg(feature = "gateway-core")]
impl JoinError {
    /// Indicates whether this failure may have left (or been
    /// caused by) Discord's gateway state being in an
    /// inconsistent state.
    ///
    /// Failure to `leave` before rejoining may cause further
    /// timeouts.
    pub fn should_leave_server(&self) -> bool {
        matches!(self, JoinError::TimedOut)
    }

    #[cfg(feature = "driver-core")]
    /// Indicates whether this failure can be reattempted via
    /// [`Driver::connect`] with retreived connection info.
    ///
    /// Failure to `leave` before rejoining may cause further
    /// timeouts.
    ///
    /// [`Driver::connect`]: crate::driver::Driver
    pub fn should_reconnect_driver(&self) -> bool {
        matches!(self, JoinError::Driver(_))
    }
}

#[cfg(feature = "gateway-core")]
impl fmt::Display for JoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Failed to Join Voice channel: ")?;
        match self {
            JoinError::Dropped => write!(f, "request was cancelled/dropped."),
            JoinError::NoSender => write!(f, "no gateway destination."),
            JoinError::NoCall => write!(f, "tried to leave a non-existent call."),
            JoinError::TimedOut => write!(f, "gateway response from Discord timed out."),
            #[cfg(feature = "driver-core")]
            JoinError::Driver(t) => write!(f, "internal driver error {}.", t),
            #[cfg(feature = "serenity")]
            JoinError::Serenity(t) => write!(f, "serenity failure {}.", t),
            #[cfg(feature = "twilight")]
            JoinError::Twilight(t) => write!(f, "twilight failure {}.", t),
        }
    }
}

#[cfg(feature = "gateway-core")]
impl Error for JoinError {}

#[cfg(all(feature = "serenity", feature = "gateway-core"))]
impl From<TrySendError<InterMessage>> for JoinError {
    fn from(e: TrySendError<InterMessage>) -> Self {
        JoinError::Serenity(e)
    }
}

#[cfg(all(feature = "twilight", feature = "gateway-core"))]
impl From<CommandError> for JoinError {
    fn from(e: CommandError) -> Self {
        JoinError::Twilight(e)
    }
}

#[cfg(all(feature = "driver-core", feature = "gateway-core"))]
impl From<ConnectionError> for JoinError {
    fn from(e: ConnectionError) -> Self {
        JoinError::Driver(e)
    }
}

#[cfg(feature = "gateway-core")]
/// Convenience type for Discord gateway error handling.
pub type JoinResult<T> = Result<T, JoinError>;

#[cfg(feature = "driver-core")]
pub use crate::{
    driver::connection::error::{Error as ConnectionError, Result as ConnectionResult},
    tracks::{TrackError, TrackResult},
};
