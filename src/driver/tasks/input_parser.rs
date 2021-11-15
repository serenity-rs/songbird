//! The input parser looks at audio bytestreams to extract decoder and format information
//! needed to .
//!
//! Bytestreams are Read/Seek in accordance with Symphonia, so this module is synchronous.

use super::message::{InputParseMessage, MixerInputResultMessage};

use crate::{input::LiveInput, Config};
use flume::Receiver;

pub(crate) fn runner(
    rx: Receiver<InputParseMessage>,
    mut config: Config,
) {
    loop {
        match rx.recv() {
            Ok(InputParseMessage::Promote(callback, input, maybe_compose)) => {
                match input.promote(config.codec_registry, config.format_registry) {
                    Ok(LiveInput::Parsed(parsed)) => {
                        let _ = callback
                            .send(MixerInputResultMessage::InputBuilt(parsed, maybe_compose));
                    },
                    Ok(_) => unreachable!(),
                    Err(e) => {
                        let _ = callback.send(MixerInputResultMessage::InputParseErr(e));
                    },
                }
            },
            Ok(InputParseMessage::Config(c)) => config = c,
            Err(_) | Ok(InputParseMessage::Poison) => break,
        }
    }
}
