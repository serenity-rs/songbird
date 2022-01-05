use audiopus::Channels;
use symphonia_core::audio::Layout;

use crate::constants::{MONO_FRAME_SIZE, STEREO_FRAME_SIZE};

/// Mixing behaviour for sent audio sources processed within the driver.
///
/// This has no impact on Opus packet passthrough, which will pass packets
/// irrespective of their channel count.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MixMode {
    /// Audio sources will be downmixed into a mono buffer.
    Mono,
    /// Audio sources will be mixed into into a stereo buffer, where mono sources
    /// will be duplicated into both channels.
    Stereo,
}

impl MixMode {
    pub(crate) const fn to_opus(self) -> Channels {
        use MixMode::*;
        match self {
            Mono => Channels::Mono,
            Stereo => Channels::Stereo,
        }
    }

    pub(crate) const fn sample_count_in_frame(self) -> usize {
        use MixMode::*;
        match self {
            Mono => MONO_FRAME_SIZE,
            Stereo => STEREO_FRAME_SIZE,
        }
    }

    pub(crate) const fn channels(self) -> usize {
        use MixMode::*;
        match self {
            Mono => 1,
            Stereo => 2,
        }
    }

    pub(crate) const fn symph_layout(self) -> Layout {
        use MixMode::*;
        match self {
            Mono => Layout::Mono,
            Stereo => Layout::Stereo,
        }
    }
}
