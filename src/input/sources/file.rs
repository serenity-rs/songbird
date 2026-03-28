use crate::input::{AudioStream, AudioStreamError, AuxMetadata, Compose, Input};
use std::{error::Error, ffi::OsStr, path::Path, time::Duration};
use symphonia_core::{
    codecs::CODEC_TYPE_NULL,
    formats::FormatOptions,
    io::{MediaSource, MediaSourceStream},
    meta::MetadataOptions,
    probe::Hint,
};

/// A lazily instantiated local file.
#[derive(Clone, Debug)]
pub struct File<P: AsRef<Path>> {
    path: P,
}

impl<P: AsRef<Path>> File<P> {
    /// Creates a lazy file object, which will open the target path.
    ///
    /// This is infallible as the path is only checked during creation.
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
        if let Some(ext) = self.path.as_ref().extension().and_then(OsStr::to_str) {
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

    // Probes for metadata about this audio file using symphonia probe
    async fn aux_metadata(&mut self) -> Result<AuxMetadata, AudioStreamError> {
        let file = self.create_async().await?;
        let mss = MediaSourceStream::new(file.input, Default::default());

        // Probe for metadata about the audio file
        let probe = symphonia::default::get_probe()
            .format(
                &file.hint.unwrap_or_default(),
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| AudioStreamError::Fail(Box::new(e)))?;

        // Find the first track with a valid codec
        let track = probe
            .format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or(AudioStreamError::Unsupported)?;

        // Gather channels, frame count, sample rate, and calculate duration based on frame count and sample rate.
        let channels = track.codec_params.channels.map(|c| c.count() as u8);
        let frame_count: usize = track.codec_params.n_frames.map(|n| n as usize).unwrap_or(0);
        let sample_rate = track.codec_params.sample_rate;
        let duration = sample_rate
            .map(|rate| Duration::from_millis((frame_count as f64 / rate as f64 * 1000.0) as u64));

        // Return the metadata
        Ok(AuxMetadata {
            channels,
            duration,
            sample_rate,
            ..Default::default()
        })
    }
}
