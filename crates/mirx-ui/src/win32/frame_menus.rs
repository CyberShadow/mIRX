//! The menubar, popup and context menus: building, refreshing and
//! dispatching their commands.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SelectedMediaAttachment {
    pub(super) room_id: String,
    pub(super) activity_id: String,
    pub(super) event_id: String,
    pub(super) filename: Option<String>,
    pub(super) source: RoomMediaSource,
}

pub(super) unsafe fn context_menu_screen_point_for_lparam(lparam: LPARAM) -> Option<(i32, i32)> {
    (lparam != -1).then_some((((lparam as i16) as i32), (((lparam >> 16) as i16) as i32)))
}

pub(super) unsafe fn default_context_point_for_window(hwnd: HWND) -> Result<(i32, i32), String> {
    let mut rect = RECT::default();
    if GetClientRect(hwnd, &mut rect) == 0 {
        return Err(last_os_error("GetClientRect"));
    }
    let mut point = POINT {
        x: (rect.right - rect.left) / 2,
        y: (rect.bottom - rect.top) / 2,
    };
    if ClientToScreen(hwnd, &mut point) == 0 {
        return Err(last_os_error("ClientToScreen"));
    }
    Ok((point.x, point.y))
}

pub(super) unsafe fn show_context_menu(
    hwnd: HWND,
    menu: HMENU,
    x: i32,
    y: i32,
    requested_command: Option<usize>,
) -> Result<Option<usize>, String> {
    if let Some(command) = requested_command {
        return Ok(Some(command));
    }
    let command = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, x, y, 0, hwnd, null());
    Ok((command != 0).then_some(command as usize))
}

pub(super) unsafe fn show_timeline_message_context_menu(
    timeline: HWND,
    lparam: LPARAM,
    requested_command: Option<usize>,
) -> Result<(), String> {
    let frame = frame_window_for(timeline);
    let (x, y) = match context_menu_screen_point_for_lparam(lparam) {
        Some((x, y)) => {
            let mut point = POINT { x, y };
            if ScreenToClient(timeline, &mut point) == 0 {
                return Err(last_os_error("ScreenToClient"));
            }
            select_timeline_event_at_client_y(timeline, point.y);
            (x, y)
        }
        None => default_context_point_for_window(timeline)?,
    };
    let menu = state_ref(frame).controls.message_menu;
    refresh_message_menu_state(frame, menu)?;
    let action_state = active_message_action_state_for_automation(frame);
    if requested_command.is_some_and(|command| {
        action_state.selected_event_id.is_none()
            || (command == MENU_ID_MESSAGE_EDIT && !action_state.edit_enabled)
            || (command == MENU_ID_MESSAGE_SAVE_ATTACHMENT && !action_state.save_attachment_enabled)
            || (command == MENU_ID_MESSAGE_GO_TO_ORIGINAL && !action_state.go_to_original_enabled)
    }) {
        return Err("message context menu action is disabled".to_owned());
    }
    if let Some(command) = show_context_menu(frame, menu, x, y, requested_command)?
        && let Some(result) = handle_menu_command(frame, command)
    {
        result?;
    }
    Ok(())
}

unsafe fn create_popup_menu() -> Result<HMENU, String> {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return Err(last_os_error("CreatePopupMenu"));
    }
    Ok(menu)
}

unsafe fn create_menu_from_entries(entries: &[MenuEntry]) -> Result<HMENU, String> {
    let menu = create_popup_menu()?;
    for entry in entries {
        match *entry {
            MenuEntry::Item { id, label } => append_menu_text(menu, MF_STRING, id, label)?,
            MenuEntry::Separator => {
                if AppendMenuW(menu, MF_SEPARATOR, 0, null()) == 0 {
                    return Err(last_os_error("AppendMenuW"));
                }
            }
        }
    }
    Ok(menu)
}

pub(super) unsafe fn create_frame_menu(hwnd: HWND) -> Result<(), String> {
    let room_menu = create_popup_menu()?;
    append_menu_text(
        room_menu,
        MF_STRING,
        MENU_ID_ROOM_ACCEPT_INVITE,
        "Accept Invite",
    )?;
    append_menu_text(
        room_menu,
        MF_STRING,
        MENU_ID_ROOM_REJECT_INVITE,
        "Reject Invite",
    )?;
    append_menu_text(room_menu, MF_STRING, MENU_ID_ROOM_PART, "Part Room")?;
    if AppendMenuW(room_menu, MF_SEPARATOR, 0, null()) == 0 {
        return Err(last_os_error("AppendMenuW"));
    }
    append_menu_text(
        room_menu,
        MF_STRING,
        MENU_ID_ROOM_CHANNEL_CENTRAL,
        "Channel &Central...",
    )?;
    let room_notifications_menu = create_popup_menu()?;
    for (_, id, label) in ROOM_NOTIFICATIONS_MENU {
        append_menu_text(room_notifications_menu, MF_STRING, id, label)?;
    }
    append_menu_text(
        room_menu,
        MF_POPUP,
        room_notifications_menu as usize,
        "&Notifications",
    )?;

    let message_menu = create_popup_menu()?;
    for (id, label) in [
        (MENU_ID_MESSAGE_REPLY, "Reply to Message"),
        (MENU_ID_MESSAGE_GO_TO_ORIGINAL, "Go to Original Message"),
        (MENU_ID_MESSAGE_EDIT, "Edit Message"),
        (MENU_ID_MESSAGE_REACT, "React to Message"),
        (MENU_ID_MESSAGE_REDACT, "Redact Message"),
        (MENU_ID_MESSAGE_SAVE_ATTACHMENT, "Save Attachment..."),
    ] {
        append_menu_text(message_menu, MF_STRING, id, label)?;
    }

    let member_menu = create_menu_from_entries(MEMBER_MENU)?;
    append_menu_text(
        member_menu,
        MF_POPUP,
        create_menu_from_entries(MEMBER_CONTROL_MENU)? as usize,
        "Control",
    )?;

    let frame_menu = CreateMenu();
    if frame_menu.is_null() {
        return Err(last_os_error("CreateMenu"));
    }
    let (mut file_menu, mut view_menu, mut tools_menu) = (null_mut(), null_mut(), null_mut());
    for (bar_menu, label) in MENU_BAR {
        let popup = match bar_menu {
            BarMenu::File => {
                file_menu = create_menu_from_entries(FILE_MENU)?;
                file_menu
            }
            BarMenu::View => {
                view_menu = create_menu_from_entries(VIEW_MENU)?;
                view_menu
            }
            BarMenu::Tools => {
                tools_menu = create_menu_from_entries(TOOLS_MENU)?;
                tools_menu
            }
            BarMenu::Commands => {
                let menu = create_popup_menu()?;
                for (index, item) in COMMANDS_MENU.iter().enumerate() {
                    append_menu_text(menu, MF_STRING, MENU_ID_COMMANDS_FIRST + index, item.label)?;
                }
                menu
            }
            BarMenu::Window => create_menu_from_entries(WINDOW_MENU)?,
            BarMenu::Help => create_menu_from_entries(HELP_MENU)?,
        };
        append_menu_text(frame_menu, MF_POPUP, popup as usize, label)?;
    }

    if SetMenu(hwnd, frame_menu) == 0 {
        return Err(last_os_error("SetMenu"));
    }
    let accelerators = [
        ACCEL {
            fVirt: FCONTROL | FVIRTKEY,
            key: u16::from(b'F'),
            cmd: MENU_ID_FIND as u16,
        },
        // Under Wine too, Alt+E arrives as WM_SYSKEYDOWN, which FVIRTKEY
        // with FALT matches.
        ACCEL {
            fVirt: FALT | FVIRTKEY,
            key: u16::from(b'E'),
            cmd: MENU_ID_FILE_SELECT_SERVER as u16,
        },
        // mIRC's List Channels.
        ACCEL {
            fVirt: FALT | FVIRTKEY,
            key: u16::from(b'L'),
            cmd: MENU_ID_COMMANDS_LIST as u16,
        },
        ACCEL {
            fVirt: FALT | FVIRTKEY,
            key: u16::from(b'O'),
            cmd: MENU_ID_TOOLS_OPTIONS as u16,
        },
        ACCEL {
            fVirt: FALT | FVIRTKEY,
            key: u16::from(b'K'),
            cmd: MENU_ID_TOOLS_COLORS as u16,
        },
        ACCEL {
            fVirt: FALT | FVIRTKEY,
            key: u16::from(b'A'),
            cmd: MENU_ID_TOOLS_ALIASES as u16,
        },
        ACCEL {
            fVirt: FALT | FVIRTKEY,
            key: u16::from(b'B'),
            cmd: MENU_ID_TOOLS_ADDRESS_BOOK as u16,
        },
    ]
    .into_iter()
    .chain(window_key_accelerators())
    .collect::<Vec<_>>();
    let accelerator_table =
        CreateAcceleratorTableW(accelerators.as_ptr(), accelerators.len() as i32);
    if accelerator_table.is_null() {
        return Err(last_os_error("CreateAcceleratorTableW"));
    }
    let controls = &mut state_mut(hwnd).controls;
    controls.file_menu = file_menu;
    controls.view_menu = view_menu;
    controls.tools_menu = tools_menu;
    controls.room_menu = room_menu;
    controls.room_notifications_menu = room_notifications_menu;
    controls.message_menu = message_menu;
    controls.member_menu = member_menu;
    controls.accelerators = accelerator_table;
    if DrawMenuBar(hwnd) == 0 {
        return Err(last_os_error("DrawMenuBar"));
    }
    Ok(())
}

