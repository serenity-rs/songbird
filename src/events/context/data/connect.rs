/// Voice connection details gathered at setup/reinstantiation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub struct ConnectData<'a> {
    /// The domain name of Discord's voice/TURN server.
    ///
    /// With the introduction of Discord's automatic voice server selection,
    /// this is no longer guaranteed to match a server's settings. This field
    /// may be useful if you need/wish to move your voice connection to a node/shard
    /// closer to Discord.
    pub server: &'a str,
    /// The [RTP SSRC] *("Synchronisation source")* assigned by the voice server
    /// for the duration of this call.
    ///
    /// All packets sent will use this SSRC, which is not related to the sender's User
    /// ID. These are usually allocated sequentially by Discord, following on from
    /// a random starting SSRC.
    ///
    /// [RTP SSRC]: https://tools.ietf.org/html/rfc3550#section-3
    pub ssrc: u32,
}
