use super::{AudioStream, Metadata, MetadataError, Parsed};

use symphonia_core::{
    codecs::{CodecRegistry, Decoder, DecoderOptions},
    errors::Error as SymphError,
    formats::FormatOptions,
    io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
    probe::Probe,
};

/// An initialised audio source.
///
/// This type's variants reflect files at different stages of readiness for use by
/// symphonia. [`Parsed`] file streams are ready for playback.
///
/// [`Parsed`]: Self::Parsed
pub enum LiveInput {
    /// An unread, raw file stream.
    Raw(AudioStream<Box<dyn MediaSource>>),
    /// An unread file which has been wrapped with a large read-ahead buffer.
    Wrapped(AudioStream<MediaSourceStream>),
    /// An audio file which has had its headers parsed and decoder state built.
    Parsed(Parsed),
}

impl LiveInput {
    /// Converts this audio source into a [`Parsed`] object using the supplied format and codec
    /// registries.
    ///
    /// Where applicable, this will convert [`Raw`] -> [`Wrapped`] -> [`Parsed`], and will
    /// play the default track (or the first encountered track if this is not available) if a
    /// container holds multiple audio streams.
    ///
    /// *This is a blocking operation. Symphonia uses standard library I/O (e.g., [`Read`], [`Seek`]).
    /// If you wish to use this from an async task, you must do so within `spawn_blocking`.*
    ///
    /// [`Parsed`]: Self::Parsed
    /// [`Raw`]: Self::Raw
    /// [`Wrapped`]: Self::Wrapped
    /// [`Read`]: https://doc.rust-lang.org/std/io/trait.Read.html
    /// [`Seek`]: https://doc.rust-lang.org/std/io/trait.Seek.html
    pub fn promote(self, codecs: &CodecRegistry, probe: &Probe) -> Result<Self, SymphError> {
        let mut out = self;

        if let LiveInput::Raw(r) = out {
            // TODO: allow passing in of MSS options?
            let mss = MediaSourceStream::new(r.input, MediaSourceStreamOptions::default());
            out = LiveInput::Wrapped(AudioStream {
                input: mss,
                hint: r.hint,
            });
        }

        if let LiveInput::Wrapped(w) = out {
            let hint = w.hint.unwrap_or_default();
            let input = w.input;
            let supports_backseek = input.is_seekable();

            let probe_data = probe.format(
                &hint,
                input,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )?;
            let format = probe_data.format;
            let meta = probe_data.metadata;

            let mut default_track_id = format.default_track().map(|track| track.id);
            let mut decoder: Option<Box<dyn Decoder>> = None;

            // Awkward loop: we need BOTH a track ID, and a decoder matching that track ID.
            // Take default track (if it exists), take first track to be found otherwise.
            for track in format.tracks() {
                if default_track_id.is_some() && Some(track.id) != default_track_id {
                    continue;
                }

                let this_decoder = codecs.make(&track.codec_params, &DecoderOptions::default())?;

                decoder = Some(this_decoder);
                default_track_id = Some(track.id);

                break;
            }

            // No tracks is a playout error, a bad default track is also possible.
            // These are probably malformed? We could go best-effort, and fall back to tracks[0]
            // but drop such tracks for now.
            let track_id = default_track_id.ok_or(SymphError::DecodeError("no track found"))?;
            let decoder = decoder.ok_or(SymphError::DecodeError(
                "reported default track did not exist",
            ))?;

            let p = Parsed {
                format,
                decoder,
                track_id,
                meta,
                supports_backseek,
            };

            out = LiveInput::Parsed(p);
        }

        Ok(out)
    }

    /// Returns a reference to the data parsed from this input stream, if it has
    /// been made available via [`Self::promote`].
    #[must_use]
    pub fn parsed(&self) -> Option<&Parsed> {
        if let Self::Parsed(parsed) = self {
            Some(parsed)
        } else {
            None
        }
    }

    /// Returns a mutable reference to the data parsed from this input stream, if it
    /// has been made available via [`Self::promote`].
    pub fn parsed_mut(&mut self) -> Option<&mut Parsed> {
        if let Self::Parsed(parsed) = self {
            Some(parsed)
        } else {
            None
        }
    }

    /// Returns whether this stream's headers have been fully parsed, and so whether
    /// the track can be played or have its metadata read.
    #[must_use]
    pub fn is_playable(&self) -> bool {
        self.parsed().is_some()
    }

    /// Tries to get any information about this audio stream acquired during parsing.
    ///
    /// Only exists when this input is [`LiveInput::Parsed`].
    pub fn metadata(&mut self) -> Result<Metadata, MetadataError> {
        if let Some(parsed) = self.parsed_mut() {
            Ok(parsed.into())
        } else {
            Err(MetadataError::NotParsed)
        }
    }
}