pub(super) fn window_menu_command_id(arrangement: MdiArrangement) -> usize {
    match arrangement {
        MdiArrangement::Cascade => MENU_ID_WINDOW_CASCADE,
        MdiArrangement::TileHorizontal => MENU_ID_WINDOW_TILE_HORIZONTAL,
        MdiArrangement::TileVertical => MENU_ID_WINDOW_TILE_VERTICAL,
        MdiArrangement::ArrangeIcons => MENU_ID_WINDOW_ARRANGE_ICONS,
    }
}

/// Arranges the MDI windows. MDI itself restores a maximized window before
/// cascading or tiling.
unsafe fn arrange_mdi_windows(hwnd: HWND, arrangement: MdiArrangement) {
    let mdi_client = state_ref(hwnd).controls.mdi_client;
    let (message, wparam) = match arrangement {
        MdiArrangement::Cascade => (WM_MDICASCADE, 0),
        MdiArrangement::TileHorizontal => (WM_MDITILE, MDITILE_HORIZONTAL as WPARAM),
        MdiArrangement::TileVertical => (WM_MDITILE, MDITILE_VERTICAL as WPARAM),
        MdiArrangement::ArrangeIcons => (WM_MDIICONARRANGE, 0),
    };
    SendMessageW(mdi_client, message, wparam, 0);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RoomActionState {
    pub(super) accept_invite_enabled: bool,
    pub(super) reject_invite_enabled: bool,
    pub(super) part_room_enabled: bool,
    /// The active window's room is a channel Channel Central can show.
    pub(super) channel_central_enabled: bool,
    /// The active joined room's mode, which Notifications checks; `None`
    /// greys Notifications (an invite, or no room).
    pub(super) notification_mode: Option<RoomNotificationMode>,
}

/// What the Room menu can do in the window in front, which greys it.
pub(super) fn active_room_action_state(state: &WindowState) -> RoomActionState {
    let front = &state.active_view;
    let joined_room = resolve_joined_room_target(state, front, None);
    RoomActionState {
        accept_invite_enabled: resolve_invite_command_room_id(state, front, "accept", None).is_ok(),
        reject_invite_enabled: resolve_invite_command_room_id(state, front, "reject", None).is_ok(),
        part_room_enabled: joined_room.is_ok(),
        channel_central_enabled: channel_central_room(state, front, None).is_ok(),
        notification_mode: joined_room.ok().map(|room_id| {
            joined_room_notification_mode(
                sendable_room_by_id(&state.model.rooms, &room_id)
                    .expect("the joined room target is a joined room"),
            )
        }),
    }
}

/// What a double-click in a window's text does, off a link and a quote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DoubleClick {
    ChannelCentral,
    /// `/whois` of this user.
    Whois(String),
}

/// What a double-click in `window`'s text does, as mIRC's defaults
/// (Options > Mouse): a channel window opens Channel Central while it can,
/// a query runs `/whois` on its user; Status, a thread, an invite and a
/// direct room that is not one user's query do nothing.
pub(super) fn double_click_action(state: &WindowState, window: &ViewId) -> Option<DoubleClick> {
    let ViewId::Room(room_id) = window else {
        return None;
    };
    let room = room_by_id(&state.model.rooms, room_id).expect("a room window's room exists");
    match (room_window_kind(room), room.direct_user_ids.as_slice()) {
        (RoomWindowKind::Channel, _) => channel_central_room(state, window, None)
            .is_ok()
            .then_some(DoubleClick::ChannelCentral),
        (RoomWindowKind::Query, [user_id]) => Some(DoubleClick::Whois(user_id.clone())),
        (RoomWindowKind::Query, _) | (RoomWindowKind::Invite, _) => None,
    }
}

/// Notifications checks the room's mode, or nothing while it is greyed.
unsafe fn refresh_room_notifications_menu_state(
    hwnd: HWND,
    popup_menu: HMENU,
) -> Result<(), String> {
    let mode = active_room_action_state(state_ref(hwnd)).notification_mode;
    for (_, item_id, _) in ROOM_NOTIFICATIONS_MENU {
        let flags = MF_BYCOMMAND
            | if mode.is_some() {
                MF_ENABLED
            } else {
                MF_GRAYED
            };
        if EnableMenuItem(popup_menu, item_id as u32, flags) == -1 {
            return Err(last_os_error("EnableMenuItem"));
        }
    }
    match mode {
        Some(mode) => {
            let [(_, first, _), _, (_, last, _)] = ROOM_NOTIFICATIONS_MENU;
            if CheckMenuRadioItem(
                popup_menu,
                first as u32,
                last as u32,
                notification_menu_id(mode) as u32,
                MF_BYCOMMAND,
            ) == 0
            {
                return Err(last_os_error("CheckMenuRadioItem"));
            }
        }
        None => {
            for (_, item_id, _) in ROOM_NOTIFICATIONS_MENU {
                if CheckMenuItem(popup_menu, item_id as u32, MF_BYCOMMAND | MF_UNCHECKED)
                    == u32::MAX
                {
                    return Err(format!("CheckMenuItem: menu item {item_id} does not exist"));
                }
            }
        }
    }
    Ok(())
}

