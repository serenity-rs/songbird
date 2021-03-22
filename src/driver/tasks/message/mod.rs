#![allow(missing_docs)]

mod core;
mod disposal;
mod events;
mod mixer;
mod udp_rx;
mod udp_tx;
mod ws;

pub use self::{core::*, disposal::*, events::*, mixer::*, udp_rx::*, udp_tx::*, ws::*};

use flume::Sender;
#[cfg(not(feature = "tokio-02-marker"))]
use tokio::spawn;
#[cfg(feature = "tokio-02-marker")]
use tokio_compat::spawn;
use tracing::trace;

#[derive(Clone, Debug)]
pub struct Interconnect {
    pub core: Sender<CoreMessage>,
    pub events: Sender<EventMessage>,
    pub mixer: Sender<MixerMessage>,
}

impl Interconnect {
    pub fn poison(&self) {
        let _ = self.events.send(EventMessage::Poison);
    }

    pub fn poison_all(&self) {
        let _ = self.mixer.send(MixerMessage::Poison);
        self.poison();
    }

    pub fn restart_volatile_internals(&mut self) {
        self.poison();

        let (evt_tx, evt_rx) = flume::unbounded();

        self.events = evt_tx;

        let ic = self.clone();
        spawn(async move {
            trace!("Event processor restarted.");
            super::events::runner(ic, evt_rx).await;
            trace!("Event processor finished.");
        });

        // Make mixer aware of new targets...
        let _ = self
            .mixer
            .send(MixerMessage::ReplaceInterconnect(self.clone()));
    }
}
