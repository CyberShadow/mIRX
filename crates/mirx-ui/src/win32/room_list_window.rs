//! The `/list` ListView window; the listing itself is `room_list.rs`.

use super::*;

/// Copies `text` into a control's text buffer of `capacity` UTF-16 units,
/// NUL-terminated and cut short when it does not fit, never inside a
/// surrogate pair.
unsafe fn copy_text_to_buffer(text: &str, buffer: *mut u16, capacity: i32) {
    let capacity = usize::try_from(capacity).expect("a text buffer has a capacity");
    assert!(capacity > 0, "a text buffer holds at least the NUL");
    let encoded = text.encode_utf16().collect::<Vec<_>>();
    let mut copied = encoded.len().min(capacity - 1);
    if copied < encoded.len() && (0xD800..0xDC00).contains(&encoded[copied]) {
        // `copied` is the low half of a pair cut off: drop the high half.
        copied -= 1;
    }
    copy_nonoverlapping(encoded.as_ptr(), buffer, copied);
    *buffer.add(copied) = 0;
}

/// A directory page fills the Rooms List and asks for the next one. A page
/// for a listing the user closed or replaced is dropped.
pub(super) fn apply_public_rooms_page(
    state: &mut WindowState,
    page: PublicRoomsPage,
) -> Result<(), String> {
    let Some(room_list) = &mut state.model.room_list else {
        return Ok(());
    };
    match room_list.apply_page(page) {
        Err(room_list::Ignored) | Ok(PageApplied::Finished) => {}
        Ok(PageApplied::Next(request)) => {
            enqueue_command(state, Command::ListPublicRooms(request))?;
        }
        Ok(PageApplied::Failed(message)) => {
            log_status(state, StatusLineKind::Error, format!("/list: {message}"));
        }
    }
    Ok(())
}

/// `/list`: opens the Rooms List, or reuses it, for a new listing and asks
/// for its first page. The window follows the model: the view sync creates
/// it.
pub(super) unsafe fn start_room_list(hwnd: HWND, query: RoomListQuery) -> Result<String, String> {
    let state = state_mut(hwnd);
    let room_list = RoomListState::new(query, own_server(&state.model)?);
    enqueue_command(state, Command::ListPublicRooms(room_list.first_request()))?;
    state.model.room_list = Some(room_list);
    sync_views_and_tree(hwnd)?;
    let list = mdi_child_content(ensure_room_list_view(hwnd)?);
    // A new listing starts with nothing selected.
    let unselect = LVITEMW {
        stateMask: LVIS_SELECTED | LVIS_FOCUSED,
        ..LVITEMW::default()
    };
    SendMessageW(
        list,
        LVM_SETITEMSTATE,
        usize::MAX,
        &unselect as *const _ as LPARAM,
    );
    activate_view(hwnd, &ViewId::RoomList, None)?;
    Ok("listing rooms".to_owned())
}

/// Column widths in pixels at 96 dpi, for the frame's Fixedsys text.
const ROOM_LIST_COLUMN_WIDTHS: [i32; 4] = [240, 64, 200, 640];

pub(super) unsafe fn add_room_list_columns(list: HWND) -> Result<(), String> {
    SendMessageW(
        list,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        LVS_EX_FULLROWSELECT as WPARAM,
        LVS_EX_FULLROWSELECT as LPARAM,
    );
    let dpi = Dpi::of_window(list);
    for (index, column) in RoomListColumn::ALL.into_iter().enumerate() {
        let mut title = wide_null(column.title());
        let format = match column {
            RoomListColumn::Users => LVCFMT_RIGHT,
            RoomListColumn::Room | RoomListColumn::Name | RoomListColumn::Topic => LVCFMT_LEFT,
        };
        let lv_column = LVCOLUMNW {
            mask: LVCF_FMT | LVCF_TEXT | LVCF_WIDTH,
            fmt: format,
            cx: dpi.px(ROOM_LIST_COLUMN_WIDTHS[index]),
            pszText: title.as_mut_ptr(),
            ..LVCOLUMNW::default()
        };
        if SendMessageW(
            list,
            LVM_INSERTCOLUMNW,
            index,
            &lv_column as *const _ as LPARAM,
        ) != index as LRESULT
        {
            return Err(format!("LVM_INSERTCOLUMNW {} failed", column.title()));
        }
    }
    Ok(())
}

