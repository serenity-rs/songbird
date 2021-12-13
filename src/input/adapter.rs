#![allow(missing_docs)]

use super::{AudioStreamError, Compose, SymphInput};
use flume::{Receiver, RecvError, Sender, TryRecvError};
use futures::{future::Either, stream::FuturesUnordered, FutureExt, StreamExt, TryStreamExt};
use ringbuf::*;
use std::{
    io::{
        Error as IoError,
        ErrorKind as IoErrorKind,
        Read,
        Result as IoResult,
        Seek,
        SeekFrom,
        Write,
    },
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use symphonia_core::io::MediaSource;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt},
    sync::Notify,
};

struct AsyncAdapterSink {
    bytes_in: Producer<u8>,
    req_rx: Receiver<AdapterRequest>,
    resp_tx: Sender<AdapterResponse>,
    stream: Box<dyn AsyncMediaSource>,
    notify_rx: Arc<Notify>,
}

impl AsyncAdapterSink {
    async fn launch(mut self) {
        let mut inner_buf = [0u8; 1024];
        let mut read_region = 0..0;
        let mut hit_end = false;
        let mut blocked = false;
        let mut pause_buf_moves = false;
        let mut seek_res = None;

        println!("New asyncread");

        loop {
            // if read_region is empty, refill from src.
            //  if that read is zero, tell other half.
            // if WouldBlock, block on msg acquire,
            // else non_block msg acquire.

            if !pause_buf_moves {
                if !hit_end && read_region.is_empty() {
                    // println!("tryna...");
                    if let Ok(n) = self.stream.read(&mut inner_buf).await {
                        // println!("read in {} bytes on asyncland", n);
                        read_region = 0..n;
                        if n == 0 {
                            let _ = self.resp_tx.send_async(AdapterResponse::ReadZero).await;
                            hit_end = true;
                        }
                    } else {
                        break;
                    }
                }

                while !read_region.is_empty() && !blocked {
                    // println!("loopy");
                    if let Ok(n_moved) = self
                        .bytes_in
                        .write(&mut inner_buf[read_region.start..read_region.end])
                    {
                        // println!("copied {} bytes to ring", n_moved);
                        read_region.start += n_moved;
                    } else {
                        blocked = true;
                    }
                }
            }

            let msg = if blocked || hit_end {
                let mut fs = FuturesUnordered::new();
                fs.push(Either::Left(self.req_rx.recv_async()));
                fs.push(Either::Right(self.notify_rx.notified().map(|_| {
                    let o: Result<AdapterRequest, RecvError> = Ok(AdapterRequest::Wake);
                    o
                })));

                match fs.next().await {
                    Some(Ok(a)) => a,
                    _ => break,
                }
            } else {
                match self.req_rx.try_recv() {
                    Ok(a) => a,
                    Err(TryRecvError::Empty) => continue,
                    _ => break,
                }
            };

            match msg {
                AdapterRequest::Wake => blocked = false,
                AdapterRequest::ByteLen => {
                    let _ = self
                        .resp_tx
                        .send_async(AdapterResponse::ByteLen(self.stream.byte_len().await))
                        .await;
                },
                AdapterRequest::Seek(pos) => {
                    pause_buf_moves = true;
                    let _ = self.resp_tx.send_async(AdapterResponse::SeekClear).await;
                    seek_res = Some(self.stream.seek(pos).await);
                },
                AdapterRequest::SeekCleared => {
                    if let Some(res) = seek_res.take() {
                        let _ = self
                            .resp_tx
                            .send_async(AdapterResponse::SeekResult(res))
                            .await;
                    }
                    pause_buf_moves = false;
                },
            }
        }

        println!("Dropped");
    }
}

