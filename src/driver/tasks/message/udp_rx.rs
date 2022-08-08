#![allow(missing_docs)]

use super::Interconnect;
use crate::driver::Config;
use dashmap::{DashMap, DashSet};
use serenity_voice_model::id::UserId;

pub enum UdpRxMessage {
    SetConfig(Config),
    ReplaceInterconnect(Interconnect),
}

#[derive(Debug, Default)]
pub struct SsrcTracker {
    pub disconnected_users: DashSet<UserId>,
    pub user_ssrc_map: DashMap<UserId, u32>,
}
