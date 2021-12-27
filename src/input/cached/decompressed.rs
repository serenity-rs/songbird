/// A wrapper around an existing [`Input`] which caches
/// the decoded and converted audio data locally in memory
/// as `f32`-format PCM WAV files.
///
/// The main purpose of this wrapper is to enable seeking on
/// incompatible sources (i.e., ffmpeg output) and to ease resource
/// consumption for commonly reused/shared tracks. [`Compressed`]
/// offers similar functionality with different
/// tradeoffs.
///
/// This is intended for use with small, repeatedly used audio
/// tracks shared between sources, and stores the sound data
/// retrieved in **uncompressed floating point** form to minimise the
/// cost of audio processing when mixing several tracks together.
/// This must be used sparingly: these cost a significant
/// *3 Mbps (375 kiB/s)*, or 131 MiB of RAM for a 6 minute song.
///
/// [`Input`]: Input
/// [`Compressed`]: super::Compressed
/// [`Restartable`]: crate::input::restartable::Restartable
#[derive(Clone, Debug)]
pub struct Decompressed {}
