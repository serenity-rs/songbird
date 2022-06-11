#![allow(missing_docs)]

use flume::{Receiver, Sender};

#[derive(Clone, Debug)]
pub enum TickStyle {
    Timed,
    UntimedWithExecLimit(u64),
    ManualControl(Receiver<()>),
}

#[derive(Clone, Debug)]
pub enum OutputMessage {
    Passthrough(Vec<u8>),
    Mixed(Vec<f32>),
}

#[derive(Clone, Debug)]
pub enum OutputMode {
    Raw(Sender<OutputMessage>),
    Rtp(Sender<Vec<u8>>),
}
