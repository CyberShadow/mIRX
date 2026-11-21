//! The treebar and switchbar: layout, labels, activity colours and
//! selection.

use super::*;

/// A room's name, as the treebar, switchbar and caption show it. An invite
/// is marked, since mIRC has no invite windows to tell it by.
fn room_tree_label(room: &RoomSummary) -> String {
    if is_invited_room(room) {
        format!("{} [invite]", room.display_name)
    } else {
        room.display_name.clone()
    }
}

fn space_tree_label(space: &RoomSummary) -> String {
    space.display_name.clone()
}

/// The Rooms List's switchbar and treebar label.
pub(super) const ROOM_LIST_TITLE: &str = "Rooms List";

/// The MDI window caption: the short title, with the topic for channels and
/// the other user's ID for queries. A query ignores its room's topic, as
/// mIRC queries have none.
pub(super) fn view_caption(model: &UiModel, view_id: &ViewId) -> String {
    let title = view_short_title(model, view_id);
    match view_id {
        ViewId::Room(room_id) => {
            let room = room_by_id(&model.rooms, room_id)
                .expect("room view caption must target an existing room");
            match (room_window_kind(room), room.direct_user_ids.as_slice()) {
                (RoomWindowKind::Query, [user_id]) => mdi::query_caption(&title, user_id),
                (RoomWindowKind::Query, _) => title,
                (RoomWindowKind::Channel | RoomWindowKind::Invite, _) => mdi::room_caption(
                    &title,
                    model
                        .room_views
                        .get(room_id)
                        .and_then(|room_view| room_view.topic.as_deref()),
                ),
            }
        }
        ViewId::Status => mdi::status_caption(model.own_user_id.as_deref()),
        ViewId::Thread { .. } => title,
        ViewId::RoomList => model
            .room_list
            .as_ref()
            .expect("the Rooms List window has its listing")
            .caption(),
    }
}

/// The short title the switchbar and treebar show: the window's name only,
/// as in mIRC, where unread state shows by colour alone.
pub(super) fn view_short_title(model: &UiModel, view_id: &ViewId) -> String {
    if let Some((room_id, thread_root_event_id)) = view_id.thread_key() {
        return format!(
            "Thread: {}",
            projected_thread_by_id(model, room_id, thread_root_event_id)
                .expect("thread view activation must target an existing projected thread")
                .label
        );
    }

    match view_id {
        // mIRC labels a connection's Status window with its network; a
        // Matrix user ID names both the account and its server.
        ViewId::Status => model
            .own_user_id
            .clone()
            .unwrap_or_else(|| "Status".to_owned()),
        ViewId::Room(room_id) => {
            let room = room_by_id(&model.rooms, room_id)
                .expect("room view activation must target an existing room");
            room_tree_label(room)
        }
        ViewId::Thread { .. } => unreachable!("thread views are handled above"),
        ViewId::RoomList => ROOM_LIST_TITLE.to_owned(),
    }
}

fn switchbar_button_state(state: &WindowState, view_id: &ViewId) -> u8 {
    let mut button_state = TBSTATE_ENABLED as u8;
    if state.active_view == *view_id {
        button_state |= TBSTATE_CHECKED as u8;
    }
    button_state
}

/// A view's activity, which its switchbar button and treebar items show.
fn view_activity(state: &WindowState, view_id: &ViewId) -> Option<ViewActivityLevel> {
    let level = state.room_activity.get(view_id)?;
    assert_ne!(
        *view_id, state.active_view,
        "the active view never records activity"
    );
    Some(*level)
}

/// The highest activity of `views`, for a collapsed treebar item.
fn rolled_up_activity<'a>(
    state: &WindowState,
    views: impl IntoIterator<Item = &'a ViewId>,
) -> Option<ViewActivityLevel> {
    views
        .into_iter()
        .filter_map(|view_id| view_activity(state, view_id))
        .max()
}

/// The activity a treebar item is drawn with: its own view's, or for a
/// collapsed item the highest of its own and everything under it, as mIRC
/// shows a collapsed folder. `is_expanded` is asked for items with children
/// only.
fn tree_item_activity(
    state: &WindowState,
    tree_item: isize,
    is_expanded: impl FnOnce() -> bool,
) -> Option<ViewActivityLevel> {
    let item = &state.tree_items[&tree_item];
    match &item.descendant_views {
        Some(descendant_views) if !is_expanded() => {
            rolled_up_activity(state, item.node.view().into_iter().chain(descendant_views))
        }
        _ => rolled_up_activity(state, item.node.view()),
    }
}

/// The custom-draw reply that lets the control paint as usual.
const CDRF_DEFAULT: LRESULT = 0;

/// Asks for per-item notifications, then colours each item's text through
/// `item_color`. Those are the only stages requested.
fn activity_custom_draw(
    draw_stage: u32,
    clr_text: &mut u32,
    item_color: impl FnOnce() -> Option<u32>,
) -> LRESULT {
    match draw_stage {
        CDDS_PREPAINT => CDRF_NOTIFYITEMDRAW as LRESULT,
        CDDS_ITEMPREPAINT => {
            if let Some(color) = item_color() {
                *clr_text = color;
            }
            CDRF_DEFAULT
        }
        stage => panic!("unrequested custom-draw stage {stage:#x}"),
    }
}

/// Switchbar items are identified by their button command id.
pub(super) fn switchbar_custom_draw(state: &WindowState, draw: &mut NMTBCUSTOMDRAW) -> LRESULT {
    let command_id = draw.nmcd.dwItemSpec;
    activity_custom_draw(draw.nmcd.dwDrawStage, &mut draw.clrText, || {
        let view_id = &state.switchbar_views[command_id - SWITCHBAR_COMMAND_FIRST];
        view_activity(state, view_id)
            .map(|level| activity_level_color(level, &state.settings.settings().switchbar.activity))
    })
}

/// Treebar items are identified by their `HTREEITEM`. A space folder has no
/// view: it keeps the tree's text colour unless it is collapsed.
pub(super) fn tree_custom_draw(state: &WindowState, draw: &mut NMTVCUSTOMDRAW) -> LRESULT {
    let tree = draw.nmcd.hdr.hwndFrom;
    let tree_item = draw.nmcd.dwItemSpec as isize;
    activity_custom_draw(draw.nmcd.dwDrawStage, &mut draw.clrText, || {
        tree_item_activity(state, tree_item, || unsafe {
            tree_item_expanded(tree, tree_item)
        })
        .map(|level| activity_level_color(level, &state.settings.settings().switchbar.activity))
    })
}

unsafe fn tree_item_expanded(tree: HWND, item: isize) -> bool {
    let state = SendMessageW(
        tree,
        TVM_GETITEMSTATE,
        item as WPARAM,
        TVIS_EXPANDED as LPARAM,
    ) as u32;
    state & TVIS_EXPANDED != 0
}

pub(super) fn view_identity_from_view_id(view_id: &ViewId) -> ViewIdentity {
    match view_id {
        ViewId::Status => ViewIdentity::Status,
        ViewId::RoomList => ViewIdentity::RoomList,
        ViewId::Room(room_id) => ViewIdentity::Room {
            room_id: room_id.clone(),
        },
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => ViewIdentity::Thread {
            room_id: room_id.clone(),
            thread_root_event_id: thread_root_event_id.clone(),
        },
    }
}

pub(super) fn view_id_from_identity(identity: &ViewIdentity) -> ViewId {
    match identity {
        ViewIdentity::Status => ViewId::Status,
        ViewIdentity::RoomList => ViewId::RoomList,
        ViewIdentity::Room { room_id } => ViewId::Room(room_id.clone()),
        ViewIdentity::Thread {
            room_id,
            thread_root_event_id,
        } => ViewId::Thread {
            room_id: room_id.clone(),
            thread_root_event_id: thread_root_event_id.clone(),
        },
    }
}

pub(super) unsafe fn sync_views_and_tree(hwnd: HWND) -> Result<(), String> {
    ensure_status_view(hwnd)?;

    let room_ids = {
        let state = state_ref(hwnd);
        state
            .model
            .rooms
            .iter()
            .filter(|room| is_navigable_room(room) && !state.closed_queries.contains(&room.room_id))
            .map(|room| room.room_id.clone())
            .collect::<Vec<_>>()
    };
    destroy_stale_room_views(hwnd);
    for room_id in room_ids {
        ensure_room_view(hwnd, &room_id)?;
    }
    if state_ref(hwnd).model.room_list.is_some() {
        ensure_room_list_view(hwnd)?;
    }

    let view_ids = state_ref(hwnd).views.keys().cloned().collect::<Vec<_>>();
    for view_id in &view_ids {
        sync_view_content(hwnd, view_id)?;
    }

    // Activation makes it the active view: one that changes here (its room
    // went) unfolds its treebar folder and takes the focus, as any newly
    // active window does. The same one is only re-applied.
    let active_view = {
        let state = state_ref(hwnd);
        resolve_active_view(
            &state.active_view,
            state.model.selected_room.as_deref(),
            &state.model,
            &state.closed_queries,
        )
    };
    refresh_tree(hwnd);
    rebuild_switchbar(hwnd)?;
    refresh_status_bar(hwnd)?;
    if active_view == state_ref(hwnd).active_view {
        show_view(hwnd, &active_view, None).map(drop)
    } else {
        activate_view(hwnd, &active_view, None)
    }
}

