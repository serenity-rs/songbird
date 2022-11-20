//! This example adapts Twilight's [basic lavalink bot] to use Songbird as its voice driver.
//!
//! # Twilight-rs attribution
//! ISC License (ISC)
//!
//! Copyright (c) 2019, 2020 (c) The Twilight Contributors
//!
//! Permission to use, copy, modify, and/or distribute this software for any purpose
//! with or without fee is hereby granted, provided that the above copyright notice
//! and this permission notice appear in all copies.
//!
//! THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
//! REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
//! FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
//! INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
//! OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
//! TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
//! THIS SOFTWARE.
//!
//!
//! [basic lavalink bot]: https://github.com/twilight-rs/twilight/tree/main/examples/lavalink-basic-bot.rs

use futures::StreamExt;
use songbird::{
    input::{Compose, YoutubeDl},
    tracks::{PlayMode, TrackHandle},
    Songbird,
};
use std::{collections::HashMap, env, error::Error, future::Future, num::NonZeroU64, sync::Arc};
use tokio::sync::RwLock;
use twilight_gateway::{Cluster, Event, Intents};
use twilight_http::Client as HttpClient;
use twilight_model::{
    channel::Message,
    gateway::payload::incoming::MessageCreate,
    id::{marker::GuildMarker, Id},
};
use twilight_standby::Standby;

type State = Arc<StateRef>;

#[derive(Debug)]
struct StateRef {
    http: HttpClient,
    trackdata: RwLock<HashMap<Id<GuildMarker>, TrackHandle>>,
    songbird: Songbird,
    standby: Standby,
}

fn spawn(
    fut: impl Future<Output = Result<(), Box<dyn Error + Send + Sync + 'static>>> + Send + 'static,
) {
    tokio::spawn(async move {
        if let Err(why) = fut.await {
            tracing::debug!("handler error: {:?}", why);
        }
    });
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    // Initialize the tracing subscriber.
    tracing_subscriber::fmt::init();

    let (mut events, state) = {
        let token = env::var("DISCORD_TOKEN")?;

        let http = HttpClient::new(token.clone());
        let user_id = http.current_user().await?.model().await?.id;

        let intents =
            Intents::GUILD_MESSAGES | Intents::MESSAGE_CONTENT | Intents::GUILD_VOICE_STATES;
        let (cluster, events) = Cluster::new(token, intents).await?;
        cluster.up().await;

        let songbird = Songbird::twilight(Arc::new(cluster), user_id);

        (
            events,
            Arc::new(StateRef {
                http,
                trackdata: Default::default(),
                songbird,
                standby: Standby::new(),
            }),
        )
    };

    while let Some((_, event)) = events.next().await {
        state.standby.process(&event);
        state.songbird.process(&event).await;

        if let Event::MessageCreate(msg) = event {
            if msg.guild_id.is_none() || !msg.content.starts_with('!') {
                continue;
            }

            match msg.content.splitn(2, ' ').next() {
                Some("!join") => spawn(join(msg.0, Arc::clone(&state))),
                Some("!leave") => spawn(leave(msg.0, Arc::clone(&state))),
                Some("!pause") => spawn(pause(msg.0, Arc::clone(&state))),
                Some("!play") => spawn(play(msg.0, Arc::clone(&state))),
                Some("!seek") => spawn(seek(msg.0, Arc::clone(&state))),
                Some("!stop") => spawn(stop(msg.0, Arc::clone(&state))),
                Some("!volume") => spawn(volume(msg.0, Arc::clone(&state))),
                _ => continue,
            }
        }
    }

    Ok(())
}

async fn join(msg: Message, state: State) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    state
        .http
        .create_message(msg.channel_id)
        .content("What's the channel ID you want me to join?")?
        .await?;

    let author_id = msg.author.id;
    let msg = state
        .standby
        .wait_for_message(msg.channel_id, move |new_msg: &MessageCreate| {
            new_msg.author.id == author_id
        })
        .await?;
    let channel_id = msg.content.parse::<u64>()?;

    let guild_id = msg.guild_id.ok_or("Can't join a non-guild channel.")?;
    let channel_id =
        NonZeroU64::new(channel_id).ok_or("Joined voice channel must have nonzero ID.")?;

    let (_handle, success) = state.songbird.join(guild_id, channel_id).await;

    let content = match success {
        Ok(()) => format!("Joined <#{}>!", channel_id),
        Err(e) => format!("Failed to join <#{}>! Why: {:?}", channel_id, e),
    };

    state
        .http
        .create_message(msg.channel_id)
        .content(&content)?
        .await?;

    Ok(())
}

async fn leave(msg: Message, state: State) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    tracing::debug!(
        "leave command in channel {} by {}",
        msg.channel_id,
        msg.author.name
    );

    let guild_id = msg.guild_id.unwrap();

    state.songbird.leave(guild_id).await?;

    state
        .http
        .create_message(msg.channel_id)
        .content("Left the channel")?
        .await?;

    Ok(())
}

