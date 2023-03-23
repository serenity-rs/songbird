use super::AuxMetadata;
use crate::constants::SAMPLE_RATE_RAW;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};

#[derive(Deserialize, Serialize, Debug)]
pub struct Output {
    pub artist: Option<String>,
    pub album: Option<String>,
    pub channel: Option<String>,
    pub duration: Option<f64>,
    pub filesize: Option<u64>,
    pub http_headers: Option<HashMap<String, String>>,
    pub release_date: Option<String>,
    pub thumbnail: Option<String>,
    pub title: Option<String>,
    pub track: Option<String>,
    pub upload_date: Option<String>,
    pub uploader: Option<String>,
    pub url: String,
    pub webpage_url: Option<String>,
}

impl Output {
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