pub(super) unsafe fn refresh_room_menu_state(hwnd: HWND, popup_menu: HMENU) -> Result<(), String> {
    let action_state = active_room_action_state(state_ref(hwnd));
    for (item_id, enabled) in [
        (
            MENU_ID_ROOM_ACCEPT_INVITE,
            action_state.accept_invite_enabled,
        ),
        (
            MENU_ID_ROOM_REJECT_INVITE,
            action_state.reject_invite_enabled,
        ),
        (MENU_ID_ROOM_PART, action_state.part_room_enabled),
        (
            MENU_ID_ROOM_CHANNEL_CENTRAL,
            action_state.channel_central_enabled,
        ),
    ] {
        let flags = MF_BYCOMMAND | if enabled { MF_ENABLED } else { MF_GRAYED };
        if EnableMenuItem(popup_menu, item_id as u32, flags) == -1 {
            return Err(last_os_error("EnableMenuItem"));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MessageActionState {
    pub(super) selected_event_id: Option<String>,
    /// The selected message is the user's own text or emote.
    pub(super) edit_enabled: bool,
    pub(super) save_attachment_enabled: bool,
    /// The selected message is a reply with a quote line.
    pub(super) go_to_original_enabled: bool,
}

/// The message the selected reply quotes, if it has a quote line.
pub(super) fn selected_message_quote_target<'a>(
    state: &'a WindowState,
    selected_event_id: Option<&str>,
) -> Option<&'a str> {
    window_message(&state.model, &state.active_view, selected_event_id?)?
        .relation
        .as_ref()
        .and_then(replies::reply_quote_target)
}

fn selected_message_editable(state: &WindowState, selected_event_id: Option<&str>) -> bool {
    selected_event_id
        .and_then(|event_id| window_message(&state.model, &state.active_view, event_id))
        .is_some_and(|activity| {
            replies::editable_message(activity, state.model.own_user_id()).is_ok()
        })
}

pub(super) fn selected_timeline_media_attachment(
    state: &WindowState,
) -> Option<SelectedMediaAttachment> {
    let selected = state.selected_timeline_event.as_ref()?;
    if selected.view_id != state.active_view {
        return None;
    }
    let room_id = match &state.active_view {
        ViewId::Room(room_id) => room_id,
        ViewId::Thread { room_id, .. } => room_id,
        ViewId::Status | ViewId::RoomList => return None,
    };
    let activity = state
        .model
        .room_views
        .get(room_id)?
        .activities
        .iter()
        .find(|activity| match &activity.kind {
            RoomActivityKind::File(file) => file.event_id == selected.event_id,
            RoomActivityKind::Audio(audio) => audio.event_id == selected.event_id,
            RoomActivityKind::Video(video) => video.event_id == selected.event_id,
            _ => false,
        })?;
    let (event_id, filename, source) = match &activity.kind {
        RoomActivityKind::File(file) => {
            (&file.event_id, file.filename.clone(), file.source.clone())
        }
        RoomActivityKind::Audio(audio) => (
            &audio.event_id,
            audio.filename.clone(),
            audio.source.clone(),
        ),
        RoomActivityKind::Video(video) => (
            &video.event_id,
            video.filename.clone(),
            video.source.clone(),
        ),
        _ => return None,
    };
    Some(SelectedMediaAttachment {
        room_id: room_id.clone(),
        activity_id: activity.activity_id.clone(),
        event_id: event_id.clone(),
        filename,
        source,
    })
}

/// A row is selected only when there is a selection and the row is that
/// event. Rows without an event id (opening lines, typing, backfill status)
/// are never selected, even when nothing is selected.
pub(super) fn timeline_row_is_selected(
    row_event_id: Option<&str>,
    selected_event_id: Option<&str>,
) -> bool {
    selected_event_id.is_some_and(|selected| row_event_id == Some(selected))
}

pub(super) fn selected_timeline_event_id(state: &WindowState) -> Option<&str> {
    state.selected_timeline_event.as_ref().and_then(|selected| {
        (selected.view_id == state.active_view).then_some(selected.event_id.as_str())
    })
}

pub(super) unsafe fn selected_timeline_event_id_for_automation(hwnd: HWND) -> Option<String> {
    let state = state_ref(hwnd);
    let selected = state.selected_timeline_event.as_ref()?;
    if selected.view_id != state.active_view {
        return None;
    }
    let content = state.views.get(&state.active_view)?.content;
    timeline_state_ref(room_content_timeline(content))
        .source_rows
        .iter()
        .any(|row| row.event_id.as_deref() == Some(&selected.event_id))
        .then(|| selected.event_id.clone())
}

#[cfg(test)]
pub(super) fn active_message_action_state(state: &WindowState) -> MessageActionState {
    let selected_event_id = match state.active_view {
        ViewId::Room(_) | ViewId::Thread { .. } => {
            selected_timeline_event_id(state).map(str::to_owned)
        }
        ViewId::Status | ViewId::RoomList => None,
    };
    let save_attachment_enabled = selected_timeline_media_attachment(state).is_some();
    MessageActionState {
        edit_enabled: selected_message_editable(state, selected_event_id.as_deref()),
        go_to_original_enabled: selected_message_quote_target(state, selected_event_id.as_deref())
            .is_some(),
        selected_event_id,
        save_attachment_enabled,
    }
}

pub(super) unsafe fn active_message_action_state_for_automation(hwnd: HWND) -> MessageActionState {
    let selected_event_id = match state_ref(hwnd).active_view {
        ViewId::Room(_) | ViewId::Thread { .. } => selected_timeline_event_id_for_automation(hwnd),
        ViewId::Status | ViewId::RoomList => None,
    };
    let state = state_ref(hwnd);
    let save_attachment_enabled = selected_timeline_media_attachment(state).is_some();
    MessageActionState {
        edit_enabled: selected_message_editable(state, selected_event_id.as_deref()),
        go_to_original_enabled: selected_message_quote_target(state, selected_event_id.as_deref())
            .is_some(),
        selected_event_id,
        save_attachment_enabled,
    }
}

pub(super) unsafe fn refresh_message_menu_state(
    hwnd: HWND,
    popup_menu: HMENU,
) -> Result<(), String> {
    let action_state = active_message_action_state_for_automation(hwnd);
    let flags = MF_BYCOMMAND
        | if action_state.selected_event_id.is_some() {
            MF_ENABLED
        } else {
            MF_GRAYED
        };
    for item_id in [
        MENU_ID_MESSAGE_REPLY,
        MENU_ID_MESSAGE_REACT,
        MENU_ID_MESSAGE_REDACT,
    ] {
        if EnableMenuItem(popup_menu, item_id as u32, flags) == -1 {
            return Err(last_os_error("EnableMenuItem"));
        }
    }
    let flags = MF_BYCOMMAND
        | if action_state.edit_enabled {
            MF_ENABLED
        } else {
            MF_GRAYED
        };
    if EnableMenuItem(popup_menu, MENU_ID_MESSAGE_EDIT as u32, flags) == -1 {
        return Err(last_os_error("EnableMenuItem"));
    }
    let flags = MF_BYCOMMAND
        | if action_state.save_attachment_enabled {
            MF_ENABLED
        } else {
            MF_GRAYED
        };
    if EnableMenuItem(popup_menu, MENU_ID_MESSAGE_SAVE_ATTACHMENT as u32, flags) == -1 {
        return Err(last_os_error("EnableMenuItem"));
    }
    let flags = MF_BYCOMMAND
        | if action_state.go_to_original_enabled {
            MF_ENABLED
        } else {
            MF_GRAYED
        };
    if EnableMenuItem(popup_menu, MENU_ID_MESSAGE_GO_TO_ORIGINAL as u32, flags) == -1 {
        return Err(last_os_error("EnableMenuItem"));
    }
    Ok(())
}

pub(super) unsafe fn append_menu_text(
    menu: HMENU,
    flags: u32,
    item_id: usize,
    text: &str,
) -> Result<(), String> {
    let wide = wide_null(text);
    if AppendMenuW(menu, flags, item_id, wide.as_ptr()) == 0 {
        return Err(last_os_error("AppendMenuW"));
    }
    Ok(())
}

/// Updates a menu's items before it opens, as `WM_INITMENUPOPUP` and the
/// automation dumps do. Other menus (Commands, Window, Help, the window
/// menu) have nothing that greys.
pub(super) unsafe fn refresh_menu_popup(hwnd: HWND, popup_menu: HMENU) -> Result<(), String> {
    let controls = &state_ref(hwnd).controls;
    if popup_menu == controls.file_menu {
        refresh_file_menu_state(hwnd, popup_menu)
    } else if popup_menu == controls.view_menu {
        refresh_view_menu_checks(hwnd, popup_menu)
    } else if popup_menu == controls.tools_menu {
        refresh_tools_menu_state(hwnd, popup_menu)
    } else if popup_menu == controls.room_menu {
        refresh_room_menu_state(hwnd, popup_menu)
    } else if popup_menu == controls.room_notifications_menu {
        refresh_room_notifications_menu_state(hwnd, popup_menu)
    } else if popup_menu == controls.message_menu {
        refresh_message_menu_state(hwnd, popup_menu)
    } else {
        Ok(())
    }
}

/// Whether a menu bar item is enabled. File greys by connection and the
/// verification items by the current flow; no other bar item ever greys.
pub(super) fn frame_menu_item_enabled(state: &WindowState, command_id: usize) -> bool {
    let file = file_menu_state(state.model.connection);
    let verification =
        |action| verification_menu_action_enabled(state.model.last_verification.as_ref(), action);
    match command_id {
        MENU_ID_FILE_CONNECT => file.connect,
        MENU_ID_FILE_DISCONNECT => file.disconnect,
        MENU_ID_FILE_SELECT_SERVER => file.select_server,
        MENU_ID_LOGOUT => file.log_out,
        MENU_ID_VERIFY_ACCEPT => verification(VerificationAction::Accept),
        MENU_ID_VERIFY_CANCEL => verification(VerificationAction::Cancel),
        MENU_ID_VERIFY_CONFIRM_SAS => verification(VerificationAction::Confirm),
        MENU_ID_VERIFY_REJECT_SAS => verification(VerificationAction::Reject),
        MENU_ID_ROOM_CHANNEL_CENTRAL => active_room_action_state(state).channel_central_enabled,
        _ => true,
    }
}

/// Greys or enables each of `items` in `popup_menu`.
unsafe fn refresh_frame_menu_items(
    hwnd: HWND,
    popup_menu: HMENU,
    items: &[usize],
) -> Result<(), String> {
    for item in items {
        let flags = MF_BYCOMMAND
            | if frame_menu_item_enabled(state_ref(hwnd), *item) {
                MF_ENABLED
            } else {
                MF_GRAYED
            };
        if EnableMenuItem(popup_menu, *item as u32, flags) == -1 {
            return Err(last_os_error("EnableMenuItem"));
        }
    }
    Ok(())
}

unsafe fn refresh_file_menu_state(hwnd: HWND, popup_menu: HMENU) -> Result<(), String> {
    refresh_frame_menu_items(
        hwnd,
        popup_menu,
        &[
            MENU_ID_FILE_CONNECT,
            MENU_ID_FILE_DISCONNECT,
            MENU_ID_FILE_SELECT_SERVER,
            MENU_ID_LOGOUT,
        ],
    )
}

unsafe fn refresh_view_menu_checks(hwnd: HWND, popup_menu: HMENU) -> Result<(), String> {
    for (item, checked) in view_menu_checks(state_ref(hwnd).settings.settings()) {
        let flags = MF_BYCOMMAND | if checked { MF_CHECKED } else { MF_UNCHECKED };
        if CheckMenuItem(popup_menu, item as u32, flags) == u32::MAX {
            return Err(format!("CheckMenuItem: menu item {item} does not exist"));
        }
    }
    Ok(())
}

pub(super) unsafe fn refresh_tools_menu_state(hwnd: HWND, popup_menu: HMENU) -> Result<(), String> {
    refresh_frame_menu_items(
        hwnd,
        popup_menu,
        &[
            MENU_ID_VERIFY_ACCEPT,
            MENU_ID_VERIFY_CANCEL,
            MENU_ID_VERIFY_CONFIRM_SAS,
            MENU_ID_VERIFY_REJECT_SAS,
        ],
    )
}

pub(super) unsafe fn select_timeline_event_for_automation(
    hwnd: HWND,
    event_id: &str,
) -> Result<(), String> {
    let active_view = state_ref(hwnd).active_view.clone();
    if !matches!(active_view, ViewId::Room(_) | ViewId::Thread { .. }) {
        return Err("selected_message_unavailable: active view is not a room or thread".to_owned());
    }
    let content = state_ref(hwnd)
        .views
        .get(&active_view)
        .ok_or_else(|| "selected_message_unavailable: active view is unavailable".to_owned())?
        .content;
    let timeline = room_content_timeline(content);
    if !timeline_state_ref(timeline)
        .source_rows
        .iter()
        .any(|row| row.event_id.as_deref() == Some(event_id))
    {
        return Err(format!("unknown_event: {event_id}"));
    }
    state_mut(hwnd).selected_timeline_event = Some(SelectedTimelineEvent {
        view_id: active_view,
        event_id: event_id.to_owned(),
    });
    InvalidateRect(timeline, null(), 1);
    Ok(())
}

unsafe fn window_menu_command(
    hwnd: HWND,
    arrangement: MdiArrangement,
) -> (&'static str, Result<Option<()>, String>) {
    arrange_mdi_windows(hwnd, arrangement);
    ("Window", Ok(Some(())))
}

/// Export or Import Room Keys...: `/e2ee export|import` without a file.
unsafe fn room_keys_menu_command(
    hwnd: HWND,
    window: &ViewId,
    transfer: RoomKeyTransfer,
) -> (&'static str, Result<Option<()>, String>) {
    let command = SlashCommand::E2ee(E2eeSlashCommand::RoomKeys {
        transfer,
        path: None,
    });
    (
        "Tools",
        run_local_command(hwnd, window, command).map(|_| Some(())),
    )
}

/// Runs a menu command. It acts in the window active when it was picked,
/// taken before any prompt opens: a prompt's modal loop handles core
/// events, and a join landing meanwhile activates another window.
unsafe fn menu_command_result(
    hwnd: HWND,
    wparam: WPARAM,
) -> Option<(&'static str, Result<Option<()>, String>)> {
    let command_id = wparam & 0xffff;
    let window = state_ref(hwnd).active_view.clone();
    let window = &window;
    let (title, result) = match command_id {
        MENU_ID_FIND => ("Find", begin_find_dialog(hwnd, window).map(|_| Some(()))),
        MENU_ID_WINDOW_CASCADE => window_menu_command(hwnd, MdiArrangement::Cascade),
        MENU_ID_WINDOW_TILE_HORIZONTAL => window_menu_command(hwnd, MdiArrangement::TileHorizontal),
        MENU_ID_WINDOW_TILE_VERTICAL => window_menu_command(hwnd, MdiArrangement::TileVertical),
        MENU_ID_WINDOW_ARRANGE_ICONS => window_menu_command(hwnd, MdiArrangement::ArrangeIcons),
        MENU_ID_ROOM_ACCEPT_INVITE => (
            "Room",
            submit_room_menu_action(hwnd, window, submit_accept_slash_command).map(|_| Some(())),
        ),
        MENU_ID_ROOM_REJECT_INVITE => (
            "Room",
            submit_room_menu_action(hwnd, window, submit_reject_slash_command).map(|_| Some(())),
        ),
        MENU_ID_ROOM_PART => (
            "Room",
            submit_room_menu_action(hwnd, window, submit_part_slash_command).map(|_| Some(())),
        ),
        MENU_ID_ROOM_CHANNEL_CENTRAL => (
            "Channel Central",
            run_local_command(hwnd, window, SlashCommand::Channel { target: None })
                .map(|_| Some(())),
        ),
        MENU_ID_ROOM_NOTIFY_ALL | MENU_ID_ROOM_NOTIFY_MENTIONS | MENU_ID_ROOM_NOTIFY_MUTE => {
            let mode = notification_menu_mode(command_id).expect("a Notifications item");
            (
                "Room",
                submit_room_menu_action(hwnd, window, |state, window, target| {
                    submit_room_notification_mode(state, window, target, mode)
                })
                .map(|_| Some(())),
            )
        }
        MENU_ID_MESSAGE_REPLY => (
            "Message",
            arm_compose_target_on_selected_message(hwnd, window, |event_id| ComposeTarget::Reply {
                event_id,
            })
            .map(|_| Some(())),
        ),
        MENU_ID_MESSAGE_GO_TO_ORIGINAL => (
            "Message",
            go_to_original_of_selected_message(hwnd).map(|_| Some(())),
        ),
        MENU_ID_MESSAGE_EDIT => (
            "Message",
            arm_compose_target_on_selected_message(hwnd, window, |event_id| ComposeTarget::Edit {
                event_id,
            })
            .map(|_| Some(())),
        ),
        MENU_ID_MESSAGE_REACT => (
            "Message",
            prefill_react_to_selected_message(hwnd).map(|_| Some(())),
        ),
        MENU_ID_MESSAGE_REDACT => (
            "Message",
            submit_redact_selected_message(hwnd, window).map(|_| Some(())),
        ),
        MENU_ID_MESSAGE_SAVE_ATTACHMENT => (
            "Message",
            save_selected_message_attachment(hwnd).map(|_| Some(())),
        ),
        MENU_ID_MEMBER_QUERY
        | MENU_ID_MEMBER_MENTION
        | MENU_ID_MEMBER_WHOIS
        | MENU_ID_MEMBER_DEVICES
        | MENU_ID_MEMBER_VERIFY
        | MENU_ID_MEMBER_ACCEPT_IDENTITY
        | MENU_ID_MEMBER_IGNORE
        | MENU_ID_MEMBER_UNIGNORE
        | MENU_ID_MEMBER_OP
        | MENU_ID_MEMBER_DEOP
        | MENU_ID_MEMBER_VOICE
        | MENU_ID_MEMBER_DEVOICE
        | MENU_ID_MEMBER_KICK
        | MENU_ID_MEMBER_BAN => (
            "Member",
            run_member_menu_item(hwnd, window, command_id).map(|_| Some(())),
        ),
        MENU_ID_FILE_CONNECT => (
            "File",
            connect(hwnd, None).map(|performed| performed.then_some(())),
        ),
        MENU_ID_FILE_DISCONNECT => ("File", disconnect(hwnd).map(|_| Some(()))),
        MENU_ID_FILE_SELECT_SERVER => (
            "File",
            show_select_server_dialog(hwnd, "").map(|performed| performed.then_some(())),
        ),
        MENU_ID_FILE_EXIT => (
            "File",
            run_local_command(hwnd, window, SlashCommand::Exit).map(|_| Some(())),
        ),
        MENU_ID_HELP_TOPICS => (
            "Help",
            run_local_command(hwnd, window, SlashCommand::Help { topic: None }).map(|_| Some(())),
        ),
        MENU_ID_HELP_ABOUT => ("Help", show_about_dialog(hwnd).map(|()| Some(()))),
        MENU_ID_TOOLS_OPTIONS => ("Options", run_options_dialog(hwnd).map(|()| Some(()))),
        MENU_ID_TOOLS_COLORS => ("Colors", run_colors_dialog(hwnd).map(|()| Some(()))),
        MENU_ID_TOOLS_ADDRESS_BOOK => (
            "Address Book",
            address_book_dialog::run_address_book_dialog(hwnd, AddressBookTab::Notify)
                .map(|()| Some(())),
        ),
        MENU_ID_TOOLS_ALIASES => (
            "Aliases",
            aliases_dialog::run_aliases_dialog(hwnd).map(|()| Some(())),
        ),
        MENU_ID_VIEW_TOOLBAR | MENU_ID_VIEW_SWITCHBAR | MENU_ID_VIEW_TREEBAR => {
            let bar = view_menu_bar(command_id).expect("View items show and hide a bar");
            let command = if state_ref(hwnd).settings.settings().bar_shown(bar) {
                BarCommand::Off
            } else {
                BarCommand::On
            };
            (
                "View",
                run_local_command(hwnd, window, SlashCommand::Bar { bar, command })
                    .map(|_| Some(())),
            )
        }
        MENU_ID_RECOVER_E2EE => (
            "Tools",
            show_recover_e2ee_menu_dialog(hwnd).map(|handled| handled.then_some(())),
        ),
        MENU_ID_SETUP_E2EE_RECOVERY => (
            "Tools",
            show_setup_e2ee_menu_dialog(hwnd).map(|handled| handled.then_some(())),
        ),
        MENU_ID_RESET_E2EE => (
            "Tools",
            show_reset_e2ee_menu_dialog(hwnd).map(|handled| handled.then_some(())),
        ),
        MENU_ID_EXPORT_ROOM_KEYS => room_keys_menu_command(hwnd, window, RoomKeyTransfer::Export),
        MENU_ID_IMPORT_ROOM_KEYS => room_keys_menu_command(hwnd, window, RoomKeyTransfer::Import),
        MENU_ID_VERIFY_THIS_SESSION => (
            "Tools",
            run_local_command(
                hwnd,
                window,
                SlashCommand::VerifyStart {
                    target: None,
                    device_id: None,
                },
            )
            .map(|_| Some(())),
        ),
        MENU_ID_TOOLS_SESSIONS => (
            "Sessions",
            sessions_dialog::run_sessions_dialog(hwnd).map(|()| Some(())),
        ),
        MENU_ID_VERIFY_ACCEPT => (
            "Tools",
            submit_menu_verification_action(hwnd, VerificationAction::Accept).map(|_| Some(())),
        ),
        MENU_ID_VERIFY_CANCEL => (
            "Tools",
            submit_menu_verification_action(hwnd, VerificationAction::Cancel).map(|_| Some(())),
        ),
        MENU_ID_VERIFY_CONFIRM_SAS => (
            "Tools",
            submit_menu_verification_action(hwnd, VerificationAction::Confirm).map(|_| Some(())),
        ),
        MENU_ID_VERIFY_REJECT_SAS => (
            "Tools",
            submit_menu_verification_action(hwnd, VerificationAction::Reject).map(|_| Some(())),
        ),
        MENU_ID_LOGOUT => ("File", log_out(hwnd).map(|_| Some(()))),
        TOOLBAR_ID_SEND_FILE => (
            "Send File",
            run_local_command(hwnd, window, SlashCommand::SendFile { path: None })
                .map(|_| Some(())),
        ),
        _ => {
            let item = commands_menu_item(command_id)?;
            (
                "Commands",
                run_commands_menu_item(hwnd, window, item).map(|performed| performed.then_some(())),
            )
        }
    };

    Some((title, result))
}

pub(super) unsafe fn handle_menu_command(hwnd: HWND, wparam: WPARAM) -> Option<Result<(), String>> {
    let (title, result) = menu_command_result(hwnd, wparam)?;

    if let Err(message) = &result {
        show_error_message(hwnd, title, message);
    }

    Some(
        result
            .and_then(|handled| handled.ok_or_else(|| "menu action was not performed".to_owned())),
    )
}

pub(super) unsafe fn invoke_menu_command_for_automation(
    hwnd: HWND,
    command_id: usize,
) -> Result<(), String> {
    let room_action_enabled = match command_id {
        MENU_ID_ROOM_ACCEPT_INVITE => {
            active_room_action_state(state_ref(hwnd)).accept_invite_enabled
        }
        MENU_ID_ROOM_REJECT_INVITE => {
            active_room_action_state(state_ref(hwnd)).reject_invite_enabled
        }
        MENU_ID_ROOM_PART => active_room_action_state(state_ref(hwnd)).part_room_enabled,
        MENU_ID_ROOM_NOTIFY_ALL | MENU_ID_ROOM_NOTIFY_MENTIONS | MENU_ID_ROOM_NOTIFY_MUTE => {
            active_room_action_state(state_ref(hwnd))
                .notification_mode
                .is_some()
        }
        _ => true,
    };
    if !room_action_enabled {
        return Err("room menu action is disabled".to_owned());
    }
    let Some((_, result)) = menu_command_result(hwnd, command_id) else {
        return Err(format!("unknown menu command {command_id}"));
    };
    result.and_then(|handled| handled.ok_or_else(|| "menu action was not performed".to_owned()))
}

/// The user's gesture that opens a dialog automation asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DialogGesture {
    /// A menu bar item, by command id.
    MenuCommand(usize),
    /// The active window's close box.
    CloseActiveView,
    /// Return in the active window's editbox.
    ComposeReturn,
}

