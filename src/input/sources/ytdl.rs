use crate::input::{AudioStream, AudioStreamError, AuxMetadata, Compose, HttpRequest, Input};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use std::error::Error;
use symphonia_core::io::MediaSource;
use tokio::process::Command;

const YOUTUBE_DL_COMMAND: &str = "yt-dlp";

/// A lazily instantiated call to download a file, finding its URL via youtube-dl.
///
/// By default, this uses yt-dlp and is backed by an [`HttpRequest`]. This handler
/// attempts to find the best audio-only source (typically `WebM`, enabling low-cost
/// Opus frame passthrough).
///
/// [`HttpRequest`]: super::HttpRequest
#[derive(Clone, Debug)]
pub struct YoutubeDl {
    program: &'static str,
    client: Client,
    metadata: Option<AuxMetadata>,
    url: String,
}

impl YoutubeDl {
    /// Creates a lazy request to select an audio stream from `url`, using "yt-dlp".
    ///
    /// This requires a reqwest client: ideally, one should be created and shared between
    /// all requests.
    #[must_use]
    pub fn new(client: Client, url: String) -> Self {
        Self::new_ytdl_like(YOUTUBE_DL_COMMAND, client, url)
    }

    /// Creates a lazy request to select an audio stream from `url` as in [`new`], using `program`.
    ///
    /// [`new`]: Self::new
    #[must_use]
    pub fn new_ytdl_like(program: &'static str, client: Client, url: String) -> Self {
        Self {
            program,
            client,
            metadata: None,
            url,
        }
    }

    async fn query(&mut self) -> Result<Value, AudioStreamError> {
        let ytdl_args = ["-j", &self.url, "-f", "ba[abr>0][vcodec=none]/best"];

        let output = Command::new(self.program)
            .args(&ytdl_args)
            .output()
            .await
            .map_err(|e| AudioStreamError::Fail(Box::new(e)))?;

        let stdout: Value = serde_json::from_slice(&output.stdout[..])
            .map_err(|e| AudioStreamError::Fail(Box::new(e)))?;

        let metadata = Some(AuxMetadata::from_ytdl_output(&stdout));
        self.metadata = metadata;

        Ok(stdout)
    }
}

impl From<YoutubeDl> for Input {
    fn from(val: YoutubeDl) -> Self {
        Input::Lazy(Box::new(val))
    }
}

#[async_trait]
impl Compose for YoutubeDl {
    fn create(&mut self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        Err(AudioStreamError::Unsupported)
    }

    async fn create_async(
        &mut self,
    ) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        let stdout = self.query().await?;

        let url = stdout
            .as_object()
            .and_then(|top| top.get("url"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                let msg: Box<dyn Error + Send + Sync + 'static> =
                    "URL field not found on youtube-dl output.".into();
                AudioStreamError::Fail(msg)
            })?;

        let mut req = HttpRequest {
            client: self.client.clone(),
            request: url.to_string(),
        };

        let out = req.create_async().await;

        out
    }

    fn should_create_async(&self) -> bool {
        true
    }

    async fn aux_metadata(&mut self) -> Result<AuxMetadata, AudioStreamError> {
        if let Some(meta) = self.metadata.as_ref() {
            return Ok(meta.clone());
        }

        self.query().await?;

        self.metadata.clone().ok_or_else(|| {
            let msg: Box<dyn Error + Send + Sync + 'static> =
                "Failed to instansiate any metadata... Should be unreachable.".into();
            AudioStreamError::Fail(msg)
        })
    }
}

#[cfg(test)]
mod tests {
    use reqwest::Client;
    use std::time::Duration;

    use super::*;
    use crate::{
        constants::test_data::YTDL_TARGET,
        driver::Driver,
        tracks::{PlayMode, ReadyState},
        Config,
    };

    #[tokio::test]
    async fn ytdl_track_plays() {
        let (t_handle, config) = Config::test_cfg(true);
        let mut driver = Driver::new(config.clone());

        let file = YoutubeDl::new(Client::new(), YTDL_TARGET.into());

        // Get input in place, playing. Wait for IO to ready.
        t_handle.ready_track(&driver.play(file.into()), None).await;
        t_handle.tick(1);

        // post-conditions:
        // 1) track produces a packet.
        // 2) that packet is mixed audio.
        // 3) that packet is non-zero.
        let pkt = t_handle.recv();
        let pkt = pkt.raw().unwrap();
        assert!(pkt.is_mixed_with_nonzero_signal());
    }

    #[tokio::test]
    async fn ytdl_forward_seek_correct() {
        let (t_handle, config) = Config::test_cfg(true);
        let mut driver = Driver::new(config.clone());

        let file = YoutubeDl::new(Client::new(), YTDL_TARGET.into());
        let handle = driver.play(file.into());

        // Get input in place, playing. Wait for IO to ready.
        t_handle.ready_track(&handle, None).await;

        let target_time = Duration::from_secs(30);
        assert!(handle.seek_time(target_time).is_ok());
        t_handle.ready_track(&handle, None).await;

        // post-conditions:
        // 1) track is readied
        // 2) track's position is approx 30s
        // 3) track's play time is considerably less (O(5s))
        let state = handle.get_info();
        t_handle.tick(1);
        let state = state.await.expect("Should have received valid state.");

        assert_eq!(state.ready, ReadyState::Playable);
        assert_eq!(state.playing, PlayMode::Play);
        assert!(state.play_time < Duration::from_secs(5));
        assert!(
            state.position < target_time + Duration::from_millis(100)
                && state.position > target_time - Duration::from_millis(100)
        );
    }

    #[tokio::test]
    async fn ytdl_backward_seek_correct() {
        let (t_handle, config) = Config::test_cfg(true);
        let mut driver = Driver::new(config.clone());

        let file = YoutubeDl::new(Client::new(), YTDL_TARGET.into());
        let handle = driver.play(file.into());

        // Get input in place, playing. Wait for IO to ready.
        t_handle.ready_track(&handle, None).await;

        // Accelerated playout -- 4 seconds worth.
        let n_secs = 4;
        let n_ticks = 50 * n_secs;
        t_handle.skip(n_ticks).await;

        let target_time = Duration::from_secs(1);
        assert!(handle.seek_time(target_time).is_ok());
        t_handle.ready_track(&handle, None).await;

        // post-conditions:
        // 1) track is readied
        // 2) track's position is approx 1s
        // 3) track's play time is preserved (About 4s)
        let state = handle.get_info();
        t_handle.tick(1);
        let state = state.await.expect("Should have received valid state.");

        assert_eq!(state.ready, ReadyState::Playable);
        assert_eq!(state.playing, PlayMode::Play);
        assert!(state.play_time >= Duration::from_secs(n_secs));
        assert!(
            state.position < target_time + Duration::from_millis(100)
                && state.position > target_time - Duration::from_millis(100)
        );
    }
}
