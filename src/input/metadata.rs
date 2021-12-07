use crate::constants::*;
#[cfg(not(feature = "serenity"))]
use serde_json::Value;
#[cfg(feature = "serenity")]
use serenity::json::Value;
#[cfg(feature = "serenity")]
#[allow(unused_imports)]
use serenity::json::prelude::ValueAccess;
use std::time::Duration;

/// Information about an [`Input`] source.
///
/// [`Input`]: crate::input::Input
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Metadata {
    /// The track of this stream.
    pub track: Option<String>,
    /// The main artist of this stream.
    pub artist: Option<String>,
    /// The date of creation of this stream.
    pub date: Option<String>,

    /// The number of audio channels in this stream.
    ///
    /// Any number `>= 2` is treated as stereo.
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

impl Metadata {
    /// Extract metadata and details from the output of
    /// `ffprobe`.
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
            .and_then(|v| v.as_array())
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
    pub fn from_ytdl_output(value: Value) -> Self {
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
