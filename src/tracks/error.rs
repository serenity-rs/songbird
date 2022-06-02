use crate::input::AudioStreamError;
use std::{error::Error, fmt, sync::Arc};
use symphonia_core::errors::Error as SymphoniaError;

/// Errors associated with control and manipulation of tracks.
///
/// Unless otherwise stated, these don't invalidate an existing track,
/// but do advise on valid operations and commands.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ControlError {
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

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to operate on track (handle): ")?;
        match self {
            ControlError::Finished => write!(f, "track ended"),
            ControlError::InvalidTrackEvent => {
                write!(f, "given event listener can't be fired on a track")
            },
            ControlError::SeekUnsupported => write!(f, "track did not support seeking"),
        }
    }
}

impl Error for ControlError {}

/// Alias for most calls to a [`TrackHandle`].
///
/// [`TrackHandle`]: super::TrackHandle
pub type TrackResult<T> = Result<T, ControlError>;

#[allow(missing_docs)]
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum PlayError {
    Create(Arc<AudioStreamError>),
    Parse(Arc<SymphoniaError>),
    Decode(Arc<SymphoniaError>),
    Seek(Arc<SymphoniaError>),
}
