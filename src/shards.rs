//! Handlers for sending packets over sharded connections.

use crate::{error::JoinResult, id::*};
use async_trait::async_trait;
use derivative::Derivative;
#[cfg(feature = "serenity")]
use futures::channel::mpsc::{TrySendError, UnboundedSender as Sender};
#[cfg(feature = "serenity")]
use parking_lot::{lock_api::RwLockWriteGuard, Mutex as PMutex, RwLock as PRwLock};
use serde_json::json;
#[cfg(feature = "serenity")]
use serenity::gateway::InterMessage;
use std::sync::Arc;
#[cfg(feature = "serenity")]
use std::{collections::HashMap, result::Result as StdResult};
use tracing::{debug, error};
#[cfg(feature = "twilight")]
use twilight_gateway::{Cluster, Shard as TwilightShard};
#[cfg(feature = "twilight")]
use twilight_model::gateway::payload::outgoing::update_voice_state::UpdateVoiceState as TwilightVoiceState;

#[derive(Derivative)]
#[derivative(Debug)]
#[non_exhaustive]
/// Source of individual shard connection handles.
pub enum Sharder {
    #[cfg(feature = "serenity")]
    /// Serenity-specific wrapper for sharder state initialised by the library.
    Serenity(SerenitySharder),
    #[cfg(feature = "twilight")]
    /// Twilight-specific wrapper for sharder state initialised by the user.
    TwilightCluster(Arc<Cluster>),
    #[cfg(feature = "twilight")]
    /// Twilight-specific wrapper for a single shard initialised by the user.
    TwilightShard(Arc<TwilightShard>),
    /// A generic shard handle source.
    Generic(#[derivative(Debug = "ignore")] Arc<dyn GenericSharder + Send + Sync>),
}

/// Trait for a generic shard cluster or other handle source.
///
/// This allows any Discord library to be integrated with Songbird, and offers a source
/// of generic shard handles.
#[async_trait]
pub trait GenericSharder {
    /// Get access to a new shard
    fn get_shard(&self, shard_id: u64) -> Option<Arc<dyn VoiceUpdate + Send + Sync>>;
}

impl Sharder {
    /// Returns a new handle to the required inner shard.
    pub fn get_shard(&self, shard_id: u64) -> Option<Shard> {
        match self {
            #[cfg(feature = "serenity")]
            Sharder::Serenity(s) => Some(Shard::Serenity(s.get_or_insert_shard_handle(shard_id))),
            #[cfg(feature = "twilight")]
            Sharder::TwilightCluster(t) => Some(Shard::TwilightCluster(t.clone(), shard_id)),
            #[cfg(feature = "twilight")]
            Sharder::TwilightShard(t) => Some(Shard::TwilightShard(t.clone())),
            Sharder::Generic(src) => src.get_shard(shard_id).map(Shard::Generic),
        }
    }
}

#[cfg(feature = "serenity")]
impl Sharder {
    #[allow(unreachable_patterns)]
    pub(crate) fn register_shard_handle(&self, shard_id: u64, sender: Sender<InterMessage>) {
        if let Sharder::Serenity(s) = self {
            s.register_shard_handle(shard_id, sender);
        } else {
            error!("Called serenity management function on a non-serenity Songbird instance.");
        }
    }

    #[allow(unreachable_patterns)]
    pub(crate) fn deregister_shard_handle(&self, shard_id: u64) {
        if let Sharder::Serenity(s) = self {
            s.deregister_shard_handle(shard_id);
        } else {
            error!("Called serenity management function on a non-serenity Songbird instance.");
        }
    }
}

#[cfg(feature = "serenity")]
#[derive(Debug, Default)]
/// Serenity-specific wrapper for sharder state initialised by the library.
///
/// This is updated and maintained by the library, and is designed to prevent
/// message loss during rebalances and reconnects.
pub struct SerenitySharder(PRwLock<HashMap<u64, Arc<SerenityShardHandle>>>);

#[cfg(feature = "serenity")]
impl SerenitySharder {
    fn get_or_insert_shard_handle(&self, shard_id: u64) -> Arc<SerenityShardHandle> {
        ({
            let map_read = self.0.read();
            map_read.get(&shard_id).cloned()
        })
        .unwrap_or_else(|| {
            let mut map_read = self.0.write();
            map_read.entry(shard_id).or_default().clone()
        })
    }

    fn register_shard_handle(&self, shard_id: u64, sender: Sender<InterMessage>) {
        // Write locks are only used to add new entries to the map.
        let handle = self.get_or_insert_shard_handle(shard_id);

        handle.register(sender);
    }

