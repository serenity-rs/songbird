pub mod error;

#[cfg(feature = "receive")]
use super::tasks::udp_rx;
use super::{
    crypto::Cipher,
    tasks::{
        message::*,
        ws::{self as ws_task, AuxNetwork},
    },
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
        let (ws_msg_tx, ws_msg_rx) = flume::unbounded();

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
            let Some(value) = client.recv_json().await? else {
                continue;
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
                    // Discord hold back per-user connection state until after this handshake.
                    // There's no guarantee that will remain the case, so buffer it like all
                    // subsequent steps where we know they *do* send these packets.
                    debug!("Expected ready/hello; got: {:?}", other);
                    ws_msg_tx.send(WsMessage::Deliver(other))?;
                },
            }
        }

        let hello =
            hello.expect("Hello packet expected in connection initialisation, but not found.");
        let ready =
            ready.expect("Ready packet expected in connection initialisation, but not found.");

        let chosen_crypto = CryptoMode::negotiate(&ready.modes, Some(config.crypto_mode))?;

        info!(
            "Crypto scheme negotiation -- wanted {:?}. Chose {:?} from modes {:?}.",
            config.crypto_mode, chosen_crypto, ready.modes
        );

        let udp = UdpSocket::bind("0.0.0.0:0").await?;

        // Optimisation for non-receive case: set rx buffer size to zero.
        let udp = if cfg!(feature = "receive") {
            udp
        } else {
            let socket = Socket::from(udp.into_std()?);

            // Some operating systems do not allow setting the recv buffer to 0.
            #[cfg(any(target_os = "linux", target_os = "windows"))]
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

            let address = IpAddr::from_str(address_str).map_err(|_| Error::IllegalIp)?;

            client
                .send_json(&GatewayEvent::from(SelectProtocol {
                    protocol: "udp".into(),
                    data: ProtocolData {
                        address,
                        mode: chosen_crypto.to_request_str().into(),
                        port: view.get_port(),
                    },
                }))
                .await?;
        }

        let cipher = init_cipher(&mut client, chosen_crypto, &ws_msg_tx).await?;

        info!("Connected to: {}", info.endpoint);

        info!("WS heartbeat duration {}ms.", hello.heartbeat_interval);

        #[cfg(feature = "receive")]
        let (udp_receiver_msg_tx, udp_receiver_msg_rx) = flume::unbounded();

        // NOTE: This causes the UDP Socket on "receive" to be non-blocking,
        // and the standard to be blocking. A UDP send should only WouldBlock if
        // you're sending more data than the OS can handle (not likely, and
        // at that point you should scale horizontally).
        //
        // If this is a problem for anyone, we can make non-blocking sends
        // queue up a delayed send up to a limit.
        #[cfg(feature = "receive")]
        let (udp_rx, udp_tx) = {
            let udp_tx = udp.into_std()?;
            let udp_rx = UdpSocket::from_std(udp_tx.try_clone()?)?;
            (udp_rx, udp_tx)
        };
        #[cfg(not(feature = "receive"))]
        let udp_tx = udp.into_std()?;

        let ssrc = ready.ssrc;

        let mix_conn = MixerConnection {
            #[cfg(feature = "receive")]
            cipher: cipher.clone(),
            #[cfg(not(feature = "receive"))]
            cipher,
            crypto_state: chosen_crypto.into(),
            #[cfg(feature = "receive")]
            udp_rx: udp_receiver_msg_tx,
            udp_tx,
        };

        interconnect
            .mixer
            .send(MixerMessage::Ws(Some(ws_msg_tx.clone())))?;

        interconnect
            .mixer
            .send(MixerMessage::SetConn(mix_conn, ready.ssrc))?;

        #[cfg(feature = "receive")]
        let ssrc_tracker = Arc::new(SsrcTracker::default());

        let ws_state = AuxNetwork::new(
            ws_msg_rx,
            client,
            ssrc,
            hello.heartbeat_interval,
            idx,
            info.clone(),
            #[cfg(feature = "receive")]
            ssrc_tracker.clone(),
        );

        spawn(ws_task::runner(interconnect.clone(), ws_state));

        #[cfg(feature = "receive")]
        spawn(udp_rx::runner(
            interconnect.clone(),
            udp_receiver_msg_rx,
            cipher,
            chosen_crypto,
            config.clone(),
            udp_rx,
            ssrc_tracker,
        ));

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
            let Some(value) = client.recv_json().await? else {
                continue;
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
                    self.ws.send(WsMessage::Deliver(other))?;
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

    Url::parse(&format!("wss://{endpoint}/?v={VOICE_GATEWAY_VERSION}")).or(Err(Error::EndpointUrl))
}

#[inline]
async fn init_cipher(
    client: &mut WsStream,
    mode: CryptoMode,
    tx: &Sender<WsMessage>,
) -> Result<Cipher> {
    loop {
        let Some(value) = client.recv_json().await? else {
            continue;
        };

        match value {
            GatewayEvent::SessionDescription(desc) => {
                if desc.mode != mode.to_request_str() {
                    return Err(Error::CryptoModeInvalid);
                }

                return mode
                    .cipher_from_key(&desc.secret_key)
                    .map_err(|_| Error::CryptoInvalidLength);
            },
            other => {
                // Discord can and will send user-specific payload packets during this time
                // which are needed to map SSRCs to `UserId`s.
                tx.send(WsMessage::Deliver(other))?;
            },
        }
    }
}
