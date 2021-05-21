# Changelog

## 0.2.0-beta.2 — 2021-05-11 — **Magpie**

*These notes will be adjusted between 0.2.0-beta.0 and the stable release.*

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

### Added

- [input] Input: Implement StdError for DcaError, input::Error (#73) ([@vilgotf]) [c:e1fc041]
- [gateway] Gateway: Add debug logging around shard handling ([@FelixMcFelix]) [c:b3caf05]
- [gateway] Gateway: Add connection timeout, add `Config` to gateway. (#51) ([@FelixMcFelix]) [c:d303e0a]

### Changed

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

- [fmt] Chore: Repair formatting. ([@FelixMcFelix]) [c:cd2ade9]
- [fmt] Chore: Fix clippy warnings (useless clones). ([@FelixMcFelix]) [c:21b8383]
- [gateway] Gateway: Fix repeat joins on same channel from stalling (#47) ([@FelixMcFelix]) [c:95dd19e]

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
[0.2.0]: https://github.com/serenity-rs/songbird/compare/v0.1.6...v0.2.0
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
[@clarity0]: https://github.com/clarity0
[@DasEtwas]: https://github.com/DasEtwas
[@DoumanAsh]: https://github.com/DoumanAsh
[@Elinvynia]: https://github.com/Elinvynia
[@Erk-]: https://github.com/Erk-
[@FelixMcFelix]: https://github.com/FelixMcFelix
[@Flat]: https://github.com/Flat
[@ForsakenHarmony]: https://github.com/ForsakenHarmony
[@ftriquet]: https://github.com/ftriquet
[@hiratara]: https://github.com/hiratara
[@indiv0]: https://github.com/indiv0
[@james7132]: https://github.com/james7132
[@JellyWX]: https://github.com/JellyWX
[@Lakelezz]: https://github.com/Lakelezz
[@LikeLakers2]: https://github.com/LikeLakers2
[@Maspenguin]: https://github.com/Maspenguin
[@mendess]: https://github.com/mendess
[@nickelc]: https://github.com/nickelc
[@nitsuga5124]: https://github.com/nitsuga5124
[@peppizza]: https://github.com/peppizza
[@perryprog]: https://github.com/perryprog
[@Prof-Bloodstone]: https://github.com/Prof-Bloodstone
[@Proximyst]: https://github.com/Proximyst
[@Roughsketch]: https://github.com/Roughsketch
[@saanuregh]: https://github.com/saanuregh
[@s0lst1ce]: https://github.com/s0lst1ce
[@Sreyas-Sreelal]: https://github.com/Sreyas-Sreelal
[@tarcieri]: https://github.com/tarcieri
[@vilgotf]: https://github.com/vilgotf
[@vivian]: https://github.com/vivian

<!-- COMMITS -->
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
