use std::io::Write;

use gtk4::glib;

pub(crate) fn init_logging() {
    let env = env_logger::Env::default().default_filter_or("info,zbus=warn,rmcp=warn");
    let mut builder = env_logger::Builder::from_env(env);
    builder.format_timestamp_millis();

    match log_file() {
        Ok(file) => {
            builder.target(env_logger::Target::Pipe(Box::new(Tee(file))));
        },
        Err(e) => {
            eprintln!("failed to open scry log file: {e}");
        },
    }

    if let Err(e) = builder.try_init() {
        eprintln!("failed to initialize logging: {e}");
    }
}

fn log_file() -> std::io::Result<std::fs::File> {
    let dir = user_state_dir().join("scry/logs");
    std::fs::create_dir_all(&dir)?;

    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(log_file_name()))
}

fn user_state_dir() -> std::path::PathBuf {
    glib::getenv("XDG_STATE_HOME")
        .filter(|path| !path.as_os_str().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| glib::home_dir().join(".local/state"))
}

fn log_file_name() -> String {
    let stamp = glib::DateTime::now_utc()
        .and_then(|now| now.format("%Y-%m-%d"))
        .map(|stamp| stamp.to_string())
        .unwrap_or_else(|e| {
            eprintln!("failed to format log timestamp: {e}");
            "unknown-date".to_string()
        });

    format!("scry-{stamp}.log")
}

struct Tee(std::fs::File);

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        self.0.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        self.0.flush()
    }
}
