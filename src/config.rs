#[cfg(feature = "driver")]
use crate::{
    driver::{retry::Retry, CryptoMode, DecodeMode, MixMode},
    input::codecs::*,
};

#[cfg(test)]
use crate::driver::test_config::*;

#[cfg(feature = "driver")]
use symphonia::core::{codecs::CodecRegistry, probe::Probe};

use derivative::Derivative;
use std::time::Duration;

/// Configuration for drivers and calls.
#[derive(Clone, Derivative)]
#[derivative(Debug)]
#[non_exhaustive]
pub struct Config {
    #[cfg(feature = "driver")]
    /// Selected tagging mode for voice packet encryption.
    ///
    /// Defaults to [`CryptoMode::Normal`].
    ///
    /// Changes to this field will not immediately apply if the
    /// driver is actively connected, but will apply to subsequent
    /// sessions.
    ///
    /// [`CryptoMode::Normal`]: CryptoMode::Normal
    pub crypto_mode: CryptoMode,
    #[cfg(feature = "driver")]
    /// Configures whether decoding and decryption occur for all received packets.
    ///
    /// If voice receiving voice packets, generally you should choose [`DecodeMode::Decode`].
    /// [`DecodeMode::Decrypt`] is intended for users running their own selective decoding,
    /// who rely upon [user speaking events], or who need to inspect Opus packets.
    /// If you're certain you will never need any RT(C)P events, then consider [`DecodeMode::Pass`].
    ///
    /// Defaults to [`DecodeMode::Decrypt`]. This is due to per-packet decoding costs,
    /// which most users will not want to pay, but allowing speaking events which are commonly used.
    ///
    /// [`DecodeMode::Decode`]: DecodeMode::Decode
    /// [`DecodeMode::Decrypt`]: DecodeMode::Decrypt
    /// [`DecodeMode::Pass`]: DecodeMode::Pass
    /// [user speaking events]: crate::events::CoreEvent::SpeakingUpdate
    pub decode_mode: DecodeMode,
    #[cfg(feature = "gateway")]
    /// Configures the amount of time to wait for Discord to reply with connection information
    /// if [`Call::join`]/[`join_gateway`] are used.
    ///
    /// This is a useful fallback in the event that:
    ///  * the underlying Discord client restarts and loses a join request, or
    ///  * a channel join fails because the bot is already believed to be there.
    ///
    /// Defaults to 10 seconds. If set to `None`, connections will never time out.
    ///
    /// [`Call::join`]: crate::Call::join
    /// [`join_gateway`]: crate::Call::join_gateway
    pub gateway_timeout: Option<Duration>,
    #[cfg(feature = "driver")]
    /// Configures the maximum amount of time to wait for an attempted voice
    /// connection to Discord.
    ///
    /// Defaults to [`Stereo`].
    ///
    /// [`Stereo`]: MixMode::Stereo
    pub mix_mode: MixMode,
    #[cfg(feature = "driver")]
    /// Number of concurrently active tracks to allocate memory for.
    ///
    /// This should be set at, or just above, the maximum number of tracks
    /// you expect your bot will play at the same time. Exceeding the size of
    /// the internal queue will trigger a larger memory allocation and copy,
    /// possibly causing the mixer thread to miss a packet deadline.
    ///
    /// Defaults to `1`.
    ///
    /// Changes to this field in a running driver will only ever increase
    /// the capacity of the track store.
    pub preallocated_tracks: usize,
    #[cfg(feature = "driver")]
    /// Connection retry logic for the [`Driver`].
    ///
    /// This controls how many times the [`Driver`] should retry any connections,
    /// as well as how long to wait between attempts.
    ///
    /// [`Driver`]: crate::driver::Driver
    pub driver_retry: Retry,
    #[cfg(feature = "driver")]
    /// Configures the maximum amount of time to wait for an attempted voice
    /// connection to Discord.
    ///
    /// Defaults to 10 seconds. If set to `None`, connections will never time out.
    pub driver_timeout: Option<Duration>,
    #[cfg(feature = "driver")]
    #[derivative(Debug = "ignore")]
    /// Registry of the inner codecs supported by the driver, adding audiopus-based
    /// Opus codec support to all of Symphonia's default codecs.
    ///
    /// Defaults to [`CODEC_REGISTRY`].
    ///
    /// [`CODEC_REGISTRY`]: static@CODEC_REGISTRY
    pub codec_registry: &'static CodecRegistry,
    #[cfg(feature = "driver")]
    #[derivative(Debug = "ignore")]
    /// Registry of the muxers and container formats supported by the driver.
    ///
    /// Defaults to [`PROBE`], which includes all of Symphonia's default format handlers
    /// and DCA format support.
    ///
    /// [`PROBE`]: static@PROBE
    pub format_registry: &'static Probe,

