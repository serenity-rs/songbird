use std::{error::Error, fmt::Display, time::Duration};
use symphonia_core::errors::Error as SymphError;

/// Errors encountered when creating an [`AudioStream`] or requesting metadata
/// from a [`Compose`].
///
/// [`AudioStream`]: super::AudioStream
/// [`Compose`]: super::Compose
#[non_exhaustive]
#[derive(Debug)]
pub enum AudioStreamError {
    /// The operation failed, and should be retried after a given time.
    ///
    /// Create operations invoked by the driver will retry on the first tick
    /// after this time has passed.
    RetryIn(Duration),
    /// The operation failed, and should not be retried.
    Fail(Box<dyn Error + Send>),
    /// The operation was not supported, and will never succeed.
    Unsupported,
}

impl Display for AudioStreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("failed to create audio -- ")?;
        match self {
            Self::RetryIn(t) => f.write_fmt(format_args!("retry in {:.2}s", t.as_secs_f32())),
            Self::Fail(why) => f.write_fmt(format_args!("{}", why)),
            Self::Unsupported => f.write_str("operation was not supported"),
        }
    }
}

impl Error for AudioStreamError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }

    fn cause(&self) -> Option<&dyn Error> {
        self.source()
    }
}

// TODO: display

#[allow(missing_docs)]
#[non_exhaustive]
#[derive(Debug)]
pub enum MakePlayableError {
    Create(AudioStreamError),
    Parse(SymphError),
}

impl From<AudioStreamError> for MakePlayableError {
    fn from(val: AudioStreamError) -> Self {
        Self::Create(val)
    }
}

impl From<SymphError> for MakePlayableError {
    fn from(val: SymphError) -> Self {
        Self::Parse(val)
    }
}

#[allow(missing_docs)]
#[non_exhaustive]
#[derive(Debug)]
pub enum MetadataError {
    NotLive,
    NotParsed,
    Fail(AudioStreamError),
}