pub(super) fn dialog_gesture(dialog: AutomationDialog) -> DialogGesture {
    match dialog {
        AutomationDialog::Options => DialogGesture::MenuCommand(MENU_ID_TOOLS_OPTIONS),
        AutomationDialog::Colors => DialogGesture::MenuCommand(MENU_ID_TOOLS_COLORS),
        AutomationDialog::Aliases => DialogGesture::MenuCommand(MENU_ID_TOOLS_ALIASES),
        AutomationDialog::AddressBook => DialogGesture::MenuCommand(MENU_ID_TOOLS_ADDRESS_BOOK),
        AutomationDialog::ChannelCentral => {
            DialogGesture::MenuCommand(MENU_ID_ROOM_CHANNEL_CENTRAL)
        }
        AutomationDialog::Sessions => DialogGesture::MenuCommand(MENU_ID_TOOLS_SESSIONS),
        AutomationDialog::SelectServer => DialogGesture::MenuCommand(MENU_ID_FILE_SELECT_SERVER),
        AutomationDialog::About => DialogGesture::MenuCommand(MENU_ID_HELP_ABOUT),
        AutomationDialog::CommandsJoin => DialogGesture::MenuCommand(MENU_ID_COMMANDS_JOIN),
        AutomationDialog::RecoverE2ee => DialogGesture::MenuCommand(MENU_ID_RECOVER_E2EE),
        AutomationDialog::SetupE2eeRecovery => {
            DialogGesture::MenuCommand(MENU_ID_SETUP_E2EE_RECOVERY)
        }
        AutomationDialog::ResetE2ee => DialogGesture::MenuCommand(MENU_ID_RESET_E2EE),
        AutomationDialog::CloseActiveView => DialogGesture::CloseActiveView,
        AutomationDialog::ComposeReturn => DialogGesture::ComposeReturn,
    }
}

