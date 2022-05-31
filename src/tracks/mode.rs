use crate::events::TrackEvent;

/// Playback status of a track.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PlayMode {
    /// The track is currently playing.
    Play,
    /// The track is currently paused, and may be resumed.
    Pause,
    /// The track has been manually stopped, and cannot be restarted.
    Stop,
    /// The track has naturally ended, and cannot be restarted.
    End,
}

impl PlayMode {
    /// Returns whether the track has irreversibly stopped.
    #[must_use]
    pub fn is_done(self) -> bool {
        matches!(self, PlayMode::Stop | PlayMode::End)
    }

    #[must_use]
    pub(crate) fn next_state(self, other: Self) -> PlayMode {
        use PlayMode::*;

        // Idea: a finished track cannot be restarted -- this action is final.
        // We may want to change this in future so that seekable tracks can uncancel
        // themselves, perhaps, but this requires a bit more machinery to readd...
        match self {
            Play | Pause => other,
            state => state,
        }
    }

    pub(crate) fn change_to(&mut self, other: Self) {
        *self = self.next_state(other);
    }

    #[must_use]
    pub(crate) fn as_track_event(self) -> TrackEvent {
        use PlayMode::*;
        match self {
            Play => TrackEvent::Play,
            Pause => TrackEvent::Pause,
            Stop | End => TrackEvent::End,
        }
    }
}

impl Default for PlayMode {
    fn default() -> Self {
        PlayMode::Play
    }
}