unsafe fn refresh_tree(hwnd: HWND) {
    let state = state_mut(hwnd);
    let tree = state.controls.tree;
    if tree.is_null() {
        return;
    }

    // The control's own expanded state is the only record of what the user
    // collapsed: carry it over to the rebuilt items, by node.
    let collapsed = state
        .tree_items
        .iter()
        .filter(|(item, tree_item)| {
            tree_item.descendant_views.is_some() && !tree_item_expanded(tree, **item)
        })
        .map(|(_, tree_item)| tree_item.node.clone())
        .collect::<BTreeSet<_>>();
    // So is the caret, which the treebar keys move without activating: it
    // goes back to the same occurrence of its node.
    let caret = selected_tree_item(tree).map(|caret| {
        let node = state.tree_items[&caret].node.clone();
        let occurrence = tree_items_of(state, tree, &node)
            .iter()
            .position(|item| *item == caret)
            .expect("the caret is one of its node's items");
        (node, occurrence)
    });
    SendMessageW(tree, TVM_DELETEITEM, 0, TVI_ROOT as LPARAM);
    state.refresh_counters.count_treebar_rebuild();
    state.tree_items.clear();
    state.primary_tree_items.clear();

    let layout = navigation_layout(&state.model);
    insert_nav_item(state, TVI_ROOT, &layout, &collapsed);
    // A caret whose node went is set again by the activation that follows.
    if let Some((node, occurrence)) = caret
        && let Some(item) = tree_items_of(state, tree, &node).get(occurrence)
    {
        SendMessageW(tree, TVM_SELECTITEM, TVGN_CARET as WPARAM, *item as LPARAM);
    }
}

/// Inserts `item` and its subtree, mapping each item, and expands every
/// item that has children unless it is in `collapsed`. A space or room
/// listed under two spaces has one node: both of its items fold together.
unsafe fn insert_nav_item(
    state: &mut WindowState,
    parent: isize,
    item: &NavItem,
    collapsed: &BTreeSet<NavNode>,
) {
    let tree = state.controls.tree;
    let tree_item = insert_tree_item(tree, parent, &item.label);
    state.tree_items.insert(
        tree_item,
        TreeItem {
            node: item.node.clone(),
            descendant_views: (!item.children.is_empty()).then(|| item.descendant_views()),
        },
    );
    if let Some(view_id) = item.node.view() {
        state
            .primary_tree_items
            .entry(view_id.clone())
            .or_insert(tree_item);
    }
    for child in &item.children {
        insert_nav_item(state, tree_item, child, collapsed);
    }
    if !item.children.is_empty() && !collapsed.contains(&item.node) {
        expand_tree_item(tree, tree_item);
    }
}

/// Puts the treebar caret on the view's item. `reveal` (a newly active
/// view) unfolds the folders hiding it, as mIRC does; otherwise a hidden
/// item's outermost collapsed folder takes the caret, as when the user
/// collapsed it, and the folders stay as they are.
pub(super) unsafe fn select_tree_item(
    hwnd: HWND,
    view_id: &ViewId,
    preferred_tree_item: Option<isize>,
    reveal: bool,
) {
    let (tree, item) = {
        let state = state_ref(hwnd);
        let item = preferred_tree_item
            .filter(|item| {
                state
                    .tree_items
                    .get(item)
                    .is_some_and(|tree_item| tree_item.node.view() == Some(view_id))
            })
            .or_else(|| state.primary_tree_items.get(view_id).copied());
        (state.controls.tree, item)
    };
    let Some(item) = item else {
        return;
    };
    let target = if reveal {
        item
    } else {
        outermost_collapsed_ancestor(tree, item).unwrap_or(item)
    };
    let current = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CARET as WPARAM, 0) as isize;
    if current == target {
        return;
    }

    // Selecting an item unfolds every folder above it.
    SendMessageW(tree, TVM_SELECTITEM, TVGN_CARET as WPARAM, target as LPARAM);
}

unsafe fn outermost_collapsed_ancestor(tree: HWND, item: isize) -> Option<isize> {
    let mut outermost = None;
    let mut ancestor =
        SendMessageW(tree, TVM_GETNEXTITEM, TVGN_PARENT as WPARAM, item as LPARAM) as isize;
    while ancestor != 0 {
        if !tree_item_expanded(tree, ancestor) {
            outermost = Some(ancestor);
        }
        ancestor = SendMessageW(
            tree,
            TVM_GETNEXTITEM,
            TVGN_PARENT as WPARAM,
            ancestor as LPARAM,
        ) as isize;
    }
    outermost
}

pub(super) unsafe fn insert_tree_item(tree: HWND, parent: isize, text: &str) -> isize {
    let mut text = wide_null(text);
    let item = TVITEMEXW {
        mask: TVIF_TEXT,
        pszText: text.as_mut_ptr(),
        cchTextMax: text.len() as i32,
        ..zeroed()
    };
    let insert = TVINSERTSTRUCTW {
        hParent: parent as _,
        hInsertAfter: TVI_LAST as _,
        Anonymous: TVINSERTSTRUCTW_0 { itemex: item },
    };

    SendMessageW(
        tree,
        TVM_INSERTITEMW,
        0,
        (&insert as *const TVINSERTSTRUCTW) as LPARAM,
    ) as isize
}

pub(super) unsafe fn expand_tree_item(tree: HWND, item: isize) {
    SendMessageW(tree, TVM_EXPAND, TVE_EXPAND as WPARAM, item as LPARAM);
}

/// What a treebar item stands for, which also keys its expanded state
/// across rebuilds: a window's view, or a joined space's folder.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum NavNode {
    View(ViewId),
    Space { room_id: String },
}

impl NavNode {
    pub(super) fn view(&self) -> Option<&ViewId> {
        match self {
            Self::View(view_id) => Some(view_id),
            Self::Space { .. } => None,
        }
    }
}

/// A treebar item as inserted: its node, and for an item with children,
/// every view under it (all depths) for the collapsed roll-up colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TreeItem {
    pub(super) node: NavNode,
    pub(super) descendant_views: Option<Vec<ViewId>>,
}

/// One treebar item: a window's view, or a space folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NavItem {
    pub(super) label: String,
    pub(super) node: NavNode,
    pub(super) children: Vec<NavItem>,
}

impl NavItem {
    /// Every view under the item, at all depths, in tree order.
    fn descendant_views(&self) -> Vec<ViewId> {
        let mut views = Vec::new();
        let mut stack = self.children.iter().rev().collect::<Vec<_>>();
        while let Some(item) = stack.pop() {
            views.extend(item.node.view().cloned());
            stack.extend(item.children.iter().rev());
        }
        views
    }
}

