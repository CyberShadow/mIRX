//! The timeline control: its window procedure, content, scrolling,
//! selection, history backfill anchor and Go to Original.

use super::*;
use windows_sys::Win32::System::SystemServices::MK_CONTROL;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, HTCLIENT, IDC_HAND, SM_CXDRAG, SM_CYDRAG,
    SPI_GETWHEELSCROLLLINES, SetCursor, SystemParametersInfoW, WHEEL_DELTA, WM_CAPTURECHANGED,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_SETCURSOR,
};

use super::clipboard::copy_to_clipboard;
use super::hotlinks::{LinkKind, link_at};
use super::open_link::open_link;
use super::text_selection::{
    LineGeometry, TextDrag, TextPoint, exact_point_at, left_drag_box, point_at, remap,
    selected_text,
};

const WM_TIMELINE_GET_TOP_LINE: u32 = WM_APP + 100;

const TIMELINE_VERTICAL_PADDING: i32 = 6;

pub(super) struct TimelineState {
    pub(super) source_rows: Vec<TimelineRow>,
    rendered_text: String,
    lines: Vec<String>,
    pub(super) paint_lines: Vec<TimelinePaintLine>,
    pub(super) selected_find_source_row_index: Option<usize>,
    pub(super) top_line: usize,
    /// The `WM_SETFONT` font, borrowed from the frame's `TextFont`; null
    /// draws in the system font.
    pub(super) font: HFONT,
    /// The height of a line in `font`.
    pub(super) line_height: i32,
    pub(super) vertical_padding: i32,
    /// The timestamp prefix sample that log rows' continuation lines are
    /// indented past; set with the rows, as the rows were stamped with it.
    pub(super) indent_sample: String,
    /// The wheel's turn since its last whole notch, for wheels that report
    /// finer steps than `WHEEL_DELTA`.
    wheel_remainder: i32,
    /// The rows `top_line` was last placed for. A resize or a new font
    /// changes the client's rows before the control hears of it, and whether
    /// the window showed its newest line is a question about the old layout.
    laid_out_visible_rows: usize,
    /// The left button marking text, from its press to its release.
    pub(super) drag: Option<TextDrag>,
}

