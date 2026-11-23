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

/// The member a room content window's nick list shows at `index`, looked
/// up by user ID in its room's members: the window's listing
/// (`MdiViewState::nick_list`) maps an item to its member.
fn listed_member(state: &WindowState, content: HWND, index: u32) -> &RoomMemberSummary {
    let view_id = content_view(state, content);
    let user_id = &state.views[view_id].nick_list[index as usize].user_id;
    room_member(state, view_id, user_id)
}

/// The window whose room content window is `content`.
fn content_view(state: &WindowState, content: HWND) -> &ViewId {
    state
        .views
        .iter()
        .find(|(_, view)| view.content == content)
        .map(|(view_id, _)| view_id)
        .expect("a nick list's window is a view's")
}

/// A double-click on a nick in the nick list of `content`, a room content
/// window, opens a query with it, mIRC's default (Options > Mouse >
/// Nicklist `/query $$1`); its first click made it the only selection.
/// The window need not be the active one. In an empty list it does
/// nothing.
pub(super) unsafe fn query_double_clicked_member(content: HWND) -> Result<(), String> {
    let frame = frame_window_for(content);
    let action = {
        let state = state_ref(frame);
        let members = selected_members(state, content_view(state, content));
        if members.is_empty() {
            return Ok(());
        }
        member_action(state, MENU_ID_MEMBER_QUERY, &members)
    };
    let window = content_view(state_ref(frame), content).clone();
    run_member_action(frame, &window, action)
}

/// A member of the room of `view_id`, a window with a nick list. The
/// listing is synced in the same pass that changes the members, so a
/// listed member is always one of them.
fn room_member<'a>(
    state: &'a WindowState,
    view_id: &ViewId,
    user_id: &str,
) -> &'a RoomMemberSummary {
    let room_id = view_id
        .room_id()
        .expect("a window with a nick list has a room");
    state
        .model
        .room_views
        .get(room_id)
        .and_then(|room_view| {
            room_view
                .members
                .iter()
                .find(|member| member.user_id == user_id)
        })
        .unwrap_or_else(|| panic!("listed member {user_id} is one of its room's members"))
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
            select_clicked_item(member_list, hit as usize);
            (x, y)
        }
        None => default_context_point_for_window(member_list)?,
    };
    // The list's window, which the menu's commands act in, even if the
    // menu's modal loop activates another.
    let window = content_view(state_ref(frame), GetParent(member_list)).clone();
    let menu = state_ref(frame).controls.member_menu;
    refresh_member_menu_state(frame, menu)?;
    let action_state = active_member_action_state(frame);
    if requested_command.is_some_and(|command| !action_state.enabled(command)) {
        return Err("member context menu action is disabled".to_owned());
    }
    match show_context_menu(frame, menu, x, y, requested_command)? {
        Some(command) => run_member_menu_item(frame, &window, command),
        None => Ok(()),
    }
}

/// A right-click on item `hit`: a nick not selected becomes the only
/// selection, a selected one keeps the whole selection for the menu, as
/// in Explorer and mIRC.
unsafe fn select_clicked_item(list: HWND, hit: usize) {
    let selected = SendMessageW(list, LB_GETSEL, hit, 0);
    assert_ne!(selected, LB_ERR as isize, "item {hit} is in the list");
    if selected == 0 {
        SendMessageW(list, LB_SETSEL, 0, -1);
        select_list_items(list, &[hit]);
        SendMessageW(list, LB_SETCARETINDEX, hit, 0);
    }
}

/// Runs a member menu item, picked in `window`, for the members its nick
/// list selects, taken when it runs: the menu's modal loop handles core
/// events, and a member who left meanwhile is no longer selected.
pub(super) unsafe fn run_member_menu_item(
    hwnd: HWND,
    window: &ViewId,
    id: usize,
) -> Result<(), String> {
    let action = {
        let state = state_ref(hwnd);
        member_action(state, id, &window_selected_members(state, window))
    };
    run_member_action(hwnd, window, action)
}

/// Runs what a member menu item does: commands as if typed (their
/// refusals and errors print where typed ones do), queries as Query User
/// opens them (the status bar shows the last), and nicks inserted at the
/// compose caret, replacing any selection, followed by a space to type on.
/// All of it in `window`, whose nick list it came from. An empty action had
/// its members leave while the menu was open.
unsafe fn run_member_action(
    hwnd: HWND,
    window: &ViewId,
    action: MemberMenuAction,
) -> Result<(), String> {
    if action.is_empty() {
        return Err("no member is selected".to_owned());
    }
    match action {
        MemberMenuAction::Commands(commands) => {
            for command in commands {
                run_local_command(hwnd, window, command)?;
            }
            Ok(())
        }
        MemberMenuAction::Query(user_ids) => {
            for user_id in user_ids {
                let status = submit_query_slash_command(state_mut(hwnd), user_id)?;
                state_mut(hwnd).model.status = status;
            }
            refresh_status_bar(hwnd)
        }
        MemberMenuAction::Mention(nicks) => {
            // The editbox takes the focus, which only the active window's can.
            if *window != state_ref(hwnd).active_view {
                return Err("a name can be inserted only in the active window".to_owned());
            }
            let edit = compose_edit(hwnd);
            active_compose_history(state_mut(hwnd)).reset_navigation();
            replace_edit_selection(edit, &format!("{} ", nicks.join(" ")));
            SetFocus(edit);
            Ok(())
        }
    }
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
    let index = active_listed_index(hwnd, user_id)?;
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

/// One item of a nick list: its member, and the text it shows (the rank
/// prefix and the label). Colours and presence are not here: they are
/// drawn from the model, so a change of them only repaints the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ListedMember {
    user_id: String,
    text: String,
}

