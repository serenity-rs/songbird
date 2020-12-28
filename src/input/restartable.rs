//! A source which supports seeking by recreating its input stream.
//!
//! This is intended for use with single-use audio tracks which
//! may require looping or seeking, but where additional memory
//! cannot be spared. Forward seeks will drain the track until reaching
//! the desired timestamp.
//!
//! Restarting occurs by temporarily pausing the track, running the restart
//! mechanism, and then passing the handle back to the mixer thread. Until
//! success/failure is confirmed, the track produces silence.

use super::*;
use async_trait::async_trait;
use flume::{Receiver, TryRecvError};
use std::{
    ffi::OsStr,
    fmt::{Debug, Error as FormatError, Formatter},
    io::{Error as IoError, ErrorKind as IoErrorKind, Read, Result as IoResult, Seek, SeekFrom},
    result::Result as StdResult,
    time::Duration,
};

type Recreator = Box<dyn Restart + Send + 'static>;
type RecreateChannel = Receiver<Result<(Box<Input>, Recreator)>>;

// Use options here to make "take" more doable from a mut ref.
enum LazyProgress {
    Dead(Box<Metadata>, Option<Recreator>, Codec, Container),
    Live(Box<Input>, Option<Recreator>),
    Working(Codec, Container, bool, RecreateChannel),
}

impl Debug for LazyProgress {
    fn fmt(&self, f: &mut Formatter<'_>) -> StdResult<(), FormatError> {
        match self {
            LazyProgress::Dead(meta, _, codec, container) => f
                .debug_tuple("Dead")
                .field(meta)
                .field(&"<fn>")
                .field(codec)
                .field(container)
                .finish(),
            LazyProgress::Live(input, _) =>
                f.debug_tuple("Live").field(input).field(&"<fn>").finish(),
            LazyProgress::Working(codec, container, stereo, chan) => f
                .debug_tuple("Working")
                .field(codec)
                .field(container)
                .field(stereo)
                .field(chan)
                .finish(),
        }
    }
}

/// A wrapper around a method to create a new [`Input`] which
/// seeks backward by recreating the source.
///
/// The main purpose of this wrapper is to enable seeking on
/// incompatible sources (i.e., ffmpeg output) and to ease resource
/// consumption for commonly reused/shared tracks. [`Compressed`]
/// and [`Memory`] offer the same functionality with different
/// tradeoffs.
///
/// This is intended for use with single-use audio tracks which
/// may require looping or seeking, but where additional memory
/// cannot be spared. Forward seeks will drain the track until reaching
/// the desired timestamp.
///
/// [`Input`]: Input
/// [`Memory`]: cached::Memory
/// [`Compressed`]: cached::Compressed
#[derive(Debug)]
pub struct Restartable {
    async_handle: Option<Handle>,
    position: usize,
    source: LazyProgress,
}

impl Restartable {
    /// Create a new source, which can be restarted using a `recreator` function.
    ///
    /// Lazy sources will not run their input recreator until the first byte
    /// is needed, or are sent [`Track::make_playable`]/[`TrackHandle::make_playable`].
    ///
    /// [`Track::make_playable`]: crate::tracks::Track::make_playable
    /// [`TrackHandle::make_playable`]: crate::tracks::TrackHandle::make_playable
    pub async fn new(mut recreator: impl Restart + Send + 'static, lazy: bool) -> Result<Self> {
        if lazy {
            recreator
                .lazy_init()
                .await
                .map(move |(meta, kind, codec)| Self {
                    async_handle: None,
                    position: 0,
                    source: LazyProgress::Dead(
                        meta.unwrap_or_default().into(),
                        Some(Box::new(recreator)),
                        kind,
                        codec,
                    ),
                })
        } else {
            recreator.call_restart(None).await.map(move |source| Self {
                async_handle: None,
                position: 0,
                source: LazyProgress::Live(source.into(), Some(Box::new(recreator))),
            })
        }
    }

    /// Create a new restartable ffmpeg source for a local file.
    pub async fn ffmpeg<P: AsRef<OsStr> + Send + Clone + Sync + 'static>(
        path: P,
        lazy: bool,
    ) -> Result<Self> {
        Self::new(FfmpegRestarter { path }, lazy).await
    }

    /// Create a new restartable ytdl source.
    ///
    /// The cost of restarting and seeking will probably be *very* high:
    /// expect a pause if you seek backwards.
    pub async fn ytdl<P: AsRef<str> + Send + Clone + Sync + 'static>(
        uri: P,
        lazy: bool,
    ) -> Result<Self> {
        Self::new(YtdlRestarter { uri }, lazy).await
    }

    /// Create a new restartable ytdl source, using the first result of a youtube search.
    ///
    /// The cost of restarting and seeking will probably be *very* high:
    /// expect a pause if you seek backwards.
    pub async fn ytdl_search(name: &str, lazy: bool) -> Result<Self> {
        Self::ytdl(format!("ytsearch1:{}", name), lazy).await
    }

    pub(crate) fn prep_with_handle(&mut self, handle: Handle) {
        self.async_handle = Some(handle);
    }

    pub(crate) fn make_playable(&mut self) {
        if matches!(self.source, LazyProgress::Dead(_, _, _, _)) {
            // This read triggers creation of a source, and is guaranteed not to modify any internals.
            // It will harmlessly write out zeroes into the target buffer.
            let mut bytes = [0u8; 0];
            let _ = Read::read(self, &mut bytes[..]);
        }
    }
}

