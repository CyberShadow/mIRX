//! Automation dumps of the UI state, the model and the menus.

use super::*;

pub(super) unsafe fn dump_member_menu_state(hwnd: HWND) -> Result<DumpMemberMenuState, String> {
    let menu = state_ref(hwnd).controls.member_menu;
    if menu.is_null() {
        return Err("Member menu is not available".to_owned());
    }
    refresh_member_menu_state(hwnd, menu)?;
    let state = active_member_action_state(hwnd);
    Ok(DumpMemberMenuState {
        whois_enabled: state.enabled(MENU_ID_MEMBER_WHOIS),
        query_enabled: state.enabled(MENU_ID_MEMBER_QUERY),
        mention_enabled: state.enabled(MENU_ID_MEMBER_MENTION),
        ignore_enabled: state.enabled(MENU_ID_MEMBER_IGNORE),
        unignore_enabled: state.enabled(MENU_ID_MEMBER_UNIGNORE),
        op_enabled: state.enabled(MENU_ID_MEMBER_OP),
        deop_enabled: state.enabled(MENU_ID_MEMBER_DEOP),
        voice_enabled: state.enabled(MENU_ID_MEMBER_VOICE),
        devoice_enabled: state.enabled(MENU_ID_MEMBER_DEVOICE),
        kick_enabled: state.enabled(MENU_ID_MEMBER_KICK),
        ban_enabled: state.enabled(MENU_ID_MEMBER_BAN),
        selected_user_id: state.selected_user_id,
        selected_label: state.selected_label,
    })
}

pub(super) unsafe fn dump_verification_menu_state(
    hwnd: HWND,
) -> Result<DumpVerificationMenuState, String> {
    refresh_tools_menu_state(hwnd, state_ref(hwnd).controls.tools_menu)?;

    let last_verification = state_ref(hwnd).model.last_verification.as_ref();
    Ok(DumpVerificationMenuState {
        accept_enabled: verification_menu_action_enabled(
            last_verification,
            VerificationAction::Accept,
        ),
        cancel_enabled: verification_menu_action_enabled(
            last_verification,
            VerificationAction::Cancel,
        ),
        confirm_sas_enabled: verification_menu_action_enabled(
            last_verification,
            VerificationAction::Confirm,
        ),
        reject_sas_enabled: verification_menu_action_enabled(
            last_verification,
            VerificationAction::Reject,
        ),
    })
}

pub(super) unsafe fn dump_room_menu_state(hwnd: HWND) -> Result<DumpRoomMenuState, String> {
    let room_menu = state_ref(hwnd).controls.room_menu;
    if room_menu.is_null() {
        return Err("Room menu is not available".to_owned());
    }
    refresh_menu_popup(hwnd, room_menu)?;
    let action_state = active_room_action_state(state_ref(hwnd));
    // Notifications as its own WM_INITMENUPOPUP leaves it, read back from
    // the menu.
    let notifications_menu = state_ref(hwnd).controls.room_notifications_menu;
    refresh_menu_popup(hwnd, notifications_menu)?;
    let mut notification_mode = None;
    let mut enabled_items = 0;
    for (mode, item_id, _) in ROOM_NOTIFICATIONS_MENU {
        let item_state = GetMenuState(notifications_menu, item_id as u32, MF_BYCOMMAND);
        if item_state == u32::MAX {
            return Err(format!("GetMenuState: menu item {item_id} does not exist"));
        }
        if item_state & MF_CHECKED != 0 {
            if notification_mode.is_some() {
                return Err("Notifications checks more than one mode".to_owned());
            }
            notification_mode = Some(mode);
        }
        if item_state & MF_GRAYED == 0 {
            enabled_items += 1;
        }
    }
    let notification_items_enabled = match enabled_items {
        0 => false,
        3 => true,
        _ => return Err("Notifications greys only some of its items".to_owned()),
    };
    Ok(DumpRoomMenuState {
        accept_invite_enabled: action_state.accept_invite_enabled,
        reject_invite_enabled: action_state.reject_invite_enabled,
        part_room_enabled: action_state.part_room_enabled,
        notification_mode,
        notification_items_enabled,
    })
}

pub(super) unsafe fn dump_message_menu_state(hwnd: HWND) -> Result<DumpMessageMenuState, String> {
    let message_menu = state_ref(hwnd).controls.message_menu;
    if message_menu.is_null() {
        return Err("Message menu is not available".to_owned());
    }
    refresh_message_menu_state(hwnd, message_menu)?;
    let action_state = active_message_action_state_for_automation(hwnd);
    let enabled = action_state.selected_event_id.is_some();
    Ok(DumpMessageMenuState {
        reply_enabled: enabled,
        edit_enabled: action_state.edit_enabled,
        react_enabled: enabled,
        redact_enabled: enabled,
        save_attachment_enabled: action_state.save_attachment_enabled,
        go_to_original_enabled: action_state.go_to_original_enabled,
        selected_event_id: action_state.selected_event_id,
    })
}

unsafe fn dump_status_bar_state(hwnd: HWND) -> Result<StatusBarState, String> {
    if state_ref(hwnd).controls.status_bar.is_null() {
        return Err("status bar control was not created".to_owned());
    }

    Ok(StatusBarState {
        text: frame_status_text(state_ref(hwnd)),
    })
}

