use std::{error::Error, fmt};

/// Errors associated with control and manipulation of tracks.
///
/// Unless otherwise stated, these don't invalidate an existing track,
/// but do advise on valid operations and commands.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TrackError {
    /// The operation failed because the track has ended, has been removed
    /// due to call closure, or some error within the driver.
    Finished,
    /// The supplied event listener can never be fired by a track, and should
    /// be attached to the driver instead.
    InvalidTrackEvent,
    /// The track's underlying [`Input`] doesn't support seeking operations.
    ///
    /// [`Input`]: crate::input::Input
    SeekUnsupported,
}

impl fmt::Display for TrackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to operate on track (handle): ")?;
        match self {
            TrackError::Finished => write!(f, "track ended"),
            TrackError::InvalidTrackEvent =>
                write!(f, "given event listener can't be fired on a track"),
            TrackError::SeekUnsupported => write!(f, "track did not support seeking"),
        }
    }
}

impl Error for TrackError {}

/// Alias for most calls to a [`TrackHandle`].
///
/// [`TrackHandle`]: super::TrackHandle
pub type TrackResult<T> = Result<T, TrackError>;