impl TimelineState {
    fn new(line_height: i32) -> Self {
        Self {
            source_rows: Vec::new(),
            rendered_text: String::new(),
            lines: Vec::new(),
            paint_lines: Vec::new(),
            selected_find_source_row_index: None,
            top_line: 0,
            font: null_mut(),
            line_height,
            vertical_padding: TIMELINE_VERTICAL_PADDING,
            indent_sample: String::new(),
            wheel_remainder: 0,
            laid_out_visible_rows: 1,
            drag: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TimelineBackfillAnchor {
    room_id: String,
    source_row: TimelineRow,
    source_row_index: usize,
    source_row_count: usize,
}

pub(super) fn register_timeline_class(instance: *mut c_void) -> Result<(), String> {
    let class_name = wide_null(TIMELINE_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        // A double-click on a quote line goes to the message it quotes.
        style: CS_DBLCLKS,
        lpfnWndProc: Some(timeline_window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_WINDOW as isize + 1) as HBRUSH,
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: null_mut(),
    };

    let atom = unsafe { RegisterClassExW(&window_class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        if error != 1410 {
            return Err(last_os_error("RegisterClassExW timeline"));
        }
    }

    Ok(())
}

unsafe extern "system" fn timeline_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            let line_height = font_line_height(null_mut());
            window_data::adopt(hwnd, TimelineState::new(line_height));
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_SETFONT => {
            set_timeline_font(hwnd, wparam as HFONT);
            if lparam != 0 {
                InvalidateRect(hwnd, null(), 1);
            }
            0
        }
        WM_GETFONT => timeline_state_ref(hwnd).font as LRESULT,
        WM_SETTEXT => set_timeline_text(hwnd, lparam as *const u16),
        WM_GETTEXTLENGTH => timeline_text_len(hwnd),
        WM_GETTEXT => timeline_get_text(hwnd, wparam, lparam),
        WM_TIMELINE_GET_TOP_LINE => timeline_state_ref(hwnd).top_line as LRESULT,
        WM_SIZE => {
            relayout_timeline(hwnd);
            InvalidateRect(hwnd, null(), 1);
            0
        }
        WM_VSCROLL => scroll_timeline(hwnd, wparam),
        WM_MOUSEWHEEL => {
            scroll_timeline_with_wheel(hwnd, wparam);
            0
        }
        // A press selects the message for the menus, and starts marking
        // text; moving with the button down marks, and the release copies
        // what is marked, as mIRC does.
        WM_LBUTTONDOWN => {
            select_timeline_row(hwnd, lparam);
            start_text_drag(hwnd, client_point(lparam));
            0
        }
        WM_MOUSEMOVE => {
            if timeline_state_ref(hwnd).drag.is_some() {
                extend_text_drag(hwnd, client_point(lparam));
            }
            0
        }
        WM_LBUTTONUP => {
            finish_text_drag(hwnd, wparam as u32 & MK_CONTROL != 0);
            0
        }
        // Another window took the mouse: the mark goes, copied or not.
        WM_CAPTURECHANGED => {
            timeline_state_mut(hwnd).drag = None;
            InvalidateRect(hwnd, null(), 1);
            0
        }
        // The pointer is a hand over a link, as in mIRC.
        WM_SETCURSOR
            if (lparam & 0xffff) as u32 == HTCLIENT && timeline_link_under_cursor(hwnd) =>
        {
            SetCursor(LoadCursorW(null_mut(), IDC_HAND));
            1
        }
        // The second click of a double-click: it selects as a click does,
        // and follows a link, or on a quote line goes to the quoted message.
        // It marks nothing.
        WM_LBUTTONDBLCLK => {
            select_timeline_row(hwnd, lparam);
            let frame = frame_window_for(hwnd);
            if let Some((text, kind)) = timeline_link_at(hwnd, client_point(lparam)) {
                if let Err(message) = open_link(frame, &text, kind) {
                    show_error_message(frame, "Link", &message);
                }
                return 0;
            }
            let y = ((lparam >> 16) as i16) as i32;
            let on_quote = timeline_source_row_index_at_client_y(hwnd, y).is_some_and(|index| {
                timeline_state_ref(hwnd).source_rows[index].kind == TimelineRowKind::ReplyQuote
            });
            if on_quote && let Err(message) = go_to_original_of_selected_message(frame) {
                show_error_message(frame, "Message", &message);
            }
            0
        }
        WM_CONTEXTMENU => {
            let frame = frame_window_for(hwnd);
            if let Err(message) = show_timeline_message_context_menu(hwnd, lparam, None) {
                show_error_message(frame, "Message", &message);
            }
            0
        }
        WM_PAINT => {
            paint_timeline(hwnd);
            0
        }
        WM_NCDESTROY => {
            window_data::drop_owned::<TimelineState>(hwnd);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

pub(super) unsafe fn timeline_state_ref(hwnd: HWND) -> &'static TimelineState {
    timeline_state_mut(hwnd)
}

pub(super) unsafe fn timeline_state_mut(hwnd: HWND) -> &'static mut TimelineState {
    window_data::get(hwnd).expect("timeline state must exist while the control is alive")
}

unsafe fn timeline_client_width(hwnd: HWND) -> i32 {
    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);
    rect.right - rect.left
}

/// Rewraps the lines for the control's size and font. A window that showed
/// its newest line still does, as in mIRC; otherwise the top line stays,
/// within the new end.
pub(super) unsafe fn relayout_timeline(hwnd: HWND) {
    let client_width = timeline_client_width(hwnd);
    let (source_rows, lines, tail_pinned) = {
        let state = timeline_state_ref(hwnd);
        (
            state.source_rows.clone(),
            state.lines.clone(),
            timeline_is_tail_pinned(
                state.source_rows.len(),
                state.top_line,
                state.paint_lines.len(),
                state.laid_out_visible_rows,
            ),
        )
    };
    let paint_lines = if source_rows.is_empty() {
        plain_timeline_paint_lines(&lines)
    } else {
        timeline_row_paint_lines(hwnd, &source_rows, client_width)
    };
    let visible_rows = timeline_visible_rows(hwnd);
    let state = timeline_state_mut(hwnd);
    state.paint_lines = paint_lines;
    state.laid_out_visible_rows = visible_rows;
    state.top_line = timeline_top_line_after_content_replaced(
        state.top_line,
        tail_pinned,
        state.paint_lines.len(),
        visible_rows,
    );
    // Showing or hiding the scroll bar resizes the client, which lays the
    // lines out again inside this call.
    update_timeline_scrollbar(hwnd);
}

unsafe fn apply_timeline_content(
    hwnd: HWND,
    source_rows: Vec<TimelineRow>,
    rendered_text: String,
    lines: Vec<String>,
    paint_lines: Vec<TimelinePaintLine>,
    retain_tail: bool,
) {
    let visible_rows = timeline_visible_rows(hwnd);
    let (top_line, tail_pinned) = {
        let state = timeline_state_ref(hwnd);
        (
            state.top_line,
            timeline_is_tail_pinned(
                state.source_rows.len(),
                state.top_line,
                state.paint_lines.len(),
                visible_rows,
            ),
        )
    };
    let state = timeline_state_mut(hwnd);
    // New lines arrive, and history above, while the user marks: the mark
    // follows its rows, and goes if one of them does or changes.
    state.drag = state.drag.and_then(|drag| {
        Some(TextDrag {
            anchor: remap(drag.anchor, &state.source_rows, &source_rows)?,
            focus: remap(drag.focus, &state.source_rows, &source_rows)?,
            ..drag
        })
    });
    state.source_rows = source_rows;
    state.rendered_text = rendered_text;
    state.lines = lines;
    state.paint_lines = paint_lines;
    state.laid_out_visible_rows = visible_rows;
    state.top_line = timeline_top_line_after_content_replaced(
        top_line,
        tail_pinned && retain_tail,
        state.paint_lines.len(),
        visible_rows,
    );
    update_timeline_scrollbar(hwnd);
    InvalidateRect(hwnd, null(), 1);
}

/// The client point of a mouse message, which is outside the client while
/// the mouse is captured.
fn client_point(lparam: LPARAM) -> (i32, i32) {
    (
        i32::from(lparam as u16 as i16),
        i32::from((lparam >> 16) as u16 as i16),
    )
}

pub(super) unsafe fn timeline_line_geometry(hwnd: HWND) -> LineGeometry {
    let state = timeline_state_ref(hwnd);
    LineGeometry {
        top_line: state.top_line,
        visible_rows: timeline_visible_rows(hwnd),
        line_height: state.line_height,
        vertical_padding: state.vertical_padding,
    }
}

type TextHitTest =
    fn(&[TimelinePaintLine], LineGeometry, i32, i32, &dyn Fn(&str) -> i32) -> Option<TextPoint>;

/// `hit` at client `at`, measuring in the text font.
pub(super) unsafe fn timeline_text_hit(
    hwnd: HWND,
    at: (i32, i32),
    hit: TextHitTest,
) -> Option<TextPoint> {
    let glyphs = &state_ref(frame_window_for(hwnd)).text_font.glyphs;
    let hdc = GetDC(hwnd);
    assert!(
        !hdc.is_null(),
        "GetDC must succeed for timeline hit testing"
    );
    let point = hit(
        &timeline_state_ref(hwnd).paint_lines,
        timeline_line_geometry(hwnd),
        at.0,
        at.1,
        &|text| glyphs.measure(hdc, text),
    );
    ReleaseDC(hwnd, hdc);
    point
}

/// The link under client `at`, if a character of one is there: its text
/// and what it opens.
unsafe fn timeline_link_at(hwnd: HWND, at: (i32, i32)) -> Option<(String, LinkKind)> {
    let point = timeline_text_hit(hwnd, at, exact_point_at)?;
    let text = &timeline_state_ref(hwnd).source_rows[point.row].logical_text;
    let link = link_at(text, point.offset)?;
    Some((text[link.range].to_owned(), link.kind))
}

unsafe fn timeline_link_under_cursor(hwnd: HWND) -> bool {
    let mut cursor = POINT::default();
    assert_ne!(GetCursorPos(&mut cursor), 0, "the cursor has a position");
    ScreenToClient(hwnd, &mut cursor);
    timeline_link_at(hwnd, (cursor.x, cursor.y)).is_some()
}

/// Starts marking at client `at`, if the timeline has rows there: plain
/// text set with `WM_SETTEXT` has none. Pressing selected the message
/// first, which may have refreshed the rows, so the point is found now.
unsafe fn start_text_drag(hwnd: HWND, at: (i32, i32)) {
    if let Some(point) = timeline_text_hit(hwnd, at, point_at) {
        timeline_state_mut(hwnd).drag = Some(TextDrag::new(point, at));
        // Marking goes on outside the window, clamped to its lines.
        SetCapture(hwnd);
    }
}

unsafe fn extend_text_drag(hwnd: HWND, at: (i32, i32)) {
    let drag = timeline_state_ref(hwnd)
        .drag
        .expect("only a drag is extended");
    let drag_box = (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG));
    if !drag.moved && !left_drag_box(drag.down, at, drag_box) {
        return;
    }
    let focus =
        timeline_text_hit(hwnd, at, point_at).expect("a timeline marking text shows its rows");
    timeline_state_mut(hwnd).drag = Some(TextDrag {
        focus,
        moved: true,
        ..drag
    });
    InvalidateRect(hwnd, null(), 1);
}

/// The release: copies the mark, with mIRC's control codes if Ctrl is
/// held, and drops it. The button message's own `MK_CONTROL` says whether
/// it was, for this very release.
unsafe fn finish_text_drag(hwnd: HWND, with_codes: bool) {
    // Taken before the capture goes, so WM_CAPTURECHANGED finds no drag.
    let drag = timeline_state_mut(hwnd).drag.take();
    ReleaseCapture();
    let Some((anchor, focus)) = drag.and_then(|drag| drag.mark()) else {
        return;
    };
    InvalidateRect(hwnd, null(), 1);
    let text = selected_text(
        &timeline_state_ref(hwnd).source_rows,
        anchor,
        focus,
        with_codes,
    );
    if let Err(error) = copy_to_clipboard(hwnd, &text) {
        let frame = frame_window_for(hwnd);
        if let Err(message) = report_error_line(frame, format!("* Unable to copy: {error}")) {
            show_error_message(frame, "Copy", &message);
        }
    }
}

unsafe fn set_timeline_text(hwnd: HWND, text_ptr: *const u16) -> LRESULT {
    let (rendered_text, lines) = normalize_timeline_text(&wide_ptr_to_string(text_ptr));
    let paint_lines = plain_timeline_paint_lines(&lines);
    apply_timeline_content(hwnd, Vec::new(), rendered_text, lines, paint_lines, false);
    1
}

/// Shows `rows`, whose timestamps are as wide as `indent_sample`.
pub(super) unsafe fn set_timeline_rows(hwnd: HWND, rows: &[TimelineRow], indent_sample: String) {
    timeline_state_mut(hwnd).indent_sample = indent_sample;
    let (rendered_text, lines) =
        normalize_timeline_text(&render_view_text(&project_timeline_rows(rows)));
    let paint_lines = timeline_row_paint_lines(hwnd, rows, timeline_client_width(hwnd));
    debug_assert_eq!(
        project_timeline_rows(rows),
        rows.iter()
            .map(|row| row.logical_text.clone())
            .collect::<Vec<_>>()
    );
    apply_timeline_content(hwnd, rows.to_vec(), rendered_text, lines, paint_lines, true);
}

unsafe fn select_timeline_row(hwnd: HWND, lparam: LPARAM) {
    let y = ((lparam >> 16) as i16) as i32;
    select_timeline_event_at_client_y(hwnd, y);
}

/// The source row of the line at client `y`, if a line is there.
unsafe fn timeline_source_row_index_at_client_y(hwnd: HWND, y: i32) -> Option<usize> {
    let state = timeline_state_ref(hwnd);
    let line_index = y
        .checked_sub(state.vertical_padding)
        .filter(|offset| *offset >= 0)
        .map(|offset| state.top_line + (offset / state.line_height) as usize);
    line_index
        .and_then(|index| state.paint_lines.get(index))
        .and_then(|line| line.source_row_index)
}

unsafe fn timeline_event_id_at_client_y(hwnd: HWND, y: i32) -> Option<String> {
    timeline_source_row_index_at_client_y(hwnd, y)
        .and_then(|index| timeline_state_ref(hwnd).source_rows[index].event_id.clone())
}

/// The first row of the event's lines: its quote line if it is a reply.
unsafe fn timeline_source_row_index_of_event(hwnd: HWND, event_id: &str) -> Option<usize> {
    timeline_state_ref(hwnd)
        .source_rows
        .iter()
        .position(|row| row.event_id.as_deref() == Some(event_id))
}

pub(super) unsafe fn select_timeline_event_at_client_y(hwnd: HWND, y: i32) {
    let event_id = timeline_event_id_at_client_y(hwnd, y);
    let frame = frame_window_for(hwnd);
    let window_state = state_mut(frame);
    window_state.selected_timeline_event = event_id.map(|event_id| SelectedTimelineEvent {
        view_id: window_state.active_view.clone(),
        event_id,
    });
    InvalidateRect(hwnd, null(), 1);
}

pub(super) fn validate_selected_timeline_event(
    state: &mut WindowState,
    view_id: &ViewId,
    rows: &[TimelineRow],
) {
    let Some(selected) = state.selected_timeline_event.as_ref() else {
        return;
    };
    if selected.view_id == *view_id
        && !rows
            .iter()
            .any(|row| row.event_id.as_deref() == Some(&selected.event_id))
    {
        state.selected_timeline_event = None;
    }
}

unsafe fn timeline_source_row_paint_line_count(hwnd: HWND, row: &TimelineRow) -> usize {
    timeline_row_paint_lines(hwnd, std::slice::from_ref(row), timeline_client_width(hwnd)).len()
}

pub(super) unsafe fn timeline_source_row_index_for_top_line(
    hwnd: HWND,
    rows: &[TimelineRow],
    top_line: usize,
) -> Option<usize> {
    let mut next_top_line = 0usize;
    for (index, row) in rows.iter().enumerate() {
        let line_count = timeline_source_row_paint_line_count(hwnd, row);
        if top_line < next_top_line + line_count {
            return Some(index);
        }
        next_top_line += line_count;
    }

    (!rows.is_empty()).then_some(rows.len() - 1)
}

pub(super) unsafe fn timeline_top_line_for_source_row_index(
    hwnd: HWND,
    rows: &[TimelineRow],
    source_row_index: usize,
) -> usize {
    rows.iter()
        .take(source_row_index)
        .map(|row| timeline_source_row_paint_line_count(hwnd, row))
        .sum()
}

unsafe fn timeline_text_len(hwnd: HWND) -> LRESULT {
    timeline_state_ref(hwnd)
        .rendered_text
        .encode_utf16()
        .count() as LRESULT
}

unsafe fn timeline_get_text(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if wparam == 0 || lparam == 0 {
        return 0;
    }

    let state = timeline_state_ref(hwnd);
    let capacity = wparam;
    let buffer = lparam as *mut u16;
    let encoded = state.rendered_text.encode_utf16().collect::<Vec<_>>();
    let copied = encoded.len().min(capacity.saturating_sub(1));
    copy_nonoverlapping(encoded.as_ptr(), buffer, copied);
    *buffer.add(copied) = 0;
    copied as LRESULT
}

pub(super) unsafe fn timeline_visible_rows(hwnd: HWND) -> usize {
    let state = timeline_state_ref(hwnd);
    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);
    let client_height = rect.bottom - rect.top;
    let usable_height = (client_height - state.vertical_padding * 2).max(state.line_height);
    (usable_height / state.line_height).max(1) as usize
}

