use super::message::*;
use crate::constants::*;
use discortp::discord::MutableKeepalivePacket;
use flume::Receiver;
#[cfg(feature = "receive")]
use std::sync::Arc;
use tokio::{
    net::UdpSocket,
    time::{timeout_at, Instant},
};
use tracing::{error, instrument, trace};

struct UdpTx {
    ssrc: u32,
    rx: Receiver<UdpTxMessage>,
    #[cfg(feature = "receive")]
    udp_tx: Arc<UdpSocket>,
    #[cfg(not(feature = "receive"))]
    udp_tx: UdpSocket,
}

impl UdpTx {
    async fn run(&mut self) {
        let mut keepalive_bytes = [0u8; MutableKeepalivePacket::minimum_packet_size()];
        let mut ka = MutableKeepalivePacket::new(&mut keepalive_bytes[..])
            .expect("FATAL: Insufficient bytes given to keepalive packet.");
        ka.set_ssrc(self.ssrc);

        let mut ka_time = Instant::now() + UDP_KEEPALIVE_GAP;

        loop {
            match timeout_at(ka_time, self.rx.recv_async()).await {
                Err(_) => {
                    trace!("Sending UDP Keepalive.");
                    if let Err(e) = self.udp_tx.send(&keepalive_bytes[..]).await {
                        error!("Fatal UDP keepalive send error: {:?}.", e);
                        break;
                    }
                    ka_time += UDP_KEEPALIVE_GAP;
                },
                Ok(Ok(p)) =>
                    if let Err(e) = self.udp_tx.send(&p[..]).await {
                        error!("Fatal UDP packet send error: {:?}.", e);
                        break;
                    },
                Ok(Err(flume::RecvError::Disconnected)) => {
                    break;
                },
            }
        }
    }
}

#[instrument(skip(udp_msg_rx))]
pub(crate) async fn runner(
    udp_msg_rx: Receiver<UdpTxMessage>,
    ssrc: u32,
    #[cfg(feature = "receive")] udp_tx: Arc<UdpSocket>,
    #[cfg(not(feature = "receive"))] udp_tx: UdpSocket,
) {
    trace!("UDP transmit handle started.");

    let mut txer = UdpTx {
        ssrc,
        rx: udp_msg_rx,
        udp_tx,
    };

    txer.run().await;

    trace!("UDP transmit handle stopped.");
}
