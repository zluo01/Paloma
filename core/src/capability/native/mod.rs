mod app_search;
mod calculator;
mod clipboard;
mod file_search;

pub use app_search::AppSearch;
pub use calculator::Calculator;
pub use clipboard::Clipboard;
pub use file_search::FileSearch;
use log::error;
use wl_clipboard_rs::copy::{MimeType, Options, Source};

fn copy_to_clipboard(text: &str) {
    let opts = Options::new();
    if let Err(e) = opts.copy(Source::Bytes(text.as_bytes().into()), MimeType::Autodetect) {
        error!("copy to clipboard failed: {e}");
    }
}
