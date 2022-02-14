/// Voice core events occur on receipt of
/// voice packets and telemetry.
///
/// Core events persist while the `action` in [`EventData`]
/// returns `None`.
///
/// [`EventData`]: super::EventData
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum CoreEvent {
    /// Fired on receipt of a speaking state update from another host.
    ///
    /// Note: this will fire when a user starts speaking for the first time,
    /// or changes their capabilities.
    SpeakingStateUpdate,
    /// Fires when a source starts speaking, or stops speaking
    /// (*i.e.*, 5 consecutive silent frames).
    SpeakingUpdate,
    /// Fires on receipt of a voice packet from another stream in the voice call.
    ///
    /// As RTP packets do not map to Discord's notion of users, SSRCs must be mapped
    /// back using the user IDs seen through client connection, disconnection,
    /// or speaking state update.
    VoicePacket,
    /// Fires on receipt of an RTCP packet, containing various call stats
    /// such as latency reports.
    RtcpPacket,
    #[deprecated(
        since = "0.2.2",
        note = "ClientConnect events are no longer sent by Discord. Please use SpeakingStateUpdate or Discord gateway events."
    )]
    /// Formerly fired whenever a client connects to a call for the first time, allowing SSRC/UserID
    /// matching. This event no longer fires.
    ///
    /// To detect when a user connects, you must correlate gateway (e.g., VoiceStateUpdate) events
    /// from the main part of your bot.
    ///
    /// To obtain a user's SSRC, you must use [`SpeakingStateUpdate`] events.
    ///
    /// [`SpeakingStateUpdate`]: Self::SpeakingStateUpdate
    ClientConnect,
    /// Fires whenever a user disconnects from the same stream as the bot.
    ClientDisconnect,
    /// Fires when this driver successfully connects to a voice channel.
    DriverConnect,
    /// Fires when this driver successfully reconnects after a network error.
    DriverReconnect,
    #[deprecated(
        since = "0.2.0",
        note = "Please use the DriverDisconnect event instead."
    )]
    /// Fires when this driver fails to connect to a voice channel.
    DriverConnectFailed,
    #[deprecated(
        since = "0.2.0",
        note = "Please use the DriverDisconnect event instead."
    )]
    /// Fires when this driver fails to reconnect to a voice channel after a network error.
    ///
    /// Users will need to manually reconnect on receipt of this error.
    DriverReconnectFailed,
    /// Fires when this driver fails to connect to, or drops from, a voice channel.
    DriverDisconnect,
    /// Fires whenever the driver is assigned a new [RTP SSRC] by the voice server.
    ///
    /// This typically fires alongside a [DriverConnect], or a full [DriverReconnect].
    ///
    /// [RTP SSRC]: https://tools.ietf.org/html/rfc3550#section-3
    /// [DriverConnect]: Self::DriverConnect
    /// [DriverReconnect]: Self::DriverReconnect
    #[deprecated(
        since = "0.2.0",
        note = "Please use the DriverConnect/Reconnect events instead."
    )]
    SsrcKnown,
}
