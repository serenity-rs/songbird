//! Newtypes around Discord IDs for library cross-compatibility.

#[cfg(feature = "driver-core")]
use crate::model::id::{GuildId as DriverGuild, UserId as DriverUser};
#[cfg(feature = "serenity")]
use serenity::model::id::{
    ChannelId as SerenityChannel,
    GuildId as SerenityGuild,
    UserId as SerenityUser,
};
use std::fmt::{Display, Formatter, Result as FmtResult};
#[cfg(feature = "twilight")]
use twilight_model::id::{
    marker::{ChannelMarker, GuildMarker, UserMarker},
    Id as TwilightId,
};

/// ID of a Discord voice/text channel.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct ChannelId(pub u64);

/// ID of a Discord guild (colloquially, "server").
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct GuildId(pub u64);

/// ID of a Discord user.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct UserId(pub u64);

impl Display for ChannelId {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        Display::fmt(&self.0, f)
    }
}

impl From<u64> for ChannelId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

#[cfg(feature = "serenity")]
impl From<SerenityChannel> for ChannelId {
    fn from(id: SerenityChannel) -> Self {
        Self(id.0)
    }
}

#[cfg(feature = "twilight")]
impl From<TwilightId<ChannelMarker>> for ChannelId {
    fn from(id: TwilightId<ChannelMarker>) -> Self {
        Self(id.get().into())
    }
}

impl Display for GuildId {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        Display::fmt(&self.0, f)
    }
}

impl From<u64> for GuildId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

#[cfg(feature = "serenity")]
impl From<SerenityGuild> for GuildId {
    fn from(id: SerenityGuild) -> Self {
        Self(id.0)
    }
}

#[cfg(feature = "driver-core")]
impl From<GuildId> for DriverGuild {
    fn from(id: GuildId) -> Self {
        Self(id.0)
    }
}

#[cfg(feature = "twilight")]
impl From<TwilightId<GuildMarker>> for GuildId {
    fn from(id: TwilightId<GuildMarker>) -> Self {
        Self(id.get().into())
    }
}

impl Display for UserId {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        Display::fmt(&self.0, f)
    }
}

impl From<u64> for UserId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

#[cfg(feature = "serenity")]
impl From<SerenityUser> for UserId {
    fn from(id: SerenityUser) -> Self {
        Self(id.0)
    }
}

#[cfg(feature = "driver-core")]
impl From<UserId> for DriverUser {
    fn from(id: UserId) -> Self {
        Self(id.0)
    }
}

#[cfg(feature = "twilight")]
impl From<TwilightId<UserMarker>> for UserId {
    fn from(id: TwilightId<UserMarker>) -> Self {
        Self(id.get().into())
    }
}
