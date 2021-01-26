#![allow(missing_docs)]

use crate::tracks::Track;

pub enum DisposalMessage {
    Track(Track),

    Poison,
}