/// Opens `dialog` as the user's gesture does. The gesture is posted, not
/// run: the dialog's modal loop must not block the automation request.
/// Fails, as `(code, message)`, where the gesture would not open it: a
/// greyed item, or a window that closes without asking.
pub(super) unsafe fn open_dialog_for_automation(
    hwnd: HWND,
    dialog: AutomationDialog,
) -> Result<(), (&'static str, String)> {
    let (target, message, wparam) = match dialog_gesture(dialog) {
        DialogGesture::MenuCommand(command_id) => {
            if !frame_menu_item_enabled(state_ref(hwnd), command_id) {
                return Err((
                    "menu_item_disabled",
                    format!("the menu item for {dialog:?} is greyed"),
                ));
            }
            (hwnd, WM_COMMAND, command_id)
        }
        DialogGesture::CloseActiveView => {
            let state = state_ref(hwnd);
            let action = close_action(&state.model, &state.active_view);
            if !matches!(action, CloseAction::Confirm { .. }) {
                return Err((
                    "no_close_confirmation",
                    format!("closing the active window does not ask: {action:?}"),
                ));
            }
            let child = state
                .views
                .get(&state.active_view)
                .expect("the active view has a window")
                .child;
            (child, WM_SYSCOMMAND, SC_CLOSE as usize)
        }
        DialogGesture::ComposeReturn => {
            let edit = compose_edit(hwnd);
            if read_window_text(edit).is_empty() {
                return Err((
                    "empty_compose",
                    "the active window's editbox is empty".to_owned(),
                ));
            }
            (edit, WM_KEYDOWN, VK_RETURN_KEY)
        }
    };
    if PostMessageW(target, message, wparam, 0) == 0 {
        return Err(("post_failed", last_os_error("PostMessageW")));
    }
    Ok(())
}

pub(super) fn context_menu_command_id(command: &ContextMenuCommand) -> Option<usize> {
    Some(match command {
        ContextMenuCommand::AcceptInvite => MENU_ID_ROOM_ACCEPT_INVITE,
        ContextMenuCommand::RejectInvite => MENU_ID_ROOM_REJECT_INVITE,
        ContextMenuCommand::PartRoom => MENU_ID_ROOM_PART,
        ContextMenuCommand::NotifyAllMessages => MENU_ID_ROOM_NOTIFY_ALL,
        ContextMenuCommand::NotifyMentionsAndKeywords => MENU_ID_ROOM_NOTIFY_MENTIONS,
        ContextMenuCommand::NotifyMute => MENU_ID_ROOM_NOTIFY_MUTE,
        ContextMenuCommand::ReplyMessage => MENU_ID_MESSAGE_REPLY,
        ContextMenuCommand::GoToOriginal => MENU_ID_MESSAGE_GO_TO_ORIGINAL,
        ContextMenuCommand::EditMessage => MENU_ID_MESSAGE_EDIT,
        ContextMenuCommand::ReactMessage => MENU_ID_MESSAGE_REACT,
        ContextMenuCommand::RedactMessage => MENU_ID_MESSAGE_REDACT,
        ContextMenuCommand::SaveAttachment => MENU_ID_MESSAGE_SAVE_ATTACHMENT,
        ContextMenuCommand::WhoisMember => MENU_ID_MEMBER_WHOIS,
        ContextMenuCommand::DevicesMember => MENU_ID_MEMBER_DEVICES,
        ContextMenuCommand::VerifyMember => MENU_ID_MEMBER_VERIFY,
        ContextMenuCommand::AcceptIdentityMember => MENU_ID_MEMBER_ACCEPT_IDENTITY,
        ContextMenuCommand::QueryMember => MENU_ID_MEMBER_QUERY,
        ContextMenuCommand::MentionMember => MENU_ID_MEMBER_MENTION,
        ContextMenuCommand::IgnoreMember => MENU_ID_MEMBER_IGNORE,
        ContextMenuCommand::UnignoreMember => MENU_ID_MEMBER_UNIGNORE,
        ContextMenuCommand::OpMember => MENU_ID_MEMBER_OP,
        ContextMenuCommand::DeopMember => MENU_ID_MEMBER_DEOP,
        ContextMenuCommand::VoiceMember => MENU_ID_MEMBER_VOICE,
        ContextMenuCommand::DevoiceMember => MENU_ID_MEMBER_DEVOICE,
        ContextMenuCommand::KickMember => MENU_ID_MEMBER_KICK,
        ContextMenuCommand::BanMember => MENU_ID_MEMBER_BAN,
    })
}

