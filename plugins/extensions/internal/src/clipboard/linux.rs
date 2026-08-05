use std::{
    collections::VecDeque,
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::RwLock,
};

use log::error;
use wl_clipboard_rs::copy::{MimeType, Options, Source};

use crate::clipboard::push_entry;

pub(super) fn watch_clipboard(history: &RwLock<VecDeque<String>>) -> std::io::Result<()> {
    // wl-paste --watch runs the inner command on every clipboard change.
    // The inner command writes the new selection followed by a NUL byte so
    // we can frame entries that themselves contain newlines.
    let mut child = Command::new("wl-paste")
        .args(["--watch", "sh", "-c", "wl-paste --no-newline; printf '\\0'"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let stdout = child.stdout.take().expect("piped stdout");
    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::with_capacity(4096);

    loop {
        buf.clear();
        match reader.read_until(0u8, &mut buf) {
            Ok(0) => {
                let _ = child.wait();
                return Ok(());
            },
            Ok(_) => {
                if buf.last() == Some(&0u8) {
                    buf.pop();
                }
                let text = String::from_utf8_lossy(&buf).into_owned();
                if !text.trim().is_empty() {
                    push_entry(history, text);
                }
            },
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            },
        }
    }
}

pub fn copy_to_clipboard(text: &str) {
    let opts = Options::new();
    if let Err(e) = opts.copy(Source::Bytes(text.as_bytes().into()), MimeType::Autodetect) {
        error!("copy to clipboard failed: {e}");
    }
}
