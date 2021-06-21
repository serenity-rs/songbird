#[cfg(feature = "driver-core")]
use super::driver::{retry::Retry, CryptoMode, DecodeMode};

use std::time::Duration;

/// Configuration for drivers and calls.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Config {
    #[cfg(feature = "driver-core")]
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
    #[cfg(feature = "driver-core")]
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
    #[cfg(feature = "gateway-core")]
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
    #[cfg(feature = "driver-core")]
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
    #[cfg(feature = "driver-core")]
    /// Connection retry logic for the [`Driver`].
    ///
    /// This controls how many times the [`Driver`] should retry any connections,
    /// as well as how long to wait between attempts.
    ///
    /// [`Driver`]: crate::driver::Driver
    pub driver_retry: Retry,
    #[cfg(feature = "driver-core")]
    /// Configures the maximum amount of time to wait for an attempted voice
    /// connection to Discord.
    ///
    /// Defaults to 10 seconds. If set to `None`, connections will never time out.
    pub driver_timeout: Option<Duration>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            #[cfg(feature = "driver-core")]
            crypto_mode: CryptoMode::Normal,
            #[cfg(feature = "driver-core")]
            decode_mode: DecodeMode::Decrypt,
            #[cfg(feature = "gateway-core")]
            gateway_timeout: Some(Duration::from_secs(10)),
            #[cfg(feature = "driver-core")]
            preallocated_tracks: 1,
            #[cfg(feature = "driver-core")]
            driver_retry: Default::default(),
            #[cfg(feature = "driver-core")]
            driver_timeout: Some(Duration::from_secs(10)),
        }
    }
}

#[cfg(feature = "driver-core")]
impl Config {
    /// Sets this `Config`'s chosen cryptographic tagging scheme.
    pub fn crypto_mode(mut self, crypto_mode: CryptoMode) -> Self {
        self.crypto_mode = crypto_mode;
        self
    }

    /// Sets this `Config`'s received packet decryption/decoding behaviour.
    pub fn decode_mode(mut self, decode_mode: DecodeMode) -> Self {
        self.decode_mode = decode_mode;
        self
    }

    /// Sets this `Config`'s number of tracks to preallocate.
    pub fn preallocated_tracks(mut self, preallocated_tracks: usize) -> Self {
        self.preallocated_tracks = preallocated_tracks;
        self
    }

    /// Sets this `Config`'s timeout for establishing a voice connection.
    pub fn driver_timeout(mut self, driver_timeout: Option<Duration>) -> Self {
        self.driver_timeout = driver_timeout;
        self
    }

    /// Sets this `Config`'s voice connection retry configuration.
    pub fn driver_retry(mut self, driver_retry: Retry) -> Self {
        self.driver_retry = driver_retry;
        self
    }

    /// This is used to prevent changes which would invalidate the current session.
    pub(crate) fn make_safe(&mut self, previous: &Config, connected: bool) {
        if connected {
            self.crypto_mode = previous.crypto_mode;
        }
    }
}

#[cfg(feature = "gateway-core")]
impl Config {
    /// Sets this `Config`'s timeout for joining a voice channel.
    pub fn gateway_timeout(mut self, gateway_timeout: Option<Duration>) -> Self {
        self.gateway_timeout = gateway_timeout;
        self
    }
}
