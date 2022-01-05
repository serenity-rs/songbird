use super::{AudioStream, AudioStreamError, Compose, Input};

use std::{error::Error, path::Path};
use symphonia_core::{io::MediaSource, probe::Hint};

#[allow(missing_docs)]
pub struct File<P: AsRef<Path>> {
    path: P,
}

#[allow(missing_docs)]
impl<P: AsRef<Path>> File<P> {
    pub fn new(path: P) -> Self {
        Self { path }
    }
}

impl<P: AsRef<Path> + Send + Sync + 'static> From<File<P>> for Input {
    fn from(val: File<P>) -> Self {
        Input::Lazy(Box::new(val))
    }
}

#[async_trait::async_trait]
impl<P: AsRef<Path> + Send + Sync> Compose for File<P> {
    fn create(&mut self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        let err: Box<dyn Error + Send + Sync> =
            "Files should be created asynchronously.".to_string().into();
        Err(AudioStreamError::Fail(err))
    }

    async fn create_async(
        &mut self,
    ) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        let file = tokio::fs::File::open(&self.path)
            .await
            .map_err(|io| AudioStreamError::Fail(Box::new(io)))?;

        let input = Box::new(file.into_std().await);

        let mut hint = Hint::default();
        if let Some(ext) = self.path.as_ref().extension().and_then(|s| s.to_str()) {
            hint.with_extension(ext);
        }

        Ok(AudioStream {
            input,
            hint: Some(hint),
        })
    }

    fn should_create_async(&self) -> bool {
        true
    }
}
