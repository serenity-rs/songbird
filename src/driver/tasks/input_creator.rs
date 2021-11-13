//! The input creator is responsible for converting `Lazy` Inputs into actual bytestreams.
//!
//! This task is an asynchronous thread, which will either spawn_blocking or run async as needed.

use super::message::{
    InputCreateMessage,
    InputParseMessage,
    Interconnect,
    MixerInputResultMessage,
};

use crate::input::{AudioStreamError, LiveInput, SymphInput};
use flume::{Receiver, Sender};

pub(crate) async fn runner(
    mut interconnect: Interconnect,
    rx: Receiver<InputCreateMessage>,
    tx: Sender<InputParseMessage>,
) {
    loop {
        match rx.recv_async().await {
            Ok(InputCreateMessage::Create(callback, input)) => match input {
                SymphInput::Lazy(mut lazy) => {
                    let (out, lazy) = if lazy.should_create_async() {
                        (lazy.create_async().await, Some(lazy))
                    } else {
                        let out = tokio::task::spawn_blocking(move || {
                            let out = lazy.create();
                            (out, Some(lazy))
                        })
                        .await;

                        match out {
                            Ok(o) => o,
                            Err(e) => (Err(AudioStreamError::Fail(Box::new(e))), None),
                        }
                    };

                    match out {
                        Ok(r) => {
                            let _ = tx
                                .send_async(InputParseMessage::Promote(
                                    callback,
                                    LiveInput::Raw(r),
                                    lazy,
                                ))
                                .await;
                        },
                        Err(e) => {
                            let _ = callback
                                .send_async(MixerInputResultMessage::InputCreateErr(e))
                                .await;
                        },
                    }
                },
                SymphInput::Live(live, maybe_create) => {
                    let _ = tx
                        .send_async(InputParseMessage::Promote(callback, live, maybe_create))
                        .await;
                },
            },
            Ok(InputCreateMessage::ReplaceInterconnect(r)) => interconnect = r,
            Err(_) | Ok(InputCreateMessage::Poison) => break,
        }
    }
}
