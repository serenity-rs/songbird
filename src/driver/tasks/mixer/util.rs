use symphonia::core::formats::SeekTo;

// SeekTo lacks Copy and Clone... somehow.
pub fn copy_seek_to(pos: &SeekTo) -> SeekTo {
    match *pos {
        SeekTo::Time { time, track_id } => SeekTo::Time { time, track_id },
        SeekTo::Timestamp { ts, track_id } => SeekTo::Timestamp { ts, track_id },
    }
}

pub fn seek_to_is_zero(pos: &SeekTo) -> bool {
    match *pos {
        SeekTo::Time { time, .. } => time.is_zero(),
        SeekTo::Timestamp { ts, .. } => ts.is_zero(),
    }
}