pub(super) fn timeline_max_top_line(line_count: usize, visible_rows: usize) -> usize {
    line_count.saturating_sub(visible_rows)
}

pub(super) fn clamped_timeline_top_line(
    top_line: usize,
    line_count: usize,
    visible_rows: usize,
) -> usize {
    top_line.min(timeline_max_top_line(line_count, visible_rows))
}

fn timeline_is_tail_pinned(
    source_row_count: usize,
    top_line: usize,
    line_count: usize,
    visible_rows: usize,
) -> bool {
    source_row_count == 0 || top_line >= timeline_max_top_line(line_count, visible_rows)
}

fn timeline_top_line_after_content_replaced(
    previous_top_line: usize,
    tail_pinned: bool,
    line_count: usize,
    visible_rows: usize,
) -> usize {
    if tail_pinned {
        timeline_max_top_line(line_count, visible_rows)
    } else {
        clamped_timeline_top_line(previous_top_line, line_count, visible_rows)
    }
}

pub(super) unsafe fn update_timeline_scrollbar(hwnd: HWND) {
    let visible_rows = timeline_visible_rows(hwnd);
    let state = timeline_state_ref(hwnd);
    let expected_max = state.paint_lines.len().saturating_sub(1) as i32;
    let expected_page = visible_rows.max(1).min(state.paint_lines.len().max(1)) as u32;
    let scroll_info = SCROLLINFO {
        cbSize: size_of::<SCROLLINFO>() as u32,
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
        nMin: 0,
        nMax: expected_max,
        nPage: expected_page,
        nPos: state.top_line as i32,
        nTrackPos: 0,
    };
    let set_result = SetScrollInfo(hwnd, SB_VERT, &scroll_info, 1);

    let mut actual = SCROLLINFO {
        cbSize: size_of::<SCROLLINFO>() as u32,
        fMask: SIF_ALL,
        nMin: 0,
        nMax: 0,
        nPage: 0,
        nPos: 0,
        nTrackPos: 0,
    };
    let get_success = GetScrollInfo(hwnd, SB_VERT, &mut actual);
    if get_success == 0
        || actual.nMax != expected_max
        || actual.nPage != expected_page
        || actual.nPos != state.top_line as i32
    {
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect);
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let error = if get_success == 0 { GetLastError() } else { 0 };
        eprintln!(
            "timeline scrollbar mismatch: hwnd={:#x} style=0x{style:x} rect={}x{} lines={} visible_rows={} expected_max={} expected_page={} expected_pos={} set_result={} get_success={} actual_max={} actual_page={} actual_pos={} last_error={}",
            hwnd as usize,
            rect.right - rect.left,
            rect.bottom - rect.top,
            state.paint_lines.len(),
            visible_rows,
            expected_max,
            expected_page,
            state.top_line,
            set_result,
            get_success,
            actual.nMax,
            actual.nPage,
            actual.nPos,
            error
        );
    }
}

