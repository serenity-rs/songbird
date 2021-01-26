use super::*;
use std::{
    io::{BufReader, Read},
    process::Child,
};
use tracing::debug;

#[cfg(unix)]
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};

/// Handle for a child process which ensures that any subprocesses are properly closed
/// on drop.
#[derive(Debug)]
pub struct ChildContainer(Child);

pub(crate) fn child_to_reader<T>(child: Child) -> Reader {
    Reader::Pipe(BufReader::with_capacity(
        STEREO_FRAME_SIZE * mem::size_of::<T>() * CHILD_BUFFER_LEN,
        ChildContainer(child),
    ))
}

impl From<Child> for Reader {
    fn from(container: Child) -> Self {
        child_to_reader::<f32>(container)
    }
}

impl Read for ChildContainer {
    fn read(&mut self, buffer: &mut [u8]) -> IoResult<usize> {
        self.0.stdout.as_mut().unwrap().read(buffer)
    }
}

impl Drop for ChildContainer {
    fn drop(&mut self) {
        #[cfg(not(unix))]
        let attempt = self.0.kill();

        #[cfg(unix)]
        let attempt = {
            let pid = Pid::from_raw(self.0.id() as i32);
            let _ = signal::kill(pid, Signal::SIGINT);

            self.0.wait()
        };

        if let Err(e) = attempt {
            debug!("Error awaiting child process: {:?}", e);
        }
    }
}
