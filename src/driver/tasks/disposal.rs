use super::message::*;
use flume::Receiver;
use tracing::instrument;

/// The mixer's disposal thread is also synchronous, due to tracks,
/// inputs, etc. being based on synchronous I/O.
///
/// The mixer uses this to offload heavy and expensive drop operations
/// to prevent deadline misses.
#[instrument(skip(mix_rx))]
pub(crate) fn runner(mix_rx: Receiver<DisposalMessage>) {
    loop {
        match mix_rx.recv() {
            Err(_) | Ok(DisposalMessage::Poison) => break,
            _ => {},
        }
    }
}
