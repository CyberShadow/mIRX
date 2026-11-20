//! mIRC's tray icon (Options > Display > Tray): minimizing to the tray, the
//! icon's clicks and its window popup.
//!
//! The shell holds the icon; mirx only asks. `sync_tray_icon` is the one
//! place that adds or removes it, from what the options, the frame and the
//! tips want. Whether the frame is in the tray is the frame's visibility:
//! nothing but `hide_to_tray` hides it, and only once the shell took the
//! icon, so the user can always get back.

use std::sync::OnceLock;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_SHIFT};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION, NIN_SELECT,
    NINF_KEY, NOTIFY_ICON_MESSAGE, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, NOTIFYICONDATAW_0,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IDI_APPLICATION, IsWindowVisible, LoadIconW,
    RegisterWindowMessageW, SW_HIDE, SW_RESTORE, SW_SHOW, WM_LBUTTONDBLCLK, WM_LBUTTONUP,
};

use super::settings::TraySetting;
use super::*;
use crate::automation::{UiTray, UiTrayMenuItem};

/// The icon's id among the frame's: it has one.
const TRAY_ICON_ID: u32 = 1;
/// `NIN_KEYSELECT`: the icon chosen with the keyboard (Enter or Space).
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

/// What mirx asked the shell for.
#[derive(Debug, Default)]
pub(super) struct TrayState {
    /// The shell holds the icon: an `NIM_ADD` succeeded, and no
    /// `NIM_DELETE` or Explorer restart has dropped it since.
    icon_added: bool,
}

/// Whether the icon should be in the tray: always, while the frame is
/// there, or while a tip shows on it.
fn icon_wanted(setting: &TraySetting, in_tray: bool, tip_up: bool) -> bool {
    setting.always || in_tray || tip_up
}

/// Whether minimizing the frame puts it in the tray: the option, or Shift
/// held, as in mIRC.
pub(super) fn minimize_goes_to_tray(setting: &TraySetting, shift: bool) -> bool {
    setting.minimize || shift
}

/// Explorer broadcasts this when it starts, having lost every icon.
pub(super) fn taskbar_created_message() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| {
        let name = wide_null("TaskbarCreated");
        let message = unsafe { RegisterWindowMessageW(name.as_ptr()) };
        assert_ne!(
            message, 0,
            "RegisterWindowMessageW registers TaskbarCreated"
        );
        message
    })
}

/// The frame is in the tray: hidden, behind its icon.
pub(super) unsafe fn frame_in_tray(hwnd: HWND) -> bool {
    IsWindowVisible(hwnd) == 0
}

/// Whether one of mirx's windows (the frame, a dialog, a popup) is the
/// one the user works in.
pub(super) unsafe fn mirx_is_foreground() -> bool {
    let foreground = GetForegroundWindow();
    if foreground.is_null() {
        return false;
    }
    let mut process_id = 0;
    GetWindowThreadProcessId(foreground, &mut process_id);
    process_id == GetCurrentProcessId()
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ICON_ID,
        ..NOTIFYICONDATAW::default()
    }
}

/// Copies `text` into a fixed UTF-16 field, cut at a character boundary so
/// it ends in a NUL.
pub(super) fn fill_utf16<const N: usize>(field: &mut [u16; N], text: &str) {
    let mut length = 0;
    for ch in text.chars() {
        let width = ch.len_utf16();
        if length + width >= N {
            break;
        }
        ch.encode_utf16(&mut field[length..length + width]);
        length += width;
    }
    field[length] = 0;
}

unsafe fn notify_icon(message: NOTIFY_ICON_MESSAGE, data: &NOTIFYICONDATAW) -> Result<(), String> {
    if Shell_NotifyIconW(message, data) == 0 {
        return Err(match GetLastError() {
            0 => "Shell_NotifyIconW failed".to_owned(),
            error => format!("Shell_NotifyIconW failed (error {error})"),
        });
    }
    Ok(())
}

/// Says in Status that the shell refused the icon.
pub(super) unsafe fn report_tray_error(hwnd: HWND, error: &str) -> Result<(), String> {
    log_status(
        state_mut(hwnd),
        StatusLineKind::Error,
        status_log::tray_icon_unavailable(error),
    );
    refresh_view_content(hwnd, &ViewId::Status)
}

