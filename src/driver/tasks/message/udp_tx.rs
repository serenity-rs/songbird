#![allow(missing_docs)]

pub enum UdpTxMessage {
    Packet(Vec<u8>), // TODO: do something cheaper.
    Poison,
}
