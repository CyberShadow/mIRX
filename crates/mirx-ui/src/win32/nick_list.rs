//! The nick list: owner draw, the selected member and its menu.

use super::colors::colorref_hex;
use super::settings::Settings;
use super::*;

pub(super) fn room_member_nick(member: &RoomMemberSummary) -> String {
    nick_label(&member.user_id, Some(&member.nick), member.nick_ambiguous)
}

pub(super) fn room_member_view_rows(
    members: &[RoomMemberSummary],
    presence: &PresenceBook,
    settings: &Settings,
) -> Vec<RoomMemberView> {
    members
        .iter()
        .map(|member| RoomMemberView {
            user_id: member.user_id.clone(),
            nick: member.nick.clone(),
            label: room_member_nick(member),
            rank: member.rank,
            can_kick: member.can_kick,
            can_ban: member.can_ban,
            presence: presence.kind(&member.user_id),
            color: colorref_hex(nick_list_color(member, presence, settings)),
        })
        .collect()
}

/// The colour a member's nick is drawn in when not selected: away and
/// offline members in grey, as HexChat shows them, whatever their nick
/// colour (presence says something, a colour only decorates); the others
/// in their nick colour, else the list's text colour.
pub(super) fn nick_list_color(
    member: &RoomMemberSummary,
    presence: &PresenceBook,
    settings: &Settings,
) -> u32 {
    let scheme = &settings.colors;
    match presence.kind(&member.user_id) {
        Some(PresenceKind::Unavailable | PresenceKind::Offline) => scheme.colorref(ColorItem::Gray),
        Some(PresenceKind::Online) | None => settings
            .nick_colors
            .resolve(
                &member.user_id,
                Some(member.rank),
                settings.notify.list.contains(&member.user_id),
            )
            .map_or(scheme.colorref(ColorItem::ListboxText), |color| {
                color.index(scheme).colorref()
            }),
    }
}

/// The member a room content window's nick list shows at `index`: the
/// list holds its room's members in order (`set_room_member_list`).
fn listed_member(state: &WindowState, content: HWND, index: u32) -> &RoomMemberSummary {
    let (view_id, _) = state
        .views
        .iter()
        .find(|(_, view)| view.content == content)
        .expect("a nick list's window is a view's");
    let room_id = view_id
        .room_id()
        .expect("a window with a nick list has a room");
    state
        .model
        .room_views
        .get(room_id)
        .and_then(|room_view| room_view.members.get(index as usize))
        .expect("a nick list item is one of its room's members")
}

/// A nick list item's height: a line of the frame's text font, as the
/// stock listbox sizes its items for its font.
pub(super) unsafe fn text_font_item_height(hwnd: HWND) -> u32 {
    let height = state_ref(frame_window_for(hwnd))
        .text_font
        .glyphs
        .metrics()
        .tmHeight;
    u32::try_from(height).expect("a font height is positive")
}

