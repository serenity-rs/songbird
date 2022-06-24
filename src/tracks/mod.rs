//! Live, controllable audio instances.
//!
//! Tracks add control and event data around the bytestreams offered by [`Input`],
//! where each represents a live audio source inside of the driver's mixer.
//!
//! To prevent locking and stalling of the driver, tracks are controlled from your bot using a
//! [`TrackHandle`]. These handles remotely send commands from your bot's (a)sync
//! context to control playback, register events, and execute synchronous closures.
//!
//! If you want a new track from an [`Input`], i.e., for direct control before
//! playing your source on the driver, use [`create_player`].
//!
//! [`Input`]: ../input/struct.Input.html
//! [`TrackHandle`]: struct.TrackHandle.html
//! [`create_player`]: fn.create_player.html

mod action;
mod command;
mod error;
mod handle;
mod looping;
mod mode;
mod queue;
mod ready;
mod state;
mod view;

pub use self::{
    action::*,
    error::*,
    handle::*,
    looping::*,
    mode::*,
    queue::*,
    ready::*,
    state::*,
    view::*,
};
pub(crate) use command::*;

use crate::{constants::*, driver::tasks::message::*, events::EventStore, input::Input};
use std::time::Duration;
use uuid::Uuid;

/// Initial state for audio playback.
///
/// [`Track`]s allow you to configure play modes, volume, event handlers, and other track state
/// before you pass an input to the [`Driver`].
///
/// Live track data is accesseed via a [`TrackHandle`], returned by [`Driver::play`] and
/// related methods.
///
/// # Example
///
/// ```rust,no_run
/// use songbird::{driver::Driver, input::File, tracks::Track};
///
/// # async {
/// // A Call is also valid here!
/// let mut handler: Driver = Default::default();
/// let source = File::new("../audio/my-favourite-song.mp3");
///
/// handler.play_only(Track::new(source.into()).volume(0.5));
///
/// // Future access occurs via audio_handle.
/// # };
/// ```
///
/// [`Driver`]: crate::driver::Driver
/// [`Driver::play`]: crate::driver::Driver::play
pub struct Track {
    /// Whether or not this sound is currently playing.
    ///
    /// Defaults to [`PlayMode::Play`].
    pub playing: PlayMode,

    /// The volume for playback.
    ///
    /// Sensible values fall between `0.0` and `1.0`. Values outside this range can
    /// cause clipping or other audio artefacts.
    ///
    /// Defaults to `1.0`.
    pub volume: f32,

    /// The live or lazily-initialised audio stream to be played.
    pub input: Input,

    /// List of events attached to this audio track.
    ///
    /// This may be used to add additional events to a track
    /// before it is sent to the audio context for playing.
    ///
    /// Defaults to an empty set.
    pub events: EventStore,

    /// Count of remaining loops.
    ///
    /// Defaults to play a track once (i.e., [`LoopState::Finite(0)`]).
    ///
    /// [`LoopState::Finite(0)`]: LoopState::Finite
    pub loops: LoopState,

    /// Unique identifier for this track.
    ///
    /// Defaults to a random 128-bit number.
    pub uuid: Uuid,
}

impl Track {
    /// Create a new track directly from an [`Input`] and a random [`Uuid`].
    #[must_use]
    pub fn new(input: Input) -> Self {
        let uuid = Uuid::new_v4();

        Self::new_with_uuid(input, uuid)
    }

    /// Create a new track directly from an [`Input`] with a custom [`Uuid`].
    #[must_use]
    pub fn new_with_uuid(input: Input, uuid: Uuid) -> Self {
        Self {
            playing: PlayMode::default(),
            volume: 1.0,
            input,
            events: EventStore::new_local(),
            loops: LoopState::Finite(0),
            uuid,
        }
    }

    #[must_use]
    /// Sets a track to playing if it is paused.
    pub fn play(mut self) -> Self {
        self.playing = PlayMode::Play;
        self
    }

    #[must_use]
    /// Pre-emptively pauses a track, preventing it from being automatically played.
    pub fn pause(mut self) -> Self {
        self.playing = PlayMode::Pause;
        self
    }

    #[must_use]
    /// Manually stops a track.
    ///
    /// This will cause the audio track to be removed by the driver almost immediately,
    /// with any relevant events triggered.
    pub fn stop(mut self) -> Self {
        self.playing = PlayMode::Stop;
        self
    }

    #[must_use]
    /// Sets [`volume`] in a manner that allows method chaining.
    ///
    /// [`volume`]: Track::volume
    pub fn volume(mut self, volume: f32) -> Self {
        self.volume = volume;

        self
    }

    #[must_use]
    /// Set an audio track to loop a set number of times.
    pub fn loops(mut self, loops: LoopState) -> Self {
        self.loops = loops;

        self
    }

    #[must_use]
    /// Returns this track's unique identifier.
    pub fn uuid(mut self, uuid: Uuid) -> Self {
        self.uuid = uuid;

        self
    }

    pub(crate) fn into_context(self) -> (TrackHandle, TrackContext) {
        let (tx, receiver) = flume::unbounded();
        let handle = TrackHandle::new(tx, self.uuid);

        let context = TrackContext {
            handle: handle.clone(),
            track: self,
            receiver,
        };

        (handle, context)
    }
}

/// Any [`Input`] (or struct which can be used as one) can also be made into a [`Track`].
impl<T: Into<Input>> From<T> for Track {
    // NOTE: this is `Into` to support user-given structs which can
    // only `impl Into<Input>`.
    fn from(val: T) -> Self {
        Track::new(val.into())
    }
}