pub(super) unsafe fn invoke_navigation_context_menu_for_automation(
    hwnd: HWND,
    command: &ContextMenuCommand,
) -> Result<(), String> {
    let command_id = context_menu_command_id(command)
        .ok_or_else(|| "room context menu command is unavailable".to_owned())?;
    if !matches!(
        command_id,
        MENU_ID_ROOM_ACCEPT_INVITE
            | MENU_ID_ROOM_REJECT_INVITE
            | MENU_ID_ROOM_PART
            | MENU_ID_ROOM_NOTIFY_ALL
            | MENU_ID_ROOM_NOTIFY_MENTIONS
            | MENU_ID_ROOM_NOTIFY_MUTE
    ) {
        return Err("room context menu command is unavailable".to_owned());
    }
    show_navigation_room_context_menu(hwnd, -1, Some(command_id))
}

pub(super) unsafe fn invoke_timeline_context_menu_for_automation(
    hwnd: HWND,
    command: &ContextMenuCommand,
    event_id: Option<&str>,
) -> Result<(), String> {
    let command_id = context_menu_command_id(command)
        .ok_or_else(|| "message context menu command is unavailable".to_owned())?;
    if !matches!(
        command_id,
        MENU_ID_MESSAGE_REPLY
            | MENU_ID_MESSAGE_GO_TO_ORIGINAL
            | MENU_ID_MESSAGE_EDIT
            | MENU_ID_MESSAGE_REACT
            | MENU_ID_MESSAGE_REDACT
            | MENU_ID_MESSAGE_SAVE_ATTACHMENT
    ) {
        return Err("message context menu command is unavailable".to_owned());
    }
    let active_view = state_ref(hwnd).active_view.clone();
    let content = state_ref(hwnd)
        .views
        .get(&active_view)
        .ok_or_else(|| "message context menu target is unavailable".to_owned())?
        .content;
    let timeline = view_timeline(&active_view, content);
    let lparam = match event_id {
        Some(event_id) => timeline_event_context_lparam_for_automation(timeline, event_id)?,
        None => -1,
    };
    show_timeline_message_context_menu(timeline, lparam, Some(command_id))
}

unsafe fn timeline_event_context_lparam_for_automation(
    timeline: HWND,
    event_id: &str,
) -> Result<LPARAM, String> {
    let state = timeline_state_ref(timeline);
    let line_index = state
        .paint_lines
        .iter()
        .position(|line| {
            line.source_row_index
                .and_then(|index| state.source_rows.get(index))
                .and_then(|row| row.event_id.as_deref())
                == Some(event_id)
        })
        .ok_or_else(|| format!("unknown_event: {event_id}"))?;
    let y = state.metrics.vertical_padding
        + (line_index.saturating_sub(state.top_line) as i32 * state.line_height)
        + state.line_height / 2;
    let mut point = POINT { x: 1, y };
    if ClientToScreen(timeline, &mut point) == 0 {
        return Err(last_os_error("ClientToScreen"));
    }
    Ok(((point.y as u32 as usize) << 16 | (point.x as u16 as usize)) as LPARAM)
}

unsafe fn submit_room_menu_action(
    hwnd: HWND,
    window: &ViewId,
    submit: impl FnOnce(&mut WindowState, &ViewId, Option<&str>) -> Result<String, String>,
) -> Result<(), String> {
    let result = submit(state_mut(hwnd), window, None);
    match result {
        Ok(message) => set_local_command_status(hwnd, message),
        Err(message) => {
            set_local_command_status(hwnd, message.clone())?;
            Err(message)
        }
    }
}

/// mIRC's window keys. They run before `TranslateMDISysAccel`, so
/// Ctrl+Tab follows the switchbar; MDI's own z-order cycling stays on
/// Ctrl+F6.
fn window_key_accelerators() -> impl Iterator<Item = ACCEL> {
    let accel = |modifiers, key: u16, cmd: usize| ACCEL {
        fVirt: modifiers,
        key,
        cmd: cmd as u16,
    };
    (b'1'..=b'9')
        .zip(ACCEL_ID_WINDOW_FIRST..=ACCEL_ID_WINDOW_LAST)
        .map(move |(digit, cmd)| accel(FALT | FVIRTKEY, u16::from(digit), cmd))
        .chain([
            accel(FCONTROL | FVIRTKEY, VK_TAB, ACCEL_ID_WINDOW_NEXT),
            accel(
                FCONTROL | FSHIFT | FVIRTKEY,
                VK_TAB,
                ACCEL_ID_WINDOW_PREVIOUS,
            ),
            accel(FCONTROL | FVIRTKEY, u16::from(b'N'), ACCEL_ID_NEXT_CHANNEL),
            accel(FCONTROL | FVIRTKEY, u16::from(b'Q'), ACCEL_ID_NEXT_QUERY),
            accel(FALT | FVIRTKEY, u16::from(b'X'), ACCEL_ID_TOGGLE_MAXIMIZE),
            accel(FCONTROL | FVIRTKEY, u16::from(b'T'), ACCEL_ID_TREEBAR_FOCUS),
            accel(FCONTROL | FVIRTKEY, u16::from(b'L'), ACCEL_ID_LINE_MARKER),
        ])
}

pub(super) unsafe fn handle_window_key_command(hwnd: HWND, wparam: WPARAM) -> bool {
    let Some(key) = window_key(wparam & 0xffff) else {
        return false;
    };
    let state = state_ref(hwnd);
    let target = match key {
        WindowKey::Nth(n) => nth_window(&ordered_view_ids(state), n).cloned(),
        WindowKey::Cycle { cycle, forward } => {
            cycled_window(state, &ordered_view_ids(state), cycle, forward)
        }
        WindowKey::ToggleMaximize => {
            toggle_active_window_maximized(hwnd);
            None
        }
        WindowKey::TreebarFocus => {
            toggle_treebar_focus(hwnd);
            None
        }
        WindowKey::LineMarker => {
            if let Err(message) = line_marker::go_to_line_marker(hwnd) {
                show_error_message(hwnd, "Line Marker", &message);
            }
            None
        }
    };
    if let Some(view_id) = target
        && let Err(message) = activate_view(hwnd, &view_id, None)
    {
        show_error_message(hwnd, "Window", &message);
    }
    true
}

/// Alt+X, mIRC's maximize toggle for the active window.
unsafe fn toggle_active_window_maximized(hwnd: HWND) {
    let mdi_client = state_ref(hwnd).controls.mdi_client;
    let child = SendMessageW(mdi_client, WM_MDIGETACTIVE, 0, 0) as HWND;
    assert!(!child.is_null(), "the Status window is always open");
    let message = if IsZoomed(child) != 0 {
        WM_MDIRESTORE
    } else {
        WM_MDIMAXIMIZE
    };
    SendMessageW(mdi_client, message, child as WPARAM, 0);
}

/// Ctrl+T: into the treebar, or back to the active window from it. With
/// the treebar hidden it does nothing, as in mIRC.
unsafe fn toggle_treebar_focus(hwnd: HWND) {
    let tree = state_ref(hwnd).controls.tree;
    if GetFocus() == tree {
        let mdi_client = state_ref(hwnd).controls.mdi_client;
        // The child hands the focus to its editbox.
        SetFocus(SendMessageW(mdi_client, WM_MDIGETACTIVE, 0, 0) as HWND);
    } else if IsWindowVisible(tree) != 0 {
        SetFocus(tree);
    }
}

pub(super) unsafe fn handle_switchbar_command(hwnd: HWND, wparam: WPARAM) -> bool {
    let command_id = wparam & 0xffff;
    if !(SWITCHBAR_COMMAND_FIRST..=SWITCHBAR_COMMAND_LAST).contains(&command_id) {
        return false;
    }

    let index = command_id - SWITCHBAR_COMMAND_FIRST;
    let Some(view_id) = state_ref(hwnd).switchbar_views.get(index).cloned() else {
        return false;
    };

    if let Err(message) = activate_view(hwnd, &view_id, None) {
        show_error_message(hwnd, "Switchbar", &message);
    }

    true
}

/// Returns whether the user logged in rather than cancelled.
unsafe fn show_select_server_dialog(hwnd: HWND, server: &str) -> Result<bool, String> {
    connection::check_sign_in(state_ref(hwnd).model.connection)?;
    let Some(form) = show_login_dialog(hwnd, server)? else {
        return Ok(false);
    };
    submit_login(
        state_mut(hwnd),
        form.server,
        form.username,
        form.password,
        form.device_display_name,
        true,
    )?;
    refresh_status_bar(hwnd)?;
    Ok(true)
}

/// File > Connect and `/server [server]`: the step that brings the account
/// online from where it is. Returns whether it ran rather than the user
/// cancelling Select Server.
pub(super) unsafe fn connect(hwnd: HWND, server: Option<&str>) -> Result<bool, String> {
    let action = connection::connect_action(
        state_ref(hwnd).model.connection,
        server,
        session_path()?.is_file(),
    )?;
    // The user's own Connect: retries start over.
    state_mut(hwnd).reconnect.reset();
    match action {
        ConnectAction::RestoreSavedSession => submit_restore_saved_session(state_mut(hwnd), true)?,
        ConnectAction::SelectServer { server } => return show_select_server_dialog(hwnd, &server),
        ConnectAction::StartSync => submit_start_sync(state_mut(hwnd))?,
        ConnectAction::RetryNow => {
            reconnect::cancel(hwnd);
            submit_start_sync(state_mut(hwnd))?;
        }
    }
    refresh_status_bar(hwnd)?;
    Ok(true)
}

