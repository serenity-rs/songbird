use std::{
    io::{ErrorKind as IoErrorKind, Result as IoResult, SeekFrom},
    pin::Pin,
    task::{Context, Poll},
};

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use pin_project::pin_project;
use reqwest::{header::HeaderMap, Client};
use stream_lib::{DownloadStream, Event};
use symphonia_core::io::MediaSource;
use tokio::io::{AsyncRead, AsyncSeek, ReadBuf};
use tokio_util::io::StreamReader;

use crate::input::{
    AsyncAdapterStream,
    AsyncMediaSource,
    AudioStream,
    AudioStreamError,
    Compose,
    Input,
};

/// Lazy HLS stream
#[derive(Debug)]
pub struct HlsRequest {
    /// HLS downloader
    pub hls: Option<DownloadStream>,
}

impl HlsRequest {
    #[must_use]
    /// Create a lazy HLS request.
    pub fn new(client: Client, request: &str) -> Self {
        Self::new_with_headers(client, request, HeaderMap::default())
    }

    #[must_use]
    /// Create a lazy HTTP request.
    pub fn new_with_headers(client: Client, request: &str, headers: HeaderMap) -> Self {
        let request = client.get(request).headers(headers).build().unwrap();
        let hls = stream_lib::download_hls(client, request, None);

        HlsRequest { hls: Some(hls) }
    }

    fn create_stream(&mut self) -> Result<HlsStream, AudioStreamError> {
        let Some(hls) = self.hls.take() else {
            return Err(AudioStreamError::Fail("hls can only be used once".into()));
        };

        let stream = Box::new(StreamReader::new(hls.map(|ev| match ev {
            Event::Bytes { bytes } => Ok(bytes),
            Event::End => Ok(Bytes::new()),
            Event::Error { error } => Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                error,
            )),
        })));

        Ok(HlsStream { stream })
    }
}

#[pin_project]
struct HlsStream {
    #[pin]
    stream: Box<dyn AsyncRead + Send + Sync + Unpin>,
}

impl AsyncRead for HlsStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<IoResult<()>> {
        AsyncRead::poll_read(self.project().stream, cx, buf)
    }
}

impl AsyncSeek for HlsStream {
    fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> IoResult<()> {
        Err(IoErrorKind::Unsupported.into())
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<IoResult<u64>> {
        unreachable!()
    }
}

#[async_trait]
impl AsyncMediaSource for HlsStream {
    fn is_seekable(&self) -> bool {
        false
    }

    async fn byte_len(&self) -> Option<u64> {
        None
    }

    async fn try_resume(
        &mut self,
        _offset: u64,
    ) -> Result<Box<dyn AsyncMediaSource>, AudioStreamError> {
        Err(AudioStreamError::Unsupported)
    }
}

#[async_trait]
impl Compose for HlsRequest {
    fn create(&mut self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        self.create_stream().map(|input| {
            let stream = AsyncAdapterStream::new(Box::new(input), 64 * 1024);

            AudioStream {
                input: Box::new(stream) as Box<dyn MediaSource>,
                hint: None,
            }
        })
    }

    async fn create_async(
        &mut self,
    ) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        Err(AudioStreamError::Unsupported)
    }

    fn should_create_async(&self) -> bool {
        false
    }
}

impl From<HlsRequest> for Input {
    fn from(val: HlsRequest) -> Self {
        Input::Lazy(Box::new(val))
    }
}
