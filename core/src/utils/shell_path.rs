#[cfg(unix)]
use std::{path::Path, process::Stdio, time::Duration};

#[cfg(unix)]
use log::warn;
use tokio::sync::OnceCell;
#[cfg(unix)]
use tokio::{process::Command, time::timeout};

const PRINT_PATH_FLAG: &str = "--print-path";
const MARKER: &str = "__paloma_path__";
#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(windows)]
async fn capture() -> Option<String> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    let machine = read_registry_path(
        HKEY_LOCAL_MACHINE,
        r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
    );
    let user = read_registry_path(HKEY_CURRENT_USER, r"Environment");
    match (machine, user) {
        // machine entries resolve first, mirroring how sessions are built
        (Some(machine), Some(user)) => Some(format!("{machine};{user}")),
        (machine, user) => machine.or(user),
    }
}

#[cfg(windows)]
fn read_registry_path(
    hive: windows_sys::Win32::System::Registry::HKEY,
    subkey: &str,
) -> Option<String> {
    use windows_sys::Win32::{
        Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS},
        System::Registry::{RRF_RT_REG_SZ, RegGetValueW},
    };

    let subkey: Vec<u16> = subkey.encode_utf16().chain([0]).collect();
    let value: Vec<u16> = "Path".encode_utf16().chain([0]).collect();

    // RRF_RT_REG_SZ has RegGetValueW expand REG_EXPAND_SZ entries; the
    // probed size can come up short for those, hence the retry loop.
    let mut bytes: u32 = 0;
    let status = unsafe {
        RegGetValueW(
            hive,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut bytes,
        )
    };
    if status != ERROR_SUCCESS && status != ERROR_MORE_DATA {
        return None;
    }
    loop {
        let mut buffer = vec![0u16; bytes.div_ceil(2) as usize];
        let status = unsafe {
            RegGetValueW(
                hive,
                subkey.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        match status {
            s if s == ERROR_SUCCESS => return parse_registry_string(&buffer, bytes),
            s if s == ERROR_MORE_DATA => continue,
            _ => return None,
        }
    }
}

/// read the registry buffer and make sure we clean up all trailing '\0' paddings
///
/// # Arguments
/// * `buffer` - UTF-16 registries string buffer
/// * `bytes` - actual buffer size
#[cfg(windows)]
fn parse_registry_string(buffer: &[u16], bytes: u32) -> Option<String> {
    let chars = (bytes as usize / 2).min(buffer.len());
    let data = &buffer[..chars];
    let end = data.iter().position(|&c| c == 0).unwrap_or(chars);
    let path = String::from_utf16_lossy(&data[..end]);
    let path = path.trim().to_owned();
    (!path.is_empty()).then_some(path)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn verify_single_terminal_should_be_striped() {
        let buffer = utf16("C:\\foo;C:\\bar\0");
        let bytes = (buffer.len() * 2) as u32;
        assert_eq!(
            parse_registry_string(&buffer, bytes),
            Some("C:\\foo;C:\\bar".to_owned())
        );
    }

    #[test]
    fn verify_mul_terminal_should_be_striped() {
        let buffer = utf16("C:\\foo\0\0\0");
        let bytes = (buffer.len() * 2) as u32;
        assert_eq!(
            parse_registry_string(&buffer, bytes),
            Some("C:\\foo".to_owned())
        );
    }

    #[test]
    fn verify_only_keep_value_before_terminal_bytes() {
        let buffer = utf16("C:\\foo\0junk");
        let bytes = (buffer.len() * 2) as u32;
        assert_eq!(
            parse_registry_string(&buffer, bytes),
            Some("C:\\foo".to_owned())
        );
    }

    #[test]
    fn verify_no_terminal_bytes_keep_whole() {
        let buffer = utf16("C:\\foo");
        let bytes = (buffer.len() * 2) as u32;
        assert_eq!(
            parse_registry_string(&buffer, bytes),
            Some("C:\\foo".to_owned())
        );
    }

    #[test]
    fn empty_value_on_empty_data_or_only_terminal_bytes() {
        assert_eq!(parse_registry_string(&[], 0), None);
        assert_eq!(parse_registry_string(&utf16("\0"), 2), None);
        assert_eq!(parse_registry_string(&utf16("\0\0\0"), 6), None);
        assert_eq!(parse_registry_string(&utf16("  \0"), 6), None);
    }

    #[test]
    fn verify_read_real_buffer_size_on_invalid_length() {
        let buffer = utf16("C:\\foo");
        assert_eq!(
            parse_registry_string(&buffer, 1024),
            Some("C:\\foo".to_owned())
        );
    }

    #[test]
    fn verify_only_keep_real_char_one_size_conflict() {
        let buffer = utf16("abc");
        assert_eq!(parse_registry_string(&buffer, 5), Some("ab".to_owned()));
    }
}
