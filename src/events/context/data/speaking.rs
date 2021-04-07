#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
/// Speaking state transition, describing whether a given source has started/stopped
/// transmitting. This fires in response to a silent burst, or the first packet
/// breaking such a burst.
pub struct SpeakingUpdateData {
    /// Whether this user is currently speaking.
    pub speaking: bool,
    /// Synchronisation Source of the user who has begun speaking.
    ///
    /// This must be combined with another event class to map this back to
    /// its original UserId.
    pub ssrc: u32,
}