pub(super) unsafe fn capture_room_history_backfill_anchor(
    state: &WindowState,
    room_id: &str,
) -> Option<TimelineBackfillAnchor> {
    let ViewId::Room(active_room_id) = &state.active_view else {
        return None;
    };
    if active_room_id != room_id {
        return None;
    }

    let view = state.views.get(&state.active_view)?;
    let timeline = room_content_timeline(view.content);
    let timeline_state = timeline_state_ref(timeline);
    let visible_source_row_index = timeline_source_row_index_for_top_line(
        timeline,
        &timeline_state.source_rows,
        timeline_state.top_line,
    )?;
    let source_row_index = timeline_history_backfill_anchor_row_index(
        &timeline_state.source_rows,
        visible_source_row_index,
    )?;
    Some(TimelineBackfillAnchor {
        room_id: room_id.to_owned(),
        source_row: timeline_state.source_rows.get(source_row_index)?.clone(),
        source_row_index,
        source_row_count: timeline_state.source_rows.len(),
    })
}

fn timeline_history_backfill_anchor_row_index(
    rows: &[TimelineRow],
    visible_source_row_index: usize,
) -> Option<usize> {
    rows.iter()
        .enumerate()
        .skip(visible_source_row_index)
        .find_map(|(index, row)| (!timeline_row_is_fixed_room_context(row)).then_some(index))
}

fn timeline_row_is_fixed_room_context(row: &TimelineRow) -> bool {
    matches!(
        &row.kind,
        TimelineRowKind::Event | TimelineRowKind::Info | TimelineRowKind::Typing
    )
}

pub(super) unsafe fn restore_room_history_backfill_anchor(
    hwnd: HWND,
    anchor: TimelineBackfillAnchor,
) {
    let state = state_ref(hwnd);
    let ViewId::Room(active_room_id) = &state.active_view else {
        return;
    };
    if *active_room_id != anchor.room_id {
        return;
    }

    let Some(view) = state.views.get(&state.active_view) else {
        return;
    };
    let timeline = room_content_timeline(view.content);
    let timeline_state = timeline_state_ref(timeline);
    if timeline_state.source_rows.is_empty() {
        return;
    }

    let source_row_index = timeline_state
        .source_rows
        .iter()
        .position(|row| row == &anchor.source_row)
        .unwrap_or_else(|| {
            anchor
                .source_row_index
                .saturating_add(
                    timeline_state
                        .source_rows
                        .len()
                        .saturating_sub(anchor.source_row_count),
                )
                .min(timeline_state.source_rows.len().saturating_sub(1))
        });
    let top_line = timeline_top_line_for_source_row_index(
        timeline,
        &timeline_state.source_rows,
        source_row_index,
    );
    let visible_rows = timeline_visible_rows(timeline);
    let max_top_line = timeline_max_top_line(timeline_state.paint_lines.len(), visible_rows);
    timeline_state_mut(timeline).top_line = top_line.min(max_top_line);
    update_timeline_scrollbar(timeline);
    InvalidateRect(timeline, null(), 1);
}

/// How the user scrolls a timeline: its scroll bar, the wheel, or the
/// editbox's paging keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScrollGesture {
    Top,
    Bottom,
    /// Lines down; negative is up.
    Lines(isize),
    /// Pages down; negative is up.
    Pages(isize),
    /// The scroll bar's thumb, at a line.
    Thumb(usize),
}

impl ScrollGesture {
    /// Whether the gesture heads for older lines: ending at the first line
    /// it asks for older history.
    fn heads_up(self) -> bool {
        match self {
            Self::Top | Self::Thumb(_) => true,
            Self::Bottom => false,
            Self::Lines(lines) | Self::Pages(lines) => lines < 0,
        }
    }
}

/// The top line after `gesture` from `current`.
fn scrolled_top_line(
    current: usize,
    gesture: ScrollGesture,
    visible_rows: usize,
    max_top_line: usize,
) -> usize {
    match gesture {
        ScrollGesture::Top => 0,
        ScrollGesture::Bottom => max_top_line,
        ScrollGesture::Lines(lines) => current.saturating_add_signed(lines),
        ScrollGesture::Pages(pages) => {
            current.saturating_add_signed(pages.saturating_mul(visible_rows as isize))
        }
        ScrollGesture::Thumb(line) => line,
    }
    .min(max_top_line)
}

/// The system's wheel setting: `WHEEL_PAGESCROLL` scrolls a page per notch.
const WHEEL_PAGESCROLL: u32 = u32::MAX;

/// The gesture for a wheel `delta` (positive is away from the user, up) on
/// top of the `remainder` of earlier partial notches, and the new remainder.
fn wheel_gesture(remainder: i32, delta: i16, lines_per_notch: u32) -> (Option<ScrollGesture>, i32) {
    let total = remainder + i32::from(delta);
    let notches = total / WHEEL_DELTA as i32;
    let remainder = total % WHEEL_DELTA as i32;
    let up_notches = -(notches as isize);
    let gesture = match (notches, lines_per_notch) {
        (0, _) | (_, 0) => None,
        (_, WHEEL_PAGESCROLL) => Some(ScrollGesture::Pages(up_notches)),
        (_, lines) => Some(ScrollGesture::Lines(up_notches * lines as isize)),
    };
    (gesture, remainder)
}

unsafe fn scroll_timeline_with_wheel(hwnd: HWND, wparam: WPARAM) {
    let delta = (wparam >> 16) as u16 as i16;
    let mut lines_per_notch = 0u32;
    let read = SystemParametersInfoW(
        SPI_GETWHEELSCROLLLINES,
        0,
        (&mut lines_per_notch as *mut u32).cast(),
        0,
    );
    assert_ne!(read, 0, "SPI_GETWHEELSCROLLLINES always answers");
    let state = timeline_state_mut(hwnd);
    let (gesture, remainder) = wheel_gesture(state.wheel_remainder, delta, lines_per_notch);
    state.wheel_remainder = remainder;
    if let Some(gesture) = gesture {
        scroll_timeline_by(hwnd, gesture);
    }
}

unsafe fn scroll_timeline(hwnd: HWND, wparam: WPARAM) -> LRESULT {
    let gesture = match (wparam & 0xffff) as i32 {
        SB_TOP => ScrollGesture::Top,
        SB_BOTTOM => ScrollGesture::Bottom,
        SB_LINEUP => ScrollGesture::Lines(-1),
        SB_LINEDOWN => ScrollGesture::Lines(1),
        SB_PAGEUP => ScrollGesture::Pages(-1),
        SB_PAGEDOWN => ScrollGesture::Pages(1),
        SB_THUMBPOSITION | SB_THUMBTRACK => {
            let mut scroll_info = SCROLLINFO {
                cbSize: size_of::<SCROLLINFO>() as u32,
                fMask: SIF_TRACKPOS,
                nMin: 0,
                nMax: 0,
                nPage: 0,
                nPos: 0,
                nTrackPos: 0,
            };
            GetScrollInfo(hwnd, SB_VERT, &mut scroll_info);
            ScrollGesture::Thumb(scroll_info.nTrackPos.max(0) as usize)
        }
        // SB_ENDSCROLL ends a gesture the scroll bar already sent.
        _ => return 0,
    };
    scroll_timeline_by(hwnd, gesture);
    0
}