/// The treebar, laid out like mIRC's: the Status window is the root, and the
/// windows sit under it. Joined spaces become folders holding their rooms
/// (a room in two spaces appears in both), then come the rooms in no space
/// in window order (channels, queries, invites). Threads sit under their
/// room. Closing a window does not remove its room: the tree is the room
/// navigator, and selecting an item opens its window.
pub(super) fn navigation_layout(model: &UiModel) -> NavItem {
    let joined = model
        .rooms
        .iter()
        .filter(|room| room.membership == RoomMembership::Join)
        .map(|room| (room.room_id.as_str(), room))
        .collect::<BTreeMap<_, _>>();
    let space_children = joined
        .values()
        .filter(|room| room.is_space)
        .map(|space| {
            (
                space.room_id.as_str(),
                space
                    .space_children
                    .iter()
                    .filter(|child| joined.contains_key(child.room_id.as_str()))
                    .map(|child| child.room_id.as_str())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let spaced_ids = space_children
        .values()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();

    // Top-level folders: spaces that are nobody's child, in model order, then
    // one entry point into each cycle of spaces that only contain each other.
    let mut folder_ids = model
        .rooms
        .iter()
        .filter(|room| space_children.contains_key(room.room_id.as_str()))
        .filter(|space| !spaced_ids.contains(space.room_id.as_str()))
        .map(|space| space.room_id.as_str())
        .collect::<Vec<_>>();
    let mut covered = BTreeSet::new();
    for folder_id in &folder_ids {
        mark_reachable_spaces(&mut covered, &joined, &space_children, folder_id);
    }
    for space in model
        .rooms
        .iter()
        .filter(|room| space_children.contains_key(room.room_id.as_str()))
    {
        if !covered.contains(space.room_id.as_str()) {
            folder_ids.push(&space.room_id);
            mark_reachable_spaces(&mut covered, &joined, &space_children, &space.room_id);
        }
    }

    let mut children = folder_ids
        .into_iter()
        .map(|folder_id| space_nav_item(model, &joined, &space_children, folder_id, &[]))
        .collect::<Vec<_>>();
    children.extend(
        rooms_in_window_order(&model.rooms)
            .into_iter()
            .filter(|room| !spaced_ids.contains(room.room_id.as_str()))
            .map(|room| room_nav_item(model, room)),
    );
    if model.room_list.is_some() {
        children.push(NavItem {
            label: ROOM_LIST_TITLE.to_owned(),
            node: NavNode::View(ViewId::RoomList),
            children: Vec::new(),
        });
    }
    NavItem {
        label: view_short_title(model, &ViewId::Status),
        node: NavNode::View(ViewId::Status),
        children,
    }
}

fn mark_reachable_spaces<'a>(
    covered: &mut BTreeSet<&'a str>,
    joined: &BTreeMap<&str, &'a RoomSummary>,
    space_children: &BTreeMap<&str, Vec<&'a str>>,
    root_id: &'a str,
) {
    let mut stack = vec![root_id];
    while let Some(space_id) = stack.pop() {
        if !covered.insert(space_id) {
            continue;
        }
        stack.extend(
            space_children[space_id]
                .iter()
                .rev()
                .copied()
                .filter(|child_id| joined[child_id].is_space),
        );
    }
}

/// A space folder with its rooms and nested folders. A space already on the
/// path from the top-level folder shows as an empty folder, which ends a
/// cycle.
fn space_nav_item(
    model: &UiModel,
    joined: &BTreeMap<&str, &RoomSummary>,
    space_children: &BTreeMap<&str, Vec<&str>>,
    space_id: &str,
    ancestors: &[&str],
) -> NavItem {
    let mut chain = ancestors.to_vec();
    chain.push(space_id);
    let children = space_children[space_id]
        .iter()
        .map(|child_id| {
            let child = joined[child_id];
            if !child.is_space {
                room_nav_item(model, child)
            } else if chain.contains(child_id) {
                NavItem {
                    label: space_tree_label(child),
                    node: NavNode::Space {
                        room_id: child.room_id.clone(),
                    },
                    children: Vec::new(),
                }
            } else {
                space_nav_item(model, joined, space_children, child_id, &chain)
            }
        })
        .collect();
    NavItem {
        label: space_tree_label(joined[space_id]),
        node: NavNode::Space {
            room_id: space_id.to_owned(),
        },
        children,
    }
}

/// A room's item, with its threads under it; an invite has none.
fn room_nav_item(model: &UiModel, room: &RoomSummary) -> NavItem {
    let children = match (is_sendable_room(room), model.room_views.get(&room.room_id)) {
        (true, Some(room_view)) => room_view
            .threads
            .iter()
            .map(|(thread_root_event_id, thread)| NavItem {
                label: thread.label.clone(),
                node: NavNode::View(ViewId::Thread {
                    room_id: room.room_id.clone(),
                    thread_root_event_id: thread_root_event_id.clone(),
                }),
                children: Vec::new(),
            })
            .collect(),
        _ => Vec::new(),
    };
    NavItem {
        label: room_tree_label(room),
        node: NavNode::View(ViewId::Room(room.room_id.clone())),
        children,
    }
}

/// The switchbar order, mIRC's: Status, channels, queries, then invites,
/// each room followed by its open threads; then views of rooms that left.
pub(super) fn ordered_view_ids(state: &WindowState) -> Vec<ViewId> {
    let mut ordered = Vec::new();
    if state.views.contains_key(&ViewId::Status) {
        ordered.push(ViewId::Status);
    }

    for room in rooms_in_window_order(&state.model.rooms) {
        let view_id = ViewId::Room(room.room_id.clone());
        if state.views.contains_key(&view_id) {
            ordered.push(view_id);
        }
        if let Some(room_view) = state.model.room_views.get(&room.room_id) {
            for thread_root_event_id in room_view.threads.keys() {
                let thread_view_id = ViewId::Thread {
                    room_id: room.room_id.clone(),
                    thread_root_event_id: thread_root_event_id.clone(),
                };
                if state.views.contains_key(&thread_view_id) {
                    ordered.push(thread_view_id);
                }
            }
        }
    }

    let mut remaining = state
        .views
        .keys()
        .filter_map(|view_id| match view_id {
            ViewId::Status | ViewId::RoomList => None,
            ViewId::Room(room_id)
                if navigable_room_by_id(&state.model.rooms, room_id).is_none() =>
            {
                Some(room_id.clone())
            }
            ViewId::Room(_) => None,
            ViewId::Thread { .. } => None,
        })
        .collect::<Vec<_>>();
    remaining.sort();
    ordered.extend(remaining.into_iter().map(ViewId::Room));
    if state.views.contains_key(&ViewId::RoomList) {
        ordered.push(ViewId::RoomList);
    }
    ordered
}

/// mIRC's Alt+1..9: the `n`th window (1-based) in switchbar order.
pub(super) fn nth_window(order: &[ViewId], n: usize) -> Option<&ViewId> {
    order.get(n - 1)
}

/// Which windows Ctrl+Tab (all), Ctrl+N (channels) and Ctrl+Q (queries)
/// step through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WindowCycle {
    All,
    Channels,
    Queries,
}

impl WindowCycle {
    fn admits(self, state: &WindowState, view_id: &ViewId) -> bool {
        let kind = match self {
            WindowCycle::All => return true,
            WindowCycle::Channels => RoomWindowKind::Channel,
            WindowCycle::Queries => RoomWindowKind::Query,
        };
        let ViewId::Room(room_id) = view_id else {
            return false;
        };
        // A window of a room that left is no channel or query any more.
        navigable_room_by_id(&state.model.rooms, room_id)
            .is_some_and(|room| room_window_kind(room) == kind)
    }
}

/// The window after (or before) the active one in switchbar `order` that
/// `cycle` admits, wrapping around; `None` when no other window qualifies.
pub(super) fn cycled_window(
    state: &WindowState,
    order: &[ViewId],
    cycle: WindowCycle,
    forward: bool,
) -> Option<ViewId> {
    let position = order
        .iter()
        .position(|view_id| *view_id == state.active_view)
        .expect("the active view is in the switchbar order");
    let (before, from_active) = order.split_at(position);
    let mut others = from_active[1..].iter().chain(before);
    let admits = |view_id: &&ViewId| cycle.admits(state, view_id);
    if forward {
        others.find(admits)
    } else {
        others.rfind(admits)
    }
    .cloned()
}

pub(super) unsafe fn rebuild_switchbar(hwnd: HWND) -> Result<(), String> {
    let switchbar = state_ref(hwnd).controls.switchbar;
    if switchbar.is_null() {
        return Err("switchbar control was not created".to_owned());
    }

    state_mut(hwnd).refresh_counters.count_switchbar_rebuild();
    loop {
        let count = SendMessageW(switchbar, TB_BUTTONCOUNT, 0, 0);
        if count < 0 {
            return Err("failed to query switchbar button count".to_owned());
        }
        if count == 0 {
            break;
        }
        if SendMessageW(switchbar, TB_DELETEBUTTON, 0, 0) == 0 {
            return Err("failed to delete switchbar button".to_owned());
        }
    }

    let view_ids = ordered_view_ids(state_ref(hwnd));
    let button_count = view_ids.len();
    if SWITCHBAR_COMMAND_FIRST
        .checked_add(button_count.saturating_sub(1))
        .is_some_and(|command_id| command_id > SWITCHBAR_COMMAND_LAST)
    {
        return Err("switchbar command range exhausted".to_owned());
    }

    let labels = {
        let state = state_ref(hwnd);
        view_ids
            .iter()
            .map(|view_id| wide_null(&view_short_title(&state.model, view_id)))
            .collect::<Vec<_>>()
    };
    let button_states = {
        let state = state_ref(hwnd);
        view_ids
            .iter()
            .map(|view_id| switchbar_button_state(state, view_id))
            .collect::<Vec<_>>()
    };
    let buttons = view_ids
        .iter()
        .enumerate()
        .map(|(index, _)| TBBUTTON {
            iBitmap: I_IMAGENONE,
            idCommand: (SWITCHBAR_COMMAND_FIRST + index) as i32,
            fsState: button_states[index],
            fsStyle: (BTNS_CHECK | BTNS_SHOWTEXT) as u8,
            iString: labels[index].as_ptr() as isize,
            ..TBBUTTON::default()
        })
        .collect::<Vec<_>>();

    // `switchbar_views` mirrors the buttons, indexed by command id: switch
    // it while the toolbar is empty, so a paint never sees a button without
    // its view.
    state_mut(hwnd).switchbar_views = view_ids;
    if !buttons.is_empty()
        && SendMessageW(
            switchbar,
            TB_ADDBUTTONSW,
            buttons.len(),
            buttons.as_ptr() as LPARAM,
        ) == 0
    {
        return Err("failed to add switchbar buttons".to_owned());
    }
    // More or fewer buttons may take more or fewer rows.
    layout_controls(hwnd);
    refresh_switchbar_state(hwnd)
}

pub(super) unsafe fn refresh_switchbar_state(hwnd: HWND) -> Result<(), String> {
    let switchbar = state_ref(hwnd).controls.switchbar;
    if switchbar.is_null() {
        return Err("switchbar control was not created".to_owned());
    }

    let view_ids = state_ref(hwnd).switchbar_views.clone();
    for (index, view_id) in view_ids.iter().enumerate() {
        let command_id = SWITCHBAR_COMMAND_FIRST + index;
        let checked = state_ref(hwnd).active_view == *view_id;
        // TB_SETSTATE would replace the whole state, wrap bit included,
        // which only the toolbar's layout sets.
        if SendMessageW(switchbar, TB_CHECKBUTTON, command_id, checked as LPARAM) == 0 {
            return Err(format!("failed to refresh switchbar button {command_id}"));
        }
    }
    // Repaint for the activity colours, which custom draw reads.
    InvalidateRect(switchbar, null(), 1);

    Ok(())
}

/// Repaints the treebar for the activity colours, which custom draw reads.
pub(super) unsafe fn refresh_tree_activity_colors(hwnd: HWND) -> Result<(), String> {
    let tree = state_ref(hwnd).controls.tree;
    if tree.is_null() {
        return Err("tree control was not created".to_owned());
    }
    InvalidateRect(tree, null(), 1);
    Ok(())
}

pub(super) unsafe fn read_tree_item_text(tree: HWND, item: isize) -> Result<String, String> {
    let mut buffer = vec![0u16; 1024];
    let mut tree_item = TVITEMEXW {
        mask: TVIF_TEXT,
        hItem: item as _,
        pszText: buffer.as_mut_ptr(),
        cchTextMax: buffer.len() as i32,
        ..zeroed()
    };

    if SendMessageW(
        tree,
        TVM_GETITEMW,
        0,
        (&mut tree_item as *mut TVITEMEXW) as LPARAM,
    ) == 0
    {
        return Err(format!(
            "failed to read tree item text for handle 0x{item:x}"
        ));
    }

    let text_len = buffer
        .iter()
        .position(|ch| *ch == 0)
        .unwrap_or(buffer.len());
    Ok(String::from_utf16_lossy(&buffer[..text_len]))
}

/// The item as drawn: its activity is its colour, which a collapsed item
/// rolls up from under it.
unsafe fn read_navigation_node(
    state: &WindowState,
    tree: HWND,
    item: isize,
) -> Result<NavigationNode, String> {
    let text = read_tree_item_text(tree, item)?;
    let tree_item = &state.tree_items[&item];
    let expanded = tree_item
        .descendant_views
        .is_some()
        .then(|| tree_item_expanded(tree, item));
    let activity = tree_item_activity(state, item, || {
        expanded.expect("items with children have an expanded state")
    });
    let mut children = Vec::new();
    let mut child =
        SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CHILD as WPARAM, item as LPARAM) as isize;
    while child != 0 {
        children.push(read_navigation_node(state, tree, child)?);
        child = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_NEXT as WPARAM, child as LPARAM) as isize;
    }

    Ok(NavigationNode {
        text,
        view: tree_item.node.view().map(view_identity_from_view_id),
        space_id: match &tree_item.node {
            NavNode::Space { room_id } => Some(room_id.clone()),
            NavNode::View(_) => None,
        },
        expanded,
        activity,
        children,
    })
}

