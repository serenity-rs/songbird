//! Codec registries extending Symphonia's probe and registry formats with Opus and DCA support.

pub(crate) mod dca;
mod opus;
mod raw;

use std::sync::OnceLock;

pub use self::{dca::DcaReader, opus::OpusDecoder, raw::*};
use symphonia::{
    core::{codecs::registry::CodecRegistry, formats::probe::Probe},
    default::*,
};

/// Default Symphonia [`CodecRegistry`], including the (opus2-backed) Opus codec.
pub fn get_codec_registry() -> &'static CodecRegistry {
    static CODEC_REGISTRY: OnceLock<CodecRegistry> = OnceLock::new();
    CODEC_REGISTRY.get_or_init(|| {
        let mut registry = CodecRegistry::new();
        register_enabled_codecs(&mut registry);
        registry.register_audio_decoder::<OpusDecoder>();
        registry
    })
}

/// Default Symphonia Probe, including DCA format support.
pub fn get_probe() -> &'static Probe {
    static PROBE: OnceLock<Probe> = OnceLock::new();
    PROBE.get_or_init(|| {
        let mut probe = Probe::default();
        probe.register_format::<DcaReader<'_>>();
        probe.register_format::<RawReader<'_>>();
        register_enabled_formats(&mut probe);
        probe
    })
}