/// Tells the owner-data list how many rows the listing has; it asks for
/// their text as it paints them.
pub(super) unsafe fn refresh_room_list_rows(state: &WindowState, list: HWND) {
    let rows = state
        .model
        .room_list
        .as_ref()
        .expect("the Rooms List window has its listing")
        .len();
    SendMessageW(list, LVM_SETITEMCOUNT, rows, LVSICF_NOSCROLL as LPARAM);
    InvalidateRect(list, null(), 1);
}

/// The Rooms List's notifications; `None` for any other, which the MDI
/// child's default handling gets.
pub(super) unsafe fn handle_room_list_notify(frame: HWND, lparam: LPARAM) -> Option<LRESULT> {
    let header = &*(lparam as *const NMHDR);
    match header.code {
        LVN_GETDISPINFOW => {
            let item = &mut (*(lparam as *mut NMLVDISPINFOW)).item;
            // Text is all the list asks the listing for.
            if item.mask & LVIF_TEXT != 0 {
                let text = state_ref(frame)
                    .model
                    .room_list
                    .as_ref()
                    .expect("the Rooms List window has its listing")
                    .cell_text(
                        item.iItem as usize,
                        RoomListColumn::from_index(item.iSubItem as usize),
                    );
                copy_text_to_buffer(&text, item.pszText, item.cchTextMax);
            }
            Some(0)
        }
        LVN_COLUMNCLICK => {
            let column = (*(lparam as *const NMLISTVIEW)).iSubItem as usize;
            state_mut(frame)
                .model
                .room_list
                .as_mut()
                .expect("the Rooms List window has its listing")
                .sort_by(RoomListColumn::from_index(column));
            if let Err(message) = refresh_view_content(frame, &ViewId::RoomList) {
                show_error_message(frame, ROOM_LIST_TITLE, &message);
            }
            Some(0)
        }
        // A double-click below the rows hits no item.
        NM_DBLCLK => {
            let item = (*(lparam as *const NMITEMACTIVATE)).iItem;
            Some(join_room_list_row_reporting(frame, item))
        }
        NM_RETURN => {
            let item = SendMessageW(
                header.hwndFrom,
                LVM_GETNEXTITEM,
                usize::MAX,
                LVNI_SELECTED as LPARAM,
            );
            Some(join_room_list_row_reporting(frame, item as i32))
        }
        _ => None,
    }
}

/// Joins the row at `item`, unless it is -1 (no row); errors go where a
/// typed command's would.
unsafe fn join_room_list_row_reporting(frame: HWND, item: i32) -> LRESULT {
    if item >= 0
        && let Err(message) = join_room_list_row(frame, item as usize)
    {
        let error = CommandError::new("list", message);
        if let Err(message) = report_command_error(frame, &ViewId::RoomList, error) {
            show_error_message(frame, ROOM_LIST_TITLE, &message);
        }
    }
    0
}

/// Double-click or Enter on a row: a room already joined comes to the
/// front; any other is joined, and opens when the join lands.
unsafe fn join_room_list_row(frame: HWND, display_index: usize) -> Result<(), String> {
    let (room_id, target, via) = {
        let room_list = state_ref(frame)
            .model
            .room_list
            .as_ref()
            .expect("the Rooms List window has its listing");
        let (target, via) = room_list.join_target(display_index);
        (room_list.row(display_index).room_id.clone(), target, via)
    };
    if navigable_room_by_id(&state_ref(frame).model.rooms, &room_id).is_some() {
        return activate_view(frame, &ViewId::Room(room_id), None);
    }
    let status = submit_join_room(state_mut(frame), target, via)?;
    set_local_command_status(frame, status)
}

pub(super) unsafe fn ensure_room_list_view(hwnd: HWND) -> Result<HWND, String> {
    if state_ref(hwnd).model.room_list.is_none() {
        return Err("no rooms are listed: use /list".to_owned());
    }
    ensure_view(hwnd, ViewId::RoomList)
}

