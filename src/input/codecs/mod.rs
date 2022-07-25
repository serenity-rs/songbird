//! Codec registries extending Symphonia's probe and registry formats with Opus and DCA support.

pub(crate) mod dca;
mod opus;
mod raw;

pub use self::{dca::DcaReader, opus::OpusDecoder, raw::*};
use once_cell::sync::Lazy;
use symphonia::{
    core::{codecs::CodecRegistry, probe::Probe},
    default::*,
};

/// Default Symphonia [`CodecRegistry`], including the (audiopus-backed) Opus codec.
pub static CODEC_REGISTRY: Lazy<CodecRegistry> = Lazy::new(|| {
    let mut registry = CodecRegistry::new();
    register_enabled_codecs(&mut registry);
    registry.register_all::<OpusDecoder>();
    registry
});

/// Default Symphonia Probe, including DCA format support.
pub static PROBE: Lazy<Probe> = Lazy::new(|| {
    let mut probe = Probe::default();
    probe.register_all::<DcaReader>();
    probe.register_all::<RawReader>();
    register_enabled_formats(&mut probe);
    probe
});
