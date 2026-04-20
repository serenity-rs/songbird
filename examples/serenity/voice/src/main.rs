//! Requires the "client", "standard_framework", and "voice" features be enabled in your
//! Cargo.toml, like so:
//!
//! ```toml
//! [dependencies.serenity]
//! git = "https://github.com/serenity-rs/serenity.git"
//! features = ["client", "standard_framework", "voice"]
//! ```
use std::env;
use std::sync::Arc;

use serenity::all as serenity;

// Event related imports to detect track creation failures.
use songbird::events::{Event, EventContext, EventHandler as VoiceEventHandler, TrackEvent};

// To turn user URLs into playable audio, we'll use yt-dlp.
use songbird::input::YoutubeDl;

// YtDl requests need an HTTP client to operate -- we'll create and store our own.
use reqwest::Client as HttpClient;

struct UserData {
    http: HttpClient,
    songbird: Arc<songbird::Songbird>,
}

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, UserData, Error>;
type CommandResult = Result<(), Error>;

struct Handler;

#[serenity::async_trait]
impl serenity::EventHandler for Handler {
    async fn ready(&self, _: serenity::Context, ready: serenity::Ready) {
        println!("{} is connected!", ready.user.name);
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    // Create our songbird voice manager
    let manager = songbird::Songbird::serenity();

    // Configure our command framework
    let options = poise::FrameworkOptions {
        commands: vec![deafen(), undeafen(), join(), leave(), play()],
        prefix_options: poise::PrefixFrameworkOptions {
            prefix: Some(String::from("~")),
            ..Default::default()
        },
        ..Default::default()
    };

    // We have to clone our voice manager's Arc to share it between serenity and our user data.
    let manager_clone = Arc::clone(&manager);
    let framework = poise::Framework::new(options, |_, _, _| {
        Box::pin(async {
            Ok(
                // We create a global HTTP client here to make use of in
                // `~play`. If we wanted, we could supply cookies and auth
                // details ahead of time.
                UserData {
                    http: HttpClient::new(),
                    songbird: manager_clone,
                },
            )
        })
    });

    let intents =
        serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT;
    let mut client = serenity::Client::builder(&token, intents)
        .voice_manager_arc(manager)
        .event_handler(Handler)
        .framework(framework)
        .await
        .expect("Err creating client");

    tokio::spawn(async move {
        let _ = client
            .start()
            .await
            .map_err(|why| println!("Client ended: {:?}", why));
    });

    let _signal_err = tokio::signal::ctrl_c().await;
    println!("Received Ctrl-C, shutting down.");
}

#[poise::command(prefix_command, guild_only)]
async fn deafen(ctx: Context<'_>) -> CommandResult {
    let guild_id = ctx.guild_id().unwrap();
    let manager = &ctx.data().songbird;

    let handler_lock = match manager.get(guild_id) {
        Some(handler) => handler,
        None => {
            check_msg(ctx.reply("Not in a voice channel").await);

            return Ok(());
        },
    };

    let mut handler = handler_lock.lock().await;

    if handler.is_deaf() {
        check_msg(ctx.say("Already deafened").await);
    } else {
        if let Err(e) = handler.deafen(true).await {
            check_msg(ctx.say(format!("Failed: {:?}", e)).await);
        }

        check_msg(ctx.say("Deafened").await);
    }

    Ok(())
}

#[poise::command(prefix_command, guild_only)]
async fn join(ctx: Context<'_>) -> CommandResult {
    let (guild_id, channel_id) = {
        let guild = ctx.guild().unwrap();
        let channel_id = guild
            .voice_states
            .get(&ctx.author().id)
            .and_then(|voice_state| voice_state.channel_id);

        (guild.id, channel_id)
    };

    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            check_msg(ctx.reply("Not in a voice channel").await);
            return Ok(());
        },
    };

    let manager = &ctx.data().songbird;
    if let Ok(handler_lock) = manager.join(guild_id, connect_to).await {
        // Attach an event handler to see notifications of all track errors.
        let mut handler = handler_lock.lock().await;
        handler.add_global_event(TrackEvent::Error.into(), TrackErrorNotifier);
    }

    Ok(())
}