pub(super) unsafe fn active_view_identity(hwnd: HWND) -> Result<ViewIdentity, String> {
    let mdi_client = state_ref(hwnd).controls.mdi_client;
    if mdi_client.is_null() {
        return Err("MDI client control was not created".to_owned());
    }

    let active_child = SendMessageW(mdi_client, WM_MDIGETACTIVE, 0, 0) as HWND;
    if !active_child.is_null() {
        let view_id = state_ref(hwnd)
            .hwnd_views
            .get(&(active_child as isize))
            .cloned()
            .ok_or_else(|| {
                format!(
                    "active MDI child 0x{:x} was not tracked",
                    active_child as usize
                )
            })?;
        return Ok(view_identity_from_view_id(&view_id));
    }

    let active_view = &state_ref(hwnd).active_view;
    if state_ref(hwnd).views.contains_key(active_view) {
        Ok(view_identity_from_view_id(active_view))
    } else {
        Err("active view was not tracked".to_owned())
    }
}

unsafe fn dump_mdi_state(hwnd: HWND) -> Result<MdiState, String> {
    let active = active_view_identity(hwnd)?;
    let view_ids = ordered_view_ids(state_ref(hwnd));
    let views = view_ids
        .into_iter()
        .map(|view_id| {
            let view = state_ref(hwnd)
                .views
                .get(&view_id)
                .ok_or_else(|| "view state was not tracked".to_owned())?;
            if !cached_view_is_alive(view) {
                return Err(format!(
                    "tracked view {:?} referenced a dead child or content control",
                    view_id
                ));
            }

            let identity = view_identity_from_view_id(&view_id);
            Ok(MdiView {
                title: read_window_text(view.child),
                active: identity == active,
                placement: mdi_window_placement(state_ref(hwnd).controls.mdi_client, view.child),
                // The Rooms List's rows are in `UiState.room_list`.
                lines: match &view_id {
                    ViewId::RoomList => Vec::new(),
                    ViewId::Status | ViewId::Room(_) | ViewId::Thread { .. } => {
                        split_rendered_lines(&read_window_text(view_timeline(
                            &view_id,
                            view.content,
                        )))
                    }
                },
                members: match &view_id {
                    ViewId::Status | ViewId::RoomList => Vec::new(),
                    ViewId::Room(room_id) => room_by_id(&state_ref(hwnd).model.rooms, room_id)
                        .filter(|room| is_sendable_room(room))
                        .map(|_| {
                            room_member_view_rows(
                                state_ref(hwnd)
                                    .model
                                    .room_views
                                    .get(room_id)
                                    .map(|room_view| room_view.members.as_slice())
                                    .unwrap_or_default(),
                            )
                        })
                        .unwrap_or_default(),
                    ViewId::Thread { room_id, .. } => room_member_view_rows(
                        state_ref(hwnd)
                            .model
                            .room_views
                            .get(room_id)
                            .map(|room_view| room_view.members.as_slice())
                            .unwrap_or_default(),
                    ),
                },
                identity,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(MdiState { active, views })
}

unsafe fn dump_switchbar_state(hwnd: HWND) -> Result<SwitchbarState, String> {
    if state_ref(hwnd).controls.switchbar.is_null() {
        return Err("switchbar control was not created".to_owned());
    }

    let active = active_view_identity(hwnd)?;
    let state = state_ref(hwnd);
    let switchbar = state.controls.switchbar;
    let items = state
        .switchbar_views
        .iter()
        .enumerate()
        .map(|(index, view_id)| {
            let mut rect = RECT::default();
            if SendMessageW(
                switchbar,
                TB_GETITEMRECT,
                index,
                &mut rect as *mut RECT as LPARAM,
            ) == 0
            {
                return Err(format!("failed to read switchbar button {index}"));
            }
            Ok(SwitchbarItem {
                identity: view_identity_from_view_id(view_id),
                title: switchbar_button_text(state, view_id),
                active: view_identity_from_view_id(view_id) == active,
                activity: state.room_activity.get(view_id).copied(),
                rect: ui_rect(rect),
            })
        })
        .collect::<Result<_, String>>()?;
    let rows = u32::try_from(SendMessageW(switchbar, TB_GETROWS, 0, 0))
        .expect("a toolbar's row count is positive");

    Ok(SwitchbarState { rows, items })
}

fn ui_rect(rect: RECT) -> UiRect {
    UiRect {
        x: rect.left,
        y: rect.top,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    }
}

/// A child window's rectangle in the frame's client coordinates, or `None`
/// while it is hidden.
pub(super) unsafe fn frame_child_rect(hwnd: HWND, child: HWND) -> Option<UiRect> {
    // Its own style: `IsWindowVisible` also asks whether the frame is shown.
    if GetWindowLongPtrW(child, GWL_STYLE) as u32 & WS_VISIBLE == 0 {
        return None;
    }
    let mut rect = RECT::default();
    GetWindowRect(child, &mut rect);
    MapWindowPoints(null_mut(), hwnd, (&mut rect as *mut RECT).cast(), 2);
    Some(ui_rect(rect))
}

pub(super) unsafe fn dump_frame_layout(hwnd: HWND) -> UiFrameLayout {
    let controls = &state_ref(hwnd).controls;
    UiFrameLayout {
        switchbar: frame_child_rect(hwnd, controls.switchbar),
        treebar: frame_child_rect(hwnd, controls.tree),
        mdi_client: frame_child_rect(hwnd, controls.mdi_client)
            .expect("the MDI client is always shown"),
    }
}

/// The text font as the active window really draws it: the face GDI
/// selected for the timeline's font, and the heights laid out from it.
unsafe fn dump_text_font(state: &WindowState, view_id: &ViewId, view: &MdiViewState) -> UiTextFont {
    let timeline = view_timeline(view_id, view.content);
    let timeline_state = timeline_state_ref(timeline);
    let hdc = GetDC(timeline);
    assert!(!hdc.is_null(), "GetDC must succeed for the timeline");
    let previous = SelectObject(hdc, timeline_state.font);
    let mut face = [0u16; 32];
    let face_len = GetTextFaceW(hdc, face.len() as i32, face.as_mut_ptr());
    SelectObject(hdc, previous);
    ReleaseDC(timeline, hdc);
    assert!(face_len > 0, "GetTextFaceW must name the timeline font");
    let face_len = face
        .iter()
        .position(|&unit| unit == 0)
        .expect("GetTextFaceW NUL-terminates the face");
    let mut edit_rect = RECT::default();
    GetWindowRect(mdi_child_edit(view.child), &mut edit_rect);
    let setting = &state.settings.settings().text_font;
    UiTextFont {
        face: String::from_utf16_lossy(&face[..face_len]),
        points: setting.points(),
        bold: setting.bold(),
        line_height: timeline_state.line_height,
        edit_height: edit_rect.bottom - edit_rect.top,
        fallback_faces: state
            .text_font
            .glyphs
            .faces()
            .skip(1)
            .map(str::to_owned)
            .collect(),
    }
}

unsafe fn system_menu_labels(child: HWND) -> Vec<String> {
    let menu = GetSystemMenu(child, 0);
    let count = GetMenuItemCount(menu);
    assert!(count >= 0, "GetMenuItemCount must read the window menu");
    (0..count as u32)
        .filter_map(|position| {
            let mut label = [0u16; 128];
            let len = GetMenuStringW(
                menu,
                position,
                label.as_mut_ptr(),
                label.len() as i32,
                MF_BYPOSITION,
            );
            // Separators have no label.
            (len > 0).then(|| String::from_utf16_lossy(&label[..len as usize]))
        })
        .collect()
}

/// A menu item's type, state, label and submenu.
unsafe fn menu_item_info(menu: HMENU, position: u32) -> Result<(MENUITEMINFOW, String), String> {
    let mut label = [0u16; 128];
    let mut info = MENUITEMINFOW {
        cbSize: size_of::<MENUITEMINFOW>() as u32,
        fMask: MIIM_FTYPE | MIIM_STRING | MIIM_SUBMENU | MIIM_STATE,
        dwTypeData: label.as_mut_ptr(),
        cch: label.len() as u32,
        ..MENUITEMINFOW::default()
    };
    if GetMenuItemInfoW(menu, position, 1, &mut info) == 0 {
        return Err(last_os_error("GetMenuItemInfoW"));
    }
    let text = String::from_utf16_lossy(&label[..info.cch as usize]);
    Ok((info, text))
}

/// The menubar as opening each menu would show it. A maximized MDI child
/// puts its window menu icon and its minimize, restore and close buttons
/// into the bar as bitmap items, which are not menus.
unsafe fn dump_menu_bar(hwnd: HWND) -> Result<Vec<UiMenu>, String> {
    let bar = GetMenu(hwnd);
    let mut menus = Vec::new();
    for position in 0..menu_item_count(bar)? {
        let (info, label) = menu_item_info(bar, position)?;
        if info.fType & MFT_BITMAP != 0 {
            continue;
        }
        refresh_menu_popup(hwnd, info.hSubMenu)?;
        let mut items = Vec::new();
        for item_position in 0..menu_item_count(info.hSubMenu)? {
            let (item, item_label) = menu_item_info(info.hSubMenu, item_position)?;
            if item.fType & MFT_SEPARATOR != 0 {
                continue;
            }
            items.push(UiMenuItem {
                label: item_label,
                enabled: item.fState & MFS_DISABLED == 0,
                checked: item.fState & MFS_CHECKED != 0,
            });
        }
        menus.push(UiMenu { label, items });
    }
    Ok(menus)
}

unsafe fn menu_item_count(menu: HMENU) -> Result<u32, String> {
    u32::try_from(GetMenuItemCount(menu)).map_err(|_| last_os_error("GetMenuItemCount"))
}

/// The lines `timeline` shows now, with the click point of each: the
/// arithmetic `timeline_source_row_index_at_client_y` inverts.
unsafe fn dump_active_timeline(timeline: HWND) -> UiActiveTimeline {
    let state = timeline_state_ref(timeline);
    let visible_rows = timeline_visible_rows(timeline);
    let visible_lines = state
        .paint_lines
        .iter()
        .enumerate()
        .skip(state.top_line)
        .take(visible_rows)
        .map(|(line_index, line)| {
            let (text, styles) = match &line.visual {
                TimelinePaintLineVisual::Text { text, styles, .. } => {
                    (text.clone(), styles.clone())
                }
                TimelinePaintLineVisual::ImageSlot => (String::new(), Vec::new()),
            };
            UiTimelineLine {
                text,
                styles,
                kind: ui_timeline_line_kind(&line.kind),
                // Text set as a whole (WM_SETTEXT) has no rows.
                event_id: line
                    .source_row_index
                    .and_then(|index| state.source_rows[index].event_id.clone()),
                click_point: UiPoint {
                    x: 8,
                    y: state.vertical_padding
                        + (line_index - state.top_line) as i32 * state.line_height
                        + state.line_height / 2,
                },
            }
        })
        .collect();
    UiActiveTimeline {
        top_line: state.top_line,
        visible_lines,
    }
}

fn ui_timeline_line_kind(kind: &TimelineRowKind) -> UiTimelineLineKind {
    match kind {
        TimelineRowKind::Event => UiTimelineLineKind::Event,
        TimelineRowKind::Info => UiTimelineLineKind::Info,
        TimelineRowKind::Error => UiTimelineLineKind::Error,
        TimelineRowKind::Typing => UiTimelineLineKind::Typing,
        TimelineRowKind::ReactionSummary => UiTimelineLineKind::ReactionSummary,
        TimelineRowKind::ReplyQuote => UiTimelineLineKind::ReplyQuote,
        TimelineRowKind::Message { .. } => UiTimelineLineKind::Message,
        TimelineRowKind::Image { .. } => UiTimelineLineKind::Image,
        TimelineRowKind::File { .. } => UiTimelineLineKind::File,
        TimelineRowKind::Audio { .. } => UiTimelineLineKind::Audio,
        TimelineRowKind::Video { .. } => UiTimelineLineKind::Video,
        TimelineRowKind::Unsupported { .. } => UiTimelineLineKind::Unsupported,
        TimelineRowKind::UnableToDecrypt => UiTimelineLineKind::UnableToDecrypt,
        TimelineRowKind::StateNotice { .. } => UiTimelineLineKind::StateNotice,
        TimelineRowKind::Plain => UiTimelineLineKind::Plain,
    }
}

pub(super) unsafe fn dump_ui_state(hwnd: HWND) -> Result<UiState, String> {
    let navigation = dump_navigation_tree(hwnd)?;
    let mdi = dump_mdi_state(hwnd)?;
    let status_bar = dump_status_bar_state(hwnd)?;
    let switchbar = dump_switchbar_state(hwnd)?;
    let frame_layout = dump_frame_layout(hwnd);
    let state = state_ref(hwnd);
    let active_view = state
        .views
        .get(&state.active_view)
        .expect("the active view has a window");
    let timestamp = &state.settings.settings().timestamp;

    // The text font is the frame's, the same in every window; the Rooms
    // List has no timeline or editbox to measure it by, Status always does.
    let text_font_view = match &state.active_view {
        ViewId::RoomList => (&ViewId::Status, &state.views[&ViewId::Status]),
        view_id => (view_id, active_view),
    };

    Ok(UiState {
        text_font: dump_text_font(state, text_font_view.0, text_font_view.1),
        timestamp: UiTimestamp {
            show: timestamp.show,
            format: timestamp.format.to_string(),
        },
        active_system_menu_items: system_menu_labels(active_view.child),
        menu_bar: dump_menu_bar(hwnd)?,
        compose_target: active_view
            .compose
            .as_ref()
            .and_then(|compose| compose.target.as_ref())
            .map(|target| UiComposeTarget {
                kind: match target {
                    ComposeTarget::Reply { .. } => UiComposeTargetKind::Reply,
                    ComposeTarget::Edit { .. } => UiComposeTargetKind::Edit,
                },
                event_id: target.event_id().to_owned(),
            }),
        room_list: state
            .views
            .get(&ViewId::RoomList)
            .map(|view| dump_room_list(view.content)),
        options_dialog: state
            .options_dialog
            .map(|dialog| options_dialog::dump(dialog))
            .transpose()?,
        ignored_users: state.model.ignored_users.clone(),
        active_timeline: match &state.active_view {
            ViewId::RoomList => None,
            view_id => Some(dump_active_timeline(view_timeline(
                view_id,
                active_view.content,
            ))),
        },
        navigation,
        mdi,
        status_bar,
        switchbar,
        frame_layout,
        active_find: dump_active_find_state(state),
        rooms: state
            .model
            .rooms
            .iter()
            .map(|room| UiRoomState {
                history_backfill: state
                    .model
                    .room_views
                    .get(&room.room_id)
                    .map_or(UiRoomHistoryBackfillState::Idle, |room_view| {
                        dump_ui_room_history_backfill_state(&room_view.history)
                    }),
                room_id: room.room_id.clone(),
                display_name: room.display_name.clone(),
                membership: room.membership,
                encrypted: room.encrypted,
                is_space: room.is_space,
                is_direct: room.is_direct,
                direct_user_ids: room.direct_user_ids.clone(),
                tags: room
                    .tags
                    .iter()
                    .map(|tag| crate::automation::DumpRoomTag {
                        tag: tag.tag.clone(),
                        order: tag.order.clone(),
                    })
                    .collect(),
                space_children: room
                    .space_children
                    .iter()
                    .map(|child| crate::automation::DumpSpaceChild {
                        room_id: child.room_id.clone(),
                        suggested: child.suggested,
                        order: child.order.clone(),
                    })
                    .collect(),
                activity: state
                    .room_activity
                    .get(&ViewId::Room(room.room_id.clone()))
                    .copied(),
                read_target_event_id: state
                    .room_activity_read_targets
                    .get(&ViewId::Room(room.room_id.clone()))
                    .cloned(),
                read_state: state
                    .model
                    .room_read_states
                    .get(&room.room_id)
                    .map(dump_ui_room_read_state_from_core),
            })
            .collect(),
    })
}

fn dump_ui_room_history_backfill_state(
    history: &RoomHistoryBackfillState,
) -> UiRoomHistoryBackfillState {
    match &history.status {
        RoomHistoryBackfillStatus::Idle => UiRoomHistoryBackfillState::Idle,
        RoomHistoryBackfillStatus::Loading { from } => {
            UiRoomHistoryBackfillState::Loading { from: from.clone() }
        }
        RoomHistoryBackfillStatus::Failed { from, message } => UiRoomHistoryBackfillState::Failed {
            from: from.clone(),
            message: message.clone(),
        },
        RoomHistoryBackfillStatus::Exhausted => UiRoomHistoryBackfillState::Exhausted,
    }
}

fn dump_submitted_membership_command(command: &Command) -> Option<DumpSubmittedMembershipCommand> {
    match command {
        Command::JoinRoom(request) => Some(DumpSubmittedMembershipCommand {
            action: DumpMembershipCommandAction::Join,
            target: request.room_or_alias.clone(),
        }),
        Command::AcceptRoomInvite(request) => Some(DumpSubmittedMembershipCommand {
            action: DumpMembershipCommandAction::AcceptInvite,
            target: request.room_id.clone(),
        }),
        Command::LeaveRoom(request) => Some(DumpSubmittedMembershipCommand {
            action: DumpMembershipCommandAction::Leave,
            target: request.room_id.clone(),
        }),
        Command::RejectRoomInvite(request) => Some(DumpSubmittedMembershipCommand {
            action: DumpMembershipCommandAction::RejectInvite,
            target: request.room_id.clone(),
        }),
        _ => None,
    }
}

fn dump_submitted_member_moderation_command(
    command: &Command,
) -> Option<DumpSubmittedMemberModerationCommand> {
    match command {
        Command::KickRoomMember(request) => Some(DumpSubmittedMemberModerationCommand {
            action: DumpMemberModerationAction::Kick,
            room_id: request.room_id.clone(),
            user_id: request.user_id.clone(),
            reason: request.reason.clone(),
        }),
        Command::BanRoomMember(request) => Some(DumpSubmittedMemberModerationCommand {
            action: DumpMemberModerationAction::Ban,
            room_id: request.room_id.clone(),
            user_id: request.user_id.clone(),
            reason: request.reason.clone(),
        }),
        Command::UnbanRoomMember(request) => Some(DumpSubmittedMemberModerationCommand {
            action: DumpMemberModerationAction::Unban,
            room_id: request.room_id.clone(),
            user_id: request.user_id.clone(),
            reason: request.reason.clone(),
        }),
        Command::InviteRoomMember(request) => Some(DumpSubmittedMemberModerationCommand {
            action: DumpMemberModerationAction::Invite,
            room_id: request.room_id.clone(),
            user_id: request.user_id.clone(),
            reason: None,
        }),
        _ => None,
    }
}

fn submitted_power_level_commands_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedPowerLevelCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(|command| match command {
            Command::SetRoomMemberPowerLevel(request) => Some(DumpSubmittedPowerLevelCommand {
                room_id: request.room_id.clone(),
                user_id: request.user_id.clone(),
                role: match request.role {
                    MemberPowerRole::Moderator => DumpMemberPowerRole::Moderator,
                    MemberPowerRole::Voiced => DumpMemberPowerRole::Voiced,
                    MemberPowerRole::Default => DumpMemberPowerRole::Default,
                },
            }),
            _ => None,
        })
        .collect()
}

fn submitted_profile_commands_from_state(state: &WindowState) -> Vec<DumpSubmittedProfileCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(|command| match command {
            Command::SetDisplayName(request) => Some(DumpSubmittedProfileCommand {
                display_name: request.display_name.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn submitted_user_commands_from_state(state: &WindowState) -> Vec<DumpSubmittedUserCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(|command| {
            let (action, user_id) = match command {
                Command::IgnoreUser(user_id) => (DumpUserCommandAction::Ignore, user_id),
                Command::UnignoreUser(user_id) => (DumpUserCommandAction::Unignore, user_id),
                Command::FetchUserProfile(user_id) => {
                    (DumpUserCommandAction::FetchProfile, user_id)
                }
                _ => return None,
            };
            Some(DumpSubmittedUserCommand {
                action,
                user_id: user_id.clone(),
            })
        })
        .collect()
}

fn submitted_member_moderation_commands_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedMemberModerationCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_member_moderation_command)
        .collect()
}

fn dump_submitted_direct_room_command(command: &Command) -> Option<DumpSubmittedDirectRoomCommand> {
    match command {
        Command::CreateDirectRoom(request) => Some(DumpSubmittedDirectRoomCommand {
            user_id: request.user_id.clone(),
        }),
        _ => None,
    }
}

fn dump_submitted_room_topic_command(command: &Command) -> Option<DumpSubmittedRoomTopicCommand> {
    match command {
        Command::SetRoomTopic(request) => Some(DumpSubmittedRoomTopicCommand {
            room_id: request.room_id.clone(),
            topic: request.topic.clone(),
        }),
        _ => None,
    }
}

fn dump_submitted_reaction_command(command: &Command) -> Option<DumpSubmittedReactionCommand> {
    match command {
        Command::SendReaction(request) => Some(DumpSubmittedReactionCommand {
            room_id: request.room_id.clone(),
            target_event_id: request.target_event_id.clone(),
            key: request.key.clone(),
        }),
        _ => None,
    }
}

fn dump_submitted_edit_command(command: &Command) -> Option<DumpSubmittedEditCommand> {
    match command {
        Command::EditMessage(request) => Some(DumpSubmittedEditCommand {
            room_id: request.room_id.clone(),
            target_event_id: request.target_event_id.clone(),
            body: request.body.clone(),
            styles: request.styles.clone(),
        }),
        _ => None,
    }
}

fn dump_submitted_reply_command(command: &Command) -> Option<DumpSubmittedReplyCommand> {
    match command {
        Command::SendMessage(request) => {
            request
                .reply
                .as_ref()
                .map(|reply| DumpSubmittedReplyCommand {
                    room_id: request.room_id.clone(),
                    target_event_id: reply.event_id.clone(),
                    sender_user_id: reply.sender_user_id.clone(),
                    body: request.body.clone(),
                    styles: request.styles.clone(),
                    thread: request.thread.as_ref().map(dump_submitted_thread_target),
                })
        }
        _ => None,
    }
}

fn dump_submitted_thread_target(
    thread: &SendMessageThreadTarget,
) -> DumpSubmittedReplyThreadTarget {
    DumpSubmittedReplyThreadTarget {
        thread_root_event_id: thread.thread_root_event_id.clone(),
        latest_event_id: thread.latest_event_id.clone(),
    }
}

fn dump_submitted_message_command(command: &Command) -> Option<DumpSubmittedMessageCommand> {
    match command {
        Command::SendMessage(request) if request.reply.is_none() => {
            Some(DumpSubmittedMessageCommand {
                room_id: request.room_id.clone(),
                presentation: request.presentation,
                body: request.body.clone(),
                styles: request.styles.clone(),
                thread: request.thread.as_ref().map(dump_submitted_thread_target),
            })
        }
        _ => None,
    }
}

fn dump_submitted_file_command(command: &Command) -> Option<DumpSubmittedFileCommand> {
    match command {
        Command::SendFile(request) => Some(DumpSubmittedFileCommand {
            room_id: request.room_id.clone(),
            path: request
                .path
                .to_str()
                .expect("file paths come from UTF-16 text")
                .to_owned(),
        }),
        _ => None,
    }
}

fn dump_submitted_redaction_command(command: &Command) -> Option<DumpSubmittedRedactionCommand> {
    match command {
        Command::RedactEvent(request) => Some(DumpSubmittedRedactionCommand {
            room_id: request.room_id.clone(),
            event_id: request.event_id.clone(),
        }),
        _ => None,
    }
}

fn dump_submitted_history_command(command: &Command) -> Option<DumpSubmittedHistoryCommand> {
    match command {
        Command::BackfillRoomHistory(request) => Some(DumpSubmittedHistoryCommand {
            room_id: request.room_id.clone(),
            from: request.from.clone(),
            limit: request.limit,
        }),
        _ => None,
    }
}

fn submitted_membership_commands_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedMembershipCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_membership_command)
        .collect()
}

fn submitted_room_list_requests_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedRoomListRequest> {
    state
        .recorded_commands
        .iter()
        .filter_map(|command| match command {
            Command::ListPublicRooms(request) => Some(DumpSubmittedRoomListRequest {
                server: request.server.clone(),
                filter: request.filter.clone(),
                since: request.since.clone(),
                limit: request.limit,
            }),
            _ => None,
        })
        .collect()
}

fn submitted_direct_room_commands_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedDirectRoomCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_direct_room_command)
        .collect()
}