/// Adds the icon if `wanted` and the shell does not hold it, or removes it
/// if not wanted. Returns whether the shell holds it; a refused add is said
/// in Status.
unsafe fn set_tray_icon(hwnd: HWND, wanted: bool) -> Result<bool, String> {
    let added = state_ref(hwnd).tray.icon_added;
    match (wanted, added) {
        (true, false) => {
            let mut data = icon_data(hwnd);
            data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
            data.uCallbackMessage = WM_TRAY_ICON;
            // No icon resource yet: the stock one, as the frame has.
            data.hIcon = LoadIconW(null_mut(), IDI_APPLICATION);
            fill_utf16(&mut data.szTip, FRAME_TITLE);
            if let Err(error) = notify_icon(NIM_ADD, &data) {
                report_tray_error(hwnd, &error)?;
                return Ok(false);
            }
            state_mut(hwnd).tray.icon_added = true;
            // Version 4 callbacks: the event in LOWORD(lParam), the
            // anchor in wParam.
            let mut version = icon_data(hwnd);
            version.Anonymous = NOTIFYICONDATAW_0 {
                uVersion: NOTIFYICON_VERSION_4,
            };
            notify_icon(NIM_SETVERSION, &version)?;
            Ok(true)
        }
        (false, true) => {
            // A shell that lost the icon (Explorer died) refuses this, and
            // the icon is gone either way.
            let _ = notify_icon(NIM_DELETE, &icon_data(hwnd));
            state_mut(hwnd).tray.icon_added = false;
            Ok(false)
        }
        (true, true) | (false, false) => Ok(added),
    }
}

/// Adds or removes the icon as the options, the frame and the tips want.
pub(super) unsafe fn sync_tray_icon(hwnd: HWND) -> Result<(), String> {
    let wanted = icon_wanted(
        &state_ref(hwnd).settings.settings().tray,
        frame_in_tray(hwnd),
        false,
    );
    set_tray_icon(hwnd, wanted).map(|_| ())
}

/// Hides the minimized frame behind its icon. Without an icon the frame
/// stays minimized on the taskbar: hidden, the user could not get it back.
pub(super) unsafe fn hide_to_tray(hwnd: HWND) -> Result<(), String> {
    if set_tray_icon(hwnd, true)? {
        ShowWindow(hwnd, SW_HIDE);
    }
    Ok(())
}

/// `WM_SYSCOMMAND SC_MINIMIZE` on the frame: the caption button, the system
/// menu, the taskbar button or the icon. Minimizes, then goes to the tray
/// if the option or Shift says so.
pub(super) unsafe fn minimize_frame(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Result<LRESULT, String> {
    let shift = GetKeyState(VK_SHIFT as i32) < 0;
    let to_tray = minimize_goes_to_tray(&state_ref(hwnd).settings.settings().tray, shift);
    let result = default_window_proc(hwnd, message, wparam, lparam);
    if to_tray {
        hide_to_tray(hwnd)?;
    }
    Ok(result)
}

/// Brings the frame back from the tray or the taskbar, in front.
pub(super) unsafe fn restore_from_tray(hwnd: HWND) -> Result<(), String> {
    if frame_in_tray(hwnd) {
        ShowWindow(hwnd, SW_SHOW);
    }
    if IsIconic(hwnd) != 0 {
        ShowWindow(hwnd, SW_RESTORE);
    }
    SetForegroundWindow(hwnd);
    sync_tray_icon(hwnd)
}

/// The icon's open gesture: minimizes mirx while the user works in it,
/// else restores it.
unsafe fn toggle_from_tray(hwnd: HWND) -> Result<(), String> {
    if !frame_in_tray(hwnd) && IsIconic(hwnd) == 0 && mirx_is_foreground() {
        // Through the system command, so the tray option applies.
        SendMessageW(hwnd, WM_SYSCOMMAND, SC_MINIMIZE as WPARAM, 0);
        return Ok(());
    }
    restore_from_tray(hwnd)
}

/// The shell's `WM_TRAY_ICON`: `NOTIFYICON_VERSION_4` puts the event in
/// `LOWORD(lParam)`.
pub(super) unsafe fn on_tray_icon(hwnd: HWND, lparam: LPARAM) -> Result<(), String> {
    let single_click = state_ref(hwnd).settings.settings().tray.single_click;
    match (lparam & 0xffff) as u32 {
        // A double click is up, double click, up: with single click on,
        // only the ups count, or it would toggle twice.
        WM_LBUTTONUP if single_click => toggle_from_tray(hwnd),
        WM_LBUTTONDBLCLK if !single_click => toggle_from_tray(hwnd),
        NIN_KEYSELECT => toggle_from_tray(hwnd),
        // Moves, downs and the clicks the option does not use.
        _ => Ok(()),
    }
}

/// Explorer started again. Whether it kept the icon is unknown (a restart
/// drops it): drop whatever it holds and add it afresh.
pub(super) unsafe fn on_taskbar_created(hwnd: HWND) -> Result<(), String> {
    if state_ref(hwnd).tray.icon_added {
        set_tray_icon(hwnd, false)?;
    }
    sync_tray_icon(hwnd)
}

/// The frame is going: no icon may stay behind until hovered.
pub(super) unsafe fn remove_tray_icon(hwnd: HWND) {
    set_tray_icon(hwnd, false).expect("removing the icon reports nothing");
}

/// A line of the icon's popup: a window, checked if it had new messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TrayMenuItem {
    pub(super) label: String,
    pub(super) view: ViewId,
    pub(super) checked: bool,
}

