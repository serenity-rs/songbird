use super::{
    child_to_reader,
    error::{Error, Result},
    Codec,
    Container,
    Input,
    Metadata,
};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};
use tokio::{process::Command as TokioCommand, task};
use tracing::trace;

const YOUTUBE_DL_COMMAND: &str = if cfg!(feature = "youtube-dlc") {
    "youtube-dlc"
} else {
    "youtube-dl"
};

/// Creates a streamed audio source with `youtube-dl` and `ffmpeg`.
///
/// This source is not seek-compatible.
/// If you need looping or track seeking, then consider using
/// [`Restartable::ytdl`].
///
/// Uses `youtube-dlc` if the `"youtube-dlc"` feature is enabled.
///
/// [`Restartable::ytdl`]: crate::input::restartable::Restartable::ytdl
pub async fn ytdl(uri: &str) -> Result<Input> {
    _ytdl(uri, &[]).await
}

pub(crate) async fn _ytdl(uri: &str, pre_args: &[&str]) -> Result<Input> {
    let ytdl_args = [
        "--print-json",
        "-f",
        "webm[abr>0]/bestaudio/best",
        "-R",
        "infinite",
        "--no-playlist",
        "--ignore-config",
        "--no-warnings",
        uri,
        "-o",
        "-",
    ];

    let ffmpeg_args = [
        "-f",
        "s16le",
        "-ac",
        "2",
        "-ar",
        "48000",
        "-acodec",
        "pcm_f32le",
        "-",
    ];

    let mut youtube_dl = Command::new(YOUTUBE_DL_COMMAND)
        .args(&ytdl_args)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    // This rigmarole is required due to the inner synchronous reading context.
    let stderr = youtube_dl.stderr.take();
    let (returned_stderr, value) = task::spawn_blocking(move || {
        let mut s = stderr.unwrap();
        let out: Result<Value> = {
            let mut o_vec = vec![];
            let mut serde_read = BufReader::new(s.by_ref());
            // Newline...
            if let Ok(len) = serde_read.read_until(0xA, &mut o_vec) {
                serde_json::from_slice(&o_vec[..len]).map_err(|err| Error::Json {
                    error: err,
                    parsed_text: std::str::from_utf8(&o_vec).unwrap_or_default().to_string(),
                })
            } else {
                Result::Err(Error::Metadata)
            }
        };

        (s, out)
    })
    .await
    .map_err(|_| Error::Metadata)?;

    youtube_dl.stderr = Some(returned_stderr);

    let ffmpeg = Command::new("ffmpeg")
        .args(pre_args)
        .arg("-i")
        .arg("-")
        .args(&ffmpeg_args)
        .stdin(youtube_dl.stdout.ok_or(Error::Stdout)?)
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()?;

    let metadata = Metadata::from_ytdl_output(value?);

    trace!("ytdl metadata {:?}", metadata);

    Ok(Input::new(
        true,
        child_to_reader::<f32>(ffmpeg),
        Codec::FloatPcm,
        Container::Raw,
        Some(metadata),
    ))
}

pub(crate) async fn _ytdl_metadata(uri: &str) -> Result<Metadata> {
    // Most of these flags are likely unused, but we want identical search
    // and/or selection as the above functions.
    let ytdl_args = [
        "-j",
        "-f",
        "webm[abr>0]/bestaudio/best",
        "-R",
        "infinite",
        "--no-playlist",
        "--ignore-config",
        "--no-warnings",
        uri,
        "-o",
        "-",
    ];

    let youtube_dl_output = TokioCommand::new(YOUTUBE_DL_COMMAND)
        .args(&ytdl_args)
        .stdin(Stdio::null())
        .output()
        .await?;

    let o_vec = youtube_dl_output.stderr;

    let end = (&o_vec)
        .iter()
        .position(|el| *el == 0xA)
        .unwrap_or_else(|| o_vec.len());

    let value = serde_json::from_slice(&o_vec[..end]).map_err(|err| Error::Json {
        error: err,
        parsed_text: std::str::from_utf8(&o_vec).unwrap_or_default().to_string(),
    })?;

    let metadata = Metadata::from_ytdl_output(value);

    Ok(metadata)
}

/// Creates a streamed audio source from YouTube search results with `youtube-dl(c)`,`ffmpeg`, and `ytsearch`.
/// Takes the first video listed from the YouTube search.
///
/// This source is not seek-compatible.
/// If you need looping or track seeking, then consider using
/// [`Restartable::ytdl_search`].
///
/// Uses `youtube-dlc` if the `"youtube-dlc"` feature is enabled.
///
/// [`Restartable::ytdl_search`]: crate::input::restartable::Restartable::ytdl_search
pub async fn ytdl_search(name: &str) -> Result<Input> {
    ytdl(&format!("ytsearch1:{}", name)).await
}
