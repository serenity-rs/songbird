#![allow(missing_docs)]

use super::{AudioStream, AudioStreamError, AuxMetadata, Compose, HttpRequest, Input};

use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use std::error::Error;
use symphonia_core::io::MediaSource;
use tokio::process::Command;

const YOUTUBE_DL_COMMAND: &str = "yt-dlp";

pub struct YoutubeDl {
    program: &'static str,
    client: Client,
    metadata: Option<AuxMetadata>,
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
            metadata: None,
            url,
        }
    }

    async fn query(&mut self) -> Result<Value, AudioStreamError> {
        let ytdl_args = [
            "-j",
            &self.url,
            "-f",
            "ba[abr>0][vcodec=none]/best",
        ];

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
            .and_then(|url| url.as_str())
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

        match &out {
            Ok(_) => println!("Created okay."),
            Err(e) => println!("Argh: {:?}", e),
        }

        out
    }

    fn should_create_async(&self) -> bool {
        true
    }

    async fn aux_metadata(&mut self) -> Result<AuxMetadata, AudioStreamError> {
        if let Some(meta) = self.metadata.as_ref() {
            return Ok(meta.clone());
        }

        let _ = self.query().await?;

        self.metadata.clone().ok_or_else(|| {
            let msg: Box<dyn Error + Send + Sync + 'static> =
                "Failed to instansiate any metadata... Should be unreachable.".into();
            AudioStreamError::Fail(msg)
        })
    }
}
