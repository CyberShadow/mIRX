//! mIRC's toolbar: flat icon buttons under the menubar, in groups, each
//! with its name as a tooltip. A button sends its menu item's command id,
//! so the frame's `WM_COMMAND` runs exactly what the menu item runs.
//!
//! The glyphs are the exe's icon groups `TOOLBAR_ICON_IDS` (`build/`), in an
//! image list at the frame's DPI.

use windows_sys::Win32::UI::Controls::{
    BTNS_AUTOSIZE, BTNS_BUTTON, BTNS_SEP, CCS_TOP, HIMAGELIST, TB_ENABLEBUTTON, TB_HIDEBUTTON,
    TB_ISBUTTONENABLED, TB_ISBUTTONHIDDEN, TB_SETIMAGELIST, TBSTYLE_EX_HIDECLIPPEDBUTTONS,
    TBSTYLE_TOOLTIPS,
};

use super::app_icon;

use super::*;

pub(super) const TOOLBAR_ID: u16 = 1006;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ToolbarButton {
    /// What it sends, as its menu item does.
    pub(super) command_id: usize,
    /// Its glyph's icon group.
    pub(super) icon_id: u16,
    /// Its tooltip.
    pub(super) label: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolbarEntry {
    Button(ToolbarButton),
    Separator,
}

use ToolbarEntry::Separator;

const fn button(command_id: usize, icon_id: u16, label: &'static str) -> ToolbarEntry {
    ToolbarEntry::Button(ToolbarButton {
        command_id,
        icon_id,
        label,
    })
}

/// mIRC's buttons, for what mirx has. Connect and Disconnect are one place:
/// the one that applies shows (`refresh_toolbar`).
pub(super) const TOOLBAR: &[ToolbarEntry] = &[
    button(MENU_ID_FILE_CONNECT, 101, "Connect"),
    button(MENU_ID_FILE_DISCONNECT, 102, "Disconnect"),
    button(MENU_ID_TOOLS_OPTIONS, 103, "Options"),
    Separator,
    button(MENU_ID_COMMANDS_LIST, 104, "List Rooms"),
    button(MENU_ID_COMMANDS_JOIN, 105, "Join Room"),
    Separator,
    button(MENU_ID_TOOLS_ADDRESS_BOOK, 106, "Address Book"),
    button(MENU_ID_TOOLS_COLORS, 107, "Colors"),
    Separator,
    button(TOOLBAR_ID_SEND_FILE, 108, "Send File"),
    Separator,
    button(MENU_ID_WINDOW_CASCADE, 109, "Cascade"),
    button(MENU_ID_WINDOW_TILE_HORIZONTAL, 110, "Tile Horizontal"),
    button(MENU_ID_WINDOW_TILE_VERTICAL, 111, "Tile Vertical"),
    button(MENU_ID_WINDOW_ARRANGE_ICONS, 112, "Arrange Icons"),
    Separator,
    button(MENU_ID_HELP_TOPICS, 113, "Help"),
];

fn toolbar_buttons() -> impl Iterator<Item = ToolbarButton> {
    TOOLBAR.iter().filter_map(|entry| match entry {
        ToolbarEntry::Button(button) => Some(*button),
        Separator => None,
    })
}

/// The toolbar and its image list, which the frame destroys after it.
/// `layout_controls` shows it, as the settings say, and sizes it.
pub(super) unsafe fn create_toolbar(
    frame: HWND,
    instance: *mut c_void,
) -> Result<(HWND, HIMAGELIST), String> {
    let toolbar = CreateWindowExW(
        0,
        TOOLBARCLASSNAMEW,
        null(),
        // At the frame's top, sized by itself (`TB_AUTOSIZE`), with the
        // etched line under the menubar mIRC's has.
        WS_CHILD | TBSTYLE_FLAT | TBSTYLE_TOOLTIPS | CCS_TOP as u32,
        0,
        0,
        0,
        0,
        frame,
        TOOLBAR_ID as usize as HMENU,
        instance,
        null_mut(),
    );
    if toolbar.is_null() {
        return Err(last_os_error("CreateWindowExW toolbar"));
    }
    SendMessageW(toolbar, TB_BUTTONSTRUCTSIZE, size_of::<TBBUTTON>(), 0);
    // Mixed buttons without BTNS_SHOWTEXT: a button's string is only its
    // tooltip.
    SendMessageW(
        toolbar,
        TB_SETEXTENDEDSTYLE,
        0,
        (TBSTYLE_EX_MIXEDBUTTONS | TBSTYLE_EX_HIDECLIPPEDBUTTONS) as LPARAM,
    );

    let size = Dpi::of_window(frame).px(16);
    let images = app_icon::image_list(
        instance,
        &toolbar_buttons()
            .map(|button| button.icon_id)
            .collect::<Vec<_>>(),
        size,
    )?;
    SendMessageW(toolbar, TB_SETIMAGELIST, 0, images);
    SendMessageW(toolbar, TB_SETBITMAPSIZE, 0, pack_size_lparam(size, size));

    let labels = toolbar_buttons()
        .map(|button| wide_null(button.label))
        .collect::<Vec<_>>();
    let mut image = 0;
    let buttons = TOOLBAR
        .iter()
        .map(|entry| match entry {
            ToolbarEntry::Button(button) => {
                image += 1;
                TBBUTTON {
                    iBitmap: image - 1,
                    idCommand: button.command_id as i32,
                    fsState: TBSTATE_ENABLED as u8,
                    fsStyle: (BTNS_BUTTON | BTNS_AUTOSIZE) as u8,
                    iString: labels[image as usize - 1].as_ptr() as isize,
                    ..TBBUTTON::default()
                }
            }
            Separator => TBBUTTON {
                fsStyle: BTNS_SEP as u8,
                ..TBBUTTON::default()
            },
        })
        .collect::<Vec<_>>();
    // The toolbar copies the strings.
    if SendMessageW(
        toolbar,
        TB_ADDBUTTONSW,
        buttons.len(),
        buttons.as_ptr() as LPARAM,
    ) == 0
    {
        app_icon::destroy_image_list(images);
        return Err("failed to add the toolbar's buttons".to_owned());
    }
    Ok((toolbar, images))
}

/// The connect button follows the connection, as mIRC's turns into
/// disconnect: Disconnect shows while it applies (connected, connecting, a
/// retry due), Connect otherwise, greyed if it does not apply either.
/// Only changes are sent, so refreshing an unchanged state repaints
/// nothing.
pub(super) unsafe fn refresh_toolbar(hwnd: HWND) {
    let state = state_ref(hwnd);
    let toolbar = state.controls.toolbar;
    let disconnect = frame_menu_item_enabled(state, MENU_ID_FILE_DISCONNECT);
    let connect = frame_menu_item_enabled(state, MENU_ID_FILE_CONNECT);
    for (command_id, hidden, enabled) in [
        (MENU_ID_FILE_CONNECT, disconnect, connect),
        (MENU_ID_FILE_DISCONNECT, !disconnect, true),
    ] {
        if (SendMessageW(toolbar, TB_ISBUTTONHIDDEN, command_id, 0) != 0) != hidden {
            SendMessageW(toolbar, TB_HIDEBUTTON, command_id, LPARAM::from(hidden));
        }
        if (SendMessageW(toolbar, TB_ISBUTTONENABLED, command_id, 0) != 0) != enabled {
            SendMessageW(toolbar, TB_ENABLEBUTTON, command_id, LPARAM::from(enabled));
        }
    }
}

/// The toolbar's buttons as the control holds them, separators omitted;
/// their places in the frame's client coordinates.
pub(super) unsafe fn dump_toolbar(hwnd: HWND) -> Vec<UiToolbarButton> {
    use windows_sys::Win32::UI::Controls::{TB_GETBUTTON, TB_GETBUTTONTEXTW, TBSTATE_HIDDEN};
    let toolbar = state_ref(hwnd).controls.toolbar;
    let count = SendMessageW(toolbar, TB_BUTTONCOUNT, 0, 0) as usize;
    (0..count)
        .filter_map(|index| {
            let mut button = TBBUTTON::default();
            SendMessageW(
                toolbar,
                TB_GETBUTTON,
                index,
                &mut button as *mut TBBUTTON as LPARAM,
            );
            if u32::from(button.fsStyle) & BTNS_SEP != 0 {
                return None;
            }
            let mut text = [0u16; 64];
            let len = SendMessageW(
                toolbar,
                TB_GETBUTTONTEXTW,
                button.idCommand as usize,
                text.as_mut_ptr() as LPARAM,
            );
            let mut rect = RECT::default();
            SendMessageW(
                toolbar,
                TB_GETITEMRECT,
                index,
                &mut rect as *mut RECT as LPARAM,
            );
            MapWindowPoints(toolbar, hwnd, (&mut rect as *mut RECT).cast(), 2);
            let state = u32::from(button.fsState);
            Some(UiToolbarButton {
                command_id: u32::try_from(button.idCommand).expect("command ids are positive"),
                label: String::from_utf16_lossy(&text[..len as usize]),
                hidden: state & TBSTATE_HIDDEN != 0,
                enabled: state & TBSTATE_ENABLED != 0,
                rect: UiRect {
                    x: rect.left,
                    y: rect.top,
                    width: rect.right - rect.left,
                    height: rect.bottom - rect.top,
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::app_icon::TOOLBAR_ICON_IDS;
    use super::super::test_fixtures::*;
    use super::*;

    #[test]
    fn the_buttons_have_the_glyphs_in_order_and_distinct_names() {
        assert_eq!(
            toolbar_buttons()
                .map(|button| button.icon_id)
                .collect::<Vec<_>>(),
            TOOLBAR_ICON_IDS.collect::<Vec<_>>()
        );
        let mut labels = toolbar_buttons()
            .map(|button| button.label)
            .collect::<Vec<_>>();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), toolbar_buttons().count());
    }

    #[test]
    fn every_button_sends_a_frame_command() {
        // The menubar's items, and the one toolbar-only command.
        let menu_ids = [FILE_MENU, TOOLS_MENU, WINDOW_MENU, HELP_MENU]
            .into_iter()
            .flatten()
            .filter_map(|entry| match entry {
                MenuEntry::Item { id, .. } => Some(*id),
                MenuEntry::Separator => None,
            })
            .chain([TOOLBAR_ID_SEND_FILE])
            .collect::<Vec<_>>();
        for button in toolbar_buttons() {
            assert!(
                menu_ids.contains(&button.command_id)
                    || commands_menu_item(button.command_id).is_some(),
                "{}",
                button.label
            );
        }
    }

    fn shown(hwnd: HWND, label: &str) -> (bool, bool) {
        let buttons = unsafe { dump_toolbar(hwnd) };
        let button = buttons
            .iter()
            .find(|button| button.label == label)
            .expect("the toolbar has the button");
        (!button.hidden, button.enabled)
    }

    #[test]
    fn the_connect_button_turns_into_disconnect_while_connected() {
        unsafe {
            with_test_frame(800, 600, |hwnd| {
                // Signed out.
                assert_eq!(shown(hwnd, "Connect"), (true, true));
                assert!(!shown(hwnd, "Disconnect").0);
                // Whether Disconnect shows, and Connect (shown or not)
                // applies.
                for (connection, disconnect, connect) in [
                    (ConnectionState::Syncing, true, false),
                    (ConnectionState::SigningIn, true, false),
                    (ConnectionState::WaitingForBrowser, true, false),
                    (ConnectionState::WaitingToReconnect, true, true),
                    (ConnectionState::SignedIn, false, true),
                    (ConnectionState::SignedOut, false, true),
                    (ConnectionState::LoggingOut, true, false),
                ] {
                    state_mut(hwnd).model.connection = connection;
                    refresh_status_bar(hwnd).unwrap();
                    assert_eq!(
                        shown(hwnd, "Disconnect"),
                        (disconnect, true),
                        "{connection:?}"
                    );
                    assert_eq!(
                        shown(hwnd, "Connect"),
                        (!disconnect, connect),
                        "{connection:?}"
                    );
                }
            });
        }
    }

    #[test]
    fn the_glyphs_are_at_the_frames_dpi() {
        use windows_sys::Win32::UI::Controls::ImageList_GetIconSize;
        unsafe {
            with_test_frame(800, 600, |hwnd| {
                let (mut cx, mut cy) = (0, 0);
                ImageList_GetIconSize(state_ref(hwnd).controls.toolbar_images, &mut cx, &mut cy);
                let size = Dpi::of_window(hwnd).px(16);
                assert_eq!((cx, cy), (size, size));
            });
        }
    }

    #[test]
    fn a_button_runs_its_menu_items_command() {
        unsafe {
            with_test_frame(800, 600, |hwnd| {
                let toolbar = state_ref(hwnd).controls.toolbar;
                // As a click does: BN_CLICKED from the toolbar.
                SendMessageW(hwnd, WM_COMMAND, MENU_ID_HELP_TOPICS, toolbar as LPARAM);
                // Help lists the commands in the active window, Status.
                assert!(
                    state_ref(hwnd)
                        .model
                        .status_log
                        .lines()
                        .iter()
                        .any(|line| line.text.starts_with("* Commands: /accept ")),
                );
            });
        }
    }
}