/// mIRC's tray popup: the channel and query windows in switchbar order,
/// after Status, as the treebar roots them. A window is checked when its
/// switchbar button shows new messages.
pub(super) fn tray_menu_items(state: &WindowState) -> Vec<TrayMenuItem> {
    ordered_view_ids(state)
        .into_iter()
        .filter(|view| matches!(view, ViewId::Status | ViewId::Room(_)))
        .map(|view| TrayMenuItem {
            label: view_short_title(&state.model, &view),
            checked: state
                .room_activity
                .get(&view)
                .is_some_and(|level| *level >= ViewActivityLevel::Message),
            view,
        })
        .collect()
}

pub(super) unsafe fn dump_tray(hwnd: HWND) -> UiTray {
    let state = state_ref(hwnd);
    UiTray {
        icon: state.tray.icon_added,
        hidden: frame_in_tray(hwnd),
        menu: tray_menu_items(state)
            .into_iter()
            .map(|item| UiTrayMenuItem {
                label: item.label,
                checked: item.checked,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;

    #[test]
    fn the_icon_is_wanted_always_in_the_tray_or_under_a_tip() {
        for (always, in_tray, tip_up, expected) in [
            (false, false, false, false),
            (true, false, false, true),
            (false, true, false, true),
            (false, false, true, true),
            (true, true, true, true),
        ] {
            let setting = TraySetting {
                always,
                ..TraySetting::default()
            };
            assert_eq!(
                icon_wanted(&setting, in_tray, tip_up),
                expected,
                "always={always} in_tray={in_tray} tip_up={tip_up}"
            );
        }
    }

    #[test]
    fn minimize_goes_to_the_tray_with_the_option_or_shift() {
        for (minimize, shift, expected) in [
            (false, false, false),
            (false, true, true),
            (true, false, true),
            (true, true, true),
        ] {
            let setting = TraySetting {
                minimize,
                ..TraySetting::default()
            };
            assert_eq!(minimize_goes_to_tray(&setting, shift), expected);
        }
    }

    #[test]
    fn text_is_cut_at_a_character_boundary_and_ends_in_nul() {
        let mut field = [0xffff_u16; 4];
        fill_utf16(&mut field, "ab");
        assert_eq!(field, [b'a' as u16, b'b' as u16, 0, 0xffff]);
        fill_utf16(&mut field, "abcdef");
        assert_eq!(field, [b'a' as u16, b'b' as u16, b'c' as u16, 0]);
        // A surrogate pair that does not fit whole is left out.
        fill_utf16(&mut field, "ab\u{1F600}");
        assert_eq!(field[..3], [b'a' as u16, b'b' as u16, 0]);
        fill_utf16(&mut field, "a\u{1F600}");
        assert_eq!(field, [b'a' as u16, 0xd83d, 0xde00, 0]);
    }

    #[test]
    fn the_popup_lists_status_then_room_windows_in_switchbar_order() {
        let mut state = test_state();
        state.model.rooms = vec![
            query("!alice:example", "Alice", "@alice:example"),
            room("!ops:example", "Ops", false),
            room("!quiet:example", "Quiet", false),
        ];
        state.model.room_views.insert(
            "!alice:example".to_owned(),
            RoomViewModel {
                threads: BTreeMap::from([(
                    "$root".to_owned(),
                    ThreadViewModel {
                        label: "root msg".to_owned(),
                    },
                )]),
                ..RoomViewModel::default()
            },
        );
        for (index, view_id) in [
            ViewId::Status,
            room_view_id("!alice:example"),
            thread_view_id("!alice:example", "$root"),
            room_view_id("!ops:example"),
            room_view_id("!quiet:example"),
            ViewId::RoomList,
        ]
        .into_iter()
        .enumerate()
        {
            state
                .views
                .insert(view_id, tracked_view(10 * (index as isize + 1)));
        }
        state.room_activity = BTreeMap::from([
            (room_view_id("!alice:example"), ViewActivityLevel::Highlight),
            (room_view_id("!ops:example"), ViewActivityLevel::Message),
            (room_view_id("!quiet:example"), ViewActivityLevel::Event),
        ]);

        assert_eq!(
            tray_menu_items(&state),
            [
                ("Status", ViewId::Status, false),
                ("Ops", room_view_id("!ops:example"), true),
                ("Quiet", room_view_id("!quiet:example"), false),
                ("Alice", room_view_id("!alice:example"), true),
            ]
            .map(|(label, view, checked)| TrayMenuItem {
                label: label.to_owned(),
                view,
                checked,
            })
        );
    }
}