pub(super) unsafe fn dump_navigation_tree(hwnd: HWND) -> Result<NavigationNode, String> {
    let tree = state_ref(hwnd).controls.tree;
    if tree.is_null() {
        return Err("tree control was not created".to_owned());
    }

    let root = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_ROOT as WPARAM, 0) as isize;
    if root == 0 {
        return Err("navigation tree root was missing".to_owned());
    }

    read_navigation_node(state_ref(hwnd), tree, root)
}

fn format_navigation_path(path: &[NavigationPathSegment]) -> String {
    path.iter()
        .map(|segment| {
            if segment.same_text_index == 0 {
                segment.text.clone()
            } else {
                format!("{} [{}]", segment.text, segment.same_text_index)
            }
        })
        .collect::<Vec<_>>()
        .join(" > ")
}

unsafe fn same_text_index_for_tree_item(tree: HWND, item: isize) -> Result<u32, String> {
    let parent =
        SendMessageW(tree, TVM_GETNEXTITEM, TVGN_PARENT as WPARAM, item as LPARAM) as isize;
    let target_text = read_tree_item_text(tree, item)?;
    let mut current = if parent == 0 {
        SendMessageW(tree, TVM_GETNEXTITEM, TVGN_ROOT as WPARAM, 0) as isize
    } else {
        SendMessageW(
            tree,
            TVM_GETNEXTITEM,
            TVGN_CHILD as WPARAM,
            parent as LPARAM,
        ) as isize
    };
    let mut same_text_index = 0u32;
    while current != 0 {
        if read_tree_item_text(tree, current)? == target_text {
            if current == item {
                return Ok(same_text_index);
            }
            same_text_index += 1;
        }
        current = SendMessageW(
            tree,
            TVM_GETNEXTITEM,
            TVGN_NEXT as WPARAM,
            current as LPARAM,
        ) as isize;
    }

    Err(format!(
        "tree item 0x{item:x} was not found among its siblings"
    ))
}

pub(super) unsafe fn dump_selected_navigation_path(
    hwnd: HWND,
) -> Result<SelectedNavigationPathResponse, String> {
    let tree = state_ref(hwnd).controls.tree;
    if tree.is_null() {
        return Err("tree control was not created".to_owned());
    }

    let mut item = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CARET as WPARAM, 0) as isize;
    if item == 0 {
        return Err("navigation tree selection was missing".to_owned());
    }
    let view = state_ref(hwnd).tree_items[&item]
        .node
        .view()
        .map(view_identity_from_view_id);

    let mut path = Vec::new();
    while item != 0 {
        path.push(NavigationPathSegment {
            text: read_tree_item_text(tree, item)?,
            same_text_index: same_text_index_for_tree_item(tree, item)?,
        });
        item = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_PARENT as WPARAM, item as LPARAM) as isize;
    }
    path.reverse();
    Ok(SelectedNavigationPathResponse { path, view })
}

pub(super) unsafe fn select_navigation_path(
    hwnd: HWND,
    path: &[NavigationPathSegment],
) -> Result<(), String> {
    if path.is_empty() {
        return Err("navigation path must not be empty".to_owned());
    }

    let tree = state_ref(hwnd).controls.tree;
    if tree.is_null() {
        return Err("tree control was not created".to_owned());
    }

    let mut current = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_ROOT as WPARAM, 0) as isize;
    if current == 0 {
        return Err("navigation tree root was missing".to_owned());
    }

    let root_text = read_tree_item_text(tree, current)?;
    let root_same_text_index = same_text_index_for_tree_item(tree, current)?;
    if root_text != path[0].text || root_same_text_index != path[0].same_text_index {
        return Err(format!(
            "unknown navigation path: {}",
            format_navigation_path(path)
        ));
    }

    for segment in &path[1..] {
        let mut child = SendMessageW(
            tree,
            TVM_GETNEXTITEM,
            TVGN_CHILD as WPARAM,
            current as LPARAM,
        ) as isize;
        let mut matched_child = None;
        let mut same_text_index = 0u32;
        while child != 0 {
            if read_tree_item_text(tree, child)? == segment.text {
                if same_text_index == segment.same_text_index {
                    matched_child = Some(child);
                    break;
                }
                same_text_index += 1;
            }
            child =
                SendMessageW(tree, TVM_GETNEXTITEM, TVGN_NEXT as WPARAM, child as LPARAM) as isize;
        }

        current = matched_child
            .ok_or_else(|| format!("unknown navigation path: {}", format_navigation_path(path)))?;
    }

    select_navigation_item(hwnd, tree, current)
}

/// Selects the view's first tree item: a room listed under two spaces has two
/// items, and `primary_tree_items` holds the first.
pub(super) unsafe fn select_navigation_view(
    hwnd: HWND,
    identity: &ViewIdentity,
) -> Result<(), String> {
    let tree = state_ref(hwnd).controls.tree;
    let item = *state_ref(hwnd)
        .primary_tree_items
        .get(&view_id_from_identity(identity))
        .ok_or_else(|| format!("unknown navigation view: {identity:?}"))?;
    select_navigation_item(hwnd, tree, item)
}

/// Expands or collapses the node's first treebar item, as its button does.
/// A collapse over the caret moves it to the item (the control does that)
/// and activates nothing.
pub(super) unsafe fn set_navigation_expanded(
    hwnd: HWND,
    node: &NavNode,
    expanded: bool,
) -> Result<(), String> {
    let tree = state_ref(hwnd).controls.tree;
    let item = *tree_items_of(state_ref(hwnd), tree, node)
        .first()
        .ok_or_else(|| format!("unknown navigation node: {node:?}"))?;
    if state_ref(hwnd).tree_items[&item].descendant_views.is_none() {
        return Err(format!("navigation node has no children: {node:?}"));
    }
    let action = if expanded { TVE_EXPAND } else { TVE_COLLAPSE };
    SendMessageW(tree, TVM_EXPAND, action as WPARAM, item as LPARAM);
    // TVM_EXPAND sends no TVN_ITEMEXPANDEDW.
    tree_expansion_changed(hwnd)
}

