use crate::{error::JsonError, model::Event};

use futures::{SinkExt, StreamExt, TryStreamExt};
use tokio::{
    net::TcpStream,
    time::{timeout, Duration},
};
#[cfg(feature = "tungstenite")]
use tokio_tungstenite::{
    tungstenite::{
        error::Error as TungsteniteError,
        protocol::{CloseFrame, WebSocketConfig as Config},
        Message,
    },
    MaybeTlsStream,
    WebSocketStream,
};
#[cfg(feature = "tws")]
use tokio_websockets::{
    CloseCode,
    Error as TwsError,
    Limits,
    MaybeTlsStream,
    Message,
    WebSocketStream,
};
use tracing::{debug, instrument};
use url::Url;

#[cfg(any(
    all(feature = "tws", feature = "tungstenite"),
    all(not(feature = "tws"), not(feature = "tungstenite"))
))]
compile_error!("specify one of `features = [\"tungstenite\"]` (recommended w/ serenity) or `features = [\"tws\"]` (recommended w/ twilight)");

pub struct WsStream(WebSocketStream<MaybeTlsStream<TcpStream>>);

impl WsStream {
    #[instrument]
    pub(crate) async fn connect(url: Url) -> Result<Self> {
        #[cfg(feature = "tungstenite")]
        let (stream, _) = tokio_tungstenite::connect_async_with_config::<Url>(
            url,
            Some(Config {
                max_message_size: None,
                max_frame_size: None,
                ..Default::default()
            }),
            true,
        )
        .await?;
        #[cfg(feature = "tws")]
        let (stream, _) = tokio_websockets::ClientBuilder::new()
            .limits(Limits::unlimited())
            .uri(url.as_str())
            .unwrap() // Any valid URL is a valid URI.
            .connect()
            .await?;

        Ok(Self(stream))
    }

    pub(crate) async fn recv_json(&mut self) -> Result<Option<Event>> {
        const TIMEOUT: Duration = Duration::from_millis(500);

        let ws_message = match timeout(TIMEOUT, self.0.next()).await {
            Ok(Some(Ok(v))) => Some(v),
            Ok(Some(Err(e))) => return Err(e.into()),
            Ok(None) | Err(_) => None,
        };

        convert_ws_message(ws_message)
    }

    pub(crate) async fn recv_json_no_timeout(&mut self) -> Result<Option<Event>> {
        convert_ws_message(self.0.try_next().await?)
    }

    pub(crate) async fn send_json(&mut self, value: &Event) -> Result<()> {
        let res = crate::json::to_string(value);
        #[cfg(feature = "tungstenite")]
        let res = res.map(Message::Text);
        #[cfg(feature = "tws")]
        let res = res.map(Message::text);
        Ok(res.map_err(Error::from).map(|m| self.0.send(m))?.await?)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Json(JsonError),

    /// The discord voice gateway does not support or offer zlib compression.
    /// As a result, only text messages are expected.
    UnexpectedBinaryMessage(Vec<u8>),

    #[cfg(feature = "tungstenite")]
    Ws(TungsteniteError),
    #[cfg(feature = "tws")]
    Ws(TwsError),

    #[cfg(feature = "tungstenite")]
    WsClosed(Option<CloseFrame<'static>>),
    #[cfg(feature = "tws")]
    WsClosed(Option<CloseCode>),
}

impl From<JsonError> for Error {
    fn from(e: JsonError) -> Error {
        Error::Json(e)
    }
}

#[cfg(feature = "tungstenite")]
impl From<TungsteniteError> for Error {
    fn from(e: TungsteniteError) -> Error {
        Error::Ws(e)
    }
}

#[cfg(feature = "tws")]
impl From<TwsError> for Error {
    fn from(e: TwsError) -> Self {
        Error::Ws(e)
    }
}

#[inline]
#[allow(unused_unsafe)]
pub(crate) fn convert_ws_message(message: Option<Message>) -> Result<Option<Event>> {
    #[cfg(feature = "tungstenite")]
    return Ok(match message {
        // SAFETY:
        // simd-json::serde::from_str may leave an &mut str in a non-UTF state on failure.
        // The below is safe as we have taken ownership of the inner `String`, and if
        // failure occurs we forcibly re-validate its contents before logging.
        Some(Message::Text(mut payload)) =>
            (unsafe { crate::json::from_str(payload.as_mut_str()) })
                .map_err(|e| {
                    let safe_payload = String::from_utf8_lossy(payload.as_bytes());
                    debug!("Unexpected JSON: {e}. Payload: {safe_payload}");
                    e
                })
                .ok(),
        Some(Message::Binary(bytes)) => {
            return Err(Error::UnexpectedBinaryMessage(bytes));
        },
        Some(Message::Close(Some(frame))) => {
            return Err(Error::WsClosed(Some(frame)));
        },
        // Ping/Pong message behaviour is internally handled by tungstenite.
        _ => None,
    });
    #[cfg(feature = "tws")]
    return Ok(if let Some(message) = message {
        if message.is_text() {
            let mut payload = message.as_text().unwrap().to_owned();
            // SAFETY:
            // simd-json::serde::from_str may leave an &mut str in a non-UTF state on failure.
            // The below is safe as we have created an owned copy of the payload `&str`, and if
            // failure occurs we forcibly re-validate its contents before logging.
            (unsafe { crate::json::from_str(payload.as_mut_str()) })
                .map_err(|e| {
                    let safe_payload = String::from_utf8_lossy(payload.as_bytes());
                    debug!("Unexpected JSON: {e}. Payload: {safe_payload}");
                    e
                })
                .ok()
        } else if message.is_binary() {
            return Err(Error::UnexpectedBinaryMessage(
                message.into_payload().to_vec(),
            ));
        } else if message.is_close() {
            return Err(Error::WsClosed(message.as_close().map(|(c, _)| c)));
        } else {
            // ping/pong; will also be internally handled by tokio-websockets
            None
        }
    } else {
        None
    });
}
