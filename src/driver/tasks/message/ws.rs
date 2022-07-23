#![allow(missing_docs)]

use super::Interconnect;
use crate::ws::WsStream;

pub enum WsMessage {
    Ws(Box<WsStream>),
    ReplaceInterconnect(Interconnect),
    SetKeepalive(f64),
    Speaking(bool),
}
