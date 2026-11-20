//! Putting text on the clipboard.

use super::*;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};

/// windows-sys files the standard clipboard formats under `System_Ole`.
const CF_UNICODETEXT: u32 = 13;

/// Puts `text` on the clipboard as Unicode text, replacing what it held.
pub(super) unsafe fn copy_to_clipboard(owner: HWND, text: &str) -> Result<(), String> {
    if OpenClipboard(owner) == 0 {
        return Err(last_os_error("OpenClipboard"));
    }
    let result = fill_open_clipboard(text);
    // Closing a clipboard this thread opened cannot fail.
    CloseClipboard();
    result
}

unsafe fn fill_open_clipboard(text: &str) -> Result<(), String> {
    if EmptyClipboard() == 0 {
        return Err(last_os_error("EmptyClipboard"));
    }
    let wide = text.encode_utf16().chain([0]).collect::<Vec<_>>();
    let memory = GlobalAlloc(GMEM_MOVEABLE, size_of_val(wide.as_slice()));
    if memory.is_null() {
        return Err(last_os_error("GlobalAlloc"));
    }
    let locked = GlobalLock(memory).cast::<u16>();
    assert!(!locked.is_null(), "a fresh moveable block locks");
    copy_nonoverlapping(wide.as_ptr(), locked, wide.len());
    GlobalUnlock(memory);
    // The clipboard owns the block once it takes it.
    if SetClipboardData(CF_UNICODETEXT, memory).is_null() {
        let error = last_os_error("SetClipboardData");
        GlobalFree(memory);
        return Err(error);
    }
    Ok(())
}
