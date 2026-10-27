//! `YoutubeDl` track metadata.

use super::AuxMetadata;
use crate::constants::SAMPLE_RATE_RAW;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// The longest [`Output::time_until_available`] asks a caller to wait.
const MAX_AVAILABILITY_WAIT: Duration = Duration::from_secs(45);

/// Information returned by yt-dlp about a URL.
///
/// Returned by [`crate::input::YoutubeDl::query`].
#[derive(Deserialize, Serialize, Debug)]
pub struct Output {
    /// The main artist.
    pub artist: Option<String>,
    /// The album name.
    pub album: Option<String>,
    /// The channel name.
    pub channel: Option<String>,
    /// The duration of the stream in seconds.
    pub duration: Option<f64>,
    /// The size of the stream.
    pub filesize: Option<u64>,
    /// Required HTTP headers to fetch the track stream.
    pub http_headers: Option<HashMap<String, String>>,
    /// Release date of this track.
    pub release_date: Option<String>,
    /// The thumbnail URL for this track.
    pub thumbnail: Option<String>,
    /// The title of this track.
    pub title: Option<String>,
    /// The track name.
    pub track: Option<String>,
    /// The date this track was uploaded on.
    pub upload_date: Option<String>,
    /// The name of the uploader.
    pub uploader: Option<String>,
    /// The stream URL.
    pub url: String,
    /// The URL of the public-facing webpage for this track.
    pub webpage_url: Option<String>,
    /// The stream protocol.
    pub protocol: Option<String>,
    /// Unix time, in seconds, before which the site refuses the stream URL.
    ///
    /// YouTube answers `403 Forbidden` until any pre-roll ad served with the
    /// URL would have finished, or become skippable. yt-dlp works out when
    /// that is from the player response, and sleeps until then before its
    /// own downloads.
    pub available_at: Option<f64>,
}

impl Output {
    /// How long to wait before fetching [`Self::url`], if at all, measured
    /// from `now` and capped at 45 s, so a bogus or far-future `available_at`
    /// cannot stall a track indefinitely.
    #[must_use]
    pub fn time_until_available(&self, now: SystemTime) -> Option<Duration> {
        let opens =
            UNIX_EPOCH.checked_add(Duration::try_from_secs_f64(self.available_at?).ok()?)?;
        let wait = opens.duration_since(now).ok()?;

        (!wait.is_zero()).then(|| wait.min(MAX_AVAILABILITY_WAIT))
    }

    /// Requests auxiliary metadata which can be accessed without parsing the file.
    pub fn as_aux_metadata(&self) -> AuxMetadata {
        let album = self.album.clone();
        let track = self.track.clone();
        let true_artist = self.artist.as_ref();
        let artist = true_artist.or(self.uploader.as_ref()).cloned();
        let r_date = self.release_date.as_ref();
        let date = r_date.or(self.upload_date.as_ref()).cloned();
        let channel = self.channel.clone();
        let duration = self.duration.map(Duration::from_secs_f64);
        let source_url = self.webpage_url.clone();
        let title = self.title.clone();
        let thumbnail = self.thumbnail.clone();

        AuxMetadata {
            track,
            artist,
            album,
            date,

            channels: Some(2),
            channel,
            duration,
            sample_rate: Some(SAMPLE_RATE_RAW as u32),
            source_url,
            title,
            thumbnail,

            ..AuxMetadata::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(json: &str) -> Output {
        serde_json::from_str(json).unwrap()
    }

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn available_at_is_read_from_ytdlp_output() {
        let out = output(r#"{"url":"u","available_at":1793073192}"#);
        assert_eq!(out.available_at, Some(1_793_073_192.0));
        assert_eq!(output(r#"{"url":"u"}"#).available_at, None);
    }

    #[test]
    fn a_stream_not_yet_available_waits_the_difference() {
        let out = output(r#"{"url":"u","available_at":1005.5}"#);
        assert_eq!(
            out.time_until_available(at(1000)),
            Some(Duration::from_millis(5500))
        );
    }

    /// Two unskippable pre-roll ads can hold a URL for 30 to 40 s.
    #[test]
    fn two_unskippable_ads_are_waited_out_in_full() {
        let out = output(r#"{"url":"u","available_at":1040}"#);
        assert_eq!(
            out.time_until_available(at(1000)),
            Some(Duration::from_secs(40))
        );
    }

    #[test]
    fn an_available_stream_does_not_wait() {
        let now = at(1000);
        assert_eq!(
            output(r#"{"url":"u","available_at":1000}"#).time_until_available(now),
            None
        );
        assert_eq!(
            output(r#"{"url":"u","available_at":990}"#).time_until_available(now),
            None
        );
        assert_eq!(output(r#"{"url":"u"}"#).time_until_available(now), None);
    }

    #[test]
    fn the_wait_is_capped() {
        let out = output(r#"{"url":"u","available_at":4600}"#);
        assert_eq!(
            out.time_until_available(at(1000)),
            Some(Duration::from_secs(45))
        );
    }
}
