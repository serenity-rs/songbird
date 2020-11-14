[![docs-badge][]][docs] [![build badge]][build]

# Songbird

![](songbird.png)

Songbird is an async, cross-library compatible voice system for Discord, written in Rust.
The library offers:
 * A standalone gateway frontend compatible with [serenity] and [twilight] using the
 `"gateway"` and `"[serenity/twilight]-[rustls/native]"` features. You can even run
 driverless, to help manage your [lavalink] sessions.
 * A standalone driver for voice calls, via the `"driver"` feature. If you can create
 a `ConnectionInfo` using any other gateway, or language for your bot, then you
 can run the songbird voice driver.
 * And, by default, a fully featured voice system featuring events, queues, RT(C)P packet
 handling, seeking on compatible streams, shared multithreaded audio stream caches,
 and direct Opus data passthrough from DCA files.

## Intents
Songbird's gateway functionality requires you to specify the `GUILD_VOICE_STATES` intent.

## Dependencies

Songbird needs a few system dependencies before you can use it.

- Opus - Audio codec that Discord uses.
You can install the library with `apt install libopus-dev` on Ubuntu or `pacman -S opus` on Arch Linux.
If you do not have it installed it will be built for you. However, you will need a C compiler and the GNU autotools installed.
Again, these can be installed with `apt insall build-essential autoconf automake libtool m4` on Ubuntu or `pacman -S base-devel` on Arch Linux.

This is a required dependency. Songbird cannot work without it.

- FFmpeg - Audio/Video conversion tool.
You can install the tool with `apt install ffmpeg` on Ubuntu or `pacman -S ffmpeg` on Arch Linux.

This is an optional, but recommended dependency. It allows Songbird to convert from, for instance, .mp4 files to the audio format Discord uses.

- youtube-dl - Audio/Video download tool.
You can install the tool with `apt install youtube-dl` on Ubuntu or `pacman -S youtube-dl` on Arch Linux.

This is an optional dependency. It allows Songbird to download an audio source from the Internet, which will be converted to the audio format Discord uses.

## Examples
Full examples showing various types of functionality and integrations can be found in [this crate's examples directory].

## Attribution

Songbird's logo is based upon the copyright-free image ["Black-Capped Chickadee"] by George Gorgas White.

[serenity]: https://github.com/serenity-rs/serenity
[twilight]: https://github.com/twilight-rs/twilight
["Black-Capped Chickadee"]: https://www.oldbookillustrations.com/illustrations/black-capped-chickadee/
[lavalink]: https://github.com/Frederikam/Lavalink
[this crate's examples directory]: https://github.com/serenity-rs/songbird/tree/current/examples

[build badge]: https://img.shields.io/github/workflow/status/serenity-rs/songbird/CI?style=flat-square
[build]: https://github.com/serenity-rs/songbird/actions

[docs-badge]: https://img.shields.io/badge/docs-online-4d76ae.svg?style=flat-square
[docs]: https://serenity-rs.github.io/songbird/current
