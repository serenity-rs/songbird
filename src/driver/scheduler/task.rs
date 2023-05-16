use std::time::Instant;

use flume::{Receiver, Sender};
use nohash_hasher::IsEnabled;
use rand::random;
use tokio::runtime::Handle;

use crate::{
    driver::tasks::{
        error::Error as DriverError,
        message::{EventMessage, Interconnect, MixerMessage},
        mixer::Mixer,
    },
    Config,
};

use super::SchedulerMessage;

#[allow(missing_docs)]
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TaskId(usize);

impl IsEnabled for TaskId {}

#[allow(missing_docs)]
impl TaskId {
    pub fn new() -> Self {
        TaskId(0)
    }

    pub fn incr(&mut self) -> Self {
        let out = *self;
        self.0 = self.0.wrapping_add(1);
        out
    }

    #[cfg(any(test, feature = "internals"))]
    pub fn get(&self) -> usize {
        self.0
    }
}

#[allow(missing_docs)]
pub struct ParkedMixer {
    pub mixer: Box<Mixer>,
    pub ssrc: u32,
    pub rtp_sequence: u16,
    pub rtp_timestamp: u32,
    pub park_time: Instant,
}

#[allow(missing_docs)]
impl ParkedMixer {
    pub fn new(mix_rx: Receiver<MixerMessage>, interconnect: Interconnect, config: Config) -> Self {
        Self {
            mixer: Box::new(Mixer::new(mix_rx, Handle::current(), interconnect, config)),
            ssrc: 0,
            rtp_sequence: random::<u16>(),
            rtp_timestamp: random::<u32>(),
            park_time: Instant::now(),
        }
    }

    pub fn spawn_forwarder(&self, tx: Sender<SchedulerMessage>, id: TaskId) {
        let remote_rx = self.mixer.mix_rx.clone();
        tokio::spawn(async move {
            while let Ok(msg) = remote_rx.recv_async().await {
                let exit = msg.is_mixer_now_live();
                let dead = tx.send_async(SchedulerMessage::Do(id, msg)).await.is_err();
                if exit || dead {
                    break;
                }
            }
        });
    }

    /// Returns whether the mixer should exit and be cleaned up.
    pub fn handle_message(&mut self, msg: MixerMessage) -> Result<bool, ()> {
        match msg {
            MixerMessage::SetConn(conn, ssrc) => {
                // Overridden because
                self.ssrc = ssrc;
                self.rtp_sequence = random::<u16>();
                self.rtp_timestamp = random::<u32>();
                self.mixer.conn_active = Some(conn);
                self.mixer.update_keepalive(ssrc);

                Ok(false)
            },
            MixerMessage::Ws(ws) => {
                // Overridden so that we don't mistakenly tell Discord we're speaking.
                self.mixer.ws = ws;
                self.send_gateway_not_speaking();

                Ok(false)
            },
            msg => {
                let (events_failure, conn_failure, should_exit) =
                    self.mixer.handle_message(msg, &mut []);

                self.mixer
                    .do_rebuilds(events_failure, conn_failure)
                    .map_err(|_| ())
                    .map(|_| should_exit)
            },
        }
    }

    pub fn tick_and_keepalive(&mut self, now: Instant) -> Result<(), ()> {
        let mut events_failure = self.mixer.fire_event(EventMessage::Tick).is_err();

        let ka_err = self
            .mixer
            .check_and_send_keepalive(Some(now))
            .or_else(DriverError::disarm_would_block);

        let conn_failure = if let Err(e) = ka_err {
            events_failure |= e.should_trigger_interconnect_rebuild();
            e.should_trigger_connect()
        } else {
            false
        };

        self.mixer
            .do_rebuilds(events_failure, conn_failure)
            .map_err(|_| ())
    }

    pub fn send_gateway_speaking(&mut self) -> Result<(), ()> {
        if let Err(e) = self.mixer.send_gateway_speaking() {
            self.mixer
                .do_rebuilds(
                    e.should_trigger_interconnect_rebuild(),
                    e.should_trigger_connect(),
                )
                .map_err(|_| ())
        } else {
            Ok(())
        }
    }

    pub fn send_gateway_not_speaking(&self) {
        self.mixer.send_gateway_not_speaking();
    }
}
