use log::error;

pub fn copy_to_clipboard(text: &str) {
    use wl_clipboard_rs::copy::{MimeType, Options, Source};

    let opts = Options::new();
    if let Err(e) = opts.copy(Source::Bytes(text.as_bytes().into()), MimeType::Autodetect) {
        error!("copy to clipboard failed: {e}");
    }
}