fn submitted_room_topic_commands_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedRoomTopicCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_room_topic_command)
        .collect()
}

fn submitted_reaction_commands_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedReactionCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_reaction_command)
        .collect()
}

fn submitted_edit_commands_from_state(state: &WindowState) -> Vec<DumpSubmittedEditCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_edit_command)
        .collect()
}

fn submitted_reply_commands_from_state(state: &WindowState) -> Vec<DumpSubmittedReplyCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_reply_command)
        .collect()
}

fn submitted_message_commands_from_state(state: &WindowState) -> Vec<DumpSubmittedMessageCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_message_command)
        .collect()
}

fn dump_submitted_notification_command(
    command: &Command,
) -> Option<DumpSubmittedNotificationCommand> {
    match command {
        Command::SetRoomNotificationMode(request) => {
            Some(DumpSubmittedNotificationCommand::SetRoomNotificationMode {
                room_id: request.room_id.clone(),
                mode: request.mode,
            })
        }
        Command::AddHighlightWord(word) => {
            Some(DumpSubmittedNotificationCommand::AddHighlightWord { word: word.clone() })
        }
        Command::RemoveHighlightWord(word) => {
            Some(DumpSubmittedNotificationCommand::RemoveHighlightWord { word: word.clone() })
        }
        _ => None,
    }
}

