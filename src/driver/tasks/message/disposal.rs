#![allow(missing_docs)]

use crate::{driver::tasks::mixer::InternalTrack, tracks::TrackHandle};

pub enum DisposalMessage {
    Track(InternalTrack),
    Handle(TrackHandle),

    Poison,
}
