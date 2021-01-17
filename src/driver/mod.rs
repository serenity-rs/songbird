//! Runner for a voice connection.
//!
//! Songbird's driver is a mixed-sync system, using:
//!  * Asynchronous connection management, event-handling, and gateway integration.
//!  * Synchronous audio mixing, packet generation, and encoding.
//!
//! This splits up work according to its IO/compute bound nature, preventing packet
//! generation from being slowed down past its deadline, or from affecting other
//! asynchronous tasks your bot must handle.

#[cfg(feature = "internals")]
pub mod bench_internals;

mod config;
pub(crate) mod connection;
mod crypto;
mod decode_mode;
pub(crate) mod tasks;

pub use config::Config;
use connection::error::{Error, Result};
pub use crypto::CryptoMode;
pub(crate) use crypto::CryptoState;
pub use decode_mode::DecodeMode;

#[cfg(feature = "builtin-queue")]
use crate::tracks::TrackQueue;
use crate::{
    events::EventData,
    input::Input,
    tracks::{self, Track, TrackHandle},
    ConnectionInfo,
    Event,
    EventHandler,
};
use audiopus::Bitrate;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use flume::{r#async::RecvFut, SendError, Sender};
use tasks::message::CoreMessage;
use tracing::instrument;

/// The control object for a Discord voice connection, handling connection,
/// mixing, encoding, en/decryption, and event generation.
///
/// When compiled with the `"builtin-queue"` feature, each driver includes a track queue
/// as a convenience to prevent the additional overhead of per-guild state management.
#[derive(Clone, Debug)]
pub struct Driver {
    config: Config,
    self_mute: bool,
    sender: Sender<CoreMessage>,
    #[cfg(feature = "builtin-queue")]
    queue: TrackQueue,
}

impl Driver {
    /// Creates a new voice driver.
    ///
    /// This will create the core voice tasks in the background.
    #[inline]
    pub fn new(config: Config) -> Self {
        let sender = Self::start_inner(config.clone());

        Driver {
            config,
            self_mute: false,
            sender,
            #[cfg(feature = "builtin-queue")]
            queue: Default::default(),
        }
    }

    fn start_inner(config: Config) -> Sender<CoreMessage> {
        let (tx, rx) = flume::unbounded();

        tasks::start(config, rx, tx.clone());

        tx
    }

    fn restart_inner(&mut self) {
        self.sender = Self::start_inner(self.config.clone());

        self.mute(self.self_mute);
    }

    /// Connects to a voice channel using the specified server.
    ///
    /// This method instantly contacts the driver tasks, and its
    /// does not need to be `await`ed to start the actual connection.
    #[instrument(skip(self))]
    pub fn connect(&mut self, info: ConnectionInfo) -> Connect {
        let (tx, rx) = flume::bounded(1);

        self.raw_connect(info, tx);

        Connect {
            inner: rx.into_recv_async(),
        }
    }

    /// Connects to a voice channel using the specified server.
    #[instrument(skip(self))]
    pub(crate) fn raw_connect(&mut self, info: ConnectionInfo, tx: Sender<Result<()>>) {
        self.send(CoreMessage::ConnectWithResult(info, tx));
    }

    /// Leaves the current voice channel, disconnecting from it.
    ///
    /// This does *not* forget settings, like whether to be self-deafened or
    /// self-muted.
    #[instrument(skip(self))]
    pub fn leave(&mut self) {
        self.send(CoreMessage::Disconnect);
    }

    /// Sets whether the current connection is to be muted.
    ///
    /// If there is no live voice connection, then this only acts as a settings
    /// update for future connections.
    #[instrument(skip(self))]
    pub fn mute(&mut self, mute: bool) {
        self.self_mute = mute;
        self.send(CoreMessage::Mute(mute));
    }

    /// Returns whether the driver is muted (i.e., processes audio internally
    /// but submits none).
    #[instrument(skip(self))]
    pub fn is_mute(&self) -> bool {
        self.self_mute
    }

    /// Plays audio from a source, returning a handle for further control.
    ///
    /// This can be a source created via [`ffmpeg`] or [`ytdl`].
    ///
    /// [`ffmpeg`]: crate::input::ffmpeg
    /// [`ytdl`]: crate::input::ytdl
    #[instrument(skip(self))]
    pub fn play_source(&mut self, source: Input) -> TrackHandle {
        let (player, handle) = super::create_player(source);
        self.send(CoreMessage::AddTrack(player));

        handle
    }

    /// Plays audio from a source, returning a handle for further control.
    ///
    /// Unlike [`play_source`], this stops all other sources attached
    /// to the channel.
    ///
    /// [`play_source`]: Driver::play_source
    #[instrument(skip(self))]
    pub fn play_only_source(&mut self, source: Input) -> TrackHandle {
        let (player, handle) = super::create_player(source);
        self.send(CoreMessage::SetTrack(Some(player)));

        handle
    }

    /// Plays audio from a [`Track`] object.
    ///
    /// This will be one half of the return value of [`create_player`].
    /// The main difference between this function and [`play_source`] is
    /// that this allows for direct manipulation of the [`Track`] object
    /// before it is passed over to the voice and mixing contexts.
    ///
    /// [`create_player`]: crate::tracks::create_player
    /// [`create_player`]: crate::tracks::Track
    /// [`play_source`]: Driver::play_source
    #[instrument(skip(self))]
    pub fn play(&mut self, track: Track) {
        self.send(CoreMessage::AddTrack(track));
    }

    /// Exclusively plays audio from a [`Track`] object.
    ///
    /// This will be one half of the return value of [`create_player`].
    /// As in [`play_only_source`], this stops all other sources attached to the
    /// channel. Like [`play`], however, this allows for direct manipulation of the
    /// [`Track`] object before it is passed over to the voice and mixing contexts.
    ///
    /// [`create_player`]: crate::tracks::create_player
    /// [`Track`]: crate::tracks::Track
    /// [`play_only_source`]: Driver::play_only_source
    /// [`play`]: Driver::play
    #[instrument(skip(self))]
    pub fn play_only(&mut self, track: Track) {
        self.send(CoreMessage::SetTrack(Some(track)));
    }

    /// Sets the bitrate for encoding Opus packets sent along
    /// the channel being managed.
    ///
    /// The default rate is 128 kbps.
    /// Sensible values range between `Bits(512)` and `Bits(512_000)`
    /// bits per second.
    /// Alternatively, `Auto` and `Max` remain available.
    #[instrument(skip(self))]
    pub fn set_bitrate(&mut self, bitrate: Bitrate) {
        self.send(CoreMessage::SetBitrate(bitrate))
    }

    /// Stops playing audio from all sources, if any are set.
    #[instrument(skip(self))]
    pub fn stop(&mut self) {
        self.send(CoreMessage::SetTrack(None))
    }

    /// Sets the configuration for this driver.
    #[instrument(skip(self))]
    pub fn set_config(&mut self, config: Config) {
        self.config = config.clone();
        self.send(CoreMessage::SetConfig(config))
    }

    /// Attach a global event handler to an audio context. Global events may receive
    /// any [`EventContext`].
    ///
    /// Global timing events will tick regardless of whether audio is playing,
    /// so long as the bot is connected to a voice channel, and have no tracks.
    /// [`TrackEvent`]s will respond to all relevant tracks, giving some audio elements.
    ///
    /// Users **must** ensure that no costly work or blocking occurs
    /// within the supplied function or closure. *Taking excess time could prevent
    /// timely sending of packets, causing audio glitches and delays*.
    ///
    /// [`Track`]: crate::tracks::Track
    /// [`TrackEvent`]: crate::events::TrackEvent
    /// [`EventContext`]: crate::events::EventContext
    #[instrument(skip(self, action))]
    pub fn add_global_event<F: EventHandler + 'static>(&mut self, event: Event, action: F) {
        self.send(CoreMessage::AddEvent(EventData::new(event, action)));
    }

    /// Removes all global event handlers from an audio context.
    #[instrument(skip(self))]
    pub fn remove_all_global_events(&mut self) {
        self.send(CoreMessage::RemoveGlobalEvents);
    }

    /// Sends a message to the inner tasks, restarting it if necessary.
    fn send(&mut self, status: CoreMessage) {
        // Restart thread if it errored.
        if let Err(SendError(status)) = self.sender.send(status) {
            self.restart_inner();

            self.sender.send(status).unwrap();
        }
    }
}

