//! Background system-integration services (no GTK).
//!
//! Both run forever on the tokio runtime and fan their events out over
//! broadcast channels consumed by the GTK main thread.

pub(crate) mod portal;
pub(crate) mod tray;
