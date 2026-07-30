use std::{path::Path, process::Stdio, time::Duration};

use log::warn;
use tokio::{process::Command, sync::OnceCell, time::timeout};

const PRINT_PATH_FLAG: &str = "--print-path";
const MARKER: &str = "__paloma_path__";
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(10);

static LOGIN_PATH: OnceCell<Option<String>> = OnceCell::const_new();

pub(crate) fn print_path_and_exit_if_requested() {
    if std::env::args().nth(1).as_deref() != Some(PRINT_PATH_FLAG) {
        return;
    }
    if let Ok(path) = std::env::var("PATH") {
        println!("{MARKER}{path}");
    }
    std::process::exit(0);
}

pub(crate) async fn shell_path() -> Option<&'static str> {
    LOGIN_PATH.get_or_init(capture).await.as_deref()
}

async fn capture() -> Option<String> {
    let shell = std::env::var("SHELL").ok()?;
    let exe = std::env::current_exe().ok()?;
    let quoted = exe.to_string_lossy().replace('\'', r"'\''");

    let args: &[&str] = match Path::new(&shell).file_name()?.to_str()? {
        "nu" => &["-e"],
        "csh" | "tcsh" => &["-i", "-c"],
        _ => &["-i", "-l", "-c"],
    };

    let output = Command::new(&shell)
        .args(args)
        .arg(format!("'{quoted}' {PRINT_PATH_FLAG}"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output();

    let Ok(Ok(output)) = timeout(CAPTURE_TIMEOUT, output).await else {
        warn!("timeout on reading PATH from {shell}");
        return None;
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let path = stdout.rsplit_once(MARKER)?.1.trim().to_owned();
    (!path.is_empty()).then_some(path)
}