#[cfg(feature = "builtin-queue")]
impl Driver {
    /// Returns a reference to this driver's built-in queue.
    ///
    /// Requires the `"builtin-queue"` feature.
    /// Queue additions should be made via [`enqueue`] and
    /// [`enqueue_source`].
    ///
    /// [`enqueue`]: Driver::enqueue
    /// [`enqueue_source`]: Driver::enqueue_source
    pub fn queue(&self) -> &TrackQueue {
        &self.queue
    }

    /// Adds an audio [`Input`] to this driver's built-in queue.
    ///
    /// Requires the `"builtin-queue"` feature.
    ///
    /// [`Input`]: crate::input::Input
    pub fn enqueue_source(&mut self, source: Input) {
        let (mut track, _) = tracks::create_player(source);
        self.queue.add_raw(&mut track);
        self.play(track);
    }

    /// Adds an existing [`Track`] to this driver's built-in queue.
    ///
    /// Requires the `"builtin-queue"` feature.
    ///
    /// [`Track`]: crate::tracks::Track
    pub fn enqueue(&mut self, mut track: Track) {
        self.queue.add_raw(&mut track);
        self.play(track);
    }
}

impl Default for Driver {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl Drop for Driver {
    /// Leaves the current connected voice channel, if connected to one, and
    /// forgets all configurations relevant to this Handler.
    fn drop(&mut self) {
        self.leave();
        let _ = self.sender.send(CoreMessage::Poison);
    }
}

/// Future for a call to [`Driver::connect`].
///
/// This future awaits the *result* of a connection; the driver
/// is messaged at the time of the call.
///
/// [`Driver::connect`]: Driver::connect
pub struct Connect {
    inner: RecvFut<'static, Result<()>>,
}

impl Future for Connect {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.inner).poll(cx) {
            Poll::Ready(r) => Poll::Ready(r.map_err(|_| Error::AttemptDiscarded).and_then(|x| x)),
            Poll::Pending => Poll::Pending,
        }
    }
}