/// Draws an item of the nick list of `hwnd`, a room content window, like
/// the stock listbox does, with the text in the frame's text font and its
/// fallbacks, and the colour of the member's presence.
pub(super) unsafe fn draw_nick_list_item(hwnd: HWND, item: &DRAWITEMSTRUCT) {
    let hdc = item.hDC;
    // An empty list with the focus: only the focus rectangle.
    if item.itemID == u32::MAX {
        if item.itemState & ODS_FOCUS != 0 {
            DrawFocusRect(hdc, &item.rcItem);
        }
        return;
    }
    let frame_state = state_ref(frame_window_for(hwnd));
    // The selected nick in the system's highlight, as the stock listbox.
    let (background, text_color) = if item.itemState & ODS_SELECTED != 0 {
        (
            GetSysColorBrush(COLOR_HIGHLIGHT),
            GetSysColor(COLOR_HIGHLIGHTTEXT),
        )
    } else {
        let member = listed_member(frame_state, hwnd, item.itemID);
        (
            frame_state.scheme_brushes.listbox,
            nick_list_color(
                member,
                &frame_state.model.presence,
                frame_state.settings.settings(),
            ),
        )
    };
    FillRect(hdc, &item.rcItem, background);
    let previous_color = SetTextColor(hdc, text_color);
    let previous_mode = SetBkMode(hdc, TRANSPARENT as i32);

    let length = SendMessageW(item.hwndItem, LB_GETTEXTLEN, item.itemID as WPARAM, 0);
    let length = usize::try_from(length).expect("a nick list item has text");
    let mut text = vec![0u16; length + 1];
    SendMessageW(
        item.hwndItem,
        LB_GETTEXT,
        item.itemID as WPARAM,
        text.as_mut_ptr() as LPARAM,
    );
    let text = String::from_utf16(&text[..length]).expect("nick list items are UTF-16 text");
    let glyphs = &frame_state.text_font.glyphs;
    glyphs.draw(
        hdc,
        item.rcItem.left + 1,
        item.rcItem.top + glyphs.metrics().tmAscent,
        &item.rcItem,
        &text,
        false,
    );

    SetBkMode(hdc, previous_mode);
    SetTextColor(hdc, previous_color);
    if item.itemState & ODS_FOCUS != 0 {
        DrawFocusRect(hdc, &item.rcItem);
    }
}

pub(super) unsafe fn show_member_context_menu(
    frame: HWND,
    member_list: HWND,
    lparam: LPARAM,
    requested_command: Option<usize>,
) -> Result<(), String> {
    let (x, y) = match context_menu_screen_point_for_lparam(lparam) {
        Some((x, y)) => {
            let mut point = POINT { x, y };
            if ScreenToClient(member_list, &mut point) == 0 {
                return Err(last_os_error("ScreenToClient"));
            }
            let hit = SendMessageW(
                member_list,
                LB_ITEMFROMPOINT,
                0,
                ((point.y as u32 as usize) << 16 | point.x as u16 as usize) as LPARAM,
            );
            if (hit as usize & 0xffff_0000) != 0 {
                return Ok(());
            }
            if SendMessageW(member_list, LB_SETCURSEL, hit as usize, 0) == LB_ERR as isize {
                return Ok(());
            }
            (x, y)
        }
        None => default_context_point_for_window(member_list)?,
    };
    let menu = state_ref(frame).controls.member_menu;
    refresh_member_menu_state(frame, menu)?;
    let action_state = active_member_action_state(frame);
    if requested_command.is_some_and(|command| !action_state.enabled(command)) {
        return Err("member context menu action is disabled".to_owned());
    }
    if let Some(command) = show_context_menu(frame, menu, x, y, requested_command)?
        && let Some(result) = handle_menu_command(frame, command)
    {
        result?;
    }
    Ok(())
}

pub(super) unsafe fn query_selected_member(hwnd: HWND) -> Result<(), String> {
    let member = selected_member(hwnd).ok_or_else(|| "no member is selected".to_owned())?;
    let status = submit_query_slash_command(state_mut(hwnd), member.user_id)?;
    state_mut(hwnd).model.status = status;
    refresh_status_bar(hwnd)
}

/// Inserts the selected member's nick at the compose caret, replacing any
/// selection, followed by a space to type on.
pub(super) unsafe fn mention_selected_member(hwnd: HWND) -> Result<(), String> {
    let member = selected_member(hwnd).ok_or_else(|| "no member is selected".to_owned())?;
    let edit = compose_edit(hwnd);
    active_compose_history(state_mut(hwnd)).reset_navigation();
    replace_edit_selection(edit, &format!("{} ", member.nick));
    SetFocus(edit);
    Ok(())
}

/// Runs a member menu item's command on the selected member, as if typed:
/// its refusals and errors print where typed ones do.
pub(super) unsafe fn run_selected_member_command(hwnd: HWND, id: usize) -> Result<(), String> {
    let member = selected_member(hwnd).ok_or_else(|| "no member is selected".to_owned())?;
    let command = member_menu_command(id, &member.user_id)
        .unwrap_or_else(|| panic!("member menu item {id} runs no command"));
    run_local_command(hwnd, command)
}