/// Trait used to create an instance of a [`Reader`] at instantiation and when
/// a backwards seek is needed.
///
/// [`Reader`]: reader::Reader
#[async_trait]
pub trait Restart {
    /// Tries to create a replacement source.
    async fn call_restart(&mut self, time: Option<Duration>) -> Result<Input>;

    /// Optionally retrieve metadata for a source which has been lazily initialised.
    ///
    /// This is particularly useful for sources intended to be queued, which
    /// should occupy few resources when not live BUT have as much information as
    /// possible made available at creation.
    async fn lazy_init(&mut self) -> Result<(Option<Metadata>, Codec, Container)>;
}

struct FfmpegRestarter<P>
where
    P: AsRef<OsStr> + Send + Sync,
{
    path: P,
}

#[async_trait]
impl<P> Restart for FfmpegRestarter<P>
where
    P: AsRef<OsStr> + Send + Sync,
{
    async fn call_restart(&mut self, time: Option<Duration>) -> Result<Input> {
        if let Some(time) = time {
            let is_stereo = is_stereo(self.path.as_ref())
                .await
                .unwrap_or_else(|_e| (false, Default::default()));
            let stereo_val = if is_stereo.0 { "2" } else { "1" };

            let ts = format!("{}.{}", time.as_secs(), time.subsec_millis());
            _ffmpeg_optioned(
                self.path.as_ref(),
                &["-ss", &ts],
                &[
                    "-f",
                    "s16le",
                    "-ac",
                    stereo_val,
                    "-ar",
                    "48000",
                    "-acodec",
                    "pcm_f32le",
                    "-",
                ],
                Some(is_stereo),
            )
            .await
        } else {
            ffmpeg(self.path.as_ref()).await
        }
    }

    async fn lazy_init(&mut self) -> Result<(Option<Metadata>, Codec, Container)> {
        is_stereo(self.path.as_ref())
            .await
            .map(|(_stereo, metadata)| (Some(metadata), Codec::FloatPcm, Container::Raw))
    }
}

struct YtdlRestarter<P>
where
    P: AsRef<str> + Send + Sync,
{
    uri: P,
}

#[async_trait]
impl<P> Restart for YtdlRestarter<P>
where
    P: AsRef<str> + Send + Sync,
{
    async fn call_restart(&mut self, time: Option<Duration>) -> Result<Input> {
        if let Some(time) = time {
            let ts = format!("{}.{}", time.as_secs(), time.subsec_millis());

            _ytdl(self.uri.as_ref(), &["-ss", &ts]).await
        } else {
            ytdl(self.uri.as_ref()).await
        }
    }

    async fn lazy_init(&mut self) -> Result<(Option<Metadata>, Codec, Container)> {
        _ytdl_metadata(self.uri.as_ref())
            .await
            .map(|m| (Some(m), Codec::FloatPcm, Container::Raw))
    }
}

impl From<Restartable> for Input {
    fn from(mut src: Restartable) -> Self {
        let (meta, stereo, kind, container) = match &mut src.source {
            LazyProgress::Dead(ref mut m, _rec, kind, container) => {
                let stereo = m.channels == Some(2);
                (Some(m.take()), stereo, kind.clone(), *container)
            },
            LazyProgress::Live(ref mut input, _rec) => (
                Some(input.metadata.take()),
                input.stereo,
                input.kind.clone(),
                input.container,
            ),
            // This branch should never be taken: this is an emergency measure.
            LazyProgress::Working(kind, container, stereo, _) =>
                (None, *stereo, kind.clone(), *container),
        };
        Input::new(stereo, Reader::Restartable(src), kind, container, meta)
    }
}

// How do these work at a high level?
// If you need to restart, send a request to do this to the async context.
// if a request is pending, then just output all zeroes.

