/// Track events correspond to certain actions or changes
/// of state, such as a track finishing, looping, or being
/// manually stopped. Voice core events occur on receipt of
/// voice packets and telemetry.
///
/// Track events persist while the `action` in [`EventData`]
/// returns `None`.
///
/// [`EventData`]: super::EventData
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum TrackEvent {
    /// The attached track has resumed playing.
    ///
    /// This event will not fire when a track first starts,
    /// but will fire when a track changes from, e.g., paused to playing.
    /// This is most relevant for queue users.
    Play,
    /// The attached track has been paused.
    Pause,
    /// The attached track has ended.
    End,
    /// The attached track has looped.
    Loop,
}
