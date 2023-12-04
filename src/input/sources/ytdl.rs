use crate::input::{
    metadata::ytdl::Output, AudioStream, AudioStreamError, AuxMetadata, Compose, HttpRequest, Input,
};
use async_trait::async_trait;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Client,
};
use std::{error::Error, io::ErrorKind};
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

    /// Creates a lazy(?) request to search youtube for a videos matching `query`.
    ///
    /// [`new`]: Self::new
    #[must_use]
    pub fn new_yt_search(client: Client, query: String) -> Self {
        Self::new_ytdl_like(YOUTUBE_DL_COMMAND, client, format!("ytsearch5:{}", query))
    }

    /// Does a search query for the given url, returning a list of possible matches
    /// which are youtube urls.
    pub async fn search_query(&mut self) -> Result<Vec<String>, AudioStreamError> {
        let search_str = if self.url.starts_with("ytsearch") {
            self.url.clone()
        } else {
            format!("ytsearch5:{}", self.url)
        };

        let ytdl_args = [
            "-s",
            &search_str,
            "--get-id",
            "--get-title",
            "--get-duration",
        ];

        let output = Command::new(self.program)
            .args(ytdl_args)
            .output()
            .await
            .map_err(|e| {
                AudioStreamError::Fail(if e.kind() == ErrorKind::NotFound {
                    format!("could not find executable '{}' on path", self.program).into()
                } else {
                    Box::new(e)
                })
            })?;

        let lines = output
            .stdout
            .split(|&b| b == b'\n')
            .map(|line| format!("{}", String::from_utf8_lossy(line)))
            .collect::<Vec<_>>();

        Ok(lines)
    }

    async fn query(&mut self) -> Result<Output, AudioStreamError> {
        let ytdl_args = [
            "-j",
            &self.url,
            "-f",
            "ba[abr>0][vcodec=none]/best",
            "--no-playlist",
        ];

        let mut output = Command::new(self.program)
            .args(ytdl_args)
            .output()
            .await
            .map_err(|e| {
                AudioStreamError::Fail(if e.kind() == ErrorKind::NotFound {
                    format!("could not find executable '{}' on path", self.program).into()
                } else {
                    Box::new(e)
                })
            })?;

        // NOTE: must be mut for simd-json.
        #[allow(clippy::unnecessary_mut_passed)]
        let stdout: Output = crate::json::from_slice(&mut output.stdout[..])
            .map_err(|e| AudioStreamError::Fail(Box::new(e)))?;

        self.metadata = Some(stdout.as_aux_metadata());

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

        let mut headers = HeaderMap::default();

        if let Some(map) = stdout.http_headers {
            headers.extend(map.iter().filter_map(|(k, v)| {
                Some((
                    HeaderName::from_bytes(k.as_bytes()).ok()?,
                    HeaderValue::from_str(v).ok()?,
                ))
            }));
        }

        let mut req = HttpRequest {
            client: self.client.clone(),
            request: stdout.url,
            headers,
            content_length: stdout.filesize,
        };

        req.create_async().await
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

    async fn search(&mut self) -> Result<Vec<String>, AudioStreamError> {
        self.search_query().await
    }
}

#[cfg(test)]
mod tests {
    use reqwest::Client;

    use super::*;
    use crate::constants::test_data::*;
    use crate::input::input_tests::*;

    #[tokio::test]
    #[ntest::timeout(20_000)]
    async fn ytdl_track_plays() {
        track_plays_mixed(|| YoutubeDl::new(Client::new(), YTDL_TARGET.into())).await;
    }

    #[tokio::test]
    #[ntest::timeout(20_000)]
    async fn ytdl_page_with_playlist_plays() {
        track_plays_passthrough(|| YoutubeDl::new(Client::new(), YTDL_PLAYLIST_TARGET.into()))
            .await;
    }

    #[tokio::test]
    #[ntest::timeout(20_000)]
    async fn ytdl_forward_seek_correct() {
        forward_seek_correct(|| YoutubeDl::new(Client::new(), YTDL_TARGET.into())).await;
    }

    #[tokio::test]
    #[ntest::timeout(20_000)]
    async fn ytdl_backward_seek_correct() {
        backward_seek_correct(|| YoutubeDl::new(Client::new(), YTDL_TARGET.into())).await;
    }

    #[tokio::test]
    #[ntest::timeout(20_000)]
    async fn fake_exe_errors() {
        let mut ytdl = YoutubeDl::new_ytdl_like("yt-dlq", Client::new(), YTDL_TARGET.into());

        assert!(ytdl.aux_metadata().await.is_err());
    }
}
