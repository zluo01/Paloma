//! Desktop integration services.
//!
//! Shortcut and tray run outside GTK and forward UI events through broadcast
//! channels.

mod shortcut;
mod tray;

pub(crate) use shortcut::Shortcut;
pub(crate) use tray::{TrayEvent, run as run_tray};