/// File > Disconnect and `/quit`: stop syncing and keep the session, stop
/// a sign-in or a Log Out in progress, or stop retrying.
pub(super) unsafe fn disconnect(hwnd: HWND) -> Result<(), String> {
    let command = match connection::disconnect_action(state_ref(hwnd).model.connection)? {
        // The core stops the Log Out under way: `LogoutCancelled`.
        DisconnectAction::StopSync | DisconnectAction::StopLogOut => Command::StopSync,
        DisconnectAction::CancelSignIn => Command::CancelLogin,
        DisconnectAction::CancelReconnect => {
            reconnect::cancel(hwnd);
            // The core is idle already: nothing answers with `SyncStopped`.
            let state = state_mut(hwnd);
            log_status(state, StatusLineKind::Info, status_log::STOPPED.to_owned());
            state.model.status = "disconnected".to_owned();
            return sync_views_and_tree(hwnd);
        }
    };
    enqueue_command(state_mut(hwnd), command)
}

/// File > Log Out. The core may answer that room keys would be lost:
/// then `log_out_warning` asks.
pub(super) unsafe fn log_out(hwnd: HWND) -> Result<(), String> {
    submit_logout_after_any_retry(hwnd, false)
}

/// A due retry goes first: it would start sync again.
unsafe fn submit_logout_after_any_retry(hwnd: HWND, discard_room_keys: bool) -> Result<(), String> {
    if state_ref(hwnd).model.connection == ConnectionState::WaitingToReconnect {
        reconnect::cancel(hwnd);
    }
    submit_logout(state_mut(hwnd), discard_room_keys)
}

/// The Log Out dialog of the core's refusal (`WM_LOG_OUT_WARNING`): its
/// Log Out logs out all the same, its Cancel leaves the session as it is.
pub(super) unsafe fn log_out_warning(hwnd: HWND) -> Result<(), String> {
    // Posted once a refusal came; a Log Out that went on to log out before
    // this message's turn took the session, and the warning, with it.
    let Some(risk) = state_mut(hwnd).log_out_warning.take() else {
        return Ok(());
    };
    if show_log_out_dialog(hwnd, &risk)? {
        submit_logout_after_any_retry(hwnd, true)?;
    }
    Ok(())
}

/// A Commands item runs the slash command it stands for, prompting first
/// for its parameters; failures echo in the active window, as typed
/// commands' do. Returns whether it ran rather than the user cancelling.
unsafe fn run_commands_menu_item(
    hwnd: HWND,
    window: &ViewId,
    item: &menus::CommandsMenuItem,
) -> Result<bool, String> {
    let input = match item.prompt {
        Some(label) => {
            match show_single_field_dialog(hwnd, &commands_menu_title(item), label, "")? {
                Some(input) => input,
                None => return Ok(false),
            }
        }
        None => String::new(),
    };
    match commands::parse(&commands_menu_line(item, &input)) {
        Ok(command) => run_local_command(hwnd, window, command)?,
        Err(error) => report_command_error(hwnd, window, error)?,
    }
    Ok(true)
}

pub(super) unsafe fn show_recover_e2ee_menu_dialog(hwnd: HWND) -> Result<bool, String> {
    let Some(form) = show_recover_e2ee_dialog(hwnd)? else {
        return Ok(false);
    };
    submit_recover_e2ee(state_mut(hwnd), form.recovery_secret)?;
    refresh_status_bar(hwnd)?;
    Ok(true)
}

/// The dialog asks for what the session's sign-in confirms a reset with:
/// the account password, or nothing for a browser approval.
pub(super) unsafe fn show_reset_e2ee_menu_dialog(hwnd: HWND) -> Result<bool, String> {
    let auth = match &state_ref(hwnd).model.own_account {
        Some(account) => account.auth.clone(),
        None => return Err("not connected to server".to_owned()),
    };
    let Some(form) = show_reset_e2ee_dialog(hwnd, &auth)? else {
        return Ok(false);
    };
    submit_identity_reset(state_mut(hwnd), form.password)?;
    refresh_status_bar(hwnd)?;
    Ok(true)
}

/// Tools > Export/Import Room Keys... and `/e2ee export|import`: the
/// Save or Open dialog unless the file is given, then its passphrase, then
/// the command. Returns the status bar's feedback, `None` if either dialog
/// is cancelled.
pub(super) unsafe fn run_room_keys_dialogs(
    hwnd: HWND,
    transfer: RoomKeyTransfer,
    path: Option<String>,
) -> Result<Option<String>, String> {
    let title = room_keys_dialog_title(transfer);
    let path = match path {
        Some(path) => path,
        None => {
            let dialog = match transfer {
                RoomKeyTransfer::Export => FileDialog::Save {
                    file_name: "mirx-keys.txt",
                    folder: None,
                },
                RoomKeyTransfer::Import => FileDialog::Open,
            };
            match file_dialogs::choose_file(hwnd, title, file_dialogs::TEXT_FILES, dialog)? {
                Some(path) => path
                    .into_os_string()
                    .into_string()
                    .expect("the dialog's path came from a String"),
                None => return Ok(None),
            }
        }
    };
    let Some(passphrase) = show_room_keys_dialog(hwnd, transfer, &path)? else {
        return Ok(None);
    };
    submit_room_key_transfer(state_mut(hwnd), transfer, path, passphrase)?;
    Ok(Some(
        match transfer {
            RoomKeyTransfer::Export => "room key export submitted",
            RoomKeyTransfer::Import => "room key import submitted",
        }
        .to_owned(),
    ))
}

