//! Example demonstrating how to store and convert audio streams which you
//! either want to reuse between servers, or to seek/loop on. See `join`, and `ting`.
//!
//! Requires the "cache", "standard_framework", and "voice" features be enabled in your
//! Cargo.toml, like so:
//!
//! ```toml
//! [dependencies.serenity]
//! git = "https://github.com/serenity-rs/serenity.git"
//! features = ["cache", "framework", "standard_framework", "voice"]
//! ```
use std::{
    env,
    sync::{Arc, Weak},
};

use serenity::{
    async_trait,
    client::{Client, Context, EventHandler},
    framework::{
        standard::{
            macros::{command, group},
            Args,
            CommandResult,
            Configuration,
        },
        StandardFramework,
    },
    model::{channel::Message, gateway::Ready},
    prelude::{GatewayIntents, Mentionable, Mutex},
    Result as SerenityResult,
};

use songbird::{
    driver::Bitrate,
    input::{
        cached::{Compressed, Memory},
        File,
        Input,
    },
    Call,
    Event,
    EventContext,
    EventHandler as VoiceEventHandler,
    TrackEvent,
};

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
}

enum CachedSound {
    Compressed(Compressed),
    Uncompressed(Memory),
}

impl From<&CachedSound> for Input {
    fn from(obj: &CachedSound) -> Self {
        use CachedSound::*;
        match obj {
            Compressed(c) => c.new_handle().into(),
            Uncompressed(u) => u.new_handle().into(),
        }
    }
}

struct UserData {
    songbird: Arc<songbird::Songbird>,
    sound_store: dashmap::DashMap<String, CachedSound>,
}

#[group]
#[commands(deafen, join, leave, mute, ting, undeafen, unmute)]
struct General;

async fn setup_cached_audio() -> dashmap::DashMap<String, CachedSound> {
    // Loading the audio ahead of time.
    let audio_map = dashmap::DashMap::new();

    // Creation of an in-memory source.
    //
    // This is a small sound effect, so storing the whole thing is relatively cheap.
    //
    // `spawn_loader` creates a new thread which works to copy all the audio into memory
    // ahead of time. We do this in both cases to ensure optimal performance for the audio
    // core.
    let ting_src = Memory::new(File::new("../../../resources/ting.wav").into())
        .await
        .expect("These parameters are well-defined.");
    let _ = ting_src.raw.spawn_loader();
    audio_map.insert("ting".into(), CachedSound::Uncompressed(ting_src));

    // Another short sting, to show where each loop occurs.
    let loop_src = Memory::new(File::new("../../../resources/loop.wav").into())
        .await
        .expect("These parameters are well-defined.");
    let _ = loop_src.raw.spawn_loader();
    audio_map.insert("loop".into(), CachedSound::Uncompressed(loop_src));

    // Creation of a compressed source.
    //
    // This is a full song, making this a much less memory-heavy choice.
    //
    // Music by Cloudkicker, used under CC BY-SC-SA 3.0 (https://creativecommons.org/licenses/by-nc-sa/3.0/).
    let song_src = Compressed::new(
        File::new("../../../resources/Cloudkicker - 2011 07.mp3").into(),
        Bitrate::BitsPerSecond(128_000),
    )
    .await
    .expect("These parameters are well-defined.");
    let _ = song_src.raw.spawn_loader();

    // Compressed sources are internally stored as DCA1 format files.
    // Because `Compressed` implements `std::io::Read`, we can save these
    // to disk and use them again later if we want!
    let mut creator = song_src.new_handle();
    std::thread::spawn(move || {
        let mut out_file = std::fs::File::create("ckick-dca1.dca").unwrap();
        std::io::copy(&mut creator, &mut out_file).expect("Error writing out song!");
    });

    audio_map.insert("song".into(), CachedSound::Compressed(song_src));
    audio_map
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    let framework = StandardFramework::new().group(&GENERAL_GROUP);
    framework.configure(Configuration::new().prefix("~"));

    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;

    let manager = songbird::Songbird::serenity();
    let user_data = UserData {
        sound_store: setup_cached_audio().await,
        songbird: Arc::clone(&manager),
    };

    let mut client = Client::builder(&token, intents)
        .voice_manager::<songbird::Songbird>(manager)
        .data(Arc::new(user_data) as _)
        .event_handler(Handler)
        .framework(framework)
        .await
        .expect("Err creating client");

    let _ = client
        .start()
        .await
        .map_err(|why| println!("Client ended: {:?}", why));
}