/// The Rooms List as its list control shows it: the header's titles and
/// every row's text as the list asks the listing for it, through
/// `LVN_GETDISPINFOW`.
pub(super) unsafe fn dump_room_list(list: HWND) -> UiRoomList {
    let header = SendMessageW(list, LVM_GETHEADER, 0, 0) as HWND;
    assert!(!header.is_null(), "a report list has a header");
    let column_count = SendMessageW(header, HDM_GETITEMCOUNT, 0, 0);
    let column_count = usize::try_from(column_count).expect("HDM_GETITEMCOUNT succeeds");
    let mut buffer = [0u16; 512];
    let mut columns = Vec::new();
    let mut column_click_points = Vec::new();
    for column in 0..column_count {
        let mut lv_column = LVCOLUMNW {
            mask: LVCF_TEXT,
            pszText: buffer.as_mut_ptr(),
            cchTextMax: buffer.len() as i32,
            ..LVCOLUMNW::default()
        };
        assert_ne!(
            SendMessageW(
                list,
                LVM_GETCOLUMNW,
                column,
                &mut lv_column as *mut _ as LPARAM
            ),
            0,
            "LVM_GETCOLUMNW must read column {column}"
        );
        columns.push(wide_buffer_text(&buffer));
        let mut rect = RECT::default();
        assert_ne!(
            SendMessageW(
                header,
                HDM_GETITEMRECT,
                column,
                &mut rect as *mut _ as LPARAM
            ),
            0,
            "HDM_GETITEMRECT must place column {column}"
        );
        column_click_points.push(rect_center(&rect));
    }

    let row_count = usize::try_from(SendMessageW(list, LVM_GETITEMCOUNT, 0, 0))
        .expect("LVM_GETITEMCOUNT succeeds");
    let rows = (0..row_count)
        .map(|row| {
            (0..column_count)
                .map(|column| {
                    let mut item = LVITEMW {
                        iSubItem: column as i32,
                        pszText: buffer.as_mut_ptr(),
                        cchTextMax: buffer.len() as i32,
                        ..LVITEMW::default()
                    };
                    SendMessageW(list, LVM_GETITEMTEXTW, row, &mut item as *mut _ as LPARAM);
                    wide_buffer_text(&buffer)
                })
                .collect()
        })
        .collect();

    let top_row = usize::try_from(SendMessageW(list, LVM_GETTOPINDEX, 0, 0))
        .expect("LVM_GETTOPINDEX succeeds");
    let per_page = usize::try_from(SendMessageW(list, LVM_GETCOUNTPERPAGE, 0, 0))
        .expect("LVM_GETCOUNTPERPAGE succeeds");
    let visible_row_click_points = (top_row..row_count.min(top_row + per_page))
        .map(|row| {
            let mut rect = RECT {
                left: LVIR_LABEL as i32,
                ..RECT::default()
            };
            assert_ne!(
                SendMessageW(list, LVM_GETITEMRECT, row, &mut rect as *mut _ as LPARAM),
                0,
                "LVM_GETITEMRECT must place row {row}"
            );
            rect_center(&rect)
        })
        .collect();
    let selected = SendMessageW(list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as LPARAM);

    UiRoomList {
        columns,
        rows,
        selected: usize::try_from(selected).ok(),
        column_click_points,
        visible_row_click_points,
        top_row,
    }
}

/// The text before the NUL a control wrote into `buffer`.
pub(super) fn wide_buffer_text(buffer: &[u16]) -> String {
    let len = buffer
        .iter()
        .position(|&unit| unit == 0)
        .expect("the control NUL-terminates its text");
    String::from_utf16_lossy(&buffer[..len])
}

pub(super) fn rect_center(rect: &RECT) -> UiPoint {
    UiPoint {
        x: (rect.left + rect.right) / 2,
        y: (rect.top + rect.bottom) / 2,
    }
}

#[cfg(test)]
mod tests {
    use mirx_core::{ListPublicRoomsRequest, PublicRoomsResult};

    use super::super::test_fixtures::*;
    use super::*;

    fn listing_rooms(filter: Option<&str>) -> RoomListState {
        RoomListState::new(
            RoomListQuery {
                server: None,
                filter: filter.map(str::to_owned),
            },
            "example",
        )
    }

    fn public_rooms_page(
        request: ListPublicRoomsRequest,
        room_ids: &[&str],
        next_batch: Option<&str>,
    ) -> Event {
        Event::PublicRooms(PublicRoomsPage {
            request,
            result: PublicRoomsResult::Loaded {
                rooms: room_ids
                    .iter()
                    .map(|room_id| mirx_core::PublicRoom {
                        room_id: (*room_id).to_owned(),
                        canonical_alias: None,
                        name: None,
                        topic: None,
                        joined_members: 1,
                    })
                    .collect(),
                next_batch: next_batch.map(str::to_owned),
                total_estimate: None,
            },
        })
    }