    // Test only attributes
    #[cfg(feature = "driver")]
    #[cfg(test)]
    /// Test config to offer precise control over mixing tick rate/count.
    pub(crate) tick_style: TickStyle,
    #[cfg(feature = "driver")]
    #[cfg(test)]
    /// If set, skip connection and encryption steps.
    pub(crate) override_connection: Option<OutputMode>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            #[cfg(feature = "driver")]
            crypto_mode: CryptoMode::Normal,
            #[cfg(feature = "driver")]
            decode_mode: DecodeMode::Decrypt,
            #[cfg(feature = "gateway")]
            gateway_timeout: Some(Duration::from_secs(10)),
            #[cfg(feature = "driver")]
            mix_mode: MixMode::Stereo,
            #[cfg(feature = "driver")]
            preallocated_tracks: 1,
            #[cfg(feature = "driver")]
            driver_retry: Retry::default(),
            #[cfg(feature = "driver")]
            driver_timeout: Some(Duration::from_secs(10)),
            #[cfg(feature = "driver")]
            codec_registry: &CODEC_REGISTRY,
            #[cfg(feature = "driver")]
            format_registry: &PROBE,
            #[cfg(feature = "driver")]
            #[cfg(test)]
            tick_style: TickStyle::Timed,
            #[cfg(feature = "driver")]
            #[cfg(test)]
            override_connection: None,
        }
    }
}

#[cfg(feature = "driver")]
impl Config {
    /// Sets this `Config`'s chosen cryptographic tagging scheme.
    #[must_use]
    pub fn crypto_mode(mut self, crypto_mode: CryptoMode) -> Self {
        self.crypto_mode = crypto_mode;
        self
    }

    /// Sets this `Config`'s received packet decryption/decoding behaviour.
    #[must_use]
    pub fn decode_mode(mut self, decode_mode: DecodeMode) -> Self {
        self.decode_mode = decode_mode;
        self
    }

    /// Sets this `Config`'s audio mixing channel count.
    #[must_use]
    pub fn mix_mode(mut self, mix_mode: MixMode) -> Self {
        self.mix_mode = mix_mode;
        self
    }

    /// Sets this `Config`'s number of tracks to preallocate.
    #[must_use]
    pub fn preallocated_tracks(mut self, preallocated_tracks: usize) -> Self {
        self.preallocated_tracks = preallocated_tracks;
        self
    }

    /// Sets this `Config`'s timeout for establishing a voice connection.
    #[must_use]
    pub fn driver_timeout(mut self, driver_timeout: Option<Duration>) -> Self {
        self.driver_timeout = driver_timeout;
        self
    }

    /// Sets this `Config`'s voice connection retry configuration.
    #[must_use]
    pub fn driver_retry(mut self, driver_retry: Retry) -> Self {
        self.driver_retry = driver_retry;
        self
    }

    /// Sets this `Config`'s symphonia codec registry.
    #[must_use]
    pub fn codec_registry(mut self, codec_registry: &'static CODEC_REGISTRY) -> Self {
        self.codec_registry = codec_registry;
        self
    }

    /// Sets this `Config`'s symphonia format registry/probe set.
    #[must_use]
    pub fn format_registry(mut self, format_registry: &'static PROBE) -> Self {
        self.format_registry = format_registry;
        self
    }

    /// This is used to prevent changes which would invalidate the current session.
    pub(crate) fn make_safe(&mut self, previous: &Config, connected: bool) {
        if connected {
            self.crypto_mode = previous.crypto_mode;
        }
    }
}

// Test only attributes
#[cfg(all(test, feature = "driver"))]
impl Config {
    #![allow(missing_docs)]
    #[must_use]
    pub fn tick_style(mut self, tick_style: TickStyle) -> Self {
        self.tick_style = tick_style;
        self
    }

    /// Sets this `Config`'s voice connection retry configuration.
    #[must_use]
    pub fn override_connection(mut self, override_connection: Option<OutputMode>) -> Self {
        self.override_connection = override_connection;
        self
    }

    pub fn test_cfg(raw_output: bool) -> (DriverTestHandle, Config) {
        let (tick_tx, tick_rx) = flume::unbounded();

        let (conn, rx) = if raw_output {
            let (pkt_tx, pkt_rx) = flume::unbounded();

            (OutputMode::Raw(pkt_tx), OutputReceiver::Raw(pkt_rx))
        } else {
            let (rtp_tx, rtp_rx) = flume::unbounded();

            (OutputMode::Rtp(rtp_tx), OutputReceiver::Rtp(rtp_rx))
        };

        let config = Config::default()
            .tick_style(TickStyle::UntimedWithExecLimit(tick_rx))
            .override_connection(Some(conn));

        let handle = DriverTestHandle { rx, tx: tick_tx };

        (handle, config)
    }
}

#[cfg(feature = "gateway")]
impl Config {
    /// Sets this `Config`'s timeout for joining a voice channel.
    #[must_use]
    pub fn gateway_timeout(mut self, gateway_timeout: Option<Duration>) -> Self {
        self.gateway_timeout = gateway_timeout;
        self
    }
}
