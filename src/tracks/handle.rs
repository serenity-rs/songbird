use super::*;
use crate::events::{Event, EventData, EventHandler};
use flume::Sender;
use std::{fmt, sync::Arc, time::Duration};
use tokio::sync::RwLock;
use typemap_rev::TypeMap;
use uuid::Uuid;

#[derive(Clone, Debug)]
/// Handle for safe control of a [`Track`] from other threads, outside
/// of the audio mixing and voice handling context.
///
/// These are cheap to clone, using `Arc<...>` internally.
///
/// Many method calls here are fallible; in most cases, this will be because
/// the underlying [`Track`] object has been discarded. Those which aren't refer
/// to immutable properties of the underlying stream, or shared data not used
/// by the driver.
///
/// [`Track`]: Track
pub struct TrackHandle {
    inner: Arc<InnerHandle>,
}

struct InnerHandle {
    command_channel: Sender<TrackCommand>,
    uuid: Uuid,
    typemap: RwLock<TypeMap>,
}

impl fmt::Debug for InnerHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InnerHandle")
            .field("command_channel", &self.command_channel)
            .field("uuid", &self.uuid)
            .field("typemap", &"<LOCK>")
            .finish()
    }
}

impl TrackHandle {
    /// Creates a new handle, using the given command sink.
    ///
    /// [`Input`]: crate::input::Input
    #[must_use]
    pub(crate) fn new(command_channel: Sender<TrackCommand>, uuid: Uuid) -> Self {
        let inner = Arc::new(InnerHandle {
            command_channel,
            uuid,
            typemap: RwLock::new(TypeMap::new()),
        });

        Self { inner }
    }

    /// Unpauses an audio track.
    pub fn play(&self) -> TrackResult<()> {
        self.send(TrackCommand::Play)
    }

    /// Pauses an audio track.
    pub fn pause(&self) -> TrackResult<()> {
        self.send(TrackCommand::Pause)
    }

    /// Stops an audio track.
    ///
    /// This is *final*, and will cause the audio context to fire
    /// a [`TrackEvent::End`] event.
    ///
    /// [`TrackEvent::End`]: crate::events::TrackEvent::End
    pub fn stop(&self) -> TrackResult<()> {
        self.send(TrackCommand::Stop)
    }

    /// Sets the volume of an audio track.
    pub fn set_volume(&self, volume: f32) -> TrackResult<()> {
        self.send(TrackCommand::Volume(volume))
    }

    /// Ready a track for playing if it is lazily initialised.
    pub fn make_playable(&self) -> TrackResult<()> {
        self.send(TrackCommand::MakePlayable)
    }

    /// Seeks along the track to the specified position.
    ///
    /// If the underlying [`Input`] does not support seeking,
    /// forward seeks will succeed. Backward seeks will recreate the
    /// track using the lazy [`Compose`] if present.
    ///
    /// [`Input`]: crate::input::Input
    /// [`Compose`]: crate::input::Compose
    pub fn seek_time(&self, position: Duration) -> TrackResult<()> {
        self.send(TrackCommand::Seek(position))
    }

    /// Attach an event handler to an audio track. These will receive [`EventContext::Track`].
    ///
    /// Events which can only be fired by the global context return [`ControlError::InvalidTrackEvent`]
    ///
    /// [`EventContext::Track`]: crate::events::EventContext::Track
    pub fn add_event<F: EventHandler + 'static>(&self, event: Event, action: F) -> TrackResult<()> {
        let cmd = TrackCommand::AddEvent(EventData::new(event, action));
        if event.is_global_only() {
            Err(ControlError::InvalidTrackEvent)
        } else {
            self.send(cmd)
        }
    }

    /// Perform an arbitrary synchronous action on a raw [`Track`] object.
    ///
    /// This will give access to a [`View`] of the current track state and [`Metadata`],
    /// which can be used to take an [`Action`].
    ///
    /// Users **must** ensure that no costly work or blocking occurs
    /// within the supplied function or closure. *Taking excess time could prevent
    /// timely sending of packets, causing audio glitches and delays*.
    ///
    /// [`Metadata`]: crate::input::Metadata
    pub fn action<F>(&self, action: F) -> TrackResult<()>
    where
        F: FnOnce(View) -> Option<Action> + Send + Sync + 'static,
    {
        self.send(TrackCommand::Do(Box::new(action)))
    }

    /// Request playback information and state from the audio context.
    pub async fn get_info(&self) -> TrackResult<TrackState> {
        let (tx, rx) = flume::bounded(1);
        self.send(TrackCommand::Request(tx))?;

        rx.recv_async().await.map_err(|_| ControlError::Finished)
    }

    /// Set an audio track to loop indefinitely.
    ///
    /// This requires either a [`Compose`] to be present or for the
    /// input stream to be seekable.
    ///
    /// [`Input`]: crate::input::Input
    /// [`Compose`]: crate::input::Compose
    pub fn enable_loop(&self) -> TrackResult<()> {
        self.send(TrackCommand::Loop(LoopState::Infinite))
    }

    /// Set an audio track to no longer loop.
    ///
    /// This follows the same rules as [`enable_loop`].
    ///
    /// [`enable_loop`]: Self::enable_loop
    pub fn disable_loop(&self) -> TrackResult<()> {
        self.send(TrackCommand::Loop(LoopState::Finite(0)))
    }

    /// Set an audio track to loop a set number of times.
    ///
    /// This follows the same rules as [`enable_loop`].
    ///
    /// [`enable_loop`]: Self::enable_loop
    pub fn loop_for(&self, count: usize) -> TrackResult<()> {
        self.send(TrackCommand::Loop(LoopState::Finite(count)))
    }

    /// Returns this handle's (and track's) unique identifier.
    #[must_use]
    pub fn uuid(&self) -> Uuid {
        self.inner.uuid
    }

    /// Allows access to this track's attached [`TypeMap`].
    ///
    /// [`TypeMap`]s allow additional, user-defined data shared by all handles
    /// to be attached to any track.
    ///
    /// Driver code will never attempt to lock access to this map,
    /// preventing deadlock/stalling.
    #[must_use]
    pub fn typemap(&self) -> &RwLock<TypeMap> {
        &self.inner.typemap
    }

    #[inline]
    /// Send a raw command to the [`Track`] object.
    ///
    /// [`Track`]: Track
    pub fn send(&self, cmd: TrackCommand) -> TrackResult<()> {
        // As the send channels are unbounded, we can be reasonably certain
        // that send failure == cancellation.
        self.inner
            .command_channel
            .send(cmd)
            .map_err(|_e| ControlError::Finished)
    }
}
