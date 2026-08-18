use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::UI::Shell::{SHFILEOPSTRUCTW, SHFileOperationW};

use super::DeleteError;

pub(super) async fn trash(paths: &[String]) -> Result<Vec<(String, String)>, DeleteError> {
    todo!()
}
