pub mod error;

#[cfg(feature = "receive")]
use super::tasks::udp_rx;
use super::{
    tasks::{message::*, udp_tx, ws as ws_task},
    Config,
    CryptoMode,
};
use crate::{
    constants::*,
    model::{
        payload::{Identify, Resume, SelectProtocol},
        Event as GatewayEvent,
        ProtocolData,
    },
    ws::WsStream,
    ConnectionInfo,
};
use discortp::discord::{IpDiscoveryPacket, IpDiscoveryType, MutableIpDiscoveryPacket};
use error::{Error, Result};
use flume::Sender;
use socket2::Socket;
#[cfg(feature = "receive")]
use std::sync::Arc;
use std::{net::IpAddr, str::FromStr};
use tokio::{net::UdpSocket, spawn, time::timeout};
use tracing::{debug, info, instrument};
use url::Url;
use xsalsa20poly1305::{aead::NewAead, XSalsa20Poly1305 as Cipher};

pub(crate) struct Connection {
    pub(crate) info: ConnectionInfo,
    pub(crate) ssrc: u32,
    pub(crate) ws: Sender<WsMessage>,
}

impl Connection {
    pub(crate) async fn new(
        info: ConnectionInfo,
        interconnect: &Interconnect,
        config: &Config,
        idx: usize,
    ) -> Result<Connection> {
        if let Some(t) = config.driver_timeout {
            timeout(t, Connection::new_inner(info, interconnect, config, idx)).await?
        } else {
            Connection::new_inner(info, interconnect, config, idx).await
        }
    }

    pub(crate) async fn new_inner(
        mut info: ConnectionInfo,
        interconnect: &Interconnect,
        config: &Config,
        idx: usize,
    ) -> Result<Connection> {
        let url = generate_url(&mut info.endpoint)?;

        let mut client = WsStream::connect(url).await?;

        let mut hello = None;
        let mut ready = None;

        client
            .send_json(&GatewayEvent::from(Identify {
                server_id: info.guild_id.into(),
                session_id: info.session_id.clone(),
                token: info.token.clone(),
                user_id: info.user_id.into(),
            }))
            .await?;

        loop {
            let value = match client.recv_json().await? {
                Some(value) => value,
                None => continue,
            };

            match value {
                GatewayEvent::Ready(r) => {
                    ready = Some(r);
                    if hello.is_some() {
                        break;
                    }
                },
                GatewayEvent::Hello(h) => {
                    hello = Some(h);
                    if ready.is_some() {
                        break;
                    }
                },
                other => {
                    debug!("Expected ready/hello; got: {:?}", other);

                    return Err(Error::ExpectedHandshake);
                },
            }
        }

        let hello =
            hello.expect("Hello packet expected in connection initialisation, but not found.");
        let ready =
            ready.expect("Ready packet expected in connection initialisation, but not found.");

        if !has_valid_mode(&ready.modes, config.crypto_mode) {
            return Err(Error::CryptoModeUnavailable);
        }

        let udp = UdpSocket::bind("0.0.0.0:0").await?;

        // Optimisation for non-receive case: set rx buffer size to zero.
        let udp = if cfg!(feature = "receive") {
            udp
        } else {
            let socket = Socket::from(udp.into_std()?);
            socket.set_recv_buffer_size(0)?;
            UdpSocket::from_std(socket.into())?
        };

        udp.connect((ready.ip, ready.port)).await?;

        // Follow Discord's IP Discovery procedures, in case NAT tunnelling is needed.
        let mut bytes = [0; IpDiscoveryPacket::const_packet_size()];
        {
            let mut view = MutableIpDiscoveryPacket::new(&mut bytes[..]).expect(
                "Too few bytes in 'bytes' for IPDiscovery packet.\
                    (Blame: IpDiscoveryPacket::const_packet_size()?)",
            );
            view.set_pkt_type(IpDiscoveryType::Request);
            view.set_length(70);
            view.set_ssrc(ready.ssrc);
        }

        udp.send(&bytes).await?;

        let (len, _addr) = udp.recv_from(&mut bytes).await?;
        {
            let view =
                IpDiscoveryPacket::new(&bytes[..len]).ok_or(Error::IllegalDiscoveryResponse)?;

            if view.get_pkt_type() != IpDiscoveryType::Response {
                return Err(Error::IllegalDiscoveryResponse);
            }

            // We could do something clever like binary search,
            // but possibility of UDP spoofing precludes us from
            // making the assumption we can find a "left edge" of '\0's.
            let nul_byte_index = view
                .get_address_raw()
                .iter()
                .position(|&b| b == 0)
                .ok_or(Error::IllegalIp)?;

            let address_str = std::str::from_utf8(&view.get_address_raw()[..nul_byte_index])
                .map_err(|_| Error::IllegalIp)?;

            let address = IpAddr::from_str(address_str).map_err(|e| {
                println!("{:?}", e);
                Error::IllegalIp
            })?;

            client
                .send_json(&GatewayEvent::from(SelectProtocol {
                    protocol: "udp".into(),
                    data: ProtocolData {
                        address,
                        mode: config.crypto_mode.to_request_str().into(),
                        port: view.get_port(),
                    },
                }))
                .await?;
        }

        let cipher = init_cipher(&mut client, config.crypto_mode).await?;

        info!("Connected to: {}", info.endpoint);

        info!("WS heartbeat duration {}ms.", hello.heartbeat_interval,);

        let (ws_msg_tx, ws_msg_rx) = flume::unbounded();
        let (udp_sender_msg_tx, udp_sender_msg_rx) = flume::unbounded();
        #[cfg(feature = "receive")]
        let (udp_receiver_msg_tx, udp_receiver_msg_rx) = flume::unbounded();

        #[cfg(feature = "receive")]
        let (udp_rx, udp_tx) = {
            let udp_rx = Arc::new(udp);
            let udp_tx = Arc::clone(&udp_rx);
            (udp_rx, udp_tx)
        };
        #[cfg(not(feature = "receive"))]
        let udp_tx = udp;

        let ssrc = ready.ssrc;

        let mix_conn = MixerConnection {
            cipher: cipher.clone(),
            crypto_state: config.crypto_mode.into(),
            #[cfg(feature = "receive")]
            udp_rx: udp_receiver_msg_tx,
            udp_tx: udp_sender_msg_tx,
        };

        interconnect
            .mixer
            .send(MixerMessage::Ws(Some(ws_msg_tx.clone())))?;

        interconnect
            .mixer
            .send(MixerMessage::SetConn(mix_conn, ready.ssrc))?;

        spawn(ws_task::runner(
            interconnect.clone(),
            ws_msg_rx,
            client,
            ssrc,
            hello.heartbeat_interval,
            idx,
            info.clone(),
        ));

        #[cfg(feature = "receive")]
        spawn(udp_rx::runner(
            interconnect.clone(),
            udp_receiver_msg_rx,
            cipher,
            config.clone(),
            udp_rx,
        ));
        spawn(udp_tx::runner(udp_sender_msg_rx, ssrc, udp_tx));

        Ok(Connection {
            info,
            ssrc,
            ws: ws_msg_tx,
        })
    }

