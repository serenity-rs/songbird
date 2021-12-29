//! Requires the "client", "standard_framework", and "voice" features be enabled in your
//! Cargo.toml, like so:
//!
//! ```toml
//! [dependencies.serenity]
//! git = "https://github.com/serenity-rs/serenity.git"
//! features = ["client", standard_framework", "voice"]
//! ```
use std::env;

// This trait adds the `register_songbird` and `register_songbird_with` methods
// to the client builder below, making it easy to install this voice client.
// The voice client can be retrieved in any command using `songbird::get(ctx).await`.
use songbird::SerenityInit;

// Import the `Context` to handle commands.
use serenity::client::Context;

use serenity::{
    async_trait,
    client::{Client, EventHandler},
    framework::{
        StandardFramework,
        standard::{
            Args, CommandResult,
            macros::{command, group},
        },
    },
    model::{channel::Message, gateway::Ready},
    Result as SerenityResult,
};

use songbird::input::Compose;

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
}

#[group]
#[commands(deafen, join, leave, mute, play, play_url, ping, undeafen, unmute)]
struct General;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    
    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN")
        .expect("Expected a token in the environment");

    let framework = StandardFramework::new()
        .configure(|c| c
                   .prefix("~"))
        .group(&GENERAL_GROUP);

    let mut client = Client::builder(&token)
        .event_handler(Handler)
        .framework(framework)
        .register_songbird()
        .await
        .expect("Err creating client");

    tokio::spawn(async move {
        let _ = client.start().await.map_err(|why| println!("Client ended: {:?}", why));
    });

    /*
    // Symphonia testing.
    // let reg = symphonia::default::get_codecs();
    let mut reg = symphonia::core::codecs::CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut reg);
    reg.register_all::<songbird::input::codec::SymphOpusDecoder>();

    let probe = symphonia::default::get_probe();

    let mut probe = symphonia::core::probe::Probe::default();
    probe.register_all::<songbird::input::SymphDcaReader>();
    symphonia::default::register_enabled_formats(&mut probe);

    let formats = [
        "02-gojira-amazonia.mp3",
        "02-gojira-amazonia.ogg",
        "02-gojira-amazonia.opus",
        "02-gojira-amazonia.flac",
        // "ckick-dca0.dca",
        "ckick-dca1.dca",
    ];
    for target_file in formats {
        let path = std::path::Path::new(target_file);
        let file = std::fs::File::open(path).unwrap();
        let mss = symphonia::core::io::MediaSourceStream::new(Box::new(file), Default::default());

        let ext = path.extension().and_then(|v| v.to_str());
        let ext = ext.as_ref().unwrap();

        println!("Hint ext: {:?}", ext);

        let mut hint = symphonia::core::probe::Hint::new();
        hint.with_extension(ext);

        let f = probe.format(
            &hint,
            mss,
            &Default::default(),
            &Default::default(),
        );

        match f {
            Ok(pr) => {
                let mut formatter = pr.format;
                println!("Symph ({}):", target_file);//formatter.tracks());

                let mut tracks = std::collections::HashMap::new();

                for track in formatter.tracks() {
                    println!("\tTrack {}: {:?} {}", track.id, track.language, track.codec_params.codec);
                    match reg.make(&track.codec_params, &Default::default()) {
                        Ok(mut txer) => {
                            println!("\t\tMake success!");

                            tracks.insert(track.id, txer);
                        },
                        Err(e) => {
                            println!("\t\tMake error: {:?}", e);
                        },
                    }
                }

                while let Ok(pkt) = formatter.next_packet() {
                    if let Some(txer) = tracks.get_mut(&pkt.track_id()) {
                        if let Err(e) = txer.decode(&pkt) {
                            println!("\t\t\tERROR: {:?}", e);
                        }
                    }
                }
            },
            Err(e) => {
                println!("Symph error ({}): {:?}", target_file, e);
            },
        }
    }

    println!("Done!");
    */
    
    tokio::signal::ctrl_c().await;
    println!("Received Ctrl-C, shutting down.");
}

#[command]
#[only_in(guilds)]
async fn deafen(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await
        .expect("Songbird Voice client placed in at initialisation.").clone();

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
            check_msg(msg.channel_id.say(&ctx.http, format!("Failed: {:?}", e)).await);
        }

        check_msg(msg.channel_id.say(&ctx.http, "Deafened").await);
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn join(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let channel_id = guild
        .voice_states.get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id);

    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            check_msg(msg.reply(ctx, "Not in a voice channel").await);

            return Ok(());
        }
    };

    let manager = songbird::get(ctx).await
        .expect("Songbird Voice client placed in at initialisation.").clone();

    let _handler = manager.join(guild_id, connect_to).await;

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn leave(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await
        .expect("Songbird Voice client placed in at initialisation.").clone();
    let has_handler = manager.get(guild_id).is_some();

    if has_handler {
        if let Err(e) = manager.remove(guild_id).await {
            check_msg(msg.channel_id.say(&ctx.http, format!("Failed: {:?}", e)).await);
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
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await
        .expect("Songbird Voice client placed in at initialisation.").clone();

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
            check_msg(msg.channel_id.say(&ctx.http, format!("Failed: {:?}", e)).await);
        }

        check_msg(msg.channel_id.say(&ctx.http, "Now muted").await);
    }

    Ok(())
}

