# Changelog

## [0.3.2] - 2023-04-09

This patch release fixes a WS disconnection that would occur when receiving a
new opcode, which was happening due to Discord sending such an opcode upon
connecting to a voice channel.

Thanks to the following for their contributions:

- [@Erk-]
- [@FelixMcFelix]

### Fixed

- [gateway] Songbird would fail if it could not deserialize ws payload ([@Erk-]) [c:752cae7]
- [docs] Fix compilation due to ambiguous reference ([@FelixMcFelix]) [c:e5d3feb]

## [0.3.1] — 2023-03-02

This patch release applies some minor fixes, while correcting documentation errors and adjusting some organisaation in the repository.

Thanks to the following for their contributions:

- [@btoschek]
- [@FelixMcFelix]
- [@JamesDSource]
- [@tazz4843]

### Added

- [repo] Repo: Update issue templates ([@FelixMcFelix]) [c:eedab8f]

### Fixed

- [docs] Chore: Fix README.md CI badge (#161) ([@FelixMcFelix]) [c:d6c82f5]
- [input] Input: Fix read position after seek restart (#154) ([@JamesDSource]) [c:39a6f69]
- [docs] Docs: Fix wrong docstring for Track::volume (#152) ([@btoschek]) [c:a2f55b7]
- [docs] Events: Fix typo in docs for VoiceData (#142) ([@tazz4843]) [c:dc53087]

## [0.3.0] — 2022-07-22 — **Chaffinch**

Abundant and ever-curious, chaffinches are a vibrant and welcome visitor in these spring and summer months.

Making a quick and colourful splash, this breaking release mainly bumps our own dependencies and support for Discord libraries without any sweeping changes -- while adding generic support for any future rust-based Discord library. However, we have now removed support for the v0.2 series of the Tokio runtime.

Thanks to the following for their contributions:

- [@Erk-]
- [@FelixMcFelix]
- [@GnomedDev]
- [@tktcorporation]
- [@vaporox]
- [@wlcx]

### Upgrade Pathway
* Tokio v0.2 support has been removed in parity with other Discord libraries -- users must now migrate to v1.x.x.
* Deprecated events (`ClientConnect`, `DriverConnectFailed`, `DriverReconnectFailed` and `SsrcKnown`) have been removed.
 * `ClientConnect` must now be detected using VoiceStateUpdate messages from your main gateway library of choice.
 * The remainder should be replaced with `DriverDisconnect`, and `DriverConnect`/`DriverReconnect`

### Added

- [queue] driver, queue: return track handle when adding an `Input` to the queue (#116) ([@vaporox]) [c:bacf681]
- [gateway] Gateway: Generic Shard and Twilight v0.8 Support (#109) ([@FelixMcFelix]) [c:b4ce845]
- [gateway] Gateway: Add generics to `Call` methods. (#102) ([@FelixMcFelix]) [c:8dedf3b]
- [docs] Events: Document format of `VoiceData`. (#114) ([@FelixMcFelix]) [c:806a422]

### Changed

- [deps] Chore: Update to twilight 0.12 ([@FelixMcFelix]) [c:865c75f]
- [deps] Chore: Update to serenity 0.11 ([@FelixMcFelix]) [c:a85a1f0]
- [deps] Update twilight support to twilight 0.11 (#132) ([@Erk-]) [c:69339e8]
- [deps] Deps: Update to Audiopus v0.3.0-rc.0 (#125) ([@FelixMcFelix]) [c:4eb95d4]
- [deps] Deps: Bump dependencies and document bumped MSRV (#119) ([@GnomedDev]) [c:98f0d02]
- [deps] Gateway: Twilight v0.10 support (#117) ([@FelixMcFelix]) [c:fac6664]
- [deps] Gateway: Twilight v0.9 support (#110) ([@FelixMcFelix]) [c:0730a00]
- [gateway] Gateway: Remove lifetime from Serenity setup trait (#103) ([@FelixMcFelix]) [c:12c76a9]
- [deps] Deps: Bump streamcatcher version -> 1.0 (#93) ([@tktcorporation]) [c:67ad7c9]
- [docs] Chore: Update link to lavalink-basic-bot.rs (#135) ([@wlcx]) [c:f9b7e76]
- [deps] Chore: Pin flume version to prevent MSRV breakage. ([@FelixMcFelix]) [c:312457e]
- [deps] Chore: Bump MSRV to 1.51.0 ([@FelixMcFelix]) [c:05c6762]

### Fixed

- [examples] Examples: support new Serenity Intents init ([@FelixMcFelix]) [c:d3a40fe]
- [examples] Examples: Fix serenity-next cache accesses (#99) ([@FelixMcFelix]) [c:f1ed41e]
- [driver] Driver: Prevent panic when decrypting undersized RTP packets (#122) ([@FelixMcFelix]) [c:8791805]

### Removed

- [driver] Driver: Remove spin_sleep in `Mixer::march_deadline` (#124) ([@GnomedDev]) [c:e3476e7]
- [driver] Driver, Gateway: Remove tokio 0.2 support (#118) ([@GnomedDev]) [c:f2cd8a0]
- [events] Events: Remove deprecated events. (#115) ([@FelixMcFelix]) [c:ac20764]

## [0.2.2] — 2022-02-13

This patch release makes it easier to create new `ChildContainer`s, and deprecates the `ClientConnect` event. Users should instead make use of `SpeakingStateUpdate` events and Discord gateway events.

Thanks to the following for their contributions:

- [@asg051]
- [@FelixMcFelix]
- [@reiyw]

### Added

- [input] Input: add ChildContainer::new (#108) ([@asg051]) [c:ecc47d5]

### Changed

- [events] Events: Deprecate `ClientConnect` (#112) ([@FelixMcFelix]) [c:c464fcc]

### Fixed

- [docs] Docs: fix ClientConnect to recommend `SpeakingStateUpdate` ([@FelixMcFelix]) [c:652ec1f]
- [repo] Chore: Fix typo in CHANGELOG.md (#111) ([@reiyw]) [c:2feadc7]

## [0.2.1] — 2022-01-05

This patch release adds support for the `yt-dlp` fork of `youtube-dl`, and fixes track events to correctly fire events when multiple timed handlers are present on a track.

Thanks to the following for their contributions:

- [@FelixMcFelix]
- [@Lunarmagpie]
- [@lajp]
- [@Miezhiko]

### Added

- [docs] Docs: added documentation for `yt-dlp` feature (#106) ([@Lunarmagpie]) [c:73323e5]
- [input] Input: Allows yt-dlp usage as another youtube-dl fork (#90) ([@Miezhiko]) [c:6fcb196]

### Fixed

- [docs] Examples: Fix unmatched quotation mark in comment. (#101) ([@lajp]) [c:62ecfe6]
- [events] Events: fix handling of multiple timed events on a single track (#96) ([@FelixMcFelix]) [c:e25cc14]

## [0.2.0] — 2021-08-17 — **Magpie**

Magpies are a common sight year-round; strong, intelligent, industrious, and loyal.

Taking after the humble magpie, this breaking release makes API changes favouring extensibility, patching some of the API rough spots, and adding resilience to some additional classes of failure.

Thanks to the following for their contributions:

- [@clarity0]
- [@james7132]
- [@FelixMcFelix]
- [@vilgotf]

### Upgrade Pathway
* References to `songbird::{opus, Bitrate};` should now use `songbird::driver::{opus, Bitrate};`.
* Custom `Inputs` (i.e., `Reader::Extension`/`ExtensionSeek`) now need to implement `input::reader::MediaSource` rather than just `Read` and/or `Seek`.
 * Sources which do not support seeking should have an `unreachable!()` function body or always return an error, as `MediaSource::is_seekable()` is used to gate support.
* Many event handler types in `songbird::EventContext` have changed to unit `enum`s, rather than `struct` variants.
 * New body types are included in `songbird::events::context_data::*`.
* `Config` structs have been made non-exhaustive; they should be initialised via `Config::default()`.
* Channel join operations may now timeout after a default 10s—which *should* be handled.
* Errors returned when joining a channel will now inform you whether you should try to `leave` a channel before rejoining.
* Youtube-dl variants of `songbird::input::error::Error` have had their case altered from `DL` -> `Dl`.
* `TrackState` sent from the driver are no longer boxed objects.
* `DriverDisconnect` events have been introduced, which cover *all* disconnect events. As a result, `DriverConnectFailed` and `DriverReconnectFailed` are deprecated.
* **Tokio 0.2 support is deprecated. Related features will be removed as of Songbird 0.3.**

### Added

- [driver] Driver: Automate (re)connection logic (#81) ([@FelixMcFelix]) [c:210e3ae]
- [input] Input: Add separate YouTube title and channel to Metadata (#75) ([@vilgotf]) [c:edcd39a]
- [input] Input: Implement StdError for DcaError, input::Error (#73) ([@vilgotf]) [c:e1fc041]
- [gateway] Gateway: Add debug logging around shard handling ([@FelixMcFelix]) [c:b3caf05]
- [gateway] Gateway: Add connection timeout, add `Config` to gateway. (#51) ([@FelixMcFelix]) [c:d303e0a]

### Changed

- [deps] Deps: Bump async-tungstenite version -> 0.14 ([@FelixMcFelix])  [c:47e20d6]
- [docs] Chore: Update Lavalink URLs ([@FelixMcFelix]) [c:3efe756]
- [deps] Deps: Bump twilight versions -> [0.5, 0.7) (#87) ([@vilgotf]) [c:91d7542]
- [tracks] Tracks: Remove box around TrackState (#84) ([@vilgotf]) [c:91d7542]
- [deps] Deps: Bump twilight versions -> 0.5 (#79) ([@vilgotf]) [c:d2bb277]
- [input] Input, Driver: Make error messages more idiomatic (#74) ([@vilgotf]) [c:a96f033]
- [docs] Chore: Rewrite update pathway. ([@FelixMcFelix]) [c:8000da6]
- [deps] Deps: Bump DiscoRTP version -> 0.4 ([@FelixMcFelix]) [c:7fc971a]
- [deps] Deps: Bump twilight versions -> 0.4 ([@FelixMcFelix]) [c:fc94ddb]
- [deps] Deps: Bump xsalsa20poly1305 version -> 0.7 ([@FelixMcFelix]) [c:eb22443]
- [input] Input: Change all Youtube-dl functions to take `AsRef<str>` (#70) ([@clarity0]) [c:a1ba760]
- [gateway] Chore: Adapt #60, #64 in line with other breaking changes. ([@FelixMcFelix]) [c:63d53b2]
- [input] Use symphonia::io::MediaSource for Reader extensions (#61) ([@james7132]) [c:a86898c]
- [input] Input: Rename YTDL error variants for Clippy (#55) ([@FelixMcFelix]) [c:3c7f86d]
- [events] Events: Break out and non-exhaust context body structs (#54) ([@FelixMcFelix]) [c:e7af0ff]
- [driver] Driver: Move `Bitrate` import out of crate root. (#53) ([@FelixMcFelix]) [c:1eed9dd]
- [deps] Deps: Bump DiscoRTP version -> 0.3 (#52) ([@FelixMcFelix]) [c:bc952d0]

### Fixed

- [driver] Driver: Fix incorrect leave behaviour in Drop handler ([@FelixMcFelix]) [c:dad48ca]
- [benchmarks] Fix: Update Benchmark Imports ([@FelixMcFelix]) [c:338a042]
- [lint] Chore: Clippy fixes for new lints ([@FelixMcFelix]) [c:a1c4f07]
- [fmt] Chore: Repair formatting. ([@FelixMcFelix]) [c:cd2ade9]
- [fmt] Chore: Fix clippy warnings (useless clones). ([@FelixMcFelix]) [c:21b8383]
- [gateway] Gateway: Fix repeat joins on same channel from stalling (#47) ([@FelixMcFelix]) [c:95dd19e]

## [0.1.8] — 2021-07-01

This release patches a metadata parsing panic caused by Ogg files with negative start times.

Thanks to the following for their contributions:

- [@JellyWX]

### Fixed

- [input] Input: Fix Duration underflow on negative start time (#83) ([@JellyWX]) [c:e58cadb]

## [0.1.7] — 2021-06-14

This release mainly patches an occasionally spinning task, due to a critical WebSocket read error.

Thanks to the following for their code contributions:

- [@FelixMcFelix]
- [@vilgotf]

And special thanks to [@jtscuba] and [@JellyWX] for their efforts in reproducing, debugging and diagnosing the above issue.

### Changed

- [tracks] Tracks: Simplify track end event handler (#77) ([@vilgotf]) [c:c97f23e]

### Fixed

- [driver] Driver: Fix for busy-wait in WS thread. (#78) ([@FelixMcFelix]) [c:b925309]

## [0.1.6] — 2021-04-11

This patch release fixes a driver crash on leaving a channel, adds a utility method for requesting the current channel ID, and limits a sub-dependency to ensure compatibility with Rust v1.48.0.

Thanks to the following for their contributions:

- [@DoumanAsh]
- [@FelixMcFelix]

### Added

- [gateway] Gateway: Introduce Call::current_channel (#60) ([@DoumanAsh]) [c:22214a0]

### Fixed

- [deps] Deps: Prevent MSRV breakage via `spinning_top` (#64) ([@FelixMcFelix]) [c:a88b185]
- [driver] Driver: Fix crash on `.leave()` (#63) ([@FelixMcFelix]) [c:24d8da6]

## [0.1.5] — 2021-03-23

This patch release adds bugfixes for incorrect seeking in Restartable sources and resource usage of inactive `Driver`s, as well as some utility methods and reduced logging.

Thanks to the following for their contributions:

- [@DasEtwas]
- [@FelixMcFelix]

### Added

- [gateway] Gateway: Allow connection info to be retrieved (#49) ([@FelixMcFelix]) [c:db79940]
- [misc] Repo: Organise and document processes and architecture (#43) ([@FelixMcFelix]) [c:1fcc8c0]

### Changed

- [deps] Deps: Update async-tungstenite -> 0.13 (#50) ([@FelixMcFelix]) [c:f230b41]
- [driver] Driver: Reduce logging level in general (#48) ([@FelixMcFelix]) [c:a3f86ad]

### Fixed

- [driver] Prevent mixer thread from waking while inactive (#46) ([@FelixMcFelix]) [c:a9b4cb7]
- [input] Fix input source timestamp pre-input argument decimal formatting (#45) ([@DasEtwas]) [c:c488ce3]
- [examples] Break reference cycle in voice storage example (#44) ([@FelixMcFelix]) [c:b9a926c]

## [0.1.4] — 2021-02-10

This patch release updates introduces a new event type, to expose a driver's SSRC externally on connect.

Thanks to the following for their contributions:

- [@FelixMcFelix]

### Added

- [events] Events: Add `SsrcKnown` event ([@FelixMcFelix]) [c:f3f5242]
- [misc] Chore: Add missing changelog notes for 0.1.3 ([@FelixMcFelix]) [c:0e860dc]

### Changed

- [deps] Deps: Update async-tungstenite -> 0.12 ([@FelixMcFelix]) [c:a40fac3]

## [0.1.3] — 2021-02-04

This patch release corrects the process drop logic to cleanup *all* chained child processes, and for `Input`s to be safe to drop in async contexts. Additionally, this adds backwards-compatibility for Tokio 0.2 in serenity-based bots.

Thanks to the following for their contributions:

- [@FelixMcFelix]

### Added

- [deps] Chore + Deps: Add the `log` feature to tracing ([@FelixMcFelix]) [c:1863d39]
- [driver] Library: Add compatibility for legacy Tokio 0.2 (#40) ([@FelixMcFelix]) [c:aaab975]

### Fixed

- [input] Fix: hand off process killing to blocking thread, await all children. ([@FelixMcFelix]) [c:b245309]

## [0.1.2] — 2021-01-26

This patch release fixes a PID/zombie process leak affecting bots running on Linux/Mac, and prevents youtube-dl warnings from being converted into fatal errors.

This release also changes `Songbird` managers to use DashMap internally, which should substantially speed up concurrent shard accesses to the central call registry.

Thanks to the following for their contributions:

- [@FelixMcFelix]

### Changed

- [gateway] Gateway: Move from RwLock<HashMap> to DashMap ([@FelixMcFelix]) [c:a0e905a]
- [misc] Chore: Categorise v0.1.1 commits ([@FelixMcFelix]) [c:196d5be]

### Fixed

- [driver] Input & Driver: Fix zombie processes on Unix (#39) ([@FelixMcFelix]) [c:fe2282c]
- [input] Fix: Prevent ytdl treating warnings as errors. ([@FelixMcFelix]) [c:658fd83]

## [0.1.1] — 2021-01-17

This is a short patch release, fixing some error message spam under network failures, adding some new convenience event classes, as well as making it easier to cancel many event handlers.

Thanks to the following for their contributions:

- [@FelixMcFelix]

### Added

- [events] Events: Add Play/Pause events. ([@FelixMcFelix]) [c:868c44c]
- [events] Events: Add (re)connect success/fail events. ([@FelixMcFelix]) [c:cb2398f]
- [driver] Driver: Add ability to clear all global event handlers. ([@FelixMcFelix]) [c:55b8e7f]

### Fixed

- [driver] Driver: Fix noisy errors, UDP message send failure spam. ([@FelixMcFelix]) [c:dcb6ad9]

## [0.1.0] — 2021-01-08 — **Robin**

We're very excited to publish and announce the first release of Songbird, an async Rust voice library for Discord!
It's been a long time coming, but all the hard work has paid off in bringing the first version of this library to completion.

Thanks to the following for their contributions:

- [@acdenisSK]
- [@FelixMcFelix]
- [@Maspenguin]
- [@peppizza]
- [@saanuregh]

Songbird is based heavily on serenity's `voice` module, which has served as its base design, informed many of the design changes, and paved a lot of the API/protocol research needed.
We'd also like to thank all users who have contributed to this module in the past for laying the groundwork for Songbird:

- [@Arcterus]
- [@acdenisSK]
- [@Elinvynia]
- [@Erk-]
- [@FelixMcFelix]
- [@Flat]
- [@ForsakenHarmony]
- [@ftriquet]
- [@hiratara]
- [@indiv0]
- [@JellyWX]
- [@Lakelezz]
- [@LikeLakers2]
- [@mendess]
- [@nickelc]
- [@nitsuga5124]
- [@perryprog]
- [@Prof-Bloodstone]
- [@Proximyst]
- [@Roughsketch]
- [@s0lst1ce]
- [@Sreyas-Sreelal]
- [@tarcieri]
- [@vivian]

### Added

- [tracks] Tracks: Add TypeMap to Handles. ([@FelixMcFelix]) [c:d42e09f]
- [tracks] Tracks: Allow custom UUID setting (#33) ([@peppizza]) [c:873458d]
- [input] Input: Allow Restartable sources to be lazy ([@FelixMcFelix]) [c:03ae0e7]
- [driver] Driver, Input: Performance & Benchmarks (#27) ([@FelixMcFelix]) [c:504b8df]
- [input] Metadata: Add source_url and thumbnail fields (#28) ([@saanuregh]) [c:700f20d]
- [tracks] TrackHandle: add metadata field (#25) ([@peppizza]) [c:57df3fe]
- [tracks] TrackQueue: Add current_queue method (#16) ([@peppizza]) [c:69acea8]
- [tracks] TrackQueues: Convenience methods and extension (#7) ([@FelixMcFelix]) [c:de65225]
- [docs] Docs: Add a dependencies section in the README (#2) ([@acdenisSK]) [c:047ce03]
- [input] Offer youtube-dlc as an alternative to youtube-dl (#1) ([@peppizza]) [c:6702520]
- [misc] Attempt CI similar to serenity ([@FelixMcFelix]) [c:c5ce107]
- [misc] Add the ISC license ([@acdenisSK]) [c:a778d24]
- [examples] Move examples from the Serenity repository ([@acdenisSK]) [c:f5bf54a]
- [misc] Add a `.gitignore` file as songbird is in its own repository ([@acdenisSK]) [c:ec7f5bc]
- [driver] Implement Songbird driver configuration (#1074) ([@FelixMcFelix]) [c:8b7f388]
- [docs] Document intents for Songbird (#1061) ([@FelixMcFelix]) [c:38a55da]
- [driver] Voice Rework -- Events, Track Queues (#806) ([@FelixMcFelix]) [c:7e4392a]

### Changed

- [misc] Chore: Bump to published twilight. ([@FelixMcFelix]) [c:7d767d2]
- [misc] Chore: Bump to published serenity. ([@FelixMcFelix]) [c:53ab9da]
- [misc] Songbird: Tokio 1.0 (#36) ([@FelixMcFelix]) [c:f05b741]
- [docs] Docs: Warn about twilight task deadlock ([@FelixMcFelix]) [c:c0d3cb3]
- [misc] Deps: Patch flume. ([@FelixMcFelix]) [c:2fc88a6]
- [input] Input: Json parsing errors now contain the parsed text (#31) ([@Maspenguin]) [c:8d6bd4f]
- [driver] Driver, Tracks: Cleanup of leaky types (#20) ([@FelixMcFelix]) [c:f222ce9]
- [docs] Docs: Move to new intra-doc links, make events non-exhaustive. (#19) ([@FelixMcFelix]) [c:94157b1]
- [input] Input: Make restartable sources fully async. (#15) ([@FelixMcFelix]) [c:2da5901]
- [misc] Lint: Clippy warning cleanup (#8) ([@peppizza]) [c:cb7d8cc]
- [docs] Docs: describe `youtube-dlc` feature ([@FelixMcFelix]) [c:45b1fb1]
- [misc] Some updated links, move to current/next branches. ([@FelixMcFelix]) [c:09da85b]
- [misc] Remove mentions of versions to Serenity git dependencies ([@acdenisSK]) [c:4a897a7]
- [misc] Update `Cargo.toml` to reflect the separation of songbird from Serenity's repository ([@acdenisSK]) [c:6724655]
- [misc] Update versions for twilight and serenity-voice-model in songbird (#1075) ([@FelixMcFelix]) [c:868785b]

### Fixed

- [driver] Driver: Handle receiving large non-standard packets (#23) ([@FelixMcFelix]) [c:9fdbcd7]
- [misc] Fix: Remove serenity default features (#18) ([@Maspenguin]) [c:1ada46d]
- [driver] Fix: Use correct tokio features for driver-only mode ([@FelixMcFelix]) [c:a9f8d6c]
- [misc] CI round 2 ([@FelixMcFelix]) [c:35d262d]
- [examples] Fix links in the README regarding examples ([@acdenisSK]) [c:4f5b767]
- [driver] Handle Voice close codes, prevent Songbird spinning WS threads (#1068) ([@FelixMcFelix]) [c:26c9c91]

<!-- COMPARISONS -->
[0.3.2]: https://github.com/serenity-rs/songbird/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/serenity-rs/songbird/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/serenity-rs/songbird/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/serenity-rs/songbird/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/serenity-rs/songbird/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/serenity-rs/songbird/compare/v0.1.8...v0.2.0
[0.1.8]: https://github.com/serenity-rs/songbird/compare/v0.1.7...v0.1.8
[0.1.7]: https://github.com/serenity-rs/songbird/compare/v0.1.6...v0.1.7
[0.1.6]: https://github.com/serenity-rs/songbird/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/serenity-rs/songbird/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/serenity-rs/songbird/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/serenity-rs/songbird/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/serenity-rs/songbird/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/serenity-rs/songbird/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/serenity-rs/songbird/compare/v0.0.1...v0.1.0

<!-- AUTHORS -->
[@acdenisSK]: https://github.com/acdenisSK
[@Arcterus]: https://github.com/Arcterus
[@asg051]: https://github.com/asg051
[@btoschek]: https://github.com/btoschek
[@clarity0]: https://github.com/clarity0
[@DasEtwas]: https://github.com/DasEtwas
[@DoumanAsh]: https://github.com/DoumanAsh
[@Elinvynia]: https://github.com/Elinvynia
[@Erk-]: https://github.com/Erk-
[@FelixMcFelix]: https://github.com/FelixMcFelix
[@Flat]: https://github.com/Flat
[@ForsakenHarmony]: https://github.com/ForsakenHarmony
[@ftriquet]: https://github.com/ftriquet
[@GnomedDev]: https://github.com/GnomedDev
[@hiratara]: https://github.com/hiratara
[@indiv0]: https://github.com/indiv0
[@james7132]: https://github.com/james7132
[@JamesDSource]: https://github.com/JamesDSource
[@JellyWX]: https://github.com/JellyWX
[@jtscuba]: https://github.com/jtscuba
[@Lakelezz]: https://github.com/Lakelezz
[@lajp]: https://github.com/lajp
[@LikeLakers2]: https://github.com/LikeLakers2
[@Lunarmagpie]: https://github.com/Lunarmagpie
[@Maspenguin]: https://github.com/Maspenguin
[@mendess]: https://github.com/mendess
[@Miezhiko]: https://github.com/Miezhiko
[@nickelc]: https://github.com/nickelc
[@nitsuga5124]: https://github.com/nitsuga5124
[@peppizza]: https://github.com/peppizza
[@perryprog]: https://github.com/perryprog
[@Prof-Bloodstone]: https://github.com/Prof-Bloodstone
[@Proximyst]: https://github.com/Proximyst
[@reiyw]: https://github.com/reiyw
[@Roughsketch]: https://github.com/Roughsketch
[@saanuregh]: https://github.com/saanuregh
[@s0lst1ce]: https://github.com/s0lst1ce
[@Sreyas-Sreelal]: https://github.com/Sreyas-Sreelal
[@tarcieri]: https://github.com/tarcieri
[@tazz4843]: https://github.com/tazz4843
[@tktcorporation]: https://github.com/tktcorporation
[@vaporox]: https://github.com/vaporox
[@vilgotf]: https://github.com/vilgotf
[@vivian]: https://github.com/vivian
[@wlcx]: https://github.com/wlcx

<!-- COMMITS -->
[c:e5d3feb]: https://github.com/serenity-rs/songbird/commit/e5d3febb7bfbc6b4b98af3dbf312c23528307544
[c:752cae7]: https://github.com/serenity-rs/songbird/commit/752cae7a09b25f69ffac110ca3ce4c841d1ec99b
[c:eedab8f]: https://github.com/serenity-rs/songbird/commit/eedab8f69d1c17125971e290ee8a50fc1adcdffc
[c:d6c82f5]: https://github.com/serenity-rs/songbird/commit/d6c82f52a6ea876d15a9196de1a7f8a12432407b
[c:39a6f69]: https://github.com/serenity-rs/songbird/commit/39a6f69f2324b89d17d7200905a9737d057c0d7e
[c:a2f55b7]: https://github.com/serenity-rs/songbird/commit/a2f55b7a35539c00e3a75edfb01d1777e8b19741
[c:dc53087]: https://github.com/serenity-rs/songbird/commit/dc530874462d5d929ecdf087d74a1301fc863981
[c:bacf681]: https://github.com/serenity-rs/songbird/commit/bacf68146555db018e59e8276d2617c69a9beaa0
[c:b4ce845]: https://github.com/serenity-rs/songbird/commit/b4ce84546b8e98d696d5b1b37f05c096486cd313
[c:8dedf3b]: https://github.com/serenity-rs/songbird/commit/8dedf3bf011640edf0834c8e931b8e5ca5b406aa
[c:806a422]: https://github.com/serenity-rs/songbird/commit/806a422a2eb6022ddaf9f9c507b9319554f3d42b
[c:865c75f]: https://github.com/serenity-rs/songbird/commit/865c75f3c3131ae43ac4beef5a080993a0bd0d74
[c:a85a1f0]: https://github.com/serenity-rs/songbird/commit/a85a1f08e15541eed9ea026423d9ed6697f390ec
[c:69339e8]: https://github.com/serenity-rs/songbird/commit/69339e8d459d3f2b9b798a16acbb25dc6b756d50
[c:4eb95d4]: https://github.com/serenity-rs/songbird/commit/4eb95d4b59846d7d7d2fcfe3d401646489aa4ca7
[c:98f0d02]: https://github.com/serenity-rs/songbird/commit/98f0d025c04c743654b51c5ca8e3d79e61ab0f55
[c:fac6664]: https://github.com/serenity-rs/songbird/commit/fac6664072ea90bb758ddaa62b01ffa3ab1eaf49
[c:0730a00]: https://github.com/serenity-rs/songbird/commit/0730a00dc7127c710defb0ab7d13c85173ae8ec3
[c:12c76a9]: https://github.com/serenity-rs/songbird/commit/12c76a9046494c929abf8e1e22e8f647109b9caf
[c:67ad7c9]: https://github.com/serenity-rs/songbird/commit/67ad7c9e4925ba68395153ea144b4902e361593c
[c:f9b7e76]: https://github.com/serenity-rs/songbird/commit/f9b7e76bb143c6e3280ed79a8258886492dffc52
[c:312457e]: https://github.com/serenity-rs/songbird/commit/312457eb74130ef30385bbf5a5bfe6e9ce8cd5fd
[c:05c6762]: https://github.com/serenity-rs/songbird/commit/05c676222870b92c5d86816708f1911b2b0fe8f2
[c:d3a40fe]: https://github.com/serenity-rs/songbird/commit/d3a40fe6913c39f866a3c8deea860a314eac009b
[c:f1ed41e]: https://github.com/serenity-rs/songbird/commit/f1ed41ea284de82fc738123a1eb182eb550f9223
[c:8791805]: https://github.com/serenity-rs/songbird/commit/87918058042c6ae8712f29f3558e27de11d15531
[c:e3476e7]: https://github.com/serenity-rs/songbird/commit/e3476e79657b8d418661e75079dfaa1ad299991e
[c:f2cd8a0]: https://github.com/serenity-rs/songbird/commit/f2cd8a0b6a1199f44126ce5b67efdc7c2ccec22b
[c:ac20764]: https://github.com/serenity-rs/songbird/commit/ac20764157e931863acfb3173782bffe650d094c
[c:ecc47d5]: https://github.com/serenity-rs/songbird/commit/ecc47d588ab4bf492cf72d13e1dc0f039f4f3aab
[c:c464fcc]: https://github.com/serenity-rs/songbird/commit/c464fcc38dc180f5409f687bc5efdbbf994b1878
[c:652ec1f]: https://github.com/serenity-rs/songbird/commit/652ec1f2934b50f43819bc92ee70d9d95586a548
[c:2feadc7]: https://github.com/serenity-rs/songbird/commit/2feadc761e01cda2aa2a31265556d9a328460d05
[c:73323e5]: https://github.com/serenity-rs/songbird/commit/73323e58ddf47dfa2bb0e334c37e939cfbd95a86
[c:6fcb196]: https://github.com/serenity-rs/songbird/commit/6fcb196e34922a7ec7e98f874a46e3c3518bfef5
[c:62ecfe6]: https://github.com/serenity-rs/songbird/commit/62ecfe68d640d793c0f6988f88938462ca2d54d7
[c:e25cc14]: https://github.com/serenity-rs/songbird/commit/e25cc140b8151d6546ae0b9c63b6fc0bb8a5e010
[c:47e20d6]: https://github.com/serenity-rs/songbird/commit/47e20d6177bc380d44c8cc456f370d2a22b975fd
[c:dad48ca]: https://github.com/serenity-rs/songbird/commit/dad48ca83595ec6693a4a089c30371e132d099b1
[c:3efe756]: https://github.com/serenity-rs/songbird/commit/3efe756ca505ee50dfdcfb25bac7ed7e58bf723b
[c:1b0bcbb]: https://github.com/serenity-rs/songbird/commit/1b0bcbb5f615843757fa2bc1f9c0d4daa7d3a0d1
[c:210e3ae]: https://github.com/serenity-rs/songbird/commit/210e3ae58499fa45edf9b65de6d9114292341d28
[c:91d7542]: https://github.com/serenity-rs/songbird/commit/91d754259381e709e0768cbf089dbb67ef84680e
[c:338a042]: https://github.com/serenity-rs/songbird/commit/338a04234375768d5f00d989b3ed519654b753ce
[c:e58cadb]: https://github.com/serenity-rs/songbird/commit/e58cadb2a436804fd7af056878fe429770d060d4
[c:c97f23e]: https://github.com/serenity-rs/songbird/commit/c97f23ee2707c8290cdc07a9553ea4a899336c37
[c:b925309]: https://github.com/serenity-rs/songbird/commit/b9253097785a0b37fc104e879c05125fe6e88afb
[c:edcd39a]: https://github.com/serenity-rs/songbird/commit/edcd39a02dbbcb5bd17303d8a6ea6e5c6031d665
[c:d2bb277]: https://github.com/serenity-rs/songbird/commit/d2bb277232e576a1aa27ac1897f4df1aed2791a1
[c:a1c4f07]: https://github.com/serenity-rs/songbird/commit/a1c4f07211226cb425e3c41fdc10cc3a061e9b54
[c:c97f23e]: https://github.com/serenity-rs/songbird/commit/c97f23ee2707c8290cdc07a9553ea4a899336c37
[c:b925309]: https://github.com/serenity-rs/songbird/commit/b9253097785a0b37fc104e879c05125fe6e88afb
[c:e1fc041]: https://github.com/serenity-rs/songbird/commit/e1fc0415b87faca9a405dc4b61e8432733bfeab3
[c:a96f033]: https://github.com/serenity-rs/songbird/commit/a96f03346d0b92cfee0344a934e13a41d83bc821
[c:8000da6]: https://github.com/serenity-rs/songbird/commit/8000da6d9a9ed0fa1d09f313dfab14c6ce64aa34
[c:7fc971a]: https://github.com/serenity-rs/songbird/commit/7fc971af24e166aa69ae5799988fc31062c5c8a2
[c:b3caf05]: https://github.com/serenity-rs/songbird/commit/b3caf05fd67d0b1e1a3c3275e7c14d853e81772e
[c:fc94ddb]: https://github.com/serenity-rs/songbird/commit/fc94ddba9135ea1d3b50d929dd50ce09870b4cc1
[c:d303e0a]: https://github.com/serenity-rs/songbird/commit/d303e0a3be9aa4f9ac782add06abb8cdc9c86fc3
[c:eb22443]: https://github.com/serenity-rs/songbird/commit/eb2244327f1171dd6941f9ee977edae2ec3b2a5a
[c:a1ba760]: https://github.com/serenity-rs/songbird/commit/a1ba760b6c773e37277da44e73e784dbb624003d
[c:63d53b2]: https://github.com/serenity-rs/songbird/commit/63d53b20bd8ea9d69a6288ebbc1904d39bba2225
[c:a86898c]: https://github.com/serenity-rs/songbird/commit/a86898cf857e71cd2f0ca236399a97b66d28900e
[c:3c7f86d]: https://github.com/serenity-rs/songbird/commit/3c7f86dda61c5004b3f178ef636fac81d7938d3f
[c:e7af0ff]: https://github.com/serenity-rs/songbird/commit/e7af0ff6da8fa263ce91fbee03a38c278cd9a412
[c:1eed9dd]: https://github.com/serenity-rs/songbird/commit/1eed9dddd5c738f4d85cc4ee66b952dc03d4df91
[c:bc952d0]: https://github.com/serenity-rs/songbird/commit/bc952d007916340423647b91e597acdff241bc08
[c:cd2ade9]: https://github.com/serenity-rs/songbird/commit/cd2ade96a3331d7beece8ef489372d7077b9fe03
[c:21b8383]: https://github.com/serenity-rs/songbird/commit/21b8383ceee9cd2568b18fd171fbfa66a9e21af9
[c:95dd19e]: https://github.com/serenity-rs/songbird/commit/95dd19e15f4992271539d6f6157b7c366863ad22
[c:22214a0]: https://github.com/serenity-rs/songbird/commit/22214a0f891946f42f7c23d7de3a1f380791e51d
[c:a88b185]: https://github.com/serenity-rs/songbird/commit/a88b18567619e62c073560b5acd18aa4f1c30199
[c:24d8da6]: https://github.com/serenity-rs/songbird/commit/24d8da69c0a38dc9ea9f679e1d40ffd3bc27f5b7
[c:db79940]: https://github.com/serenity-rs/songbird/commit/db7994087a23cf7210dc5ccd1e114618ce8c64ce
[c:1fcc8c0]: https://github.com/serenity-rs/songbird/commit/1fcc8c0eb9d07e427fd057697a3dfa6b0f89ab6b
[c:f230b41]: https://github.com/serenity-rs/songbird/commit/f230b41110e34dc8b46b19a118186f9e90e15dd2
[c:a3f86ad]: https://github.com/serenity-rs/songbird/commit/a3f86ad34db174b9e0da9529fed1cca8c1dda85b
[c:a9b4cb7]: https://github.com/serenity-rs/songbird/commit/a9b4cb7715f104dbc7aedb9859d6553914f32879
[c:c488ce3]: https://github.com/serenity-rs/songbird/commit/c488ce3dc907dd0c8ee1dd20fd07a7e83ab3466b
[c:b9a926c]: https://github.com/serenity-rs/songbird/commit/b9a926c1254b44d450f00eb161139fdd6f6bbbd1
[c:f3f5242]: https://github.com/serenity-rs/songbird/commit/f3f52427eaab6fff9f1138eb0bb0185d8efb38b7
[c:0e860dc]: https://github.com/serenity-rs/songbird/commit/0e860dc29d2c412c50aae306f9bf89cea9b507e4
[c:a40fac3]: https://github.com/serenity-rs/songbird/commit/a40fac310951a0440e654781f9b148ee6c037b3d
[c:1863d39]: https://github.com/serenity-rs/songbird/commit/1863d39356b2d2c21e0ce60907616b43c4041b67
[c:aaab975]: https://github.com/serenity-rs/songbird/commit/aaab97511dcf581fb0360adce8f6dc9963341852
[c:b245309]: https://github.com/serenity-rs/songbird/commit/b2453091e726772802b216a477841b816a137718
[c:a0e905a]: https://github.com/serenity-rs/songbird/commit/a0e905a83fc83b6eb0b8fa26340572cd15eefc35
[c:196d5be]: https://github.com/serenity-rs/songbird/commit/196d5be3d24032e671a93ff1611fb0164b20f5da
[c:fe2282c]: https://github.com/serenity-rs/songbird/commit/fe2282cfde6033a869b78fa4689926258bd6d180
[c:658fd83]: https://github.com/serenity-rs/songbird/commit/658fd830c15a5751c57290ee858eea7a92f20ae5
[c:868c44c]: https://github.com/serenity-rs/songbird/commit/868c44c19f1d223b05e7d38a5376d6a24ba353a4
[c:cb2398f]: https://github.com/serenity-rs/songbird/commit/cb2398f1827d7b34b381c389e6099b37ed505f82
[c:55b8e7f]: https://github.com/serenity-rs/songbird/commit/55b8e7fb4e58c2dacd2569ea75d59305cadc1196
[c:dcb6ad9]: https://github.com/serenity-rs/songbird/commit/dcb6ad97b2bff4ab7b270a6f95fe41126f9432a6
[c:7d767d2]: https://github.com/serenity-rs/songbird/commit/7d767d29196a5f1905a720bbebfec02ca1acc211
[c:53ab9da]: https://github.com/serenity-rs/songbird/commit/53ab9dac03d0824d9787b827ac829f9ccd789649
[c:f05b741]: https://github.com/serenity-rs/songbird/commit/f05b7414a0ec52404019dce9530b380d71e41f3b
[c:d42e09f]: https://github.com/serenity-rs/songbird/commit/d42e09f72b825ca45ba3e08cf0614eef9acecca1
[c:873458d]: https://github.com/serenity-rs/songbird/commit/873458d28872d9b4106c78938b1ba698bd55f93c
[c:03ae0e7]: https://github.com/serenity-rs/songbird/commit/03ae0e7628efd68038ac76c9110e9e8aad99b7c0
[c:c0d3cb3]: https://github.com/serenity-rs/songbird/commit/c0d3cb31130ebeece6acb1b68cf366a57196d244
[c:504b8df]: https://github.com/serenity-rs/songbird/commit/504b8dfaefb71770f9b5c8cb6d0b1d6e0881f085
[c:2fc88a6]: https://github.com/serenity-rs/songbird/commit/2fc88a6ef1f950c17a076dd6e6c2f85f99607962
[c:8d6bd4f]: https://github.com/serenity-rs/songbird/commit/8d6bd4fd637be2c50403062f6f7e462b36647687
[c:700f20d]: https://github.com/serenity-rs/songbird/commit/700f20dff9211e81f170df115433bafe113639f0
[c:f222ce9]: https://github.com/serenity-rs/songbird/commit/f222ce99696ab0dfa396bd6448bb4340b791625b
[c:9fdbcd7]: https://github.com/serenity-rs/songbird/commit/9fdbcd77be98be7a7ac20dd6901f934934fef6e6
[c:57df3fe]: https://github.com/serenity-rs/songbird/commit/57df3fe53a0d56da38bf1b0a0198af8904f054cf
[c:94157b1]: https://github.com/serenity-rs/songbird/commit/94157b12bcad4f770cffd182609af5e6ac7f823d
[c:1ada46d]: https://github.com/serenity-rs/songbird/commit/1ada46d24bde47b9ab3aede425f06257e6b38fb3
[c:69acea8]: https://github.com/serenity-rs/songbird/commit/69acea866465ce9745f5c4f0aeac0eb6a4d91a49
[c:2da5901]: https://github.com/serenity-rs/songbird/commit/2da5901930ea9a97a5101738acacbe4a7570ec55
[c:cb7d8cc]: https://github.com/serenity-rs/songbird/commit/cb7d8cc6180f1aa5ab284cb60204a6b2de3b6f28
[c:de65225]: https://github.com/serenity-rs/songbird/commit/de652250d8ce22ccab5ff45addf3f26230d18b05
[c:047ce03]: https://github.com/serenity-rs/songbird/commit/047ce0379a36011c3901d698caab8d67d4a8327d
[c:45b1fb1]: https://github.com/serenity-rs/songbird/commit/45b1fb13bf10f9273de83d25bdd9b650d97e549a
[c:6702520]: https://github.com/serenity-rs/songbird/commit/6702520b7c9be819fcbec4a2b4263b459596ff81
[c:a9f8d6c]: https://github.com/serenity-rs/songbird/commit/a9f8d6c93a9b003f004cddba0532653f4f374f4b
[c:35d262d]: https://github.com/serenity-rs/songbird/commit/35d262d9466a57a7e1e7ec8b6dbd745066aede31
[c:c5ce107]: https://github.com/serenity-rs/songbird/commit/c5ce107d55f74b54b400f8a5513418436cc7e4f2
[c:09da85b]: https://github.com/serenity-rs/songbird/commit/09da85bfc373689d022e263e9e72869493ebd654
[c:a778d24]: https://github.com/serenity-rs/songbird/commit/a778d2494166617a0684fdd71825031bdcefee3c
[c:4f5b767]: https://github.com/serenity-rs/songbird/commit/4f5b767dba218be4f5cda672603e143c043c5d77
[c:f5bf54a]: https://github.com/serenity-rs/songbird/commit/f5bf54a63d41474e3776a89306fcfc7bb2806712
[c:4a897a7]: https://github.com/serenity-rs/songbird/commit/4a897a7b76d28c5d8facc238e3fcf188ddddd04e
[c:ec7f5bc]: https://github.com/serenity-rs/songbird/commit/ec7f5bca2db903caa6c9b007567c63863e311224
[c:6724655]: https://github.com/serenity-rs/songbird/commit/67246553515a5f59a24eee892bbcb2b1781c822c
[c:868785b]: https://github.com/serenity-rs/songbird/commit/868785ba715bd26f690c1f6beff83bb98967f979
[c:8b7f388]: https://github.com/serenity-rs/songbird/commit/8b7f388f7bb4464cb786f9046dfd841fbe93857b
[c:26c9c91]: https://github.com/serenity-rs/songbird/commit/26c9c9117c5c71fc0a3d654ad4cef70f60beb878
[c:38a55da]: https://github.com/serenity-rs/songbird/commit/38a55da88bb61d862fa471e2d7b9a222c230f1cb
[c:7e4392a]: https://github.com/serenity-rs/songbird/commit/7e4392ae68f97311f2389fdf8835e70a25912ff3
