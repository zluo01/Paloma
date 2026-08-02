#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{parse_commands, try_parse_shell};
#[cfg(windows)]
pub use windows::parse_commands;
