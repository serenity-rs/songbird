//! Raw handlers for input bytestreams.

use super::*;
use std::{
    fmt::{Debug, Error as FormatError, Formatter},
    fs::File,
    io::{
        BufReader,
        Cursor,
        Error as IoError,
        ErrorKind as IoErrorKind,
        Read,
        Result as IoResult,
        Seek,
        SeekFrom,
    },
    result::Result as StdResult,
};
use streamcatcher::{Catcher, TxCatcher};
pub use symphonia_core::io::MediaSource;

/// Usable data/byte sources for an audio stream.
///
/// Users may define their own data sources using [`Extension`].
///
/// [`Extension`]: Reader::Extension
pub enum Reader {
    /// Piped output of another program (i.e., [`ffmpeg`]).
    ///
    /// Does not support seeking.
    ///
    /// [`ffmpeg`]: super::ffmpeg
    Pipe(BufReader<ChildContainer>),
    /// A cached, raw in-memory store, provided by Songbird.
    ///
    /// Supports seeking.
    Memory(Catcher<Box<Reader>>),
    /// A cached, Opus-compressed in-memory store, provided by Songbird.
    ///
    /// Supports seeking.
    Compressed(TxCatcher<Box<Input>, OpusCompressor>),
    /// A source which supports seeking by recreating its inout stream.
    ///
    /// Supports seeking.
    Restartable(Restartable),
    /// A basic user-provided source.
    ///
    /// Seeking support depends on underlying `MediaSource` implementation.
    Extension(Box<dyn MediaSource + Send>),
}

impl Reader {
    /// Returns whether the given source implements [`Seek`].
    ///
    /// This might be an expensive operation and might involve blocking IO. In such cases, it is
    /// advised to cache the return value when possible.
    ///
    /// [`Seek`]: https://doc.rust-lang.org/std/io/trait.Seek.html
    pub fn is_seekable(&self) -> bool {
        use Reader::*;
        match self {
            Restartable(_) | Compressed(_) | Memory(_) => true,
            Extension(source) => source.is_seekable(),
            _ => false,
        }
    }

    /// A source contained in a local file.
    pub fn from_file(file: File) -> Self {
        Self::Extension(Box::new(file))
    }

    /// A source contained as an array in memory.
    pub fn from_memory(buf: Vec<u8>) -> Self {
        Self::Extension(Box::new(Cursor::new(buf)))
    }

    #[allow(clippy::single_match)]
    pub(crate) fn prep_with_handle(&mut self, handle: Handle) {
        use Reader::*;
        match self {
            Restartable(r) => r.prep_with_handle(handle),
            _ => {},
        }
    }

    #[allow(clippy::single_match)]
    pub(crate) fn make_playable(&mut self) {
        use Reader::*;
        match self {
            Restartable(r) => r.make_playable(),
            _ => {},
        }
    }
}

impl Read for Reader {
    fn read(&mut self, buffer: &mut [u8]) -> IoResult<usize> {
        use Reader::*;
        match self {
            Pipe(a) => Read::read(a, buffer),
            Memory(a) => Read::read(a, buffer),
            Compressed(a) => Read::read(a, buffer),
            Restartable(a) => Read::read(a, buffer),
            Extension(a) => a.read(buffer),
        }
    }
}

impl Seek for Reader {
    fn seek(&mut self, pos: SeekFrom) -> IoResult<u64> {
        use Reader::*;
        match self {
            Pipe(_) => Err(IoError::new(
                IoErrorKind::InvalidInput,
                "Seeking not supported on Reader of this type.",
            )),
            Memory(a) => Seek::seek(a, pos),
            Compressed(a) => Seek::seek(a, pos),
            Restartable(a) => Seek::seek(a, pos),
            Extension(a) =>
                if a.is_seekable() {
                    a.seek(pos)
                } else {
                    Err(IoError::new(
                        IoErrorKind::InvalidInput,
                        "Seeking not supported on Reader of this type.",
                    ))
                },
        }
    }
}

impl Debug for Reader {
    fn fmt(&self, f: &mut Formatter<'_>) -> StdResult<(), FormatError> {
        use Reader::*;
        let field = match self {
            Pipe(a) => format!("{:?}", a),
            Memory(a) => format!("{:?}", a),
            Compressed(a) => format!("{:?}", a),
            Restartable(a) => format!("{:?}", a),
            Extension(_) => "Extension".to_string(),
        };
        f.debug_tuple("Reader").field(&field).finish()
    }
}

impl From<Vec<u8>> for Reader {
    fn from(val: Vec<u8>) -> Self {
        Self::from_memory(val)
    }
}