fn submitted_notification_commands_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedNotificationCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_notification_command)
        .collect()
}

fn submitted_file_commands_from_state(state: &WindowState) -> Vec<DumpSubmittedFileCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_file_command)
        .collect()
}

fn submitted_redaction_commands_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedRedactionCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_redaction_command)
        .collect()
}

fn submitted_history_commands_from_state(state: &WindowState) -> Vec<DumpSubmittedHistoryCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_history_command)
        .collect()
}

fn dump_submitted_verification_action(
    command: &Command,
) -> Option<DumpSubmittedVerificationAction> {
    let (action, flow) = match command {
        Command::AcceptVerification(flow) => (DumpVerificationActionKind::Accept, flow),
        Command::CancelVerification(flow) => (DumpVerificationActionKind::Cancel, flow),
        Command::ConfirmVerificationSas(flow) => (DumpVerificationActionKind::ConfirmSas, flow),
        Command::RejectVerificationSas(flow) => (DumpVerificationActionKind::RejectSas, flow),
        _ => return None,
    };
    Some(DumpSubmittedVerificationAction {
        action,
        flow_id: flow.flow_id.clone(),
        user_id: flow.user_id.clone(),
    })
}

fn submitted_verification_actions_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedVerificationAction> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_verification_action)
        .collect()
}

