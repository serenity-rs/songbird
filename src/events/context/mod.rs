pub mod data;
pub(crate) mod internal_data;

use super::*;
use crate::{
    model::payload::{ClientConnect, ClientDisconnect, Speaking},
    tracks::{TrackHandle, TrackState},
};
pub use data as context_data;
use data::*;
use internal_data::*;

/// Information about which tracks or data fired an event.
///
/// [`Track`] events may be local or global, and have no tracks
/// if fired on the global context via [`Driver::add_global_event`].
///
/// [`Track`]: crate::tracks::Track
/// [`Driver::add_global_event`]: crate::driver::Driver::add_global_event
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum EventContext<'a> {
    /// Track event context, passed to events created via [`TrackHandle::add_event`],
    /// [`EventStore::add_event`], or relevant global events.
    ///
    /// [`EventStore::add_event`]: EventStore::add_event
    /// [`TrackHandle::add_event`]: TrackHandle::add_event
    Track(&'a [(&'a TrackState, &'a TrackHandle)]),
    /// Speaking state update, typically describing how another voice
    /// user is transmitting audio data. Clients must send at least one such
    /// packet to allow SSRC/UserID matching.
    SpeakingStateUpdate(Speaking),
    /// Speaking state transition, describing whether a given source has started/stopped
    /// transmitting. This fires in response to a silent burst, or the first packet
    /// breaking such a burst.
    SpeakingUpdate(SpeakingUpdateData),
    /// Opus audio packet, received from another stream.
    VoicePacket(VoiceData<'a>),
    /// Telemetry/statistics packet, received from another stream.
    RtcpPacket(RtcpData<'a>),
    /// Fired whenever a client connects to a call for the first time, allowing SSRC/UserID
    /// matching.
    ClientConnect(ClientConnect),
    /// Fired whenever a client disconnects.
    ClientDisconnect(ClientDisconnect),
    /// Fires when this driver successfully connects to a voice channel.
    DriverConnect(ConnectData<'a>),
    /// Fires when this driver successfully reconnects after a network error.
    DriverReconnect(ConnectData<'a>),
    /// Fires when this driver fails to connect to a voice channel.
    DriverConnectFailed,
    /// Fires when this driver fails to reconnect to a voice channel after a network error.
    ///
    /// Users will need to manually reconnect on receipt of this error.
    DriverReconnectFailed,
    #[deprecated(
        since = "0.2.0",
        note = "Please use the DriverConnect/Reconnect events instead."
    )]
    /// Fires whenever the driver is assigned a new [RTP SSRC] by the voice server.
    ///
    /// This typically fires alongside a [DriverConnect], or a full [DriverReconnect].
    /// **This event is *deprecated* in favour of these alternatives**.
    ///
    /// [RTP SSRC]: https://tools.ietf.org/html/rfc3550#section-3
    /// [DriverConnect]: Self::DriverConnect
    /// [DriverReconnect]: Self::DriverReconnect
    // TODO: remove in 0.3.x
    SsrcKnown(u32),
}

#[derive(Clone, Debug)]
pub enum CoreContext {
    SpeakingStateUpdate(Speaking),
    SpeakingUpdate(InternalSpeakingUpdate),
    VoicePacket(InternalVoicePacket),
    RtcpPacket(InternalRtcpPacket),
    ClientConnect(ClientConnect),
    ClientDisconnect(ClientDisconnect),
    DriverConnect(InternalConnect),
    DriverReconnect(InternalConnect),
    DriverConnectFailed,
    DriverReconnectFailed,
    SsrcKnown(u32),
}

impl<'a> CoreContext {
    pub(crate) fn to_user_context(&'a self) -> EventContext<'a> {
        use CoreContext::*;

        match self {
            SpeakingStateUpdate(evt) => EventContext::SpeakingStateUpdate(*evt),
            SpeakingUpdate(evt) => EventContext::SpeakingUpdate(SpeakingUpdateData::from(evt)),
            VoicePacket(evt) => EventContext::VoicePacket(VoiceData::from(evt)),
            RtcpPacket(evt) => EventContext::RtcpPacket(RtcpData::from(evt)),
            ClientConnect(evt) => EventContext::ClientConnect(*evt),
            ClientDisconnect(evt) => EventContext::ClientDisconnect(*evt),
            DriverConnect(evt) => EventContext::DriverConnect(ConnectData::from(evt)),
            DriverReconnect(evt) => EventContext::DriverReconnect(ConnectData::from(evt)),
            DriverConnectFailed => EventContext::DriverConnectFailed,
            DriverReconnectFailed => EventContext::DriverReconnectFailed,
            #[allow(deprecated)]
            SsrcKnown(s) => EventContext::SsrcKnown(*s),
        }
    }
}

impl EventContext<'_> {
    /// Retreive the event class for an event (i.e., when matching)
    /// an event against the registered listeners.
    pub fn to_core_event(&self) -> Option<CoreEvent> {
        use EventContext::*;

        match self {
            SpeakingStateUpdate { .. } => Some(CoreEvent::SpeakingStateUpdate),
            SpeakingUpdate { .. } => Some(CoreEvent::SpeakingUpdate),
            VoicePacket { .. } => Some(CoreEvent::VoicePacket),
            RtcpPacket { .. } => Some(CoreEvent::RtcpPacket),
            ClientConnect { .. } => Some(CoreEvent::ClientConnect),
            ClientDisconnect { .. } => Some(CoreEvent::ClientDisconnect),
            DriverConnect(_) => Some(CoreEvent::DriverConnect),
            DriverReconnect(_) => Some(CoreEvent::DriverReconnect),
            DriverConnectFailed => Some(CoreEvent::DriverConnectFailed),
            DriverReconnectFailed => Some(CoreEvent::DriverReconnectFailed),
            #[allow(deprecated)]
            SsrcKnown(_) => Some(CoreEvent::SsrcKnown),
            _ => None,
        }
    }
}
