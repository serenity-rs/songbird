use super::message::*;
use flume::{Receiver, Sender};
use tracing::{instrument, trace};

pub(crate) fn run() -> Sender<DisposalMessage> {
    let (mix_tx, mix_rx) = flume::unbounded();
    std::thread::spawn(move || {
        trace!("Disposal thread started.");
        runner(mix_rx);
        trace!("Disposal thread finished.");
    });

    mix_tx
}

/// The mixer's disposal thread is also synchronous, due to tracks,
/// inputs, etc. being based on synchronous I/O.
///
/// The mixer uses this to offload heavy and expensive drop operations
/// to prevent deadline misses.
#[instrument(skip(mix_rx))]
pub(crate) fn runner(mix_rx: Receiver<DisposalMessage>) {
    while mix_rx.recv().is_ok() {}
}
