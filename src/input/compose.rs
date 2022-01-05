use super::{AudioStream, AudioStreamError, AuxMetadata};

use symphonia_core::io::MediaSource;

// TODO: add an optional mechanism to query lightweight metadata?
// i.e., w/o instantiating track.
#[allow(missing_docs)]
#[async_trait::async_trait]
pub trait Compose: Send {
    /// Create a source synchronously.
    fn create(&mut self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError>;
    /// Create a source asynchronously.
    async fn create_async(&mut self)
        -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError>;
    /// Hmm.
    fn should_create_async(&self) -> bool;
    /// Test.
    async fn aux_metadata(&mut self) -> Result<AuxMetadata, AudioStreamError> {
        Err(AudioStreamError::Unsupported)
    }
}
