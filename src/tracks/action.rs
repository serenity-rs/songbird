#![allow(missing_docs)]

use std::time::Duration;

#[derive(Copy, Clone, Default)]
pub struct Action {
    pub(crate) make_playable: bool,
    pub(crate) seek_point: Option<Duration>,
}

impl Action {
    #[must_use]
    pub fn seek(mut self, time: Duration) -> Self {
        self.seek_point = Some(time);

        self
    }

    #[must_use]
    pub fn make_playable(mut self) -> Self {
        self.make_playable = true;

        self
    }

    pub(crate) fn combine(&mut self, other: Self) {
        self.make_playable |= other.make_playable;
        if other.seek_point.is_some() {
            self.seek_point = other.seek_point;
        }
    }
}