unsafe fn show_setup_e2ee_menu_dialog(hwnd: HWND) -> Result<bool, String> {
    let Some(form) = show_setup_e2ee_dialog(hwnd)? else {
        return Ok(false);
    };
    submit_setup_e2ee_recovery(
        state_mut(hwnd),
        form.passphrase,
        form.wait_for_backups_to_upload,
    )?;
    refresh_status_bar(hwnd)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;

    #[test]
    fn each_dialog_opens_from_its_menu_item_or_the_close_box() {
        for (dialog, gesture) in [
            (
                AutomationDialog::Options,
                DialogGesture::MenuCommand(MENU_ID_TOOLS_OPTIONS),
            ),
            (
                AutomationDialog::Aliases,
                DialogGesture::MenuCommand(MENU_ID_TOOLS_ALIASES),
            ),
            (
                AutomationDialog::AddressBook,
                DialogGesture::MenuCommand(MENU_ID_TOOLS_ADDRESS_BOOK),
            ),
            (
                AutomationDialog::SelectServer,
                DialogGesture::MenuCommand(MENU_ID_FILE_SELECT_SERVER),
            ),
            (
                AutomationDialog::About,
                DialogGesture::MenuCommand(MENU_ID_HELP_ABOUT),
            ),
            (
                AutomationDialog::RecoverE2ee,
                DialogGesture::MenuCommand(MENU_ID_RECOVER_E2EE),
            ),
            (
                AutomationDialog::SetupE2eeRecovery,
                DialogGesture::MenuCommand(MENU_ID_SETUP_E2EE_RECOVERY),
            ),
            (
                AutomationDialog::ResetE2ee,
                DialogGesture::MenuCommand(MENU_ID_RESET_E2EE),
            ),
            (
                AutomationDialog::CloseActiveView,
                DialogGesture::CloseActiveView,
            ),
            (
                AutomationDialog::ComposeReturn,
                DialogGesture::ComposeReturn,
            ),
        ] {
            assert_eq!(dialog_gesture(dialog), gesture, "{dialog:?}");
        }
        let DialogGesture::MenuCommand(join) = dialog_gesture(AutomationDialog::CommandsJoin)
        else {
            panic!("Join is a Commands menu item");
        };
        let item = commands_menu_item(join).expect("a Commands menu id");
        assert_eq!(item.command, "join");
        assert_eq!(commands_menu_title(item), "Join room");
    }

    #[test]
    fn timeline_row_is_selected_requires_a_selection() {
        assert!(!timeline_row_is_selected(None, None));
        assert!(!timeline_row_is_selected(Some("$a"), None));
        assert!(!timeline_row_is_selected(None, Some("$a")));
        assert!(!timeline_row_is_selected(Some("$b"), Some("$a")));
        assert!(timeline_row_is_selected(Some("$a"), Some("$a")));
    }

    #[test]
    fn channel_central_opens_for_a_joined_channel_while_signed_in() {
        let mut state = test_state();
        let mut query = room("!query:example", "Bob", false);
        query.is_direct = true;
        state.model.rooms = vec![
            invited_room("!invite:example", "Invite"),
            room("!ops:example", "Ops", false),
            query,
        ];
        state.active_view = room_view_id("!ops:example");
        assert_eq!(
            channel_central_room(&state, &state.active_view, None),
            Err("not connected to server".to_owned())
        );
        assert_eq!(double_click_action(&state, &state.active_view), None);

        state.model.own_account = Some(OwnAccount::matrix("@me:example".to_owned()));
        state.model.connection = ConnectionState::Syncing;
        assert_eq!(
            channel_central_room(&state, &state.active_view, None),
            Ok("!ops:example".to_owned())
        );
        assert_eq!(
            double_click_action(&state, &state.active_view),
            Some(DoubleClick::ChannelCentral)
        );
        assert_eq!(
            channel_central_room(&state, &state.active_view, Some("Ops")),
            Ok("!ops:example".to_owned())
        );
        for (target, refusal) in [
            (Some("Bob"), "Bob is a query, not a channel"),
            (Some("Invite"), "room is not joined: Invite"),
            (Some("Nowhere"), "unknown room: Nowhere"),
        ] {
            assert_eq!(
                channel_central_room(&state, &state.active_view, target),
                Err(refusal.to_owned()),
                "{target:?}"
            );
        }
        for (view, action) in [
            (room_view_id("!query:example"), None),
            (room_view_id("!invite:example"), None),
            // The Room menu acts on a thread's room; its double-click is
            // no channel window's.
            (thread_view_id("!ops:example", "$thread:example"), None),
            (ViewId::Status, None),
        ] {
            state.active_view = view.clone();
            assert_eq!(
                double_click_action(&state, &state.active_view),
                action,
                "{view:?}"
            );
        }
        state.active_view = thread_view_id("!ops:example", "$thread:example");
        assert!(active_room_action_state(&state).channel_central_enabled);
        state.active_view = ViewId::Status;
        assert_eq!(
            channel_central_room(&state, &state.active_view, None),
            Err("no room selected".to_owned())
        );
    }

    #[test]
    fn a_double_click_in_a_query_whois_its_user() {
        let mut state = test_state();
        let direct = |room_id: &str, users: &[&str]| RoomSummary {
            is_direct: true,
            direct_user_ids: users.iter().map(|user| (*user).to_owned()).collect(),
            ..room(room_id, "Direct", false)
        };
        state.model.rooms = vec![
            room("!ops:example", "Ops", false),
            direct("!bob:example", &["@bob:example"]),
            direct("!two:example", &["@bob:example", "@carol:example"]),
            direct("!nobody:example", &[]),
        ];
        state.model.own_account = Some(OwnAccount::matrix("@me:example".to_owned()));
        state.model.connection = ConnectionState::Syncing;
        for (view, action) in [
            (
                room_view_id("!ops:example"),
                Some(DoubleClick::ChannelCentral),
            ),
            (
                room_view_id("!bob:example"),
                Some(DoubleClick::Whois("@bob:example".to_owned())),
            ),
            // A direct room is one user's query only.
            (room_view_id("!two:example"), None),
            (room_view_id("!nobody:example"), None),
            (thread_view_id("!bob:example", "$thread:example"), None),
            (ViewId::Status, None),
        ] {
            state.active_view = view.clone();
            assert_eq!(
                double_click_action(&state, &state.active_view),
                action,
                "{view:?}"
            );
        }
        // Channel Central greyed: signed out.
        state.model.connection = ConnectionState::SignedOut;
        state.active_view = room_view_id("!ops:example");
        assert_eq!(double_click_action(&state, &state.active_view), None);
    }

    #[test]
    fn active_room_action_state_matches_active_invite_and_joined_views() {
        let mut state = test_state();
        state.model.rooms = vec![
            invited_room("!invite:example", "Invite"),
            room("!joined:example", "Joined", false),
        ];

        state.active_view = room_view_id("!invite:example");
        assert_eq!(
            active_room_action_state(&state),
            RoomActionState {
                accept_invite_enabled: true,
                reject_invite_enabled: true,
                part_room_enabled: false,
                channel_central_enabled: false,
                notification_mode: None,
            }
        );

        state.active_view = thread_view_id("!joined:example", "$thread:example");
        assert_eq!(
            active_room_action_state(&state),
            RoomActionState {
                accept_invite_enabled: false,
                reject_invite_enabled: false,
                part_room_enabled: true,
                // Not signed in.
                channel_central_enabled: false,
                notification_mode: Some(RoomNotificationMode::AllMessages),
            }
        );

        state.model.rooms[1].notification_mode = Some(RoomNotificationMode::Mute);
        state.active_view = room_view_id("!joined:example");
        assert_eq!(
            active_room_action_state(&state).notification_mode,
            Some(RoomNotificationMode::Mute)
        );

        state.active_view = ViewId::Status;
        assert_eq!(active_room_action_state(&state).notification_mode, None);
    }

    #[test]
    fn active_message_action_state_requires_a_selected_rendered_event() {
        let mut state = test_state();
        state.active_view = room_view_id("!room:example");
        assert_eq!(
            active_message_action_state(&state),
            MessageActionState {
                selected_event_id: None,
                edit_enabled: false,
                save_attachment_enabled: false,
                go_to_original_enabled: false,
            }
        );

        state.selected_timeline_event = Some(SelectedTimelineEvent {
            view_id: state.active_view.clone(),
            event_id: "$missing:example".to_owned(),
        });
        let rows = vec![TimelineRow::full_width(
            TimelineRowKind::Plain,
            "context".to_owned(),
        )];
        validate_selected_timeline_event(&mut state, &room_view_id("!room:example"), &rows);
        assert_eq!(
            active_message_action_state(&state),
            MessageActionState {
                selected_event_id: None,
                edit_enabled: false,
                save_attachment_enabled: false,
                go_to_original_enabled: false,
            }
        );

        let rows = vec![
            TimelineRow::log(
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                    own: false,
                },
                "[00:00] @alice: message".to_owned(),
            )
            .with_event_id(Some("$event:example")),
        ];
        state.selected_timeline_event = Some(SelectedTimelineEvent {
            view_id: state.active_view.clone(),
            event_id: "$event:example".to_owned(),
        });
        validate_selected_timeline_event(&mut state, &room_view_id("!room:example"), &rows);
        assert_eq!(
            active_message_action_state(&state),
            MessageActionState {
                selected_event_id: Some("$event:example".to_owned()),
                edit_enabled: false,
                save_attachment_enabled: false,
                go_to_original_enabled: false,
            }
        );
    }

    #[test]
    fn only_the_users_own_messages_in_the_window_can_be_targeted() {
        unsafe {
            with_compose_target_window(|hwnd| {
                state_mut(hwnd).selected_timeline_event = Some(SelectedTimelineEvent {
                    view_id: room_view_id("!ops:example"),
                    event_id: "$q".to_owned(),
                });
                assert!(!active_message_action_state(state_ref(hwnd)).edit_enabled);
                type_compose_line(hwnd, "/edit $q");
                type_compose_line(hwnd, "/reply $elsewhere");
                type_compose_line(hwnd, "/edit $q hijacked");
                // The test clock prints them before the fixture's later lines.
                let lines = active_timeline_lines(hwnd);
                assert_eq!(
                    lines
                        .iter()
                        .filter(|line| line.starts_with("[22:12] * /"))
                        .collect::<Vec<_>>(),
                    [
                        "[22:12] * /edit: you can only edit your own messages",
                        "[22:12] * /reply: no such message in this window",
                        "[22:12] * /edit: you can only edit your own messages",
                    ]
                );
                assert_eq!(active_compose_target(hwnd), None);
                assert!(state_ref(hwnd).recorded_commands.is_empty());
            });
        }
    }

    #[test]
    fn selected_file_and_audio_enable_attachment_saving() {
        for activity in [
            file_activity(
                "!room:example",
                "$file:example",
                "@file:example",
                0,
                "file",
                None,
                None,
            ),
            audio_activity(
                "!room:example",
                "$audio:example",
                "@audio:example",
                0,
                "audio",
                None,
                None,
            ),
        ] {
            let event_id = match &activity.kind {
                RoomActivityKind::File(file) => file.event_id.clone(),
                RoomActivityKind::Audio(audio) => audio.event_id.clone(),
                _ => unreachable!(),
            };
            let mut state = test_state();
            state.active_view = room_view_id("!room:example");
            state
                .model
                .room_views
                .entry_or_default("!room:example".to_owned())
                .activities
                .push(activity);
            state.selected_timeline_event = Some(SelectedTimelineEvent {
                view_id: state.active_view.clone(),
                event_id,
            });
            let menu_state = active_message_action_state(&state);
            assert!(menu_state.selected_event_id.is_some());
            assert!(menu_state.save_attachment_enabled);
        }
    }

    #[test]
    fn go_to_original_is_offered_only_for_a_reply_with_a_quote_line() {
        let mut state = reply_quote_state("$q");
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$t", "$q", "thread reply"),
        )
        .unwrap();
        state.active_view = room_view_id("!ops:example");
        let enabled = |state: &mut WindowState, view_id: ViewId, event_id: &str| {
            state.active_view = view_id.clone();
            state.selected_timeline_event = Some(SelectedTimelineEvent {
                view_id,
                event_id: event_id.to_owned(),
            });
            active_message_action_state(state).go_to_original_enabled
        };
        assert!(enabled(&mut state, room_view_id("!ops:example"), "$a"));
        assert!(!enabled(&mut state, room_view_id("!ops:example"), "$q"));
        // A thread message replying to its root has no quote line.
        assert!(!enabled(
            &mut state,
            thread_view_id("!ops:example", "$q"),
            "$t"
        ));
    }
}