async fn play(msg: Message, state: State) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    tracing::debug!(
        "play command in channel {} by {}",
        msg.channel_id,
        msg.author.name
    );
    state
        .http
        .create_message(msg.channel_id)
        .content("What's the URL of the audio to play?")?
        .await?;

    let author_id = msg.author.id;
    let msg = state
        .standby
        .wait_for_message(msg.channel_id, move |new_msg: &MessageCreate| {
            new_msg.author.id == author_id
        })
        .await?;

    let guild_id = msg.guild_id.unwrap();

    let mut src = YoutubeDl::new(reqwest::Client::new(), msg.content.clone());
    if let Ok(metadata) = src.aux_metadata().await {
        let content = format!(
            "Playing **{:?}** by **{:?}**",
            metadata.track.as_ref().unwrap_or(&"<UNKNOWN>".to_string()),
            metadata.artist.as_ref().unwrap_or(&"<UNKNOWN>".to_string()),
        );

        state
            .http
            .create_message(msg.channel_id)
            .content(&content)?
            .await?;

        if let Some(call_lock) = state.songbird.get(guild_id) {
            let mut call = call_lock.lock().await;
            let handle = call.play_input(src.into());

            let mut store = state.trackdata.write().await;
            store.insert(guild_id, handle);
        }
    } else {
        state
            .http
            .create_message(msg.channel_id)
            .content("Didn't find any results")?
            .await?;
    }

    Ok(())
}

async fn pause(msg: Message, state: State) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    tracing::debug!(
        "pause command in channel {} by {}",
        msg.channel_id,
        msg.author.name
    );

    let guild_id = msg.guild_id.unwrap();

    let store = state.trackdata.read().await;

    let content = if let Some(handle) = store.get(&guild_id) {
        let info = handle.get_info().await?;

        let paused = match info.playing {
            PlayMode::Play => {
                let _success = handle.pause();
                false
            },
            _ => {
                let _success = handle.play();
                true
            },
        };

        let action = if paused { "Unpaused" } else { "Paused" };

        format!("{} the track", action)
    } else {
        format!("No track to (un)pause!")
    };

    state
        .http
        .create_message(msg.channel_id)
        .content(&content)?
        .await?;

    Ok(())
}

async fn seek(msg: Message, state: State) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    tracing::debug!(
        "seek command in channel {} by {}",
        msg.channel_id,
        msg.author.name
    );
    state
        .http
        .create_message(msg.channel_id)
        .content("Where in the track do you want to seek to (in seconds)?")?
        .await?;

    let author_id = msg.author.id;
    let msg = state
        .standby
        .wait_for_message(msg.channel_id, move |new_msg: &MessageCreate| {
            new_msg.author.id == author_id
        })
        .await?;
    let guild_id = msg.guild_id.unwrap();
    let position = msg.content.parse::<u64>()?;

    let store = state.trackdata.read().await;

    let content = if let Some(handle) = store.get(&guild_id) {
        let _success = handle.seek(std::time::Duration::from_secs(position));
        format!("Seeking to {}s", position)
    } else {
        format!("No track to seek over!")
    };

    state
        .http
        .create_message(msg.channel_id)
        .content(&content)?
        .await?;

    Ok(())
}

async fn stop(msg: Message, state: State) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    tracing::debug!(
        "stop command in channel {} by {}",
        msg.channel_id,
        msg.author.name
    );

    let guild_id = msg.guild_id.unwrap();

    if let Some(call_lock) = state.songbird.get(guild_id) {
        let mut call = call_lock.lock().await;
        let _ = call.stop();
    }

    state
        .http
        .create_message(msg.channel_id)
        .content("Stopped the track")?
        .await?;

    Ok(())
}

async fn volume(msg: Message, state: State) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    tracing::debug!(
        "volume command in channel {} by {}",
        msg.channel_id,
        msg.author.name
    );
    state
        .http
        .create_message(msg.channel_id)
        .content("What's the volume you want to set (0.0-10.0, 1.0 being the default)?")?
        .await?;

    let author_id = msg.author.id;
    let msg = state
        .standby
        .wait_for_message(msg.channel_id, move |new_msg: &MessageCreate| {
            new_msg.author.id == author_id
        })
        .await?;
    let guild_id = msg.guild_id.unwrap();
    let volume = msg.content.parse::<f64>()?;

    if !volume.is_finite() || volume > 10.0 || volume < 0.0 {
        state
            .http
            .create_message(msg.channel_id)
            .content("Invalid volume!")?
            .await?;

        return Ok(());
    }

    let store = state.trackdata.read().await;

    let content = if let Some(handle) = store.get(&guild_id) {
        let _success = handle.set_volume(volume as f32);
        format!("Set the volume to {}", volume)
    } else {
        format!("No track to change volume!")
    };

    state
        .http
        .create_message(msg.channel_id)
        .content(&content)?
        .await?;

    Ok(())
}
