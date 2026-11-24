//! mIRC's tray icon (Options > Display > Tray): minimizing to the tray, the
//! icon's clicks and its window popup; and mIRC's tips (Options > Display >
//! Tips), shown as the icon's balloons, which Windows 10+ shows as
//! notifications.
//!
//! Tips are paced: at most one shows every `TIP_INTERVAL_MS`, and the
//! latest line that wanted one meanwhile shows when the interval ends. A
//! burst of lines is then two balloons, not one recreated per batch (each
//! one a Windows 10 notification of its own; under Wine a balloon that
//! shows only once the updates stop).
//!
//! The shell holds the icon; mirx only asks. `sync_tray_icon` is the one
//! place that adds or removes it, from what the options, the frame and the
//! tips want. Whether the frame is in the tray is the frame's visibility:
//! nothing but `hide_to_tray` hides it, and only once the shell took the
//! icon, so the user can always get back.

use std::sync::OnceLock;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_SHIFT};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_REALTIME, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_BALLOONHIDE, NIN_BALLOONTIMEOUT,
    NIN_BALLOONUSERCLICK, NIN_SELECT, NINF_KEY, NOTIFY_ICON_MESSAGE, NOTIFYICON_VERSION_4,
    NOTIFYICONDATAW, NOTIFYICONDATAW_0, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible, RegisterWindowMessageW,
    SW_HIDE, SW_RESTORE, SW_SHOW, WM_CONTEXTMENU, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL,
};

use super::settings::TraySetting;
use super::*;

/// The icon's id among the frame's: it has one.
const TRAY_ICON_ID: u32 = 1;
/// `NIN_KEYSELECT`: the icon chosen with the keyboard (Enter or Space).
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

/// How long after a tip the next one may show: Wine's shortest balloon
/// (`BALLOON_SHOW_MIN_TIMEOUT`), twice a Windows notification's default 5
/// seconds. A timer of mirx's own: Wine never tells when a balloon ends.
const TIP_INTERVAL_MS: u32 = 10_000;

/// What mirx asked the shell for.
#[derive(Debug, Default)]
pub(super) struct TrayState {
    /// The shell holds the icon: an `NIM_ADD` succeeded, and no
    /// `NIM_DELETE` or Explorer restart has dropped it since.
    icon_added: bool,
    /// The window the tip on the icon is about, while it shows.
    tip: Option<ViewId>,
    pace: TipPace,
}

/// Whether a tip may show now.
#[derive(Debug, Default, PartialEq, Eq)]
enum TipPace {
    /// No tip showed in the last interval.
    #[default]
    Free,
    /// A tip showed less than an interval ago, and `TIP_TIMER_ID` runs:
    /// the latest line that wanted a tip since then waits for its end.
    Waiting(Option<TipCandidate>),
}

impl TipPace {
    /// A line wants a tip: returned to show now, or kept to wait (in
    /// place of an earlier one that waited).
    fn offer(&mut self, candidate: TipCandidate) -> Option<TipCandidate> {
        match self {
            Self::Free => Some(candidate),
            Self::Waiting(waiting) => {
                *waiting = Some(candidate);
                None
            }
        }
    }

    /// A tip showed: the interval starts.
    fn shown(&mut self) {
        assert_eq!(*self, Self::Free, "a tip shows only between intervals");
        *self = Self::Waiting(None);
    }

    /// The interval ended: the line that waited, to offer again.
    fn interval_ended(&mut self) -> Option<TipCandidate> {
        match std::mem::take(self) {
            Self::Waiting(waiting) => waiting,
            Self::Free => unreachable!("the interval's timer runs only after a tip"),
        }
    }
}

/// A line that may show a tip, found while the batch applies; the tip is
/// shown once the window's rows are built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TipCandidate {
    /// The window the line is in: its room's or its thread's.
    pub(super) view: ViewId,
    pub(super) activity_id: String,
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

/// The longest start of `text` that is at most `units` UTF-16 units,
/// cut at a character boundary: a surrogate pair is never split.
fn utf16_prefix(text: &str, units: usize) -> &str {
    let mut length = 0;
    for (index, ch) in text.char_indices() {
        length += ch.len_utf16();
        if length > units {
            return &text[..index];
        }
    }
    text
}

