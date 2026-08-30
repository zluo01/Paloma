use tokio::process::Command;

/// make sure on Windows we do not get popup terminal windows
pub(crate) fn hide_console(cmd: &mut Command) {
    #[cfg(windows)]
    cmd.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    #[cfg(not(windows))]
    let _ = cmd;
}