#[command]
#[only_in(guilds)]
async fn deafen(ctx: &Context, msg: &Message) -> CommandResult {
    let guild_id = msg.guild_id.unwrap();
    let manager = &ctx.data::<UserData>().songbird;

    let handler_lock = match manager.get(guild_id) {
        Some(handler) => handler,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        },
    };

    let mut handler = handler_lock.lock().await;

    if handler.is_deaf() {
        check_msg(msg.channel_id.say(&ctx.http, "Already deafened").await);
    } else {
        if let Err(e) = handler.deafen(true).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Deafened").await);
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn join(ctx: &Context, msg: &Message) -> CommandResult {
    let (guild_id, channel_id) = {
        let guild = msg.guild(&ctx.cache).unwrap();
        let channel_id = guild
            .voice_states
            .get(&msg.author.id)
            .and_then(|voice_state| voice_state.channel_id);

        (guild.id, channel_id)
    };

    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        },
    };

    let data = ctx.data::<UserData>();
    if let Ok(handler_lock) = data.songbird.join(guild_id, connect_to).await {
        let call_lock_for_evt = Arc::downgrade(&handler_lock);

        let mut handler = handler_lock.lock().await;
        check_msg(
            msg.channel_id
                .say(&ctx.http, &format!("Joined {}", connect_to.mention()))
                .await,
        );

        let source = data
            .sound_store
            .get("song")
            .map(|s| s.value().into())
            .expect("Handle placed into cache at startup.");

        let song = handler.play_input(source);
        let _ = song.set_volume(1.0);
        let _ = song.enable_loop();

        // Play a guitar chord whenever the main backing track loops.
        let _ = song.add_event(
            Event::Track(TrackEvent::Loop),
            LoopPlaySound {
                call_lock: call_lock_for_evt,
                data,
            },
        );
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Error joining the channel")
                .await,
        );
    }

    Ok(())
}

struct LoopPlaySound {
    call_lock: Weak<Mutex<Call>>,
    data: Arc<UserData>,
}

#[async_trait]
impl VoiceEventHandler for LoopPlaySound {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        if let Some(call_lock) = self.call_lock.upgrade() {
            let src = {
                self.data
                    .sound_store
                    .get("loop")
                    .map(|c| c.value().into())
                    .expect("Handle placed into cache at startup.")
            };

            let mut handler = call_lock.lock().await;
            let sound = handler.play_input(src);
            let _ = sound.set_volume(0.5);
        }

        None
    }
}

#[command]
#[only_in(guilds)]
async fn leave(ctx: &Context, msg: &Message) -> CommandResult {
    let guild_id = msg.guild_id.unwrap();

    let manager = &ctx.data::<UserData>().songbird;
    let has_handler = manager.get(guild_id).is_some();

    if has_handler {
        if let Err(e) = manager.remove(guild_id).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Left voice channel").await);
    } else {
        check_msg(msg.reply(ctx, "Not in a voice channel").await);
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn mute(ctx: &Context, msg: &Message) -> CommandResult {
    let guild_id = msg.guild_id.unwrap();
    let manager = &ctx.data::<UserData>().songbird;

    let handler_lock = match manager.get(guild_id) {
        Some(handler) => handler,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        },
    };

    let mut handler = handler_lock.lock().await;

    if handler.is_mute() {
        check_msg(msg.channel_id.say(&ctx.http, "Already muted").await);
    } else {
        if let Err(e) = handler.mute(true).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Now muted").await);
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn ting(ctx: &Context, msg: &Message, _args: Args) -> CommandResult {
    let guild_id = msg.guild_id.unwrap();
    let data = ctx.data::<UserData>();

    if let Some(handler_lock) = data.songbird.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        let source = data
            .sound_store
            .get("ting")
            .map(|c| c.value().into())
            .expect("Handle placed into cache at startup.");

        let _sound = handler.play_input(source);

        check_msg(msg.channel_id.say(&ctx.http, "Ting!").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to play in")
                .await,
        );
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn undeafen(ctx: &Context, msg: &Message) -> CommandResult {
    let guild_id = msg.guild_id.unwrap();

    let manager = &ctx.data::<UserData>().songbird;
    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        if let Err(e) = handler.deafen(false).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Undeafened").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to undeafen in")
                .await,
        );
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn unmute(ctx: &Context, msg: &Message) -> CommandResult {
    let guild_id = msg.guild_id.unwrap();
    let manager = &ctx.data::<UserData>().songbird;

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        if let Err(e) = handler.mute(false).await {
            check_msg(
                msg.channel_id
                    .say(&ctx.http, format!("Failed: {:?}", e))
                    .await,
            );
        }

        check_msg(msg.channel_id.say(&ctx.http, "Unmuted").await);
    } else {
        check_msg(
            msg.channel_id
                .say(&ctx.http, "Not in a voice channel to unmute in")
                .await,
        );
    }

    Ok(())
}

/// Checks that a message successfully sent; if not, then logs why to stdout.
fn check_msg(result: SerenityResult<Message>) {
    if let Err(why) = result {
        println!("Error sending message: {:?}", why);
    }
}
