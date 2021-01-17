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
    /// Fires whenever a user connects to the same stream as the bot.
    ClientConnect,
    /// Fires whenever a user disconnects from the same stream as the bot.
    ClientDisconnect,
    /// Fires when this driver successully connects to a voice channel.
    DriverConnect,
    /// Fires when this driver successful reconnects after a network error.
    DriverReconnect,
    /// Fires when this driver fails to connect to a voice channel.
    DriverConnectFailed,
    /// Fires when this driver fails to reconnect to a voice channel after a network error.
    ///
    /// Users will need to manually reconnect on receipt of this error.
    DriverReconnectFailed,
}
