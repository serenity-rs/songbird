//! Errors caused by input creation.

use audiopus::Error as OpusError;
use core::fmt;
use serde_json::{Error as JsonError, Value};
use std::{error::Error as StdError, io::Error as IoError, process::Output};
use streamcatcher::CatcherError;

/// An error returned when creating a new [`Input`].
///
/// [`Input`]: crate::input::Input
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// An error occurred while opening a new DCA source.
    Dca(DcaError),
    /// An error occurred while reading, or opening a file.
    Io(IoError),
    /// An error occurred while parsing JSON (i.e., during metadata/stereo detection).
    Json {
        /// Json error
        error: JsonError,
        /// Text that failed to be parsed
        parsed_text: String,
    },
    /// An error occurred within the Opus codec.
    Opus(OpusError),
    /// Failed to extract metadata from alternate pipe.
    Metadata,
    /// Apparently failed to create stdout.
    Stdout,
    /// An error occurred while checking if a path is stereo.
    Streams,
    /// Configuration error for a cached Input.
    Streamcatcher(CatcherError),
    /// An error occurred while processing the JSON output from `youtube-dl`.
    ///
    /// The JSON output is given.
    YouTubeDlProcessing(Value),
    /// An error occurred while running `youtube-dl`.
    YouTubeDlRun(Output),
    /// The `url` field of the `youtube-dl` JSON output was not present.
    ///
    /// The JSON output is given.
    YouTubeDlUrl(Value),
}

impl From<CatcherError> for Error {
    fn from(e: CatcherError) -> Self {
        Error::Streamcatcher(e)
    }
}

impl From<DcaError> for Error {
    fn from(e: DcaError) -> Self {
        Error::Dca(e)
    }
}

impl From<IoError> for Error {
    fn from(e: IoError) -> Error {
        Error::Io(e)
    }
}

impl From<OpusError> for Error {
    fn from(e: OpusError) -> Error {
        Error::Opus(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Dca(e) => write!(f, "{}", e),
            Error::Io(e) => write!(f, "{}", e),
            Error::Json {
                error,
                parsed_text: _,
            } => write!(f, "{}", error),
            Error::Opus(e) => write!(f, "{}", e),
            Error::Metadata => write!(f, "Failed to extract metadata"),
            Error::Stdout => write!(f, "Failed to create stdout"),
            Error::Streams => write!(f, "Error while checking if path is stereo"),
            Error::Streamcatcher(e) => write!(f, "{}", e),
            Error::YouTubeDlProcessing(_) => write!(f, "Processing JSON from youtube-dl failed"),
            Error::YouTubeDlRun(_) => write!(f, "youtube-dl encountered an error"),
            Error::YouTubeDlUrl(_) => write!(f, "Missing url field in JSON"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Error::Dca(e) => Some(e),
            Error::Io(e) => Some(e),
            Error::Json {
                error,
                parsed_text: _,
            } => Some(error),
            Error::Opus(e) => Some(e),
            Error::Metadata => None,
            Error::Stdout => None,
            Error::Streams => None,
            Error::Streamcatcher(e) => Some(e),
            Error::YouTubeDlProcessing(_) => None,
            Error::YouTubeDlRun(_) => None,
            Error::YouTubeDlUrl(_) => None,
        }
    }
}

/// An error returned from the [`dca`] method.
///
/// [`dca`]: crate::input::dca
#[derive(Debug)]
#[non_exhaustive]
pub enum DcaError {
    /// An error occurred while reading, or opening a file.
    IoError(IoError),
    /// The file opened did not have a valid DCA JSON header.
    InvalidHeader,
    /// The file's metadata block was invalid, or could not be parsed.
    InvalidMetadata(JsonError),
    /// The file's header reported an invalid metadata block size.
    InvalidSize(i32),
    /// An error was encountered while creating a new Opus decoder.
    Opus(OpusError),
}

impl fmt::Display for DcaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DcaError::IoError(e) => write!(f, "{}", e),
            DcaError::InvalidHeader => write!(f, "Invalid DCA JSON header"),
            DcaError::InvalidMetadata(e) => write!(f, "{}", e),
            DcaError::InvalidSize(e) => write!(f, "Invalid metadata block size: {}", e),
            DcaError::Opus(e) => write!(f, "{}", e),
        }
    }
}

impl StdError for DcaError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            DcaError::IoError(e) => Some(e),
            DcaError::InvalidHeader => None,
            DcaError::InvalidMetadata(e) => Some(e),
            DcaError::InvalidSize(_) => None,
            DcaError::Opus(e) => Some(e),
        }
    }
}

/// Convenience type for fallible return of [`Input`]s.
///
/// [`Input`]: crate::input::Input
pub type Result<T> = std::result::Result<T, Error>;
