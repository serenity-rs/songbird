use super::MixerInputResultMessage;
use crate::input::SymphInput;
use flume::Sender;

pub enum InputCreateMessage {
    Create(Sender<MixerInputResultMessage>, SymphInput),
    Poison,
}