/// After an item folds or unfolds (`TVN_ITEMEXPANDEDW`): its colour depends
/// on it, and the caret goes back to the active view's item once a folder
/// unfolds over it, as it went to the folder when it folded.
pub(super) unsafe fn tree_expansion_changed(hwnd: HWND) -> Result<(), String> {
    let tree = state_ref(hwnd).controls.tree;
    let active_view = state_ref(hwnd).active_view.clone();
    select_tree_item(hwnd, &active_view, selected_tree_item(tree), false);
    refresh_tree_activity_colors(hwnd)
}

/// Every treebar item, in tree order.
unsafe fn tree_items_in_order(tree: HWND) -> Vec<isize> {
    unsafe fn push_siblings(tree: HWND, mut item: isize, items: &mut Vec<isize>) {
        while item != 0 {
            items.push(item);
            let child =
                SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CHILD as WPARAM, item as LPARAM) as isize;
            push_siblings(tree, child, items);
            item =
                SendMessageW(tree, TVM_GETNEXTITEM, TVGN_NEXT as WPARAM, item as LPARAM) as isize;
        }
    }
    let mut items = Vec::new();
    let root = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_ROOT as WPARAM, 0) as isize;
    push_siblings(tree, root, &mut items);
    items
}

/// The items standing for `node`, in tree order: a room or space listed
/// under two spaces has two.
unsafe fn tree_items_of(state: &WindowState, tree: HWND, node: &NavNode) -> Vec<isize> {
    tree_items_in_order(tree)
        .into_iter()
        .filter(|item| state.tree_items[item].node == *node)
        .collect()
}

/// Selects a tree item like a click does, and activates its view. Activation
/// runs here too because a selection that is already the caret sends no
/// `TVN_SELCHANGEDW`, yet a click on it still brings its window forward.
unsafe fn select_navigation_item(hwnd: HWND, tree: HWND, item: isize) -> Result<(), String> {
    let selected = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CARET as WPARAM, 0) as isize;
    if selected != item {
        SendMessageW(tree, TVM_SELECTITEM, TVGN_CARET as WPARAM, item as LPARAM);
    }
    let view_id = state_ref(hwnd).tree_items[&item].node.view().cloned();
    if let Some(view_id) = view_id {
        activate_view(hwnd, &view_id, Some(item))?;
    }

    Ok(())
}

unsafe fn tree_item_at_screen_point(tree: HWND, x: i32, y: i32) -> Option<isize> {
    let mut hit = TVHITTESTINFO {
        pt: POINT { x, y },
        flags: 0,
        hItem: 0,
    };
    (ScreenToClient(tree, &mut hit.pt) != 0)
        .then(|| {
            SendMessageW(
                tree,
                TVM_HITTEST,
                0,
                (&mut hit as *mut TVHITTESTINFO) as LPARAM,
            ) as isize
        })
        .filter(|item| *item != 0)
}

pub(super) unsafe fn selected_tree_item(tree: HWND) -> Option<isize> {
    let item = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CARET as WPARAM, 0) as isize;
    (item != 0).then_some(item)
}

unsafe fn tree_item_context_screen_point(tree: HWND, item: isize) -> Result<(i32, i32), String> {
    let mut rect = RECT::default();
    std::ptr::write_unaligned((&mut rect as *mut RECT).cast::<isize>(), item);
    if SendMessageW(tree, TVM_GETITEMRECT, 1, (&mut rect as *mut RECT) as LPARAM) == 0 {
        return Err("tree item rectangle is unavailable".to_owned());
    }
    let mut point = POINT {
        x: rect.left,
        y: rect.top + (rect.bottom - rect.top) / 2,
    };
    if ClientToScreen(tree, &mut point) == 0 {
        return Err(last_os_error("ClientToScreen"));
    }
    Ok((point.x, point.y))
}

pub(super) unsafe fn show_navigation_room_context_menu(
    frame: HWND,
    lparam: LPARAM,
    requested_command: Option<usize>,
) -> Result<(), String> {
    let tree = state_ref(frame).controls.tree;
    let (item, point) = match context_menu_screen_point_for_lparam(lparam) {
        Some((x, y)) => {
            let item = tree_item_at_screen_point(tree, x, y)
                .ok_or_else(|| "room context menu target is unavailable".to_owned())?;
            SendMessageW(tree, TVM_SELECTITEM, TVGN_CARET as WPARAM, item as LPARAM);
            (item, (x, y))
        }
        None => {
            let item = selected_tree_item(tree)
                .ok_or_else(|| "room context menu target is unavailable".to_owned())?;
            (item, tree_item_context_screen_point(tree, item)?)
        }
    };
    let Some(view_id) = state_ref(frame).tree_items[&item].node.view().cloned() else {
        return Ok(());
    };
    if !matches!(view_id, ViewId::Room(_)) {
        return Ok(());
    }
    activate_view(frame, &view_id, Some(item))?;
    let menu = state_ref(frame).controls.room_menu;
    refresh_room_menu_state(frame, menu)?;
    let action_state = active_room_action_state(state_ref(frame));
    if requested_command.is_some_and(|command| {
        (command == MENU_ID_ROOM_ACCEPT_INVITE && !action_state.accept_invite_enabled)
            || (command == MENU_ID_ROOM_REJECT_INVITE && !action_state.reject_invite_enabled)
            || (command == MENU_ID_ROOM_PART && !action_state.part_room_enabled)
            || (notification_menu_mode(command).is_some()
                && action_state.notification_mode.is_none())
    }) {
        return Err("room context menu action is disabled".to_owned());
    }
    if let Some(command) = show_context_menu(frame, menu, point.0, point.1, requested_command)?
        && let Some(result) = handle_menu_command(frame, command)
    {
        result?;
    }
    Ok(())
}

/// A selection the user makes, with the mouse or a key, activates its view.
/// The caret also moves without the user: mirx moves it while activating
/// (`select_tree_item`), and the control moves it to a folder collapsing
/// over it (which Windows reports as `TVC_UNKNOWN`, and Wine not at all).
/// Neither activates anything, so collapsing a folder keeps the window.
pub(super) unsafe fn handle_tree_selection(hwnd: HWND, change: &NMTREEVIEWW) {
    if change.action != TVC_BYMOUSE && change.action != TVC_BYKEYBOARD {
        return;
    }
    let selected_item = change.itemNew.hItem;
    let view_id = state_ref(hwnd).tree_items[&selected_item]
        .node
        .view()
        .cloned();
    let Some(view_id) = view_id else {
        return;
    };
    if let Err(message) = activate_view(hwnd, &view_id, Some(selected_item)) {
        show_error_message(hwnd, "Treebar", &message);
    }
    // MDI activation focuses the window; arrowing through the treebar
    // switches windows and keeps the keys in it.
    if change.action == TVC_BYKEYBOARD {
        SetFocus(change.hdr.hwndFrom);
    }
}