pub(super) unsafe fn invoke_member_context_menu_for_automation(
    hwnd: HWND,
    user_id: &str,
    command: &ContextMenuCommand,
) -> Result<(), String> {
    let list = active_member_list(hwnd).ok_or_else(|| "member list is unavailable".to_owned())?;
    let lparam = member_context_lparam_for_automation(hwnd, list, user_id)?;
    let command = member_context_menu_command_id(command)?;
    show_member_context_menu(hwnd, list, lparam, Some(command))
}

pub(super) unsafe fn invoke_selected_member_context_menu_for_automation(
    hwnd: HWND,
    command: &ContextMenuCommand,
) -> Result<(), String> {
    let command = member_context_menu_command_id(command)?;
    let list = active_member_list(hwnd).ok_or_else(|| "member list is unavailable".to_owned())?;
    show_member_context_menu(hwnd, list, -1, Some(command))
}

fn member_context_menu_command_id(command: &ContextMenuCommand) -> Result<usize, String> {
    context_menu_command_id(command)
        .filter(|id| member_menu_item_ids().any(|member_id| member_id == *id))
        .ok_or_else(|| "member context menu command is unavailable".to_owned())
}

unsafe fn member_context_lparam_for_automation(
    hwnd: HWND,
    list: HWND,
    user_id: &str,
) -> Result<LPARAM, String> {
    let index = active_member_rows(hwnd)
        .iter()
        .position(|member| member.user_id == user_id)
        .ok_or_else(|| format!("unknown_member: {user_id}"))?;
    let mut rect = RECT::default();
    if SendMessageW(
        list,
        LB_GETITEMRECT,
        index,
        &mut rect as *mut RECT as LPARAM,
    ) == LB_ERR as isize
    {
        return Err("member context menu target is unavailable".to_owned());
    }
    let mut point = POINT {
        x: (rect.left + rect.right) / 2,
        y: (rect.top + rect.bottom) / 2,
    };
    if ClientToScreen(list, &mut point) == 0 {
        return Err(last_os_error("ClientToScreen"));
    }
    Ok(((point.y as u32 as usize) << 16 | (point.x as u16 as usize)) as LPARAM)
}