/// What a nick list lists for `members`, in their order.
pub(super) fn nick_list_items(members: &[RoomMemberSummary]) -> Vec<ListedMember> {
    members
        .iter()
        .map(|member| ListedMember {
            user_id: member.user_id.clone(),
            text: format!("{}{}", rank_prefix(member.rank), room_member_nick(member)),
        })
        .collect()
}

/// The items of `items` whose members `user_ids` names, in list order.
fn restored_selection(items: &[ListedMember], user_ids: &[&str]) -> Vec<usize> {
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| user_ids.contains(&item.user_id.as_str()))
        .map(|(index, _)| index)
        .collect()
}

/// Brings the nick list `list`, which lists `shown`, to list `items`. It
/// is rebuilt only if they differ, keeping its selected members (by user
/// ID) and its scroll position; else it is left as it is. The caller
/// repaints it: colours and presence may have changed.
pub(super) unsafe fn sync_nick_list(
    list: HWND,
    shown: &mut Vec<ListedMember>,
    items: Vec<ListedMember>,
) {
    if *shown == items {
        return;
    }
    let selected = list_selection(list)
        .into_iter()
        .map(|index| shown[index].user_id.clone())
        .collect::<Vec<_>>();
    // The caret of an empty list is no item.
    let caret = usize::try_from(SendMessageW(list, LB_GETCARETINDEX, 0, 0))
        .ok()
        .and_then(|index| shown.get(index))
        .map(|item| item.user_id.clone());
    let top_index = SendMessageW(list, LB_GETTOPINDEX, 0, 0);
    SendMessageW(list, WM_SETREDRAW, 0, 0);
    SendMessageW(list, LB_RESETCONTENT, 0, 0);
    for item in &items {
        // In `items` order: items map to members by index, so the list
        // must not sort itself.
        let text = wide_null(&item.text);
        SendMessageW(list, LB_ADDSTRING, 0, text.as_ptr() as LPARAM);
    }
    let selected = selected.iter().map(String::as_str).collect::<Vec<_>>();
    select_list_items(list, &restored_selection(&items, &selected));
    if let Some(caret) = caret.and_then(|caret| items.iter().position(|item| item.user_id == caret))
    {
        SendMessageW(list, LB_SETCARETINDEX, caret, 0);
    }
    // After the caret, which scrolls to show itself.
    SendMessageW(list, LB_SETTOPINDEX, top_index as WPARAM, 0);
    SendMessageW(list, WM_SETREDRAW, 1, 0);
    *shown = items;
}

/// The items a nick list selects, in list order. It takes several
/// (`LBS_EXTENDEDSEL`), where `LB_GETCURSEL` names the caret item.
unsafe fn list_selection(list: HWND) -> Vec<usize> {
    let count = SendMessageW(list, LB_GETSELCOUNT, 0, 0);
    let count = usize::try_from(count).expect("the nick list takes several items");
    let mut items = vec![0i32; count];
    let copied = SendMessageW(list, LB_GETSELITEMS, count, items.as_mut_ptr() as LPARAM);
    assert_eq!(copied, count as isize, "the selected items are copied");
    items
        .into_iter()
        .map(|item| usize::try_from(item).expect("an item index"))
        .collect()
}

