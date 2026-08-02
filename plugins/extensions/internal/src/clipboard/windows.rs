use std::{collections::VecDeque, sync::RwLock, thread, time::Duration};

use log::error;
use windows::core::{HSTRING, PCWSTR, w};
use windows_sys::Win32::{
    Foundation::{GlobalFree, HWND},
    System::{
        DataExchange::{
            AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData,
            IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
            RemoveClipboardFormatListener, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
    },
    UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetMessageW, HWND_MESSAGE, MSG, WM_CLIPBOARDUPDATE,
    },
};

use super::push_entry;

/// https://learn.microsoft.com/windows/win32/dataxchg/clipboard-formats
const EXCLUDE_MARKER_FORMAT: PCWSTR = w!("ExcludeClipboardContentFromMonitorProcessing");
const HISTORY_MARKER_FORMAT: PCWSTR = w!("CanIncludeInClipboardHistory");
const CF_UNICODETEXT: u32 = 13;
/// Windows clipboard is a single global lock, the best effort to acquire the lock
const OPEN_CLIPBOARD_RETRIES: usize = 5;
const OPEN_CLIPBOARD_BACKOFF: Duration = Duration::from_millis(5);

pub(super) fn watch_clipboard(history: &RwLock<VecDeque<String>>) -> std::io::Result<()> {
    unsafe {
        // create a dummy window for the clipboard format listener
        #[rustfmt::skip]
        let hwnd = CreateWindowExW(
            0, w!("STATIC").as_ptr(), std::ptr::null(), 0, 0, 0, 0, 0,
            HWND_MESSAGE, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null(),
        );
        if hwnd.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        if AddClipboardFormatListener(hwnd) == 0 {
            let err = std::io::Error::last_os_error();
            DestroyWindow(hwnd);
            return Err(err);
        }

        // GetMessageW is blocking wait, return on listener fire an event.
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, hwnd, 0, 0) > 0 {
            if msg.message == WM_CLIPBOARDUPDATE
                && let Some(text) = recordable_text(hwnd)
                && !text.trim().is_empty()
            {
                push_entry(history, text);
            }
        }

        RemoveClipboardFormatListener(hwnd);
        DestroyWindow(hwnd);
        Ok(())
    }
}

fn recordable_text(hwnd: HWND) -> Option<String> {
    unsafe {
        if !try_acquire_clipboard_lock(hwnd) {
            return None;
        }

        let mut text = None;
        if !has_private_marker() {
            let handle = GetClipboardData(CF_UNICODETEXT);
            let ptr = if handle.is_null() {
                std::ptr::null_mut()
            } else {
                GlobalLock(handle).cast::<u16>()
            };
            if !ptr.is_null() {
                // NUL-terminated by the format contract, but the data comes
                // from arbitrary programs — bound by the actual allocation.
                let max = GlobalSize(handle) / size_of::<u16>();
                let mut len = 0;
                while len < max && *ptr.add(len) != 0 {
                    len += 1;
                }
                text = Some(String::from_utf16_lossy(std::slice::from_raw_parts(
                    ptr, len,
                )));
                GlobalUnlock(handle);
            }
        }
        CloseClipboard();
        text
    }
}

/// Both exclusion markers from
/// https://learn.microsoft.com/windows/win32/dataxchg/clipboard-formats,
/// set by apps copying secrets (password managers):
/// ExcludeClipboardContentFromMonitorProcessing excludes by presence alone,
/// CanIncludeInClipboardHistory excludes when its DWORD payload is zero.
/// Failed registration or an unreadable payload counts as excluded.
fn has_private_marker() -> bool {
    unsafe {
        let exclude = RegisterClipboardFormatW(EXCLUDE_MARKER_FORMAT.as_ptr());
        if exclude == 0 || IsClipboardFormatAvailable(exclude) != 0 {
            return true;
        }

        let can_include = RegisterClipboardFormatW(HISTORY_MARKER_FORMAT.as_ptr());
        if can_include == 0 {
            return true;
        }
        if IsClipboardFormatAvailable(can_include) == 0 {
            // absent means inclusion, per the format's contract
            return false;
        }
        let handle = GetClipboardData(can_include);
        if handle.is_null() {
            return true;
        }
        let ptr = GlobalLock(handle).cast::<u32>();
        if ptr.is_null() {
            return true;
        }
        let allowed = GlobalSize(handle) >= size_of::<u32>() && *ptr != 0;
        GlobalUnlock(handle);
        !allowed
    }
}

/// follow what chromium does https://github.com/chromium/chromium/blob/main/ui/base/clipboard/clipboard_win.cc#L102-L128
fn try_acquire_clipboard_lock(hwnd: HWND) -> bool {
    unsafe {
        for _ in 0..OPEN_CLIPBOARD_RETRIES {
            if OpenClipboard(hwnd) != 0 {
                return true;
            }
            thread::sleep(OPEN_CLIPBOARD_BACKOFF);
        }
    }
    false
}

pub fn copy_to_clipboard(text: &str) {
    let utf16 = HSTRING::from(text);
    // HSTRING does not come with the terminator
    let units = utf16.len() + 1;
    unsafe {
        if !try_acquire_clipboard_lock(std::ptr::null_mut()) {
            error!("copy to clipboard failed: cannot open the clipboard");
            return;
        }
        EmptyClipboard();
        let handle = GlobalAlloc(GMEM_MOVEABLE, units * size_of::<u16>());
        if handle.is_null() {
            error!("copy to clipboard failed: allocation failed");
            CloseClipboard();
            return;
        }
        let dst = GlobalLock(handle);
        if dst.is_null() {
            error!("copy to clipboard failed: cannot lock the allocation");
            GlobalFree(handle);
            CloseClipboard();
            return;
        }
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), dst.cast(), units);
        GlobalUnlock(handle);
        // the clipboard owns the allocation once SetClipboardData succeeds
        if SetClipboardData(CF_UNICODETEXT, handle).is_null() {
            error!("copy to clipboard failed: cannot set the clipboard data");
            GlobalFree(handle);
        }
        CloseClipboard();
    }
}