fn dump_submitted_recovery_command(command: &Command) -> Option<DumpSubmittedRecoveryCommand> {
    match command {
        Command::RecoverE2ee(_) => Some(DumpSubmittedRecoveryCommand {
            secret_redacted: true,
        }),
        _ => None,
    }
}

fn submitted_recovery_commands_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedRecoveryCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_recovery_command)
        .collect()
}

fn submitted_setup_recovery_commands_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedSetupRecoveryCommand> {
    state
        .submitted_setup_e2ee_recovery
        .iter()
        .map(|command| DumpSubmittedSetupRecoveryCommand {
            passphrase_redacted: command.passphrase_redacted,
            wait_for_backups_to_upload: command.wait_for_backups_to_upload,
        })
        .collect()
}

pub(super) fn dump_model_from_state(state: &WindowState) -> DumpModel {
    let model = &state.model;
    DumpModel {
        status: model.status.clone(),
        lifecycle_state: model.lifecycle_state,
        connection: model.connection,
        rooms: model.rooms.iter().map(dump_room_from_summary).collect(),
        last_activity: model
            .last_activity
            .as_ref()
            .map(dump_room_message_activity_line_from_last_activity),
        e2ee_status: model.last_e2ee_status.clone(),
        last_e2ee_recovery_setup_result: model.last_e2ee_recovery_setup_result.then_some(
            DumpE2eeRecoverySetupResult {
                recovery_key_present: true,
            },
        ),
        last_verification: model
            .last_verification
            .as_ref()
            .map(dump_verification_from_state_event),
        submitted_membership_commands: submitted_membership_commands_from_state(state),
        submitted_member_moderation_commands: submitted_member_moderation_commands_from_state(
            state,
        ),
        submitted_power_level_commands: submitted_power_level_commands_from_state(state),
        submitted_profile_commands: submitted_profile_commands_from_state(state),
        submitted_user_commands: submitted_user_commands_from_state(state),
        submitted_direct_room_commands: submitted_direct_room_commands_from_state(state),
        submitted_room_topic_commands: submitted_room_topic_commands_from_state(state),
        submitted_reaction_commands: submitted_reaction_commands_from_state(state),
        submitted_edit_commands: submitted_edit_commands_from_state(state),
        submitted_reply_commands: submitted_reply_commands_from_state(state),
        submitted_message_commands: submitted_message_commands_from_state(state),
        submitted_file_commands: submitted_file_commands_from_state(state),
        submitted_notification_commands: submitted_notification_commands_from_state(state),
        submitted_redaction_commands: submitted_redaction_commands_from_state(state),
        submitted_history_commands: submitted_history_commands_from_state(state),
        submitted_verification_actions: submitted_verification_actions_from_state(state),
        submitted_recovery_commands: submitted_recovery_commands_from_state(state),
        submitted_setup_recovery_commands: submitted_setup_recovery_commands_from_state(state),
        submitted_room_list_requests: submitted_room_list_requests_from_state(state),
        last_error: model.last_error.as_ref().map(dump_error_from_core),
        selected_room: model.selected_room.clone(),
    }
}

