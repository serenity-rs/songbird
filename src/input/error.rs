use std::{error::Error, fmt::Display, time::Duration};

#[allow(missing_docs)]
#[non_exhaustive]
#[derive(Debug)]
pub enum AudioStreamError {
    RetryIn(Duration),
    Fail(Box<dyn Error + Send>),
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
