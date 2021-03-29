#![doc(
    html_logo_url = "https://raw.githubusercontent.com/serenity-rs/songbird/current/songbird.png",
    html_favicon_url = "https://raw.githubusercontent.com/serenity-rs/songbird/current/songbird-ico.png"
)]
#![deny(missing_docs)]
#![deny(broken_intra_doc_links)]
//! ![project logo][logo]
//!
//! Songbird is an async, cross-library compatible voice system for Discord, written in Rust.
//! The library offers:
//!  * A standalone gateway frontend compatible with [serenity] and [twilight] using the
//!  `"gateway"` and `"[serenity/twilight]-[rustls/native]"` features. You can even run
//!  driverless, to help manage your [lavalink] sessions.
//!  * A standalone driver for voice calls, via the `"driver"` feature. If you can create
//!  a [`ConnectionInfo`] using any other gateway, or language for your bot, then you
//!  can run the songbird voice driver.
//!  * And, by default, a fully featured voice system featuring events, queues, RT(C)P packet
//!  handling, seeking on compatible streams, shared multithreaded audio stream caches,
//!  and direct Opus data passthrough from DCA files.
//!
//! ## Intents
//! Songbird's gateway functionality requires you to specify the `GUILD_VOICE_STATES` intent.
//!
//! ## Examples
//! Full examples showing various types of functionality and integrations can be found
//! in [this crate's examples directory].
//!
//! ## Attribution
//!
//! Songbird's logo is based upon the copyright-free image ["Black-Capped Chickadee"] by George Gorgas White.
//!
//! [logo]: https://raw.githubusercontent.com/serenity-rs/songbird/current/songbird.png
//! [serenity]: https://github.com/serenity-rs/serenity
//! [twilight]: https://github.com/twilight-rs/twilight
//! [this crate's examples directory]: https://github.com/serenity-rs/songbird/tree/current/examples
//! ["Black-Capped Chickadee"]: https://www.oldbookillustrations.com/illustrations/black-capped-chickadee/
//! [`ConnectionInfo`]: struct@ConnectionInfo
//! [lavalink]: https://github.com/Frederikam/Lavalink

mod config;
pub mod constants;
#[cfg(feature = "driver-core")]
pub mod driver;
pub mod error;
#[cfg(feature = "driver-core")]
pub mod events;
#[cfg(feature = "gateway-core")]
mod handler;
pub mod id;
pub(crate) mod info;
#[cfg(feature = "driver-core")]
pub mod input;
#[cfg(feature = "gateway-core")]
pub mod join;
#[cfg(feature = "gateway-core")]
mod manager;
#[cfg(feature = "serenity")]
pub mod serenity;
#[cfg(feature = "gateway-core")]
pub mod shards;
#[cfg(feature = "driver-core")]
pub mod tracks;
#[cfg(feature = "driver-core")]
mod ws;

#[cfg(feature = "driver-core")]
/// Opus encoder bitrate settings.
pub use audiopus::{self as opus, Bitrate};
#[cfg(feature = "driver-core")]
pub use discortp as packet;
#[cfg(feature = "driver-core")]
pub use serenity_voice_model as model;
#[cfg(feature = "driver-core")]
pub use typemap_rev as typemap;

#[cfg(test)]
use utils as test_utils;

#[cfg(feature = "driver-core")]
pub use crate::{
    driver::Driver,
    events::{CoreEvent, Event, EventContext, EventHandler, TrackEvent},
    input::{ffmpeg, ytdl},
    tracks::create_player,
};

#[cfg(feature = "gateway-core")]
pub use crate::{handler::*, manager::*};

#[cfg(feature = "serenity")]
pub use crate::serenity::*;

pub use config::Config;
pub use info::ConnectionInfo;