/// Fills the nick list in `rows` order: rows are looked up by list index, so
/// the list must not sort itself.
pub(super) unsafe fn set_room_member_list(hwnd: HWND, rows: &[RoomMemberView]) {
    SendMessageW(hwnd, LB_RESETCONTENT, 0, 0);
    for row in rows {
        let label = wide_null(&format!("{}{}", rank_prefix(row.rank), row.label));
        SendMessageW(hwnd, LB_ADDSTRING, 0, label.as_ptr() as LPARAM);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MemberMenuActionState {
    /// Each member menu item, and whether it applies.
    items: Vec<(usize, bool)>,
    pub(super) selected_user_id: Option<String>,
    pub(super) selected_label: Option<String>,
}

impl MemberMenuActionState {
    pub(super) fn enabled(&self, id: usize) -> bool {
        self.items
            .iter()
            .find(|(item, _)| *item == id)
            .unwrap_or_else(|| panic!("{id} is not a member menu item"))
            .1
    }
}

/// The members of the active view's room, in nick list order; a thread
/// shares its room's members and the status view has none.
pub(super) fn active_view_members(state: &WindowState) -> &[RoomMemberSummary] {
    let room_id = match &state.active_view {
        ViewId::Room(room_id) | ViewId::Thread { room_id, .. } => room_id,
        ViewId::Status | ViewId::RoomList => return &[],
    };
    state
        .model
        .room_views
        .get(room_id)
        .map(|view| view.members.as_slice())
        .unwrap_or_default()
}

unsafe fn active_member_rows(hwnd: HWND) -> Vec<RoomMemberView> {
    let state = state_ref(hwnd);
    room_member_view_rows(
        active_view_members(state),
        &state.model.presence,
        state.settings.settings(),
    )
}

unsafe fn active_member_list(hwnd: HWND) -> Option<HWND> {
    match &state_ref(hwnd).active_view {
        ViewId::Status | ViewId::RoomList => None,
        ViewId::Room(_) | ViewId::Thread { .. } => state_ref(hwnd)
            .views
            .get(&state_ref(hwnd).active_view)
            .map(|view| room_content_member_list(view.content)),
    }
}

/// The nick list's selected member: the list shows the active view's
/// members in order.
unsafe fn selected_member(hwnd: HWND) -> Option<RoomMemberSummary> {
    let list = active_member_list(hwnd)?;
    let selected = SendMessageW(list, LB_GETCURSEL, 0, 0);
    if selected == LB_ERR as isize {
        return None;
    }
    active_view_members(state_ref(hwnd))
        .get(selected as usize)
        .cloned()
}

pub(super) unsafe fn active_member_action_state(hwnd: HWND) -> MemberMenuActionState {
    let selected = selected_member(hwnd);
    let model = &state_ref(hwnd).model;
    MemberMenuActionState {
        items: member_menu_item_ids()
            .map(|id| {
                let enabled = selected.as_ref().is_some_and(|member| {
                    member_menu_item_enabled(
                        id,
                        member,
                        model.ignored_users.as_deref(),
                        model.own_user_id(),
                        identity::user_identity_warning(model, &member.user_id).is_some(),
                    )
                });
                (id, enabled)
            })
            .collect(),
        selected_user_id: selected.as_ref().map(|member| member.user_id.clone()),
        selected_label: selected.as_ref().map(room_member_nick),
    }
}

pub(super) unsafe fn refresh_member_menu_state(
    hwnd: HWND,
    popup_menu: HMENU,
) -> Result<(), String> {
    let action_state = active_member_action_state(hwnd);
    for (item, enabled) in action_state.items {
        let flags = MF_BYCOMMAND | if enabled { MF_ENABLED } else { MF_GRAYED };
        if EnableMenuItem(popup_menu, item as u32, flags) == -1 {
            return Err(last_os_error("EnableMenuItem"));
        }
    }
    Ok(())
}

pub(super) unsafe fn select_member_for_automation(hwnd: HWND, user_id: &str) -> Result<(), String> {
    let index = active_member_rows(hwnd)
        .iter()
        .position(|member| member.user_id == user_id)
        .ok_or_else(|| format!("unknown_member: {user_id}"))?;
    let list = active_member_list(hwnd).ok_or_else(|| "member list is unavailable".to_owned())?;
    let selected = SendMessageW(list, LB_GETCURSEL, 0, 0);
    if selected == LB_ERR as isize {
        if SendMessageW(list, LB_SETCURSEL, index, 0) == LB_ERR as isize {
            return Err("member selection failed".to_owned());
        }
    } else {
        let key = if index > selected as usize {
            VK_DOWN_KEY
        } else {
            VK_UP_KEY
        };
        for _ in 0..index.abs_diff(selected as usize) {
            SendMessageW(list, WM_KEYDOWN, key, 0);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use mirx_core::{PresenceChange, RoomMemberRank};

    use super::super::test_fixtures::*;
    use super::*;

    #[test]
    fn member_view_rows_disambiguate_by_the_core_ambiguity_flag() {
        let labels = room_member_view_rows(
            &[
                member_summary("@alex:example", "Alex", true),
                member_summary("@bo1:example", "Bo", false),
                member_summary("@bo2:example", "Bo", false),
                // The core names a member without a display name by localpart.
                member_summary("@nameless:example", "nameless", false),
                member_summary("@ghost:example", "ghost", true),
            ],
            &PresenceBook::default(),
            &Settings::default(),
        )
        .into_iter()
        .map(|member| member.label)
        .collect::<Vec<_>>();

        assert_eq!(
            labels,
            vec![
                "Alex (@alex:example)",
                "Bo",
                "Bo",
                "nameless",
                "ghost (@ghost:example)"
            ]
        );
    }

    #[test]
    fn member_view_rows_keep_core_order_and_carry_rank_beside_the_bare_label() {
        let ranked = |user_id: &str, name: &str, rank| RoomMemberSummary {
            rank,
            ..member_summary(user_id, name, false)
        };
        let rows = room_member_view_rows(
            &[
                ranked("@zed:example", "Zed", RoomMemberRank::Moderator),
                ranked("@amy:example", "Amy", RoomMemberRank::Voiced),
            ],
            &PresenceBook::default(),
            &Settings::default(),
        );

        assert_eq!(
            rows.iter()
                .map(|row| (row.label.as_str(), row.rank))
                .collect::<Vec<_>>(),
            vec![
                ("Zed", RoomMemberRank::Moderator),
                ("Amy", RoomMemberRank::Voiced)
            ]
        );
    }

    /// The list draws `@mod`: the op prefix, then the nick, as on IRC.
    #[test]
    fn a_nameless_moderator_reads_as_an_op_named_by_localpart() {
        let rows = room_member_view_rows(
            &[RoomMemberSummary {
                rank: RoomMemberRank::Moderator,
                ..member_summary("@mod:example", "mod", false)
            }],
            &PresenceBook::default(),
            &Settings::default(),
        );
        assert_eq!(
            rows.iter()
                .map(|row| (row.label.as_str(), row.rank))
                .collect::<Vec<_>>(),
            vec![("mod", RoomMemberRank::Moderator)]
        );
    }

    #[test]
    fn member_view_rows_carry_each_members_presence() {
        let mut presence = PresenceBook::default();
        presence.apply(vec![PresenceChange {
            user_id: "@bob:example".to_owned(),
            kind: PresenceKind::Unavailable,
            status_msg: Some("lunch".to_owned()),
        }]);
        let rows = room_member_view_rows(
            &[
                member_summary("@bob:example", "Bob", false),
                member_summary("@erin:example", "Erin", false),
            ],
            &presence,
            &Settings::default(),
        );
        assert_eq!(
            rows.iter()
                .map(|row| (row.label.as_str(), row.presence))
                .collect::<Vec<_>>(),
            vec![("Bob", Some(PresenceKind::Unavailable)), ("Erin", None)]
        );
    }

    #[test]
    fn away_greys_a_nick_over_its_colour_and_a_colour_beats_the_list_text() {
        use super::super::nick_colors::NickColorEntry;
        let mut presence = PresenceBook::default();
        presence.apply(
            [
                ("@away:example", PresenceKind::Unavailable),
                ("@off:example", PresenceKind::Offline),
                ("@on:example", PresenceKind::Online),
            ]
            .map(|(user_id, kind)| PresenceChange {
                user_id: user_id.to_owned(),
                kind,
                status_msg: None,
            })
            .into(),
        );
        let ranked = |user_id: &str, rank| RoomMemberSummary {
            rank,
            ..member_summary(user_id, "x", false)
        };
        let members = [
            ranked("@away:example", RoomMemberRank::Moderator),
            ranked("@off:example", RoomMemberRank::Moderator),
            ranked("@on:example", RoomMemberRank::Moderator),
            ranked("@unknown:example", RoomMemberRank::Moderator),
            ranked("@member:example", RoomMemberRank::Member),
        ];
        let colors = |settings: &Settings| {
            room_member_view_rows(&members, &presence, settings)
                .into_iter()
                .map(|row| row.color)
                .collect::<Vec<_>>()
        };
        let (grey, black, red) = ("#7f7f7f", "#000000", "#ff0000");
        let mut settings = Settings::default();
        assert_eq!(colors(&settings), [grey, grey, black, black, black]);
        settings
            .nick_colors
            .list
            .push(NickColorEntry::parse_ini_value("* 4 any").unwrap());
        assert_eq!(colors(&settings), [grey, grey, red, red, black]);
        settings.nick_colors.active = false;
        assert_eq!(colors(&settings), [grey, grey, black, black, black]);
    }
}
