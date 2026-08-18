#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::ProcessGroup;

#[cfg(windows)]
mod windows;
use std::io;

use tokio::process::{Child, Command};
#[cfg(windows)]
pub(crate) use windows::ProcessGroup;

pub(crate) trait ProcessGroupHolder: Sized + Send + Sync {
    fn prepare(cmd: &mut Command);
    fn from_child(child: &Child) -> io::Result<Self>;
    /// Force-kill every process in the process group.
    fn kill(&self);
}