/// Copies `text` into a fixed UTF-16 field, ending in a NUL: the caller
/// cut it to fit.
fn fill_utf16<const N: usize>(field: &mut [u16; N], text: &str) {
    let units = text.encode_utf16().collect::<Vec<_>>();
    assert!(units.len() < N, "{text:?} fits the field");
    field[..units.len()].copy_from_slice(&units);
    field[units.len()] = 0;
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
            data.hIcon = app_icon(GetModuleHandleW(null()), IconSize::Small);
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
    let state = state_ref(hwnd);
    let wanted = icon_wanted(
        &state.settings.settings().tray,
        frame_in_tray(hwnd),
        state.tray.tip.is_some(),
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
/// `wParam` is where the icon is, for its popup.
pub(super) unsafe fn on_tray_icon(
    hwnd: HWND,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Result<(), String> {
    let single_click = state_ref(hwnd).settings.settings().tray.single_click;
    match (lparam & 0xffff) as u32 {
        // A double click is up, double click, up: with single click on,
        // only the ups count, or it would toggle twice.
        WM_LBUTTONUP if single_click => toggle_from_tray(hwnd),
        WM_LBUTTONDBLCLK if !single_click => toggle_from_tray(hwnd),
        NIN_KEYSELECT => toggle_from_tray(hwnd),
        NIN_BALLOONUSERCLICK => open_tip_view(hwnd),
        NIN_BALLOONTIMEOUT | NIN_BALLOONHIDE => {
            state_mut(hwnd).tray.tip = None;
            sync_tray_icon(hwnd)
        }
        // A right click, Shift+F10 or the Apps key.
        WM_CONTEXTMENU => show_tray_menu(
            hwnd,
            (wparam & 0xffff) as u16 as i16 as i32,
            ((wparam >> 16) & 0xffff) as u16 as i16 as i32,
        ),
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

/// The icon's popup at `x`, `y` on the screen; choosing a window restores
/// mirx and opens it.
unsafe fn show_tray_menu(hwnd: HWND, x: i32, y: i32) -> Result<(), String> {
    let items = tray_menu_items(state_ref(hwnd));
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return Err(last_os_error("CreatePopupMenu"));
    }
    let chosen = (|| {
        for (index, item) in items.iter().enumerate() {
            let check = if item.checked { MF_CHECKED } else { 0 };
            append_menu_text(menu, MF_STRING | check, index + 1, &item.label)?;
        }
        // Without the foreground, the menu would not close on a click
        // elsewhere; the WM_NULL after makes the next click work.
        SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, x, y, 0, hwnd, null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        Ok::<_, String>(command)
    })();
    DestroyMenu(menu);
    match chosen? {
        0 => Ok(()),
        command => choose_tray_view(hwnd, &items[command as usize - 1].view),
    }
}

/// A window chosen in the icon's popup: mirx comes back with it active.
unsafe fn choose_tray_view(hwnd: HWND, view: &ViewId) -> Result<(), String> {
    restore_from_tray(hwnd)?;
    // The popup's modal loop ran events: the window may be gone.
    if state_ref(hwnd).views.contains_key(view) {
        activate_view(hwnd, view, None)?;
    }
    Ok(())
}

/// The line of a tip: as its window shows it, without the timestamp, in
/// plain text. `None` once the line is no longer shown.
fn tip_line(state: &WindowState, candidate: &TipCandidate) -> Option<String> {
    let timestamp = TimestampSetting {
        show: false,
        format: state.settings.settings().timestamp.format.clone(),
    };
    let timestamps = TimestampStyle::new(&timestamp, state.local_time);
    let rows = match &candidate.view {
        ViewId::Room(room_id) => build_room_timeline_rows(
            &state.model,
            room_id,
            timestamps,
            None,
            state.settings.settings().chat.rank_prefix,
        ),
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => build_thread_timeline_rows(
            &state.model,
            room_id,
            thread_root_event_id,
            timestamps,
            state.settings.settings().chat.rank_prefix,
        ),
        ViewId::Status | ViewId::RoomList => unreachable!("a room's line is in its window"),
    };
    let identity = TimelineRowIdentity::activity_primary(&candidate.activity_id);
    rows.into_iter()
        .find(|row| row.identity == identity)
        .map(|row| row.logical_text)
}

/// A line wants a tip: it shows now, or when the interval since the last
/// one ends, unless a later line's tip takes its place.
pub(super) unsafe fn offer_tip(hwnd: HWND, candidate: TipCandidate) -> Result<(), String> {
    match state_mut(hwnd).tray.pace.offer(candidate) {
        Some(candidate) => show_tip(hwnd, candidate),
        None => Ok(()),
    }
}

/// `TIP_TIMER_ID`: the interval since the last tip ended. The line that
/// waited shows now, and starts the next one.
pub(super) unsafe fn on_tip_timer(hwnd: HWND) -> Result<(), String> {
    KillTimer(hwnd, TIP_TIMER_ID);
    match state_mut(hwnd).tray.pace.interval_ended() {
        Some(candidate) => show_tip(hwnd, candidate),
        None => Ok(()),
    }
}

/// Shows a tip about a line, while the user works in another application:
/// the window's name over the line, as a balloon on the icon (added for
/// it if need be). A tip shown starts the interval before the next.
unsafe fn show_tip(hwnd: HWND, candidate: TipCandidate) -> Result<(), String> {
    if mirx_is_foreground() {
        return Ok(());
    }
    let state = state_ref(hwnd);
    // The batch may have left the room, or cleared the line.
    if !view_id_is_valid(&state.model, &candidate.view) {
        return Ok(());
    }
    let Some(line) = tip_line(state, &candidate) else {
        return Ok(());
    };
    let title = view_short_title(&state.model, &candidate.view);
    // A refused icon is said in Status; there is nothing to show it on.
    if !set_tray_icon(hwnd, true)? {
        return Ok(());
    }
    let mut data = icon_data(hwnd);
    // Real-time: a tip that cannot show now is not news later.
    data.uFlags = NIF_INFO | NIF_REALTIME;
    data.dwInfoFlags = NIIF_INFO;
    let title = utf16_prefix(&title, data.szInfoTitle.len() - 1);
    let text = utf16_prefix(&line, data.szInfo.len() - 1);
    fill_utf16(&mut data.szInfoTitle, title);
    fill_utf16(&mut data.szInfo, text);
    match notify_icon(NIM_MODIFY, &data) {
        Ok(()) => {
            let state = state_mut(hwnd);
            state.harness.record_shown_tip(&candidate.view, title, text);
            state.tray.tip = Some(candidate.view);
            state.tray.pace.shown();
            if SetTimer(hwnd, TIP_TIMER_ID, TIP_INTERVAL_MS, None) == 0 {
                return Err(last_os_error("SetTimer tips"));
            }
            Ok(())
        }
        Err(error) => {
            report_tray_error(hwnd, &error)?;
            sync_tray_icon(hwnd)
        }
    }
}

/// The tip was clicked: mirx comes back with its window active.
unsafe fn open_tip_view(hwnd: HWND) -> Result<(), String> {
    let view = state_mut(hwnd).tray.tip.take();
    restore_from_tray(hwnd)?;
    // The window may have gone while the tip showed.
    if let Some(view) = view.filter(|view| view_id_is_valid(&state_ref(hwnd).model, view)) {
        activate_view(hwnd, &view, None)?;
    }
    Ok(())
}

/// The automation harness's part of this module: what it reads and does
/// here for the protocol.
#[cfg(any(test, feature = "automation"))]
pub(super) mod automation {
    use super::*;
    use crate::automation::{UiTray, UiTrayMenuItem};

    /// The automation's choice of a popup item by its label, as a click on it.
    pub(in crate::win32) unsafe fn choose_tray_menu_item(
        hwnd: HWND,
        label: &str,
    ) -> Result<(), String> {
        let view = tray_menu_items(state_ref(hwnd))
            .into_iter()
            .find(|item| item.label == label)
            .ok_or_else(|| format!("the tray popup has no item {label}"))?
            .view;
        choose_tray_view(hwnd, &view)
    }

    pub(in crate::win32) unsafe fn dump_tray(hwnd: HWND) -> UiTray {
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
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;

    fn candidate(activity_id: &str) -> TipCandidate {
        TipCandidate {
            view: ViewId::Room("!room:example".to_owned()),
            activity_id: activity_id.to_owned(),
        }
    }

    #[test]
    fn tips_are_paced_one_per_interval_with_the_latest_waiting() {
        let mut pace = TipPace::default();
        assert_eq!(pace.offer(candidate("first")), Some(candidate("first")));
        // Not shown (mirx was in front): the next line may still show.
        assert_eq!(pace.offer(candidate("first")), Some(candidate("first")));
        pace.shown();
        assert_eq!(pace.offer(candidate("second")), None);
        assert_eq!(pace.offer(candidate("third")), None);
        assert_eq!(pace.interval_ended(), Some(candidate("third")));
        assert_eq!(pace, TipPace::Free);
        // The waiting one showed: another interval, which nothing waits for.
        pace.shown();
        assert_eq!(pace.interval_ended(), None);
        assert_eq!(pace.offer(candidate("later")), Some(candidate("later")));
    }

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
    fn text_is_cut_at_a_character_boundary() {
        assert_eq!(utf16_prefix("abc", 3), "abc");
        assert_eq!(utf16_prefix("abcdef", 3), "abc");
        assert_eq!(utf16_prefix("", 3), "");
        // A surrogate pair that does not fit whole is left out.
        assert_eq!(utf16_prefix("ab\u{1F600}", 3), "ab");
        assert_eq!(utf16_prefix("a\u{1F600}", 3), "a\u{1F600}");
        assert_eq!(utf16_prefix("héllo", 2), "hé");
    }

    #[test]
    fn a_field_holds_the_text_and_a_nul() {
        let mut field = [0xffff_u16; 4];
        fill_utf16(&mut field, "a\u{1F600}");
        assert_eq!(field, [b'a' as u16, 0xd83d, 0xde00, 0]);
        fill_utf16(&mut field, "");
        assert_eq!(field[0], 0);
    }

    #[test]
    #[should_panic(expected = "fits the field")]
    fn a_field_never_cuts_the_text() {
        fill_utf16(&mut [0_u16; 4], "abcd");
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
