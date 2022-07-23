/// Voice core events occur on receipt of
/// voice packets and telemetry.
///
/// Core events persist while the `action` in [`EventData`]
/// returns `None`.
///
/// ## Events from other users
/// Songbird can observe when a user *speaks for the first time* ([`SpeakingStateUpdate`]),
/// when a client leaves the session ([`ClientDisconnect`]), voice packets ([`VoicePacket`]), and
/// telemetry data ([`RtcpPacket`]). The format of voice packets is described by [`VoiceData`].
///
/// To detect when a user connects, you must correlate gateway (e.g., `VoiceStateUpdate`) events
/// from the main part of your bot.
///
/// To obtain a user's SSRC, you must use [`SpeakingStateUpdate`] events.
///
/// [`EventData`]: super::EventData
/// [`VoiceData`]: super::context::data::VoiceData
/// [`SpeakingStateUpdate`]: Self::SpeakingStateUpdate
/// [`ClientDisconnect`]: Self::ClientDisconnect
/// [`VoicePacket`]: Self::VoicePacket
/// [`RtcpPacket`]: Self::RtcpPacket
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum CoreEvent {
    /// Speaking state update, typically describing how another voice
    /// user is transmitting audio data. Clients must send at least one such
    /// packet to allow SSRC/UserID matching.
    ///
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
    /// Fires whenever a user disconnects from the same stream as the bot.
    ClientDisconnect,
    /// Fires when this driver successfully connects to a voice channel.
    DriverConnect,
    /// Fires when this driver successfully reconnects after a network error.
    DriverReconnect,
    /// Fires when this driver fails to connect to, or drops from, a voice channel.
    DriverDisconnect,
}