    fn deregister_shard_handle(&self, shard_id: u64) {
        // Write locks are only used to add new entries to the map.
        let handle = self.get_or_insert_shard_handle(shard_id);

        handle.deregister();
    }
}

#[derive(Derivative, Clone)]
#[derivative(Debug)]
#[non_exhaustive]
/// A reference to an individual websocket connection.
pub enum Shard {
    #[cfg(feature = "serenity")]
    /// Handle to one of serenity's shard runners.
    Serenity(Arc<SerenityShardHandle>),
    #[cfg(feature = "twilight")]
    /// Handle to a twilight shard spawned from a cluster.
    TwilightCluster(Arc<Cluster>, u64),
    #[cfg(feature = "twilight")]
    /// Handle to a twilight shard spawned from a cluster.
    TwilightShard(Arc<TwilightShard>),
    /// Handle to a generic shard instance.
    Generic(#[derivative(Debug = "ignore")] Arc<dyn VoiceUpdate + Send + Sync>),
}

#[async_trait]
impl VoiceUpdate for Shard {
    async fn update_voice_state(
        &self,
        guild_id: GuildId,
        channel_id: Option<ChannelId>,
        self_deaf: bool,
        self_mute: bool,
    ) -> JoinResult<()> {
        match self {
            #[cfg(feature = "serenity")]
            Shard::Serenity(handle) => {
                let map = json!({
                    "op": 4,
                    "d": {
                        "channel_id": channel_id.map(|c| c.0),
                        "guild_id": guild_id.0,
                        "self_deaf": self_deaf,
                        "self_mute": self_mute,
                    }
                });

                handle.send(InterMessage::Json(map))?;
                Ok(())
            },
            #[cfg(feature = "twilight")]
            Shard::TwilightCluster(handle, shard_id) => {
                let channel_id = channel_id.map(|c| c.0).map(From::from);
                let cmd = TwilightVoiceState::new(guild_id.0, channel_id, self_deaf, self_mute);
                handle.command(*shard_id, &cmd).await?;
                Ok(())
            },
            #[cfg(feature = "twilight")]
            Shard::TwilightShard(handle) => {
                let channel_id = channel_id.map(|c| c.0).map(From::from);
                let cmd = TwilightVoiceState::new(guild_id.0, channel_id, self_deaf, self_mute);
                handle.command(&cmd).await?;
                Ok(())
            },
            Shard::Generic(g) =>
                g.update_voice_state(guild_id, channel_id, self_deaf, self_mute)
                    .await,
        }
    }
}

/// Trait for a generic shard handle to send voice state updates to Discord.
///
/// This allows any Discord library to be integrated with Songbird, and is intended to
/// wrap a message channel to a single shard. Songbird only needs to send `VoiceStateUpdate`s
/// to Discord to function.
///
/// Generic libraries must be sure to call [`Call::update_server`] and [`Call::update_state`]
/// in response to their own received messages.
///
/// [`Call::update_server`]: crate::Call::update_server
/// [`Call::update_state`]: crate::Call::update_state
#[async_trait]
pub trait VoiceUpdate {
    /// Send a voice update message to the inner shard handle.
    async fn update_voice_state(
        &self,
        guild_id: GuildId,
        channel_id: Option<ChannelId>,
        self_deaf: bool,
        self_mute: bool,
    ) -> JoinResult<()>;
}

#[cfg(feature = "serenity")]
/// Handle to an individual shard designed to buffer unsent messages while
/// a reconnect/rebalance is ongoing.
#[derive(Debug, Default)]
pub struct SerenityShardHandle {
    sender: PRwLock<Option<Sender<InterMessage>>>,
    queue: PMutex<Vec<InterMessage>>,
}

#[cfg(feature = "serenity")]
impl SerenityShardHandle {
    fn register(&self, sender: Sender<InterMessage>) {
        debug!("Adding shard handle send channel...");

        let mut sender_lock = self.sender.write();
        *sender_lock = Some(sender);

        debug!("Added shard handle send channel.");

        let sender_lock = RwLockWriteGuard::downgrade(sender_lock);
        let mut messages_lock = self.queue.lock();

        debug!("Clearing queued messages...");

        if let Some(sender) = &*sender_lock {
            let mut i = 0;
            for msg in messages_lock.drain(..) {
                if let Err(e) = sender.unbounded_send(msg) {
                    error!("Error while clearing gateway message queue: {:?}", e);
                    break;
                }

                i += 1;
            }

            if i > 0 {
                debug!("{} buffered messages sent to Serenity.", i);
            }
        }

        debug!("Cleared queued messages.");
    }

    fn deregister(&self) {
        debug!("Removing shard handle send channel...");

        let mut sender_lock = self.sender.write();
        *sender_lock = None;

        debug!("Removed shard handle send channel.");
    }

    fn send(&self, message: InterMessage) -> StdResult<(), TrySendError<InterMessage>> {
        let sender_lock = self.sender.read();
        if let Some(sender) = &*sender_lock {
            sender.unbounded_send(message)
        } else {
            debug!("Serenity shard temporarily disconnected: buffering message...");
            let mut messages_lock = self.queue.lock();
            messages_lock.push(message);
            debug!("Buffered message.");
            Ok(())
        }
    }
}
