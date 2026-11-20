//! The colour index: mIRC's small palette over the editbox while the number
//! of a `^K` colour code (Ctrl+K) is typed, each colour with its number. A
//! click on a colour types its number. It never takes the focus, so typing
//! goes on in the editbox; it follows the active window's editbox only.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, COLOR_BTNFACE, ClientToScreen, DEFAULT_GUI_FONT, EndPaint, FillRect,
    GetMonitorInfoW, GetStockObject, GetSysColorBrush, HBRUSH, MONITOR_DEFAULTTONEAREST,
    MONITORINFO, MonitorFromWindow, PAINTSTRUCT, SelectObject, SetBkMode, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{EM_GETSEL, EM_POSFROMCHAR};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, GW_OWNER, GetClientRect, GetWindow,
    GetWindowRect, HWND_TOPMOST, IDC_ARROW, IsWindowVisible, LoadCursorW, MA_NOACTIVATE,
    RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOSIZE, SendMessageW,
    SetWindowPos, ShowWindow, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_PAINT, WNDCLASSEXW, WS_BORDER,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use super::colors::{
    PALETTE_COLORS, PaletteIndex, paint_palette_cells, palette_cell_at, palette_cell_rects,
};
use super::compose::{compose_edit, insert_color_number};
use super::mirc_format::{ColorCodeInProgress, ColorCodePart, color_code_in_progress};
use super::{WindowState, last_os_error, read_window_text, state_ref, wide_null};
use crate::automation::{UiColorCodePart, UiColorIndex, UiRect};

const COLOR_INDEX_CLASS_NAME: &str = "mirxColorIndex";
const CELL_WIDTH: i32 = 20;
const CELL_HEIGHT: i32 = 18;
/// Around the boxes, inside the border.
const MARGIN: i32 = 2;
/// The boxes' area: two rows of eight, 2 px apart.
const CELLS_WIDTH: i32 = 8 * CELL_WIDTH + 7 * 2;
const CELLS_HEIGHT: i32 = 2 * CELL_HEIGHT + 2;
const STYLE: u32 = WS_POPUP | WS_BORDER;
const EX_STYLE: u32 = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST;

/// Creates the frame's colour index, hidden; the frame owns it.
pub(super) unsafe fn create_color_index(frame: HWND) -> Result<HWND, String> {
    let instance = GetModuleHandleW(null());
    let class_name = wide_null(COLOR_INDEX_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        // None: `paint` fills it all.
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: null_mut(),
    };
    if RegisterClassExW(&window_class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
        return Err(last_os_error("RegisterClassExW color index"));
    }
    let mut size = RECT {
        left: 0,
        top: 0,
        right: CELLS_WIDTH + 2 * MARGIN,
        bottom: CELLS_HEIGHT + 2 * MARGIN,
    };
    AdjustWindowRectEx(&mut size, STYLE, 0, EX_STYLE);
    let popup = CreateWindowExW(
        EX_STYLE,
        class_name.as_ptr(),
        null(),
        STYLE,
        0,
        0,
        size.right - size.left,
        size.bottom - size.top,
        frame,
        null_mut(),
        instance,
        null_mut(),
    );
    if popup.is_null() {
        return Err(last_os_error("CreateWindowExW color index"));
    }
    Ok(popup)
}

/// The boxes, in the popup's client coordinates.
fn cells() -> [RECT; PALETTE_COLORS] {
    palette_cell_rects(CELLS_WIDTH, CELLS_HEIGHT).map(|cell| RECT {
        left: cell.left + MARGIN,
        top: cell.top + MARGIN,
        right: cell.right + MARGIN,
        bottom: cell.bottom + MARGIN,
    })
}

/// The colour code being typed at `edit`'s caret. A selection is not
/// typing: what is typed replaces it.
pub(super) unsafe fn color_code_at_caret(edit: HWND) -> Option<ColorCodeInProgress> {
    let (start, end) = edit_selection(edit);
    if start != end {
        return None;
    }
    let before_caret = read_window_text(edit)
        .encode_utf16()
        .take(end)
        .collect::<Vec<_>>();
    color_code_in_progress(&String::from_utf16_lossy(&before_caret))
}

/// `edit`'s selection, in UTF-16 units; the caret where they are equal.
pub(super) unsafe fn edit_selection(edit: HWND) -> (usize, usize) {
    let (mut start, mut end) = (0u32, 0u32);
    SendMessageW(
        edit,
        EM_GETSEL,
        &mut start as *mut u32 as WPARAM,
        &mut end as *mut u32 as LPARAM,
    );
    (start as usize, end as usize)
}