#[allow(missing_docs)]
pub struct AsyncAdapterStream {
    bytes_out: Consumer<u8>,
    can_seek: bool,
    // Note: this is Atomic just to work around the need for
    // check_messages to take &self rather than &mut.
    finalised: AtomicBool,
    req_tx: Sender<AdapterRequest>,
    resp_rx: Receiver<AdapterResponse>,
    notify_tx: Arc<Notify>,
}

impl AsyncAdapterStream {
    #[allow(missing_docs)]
    pub fn new(stream: Box<dyn AsyncMediaSource>, buf_len: usize) -> AsyncAdapterStream {
        let (bytes_in, bytes_out) = RingBuffer::new(buf_len).split();
        let (resp_tx, resp_rx) = flume::unbounded();
        let (req_tx, req_rx) = flume::unbounded();
        let can_seek = stream.is_seekable();
        let notify_rx = Arc::new(Notify::new());
        let notify_tx = notify_rx.clone();

        let sink = AsyncAdapterSink {
            bytes_in,
            req_rx,
            resp_tx,
            stream,
            notify_rx,
        };
        let stream = AsyncAdapterStream {
            bytes_out,
            can_seek,
            finalised: false.into(),
            req_tx,
            resp_rx,
            notify_tx,
        };

        tokio::spawn(async move {
            sink.launch().await;
        });

        stream
    }

    fn handle_messages(&self, block: bool) -> Option<AdapterResponse> {
        loop {
            match self.resp_rx.try_recv() {
                Ok(AdapterResponse::ReadZero) => {
                    self.finalised.store(true, Ordering::Relaxed);
                },
                Ok(a) => break Some(a),
                Err(TryRecvError::Empty) if !block => break None,
                Err(TryRecvError::Disconnected) => break None,
                Err(TryRecvError::Empty) => {},
            }
        }
    }

    fn is_dropped_and_clear(&self) -> bool {
        self.resp_rx.is_empty() && self.resp_rx.is_disconnected()
    }

    fn check_dropped(&self) -> IoResult<()> {
        if self.is_dropped_and_clear() {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Async half was dropped.",
            ))
        } else {
            Ok(())
        }
    }
}

impl Read for AsyncAdapterStream {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        println!("Read tried");
        // try read:
        // if nothing,
        //  convert to ok 0 if finalised
        //  convert to fatal if dropped
        // if success:
        //  tell other side to wake (i.e., "there should be more writing space").
        // also: need to check msgs.

        // TODO: make this run via condvar instead?
        // This needs to remain blocking or spin loopy
        // Mainly because this is at odds with "keep CPU low."
        loop {
            let _ = self.handle_messages(false);

            match self.bytes_out.read(buf) {
                Ok(n) => {
                    self.notify_tx.notify_one();
                    println!("read {}", n);
                    return Ok(n);
                },
                Err(e) if e.kind() == IoErrorKind::WouldBlock => {
                    // println!("Will it block? {}", self.bytes_out.len());
                    // receive side must ABSOLUTELY be unblocked here.
                    self.notify_tx.notify_one();
                    if self.finalised.load(Ordering::Relaxed) {
                        println!("It's... done?");
                        return Ok(0);
                    } else {
                        self.check_dropped()?;
                        std::hint::spin_loop();
                    }
                },
                a => {
                    println!("Misc err {:?}", a);
                    return a;
                },
            }
        }
    }
}

impl Seek for AsyncAdapterStream {
    fn seek(&mut self, pos: SeekFrom) -> IoResult<u64> {
        if !self.can_seek {
            return Err(IoError::new(
                IoErrorKind::Unsupported,
                "Async half does not support seek operations.",
            ));
        }

        self.check_dropped()?;

        let _ = self.req_tx.send(AdapterRequest::Seek(pos));

        // wait for async to tell us that it has stopped writing,
        // then clear buf and allow async to write again.
        self.finalised.store(false, Ordering::Relaxed);
        match self.handle_messages(true) {
            Some(AdapterResponse::SeekClear) => {},
            None => self.check_dropped().map(|_| unreachable!())?,
            _ => unreachable!(),
        }

        self.bytes_out.discard(self.bytes_out.capacity());

        let _ = self.req_tx.send(AdapterRequest::SeekCleared);

        match self.handle_messages(true) {
            Some(AdapterResponse::SeekResult(a)) => a,
            None => self.check_dropped().map(|_| unreachable!()),
            _ => unreachable!(),
        }
    }
}

