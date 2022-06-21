use super::util::copy_seek_to;

use crate::{
    driver::tasks::message::MixerInputResultMessage,
    input::{AudioStream, AudioStreamError, Compose, Input, LiveInput, Parsed},
    Config,
};
use flume::Sender;
use parking_lot::RwLock;
use std::{result::Result as StdResult, sync::Arc, time::Duration};
use symphonia_core::{
    formats::{SeekMode, SeekTo},
    io::MediaSource,
};
use tokio::runtime::Handle;

#[derive(Clone)]
pub struct BlockyTaskPool {
    pool: Arc<RwLock<rusty_pool::ThreadPool>>,
    handle: Handle,
}

impl BlockyTaskPool {
    pub fn new(handle: Handle) -> Self {
        Self {
            pool: Arc::new(RwLock::new(rusty_pool::ThreadPool::new(
                1,
                64,
                Duration::from_secs(300),
            ))),
            handle,
        }
    }

    pub fn create(
        &self,
        callback: Sender<MixerInputResultMessage>,
        input: Input,
        seek_time: Option<SeekTo>,
        config: Arc<Config>,
    ) {
        match input {
            Input::Lazy(mut lazy) => {
                let far_pool = self.clone();
                if lazy.should_create_async() {
                    self.handle.spawn(async move {
                        let out = lazy.create_async().await;
                        far_pool.send_to_parse(out, lazy, callback, seek_time, config);
                    });
                } else {
                    let pool = self.pool.read();
                    pool.execute(move || {
                        let out = lazy.create();
                        far_pool.send_to_parse(out, lazy, callback, seek_time, config);
                    });
                }
            },
            Input::Live(live, maybe_create) =>
                self.parse(config, callback, live, maybe_create, seek_time),
        }
    }

    pub fn send_to_parse(
        &self,
        create_res: StdResult<AudioStream<Box<dyn MediaSource>>, AudioStreamError>,
        rec: Box<dyn Compose>,
        callback: Sender<MixerInputResultMessage>,
        seek_time: Option<SeekTo>,
        config: Arc<Config>,
    ) {
        match create_res {
            Ok(o) => {
                self.parse(config, callback, LiveInput::Raw(o), Some(rec), seek_time);
            },
            Err(e) => {
                drop(callback.send(MixerInputResultMessage::CreateErr(e)));
            },
        }
    }

    pub fn parse(
        &self,
        config: Arc<Config>,
        callback: Sender<MixerInputResultMessage>,
        input: LiveInput,
        rec: Option<Box<dyn Compose>>,
        seek_time: Option<SeekTo>,
    ) {
        let pool_clone = self.clone();
        let pool = self.pool.read();

        pool.execute(
            move || match input.promote(config.codec_registry, config.format_registry) {
                Ok(LiveInput::Parsed(parsed)) =>
                    if let Some(seek_time) = seek_time {
                        pool_clone.seek(callback, parsed, rec, seek_time, false, config);
                    } else {
                        drop(callback.send(MixerInputResultMessage::Built(parsed, rec)));
                    },
                Ok(_) => unreachable!(),
                Err(e) => {
                    drop(callback.send(MixerInputResultMessage::ParseErr(e)));
                },
            },
        );
    }

    pub fn seek(
        &self,
        callback: Sender<MixerInputResultMessage>,
        mut input: Parsed,
        rec: Option<Box<dyn Compose>>,
        seek_time: SeekTo,
        // Not all of symphonia's formats bother to return SeekErrorKind::ForwardOnly.
        // So, we need *this* flag.
        backseek_needed: bool,
        config: Arc<Config>,
    ) {
        let pool_clone = self.clone();
        let pool = self.pool.read();

        pool.execute(move || match rec {
            Some(rec) if (!input.supports_backseek) && backseek_needed => {
                pool_clone.create(callback, Input::Lazy(rec), Some(seek_time), config);
            },
            _ => {
                let seek_result = input
                    .format
                    .seek(SeekMode::Accurate, copy_seek_to(&seek_time));
                input.decoder.reset();
                drop(callback.send(MixerInputResultMessage::Seek(input, rec, seek_result)));
            },
        });
    }
}
