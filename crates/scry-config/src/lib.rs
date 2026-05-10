use log::warn;
use std::path::PathBuf;

const APP_DIR: &str = "scry";

pub fn config_dir() -> PathBuf {
    home_dir().join(".config").join(APP_DIR)
}

pub fn database_path() -> PathBuf {
    config_dir().join("scry.db")
}

pub fn socket_path() -> PathBuf {
    runtime_dir().join("scry.sock")
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let user = std::env::var("USER").unwrap_or_else(|_| "unknown".into());
            warn!("XDG_RUNTIME_DIR is unset; falling back to /tmp/scry-{user}");
            PathBuf::from(format!("/tmp/scry-{user}"))
        })
        .join(APP_DIR)
}

fn home_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("HOME is unset; cannot resolve Scry config paths");
    if !home.is_absolute() {
        panic!("HOME is set but not absolute; cannot resolve Scry config paths");
    }
    home
}
