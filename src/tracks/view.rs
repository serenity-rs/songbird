#![allow(missing_docs)]

use super::*;
use std::time::Duration;

pub struct View<'a> {
    pub position: &'a Duration,
    pub play_time: &'a Duration,
    pub volume: &'a mut f32,
    pub meta: &'a (),
    pub playing: &'a mut PlayMode,
    pub loops: &'a mut LoopState,
}