pub(super) fn select_room(state: &mut WindowState, room_id: &str) -> Result<(), String> {
    if navigable_room_by_id(&state.model.rooms, room_id).is_none() {
        return Err(format!("unknown room id: {room_id}"));
    }

    state.model.selected_room = Some(room_id.to_owned());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::colors::{PaletteIndex, rgb};

    use super::super::test_fixtures::*;
    use super::*;

    #[test]
    fn room_tree_label_has_no_e2ee_or_query_markers() {
        let mut direct_room = room("!direct:example", "Direct", true);
        direct_room.is_direct = true;
        direct_room.direct_user_ids = vec!["@alice:example".to_owned()];
        assert_eq!(room_tree_label(&direct_room), "Direct");

        direct_room.direct_user_ids.clear();
        assert_eq!(room_tree_label(&direct_room), "Direct");

        assert_eq!(room_tree_label(&room("!ops:example", "Ops", true)), "Ops");
    }

    #[test]
    fn room_tree_label_marks_invites() {
        let mut invite = room("!invite:example", "Lobby", true);
        invite.membership = RoomMembership::Invite;
        invite.is_direct = true;
        assert_eq!(room_tree_label(&invite), "Lobby [invite]");
    }

    /// The read state colours a room's buttons (core_events covers each
    /// level and the mute caps) but never changes its label: mIRC shows
    /// unread state by colour alone.
    #[test]
    fn room_labels_stay_the_bare_name_while_read_state_colours_them() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.model.room_views.insert(
            "!room:example".to_owned(),
            RoomViewModel {
                threads: BTreeMap::from([(
                    "$thread-root".to_owned(),
                    ThreadViewModel {
                        label: "root msg".to_owned(),
                    },
                )]),
                ..RoomViewModel::default()
            },
        );

        apply_core_event(
            &mut state,
            room_read_state(
                "!room:example",
                Some("$latest"),
                Some("$fully-read"),
                true,
                3,
                2,
                1,
            ),
        )
        .unwrap();

        assert_eq!(
            state.room_activity.get(&room_view_id("!room:example")),
            Some(&ViewActivityLevel::Highlight)
        );
        assert_eq!(
            view_short_title(&state.model, &room_view_id("!room:example")),
            "Room"
        );
        assert_eq!(
            view_short_title(
                &state.model,
                &thread_view_id("!room:example", "$thread-root")
            ),
            "Thread: root msg"
        );
        assert_eq!(
            navigation_layout(&state.model),
            nav_view(
                "Status",
                ViewId::Status,
                vec![nav_view(
                    "Room",
                    room_view_id("!room:example"),
                    vec![nav_view(
                        "root msg",
                        thread_view_id("!room:example", "$thread-root"),
                        Vec::new()
                    )]
                )]
            )
        );
    }

    #[test]
    fn notifications_alone_or_a_marked_unread_room_colour_it_as_a_message() {
        for (marked_unread, num_notifications) in [(false, 1), (true, 0)] {
            let mut state = test_state();
            state.model.rooms = vec![room("!room:example", "Room", false)];
            apply_core_event(
                &mut state,
                room_read_state(
                    "!room:example",
                    None,
                    None,
                    marked_unread,
                    0,
                    num_notifications,
                    0,
                ),
            )
            .unwrap();

            assert_eq!(
                state.room_activity.get(&room_view_id("!room:example")),
                Some(&ViewActivityLevel::Message),
                "marked {marked_unread}, notifications {num_notifications}"
            );
        }
    }

    #[test]
    fn ordered_view_ids_follow_status_then_room_model_order_then_stale_views() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!beta:example", "Beta", false),
            room("!alpha:example", "Alpha", false),
        ];
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(ViewId::Room("!alpha:example".to_owned()), tracked_view(20));
        state
            .views
            .insert(ViewId::Room("!orphan:example".to_owned()), tracked_view(30));
        state
            .views
            .insert(ViewId::Room("!beta:example".to_owned()), tracked_view(40));

        assert_eq!(
            ordered_view_ids(&state),
            vec![
                ViewId::Status,
                ViewId::Room("!beta:example".to_owned()),
                ViewId::Room("!alpha:example".to_owned()),
                ViewId::Room("!orphan:example".to_owned()),
            ]
        );
    }

    fn space_with_children(room_id: &str, display_name: &str, children: &[&str]) -> RoomSummary {
        let mut space = space(room_id, display_name);
        space.space_children = children
            .iter()
            .map(|child_id| mirx_core::SpaceChildSummary {
                room_id: (*child_id).to_owned(),
                suggested: false,
                order: None,
            })
            .collect();
        space
    }

    fn nav_folder(label: &str, room_id: &str, children: Vec<NavItem>) -> NavItem {
        NavItem {
            label: label.to_owned(),
            node: NavNode::Space {
                room_id: room_id.to_owned(),
            },
            children,
        }
    }

    #[test]
    fn navigation_layout_roots_at_status_labelled_with_the_user_id() {
        let mut state = test_state();
        assert_eq!(
            navigation_layout(&state.model),
            nav_view("Status", ViewId::Status, Vec::new())
        );

        state.model.own_user_id = Some("@me:example".to_owned());
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        assert_eq!(
            navigation_layout(&state.model),
            nav_view(
                "@me:example",
                ViewId::Status,
                vec![nav_room("Ops", "!ops:example")]
            )
        );
    }

    #[test]
    fn navigation_layout_lists_channels_then_queries_then_invites() {
        let mut state = test_state();
        state.model.rooms = vec![
            invited_room("!invite:example", "Lobby"),
            query("!alice:example", "Alice", "@alice:example"),
            room("!ops:example", "Ops", true),
            query("!bob:example", "Bob", "@bob:example"),
            room("!random:example", "Random", false),
        ];

        assert_eq!(
            navigation_layout(&state.model),
            nav_view(
                "Status",
                ViewId::Status,
                vec![
                    nav_room("Ops", "!ops:example"),
                    nav_room("Random", "!random:example"),
                    nav_room("Alice", "!alice:example"),
                    nav_room("Bob", "!bob:example"),
                    nav_room("Lobby [invite]", "!invite:example"),
                ]
            )
        );
    }

    #[test]
    fn navigation_layout_nests_rooms_in_space_folders_and_lists_unspaced_after() {
        let mut state = test_state();
        let mut invited_space = space_with_children("!invited-space:example", "Invited", &[]);
        invited_space.membership = RoomMembership::Invite;
        state.model.rooms = vec![
            query("!direct:example", "Direct", "@alice:example"),
            room("!lobby:example", "Lobby", false),
            space_with_children(
                "!work:example",
                "Work",
                &["!ops:example", "!team:example", "!missing:example"],
            ),
            space_with_children(
                "!team:example",
                "Team",
                &["!shared:example", "!team:example"],
            ),
            space_with_children(
                "!social:example",
                "Social",
                &["!shared:example", "!dm:example"],
            ),
            // Two spaces containing only each other, reachable from no root.
            space_with_children("!loop-a:example", "Loop A", &["!loop-b:example"]),
            space_with_children("!loop-b:example", "Loop B", &["!loop-a:example"]),
            room("!ops:example", "Ops", true),
            room("!shared:example", "Shared", false),
            query("!dm:example", "Dm", "@bob:example"),
            invited_space,
        ];

        assert_eq!(
            navigation_layout(&state.model),
            nav_view(
                "Status",
                ViewId::Status,
                vec![
                    nav_folder(
                        "Work",
                        "!work:example",
                        vec![
                            nav_room("Ops", "!ops:example"),
                            nav_folder(
                                "Team",
                                "!team:example",
                                vec![
                                    nav_room("Shared", "!shared:example"),
                                    // A space containing itself ends in an empty folder.
                                    nav_folder("Team", "!team:example", Vec::new()),
                                ]
                            ),
                        ]
                    ),
                    nav_folder(
                        "Social",
                        "!social:example",
                        vec![
                            nav_room("Shared", "!shared:example"),
                            // Rooms under a space stay there whatever their kind.
                            nav_room("Dm", "!dm:example"),
                        ]
                    ),
                    nav_folder(
                        "Loop A",
                        "!loop-a:example",
                        vec![nav_folder(
                            "Loop B",
                            "!loop-b:example",
                            vec![nav_folder("Loop A", "!loop-a:example", Vec::new())]
                        )]
                    ),
                    nav_room("Lobby", "!lobby:example"),
                    nav_room("Direct", "!direct:example"),
                ]
            )
        );
    }

    #[test]
    fn navigation_layout_puts_threads_under_their_room() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        state.model.room_views.insert(
            "!ops:example".to_owned(),
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

        assert_eq!(
            navigation_layout(&state.model),
            nav_view(
                "Status",
                ViewId::Status,
                vec![nav_view(
                    "Ops",
                    room_view_id("!ops:example"),
                    vec![nav_view(
                        "root msg",
                        thread_view_id("!ops:example", "$root"),
                        Vec::new()
                    )]
                )]
            )
        );
    }

    /// Status, two channels with a thread, a query, an invite and a room
    /// that left, in switchbar order.
    fn window_cycle_fixture() -> (WindowState, Vec<ViewId>) {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!ops:example", "Ops", false),
            room("!dev:example", "Dev", false),
            query("!alice:example", "Alice", "@alice:example"),
            invited_room("!invite:example", "Lobby"),
        ];
        let order = vec![
            ViewId::Status,
            room_view_id("!ops:example"),
            thread_view_id("!ops:example", "$root"),
            room_view_id("!dev:example"),
            room_view_id("!alice:example"),
            room_view_id("!invite:example"),
            room_view_id("!gone:example"),
        ];
        (state, order)
    }

    #[test]
    fn nth_window_counts_switchbar_buttons_from_one() {
        let (_, order) = window_cycle_fixture();
        assert_eq!(nth_window(&order, 1), Some(&ViewId::Status));
        assert_eq!(nth_window(&order, 2), Some(&room_view_id("!ops:example")));
        assert_eq!(nth_window(&order, 7), Some(&room_view_id("!gone:example")));
        assert_eq!(nth_window(&order, 8), None);
    }

    #[test]
    fn ctrl_tab_steps_through_every_window_and_wraps() {
        let (mut state, order) = window_cycle_fixture();
        state.active_view = ViewId::Status;
        assert_eq!(
            cycled_window(&state, &order, WindowCycle::All, true),
            Some(room_view_id("!ops:example"))
        );
        assert_eq!(
            cycled_window(&state, &order, WindowCycle::All, false),
            Some(room_view_id("!gone:example"))
        );
        state.active_view = room_view_id("!gone:example");
        assert_eq!(
            cycled_window(&state, &order, WindowCycle::All, true),
            Some(ViewId::Status)
        );
        assert_eq!(
            cycled_window(&state, &order, WindowCycle::All, false),
            Some(room_view_id("!invite:example"))
        );
    }

    #[test]
    fn ctrl_n_steps_through_channels_only() {
        let (mut state, order) = window_cycle_fixture();
        // From a query, the next channel after it, wrapping.
        state.active_view = room_view_id("!alice:example");
        assert_eq!(
            cycled_window(&state, &order, WindowCycle::Channels, true),
            Some(room_view_id("!ops:example"))
        );
        // Threads are skipped.
        state.active_view = room_view_id("!ops:example");
        assert_eq!(
            cycled_window(&state, &order, WindowCycle::Channels, true),
            Some(room_view_id("!dev:example"))
        );
        state.active_view = thread_view_id("!ops:example", "$root");
        assert_eq!(
            cycled_window(&state, &order, WindowCycle::Channels, false),
            Some(room_view_id("!ops:example"))
        );
    }

    #[test]
    fn ctrl_q_stays_put_when_the_active_query_is_the_only_one() {
        let (mut state, order) = window_cycle_fixture();
        state.active_view = ViewId::Status;
        assert_eq!(
            cycled_window(&state, &order, WindowCycle::Queries, true),
            Some(room_view_id("!alice:example"))
        );
        state.active_view = room_view_id("!alice:example");
        assert_eq!(
            cycled_window(&state, &order, WindowCycle::Queries, true),
            None
        );
        assert_eq!(
            cycled_window(&state, &order, WindowCycle::All, true).as_ref(),
            Some(&room_view_id("!invite:example"))
        );
    }

    #[test]
    fn a_single_window_cycles_nowhere() {
        let mut state = test_state();
        state.active_view = ViewId::Status;
        assert_eq!(
            cycled_window(&state, &[ViewId::Status], WindowCycle::All, true),
            None
        );
    }

    #[test]
    fn ordered_view_ids_group_channels_before_queries() {
        let mut state = test_state();
        state.model.rooms = vec![
            invited_room("!invite:example", "Lobby"),
            query("!alice:example", "Alice", "@alice:example"),
            room("!ops:example", "Ops", false),
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
            room_view_id("!invite:example"),
            room_view_id("!alice:example"),
            thread_view_id("!alice:example", "$root"),
            room_view_id("!ops:example"),
        ]
        .into_iter()
        .enumerate()
        {
            state
                .views
                .insert(view_id, tracked_view(10 * (index as isize + 1)));
        }

        assert_eq!(
            ordered_view_ids(&state),
            vec![
                ViewId::Status,
                room_view_id("!ops:example"),
                room_view_id("!alice:example"),
                thread_view_id("!alice:example", "$root"),
                room_view_id("!invite:example"),
            ]
        );
    }

    #[test]
    fn ordered_view_ids_exclude_spaces_from_model_order() {
        let mut state = test_state();
        state.model.rooms = vec![
            space("!space:example", "Space"),
            room("!beta:example", "Beta", false),
            room("!alpha:example", "Alpha", false),
        ];
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(ViewId::Room("!space:example".to_owned()), tracked_view(20));
        state
            .views
            .insert(ViewId::Room("!alpha:example".to_owned()), tracked_view(30));
        state
            .views
            .insert(ViewId::Room("!beta:example".to_owned()), tracked_view(40));

        assert_eq!(
            ordered_view_ids(&state),
            vec![
                ViewId::Status,
                ViewId::Room("!beta:example".to_owned()),
                ViewId::Room("!alpha:example".to_owned()),
                ViewId::Room("!space:example".to_owned()),
            ]
        );
    }

    #[test]
    fn view_identity_round_trips_thread_view_id() {
        let view_id = ViewId::Thread {
            room_id: "!ops:example".to_owned(),
            thread_root_event_id: "$thread-root".to_owned(),
        };

        assert_eq!(
            view_identity_from_view_id(&view_id),
            ViewIdentity::Thread {
                room_id: "!ops:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            }
        );
        assert_eq!(
            view_id_from_identity(&ViewIdentity::Thread {
                room_id: "!ops:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            }),
            view_id
        );
    }

    #[test]
    fn ordered_view_ids_place_threads_after_parent_rooms() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!alpha:example", "Alpha", false),
            room("!beta:example", "Beta", false),
        ];
        state.model.room_views.insert(
            "!alpha:example".to_owned(),
            RoomViewModel {
                threads: BTreeMap::from([(
                    "$thread-root".to_owned(),
                    ThreadViewModel {
                        label: "root msg".to_owned(),
                    },
                )]),
                ..RoomViewModel::default()
            },
        );
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(ViewId::Room("!alpha:example".to_owned()), tracked_view(20));
        state.views.insert(
            ViewId::Thread {
                room_id: "!alpha:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            },
            tracked_view(30),
        );
        state
            .views
            .insert(ViewId::Room("!beta:example".to_owned()), tracked_view(40));

        assert_eq!(
            ordered_view_ids(&state),
            vec![
                ViewId::Status,
                ViewId::Room("!alpha:example".to_owned()),
                ViewId::Thread {
                    room_id: "!alpha:example".to_owned(),
                    thread_root_event_id: "$thread-root".to_owned(),
                },
                ViewId::Room("!beta:example".to_owned()),
            ]
        );
    }

    #[test]
    fn view_short_title_names_status_by_the_user_id() {
        let mut state = test_state();
        assert_eq!(view_short_title(&state.model, &ViewId::Status), "Status");
        state.model.own_user_id = Some("@me:example".to_owned());
        assert_eq!(
            view_short_title(&state.model, &ViewId::Status),
            "@me:example"
        );
    }

    #[test]
    fn view_caption_adds_the_room_topic_to_the_short_title() {
        let mut state = test_state();
        let mut invited_room = room("!invite:example", "Invite", false);
        invited_room.membership = RoomMembership::Invite;
        state.model.rooms = vec![
            room("!room:example", "Room", true),
            invited_room,
            room("!plain:example", "Plain", false),
        ];
        for (room_id, topic) in [
            ("!room:example", "pairing\nnotes"),
            ("!invite:example", "come join"),
        ] {
            state.model.room_views.insert(
                room_id.to_owned(),
                RoomViewModel {
                    topic: Some(topic.to_owned()),
                    threads: BTreeMap::from([(
                        "$thread-root".to_owned(),
                        ThreadViewModel {
                            label: "root msg".to_owned(),
                        },
                    )]),
                    ..RoomViewModel::default()
                },
            );
        }

        assert_eq!(
            view_caption(&state.model, &room_view_id("!room:example")),
            "Room: pairing notes"
        );
        assert_eq!(
            view_caption(&state.model, &room_view_id("!invite:example")),
            format!(
                "{}: come join",
                view_short_title(&state.model, &room_view_id("!invite:example"))
            )
        );
        assert_eq!(
            view_caption(&state.model, &room_view_id("!plain:example")),
            "Plain"
        );
        assert_eq!(view_caption(&state.model, &ViewId::Status), "Status");
        state.model.own_user_id = Some("@me:example".to_owned());
        assert_eq!(
            view_caption(&state.model, &ViewId::Status),
            "Status: @me:example"
        );
        assert_eq!(
            view_caption(
                &state.model,
                &thread_view_id("!room:example", "$thread-root")
            ),
            "Thread: root msg"
        );
    }

    #[test]
    fn view_caption_names_a_query_by_user_id() {
        let mut state = test_state();
        let mut query = room("!alice:example", "Alice", true);
        query.is_direct = true;
        query.direct_user_ids = vec!["@alice:example".to_owned()];
        let mut group_query = room("!group:example", "Group", false);
        group_query.is_direct = true;
        group_query.direct_user_ids = vec!["@alice:example".to_owned(), "@bob:example".to_owned()];
        state.model.rooms = vec![query, group_query];
        for room_id in ["!alice:example", "!group:example"] {
            state.model.room_views.insert(
                room_id.to_owned(),
                RoomViewModel {
                    topic: Some("ignored".to_owned()),
                    ..RoomViewModel::default()
                },
            );
        }

        assert_eq!(
            view_caption(&state.model, &room_view_id("!alice:example")),
            "Alice (@alice:example)"
        );
        assert_eq!(
            view_caption(&state.model, &room_view_id("!group:example")),
            "Group"
        );
    }

    #[test]
    fn navigation_layout_keeps_closed_queries() {
        let mut state = test_state();
        state.model.rooms = vec![query("!dm:example", "Alice", "@alice:example")];
        let open_layout = navigation_layout(&state.model);
        state.closed_queries.insert("!dm:example".to_owned());

        assert_eq!(navigation_layout(&state.model), open_layout);
        assert_eq!(
            open_layout,
            nav_view(
                "Status",
                ViewId::Status,
                vec![nav_room("Alice", "!dm:example")]
            )
        );
    }

    #[test]
    fn switchbar_button_state_tracks_only_the_checked_bit() {
        let mut state = test_state();
        let room_view = room_view_id("!room:example");

        state
            .room_activity
            .insert(room_view.clone(), ViewActivityLevel::Highlight);
        assert_eq!(
            switchbar_button_state(&state, &room_view),
            TBSTATE_ENABLED as u8
        );

        state.room_activity.clear();
        state.active_view = room_view.clone();
        assert_eq!(
            switchbar_button_state(&state, &room_view),
            (TBSTATE_ENABLED | TBSTATE_CHECKED) as u8
        );
    }

    fn switchbar_draw(stage: u32, command_id: usize) -> NMTBCUSTOMDRAW {
        let mut draw = NMTBCUSTOMDRAW::default();
        draw.nmcd.dwDrawStage = stage;
        draw.nmcd.dwItemSpec = command_id;
        draw.clrText = rgb(1, 2, 3);
        draw
    }

    #[test]
    fn switchbar_custom_draw_colors_buttons_by_activity_level() {
        let mut state = test_state();
        state.active_view = ViewId::Status;
        state.switchbar_views = vec![
            ViewId::Status,
            room_view_id("!event:example"),
            room_view_id("!message:example"),
            room_view_id("!highlight:example"),
        ];
        state.room_activity = BTreeMap::from([
            (room_view_id("!event:example"), ViewActivityLevel::Event),
            (room_view_id("!message:example"), ViewActivityLevel::Message),
            (
                room_view_id("!highlight:example"),
                ViewActivityLevel::Highlight,
            ),
        ]);

        let mut prepaint = switchbar_draw(CDDS_PREPAINT, 0);
        assert_eq!(
            switchbar_custom_draw(&state, &mut prepaint),
            CDRF_NOTIFYITEMDRAW as LRESULT
        );
        assert_eq!(prepaint.clrText, rgb(1, 2, 3));

        let color = |index| PaletteIndex::new(index).unwrap().colorref();
        for (index, expected) in [rgb(1, 2, 3), color(5), color(4), color(12)]
            .into_iter()
            .enumerate()
        {
            let mut draw = switchbar_draw(CDDS_ITEMPREPAINT, SWITCHBAR_COMMAND_FIRST + index);
            assert_eq!(switchbar_custom_draw(&state, &mut draw), CDRF_DEFAULT);
            assert_eq!(draw.clrText, expected, "button {index}");
        }

        // Options > Display's Message colour.
        state
            .settings
            .update(|settings| {
                settings.switchbar.activity.message = PaletteIndex::new(9).unwrap();
            })
            .unwrap();
        let mut draw = switchbar_draw(CDDS_ITEMPREPAINT, SWITCHBAR_COMMAND_FIRST + 2);
        switchbar_custom_draw(&state, &mut draw);
        assert_eq!(draw.clrText, color(9));
    }

    #[test]
    #[should_panic(expected = "the active view never records activity")]
    fn switchbar_custom_draw_rejects_activity_on_the_active_view() {
        let mut state = test_state();
        state.active_view = room_view_id("!room:example");
        state.switchbar_views = vec![room_view_id("!room:example")];
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);

        switchbar_custom_draw(
            &state,
            &mut switchbar_draw(CDDS_ITEMPREPAINT, SWITCHBAR_COMMAND_FIRST),
        );
    }

    #[test]
    fn tree_custom_draw_colors_view_items_and_leaves_childless_folders() {
        let mut state = test_state();
        state.active_view = ViewId::Status;
        let leaf = |node| TreeItem {
            node,
            descendant_views: None,
        };
        state.tree_items = BTreeMap::from([
            (
                10,
                leaf(NavNode::Space {
                    room_id: "!space:example".to_owned(),
                }),
            ),
            (11, leaf(NavNode::View(room_view_id("!room:example")))),
            (
                12,
                leaf(NavNode::View(thread_view_id("!room:example", "$root"))),
            ),
        ]);
        state.room_activity.insert(
            thread_view_id("!room:example", "$root"),
            ViewActivityLevel::Highlight,
        );

        let draw = |stage, item: usize| {
            let mut draw = NMTVCUSTOMDRAW::default();
            draw.nmcd.dwDrawStage = stage;
            draw.nmcd.dwItemSpec = item;
            draw.clrText = rgb(1, 2, 3);
            let reply = tree_custom_draw(&state, &mut draw);
            (reply, draw.clrText)
        };

        assert_eq!(
            draw(CDDS_PREPAINT, 0),
            (CDRF_NOTIFYITEMDRAW as LRESULT, rgb(1, 2, 3))
        );
        assert_eq!(draw(CDDS_ITEMPREPAINT, 10), (CDRF_DEFAULT, rgb(1, 2, 3)));
        assert_eq!(draw(CDDS_ITEMPREPAINT, 11), (CDRF_DEFAULT, rgb(1, 2, 3)));
        assert_eq!(
            draw(CDDS_ITEMPREPAINT, 12),
            (CDRF_DEFAULT, PaletteIndex::new(12).unwrap().colorref())
        );
    }

    #[test]
    fn a_collapsed_item_shows_the_highest_activity_under_it() {
        let mut state = test_state();
        state.active_view = room_view_id("!active:example");
        let parent = |node, descendants: &[ViewId]| TreeItem {
            node,
            descendant_views: Some(descendants.to_vec()),
        };
        state.tree_items = BTreeMap::from([
            (
                1,
                parent(
                    NavNode::View(ViewId::Status),
                    &[
                        room_view_id("!active:example"),
                        room_view_id("!chat:example"),
                        thread_view_id("!chat:example", "$root"),
                    ],
                ),
            ),
            (
                2,
                parent(
                    NavNode::Space {
                        room_id: "!muted-space:example".to_owned(),
                    },
                    &[room_view_id("!muted:example")],
                ),
            ),
            (
                3,
                parent(
                    NavNode::Space {
                        room_id: "!quiet-space:example".to_owned(),
                    },
                    &[room_view_id("!quiet:example")],
                ),
            ),
            (
                4,
                parent(
                    NavNode::View(room_view_id("!chat:example")),
                    &[thread_view_id("!chat:example", "$root")],
                ),
            ),
            (
                5,
                TreeItem {
                    node: NavNode::View(room_view_id("!muted:example")),
                    descendant_views: None,
                },
            ),
        ]);
        // The active view records none: it never shows in a roll-up.
        state.room_activity = BTreeMap::from([
            (room_view_id("!chat:example"), ViewActivityLevel::Message),
            (
                thread_view_id("!chat:example", "$root"),
                ViewActivityLevel::Highlight,
            ),
            // A muted room only ever reaches Event (core_events tests it).
            (room_view_id("!muted:example"), ViewActivityLevel::Event),
        ]);

        let activity = |item, expanded: bool| tree_item_activity(&state, item, || expanded);
        // Expanded: the item's own view only.
        assert_eq!(activity(1, true), None);
        assert_eq!(activity(4, true), Some(ViewActivityLevel::Message));
        assert_eq!(activity(2, true), None);
        // Collapsed: the highest of its own and everything under it.
        assert_eq!(activity(1, false), Some(ViewActivityLevel::Highlight));
        assert_eq!(activity(4, false), Some(ViewActivityLevel::Highlight));
        assert_eq!(activity(2, false), Some(ViewActivityLevel::Event));
        assert_eq!(activity(3, false), None);
        // An item without children is never asked whether it is expanded.
        assert_eq!(
            tree_item_activity(&state, 5, || unreachable!()),
            Some(ViewActivityLevel::Event)
        );
    }

    #[test]
    fn descendant_views_lists_every_view_under_an_item_in_tree_order() {
        let thread = thread_view_id("!ops:example", "$root");
        let item = nav_folder(
            "Work",
            "!work:example",
            vec![
                nav_view(
                    "Ops",
                    room_view_id("!ops:example"),
                    vec![nav_view("root", thread.clone(), Vec::new())],
                ),
                nav_folder(
                    "Team",
                    "!team:example",
                    vec![
                        nav_room("Shared", "!shared:example"),
                        nav_folder("Empty", "!empty:example", Vec::new()),
                    ],
                ),
                nav_room("Lobby", "!lobby:example"),
            ],
        );

        assert_eq!(
            item.descendant_views(),
            vec![
                room_view_id("!ops:example"),
                thread,
                room_view_id("!shared:example"),
                room_view_id("!lobby:example"),
            ]
        );
        assert_eq!(
            nav_room("Lobby", "!lobby:example").descendant_views(),
            Vec::new()
        );
    }

    #[test]
    fn caught_up_zero_read_state_clears_the_room_colour() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        apply_core_event(
            &mut state,
            room_read_state("!room:example", None, None, false, 3, 0, 0),
        )
        .unwrap();
        assert_eq!(
            state.room_activity.get(&room_view_id("!room:example")),
            Some(&ViewActivityLevel::Message)
        );

        apply_core_event(
            &mut state,
            room_read_state(
                "!room:example",
                Some("$latest"),
                Some("$latest"),
                false,
                0,
                0,
                0,
            ),
        )
        .unwrap();
        assert_eq!(
            state.room_activity.get(&room_view_id("!room:example")),
            None
        );
        assert_eq!(
            view_short_title(&state.model, &room_view_id("!room:example")),
            "Room"
        );
    }

    #[test]
    fn select_room_rejects_spaces_without_changing_existing_selection() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!room:example", "Room", false),
            space("!space:example", "Space"),
        ];

        assert_eq!(select_room(&mut state, "!room:example"), Ok(()));
        assert_eq!(state.model.selected_room, Some("!room:example".to_owned()));
        assert_eq!(
            select_room(&mut state, "!space:example"),
            Err("unknown room id: !space:example".to_owned())
        );
        assert_eq!(
            select_room(&mut state, "!missing:example"),
            Err("unknown room id: !missing:example".to_owned())
        );
        assert_eq!(state.model.selected_room, Some("!room:example".to_owned()));
    }
}