/// Scrolls the timeline as the user asked. A gesture heading up that ends
/// at the first line asks for older history: one request per gesture, which
/// `submit_backfill_room_history_for_room` refuses while one is loading or
/// history is exhausted.
pub(super) unsafe fn scroll_timeline_by(hwnd: HWND, gesture: ScrollGesture) {
    // The user scrolling a window drops its pending Go to Original; the
    // jump's own scroll sets the top line directly.
    cancel_original_jump_for_timeline(hwnd);
    let visible_rows = timeline_visible_rows(hwnd);
    let (current, max_top_line) = {
        let state = timeline_state_ref(hwnd);
        (
            state.top_line,
            timeline_max_top_line(state.paint_lines.len(), visible_rows),
        )
    };
    let next = scrolled_top_line(current, gesture, visible_rows, max_top_line);

    if next != current {
        timeline_state_mut(hwnd).top_line = next;
        update_timeline_scrollbar(hwnd);
        InvalidateRect(hwnd, null(), 1);
    }

    if gesture.heads_up()
        && next == 0
        && let Err(message) = submit_backfill_room_history(hwnd)
    {
        eprintln!("{message}");
    }
}

fn render_view_text(lines: &[String]) -> String {
    lines.join("\r\n")
}

pub(super) fn split_rendered_lines(text: &str) -> Vec<String> {
    rendered_line_ranges(text)
        .into_iter()
        .map(|line| text[line].to_owned())
        .collect()
}

fn normalize_timeline_text(text: &str) -> (String, Vec<String>) {
    let lines = split_rendered_lines(text);
    let normalized = render_view_text(&lines);
    (normalized, lines)
}

/// Status bar text while Go to Original pages older history.
const FINDING_ORIGINAL_STATUS: &str = "finding the original message...";

/// The message menu's Go to Original Message, and a double-click on a
/// quote line (which selects its reply): scrolls to the message the
/// selected reply quotes, loading older history until it is found.
pub(super) unsafe fn go_to_original_of_selected_message(hwnd: HWND) -> Result<(), String> {
    let state = state_ref(hwnd);
    let target_event_id = selected_message_quote_target(state, selected_timeline_event_id(state))
        .ok_or_else(|| "the selected message is not a reply".to_owned())?
        .to_owned();
    let view_id = state.active_view.clone();
    // A new jump replaces a pending one.
    state_mut(hwnd).pending_original_jump = Some(replies::OriginalJump {
        view_id,
        target_event_id,
    });
    continue_original_jump(hwnd, None)
}

/// Takes the pending Go to Original's next step, when it starts and after
/// every history result. `failed_backfill_room_id`: the room whose request
/// for older messages just failed.
pub(super) unsafe fn continue_original_jump(
    hwnd: HWND,
    failed_backfill_room_id: Option<&str>,
) -> Result<(), String> {
    let Some(jump) = state_ref(hwnd).pending_original_jump.clone() else {
        return Ok(());
    };
    let room_id = jump
        .view_id
        .room_id()
        .expect("only room and thread windows quote")
        .to_owned();
    let step = {
        let state = state_ref(hwnd);
        let view = state
            .views
            .get(&jump.view_id)
            .expect("closing a window drops its jump");
        let in_view_rows = timeline_source_row_index_of_event(
            view_timeline(&jump.view_id, view.content),
            &jump.target_event_id,
        )
        .is_some();
        let room_view = state
            .model
            .room_views
            .get(&room_id)
            .expect("a reply's room has its timeline");
        let history = match &room_view.history.status {
            RoomHistoryBackfillStatus::Idle => replies::JumpHistory::Idle,
            RoomHistoryBackfillStatus::Loading { .. } => replies::JumpHistory::Loading,
            RoomHistoryBackfillStatus::Exhausted => replies::JumpHistory::Exhausted,
            RoomHistoryBackfillStatus::Failed { message, .. } => replies::JumpHistory::Failed {
                message: message.clone(),
                just_now: failed_backfill_room_id == Some(room_id.as_str()),
            },
        };
        let another_backfill_loading = loading_room_history_room_id(&state.model.room_views)
            .is_some_and(|loading_room_id| loading_room_id != room_id);
        replies::original_jump_step(
            &jump.view_id,
            in_view_rows,
            room_view
                .find_activity_index_by_event_id(&jump.target_event_id)
                .is_some(),
            history,
            another_backfill_loading,
        )
    };
    let status = match step {
        replies::OriginalJumpStep::Show(view_id) => {
            state_mut(hwnd).pending_original_jump = None;
            show_original_message(hwnd, &view_id, &jump.target_event_id)?;
            "original message found".to_owned()
        }
        replies::OriginalJumpStep::LoadOlder => {
            let submitted = submit_backfill_room_history_for_room(state_mut(hwnd), &room_id)?;
            assert!(
                submitted,
                "a joined room with history left and none loading takes a request"
            );
            FINDING_ORIGINAL_STATUS.to_owned()
        }
        replies::OriginalJumpStep::Wait => FINDING_ORIGINAL_STATUS.to_owned(),
        replies::OriginalJumpStep::NotFound => {
            state_mut(hwnd).pending_original_jump = None;
            "* the original message is not in this room's history".to_owned()
        }
        replies::OriginalJumpStep::Failed(message) => {
            state_mut(hwnd).pending_original_jump = None;
            format!("* could not load older messages: {message}")
        }
    };
    set_local_command_status(hwnd, status)
}

/// Scrolls `view_id`'s window, activated first, so the message's first line
/// is at the top (as low as the end allows), and selects it.
unsafe fn show_original_message(
    hwnd: HWND,
    view_id: &ViewId,
    event_id: &str,
) -> Result<(), String> {
    if state_ref(hwnd).active_view != *view_id {
        activate_view(hwnd, view_id, None)?;
    }
    let timeline = view_timeline(view_id, state_ref(hwnd).views[view_id].content);
    let source_row_index = timeline_source_row_index_of_event(timeline, event_id)
        .expect("the jump shows a message its window holds");
    scroll_timeline_to_source_row_index(timeline, source_row_index);
    state_mut(hwnd).selected_timeline_event = Some(SelectedTimelineEvent {
        view_id: view_id.clone(),
        event_id: event_id.to_owned(),
    });
    InvalidateRect(timeline, null(), 1);
    Ok(())
}

pub(super) fn cancel_original_jump_for_view(state: &mut WindowState, view_id: &ViewId) {
    if state
        .pending_original_jump
        .as_ref()
        .is_some_and(|jump| jump.view_id == *view_id)
    {
        state.pending_original_jump = None;
    }
}

unsafe fn cancel_original_jump_for_timeline(timeline: HWND) {
    let frame = frame_window_for(timeline);
    let state = state_mut(frame);
    let Some(jump) = &state.pending_original_jump else {
        return;
    };
    if view_timeline(&jump.view_id, state.views[&jump.view_id].content) == timeline {
        state.pending_original_jump = None;
    }
}

pub(super) fn loading_room_history_room_id(
    room_views: &BTreeMap<String, RoomViewModel>,
) -> Option<String> {
    let mut loading_room_ids = room_views
        .iter()
        .filter(|(_, room_view)| {
            matches!(
                room_view.history.status,
                RoomHistoryBackfillStatus::Loading { .. }
            )
        })
        .map(|(room_id, _)| room_id.clone());
    let loading_room_id = loading_room_ids.next();
    #[cfg(test)]
    {
        assert!(
            loading_room_ids.next().is_none(),
            "room history backfill must keep a single in-flight loading room"
        );
    }
    loading_room_id
}

