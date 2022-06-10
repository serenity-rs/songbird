use crate::constants::*;
use serde_json::Value;
use std::time::Duration;
use symphonia_core::{meta::Metadata as ContainerMetadata, probe::ProbedMetadata};

use super::Parsed;

/// Extra information about an [`Input`] which is acquired without
/// parsing the file itself (e.g., from a webpage).
///
/// You can access this via [`Input::aux_metadata`] and [`Compose::aux_metadata`].
///
/// [`Input`]: crate::input::Input
/// [`Input::aux_metadata`]: crate::input::Input::aux_metadata
/// [`Compose::aux_metadata`]: crate::input::Compose::aux_metadata
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuxMetadata {
    /// The track name of this stream.
    pub track: Option<String>,
    /// The main artist of this stream.
    pub artist: Option<String>,
    /// The date of creation of this stream.
    pub date: Option<String>,

    /// The number of audio channels in this stream.
    pub channels: Option<u8>,
    /// The YouTube channel of this stream.
    pub channel: Option<String>,
    /// The time at which the first true sample is played back.
    ///
    /// This occurs as an artefact of coder delay.
    pub start_time: Option<Duration>,
    /// The reported duration of this stream.
    pub duration: Option<Duration>,
    /// The sample rate of this stream.
    pub sample_rate: Option<u32>,
    /// The source url of this stream.
    pub source_url: Option<String>,
    /// The YouTube title of this stream.
    pub title: Option<String>,
    /// The thumbnail url of this stream.
    pub thumbnail: Option<String>,
}

impl AuxMetadata {
    /// Extract metadata and details from the output of `ffprobe`.
    pub fn from_ffprobe_json(value: &Value) -> Self {
        let format = value.as_object().and_then(|m| m.get("format"));

        let duration = format
            .and_then(|m| m.get("duration"))
            .and_then(Value::as_str)
            .and_then(|v| v.parse::<f64>().ok())
            .map(Duration::from_secs_f64);

        let start_time = format
            .and_then(|m| m.get("start_time"))
            .and_then(Value::as_str)
            .and_then(|v| v.parse::<f64>().ok().map(|t| t.max(0.0)))
            .map(Duration::from_secs_f64);

        let tags = format.and_then(|m| m.get("tags"));

        let track = tags
            .and_then(|m| m.get("title"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let artist = tags
            .and_then(|m| m.get("artist"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let date = tags
            .and_then(|m| m.get("date"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let stream = value
            .as_object()
            .and_then(|m| m.get("streams"))
            .and_then(Value::as_array)
            .and_then(|v| {
                v.iter()
                    .find(|line| line.get("codec_type").and_then(Value::as_str) == Some("audio"))
            });

        let channels = stream
            .and_then(|m| m.get("channels"))
            .and_then(Value::as_u64)
            .map(|v| v as u8);

        let sample_rate = stream
            .and_then(|m| m.get("sample_rate"))
            .and_then(Value::as_str)
            .and_then(|v| v.parse::<u64>().ok())
            .map(|v| v as u32);

        Self {
            track,
            artist,
            date,

            channels,
            start_time,
            duration,
            sample_rate,

            ..Default::default()
        }
    }

    /// Use `youtube-dl`'s JSON output for metadata for an online resource.
    pub fn from_ytdl_output(value: &Value) -> Self {
        let obj = value.as_object();

        let track = obj
            .and_then(|m| m.get("track"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let true_artist = obj
            .and_then(|m| m.get("artist"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let artist = true_artist.or_else(|| {
            obj.and_then(|m| m.get("uploader"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });

        let r_date = obj
            .and_then(|m| m.get("release_date"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let date = r_date.or_else(|| {
            obj.and_then(|m| m.get("upload_date"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });

        let channel = obj
            .and_then(|m| m.get("channel"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let duration = obj
            .and_then(|m| m.get("duration"))
            .and_then(Value::as_f64)
            .map(Duration::from_secs_f64);

        let source_url = obj
            .and_then(|m| m.get("webpage_url"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let title = obj
            .and_then(|m| m.get("title"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let thumbnail = obj
            .and_then(|m| m.get("thumbnail"))
            .and_then(Value::as_str)
            .map(str::to_string);

        Self {
            track,
            artist,
            date,

            channels: Some(2),
            channel,
            duration,
            sample_rate: Some(SAMPLE_RATE_RAW as u32),
            source_url,
            title,
            thumbnail,

            ..Default::default()
        }
    }

    /// Move all fields from a `Metadata` object into a new one.
    #[must_use]
    pub fn take(&mut self) -> Self {
        Self {
            track: self.track.take(),
            artist: self.artist.take(),
            date: self.date.take(),
            channels: self.channels.take(),
            channel: self.channel.take(),
            start_time: self.start_time.take(),
            duration: self.duration.take(),
            sample_rate: self.sample_rate.take(),
            source_url: self.source_url.take(),
            title: self.title.take(),
            thumbnail: self.thumbnail.take(),
        }
    }
}

/// In-stream information about an [`Input`] acquired by parsing an audio file.
///
/// To access this, the [`Input`] must be made live and parsed by symphonia. To do
/// this, you can:
/// * Pre-process the track in your own code using [`Input::make_playable`], and
///   then [`Input::metadata`].
/// * Use [`TrackHandle::action`] to access the track's metadata via [`View`],
///   *if the track has started or been made playable*.
///
/// You probably want to use [`AuxMetadata`] instead; this requires a live track,
/// which has higher memory use for buffers etc.
///
/// [`Input`]: crate::input::Input
/// [`Input::make_playable`]: super::Input::make_playable
/// [`Input::metadata`]: super::Input::metadata
/// [`TrackHandle::action`]: crate::tracks::TrackHandle::action
/// [`View`]: crate::tracks::View
pub struct Metadata<'a> {
    /// Metadata found while probing for the format of an [`Input`] (e.g., ID3 tags).
    ///
    /// [`Input`]: crate::input::Input
    pub probe: &'a mut ProbedMetadata,
    /// Metadata found inside the format/container of an audio stream.
    pub format: ContainerMetadata<'a>,
}

impl<'a> From<&'a mut Parsed> for Metadata<'a> {
    fn from(val: &'a mut Parsed) -> Self {
        Metadata {
            probe: &mut val.meta,
            format: val.format.metadata(),
        }
    }
}
