//! Types containing the main body of an [`EventContext`].
//!
//! [`EventContext`]: super::EventContext
mod connect;
mod rtcp;
mod speaking;
mod voice;

use discortp::{rtcp::Rtcp, rtp::Rtp};

pub use self::{connect::*, rtcp::*, speaking::*, voice::*};