pub(super) fn submit_backfill_room_history_for_room(
    state: &mut WindowState,
    room_id: &str,
) -> Result<bool, String> {
    if sendable_room_by_id(&state.model.rooms, room_id).is_none() {
        return Ok(false);
    }
    if loading_room_history_room_id(&state.model.room_views).is_some() {
        return Ok(false);
    }

    let Some(sender) = state.command_sender.as_ref().cloned() else {
        return Ok(false);
    };

    let command = {
        let room_view = state
            .model
            .room_views
            .entry(room_id.to_owned())
            .or_default();
        if matches!(
            room_view.history.status,
            RoomHistoryBackfillStatus::Exhausted
        ) {
            return Ok(false);
        }

        let from = room_view.history.next_token.clone();
        room_view.history.status = RoomHistoryBackfillStatus::Loading { from: from.clone() };
        Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
            room_id: room_id.to_owned(),
            from,
            limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
        })
    };

    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue backfill room history command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(true)
}

/// Asks for older history of the room whose window `hwnd` is the timeline
/// of, if it is the active window: the backfill anchor keeps the active
/// window's reading position.
unsafe fn submit_backfill_room_history(hwnd: HWND) -> Result<(), String> {
    let state = state_mut(frame_window_for(hwnd));
    let room_id = match &state.active_view {
        ViewId::Room(room_id) | ViewId::Thread { room_id, .. } => room_id.clone(),
        ViewId::Status | ViewId::RoomList => return Ok(()),
    };
    let active_view = &state.views[&state.active_view];
    if view_timeline(&state.active_view, active_view.content) != hwnd {
        return Ok(());
    }

    submit_backfill_room_history_for_room(state, &room_id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SWP_NOMOVE, SWP_NOZORDER, SetWindowPos};

    fn history_backfill_event_from_event(event: Event) -> RoomHistoryBackfillEvent {
        match event {
            Event::RoomActivity(activity) => RoomHistoryBackfillEvent::Activity(activity),
            Event::RoomActivityUpdate(update) => RoomHistoryBackfillEvent::ActivityUpdate(update),
            other => panic!("expected room history backfill event payload, got {other:?}"),
        }
    }

    #[test]
    fn normalize_timeline_text_converts_embedded_newlines_to_crlf() {
        let text = "[22:13] <@bob:example> multiline row 00 line a\nmultiline row 00 line b";
        let (normalized, lines) = normalize_timeline_text(text);

        assert_eq!(
            normalized,
            "[22:13] <@bob:example> multiline row 00 line a\r\nmultiline row 00 line b"
        );
        assert_eq!(
            lines,
            vec![
                "[22:13] <@bob:example> multiline row 00 line a".to_owned(),
                "multiline row 00 line b".to_owned(),
            ]
        );
    }

    #[test]
    fn normalize_timeline_text_preserves_trailing_blank_line() {
        let text = "trailing blank line a\n";
        let (normalized, lines) = normalize_timeline_text(text);

        assert_eq!(normalized, "trailing blank line a\r\n");
        assert_eq!(
            lines,
            vec!["trailing blank line a".to_owned(), String::new()]
        );
    }

    #[test]
    fn clamped_timeline_top_line_respects_visible_row_growth() {
        assert_eq!(clamped_timeline_top_line(8, 10, 3), 7);
        assert_eq!(clamped_timeline_top_line(8, 10, 5), 5);
        assert_eq!(clamped_timeline_top_line(2, 10, 20), 0);
    }

    #[test]
    fn timeline_content_replacement_follows_tail_or_retains_scrolled_position() {
        assert!(timeline_is_tail_pinned(0, 0, 0, 3));
        assert_eq!(timeline_top_line_after_content_replaced(0, true, 10, 3), 7);

        assert!(timeline_is_tail_pinned(8, 5, 8, 3));
        assert_eq!(timeline_top_line_after_content_replaced(5, true, 12, 3), 9);

        assert!(!timeline_is_tail_pinned(8, 2, 8, 3));
        assert_eq!(timeline_top_line_after_content_replaced(2, false, 12, 3), 2);

        assert_eq!(timeline_top_line_after_content_replaced(8, false, 10, 5), 5);
    }

    #[test]
    fn scroll_gestures_move_the_top_line_within_the_buffer() {
        // 50 lines, 10 visible: the top line runs 0..=40.
        let scrolled = |current, gesture| scrolled_top_line(current, gesture, 10, 40);
        assert_eq!(scrolled(20, ScrollGesture::Top), 0);
        assert_eq!(scrolled(20, ScrollGesture::Bottom), 40);
        assert_eq!(scrolled(20, ScrollGesture::Lines(-3)), 17);
        assert_eq!(scrolled(2, ScrollGesture::Lines(-3)), 0);
        assert_eq!(scrolled(39, ScrollGesture::Lines(3)), 40);
        assert_eq!(scrolled(20, ScrollGesture::Pages(-1)), 10);
        assert_eq!(scrolled(5, ScrollGesture::Pages(-1)), 0);
        assert_eq!(scrolled(35, ScrollGesture::Pages(1)), 40);
        assert_eq!(scrolled(20, ScrollGesture::Thumb(55)), 40);
        assert_eq!(scrolled(20, ScrollGesture::Thumb(7)), 7);
    }

    #[test]
    fn only_gestures_heading_up_ask_for_older_history() {
        assert!(ScrollGesture::Top.heads_up());
        assert!(ScrollGesture::Thumb(0).heads_up());
        assert!(ScrollGesture::Lines(-1).heads_up());
        assert!(ScrollGesture::Pages(-1).heads_up());
        assert!(!ScrollGesture::Bottom.heads_up());
        assert!(!ScrollGesture::Lines(1).heads_up());
        assert!(!ScrollGesture::Pages(1).heads_up());
    }

    #[test]
    fn the_wheel_scrolls_the_system_lines_per_whole_notch() {
        assert_eq!(
            wheel_gesture(0, 120, 3),
            (Some(ScrollGesture::Lines(-3)), 0)
        );
        assert_eq!(
            wheel_gesture(0, -240, 3),
            (Some(ScrollGesture::Lines(6)), 0)
        );
        // A finer wheel's steps add up to a notch.
        assert_eq!(wheel_gesture(0, 60, 3), (None, 60));
        assert_eq!(
            wheel_gesture(60, 60, 3),
            (Some(ScrollGesture::Lines(-3)), 0)
        );
        assert_eq!(
            wheel_gesture(60, 100, 3),
            (Some(ScrollGesture::Lines(-3)), 40)
        );
        assert_eq!(wheel_gesture(40, -60, 3), (None, -20));
        assert_eq!(
            wheel_gesture(0, 120, WHEEL_PAGESCROLL),
            (Some(ScrollGesture::Pages(-1)), 0)
        );
        // A wheel set to scroll nothing scrolls nothing.
        assert_eq!(wheel_gesture(0, 120, 0), (None, 0));
    }

    #[test]
    fn history_backfill_anchor_skips_only_fixed_room_context() {
        let rows = vec![
            TimelineRow::log(
                TimelineRowKind::Event,
                "* Now talking in Beta (!beta:example)".to_owned(),
            ),
            TimelineRow::log(TimelineRowKind::Event, "* Topic is 'incident'".to_owned()),
            TimelineRow::log(
                TimelineRowKind::ReactionSummary,
                "    reactions: 👍 Alice".to_owned(),
            ),
            TimelineRow::log(
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                "[22:13] <@bob:example> current row".to_owned(),
            )
            .with_event_id(Some("$current")),
        ];

        assert_eq!(
            timeline_history_backfill_anchor_row_index(&rows, 0),
            Some(2)
        );
        assert_eq!(
            timeline_history_backfill_anchor_row_index(&rows, 2),
            Some(2)
        );
    }

    #[test]
    fn room_history_backfill_submit_records_initial_loading_command() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: None,
                limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
            })]
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|room_view| &room_view.history),
            Some(&RoomHistoryBackfillState {
                next_token: None,
                status: RoomHistoryBackfillStatus::Loading { from: None },
            })
        );

        stop_test_core(handle);
    }

    #[test]
    fn room_history_backfill_submit_ignores_repeat_submit_while_loading() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(false)
        );
        assert_eq!(state.recorded_commands.len(), 1);
        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|room_view| &room_view.history),
            Some(&RoomHistoryBackfillState {
                next_token: None,
                status: RoomHistoryBackfillStatus::Loading { from: None },
            })
        );

        stop_test_core(handle);
    }

    #[test]
    fn room_history_backfill_success_sets_next_token_and_reuses_it_on_retry() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        apply_core_event(
            &mut state,
            room_history_backfill(
                "!room:example",
                "token-1",
                Some("token-2"),
                vec![history_backfill_event_from_event(timeline(
                    "!room:example",
                    "older row",
                ))],
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|room_view| &room_view.history),
            Some(&RoomHistoryBackfillState {
                next_token: Some("token-2".to_owned()),
                status: RoomHistoryBackfillStatus::Idle,
            })
        );

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        assert_eq!(
            state.recorded_commands.last(),
            Some(&Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: Some("token-2".to_owned()),
                limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
            }))
        );

        stop_test_core(handle);
    }

    #[test]
    fn room_history_backfill_success_with_no_end_exhausts_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        apply_core_event(
            &mut state,
            room_history_backfill(
                "!room:example",
                "token-1",
                None,
                vec![history_backfill_event_from_event(timeline(
                    "!room:example",
                    "older row",
                ))],
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|room_view| &room_view.history),
            Some(&RoomHistoryBackfillState {
                next_token: None,
                status: RoomHistoryBackfillStatus::Exhausted,
            })
        );
        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(false)
        );
        assert_eq!(state.recorded_commands.len(), 1);

        stop_test_core(handle);
    }

    #[test]
    fn room_history_backfill_error_preserves_retry_token_and_allows_retry() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .model
            .room_views
            .entry("!room:example".to_owned())
            .or_default()
            .history
            .next_token = Some("token-2".to_owned());

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::BackfillRoomHistory,
                room_id: Some("!room:example".to_owned()),
                message: "temporary backfill failure".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|room_view| &room_view.history),
            Some(&RoomHistoryBackfillState {
                next_token: Some("token-2".to_owned()),
                status: RoomHistoryBackfillStatus::Failed {
                    from: Some("token-2".to_owned()),
                    message: "temporary backfill failure".to_owned(),
                },
            })
        );

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        assert_eq!(
            state.recorded_commands.last(),
            Some(&Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: Some("token-2".to_owned()),
                limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
            }))
        );

        stop_test_core(handle);
    }

    /// A real frame showing Ops: Alice's question `$q`, filler lines, and
    /// Bob's reply `$a` to `target` last, selected, with a sender whose
    /// commands wait.
    unsafe fn with_original_jump_frame(target: &str, test: impl FnOnce(HWND)) {
        with_test_frame(600, 400, |hwnd| {
            let (sender, commands) = inert_command_sender();
            std::mem::forget(commands);
            state_mut(hwnd).command_sender = Some(sender);
            state_mut(hwnd).model.rooms = vec![room("!ops:example", "Ops", false)];
            sync_views_and_tree(hwnd).unwrap();
            let mut events = vec![timeline_with_transaction_id(
                "!ops:example",
                "$q",
                None,
                "are we deploying today?",
                1_700_000_000_000,
            )];
            for index in 0..40 {
                events.push(timeline_with_transaction_id(
                    "!ops:example",
                    &format!("$filler{index}"),
                    None,
                    &format!("filler {index}"),
                    1_700_000_001_000 + index * 1_000,
                ));
            }
            events.push(timeline_with_relation_and_presentation(
                "!ops:example",
                "$a",
                None,
                Some(reply_relation(target)),
                RoomMessagePresentationKind::Text,
                "yes, after lunch",
                1_700_000_100_000,
            ));
            apply_events_and_refresh(hwnd, events).unwrap();
            activate_view(hwnd, &room_view_id("!ops:example"), None).unwrap();
            state_mut(hwnd).selected_timeline_event = Some(SelectedTimelineEvent {
                view_id: room_view_id("!ops:example"),
                event_id: "$a".to_owned(),
            });
            test(hwnd);
        });
    }

    unsafe fn ops_timeline(hwnd: HWND, view_id: &ViewId) -> HWND {
        view_timeline(view_id, state_ref(hwnd).views[view_id].content)
    }

    /// The event whose line is the timeline's top line.
    unsafe fn top_line_event_id(timeline: HWND) -> Option<String> {
        let state = timeline_state_ref(timeline);
        state.paint_lines[state.top_line]
            .source_row_index
            .and_then(|index| state.source_rows[index].event_id.clone())
    }

    fn backfill_requests(state: &WindowState) -> usize {
        state
            .recorded_commands
            .iter()
            .filter(|command| matches!(command, Command::BackfillRoomHistory(_)))
            .count()
    }

    #[test]
    fn go_to_original_scrolls_to_a_loaded_target_and_selects_it() {
        unsafe {
            with_original_jump_frame("$q", |hwnd| {
                let timeline = ops_timeline(hwnd, &room_view_id("!ops:example"));
                assert_ne!(
                    top_line_event_id(timeline).as_deref(),
                    Some("$q"),
                    "the window starts at its end"
                );
                go_to_original_of_selected_message(hwnd).unwrap();
                assert_eq!(top_line_event_id(timeline).as_deref(), Some("$q"));
                assert_eq!(selected_timeline_event_id(state_ref(hwnd)), Some("$q"));
                assert_eq!(state_ref(hwnd).pending_original_jump, None);
                assert_eq!(state_ref(hwnd).model.status, "original message found");
                assert_eq!(backfill_requests(state_ref(hwnd)), 0);
            });
        }
    }

    #[test]
    fn go_to_original_pages_history_until_the_target_arrives() {
        unsafe {
            with_original_jump_frame("$old", |hwnd| {
                go_to_original_of_selected_message(hwnd).unwrap();
                assert_eq!(
                    state_ref(hwnd).pending_original_jump,
                    Some(replies::OriginalJump {
                        view_id: room_view_id("!ops:example"),
                        target_event_id: "$old".to_owned(),
                    })
                );
                assert_eq!(backfill_requests(state_ref(hwnd)), 1);
                assert_eq!(state_ref(hwnd).model.status, FINDING_ORIGINAL_STATUS);

                // A page without it asks for the next.
                apply_events_and_refresh(
                    hwnd,
                    vec![room_history_backfill(
                        "!ops:example",
                        "t0",
                        Some("t1"),
                        vec![],
                    )],
                )
                .unwrap();
                assert!(state_ref(hwnd).pending_original_jump.is_some());
                assert_eq!(backfill_requests(state_ref(hwnd)), 2);

                apply_events_and_refresh(
                    hwnd,
                    vec![room_history_backfill(
                        "!ops:example",
                        "t1",
                        Some("t2"),
                        vec![history_backfill_event_from_event(
                            timeline_with_transaction_id(
                                "!ops:example",
                                "$old",
                                None,
                                "the old question",
                                1_699_999_000_000,
                            ),
                        )],
                    )],
                )
                .unwrap();
                let timeline = ops_timeline(hwnd, &room_view_id("!ops:example"));
                // The jump's scroll wins over the backfill anchor.
                assert_eq!(top_line_event_id(timeline).as_deref(), Some("$old"));
                assert_eq!(selected_timeline_event_id(state_ref(hwnd)), Some("$old"));
                assert_eq!(state_ref(hwnd).pending_original_jump, None);
                assert_eq!(state_ref(hwnd).model.status, "original message found");
                assert_eq!(backfill_requests(state_ref(hwnd)), 2);
            });
        }
    }

    #[test]
    fn go_to_original_stops_at_the_start_of_history_or_a_failure() {
        unsafe {
            with_original_jump_frame("$old", |hwnd| {
                go_to_original_of_selected_message(hwnd).unwrap();
                apply_events_and_refresh(
                    hwnd,
                    vec![room_history_backfill("!ops:example", "t0", None, vec![])],
                )
                .unwrap();
                assert_eq!(state_ref(hwnd).pending_original_jump, None);
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "* the original message is not in this room's history"
                );
                assert_eq!(selected_timeline_event_id(state_ref(hwnd)), Some("$a"));
            });
            with_original_jump_frame("$old", |hwnd| {
                go_to_original_of_selected_message(hwnd).unwrap();
                apply_events_and_refresh(
                    hwnd,
                    vec![Event::Error(CoreErrorEvent {
                        operation: Operation::BackfillRoomHistory,
                        room_id: Some("!ops:example".to_owned()),
                        message: "offline".to_owned(),
                    })],
                )
                .unwrap();
                assert_eq!(state_ref(hwnd).pending_original_jump, None);
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "* could not load older messages: offline"
                );
                // A failure left from before is retried by the next jump.
                go_to_original_of_selected_message(hwnd).unwrap();
                assert!(state_ref(hwnd).pending_original_jump.is_some());
                assert_eq!(backfill_requests(state_ref(hwnd)), 2);
            });
        }
    }

    #[test]
    fn scrolling_the_window_drops_its_pending_go_to_original() {
        unsafe {
            with_original_jump_frame("$old", |hwnd| {
                go_to_original_of_selected_message(hwnd).unwrap();
                let timeline = ops_timeline(hwnd, &room_view_id("!ops:example"));
                SendMessageW(timeline, WM_VSCROLL, SB_LINEUP as WPARAM, 0);
                assert_eq!(state_ref(hwnd).pending_original_jump, None);
                // The page it asked for arrives and changes nothing.
                let top_line = timeline_state_ref(timeline).top_line;
                apply_events_and_refresh(
                    hwnd,
                    vec![room_history_backfill("!ops:example", "t0", None, vec![])],
                )
                .unwrap();
                assert_ne!(top_line_event_id(timeline).as_deref(), Some("$old"));
                assert!(timeline_state_ref(timeline).top_line >= top_line);
            });
        }
    }

    #[test]
    fn the_wheel_to_the_top_asks_for_older_history_once_per_gesture() {
        unsafe {
            with_original_jump_frame("$q", |hwnd| {
                let timeline = ops_timeline(hwnd, &room_view_id("!ops:example"));
                assert_ne!(timeline_state_ref(timeline).top_line, 0);
                let wheel_up = (i16::MAX as u16 as WPARAM) << 16;
                SendMessageW(timeline, WM_MOUSEWHEEL, wheel_up, 0);
                assert_eq!(timeline_state_ref(timeline).top_line, 0);
                assert_eq!(backfill_requests(state_ref(hwnd)), 1);
                // Loading: a further notch asks for nothing more.
                SendMessageW(timeline, WM_MOUSEWHEEL, 120 << 16, 0);
                assert_eq!(backfill_requests(state_ref(hwnd)), 1);
                // Down and back up to the top by lines is a new gesture,
                // refused while the first request loads.
                SendMessageW(timeline, WM_VSCROLL, SB_LINEDOWN as WPARAM, 0);
                SendMessageW(timeline, WM_VSCROLL, SB_LINEUP as WPARAM, 0);
                assert_eq!(timeline_state_ref(timeline).top_line, 0);
                assert_eq!(backfill_requests(state_ref(hwnd)), 1);
            });
        }
    }

    unsafe fn resize(hwnd: HWND, width: i32, height: i32) {
        SetWindowPos(
            hwnd,
            null_mut(),
            0,
            0,
            width,
            height,
            SWP_NOMOVE | SWP_NOZORDER,
        );
    }

    unsafe fn at_its_end(timeline: HWND) -> bool {
        let state = timeline_state_ref(timeline);
        state.top_line
            == timeline_max_top_line(state.paint_lines.len(), timeline_visible_rows(timeline))
    }

    #[test]
    fn a_window_showing_its_newest_line_still_does_after_a_resize() {
        unsafe {
            with_original_jump_frame("$q", |hwnd| {
                let timeline = ops_timeline(hwnd, &room_view_id("!ops:example"));
                assert!(at_its_end(timeline));
                // Narrower rewraps into more lines, shorter shows fewer.
                resize(timeline, 120, 200);
                assert!(at_its_end(timeline));
                resize(timeline, 120, 90);
                assert!(at_its_end(timeline));
                resize(timeline, 400, 300);
                assert!(at_its_end(timeline));

                // A window scrolled back keeps its top line.
                SendMessageW(timeline, WM_VSCROLL, SB_LINEUP as WPARAM, 0);
                let top_line = timeline_state_ref(timeline).top_line;
                resize(timeline, 400, 200);
                assert_eq!(timeline_state_ref(timeline).top_line, top_line);
                assert!(!at_its_end(timeline));
            });
        }
    }

    #[test]
    fn scrolling_the_status_window_to_the_top_asks_for_nothing() {
        unsafe {
            with_original_jump_frame("$q", |hwnd| {
                activate_view(hwnd, &ViewId::Status, None).unwrap();
                let status = ops_timeline(hwnd, &ViewId::Status);
                SendMessageW(status, WM_VSCROLL, SB_TOP as WPARAM, 0);
                SendMessageW(status, WM_MOUSEWHEEL, 120 << 16, 0);
                assert_eq!(backfill_requests(state_ref(hwnd)), 0);
            });
        }
    }

    #[test]
    fn go_to_original_from_a_thread_shows_a_target_outside_it_in_the_room() {
        unsafe {
            with_original_jump_frame("$q", |hwnd| {
                let thread = thread_view_id("!ops:example", "$filler0");
                apply_events_and_refresh(
                    hwnd,
                    vec![timeline_with_relation_and_presentation(
                        "!ops:example",
                        "$t1",
                        None,
                        Some(RoomActivityRelation::Thread {
                            thread_root_event_id: "$filler0".to_owned(),
                            in_reply_to_event_id: Some("$q".to_owned()),
                            is_falling_back: false,
                        }),
                        RoomMessagePresentationKind::Text,
                        "in the thread",
                        1_700_000_200_000,
                    )],
                )
                .unwrap();
                activate_view(hwnd, &thread, None).unwrap();
                state_mut(hwnd).selected_timeline_event = Some(SelectedTimelineEvent {
                    view_id: thread.clone(),
                    event_id: "$t1".to_owned(),
                });
                assert!(active_message_action_state(state_ref(hwnd)).go_to_original_enabled);
                go_to_original_of_selected_message(hwnd).unwrap();
                assert_eq!(state_ref(hwnd).active_view, room_view_id("!ops:example"));
                assert_eq!(selected_timeline_event_id(state_ref(hwnd)), Some("$q"));
                let timeline = ops_timeline(hwnd, &room_view_id("!ops:example"));
                assert_eq!(top_line_event_id(timeline).as_deref(), Some("$q"));
            });
        }
    }
}
