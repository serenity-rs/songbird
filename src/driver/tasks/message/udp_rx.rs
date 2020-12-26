#![allow(missing_docs)]

use super::Interconnect;
use crate::driver::Config;

pub enum UdpRxMessage {
    SetConfig(Config),
    ReplaceInterconnect(Interconnect),

    Poison,
}