impl Read for Restartable {
    fn read(&mut self, buffer: &mut [u8]) -> IoResult<usize> {
        use LazyProgress::*;
        let (out_val, march_pos, next_source) = match &mut self.source {
            Dead(meta, rec, kind, container) => {
                let stereo = meta.channels == Some(2);
                let handle = self.async_handle.clone();
                let new_chan = if let Some(rec) = rec.take() {
                    Some(regenerate_channel(
                        rec,
                        0,
                        stereo,
                        kind.clone(),
                        *container,
                        handle,
                    )?)
                } else {
                    return Err(IoError::new(
                        IoErrorKind::UnexpectedEof,
                        "Illegal state: taken recreator was observed.".to_string(),
                    ));
                };

                // Then, output all zeroes.
                for el in buffer.iter_mut() {
                    *el = 0;
                }
                (Ok(buffer.len()), false, new_chan)
            },
            Live(source, _) => (Read::read(source, buffer), true, None),
            Working(_, _, _, chan) => {
                match chan.try_recv() {
                    Ok(Ok((mut new_source, recreator))) => {
                        // Completed!
                        // Do read, then replace inner progress.
                        let bytes_read = Read::read(&mut new_source, buffer);

                        (bytes_read, true, Some(Live(new_source, Some(recreator))))
                    },
                    Ok(Err(source_error)) => {
                        let e = Err(IoError::new(
                            IoErrorKind::UnexpectedEof,
                            format!("Failed to create new reader: {:?}.", source_error),
                        ));
                        (e, false, None)
                    },
                    Err(TryRecvError::Empty) => {
                        // Output all zeroes.
                        for el in buffer.iter_mut() {
                            *el = 0;
                        }
                        (Ok(buffer.len()), false, None)
                    },
                    Err(_) => {
                        let e = Err(IoError::new(
                            IoErrorKind::UnexpectedEof,
                            "Failed to create new reader: dropped.",
                        ));
                        (e, false, None)
                    },
                }
            },
        };

        if let Some(src) = next_source {
            self.source = src;
        }

        if march_pos {
            out_val.map(|a| {
                self.position += a;
                a
            })
        } else {
            out_val
        }
    }
}

impl Seek for Restartable {
    fn seek(&mut self, pos: SeekFrom) -> IoResult<u64> {
        let _local_pos = self.position as u64;

        use SeekFrom::*;
        match pos {
            Start(offset) => {
                let offset = offset as usize;
                let handle = self.async_handle.clone();

                use LazyProgress::*;
                match &mut self.source {
                    Dead(meta, rec, kind, container) => {
                        // regen at given start point
                        self.source = if let Some(rec) = rec.take() {
                            regenerate_channel(
                                rec,
                                offset,
                                meta.channels == Some(2),
                                kind.clone(),
                                *container,
                                handle,
                            )?
                        } else {
                            return Err(IoError::new(
                                IoErrorKind::UnexpectedEof,
                                "Illegal state: taken recreator was observed.".to_string(),
                            ));
                        };

                        self.position = offset;
                    },
                    Live(input, rec) =>
                        if offset < self.position {
                            // regen at given start point
                            // We're going back in time.
                            self.source = if let Some(rec) = rec.take() {
                                regenerate_channel(
                                    rec,
                                    offset,
                                    input.stereo,
                                    input.kind.clone(),
                                    input.container,
                                    handle,
                                )?
                            } else {
                                return Err(IoError::new(
                                    IoErrorKind::UnexpectedEof,
                                    "Illegal state: taken recreator was observed.".to_string(),
                                ));
                            };

                            self.position = offset;
                        } else {
                            // march on with live source.
                            self.position += input.consume(offset - self.position);
                        },
                    Working(_, _, _, _) => {
                        return Err(IoError::new(
                            IoErrorKind::Interrupted,
                            "Previous seek in progress.",
                        ));
                    },
                }

                Ok(offset as u64)
            },
            End(_offset) => Err(IoError::new(
                IoErrorKind::InvalidInput,
                "End point for Restartables is not known.",
            )),
            Current(_offset) => unimplemented!(),
        }
    }
}

fn regenerate_channel(
    mut rec: Recreator,
    offset: usize,
    stereo: bool,
    kind: Codec,
    container: Container,
    handle: Option<Handle>,
) -> IoResult<LazyProgress> {
    if let Some(handle) = handle.as_ref() {
        let (tx, rx) = flume::bounded(1);

        handle.spawn(async move {
            let ret_val = rec
                .call_restart(Some(utils::byte_count_to_timestamp(offset, stereo)))
                .await;

            let _ = tx.send(ret_val.map(Box::new).map(|v| (v, rec)));
        });

        Ok(LazyProgress::Working(kind, container, stereo, rx))
    } else {
        Err(IoError::new(
            IoErrorKind::Interrupted,
            "Cannot safely call seek until provided an async context handle.",
        ))
    }
}
