#![allow(missing_docs)]

use super::*;
use crate::input::Metadata;
use std::time::Duration;

pub struct View<'a> {
    pub position: &'a Duration,
    pub play_time: &'a Duration,
    pub volume: &'a mut f32,
    pub meta: Option<Metadata<'a>>,
    pub playing: &'a mut PlayMode,
    pub ready: ReadyState,
    pub loops: &'a mut LoopState,
}