pub(super) fn dump_room_activity_entry_from_room_entry(
    room_view: &RoomViewModel,
    activity: &RoomActivity,
) -> DumpRoomActivity {
    let mut dump = dump_room_activity_from_activity(activity);
    if let Some(event_id) = room_activity_event_id(activity) {
        dump.reactions = room_view.reaction_summaries_for_event(event_id);
    }
    match (&activity.kind, &mut dump.kind) {
        (RoomActivityKind::Image(image), DumpRoomActivityKind::Image(dump_image)) => {
            dump_image.media_state = dump_room_preview_media_state(
                room_view,
                &activity.activity_id,
                &image.event_id,
                &timeline_thumbnail_format(),
            );
        }
        (RoomActivityKind::Video(video), DumpRoomActivityKind::Video(dump_video)) => {
            dump_video.media_state = dump_room_preview_media_state(
                room_view,
                &activity.activity_id,
                &video.event_id,
                &timeline_thumbnail_format(),
            );
        }
        _ => {}
    }
    dump
}

fn dump_room_preview_media_state(
    room_view: &RoomViewModel,
    activity_id: &str,
    event_id: &str,
    format: &RoomMediaFormat,
) -> Option<DumpRoomPreviewMediaState> {
    match room_view.preview_media.get(activity_id) {
        Some(RoomPreviewMediaState::Pending {
            event_id: stored_event_id,
            format: stored_format,
        }) if stored_event_id == event_id && stored_format == format => {
            Some(DumpRoomPreviewMediaState::Pending)
        }
        Some(RoomPreviewMediaState::Loaded {
            event_id: stored_event_id,
            format: stored_format,
            width,
            height,
            ..
        }) if stored_event_id == event_id && stored_format == format => {
            Some(DumpRoomPreviewMediaState::Loaded {
                width: *width,
                height: *height,
            })
        }
        Some(RoomPreviewMediaState::Failed {
            event_id: stored_event_id,
            format: stored_format,
            message,
        }) if stored_event_id == event_id && stored_format == format => {
            Some(DumpRoomPreviewMediaState::Failed {
                message: message.clone(),
            })
        }
        _ => None,
    }
}

