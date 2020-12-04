#![allow(missing_docs)]

use super::Interconnect;
use crate::ws::WsStream;

#[allow(dead_code)]
pub enum WsMessage {
    Ws(Box<WsStream>),
    ReplaceInterconnect(Interconnect),
    SetKeepalive(f64),
    Speaking(bool),

    Poison,
}
