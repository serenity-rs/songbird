//! Codec registries extending Symphonia's probe and registry formats with Opus and DCA support.

use crate::input::opus::*;
use lazy_static::lazy_static;
use symphonia::{
    core::{codecs::CodecRegistry, probe::Probe},
    default::*,
};

lazy_static! {
    /// Default Symphonia CodecRegistry, including the (audiopus-backed)
    /// Opus codec.
    pub static ref CODEC_REGISTRY: CodecRegistry = {
        let mut registry = CodecRegistry::new();
        register_enabled_codecs(&mut registry);
        registry.register_all::<OpusDecoder>();
        registry
    };
}

lazy_static! {
    /// Default Symphonia Probe, including DCA format support.
    pub static ref PROBE: Probe = {
        let mut probe = Probe::default();
        probe.register_all::<crate::input::DcaReader>();
        register_enabled_formats(&mut probe);
        probe
    };
}