/// Adds `items` to a nick list's selection.
unsafe fn select_list_items(list: HWND, items: &[usize]) {
    for &item in items {
        let selected = SendMessageW(list, LB_SETSEL, 1, item as LPARAM);
        assert_ne!(selected, LB_ERR as isize, "item {item} is in the list");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MemberMenuActionState {
    /// Each member menu item, and whether it applies.
    items: Vec<(usize, bool)>,
    pub(super) selected_user_ids: Vec<String>,
    pub(super) selected_labels: Vec<String>,
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

/// The index of `user_id`'s item in the active window's nick list.
unsafe fn active_listed_index(hwnd: HWND, user_id: &str) -> Result<usize, String> {
    let state = state_ref(hwnd);
    state.views[&state.active_view]
        .nick_list
        .iter()
        .position(|item| item.user_id == user_id)
        .ok_or_else(|| format!("unknown_member: {user_id}"))
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

/// The members a window's nick list selects, in list order, through its
/// listing.
unsafe fn selected_members(state: &WindowState, view_id: &ViewId) -> Vec<RoomMemberSummary> {
    let view = &state.views[view_id];
    list_selection(room_content_member_list(view.content))
        .into_iter()
        .map(|index| room_member(state, view_id, &view.nick_list[index].user_id).clone())
        .collect()
}

/// The members `window`'s nick list selects; none in a window without one.
unsafe fn window_selected_members(state: &WindowState, window: &ViewId) -> Vec<RoomMemberSummary> {
    match window {
        ViewId::Status | ViewId::RoomList => Vec::new(),
        view_id @ (ViewId::Room(_) | ViewId::Thread { .. }) => selected_members(state, view_id),
    }
}

/// What member menu item `id` does for `members` now.
fn member_action(
    state: &WindowState,
    id: usize,
    members: &[RoomMemberSummary],
) -> MemberMenuAction {
    let model = &state.model;
    let identity_warned = members
        .iter()
        .filter(|member| identity::user_identity_warning(model, &member.user_id).is_some())
        .map(|member| member.user_id.as_str())
        .collect::<Vec<_>>();
    member_menu_action(
        id,
        members,
        &MemberMenuContext {
            ignored_users: model.ignored_users.as_deref(),
            own_user_id: model.own_user_id(),
            identity_warned: &identity_warned,
        },
    )
}

pub(super) unsafe fn active_member_action_state(hwnd: HWND) -> MemberMenuActionState {
    let state = state_ref(hwnd);
    let selected = window_selected_members(state, &state.active_view);
    MemberMenuActionState {
        items: member_menu_item_ids()
            .map(|id| (id, !member_action(state, id, &selected).is_empty()))
            .collect(),
        selected_user_ids: selected
            .iter()
            .map(|member| member.user_id.clone())
            .collect(),
        selected_labels: selected.iter().map(room_member_nick).collect(),
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

/// Selects only `user_id`'s nick in the active window, with the caret on
/// it.
pub(super) unsafe fn select_member_for_automation(hwnd: HWND, user_id: &str) -> Result<(), String> {
    let index = active_listed_index(hwnd, user_id)?;
    let list = active_member_list(hwnd).ok_or_else(|| "member list is unavailable".to_owned())?;
    SendMessageW(list, LB_SETSEL, 0, -1);
    select_list_items(list, &[index]);
    SendMessageW(list, LB_SETCARETINDEX, index, 0);
    assert_eq!(list_selection(list), [index], "only {user_id} is selected");
    Ok(())
}

#[cfg(test)]
mod tests {
    use mirx_core::{PresenceChange, RoomMemberRank};

    use super::super::test_fixtures::*;
    use super::*;

    #[test]
    fn the_nick_list_is_rebuilt_only_when_what_it_lists_changes() {
        let bob = member_summary("@bob:example", "Bob", false);
        let carol = member_summary("@carol:example", "Carol", false);
        let listed = nick_list_items(&[bob.clone(), carol.clone()]);
        // What it does not show: rights and whether it is you. Presence
        // and colours are not inputs at all: they are drawn.
        let unshown = RoomMemberSummary {
            can_kick: true,
            can_ban: true,
            is_own_user: true,
            ..bob.clone()
        };
        assert_eq!(nick_list_items(&[unshown, carol.clone()]), listed);
        let changes = [
            vec![
                RoomMemberSummary {
                    rank: RoomMemberRank::Voiced,
                    ..bob.clone()
                },
                carol.clone(),
            ],
            vec![
                member_summary("@bob:example", "Bobby", false),
                carol.clone(),
            ],
            // A nick shared with someone gains the user ID.
            vec![member_summary("@bob:example", "Bob", true), carol.clone()],
            vec![member_summary("@bob2:example", "Bob", false), carol.clone()],
            vec![carol.clone(), bob.clone()],
            vec![bob.clone()],
            vec![
                bob.clone(),
                carol.clone(),
                member_summary("@dan:example", "Dan", false),
            ],
        ];
        for members in changes {
            assert_ne!(nick_list_items(&members), listed, "{members:?}");
        }
    }

    #[test]
    fn a_rebuilt_nick_list_reselects_its_members_by_user_id() {
        let items = nick_list_items(&[
            member_summary("@amy:example", "Amy", false),
            member_summary("@bob:example", "Bob", false),
            member_summary("@carol:example", "Carol", false),
        ]);
        // Kept, moved (Carol was first), and gone (Dan left).
        assert_eq!(
            restored_selection(&items, &["@carol:example", "@dan:example", "@amy:example"]),
            vec![0, 2]
        );
        assert_eq!(restored_selection(&items, &[]), Vec::<usize>::new());
    }

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
