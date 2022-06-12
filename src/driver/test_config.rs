#![allow(missing_docs)]

use flume::{Receiver, Sender};

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum TickStyle {
    Timed,
    UntimedWithExecLimit(Receiver<u64>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum OutputMessage {
    Passthrough(Vec<u8>),
    Mixed(Vec<f32>),
    Silent,
}

#[allow(dead_code)]
impl OutputMessage {
    pub fn is_passthrough(&self) -> bool {
        matches!(self, Self::Passthrough(_))
    }

    pub fn is_mixed(&self) -> bool {
        matches!(self, Self::Mixed(_))
    }

    pub fn is_mixed_with_nonzero_signal(&self) -> bool {
        if let Self::Mixed(data) = self {
            data.iter().any(|v| *v != 0.0f32)
        } else {
            false
        }
    }

    pub fn is_explicit_silence(&self) -> bool {
        *self == Self::Silent
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum OutputMode {
    Raw(Sender<TickMessage<OutputMessage>>),
    Rtp(Sender<TickMessage<Vec<u8>>>),
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum TickMessage<T> {
    El(T),
    NoEl,
}

impl<T> From<T> for TickMessage<T> {
    fn from(val: T) -> Self {
        TickMessage::El(val)
    }
}

#[cfg(test)]
impl From<TickMessage<OutputMessage>> for OutputPacket {
    fn from(val: TickMessage<OutputMessage>) -> Self {
        match val {
            TickMessage::El(e) => OutputPacket::Raw(e),
            TickMessage::NoEl => OutputPacket::Empty,
        }
    }
}

#[cfg(test)]
impl From<TickMessage<Vec<u8>>> for OutputPacket {
    fn from(val: TickMessage<Vec<u8>>) -> Self {
        match val {
            TickMessage::El(e) => OutputPacket::Rtp(e),
            TickMessage::NoEl => OutputPacket::Empty,
        }
    }
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub enum OutputPacket {
    Raw(OutputMessage),
    Rtp(Vec<u8>),
    Empty,
}

#[cfg(test)]
impl OutputPacket {
    pub fn raw(&self) -> Option<&OutputMessage> {
        if let Self::Raw(o) = self {
            Some(o)
        } else {
            None
        }
    }
}

#[cfg(test)]
#[derive(Clone, Debug)]
pub enum OutputReceiver {
    Raw(Receiver<TickMessage<OutputMessage>>),
    Rtp(Receiver<TickMessage<Vec<u8>>>),
}

#[cfg(test)]
pub struct DriverTestHandle {
    pub rx: OutputReceiver,
    pub tx: Sender<u64>,
}

#[cfg(test)]
impl DriverTestHandle {
    pub fn recv(&self) -> OutputPacket {
        match &self.rx {
            OutputReceiver::Raw(rx) => rx.recv().unwrap().into(),
            OutputReceiver::Rtp(rx) => rx.recv().unwrap().into(),
        }
    }

    pub fn wait(&self, n_ticks: u64) {
        for _i in 0..n_ticks {
            drop(self.recv());
        }
    }

    pub fn tick(&self, n_ticks: u64) {
        if n_ticks == 0 {
            panic!("Number of ticks to advance driver/mixer must be >= 1.");
        }
        self.tx.send(n_ticks).unwrap();
    }
}
