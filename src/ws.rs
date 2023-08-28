use crate::{error::JsonError, model::Event};

use futures::{SinkExt, StreamExt, TryStreamExt};
use tokio::{
    net::TcpStream,
    time::{timeout, Duration},
};
use tokio_tungstenite::{
    tungstenite::{
        error::Error as TungsteniteError,
        protocol::{CloseFrame, WebSocketConfig as Config},
        Message,
    },
    MaybeTlsStream,
    WebSocketStream,
};
use tracing::{debug, instrument};
use url::Url;

pub struct WsStream(WebSocketStream<MaybeTlsStream<TcpStream>>);

impl WsStream {
    #[instrument]
    pub(crate) async fn connect(url: Url) -> Result<Self> {
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
        Ok(crate::json::to_string(value)
            .map(Message::Text)
            .map_err(Error::from)
            .map(|m| self.0.send(m))?
            .await?)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Json(JsonError),

    /// The discord voice gateway does not support or offer zlib compression.
    /// As a result, only text messages are expected.
    UnexpectedBinaryMessage(Vec<u8>),

    Ws(TungsteniteError),

    WsClosed(Option<CloseFrame<'static>>),
}

impl From<JsonError> for Error {
    fn from(e: JsonError) -> Error {
        Error::Json(e)
    }
}

impl From<TungsteniteError> for Error {
    fn from(e: TungsteniteError) -> Error {
        Error::Ws(e)
    }
}

#[inline]
#[allow(unused_unsafe)]
pub(crate) fn convert_ws_message(message: Option<Message>) -> Result<Option<Event>> {
    Ok(match message {
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
    })
}
