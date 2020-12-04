//! Driver and gateway error handling.

#[cfg(feature = "serenity")]
use futures::channel::mpsc::TrySendError;
#[cfg(feature = "serenity")]
use serenity::gateway::InterMessage;
#[cfg(feature = "gateway")]
use std::{error::Error, fmt};
#[cfg(feature = "twilight")]
use twilight_gateway::shard::CommandError;

#[cfg(feature = "gateway")]
#[derive(Debug)]
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
    #[cfg(feature = "driver")]
    /// The driver failed to establish a voice connection.
    Driver(ConnectionError),
    #[cfg(feature = "serenity")]
    /// Serenity-specific WebSocket send error.
    Serenity(TrySendError<InterMessage>),
    #[cfg(feature = "twilight")]
    /// Twilight-specific WebSocket send error.
    Twilight(CommandError),
}

#[cfg(feature = "gateway")]
impl fmt::Display for JoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Failed to Join Voice channel: ")?;
        match self {
            JoinError::Dropped => write!(f, "request was cancelled/dropped."),
            JoinError::NoSender => write!(f, "no gateway destination."),
            JoinError::NoCall => write!(f, "tried to leave a non-existent call."),
            #[cfg(feature = "driver")]
            JoinError::Driver(t) => write!(f, "internal driver error {}.", t),
            #[cfg(feature = "serenity")]
            JoinError::Serenity(t) => write!(f, "serenity failure {}.", t),
            #[cfg(feature = "twilight")]
            JoinError::Twilight(t) => write!(f, "twilight failure {}.", t),
        }
    }
}

#[cfg(feature = "gateway")]
impl Error for JoinError {}

#[cfg(all(feature = "serenity", feature = "gateway"))]
impl From<TrySendError<InterMessage>> for JoinError {
    fn from(e: TrySendError<InterMessage>) -> Self {
        JoinError::Serenity(e)
    }
}

#[cfg(all(feature = "twilight", feature = "gateway"))]
impl From<CommandError> for JoinError {
    fn from(e: CommandError) -> Self {
        JoinError::Twilight(e)
    }
}

#[cfg(all(feature = "driver", feature = "gateway"))]
impl From<ConnectionError> for JoinError {
    fn from(e: ConnectionError) -> Self {
        JoinError::Driver(e)
    }
}

#[cfg(feature = "gateway")]
/// Convenience type for Discord gateway error handling.
pub type JoinResult<T> = Result<T, JoinError>;

#[cfg(feature = "driver")]
pub use crate::{
    driver::connection::error::{Error as ConnectionError, Result as ConnectionResult},
    tracks::{TrackError, TrackResult},
};