/// Shows the colour index over the active window's editbox while a colour
/// number is typed at its caret, by the code, and hides it otherwise.
pub(super) unsafe fn update_color_index(frame: HWND) {
    let popup = state_ref(frame).controls.color_index;
    let edit = compose_edit(frame);
    match color_code_at_caret(edit) {
        Some(code) => {
            place(popup, edit, code.code_offset);
            ShowWindow(popup, SW_SHOWNOACTIVATE);
        }
        None => {
            ShowWindow(popup, SW_HIDE);
        }
    }
}

/// Hides the colour index; for Esc, the editbox losing the focus, and
/// another window becoming the active one.
pub(super) fn hide_color_index(state: &WindowState) {
    unsafe { ShowWindow(state.controls.color_index, SW_HIDE) };
}

pub(super) unsafe fn color_index_shown(frame: HWND) -> bool {
    IsWindowVisible(state_ref(frame).controls.color_index) != 0
}

/// Puts the popup's bottom-left on the editbox's top edge, at the `^K`
/// code's x (`code_offset`): it stays put while the number is typed. A
/// code scrolled out of the editbox puts it at the nearer end, and the
/// whole popup stays on the editbox's monitor.
unsafe fn place(popup: HWND, edit: HWND, code_offset: usize) {
    // The code is before the caret, so there is a character at its offset.
    let position = SendMessageW(edit, EM_POSFROMCHAR, code_offset, 0);
    assert_ne!(position, -1, "the colour code is in the editbox");
    let mut client = RECT::default();
    GetClientRect(edit, &mut client);
    let mut anchor = POINT {
        x: i32::from(position as u16 as i16).clamp(0, client.right),
        y: 0,
    };
    ClientToScreen(edit, &mut anchor);
    let mut edit_rect = RECT::default();
    GetWindowRect(edit, &mut edit_rect);
    let mut popup_rect = RECT::default();
    GetWindowRect(popup, &mut popup_rect);
    let width = popup_rect.right - popup_rect.left;
    let height = popup_rect.bottom - popup_rect.top;
    let mut monitor = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    GetMonitorInfoW(
        MonitorFromWindow(edit, MONITOR_DEFAULTTONEAREST),
        &mut monitor,
    );
    let work = monitor.rcWork;
    SetWindowPos(
        popup,
        HWND_TOPMOST,
        anchor.x.clamp(work.left, work.right - width),
        (edit_rect.top - height).max(work.top),
        0,
        0,
        SWP_NOSIZE | SWP_NOACTIVATE,
    );
}

unsafe fn paint(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint);
    let mut client = RECT::default();
    GetClientRect(hwnd, &mut client);
    FillRect(hdc, &client, GetSysColorBrush(COLOR_BTNFACE) as HBRUSH);
    let font = SelectObject(hdc, GetStockObject(DEFAULT_GUI_FONT));
    let mode = SetBkMode(hdc, TRANSPARENT as i32);
    let first = PaletteIndex::new(0).expect("0 is a palette index");
    paint_palette_cells(hdc, &cells(), first, None, true);
    SetBkMode(hdc, mode);
    SelectObject(hdc, font);
    EndPaint(hwnd, &paint);
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // A click keeps the focus in the editbox.
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = (
                i32::from(lparam as u16 as i16),
                i32::from((lparam >> 16) as u16 as i16),
            );
            // A click between boxes picks nothing.
            if let Some(index) = palette_cell_at(&cells(), x, y) {
                insert_color_number(GetWindow(hwnd, GW_OWNER), index);
            }
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// The colour index as automation sees it, while it shows.
pub(super) unsafe fn dump(frame: HWND) -> Option<UiColorIndex> {
    let popup = state_ref(frame).controls.color_index;
    if IsWindowVisible(popup) == 0 {
        return None;
    }
    let code = color_code_at_caret(compose_edit(frame))
        .expect("the colour index shows only while a colour number is typed");
    let mut rect = RECT::default();
    GetWindowRect(popup, &mut rect);
    let ui_rect = |rect: &RECT| UiRect {
        x: rect.left,
        y: rect.top,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    };
    Some(UiColorIndex {
        part: match code.part {
            ColorCodePart::Foreground => UiColorCodePart::Foreground,
            ColorCodePart::Background => UiColorCodePart::Background,
        },
        rect: ui_rect(&rect),
        cells: cells().iter().map(ui_rect).collect(),
    })
}
