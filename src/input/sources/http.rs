use crate::input::{
    AsyncAdapterStream,
    AsyncMediaSource,
    AudioStream,
    AudioStreamError,
    Compose,
    Input,
};
use async_trait::async_trait;
use futures::TryStreamExt;
use pin_project::pin_project;
use reqwest::{
    header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_TYPE, RANGE, RETRY_AFTER},
    Client,
};
use std::{
    io::{Error as IoError, ErrorKind as IoErrorKind, Result as IoResult, SeekFrom},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use symphonia_core::{io::MediaSource, probe::Hint};
use tokio::io::{AsyncRead, AsyncSeek, ReadBuf};
use tokio_util::io::StreamReader;

#[derive(Clone, Debug)]
/// A lazily instantiated HTTP request.
pub struct HttpRequest {
    /// A reqwest client instance used to send the HTTP GET request.
    pub client: Client,
    /// The target URL of the required resource.
    pub request: String,
}

impl HttpRequest {
    async fn create_stream(
        &mut self,
        offset: Option<u64>,
    ) -> Result<(HttpStream, Option<Hint>), AudioStreamError> {
        let mut resp = self.client.get(&self.request);

        if let Some(offset) = offset {
            resp = resp.header(RANGE, format!("bytes={}-", offset));
        };

        let resp = resp
            .send()
            .await
            .map_err(|e| AudioStreamError::Fail(Box::new(e)))?;

        if let Some(t) = resp.headers().get(RETRY_AFTER) {
            t.to_str()
                .map_err(|_| {
                    let msg: Box<dyn std::error::Error + Send + Sync + 'static> =
                        "Retry-after field contained non-ASCII data.".into();
                    AudioStreamError::Fail(msg)
                })
                .and_then(|str_text| {
                    str_text.parse().map_err(|_| {
                        let msg: Box<dyn std::error::Error + Send + Sync + 'static> =
                            "Retry-after field was non-numeric.".into();
                        AudioStreamError::Fail(msg)
                    })
                })
                .and_then(|t| Err(AudioStreamError::RetryIn(Duration::from_secs(t))))
        } else {
            let headers = resp.headers();

            let hint = headers
                .get(CONTENT_TYPE)
                .and_then(|val| val.to_str().ok())
                .map(|val| {
                    let mut out = Hint::default();
                    out.mime_type(val);
                    out
                });

            let len = headers
                .get(CONTENT_LENGTH)
                .and_then(|val| val.to_str().ok())
                .and_then(|val| val.parse().ok());

            let resume = headers
                .get(ACCEPT_RANGES)
                .and_then(|a| a.to_str().ok())
                .and_then(|a| {
                    if a == "bytes" {
                        Some(self.clone())
                    } else {
                        None
                    }
                });

            let stream = Box::new(StreamReader::new(
                resp.bytes_stream()
                    .map_err(|e| IoError::new(IoErrorKind::Other, e)),
            ));

            let input = HttpStream {
                stream,
                len,
                resume,
            };

            Ok((input, hint))
        }
    }
}

#[pin_project]
struct HttpStream {
    #[pin]
    stream: Box<dyn AsyncRead + Send + Sync + Unpin>,
    len: Option<u64>,
    resume: Option<HttpRequest>,
}

impl AsyncRead for HttpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<IoResult<()>> {
        AsyncRead::poll_read(self.project().stream, cx, buf)
    }
}

impl AsyncSeek for HttpStream {
    fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> IoResult<()> {
        Err(IoErrorKind::Unsupported.into())
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<IoResult<u64>> {
        unreachable!()
    }
}

#[async_trait]
impl AsyncMediaSource for HttpStream {
    fn is_seekable(&self) -> bool {
        false
    }

    async fn byte_len(&self) -> Option<u64> {
        self.len
    }

    async fn try_resume(
        &mut self,
        offset: u64,
    ) -> Result<Box<dyn AsyncMediaSource>, AudioStreamError> {
        if let Some(resume) = &mut self.resume {
            resume
                .create_stream(Some(offset))
                .await
                .map(|a| Box::new(a.0) as Box<dyn AsyncMediaSource>)
        } else {
            Err(AudioStreamError::Unsupported)
        }
    }
}

#[async_trait]
impl Compose for HttpRequest {
    fn create(&mut self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        Err(AudioStreamError::Unsupported)
    }

    async fn create_async(
        &mut self,
    ) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        self.create_stream(None).await.map(|(input, hint)| {
            let stream = AsyncAdapterStream::new(Box::new(input), 64 * 1024);

            AudioStream {
                input: Box::new(stream) as Box<dyn MediaSource>,
                hint,
            }
        })
    }

    fn should_create_async(&self) -> bool {
        true
    }
}

impl From<HttpRequest> for Input {
    fn from(val: HttpRequest) -> Self {
        Input::Lazy(Box::new(val))
    }
}