impl MediaSource for AsyncAdapterStream {
    fn is_seekable(&self) -> bool {
        self.can_seek
    }

    fn byte_len(&self) -> Option<u64> {
        self.check_dropped().ok()?;

        let _ = self.req_tx.send(AdapterRequest::ByteLen);

        match self.handle_messages(true) {
            Some(AdapterResponse::ByteLen(a)) => a,
            None => self.check_dropped().ok().map(|_| unreachable!()),
            _ => unreachable!(),
        }
    }
}

enum AdapterRequest {
    Wake,
    Seek(SeekFrom),
    SeekCleared,
    ByteLen,
}

enum AdapterResponse {
    SeekResult(IoResult<u64>),
    SeekClear,
    ByteLen(Option<u64>),
    ReadZero,
}

#[async_trait::async_trait]
pub trait AsyncMediaSource: AsyncRead + AsyncSeek + Send + Sync + Unpin {
    fn is_seekable(&self) -> bool;

    async fn byte_len(&self) -> Option<u64>;
}

pub struct HttpRequest {
    pub client: reqwest::Client,
    pub request: String,
}

#[pin_project::pin_project]
struct HttpStream {
    #[pin]
    stream: Box<dyn AsyncRead + Send + Sync + Unpin>,
    len: Option<u64>,
}

impl AsyncRead for HttpStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        AsyncRead::poll_read(self.project().stream, cx, buf)
    }
}

impl AsyncSeek for HttpStream {
    fn start_seek(self: std::pin::Pin<&mut Self>, position: SeekFrom) -> std::io::Result<()> {
        Err(IoErrorKind::Unsupported.into())
    }

    fn poll_complete(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<u64>> {
        unreachable!()
    }
}

#[async_trait::async_trait]
impl AsyncMediaSource for HttpStream {
    fn is_seekable(&self) -> bool {
        false
    }

    async fn byte_len(&self) -> Option<u64> {
        self.len
    }
}

#[async_trait::async_trait]
impl Compose for HttpRequest {
    fn create(
        &mut self,
    ) -> Result<super::AudioStream<Box<dyn MediaSource>>, super::AudioStreamError> {
        unimplemented!()
    }

    async fn create_async(
        &mut self,
    ) -> Result<super::AudioStream<Box<dyn MediaSource>>, super::AudioStreamError> {
        let resp = self
            .client
            .get(&self.request)
            .send()
            .await
            .map_err(|e| AudioStreamError::Fail(Box::new(e)))?;

        if let Some(t) = resp.headers().get(reqwest::header::RETRY_AFTER) {
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
            let hint = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|val| val.to_str().ok())
                .map(|val| {
                    let mut out: symphonia_core::probe::Hint = Default::default();
                    out.mime_type(val);
                    out
                });

            let len = resp
                .headers()
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|val| val.to_str().ok())
                .and_then(|val| val.parse().ok());

            let stream = Box::new(tokio_util::io::StreamReader::new(
                resp.bytes_stream()
                    .map_err(|e| IoError::new(IoErrorKind::Other, e)),
            ));
            let input = HttpStream { stream, len };
            let stream = AsyncAdapterStream::new(Box::new(input), 1024 * 1024);

            Ok(super::AudioStream {
                input: Box::new(stream),
                hint,
            })
        }
    }

    fn should_create_async(&self) -> bool {
        true
    }
}

impl From<HttpRequest> for SymphInput {
    fn from(val: HttpRequest) -> Self {
        SymphInput::Lazy(Box::new(val))
    }
}
