use super::*;
use crate::{
    events::{Event, EventData, EventHandler},
    input::Metadata,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc::UnboundedSender, oneshot};
use uuid::Uuid;

#[derive(Clone, Debug)]
/// Handle for safe control of a [`Track`] track from other threads, outside
/// of the audio mixing and voice handling context.
///
/// Almost all method calls here are fallible; in most cases, this will be because
/// the underlying [`Track`] object has been discarded. Those which aren't refer
/// to immutable properties of the underlying stream.
///
/// [`Track`]: Track
pub struct TrackHandle {
    inner: Arc<InnerHandle>,
}

#[derive(Clone, Debug)]
struct InnerHandle {
    command_channel: UnboundedSender<TrackCommand>,
    seekable: bool,
    uuid: Uuid,
    metadata: Box<Metadata>,
}

impl TrackHandle {
    /// Creates a new handle, using the given command sink and hint as to whether
    /// the underlying [`Input`] supports seek operations.
    ///
    /// [`Input`]: crate::input::Input
    pub fn new(
        command_channel: UnboundedSender<TrackCommand>,
        seekable: bool,
        uuid: Uuid,
        metadata: Box<Metadata>,
    ) -> Self {
        let inner = Arc::new(InnerHandle {
            command_channel,
            seekable,
            uuid,
            metadata,
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
    ///
    /// Currently, only [`Restartable`] sources support lazy setup.
    /// This call is a no-op for all others.
    ///
    /// [`Restartable`]: crate::input::restartable::Restartable
    pub fn make_playable(&self) -> TrackResult<()> {
        self.send(TrackCommand::MakePlayable)
    }

    /// Denotes whether the underlying [`Input`] stream is compatible with arbitrary seeking.
    ///
    /// If this returns `false`, all calls to [`seek_time`] will fail, and the track is
    /// incapable of looping.
    ///
    /// [`seek_time`]: TrackHandle::seek_time
    /// [`Input`]: crate::input::Input
    pub fn is_seekable(&self) -> bool {
        self.inner.seekable
    }

    /// Seeks along the track to the specified position.
    ///
    /// If the underlying [`Input`] does not support seeking,
    /// then all calls will fail with [`TrackError::SeekUnsupported`].
    ///
    /// [`Input`]: crate::input::Input
    /// [`TrackError::SeekUnsupported`]: TrackError::SeekUnsupported
    pub fn seek_time(&self, position: Duration) -> TrackResult<()> {
        if self.is_seekable() {
            self.send(TrackCommand::Seek(position))
        } else {
            Err(TrackError::SeekUnsupported)
        }
    }

    /// Attach an event handler to an audio track. These will receive [`EventContext::Track`].
    ///
    /// Events which can only be fired by the global context return [`TrackError::InvalidTrackEvent`]
    ///
    /// [`Track`]: Track
    /// [`EventContext::Track`]: crate::events::EventContext::Track
    /// [`TrackError::InvalidTrackEvent`]: TrackError::InvalidTrackEvent
    pub fn add_event<F: EventHandler + 'static>(&self, event: Event, action: F) -> TrackResult<()> {
        let cmd = TrackCommand::AddEvent(EventData::new(event, action));
        if event.is_global_only() {
            Err(TrackError::InvalidTrackEvent)
        } else {
            self.send(cmd)
        }
    }

    /// Perform an arbitrary synchronous action on a raw [`Track`] object.
    ///
    /// Users **must** ensure that no costly work or blocking occurs
    /// within the supplied function or closure. *Taking excess time could prevent
    /// timely sending of packets, causing audio glitches and delays*.
    ///
    /// [`Track`]: Track
    pub fn action<F>(&self, action: F) -> TrackResult<()>
    where
        F: FnOnce(&mut Track) + Send + Sync + 'static,
    {
        self.send(TrackCommand::Do(Box::new(action)))
    }

    /// Request playback information and state from the audio context.
    pub async fn get_info(&self) -> TrackResult<Box<TrackState>> {
        let (tx, rx) = oneshot::channel();
        self.send(TrackCommand::Request(tx))?;

        rx.await.map_err(|_| TrackError::Finished)
    }

    /// Set an audio track to loop indefinitely.
    ///
    /// If the underlying [`Input`] does not support seeking,
    /// then all calls will fail with [`TrackError::SeekUnsupported`].
    ///
    /// [`Input`]: crate::input::Input
    /// [`TrackError::SeekUnsupported`]: TrackError::SeekUnsupported
    pub fn enable_loop(&self) -> TrackResult<()> {
        if self.is_seekable() {
            self.send(TrackCommand::Loop(LoopState::Infinite))
        } else {
            Err(TrackError::SeekUnsupported)
        }
    }

    /// Set an audio track to no longer loop.
    ///
    /// If the underlying [`Input`] does not support seeking,
    /// then all calls will fail with [`TrackError::SeekUnsupported`].
    ///
    /// [`Input`]: crate::input::Input
    /// [`TrackError::SeekUnsupported`]: TrackError::SeekUnsupported
    pub fn disable_loop(&self) -> TrackResult<()> {
        if self.is_seekable() {
            self.send(TrackCommand::Loop(LoopState::Finite(0)))
        } else {
            Err(TrackError::SeekUnsupported)
        }
    }

    /// Set an audio track to loop a set number of times.
    ///
    /// If the underlying [`Input`] does not support seeking,
    /// then all calls will fail with [`TrackError::SeekUnsupported`].
    ///
    /// [`Input`]: crate::input::Input
    /// [`TrackError::SeekUnsupported`]: TrackError::SeekUnsupported
    pub fn loop_for(&self, count: usize) -> TrackResult<()> {
        if self.is_seekable() {
            self.send(TrackCommand::Loop(LoopState::Finite(count)))
        } else {
            Err(TrackError::SeekUnsupported)
        }
    }

    /// Returns this handle's (and track's) unique identifier.
    pub fn uuid(&self) -> Uuid {
        self.inner.uuid
    }

    /// Returns the metadata stored in the handle.
    ///
    /// Metadata is cloned from the inner [`Input`] at
    /// the time a track/handle is created, and is effectively
    /// read-only from then on.
    ///
    /// [`Input`]: crate::input::Input
    pub fn metadata(&self) -> &Metadata {
        &self.inner.metadata
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
            .map_err(|_e| TrackError::Finished)
    }
}