struct TrackErrorNotifier;

#[serenity::async_trait]
impl VoiceEventHandler for TrackErrorNotifier {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        if let EventContext::Track(track_list) = ctx {
            for (state, handle) in *track_list {
                println!(
                    "Track {:?} encountered an error: {:?}",
                    handle.uuid(),
                    state.playing
                );
            }
        }

        None
    }
}

#[poise::command(prefix_command, guild_only)]
async fn leave(ctx: Context<'_>) -> CommandResult {
    let guild_id = ctx.guild_id().unwrap();

    let manager = &ctx.data().songbird;
    let has_handler = manager.get(guild_id).is_some();

    if has_handler {
        if let Err(e) = manager.remove(guild_id).await {
            check_msg(ctx.say(format!("Failed: {:?}", e)).await);
        }

        check_msg(ctx.say("Left voice channel").await);
    } else {
        check_msg(ctx.reply("Not in a voice channel").await);
    }

    Ok(())
}

#[poise::command(prefix_command, guild_only)]
async fn mute(ctx: Context<'_>) -> CommandResult {
    let guild_id = ctx.guild_id().unwrap();
    let manager = &ctx.data().songbird;

    let handler_lock = match manager.get(guild_id) {
        Some(handler) => handler,
        None => {
            check_msg(ctx.reply("Not in a voice channel").await);

            return Ok(());
        },
    };

    let mut handler = handler_lock.lock().await;

    if handler.is_mute() {
        check_msg(ctx.say("Already muted").await);
    } else {
        if let Err(e) = handler.mute(true).await {
            check_msg(ctx.say(format!("Failed: {:?}", e)).await);
        }

        check_msg(ctx.say("Now muted").await);
    }

    Ok(())
}

#[poise::command(prefix_command, guild_only)]
async fn ping(ctx: Context<'_>) -> CommandResult {
    check_msg(ctx.say("Pong!").await);
    Ok(())
}

#[poise::command(prefix_command, guild_only)]
async fn play(ctx: Context<'_>, url: String) -> CommandResult {
    let do_search = !url.starts_with("http");

    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();

    if let Some(handler_lock) = data.songbird.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        let src = if do_search {
            YoutubeDl::new_search(data.http.clone(), url)
        } else {
            YoutubeDl::new(data.http.clone(), url)
        };
        let _ = handler.play_input(src.into());

        check_msg(ctx.say("Playing song").await);
    } else {
        check_msg(ctx.say("Not in a voice channel to play in").await);
    }

    Ok(())
}

#[poise::command(prefix_command, guild_only)]
async fn undeafen(ctx: Context<'_>) -> CommandResult {
    let guild_id = ctx.guild_id().unwrap();
    let manager = &ctx.data().songbird;

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;
        if let Err(e) = handler.deafen(false).await {
            check_msg(ctx.say(format!("Failed: {:?}", e)).await);
        }

        check_msg(ctx.say("Undeafened").await);
    } else {
        check_msg(ctx.say("Not in a voice channel to undeafen in").await);
    }

    Ok(())
}

#[poise::command(prefix_command, guild_only)]
async fn unmute(ctx: Context<'_>) -> CommandResult {
    let guild_id = ctx.guild_id().unwrap();
    let manager = &ctx.data().songbird;

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;
        if let Err(e) = handler.mute(false).await {
            check_msg(ctx.say(format!("Failed: {:?}", e)).await);
        }

        check_msg(ctx.say("Unmuted").await);
    } else {
        check_msg(ctx.say("Not in a voice channel to unmute in").await);
    }

    Ok(())
}

/// Checks that a message successfully sent; if not, then logs why to stdout.
fn check_msg<T>(result: serenity::Result<T>) {
    if let Err(why) = result {
        println!("Error sending message: {:?}", why);
    }
}