#[command]
async fn ping(ctx: &Context, msg: &Message) -> CommandResult {
    check_msg(msg.channel_id.say(&ctx.http, "Pong!").await);

    let songs = [
        // "02-gojira-amazonia.mp3",
        // "02-gojira-amazonia.ogg",
        // "04 - Fix The Error.m4a",
        // "02-gojira-amazonia.opus",
        // "02-gojira-amazonia.flac",
        "monot.mp3",
        // // "ckick-dca0.dca",
        // "ckick-dca1.dca",
    ];

    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await
        .expect("Songbird Voice client placed in at initialisation.").clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        let path = std::path::Path::new(songs[0]);
        let file = songbird::input::File::new(path);


        // use std::io::Read;
        // let mut testy = test.new_handle().raw;
        // let mut space = [0u8; 1024 + 2048 + 4096];
        // testy.read(&mut space[..1024]);
        // testy.read(&mut space[1024..1024+2048]);
        // testy.read(&mut space[1024+2048..]);
        // println!("{:x?}", &space[3934-4..][..12]);


        // let test = songbird::input::cached::Memory::new(file.into()).await.unwrap();

        // let test = songbird::input::cached::Compressed::new(file.into(), songbird::driver::Bitrate::BitsPerSecond(128_000)).await.unwrap();
        // let handle = handler.play_source(test.into());

        let handle = handler.play_source(file.into());

        // tokio::spawn(async move {
        //     println!("Spawned!");
        //     tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        //     handle.seek_time(std::time::Duration::from_secs(60));
        //     println!("Sent!");
        //     tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        //     handle.seek_time(std::time::Duration::from_secs(20));
        //     println!("Sent!");
        // });

        check_msg(msg.channel_id.say(&ctx.http, "Playing song").await);
    } else {
        check_msg(msg.channel_id.say(&ctx.http, "Not in a voice channel to play in").await);
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn play(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let url = match args.single::<String>() {
        Ok(url) => url,
        Err(_) => {
            check_msg(msg.channel_id.say(&ctx.http, "Must provide a URL to a video or audio").await);

            return Ok(());
        },
    };

    if !url.starts_with("http") {
        check_msg(msg.channel_id.say(&ctx.http, "Must provide a valid URL").await);

        return Ok(());
    }

    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await
        .expect("Songbird Voice client placed in at initialisation.").clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        let mut src = songbird::input::YoutubeDl::new_ytdl_like("yt-dlp", reqwest::Client::new(), url);
        // src.create_async().await;

        // let source = match songbird::ytdl(&url).await {
        //     Ok(source) => source,
        //     Err(why) => {
        //         println!("Err starting source: {:?}", why);

        //         check_msg(msg.channel_id.say(&ctx.http, "Error sourcing ffmpeg").await);

        //         return Ok(());
        //     },
        // };

        handler.play_source(src.into());

        check_msg(msg.channel_id.say(&ctx.http, "Playing song").await);
    } else {
        check_msg(msg.channel_id.say(&ctx.http, "Not in a voice channel to play in").await);
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn play_url(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let url = match args.single::<String>() {
        Ok(url) => url,
        Err(_) => {
            check_msg(msg.channel_id.say(&ctx.http, "Must provide a URL to a video or audio").await);

            return Ok(());
        },
    };

    if !url.starts_with("http") {
        check_msg(msg.channel_id.say(&ctx.http, "Must provide a valid URL").await);

        return Ok(());
    }

    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await
        .expect("Songbird Voice client placed in at initialisation.").clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;

        let lazy = songbird::input::HttpRequest {
            client: reqwest::Client::new(),
            request: url.to_string(),
        };

        let handle = handler.play_source(lazy.into());

        // let source = match songbird::input(&url).await {
        //     Ok(source) => source,
        //     Err(why) => {
        //         println!("Err starting source: {:?}", why);

        //         check_msg(msg.channel_id.say(&ctx.http, "Error sourcing ffmpeg").await);

        //         return Ok(());
        //     },
        // };

        // handler.play_source(source);

        tokio::spawn(async move {
            println!("Spawned!");
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            handle.seek_time(std::time::Duration::from_secs(5));
            println!("Sent!");
        });

        check_msg(msg.channel_id.say(&ctx.http, "Playing song").await);
    } else {
        check_msg(msg.channel_id.say(&ctx.http, "Not in a voice channel to play in").await);
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn undeafen(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;

    let manager = songbird::get(ctx).await
        .expect("Songbird Voice client placed in at initialisation.").clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;
        if let Err(e) = handler.deafen(false).await {
            check_msg(msg.channel_id.say(&ctx.http, format!("Failed: {:?}", e)).await);
        }

        check_msg(msg.channel_id.say(&ctx.http, "Undeafened").await);
    } else {
        check_msg(msg.channel_id.say(&ctx.http, "Not in a voice channel to undeafen in").await);
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn unmute(ctx: &Context, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap();
    let guild_id = guild.id;
    
    let manager = songbird::get(ctx).await
        .expect("Songbird Voice client placed in at initialisation.").clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut handler = handler_lock.lock().await;
        if let Err(e) = handler.mute(false).await {
            check_msg(msg.channel_id.say(&ctx.http, format!("Failed: {:?}", e)).await);
        }

        check_msg(msg.channel_id.say(&ctx.http, "Unmuted").await);
    } else {
        check_msg(msg.channel_id.say(&ctx.http, "Not in a voice channel to unmute in").await);
    }

    Ok(())
}

/// Checks that a message successfully sent; if not, then logs why to stdout.
fn check_msg(result: SerenityResult<Message>) {
    if let Err(why) = result {
        println!("Error sending message: {:?}", why);
    }
}
