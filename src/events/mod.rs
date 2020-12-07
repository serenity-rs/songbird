//! Events relating to tracks, timing, and other callers.

mod context;
mod core;
mod data;
mod store;
mod track;
mod untimed;

pub use self::{context::EventContext, core::*, data::*, store::*, track::*, untimed::*};
pub(crate) use context::CoreContext;

use async_trait::async_trait;
use std::time::Duration;

/// Trait to handle an event which can be fired per-track, or globally.
///
/// These may be feasibly reused between several event sources.
#[async_trait]
pub trait EventHandler: Send + Sync {
    /// Respond to one received event.
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event>;
}

/// Classes of event which may occur, triggering a handler
/// at the local (track-specific) or global level.
///
/// Local time-based events rely upon the current playback
/// time of a track, and so will not fire if a track becomes paused
/// or stops. In case this is required, global events are a better
/// fit.
///
/// Event handlers themselves are described in [`EventData::action`].
///
/// [`EventData::action`]: EventData::action
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Event {
    /// Periodic events rely upon two parameters: a *period*
    /// and an optional *phase*.
    ///
    /// If the *phase* is `None`, then the event will first fire
    /// in one *period*. Periodic events repeat automatically
    /// so long as the `action` in [`EventData`] returns `None`.
    ///
    /// [`EventData`]: EventData
    Periodic(Duration, Option<Duration>),
    /// Delayed events rely upon a *delay* parameter, and
    /// fire one *delay* after the audio context processes them.
    ///
    /// Delayed events are automatically removed once fired,
    /// so long as the `action` in [`EventData`] returns `None`.
    ///
    /// [`EventData`]: EventData
    Delayed(Duration),
    /// Track events correspond to certain actions or changes
    /// of state, such as a track finishing, looping, or being
    /// manually stopped.
    ///
    /// Track events persist while the `action` in [`EventData`]
    /// returns `None`.
    ///
    /// [`EventData`]: EventData
    Track(TrackEvent),
    /// Core events
    ///
    /// Track events persist while the `action` in [`EventData`]
    /// returns `None`. Core events **must** be applied globally,
    /// as attaching them to a track is a no-op.
    ///
    /// [`EventData`]: EventData
    Core(CoreEvent),
    /// Cancels the event, if it was intended to persist.
    Cancel,
}

impl Event {
    pub(crate) fn is_global_only(&self) -> bool {
        matches!(self, Self::Core(_))
    }
}

impl From<TrackEvent> for Event {
    fn from(evt: TrackEvent) -> Self {
        Event::Track(evt)
    }
}

impl From<CoreEvent> for Event {
    fn from(evt: CoreEvent) -> Self {
        Event::Core(evt)
    }
}
