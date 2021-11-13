use super::{Interconnect, MixerInputResultMessage};
use crate::{
    input::{Compose, LiveInput},
    Config,
};
use flume::Sender;

pub enum InputParseMessage {
    Promote(
        Sender<MixerInputResultMessage>,
        LiveInput,
        Option<Box<dyn Compose>>,
    ),
    Config(Config),
    ReplaceInterconnect(Interconnect),
    Poison,
}