    #[test]
    fn public_rooms_page_fills_the_listing_and_asks_for_the_next_page() {
        let (mut state, handle) = test_state_with_command_sender();
        let room_list = listing_rooms(Some("rust"));
        let first = room_list.first_request();
        state.model.room_list = Some(room_list);

        apply_core_event(
            &mut state,
            public_rooms_page(first, &["!a:example", "!b:example"], Some("page2")),
        )
        .unwrap();

        assert_eq!(state.model.room_list.as_ref().unwrap().len(), 2);
        assert_eq!(
            state.harness.commands,
            vec![Command::ListPublicRooms(ListPublicRoomsRequest {
                server: None,
                filter: Some("rust".to_owned()),
                since: Some("page2".to_owned()),
                limit: room_list::ROOM_LIST_PAGE_SIZE,
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_room_list_requests,
            vec![DumpSubmittedRoomListRequest {
                server: None,
                filter: Some("rust".to_owned()),
                since: Some("page2".to_owned()),
                limit: room_list::ROOM_LIST_PAGE_SIZE,
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn public_rooms_page_after_the_listing_closed_is_dropped() {
        let mut state = test_state();
        let first = listing_rooms(None).first_request();

        apply_core_event(
            &mut state,
            public_rooms_page(first, &["!a:example"], Some("page2")),
        )
        .unwrap();

        assert!(state.model.room_list.is_none());
        assert!(state.harness.commands.is_empty());
    }

    #[test]
    fn failed_public_rooms_page_logs_the_error_to_status() {
        let mut state = test_state();
        let room_list = listing_rooms(None);
        let request = room_list.first_request();
        state.model.room_list = Some(room_list);

        apply_core_event(
            &mut state,
            Event::PublicRooms(PublicRoomsPage {
                request,
                result: PublicRoomsResult::Failed {
                    message: "failed to list public rooms: forbidden".to_owned(),
                },
            }),
        )
        .unwrap();

        assert!(state.harness.commands.is_empty());
        assert_eq!(
            state
                .model
                .status_log
                .lines()
                .last()
                .map(|line| line.text.as_str()),
            Some("* /list: failed to list public rooms: forbidden")
        );
        assert_eq!(
            view_caption(&state.model, &ViewId::RoomList),
            "Rooms List [example] - failed"
        );
    }

    #[test]
    fn the_rooms_list_view_exists_while_its_listing_does() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        let resolve = |state: &WindowState| {
            super::super::views::resolve_active_view(
                &ViewId::RoomList,
                None,
                &state.model,
                &state.closed_queries,
            )
        };
        assert!(!view_id_is_valid(&state.model, &ViewId::RoomList));
        assert_eq!(resolve(&state), room_view_id("!ops:example"));
        assert_eq!(
            navigation_layout(&state.model),
            nav_view(
                "Status",
                ViewId::Status,
                vec![nav_room("Ops", "!ops:example")]
            )
        );

        state.model.room_list = Some(listing_rooms(None));

        assert!(view_id_is_valid(&state.model, &ViewId::RoomList));
        assert_eq!(resolve(&state), ViewId::RoomList);
        assert_eq!(
            navigation_layout(&state.model),
            nav_view(
                "Status",
                ViewId::Status,
                vec![
                    nav_room("Ops", "!ops:example"),
                    nav_view("Rooms List", ViewId::RoomList, Vec::new()),
                ]
            )
        );
        state.views.insert(ViewId::RoomList, tracked_view(30));
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(room_view_id("!ops:example"), tracked_view(20));
        assert_eq!(
            ordered_view_ids(&state),
            vec![
                ViewId::Status,
                room_view_id("!ops:example"),
                ViewId::RoomList
            ]
        );
    }

    #[test]
    fn logging_out_drops_the_rooms_list() {
        let mut state = test_state();
        state.model.room_list = Some(listing_rooms(None));
        state.views.insert(ViewId::Status, tracked_view(10));
        state.views.insert(ViewId::RoomList, tracked_view(20));
        state.hwnd_views.insert(10, ViewId::Status);
        state.hwnd_views.insert(20, ViewId::RoomList);

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut).unwrap();
        });
        let removed = take_stale_room_views(&mut state);

        assert!(state.model.room_list.is_none());
        assert_eq!(removed.len(), 1);
        assert!(!state.views.contains_key(&ViewId::RoomList));
    }
}
