//! Newtypes around Discord IDs for library cross-compatibility.

#[cfg(feature = "driver")]
use crate::model::id::{GuildId as DriverGuild, UserId as DriverUser};
use nonmax::NonMaxU64;
#[cfg(feature = "serenity")]
use serenity::model::id::{
    ChannelId as SerenityChannel,
    GuildId as SerenityGuild,
    UserId as SerenityUser,
};
use std::{
    fmt::{Display, Formatter, Result as FmtResult},
    num::NonZeroU64,
};
#[cfg(feature = "twilight")]
use twilight_model::id::{
    marker::{ChannelMarker, GuildMarker, UserMarker},
    Id as TwilightId,
};

#[cfg(feature = "twilight")]
fn nonmax_from_nonzero(val: NonZeroU64) -> NonMaxU64 {
    NonMaxU64::new(val.get()).unwrap_or(NonMaxU64::ZERO)
}

macro_rules! impl_id {
    ($Id:ident, $SerenityId:path, $TwilightId:path) => {
        impl $Id {
            /// Returns the u64 representation of this Id.
            #[must_use]
            pub fn get(self) -> u64 {
                { self.0 }.get()
            }

            #[allow(unused)]
            pub(crate) fn into_nonzero(self) -> NonZeroU64 {
                NonZeroU64::new(self.get()).unwrap_or(NonZeroU64::MAX)
            }
        }

        impl Display for $Id {
            fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
                Display::fmt(&{ self.0 }, f)
            }
        }

        #[cfg(feature = "serenity")]
        impl From<$SerenityId> for $Id {
            fn from(id: $SerenityId) -> Self {
                Self(NonMaxU64::new(id.get()).unwrap())
            }
        }

        #[cfg(feature = "twilight")]
        impl From<$TwilightId> for $Id {
            fn from(id: $TwilightId) -> Self {
                // Map u64::MAX -> u64::ZERO
                Self(nonmax_from_nonzero(id.into_nonzero()))
            }
        }
    };
}

/// ID of a Discord voice/text channel.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(packed)]
pub struct ChannelId(NonMaxU64);

/// ID of a Discord guild (colloquially, "server").
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(packed)]
pub struct GuildId(NonMaxU64);

/// ID of a Discord user.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(packed)]
pub struct UserId(NonMaxU64);

impl_id! {ChannelId, SerenityChannel, TwilightId<ChannelMarker>}
impl_id! {GuildId, SerenityGuild, TwilightId<GuildMarker>}
impl_id! {UserId, SerenityUser, TwilightId<UserMarker>}

#[cfg(feature = "driver")]
impl From<GuildId> for DriverGuild {
    fn from(id: GuildId) -> Self {
        Self(id.get())
    }
}

#[cfg(feature = "driver")]
impl From<UserId> for DriverUser {
    fn from(id: UserId) -> Self {
        Self(id.get())
    }
}
