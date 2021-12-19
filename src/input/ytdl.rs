#![allow(missing_docs)]

use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use symphonia_core::io::MediaSource;
use tokio::process::Command;

use super::{AudioStreamError, Compose, SymphInput};

const YOUTUBE_DL_COMMAND: &str = "yt-dlp";

pub struct YoutubeDl {
    program: &'static str,
    client: Client,
    url: String,
}

impl YoutubeDl {
    pub fn new(client: Client, url: String) -> Self {
        Self::new_ytdl_like(YOUTUBE_DL_COMMAND, client, url)
    }

    pub fn new_ytdl_like(program: &'static str, client: Client, url: String) -> Self {
        Self {
            program,
            client,
            url,
        }
    }
}

impl From<YoutubeDl> for SymphInput {
    fn from(val: YoutubeDl) -> Self {
        SymphInput::Lazy(Box::new(val))
    }
}

#[async_trait]
impl Compose for YoutubeDl {
    fn create(&mut self) -> Result<super::AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        unimplemented!()
    }

    async fn create_async(
        &mut self,
    ) -> Result<super::AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        let ytdl_args = [
            "-j",
            &self.url,
            "-f",
            "ba[abr>0][ext!*=webm][vcodec=none]/best[ext!*=webm]",
        ];

        let output = Command::new(self.program)
            .args(&ytdl_args)
            .output()
            .await
            .map_err(|e| AudioStreamError::Fail(Box::new(e)))?;

        let stdout: Value = serde_json::from_slice(&output.stdout[..])
            .map_err(|e| AudioStreamError::Fail(Box::new(e)))?;

        let url = stdout
            .as_object()
            .and_then(|top| top.get("url"))
            .and_then(|url| url.as_str())
            .ok_or_else(|| {
                let msg: Box<dyn std::error::Error + Send + Sync + 'static> =
                    "URL field not found on youtube-dl output.".into();
                AudioStreamError::Fail(msg)
            })?;

        let mut req = super::adapter::HttpRequest {
            client: self.client.clone(),
            request: url.to_string(),
        };

        let out = req.create_async().await;

        match &out {
            Ok(_) => println!("Created okay."),
            Err(e) => println!("Argh: {:?}", e),
        }

        out
    }

    fn should_create_async(&self) -> bool {
        true
    }
}
