use symphonia_core::formats::SeekTo;

// SeekTo lacks Copy and Clone... somehow.
pub fn copy_seek_to(pos: &SeekTo) -> SeekTo {
    match *pos {
        SeekTo::Time { time, track_id } => SeekTo::Time { time, track_id },
        SeekTo::TimeStamp { ts, track_id } => SeekTo::TimeStamp { ts, track_id },
    }
}