fn dump_room_message_activity_line_from_last_activity(
    activity: &LastMessageActivity,
) -> DumpRoomMessageActivityLine {
    DumpRoomMessageActivityLine {
        room_id: activity.room_id.clone(),
        activity_id: activity
            .event_id
            .as_ref()
            .map(|event_id| format!("event:{event_id}"))
            .or_else(|| {
                activity
                    .transaction_id
                    .as_ref()
                    .map(|transaction_id| format!("txn:{transaction_id}"))
            })
            .expect("last activity message must have an event id or transaction id"),
        timestamp_ms: activity.timestamp_ms,
        event_id: activity.event_id.clone(),
        transaction_id: activity.transaction_id.clone(),
        sender: activity.sender.clone(),
        presentation: activity.presentation,
        body: activity.body.clone(),
        send_state: dump_room_message_send_state_from_core(&activity.send_state),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;

    #[test]
    fn dump_room_activity_entry_attaches_reactions_without_creating_pseudo_entries() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event-1:example.org",
                None,
                "first",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event-2:example.org",
                None,
                "second",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$event-1:example.org",
                "$reaction-1",
                "@bob:example.org",
                "heart",
                1_700_000_000_200,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$event-1:example.org",
                "$reaction-2",
                "@carol:example.org",
                "heart",
                1_700_000_000_300,
            ),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        let dumps = room_view
            .activities
            .iter()
            .map(|activity| dump_room_activity_entry_from_room_entry(room_view, activity))
            .collect::<Vec<_>>();

        assert_eq!(dumps.len(), room_view.activities.len());
        assert_eq!(
            dumps
                .iter()
                .map(|dump| dump.activity_id.clone())
                .collect::<Vec<_>>(),
            room_view
                .activities
                .iter()
                .map(|activity| activity.activity_id.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            dumps[0].reactions,
            vec![DumpRoomReactionSummary {
                key: "heart".to_owned(),
                count: 2,
                senders: vec![
                    "@bob:example.org".to_owned(),
                    "@carol:example.org".to_owned(),
                ],
                reaction_event_ids: vec!["$reaction-1".to_owned(), "$reaction-2".to_owned()],
            }]
        );
        assert!(dumps[1].reactions.is_empty());
    }

    #[test]
    fn dump_ui_room_history_backfill_state_projects_all_variants() {
        assert_eq!(
            dump_ui_room_history_backfill_state(&RoomHistoryBackfillState::default()),
            UiRoomHistoryBackfillState::Idle
        );
        assert_eq!(
            dump_ui_room_history_backfill_state(&RoomHistoryBackfillState {
                next_token: None,
                status: RoomHistoryBackfillStatus::Loading {
                    from: Some("token-1".to_owned()),
                },
            }),
            UiRoomHistoryBackfillState::Loading {
                from: Some("token-1".to_owned()),
            }
        );
        assert_eq!(
            dump_ui_room_history_backfill_state(&RoomHistoryBackfillState {
                next_token: Some("token-2".to_owned()),
                status: RoomHistoryBackfillStatus::Failed {
                    from: Some("token-1".to_owned()),
                    message: "homeserver unavailable".to_owned(),
                },
            }),
            UiRoomHistoryBackfillState::Failed {
                from: Some("token-1".to_owned()),
                message: "homeserver unavailable".to_owned(),
            }
        );
        assert_eq!(
            dump_ui_room_history_backfill_state(&RoomHistoryBackfillState {
                next_token: None,
                status: RoomHistoryBackfillStatus::Exhausted,
            }),
            UiRoomHistoryBackfillState::Exhausted
        );
    }
}
