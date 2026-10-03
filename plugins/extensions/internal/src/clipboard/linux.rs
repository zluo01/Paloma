use std::{
    collections::VecDeque,
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::RwLock,
};

use log::error;
use wl_clipboard_rs::copy::{MimeType, Options, Source};

use crate::clipboard::push_entry;

const WATCH_COMMAND: &str = r#"[ "$CLIPBOARD_STATE" = sensitive ] && exit 0; cat; printf '\0'"#;

pub(super) fn watch_clipboard(history: &RwLock<VecDeque<String>>) -> std::io::Result<()> {
    let mut child = Command::new("wl-paste")
        .args(["--type", "text", "--watch", "sh", "-c", WATCH_COMMAND])
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
                if let Some(text) = decode_frame(&buf) {
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

fn decode_frame(frame: &[u8]) -> Option<String> {
    let frame = frame.strip_suffix(&[0u8]).unwrap_or(frame);
    let text = std::str::from_utf8(frame).ok()?;
    (!text.trim().is_empty()).then(|| text.to_owned())
}

pub fn copy_to_clipboard(text: &str) {
    let opts = Options::new();
    if let Err(e) = opts.copy(Source::Bytes(text.as_bytes().into()), MimeType::Autodetect) {
        error!("copy to clipboard failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn run_watch_command(state: &str, input: &[u8]) -> Vec<u8> {
        let mut child = Command::new("sh")
            .args(["-c", WATCH_COMMAND])
            .env("CLIPBOARD_STATE", state)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn sh");
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(input)
            .expect("write stdin");
        child.wait_with_output().expect("wait for sh").stdout
    }

    #[test]
    fn given_a_terminated_frame_when_decoding_should_strip_the_nul_and_keep_newlines() {
        assert_eq!(
            decode_frame(b"line one\nline two\0"),
            Some("line one\nline two".into())
        );
    }

    #[test]
    fn given_a_frame_without_terminator_when_decoding_should_return_the_text() {
        assert_eq!(decode_frame(b"tail"), Some("tail".into()));
    }

    #[test]
    fn given_an_empty_or_whitespace_frame_when_decoding_should_return_none() {
        assert_eq!(decode_frame(b"\0"), None);
        assert_eq!(decode_frame(b" \r\n\t\0"), None);
    }

    #[test]
    fn given_a_binary_frame_when_decoding_should_return_none() {
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\xff\xfe\0";
        assert_eq!(decode_frame(png), None);
    }

    #[test]
    fn given_text_on_stdin_when_running_the_watch_command_should_emit_it_with_a_nul() {
        assert_eq!(
            run_watch_command("data", b"hello\nworld"),
            b"hello\nworld\0"
        );
    }

    #[test]
    fn given_a_sensitive_state_when_running_the_watch_command_should_emit_nothing() {
        assert!(run_watch_command("sensitive", b"test123").is_empty());
    }
}