    #[instrument(skip(self))]
    pub async fn reconnect(&mut self, config: &Config) -> Result<()> {
        if let Some(t) = config.driver_timeout {
            timeout(t, self.reconnect_inner()).await?
        } else {
            self.reconnect_inner().await
        }
    }

    #[instrument(skip(self))]
    pub async fn reconnect_inner(&mut self) -> Result<()> {
        let url = generate_url(&mut self.info.endpoint)?;

        // Thread may have died, we want to send to prompt a clean exit
        // (if at all possible) and then proceed as normal.
        let mut client = WsStream::connect(url).await?;

        client
            .send_json(&GatewayEvent::from(Resume {
                server_id: self.info.guild_id.into(),
                session_id: self.info.session_id.clone(),
                token: self.info.token.clone(),
            }))
            .await?;

        let mut hello = None;
        let mut resumed = None;

        loop {
            let value = match client.recv_json().await? {
                Some(value) => value,
                None => continue,
            };

            match value {
                GatewayEvent::Resumed => {
                    resumed = Some(());
                    if hello.is_some() {
                        break;
                    }
                },
                GatewayEvent::Hello(h) => {
                    hello = Some(h);
                    if resumed.is_some() {
                        break;
                    }
                },
                other => {
                    debug!("Expected resumed/hello; got: {:?}", other);

                    return Err(Error::ExpectedHandshake);
                },
            }
        }

        let hello =
            hello.expect("Hello packet expected in connection initialisation, but not found.");

        self.ws
            .send(WsMessage::SetKeepalive(hello.heartbeat_interval))?;
        self.ws.send(WsMessage::Ws(Box::new(client)))?;

        info!("Reconnected to: {}", &self.info.endpoint);
        Ok(())
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        info!("Disconnected");
    }
}

fn generate_url(endpoint: &mut String) -> Result<Url> {
    if endpoint.ends_with(":80") {
        let len = endpoint.len();

        endpoint.truncate(len - 3);
    }

    Url::parse(&format!("wss://{}/?v={}", endpoint, VOICE_GATEWAY_VERSION))
        .or(Err(Error::EndpointUrl))
}

#[inline]
async fn init_cipher(client: &mut WsStream, mode: CryptoMode) -> Result<Cipher> {
    loop {
        let value = match client.recv_json().await? {
            Some(value) => value,
            None => continue,
        };

        match value {
            GatewayEvent::SessionDescription(desc) => {
                if desc.mode != mode.to_request_str() {
                    return Err(Error::CryptoModeInvalid);
                }

                return Ok(Cipher::new_from_slice(&desc.secret_key)?);
            },
            other => {
                debug!(
                    "Expected ready for key; got: op{}/v{:?}",
                    other.kind() as u8,
                    other
                );
            },
        }
    }
}

#[inline]
fn has_valid_mode<T, It>(modes: It, mode: CryptoMode) -> bool
where
    T: for<'a> PartialEq<&'a str>,
    It: IntoIterator<Item = T>,
{
    modes.into_iter().any(|s| s == mode.to_request_str())
}
