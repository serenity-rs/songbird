#![allow(missing_docs)]

use crate::{
    driver::tasks::mixer::{InputState, LocalInput},
    tracks::Track,
};

pub enum DisposalMessage {
    Track(Track),
    Local(LocalInput),
    State(InputState),

    Poison,
}
