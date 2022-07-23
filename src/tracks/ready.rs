/// Whether this track has been made live, is being processed, or is
/// currently uninitialised.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ReadyState {
    /// This track is still a lazy [`Compose`] object, and hasn't been made playable.
    ///
    /// [`Compose`]: crate::input::Compose
    Uninitialised,

    /// The mixer is currently creating and parsing this track's bytestream.
    Preparing,

    /// This track is fully initialised and usable.
    Playable,
}

impl Default for ReadyState {
    fn default() -> Self {
        Self::Uninitialised
    }
}
