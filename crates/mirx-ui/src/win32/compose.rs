//! The editbox: history, Tab completion, format keys, reply and edit
//! targets, and submitting what was typed.

use super::*;
use windows_sys::Win32::UI::Controls::EM_UNDO;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowTextLengthW, WM_CLEAR, WM_CUT, WM_KILLFOCUS, WM_LBUTTONUP, WM_MOUSEWHEEL, WM_UNDO,
};

use super::color_index::{
    color_code_at_caret, color_index_shown, edit_selection, hide_color_index, update_color_index,
};
use super::colors::PaletteIndex;

const VK_DELETE_KEY: WPARAM = 46;
const VK_TAB_KEY: WPARAM = 9;
const VK_PRIOR_KEY: WPARAM = 0x21;
const VK_NEXT_KEY: WPARAM = 0x22;
const VK_END_KEY: WPARAM = 0x23;
const VK_HOME_KEY: WPARAM = 0x24;

/// The timeline gesture of a paging key in the editbox, as in mIRC: Page
/// Up/Down scroll a page, a line with Ctrl, and Ctrl+Home/End go to the
/// top or bottom while the editbox is empty (otherwise they move the
/// caret, which is the edit control's).
fn compose_scroll_gesture(key: WPARAM, ctrl: bool, edit_empty: bool) -> Option<ScrollGesture> {
    match (key, ctrl) {
        (VK_PRIOR_KEY, false) => Some(ScrollGesture::Pages(-1)),
        (VK_NEXT_KEY, false) => Some(ScrollGesture::Pages(1)),
        (VK_PRIOR_KEY, true) => Some(ScrollGesture::Lines(-1)),
        (VK_NEXT_KEY, true) => Some(ScrollGesture::Lines(1)),
        (VK_HOME_KEY, true) if edit_empty => Some(ScrollGesture::Top),
        (VK_END_KEY, true) if edit_empty => Some(ScrollGesture::Bottom),
        _ => None,
    }
}

/// The timeline of the window `edit` is the editbox of.
unsafe fn editbox_timeline(frame: HWND, edit: HWND) -> HWND {
    let state = state_ref(frame);
    let view_id = &state.hwnd_views[&(GetParent(edit) as isize)];
    view_timeline(view_id, state.views[view_id].content)
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub(super) struct ComposeHistory {
    pub(super) entries: Vec<String>,
    navigation: Option<ComposeHistoryNavigation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComposeHistoryNavigation {
    index: usize,
    draft: String,
}

impl ComposeHistory {
    fn record_submission(&mut self, body: String) {
        self.entries.push(body);
        self.navigation = None;
    }

    pub(super) fn reset_navigation(&mut self) {
        self.navigation = None;
    }

    fn previous(&mut self, current_text: String) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }

        match self.navigation.as_mut() {
            Some(navigation) => {
                if navigation.index > 0 {
                    navigation.index -= 1;
                }
            }
            None => {
                self.navigation = Some(ComposeHistoryNavigation {
                    index: self.entries.len() - 1,
                    draft: current_text,
                });
            }
        }

        let index = self
            .navigation
            .as_ref()
            .expect("compose history navigation must exist after previous")
            .index;
        Some(&self.entries[index])
    }

    fn next(&mut self) -> Option<String> {
        let navigation = self.navigation.as_mut()?;
        if navigation.index + 1 < self.entries.len() {
            navigation.index += 1;
            return Some(self.entries[navigation.index].clone());
        }

        let draft = navigation.draft.clone();
        self.navigation = None;
        Some(draft)
    }
}

enum ComposeHistoryDirection {
    Previous,
    Next,
}

/// A Tab completion and the view whose members it completed; switching
/// views ends it, as the compose box is shared by every view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ComposeNickCompletion {
    view_id: ViewId,
    completion: NickCompletion,
}

/// Replaces the edit control's selection, or inserts at its caret, and
/// leaves the caret after `text`.
pub(super) unsafe fn replace_edit_selection(edit: HWND, text: &str) {
    let text = wide_null(text);
    // FALSE: no separate undo step for the replacement.
    SendMessageW(edit, EM_REPLACESEL, 0, text.as_ptr() as LPARAM);
}

/// The compose text as UTF-16 and the caret position in it.
unsafe fn compose_text_and_caret(edit: HWND) -> (Vec<u16>, usize) {
    let text_len = SendMessageW(edit, WM_GETTEXTLENGTH, 0, 0) as usize;
    let mut text = vec![0u16; text_len + 1];
    let copied = SendMessageW(edit, WM_GETTEXT, text.len(), text.as_mut_ptr() as LPARAM) as usize;
    text.truncate(copied);
    let mut start = 0u32;
    let mut end = 0u32;
    SendMessageW(
        edit,
        EM_GETSEL,
        (&mut start as *mut u32) as WPARAM,
        (&mut end as *mut u32) as LPARAM,
    );
    // The caret is at the selection's end; completing replaces the word
    // before it, which drops any selection.
    (text, end as usize)
}

/// Tab (or Shift+Tab when `reverse`) in the compose box: continues the last
/// completion while the box still holds what it inserted, else completes
/// the word before the caret against the active view's members.
unsafe fn complete_compose_nick(hwnd: HWND, reverse: bool) -> Result<(), String> {
    let edit = compose_edit(hwnd);
    let (text, caret) = compose_text_and_caret(edit);
    let state = state_mut(hwnd);
    let view_id = state.active_view.clone();
    let edit_op = match state.nick_completion.as_mut() {
        Some(last) if last.view_id == view_id && last.completion.continues(&text, caret) => {
            last.completion.advance(reverse)
        }
        _ => {
            let started = NickCompletion::start(&text, caret, active_view_members(state), reverse);
            let Some((completion, edit_op)) = started else {
                state.nick_completion = None;
                return Ok(());
            };
            state.nick_completion = Some(ComposeNickCompletion {
                view_id,
                completion,
            });
            edit_op
        }
    };
    active_compose_history(state).reset_navigation();
    SendMessageW(
        edit,
        EM_SETSEL,
        edit_op.replace.start,
        edit_op.replace.end as LPARAM,
    );
    replace_edit_selection(edit, &edit_op.text);
    Ok(())
}

/// Message > Reply to Message and Edit Message: arm the editbox with the
/// selected message.
pub(super) unsafe fn arm_compose_target_on_selected_message(
    hwnd: HWND,
    target: fn(String) -> ComposeTarget,
) -> Result<(), String> {
    let Some(event_id) = selected_timeline_event_id_for_automation(hwnd) else {
        set_local_command_status(hwnd, "no message selected".to_owned())?;
        return Err("no message selected".to_owned());
    };
    if let Err(message) = arm_compose_target(hwnd, target(event_id)) {
        set_local_command_status(hwnd, message.clone())?;
        return Err(message);
    }
    Ok(())
}

/// Arms the active window's editbox with `target` and focuses it. A reply
/// keeps what was typed; an edit replaces it with the message as one would
/// type it.
pub(super) unsafe fn arm_compose_target(hwnd: HWND, target: ComposeTarget) -> Result<(), String> {
    let edit_text = {
        let state = state_ref(hwnd);
        let activity = window_message(&state.model, &state.active_view, target.event_id())
            .ok_or_else(|| "no such message in this window".to_owned())?;
        match &target {
            ComposeTarget::Reply { .. } => None,
            ComposeTarget::Edit { .. } => Some(replies::compose_text_for_edit(
                replies::editable_message(activity, state.model.own_user_id.as_deref())?,
            )),
        }
    };
    let view_id = state_ref(hwnd).active_view.clone();
    state_mut(hwnd)
        .views
        .get_mut(&view_id)
        .expect("the active view has a window")
        .compose_mut()
        .target = Some(target);
    let edit = compose_edit(hwnd);
    match edit_text {
        Some(text) => set_compose_text(hwnd, &text)?,
        None => {
            let len = read_window_text(edit).encode_utf16().count();
            SendMessageW(edit, EM_SETSEL, len, len as LPARAM);
        }
    }
    refresh_view_content(hwnd, &view_id)?;
    SetFocus(edit);
    Ok(())
}

/// Esc in the editbox: disarms it; an edit also drops the old text it put
/// there.
unsafe fn cancel_compose_target(hwnd: HWND) -> Result<(), String> {
    let view_id = state_ref(hwnd).active_view.clone();
    let target = state_mut(hwnd)
        .views
        .get_mut(&view_id)
        .expect("the active view has a window")
        .compose_mut()
        .target
        .take();
    match target {
        None => return Ok(()),
        Some(ComposeTarget::Edit { .. }) => set_compose_text(hwnd, "")?,
        Some(ComposeTarget::Reply { .. }) => {}
    }
    refresh_view_content(hwnd, &view_id)
}

pub(super) unsafe fn prefill_react_to_selected_message(hwnd: HWND) -> Result<(), String> {
    let Some(event_id) = selected_timeline_event_id_for_automation(hwnd) else {
        set_local_command_status(hwnd, "no message selected".to_owned())?;
        return Err("no message selected".to_owned());
    };
    set_compose_text(hwnd, &react_compose_text(&event_id))?;
    SetFocus(compose_edit(hwnd));
    Ok(())
}

fn react_compose_text(event_id: &str) -> String {
    format!("/react {event_id} ")
}

pub(super) unsafe fn submit_redact_selected_message(hwnd: HWND) -> Result<(), String> {
    let Some(event_id) = selected_timeline_event_id_for_automation(hwnd) else {
        set_local_command_status(hwnd, "no message selected".to_owned())?;
        return Err("no message selected".to_owned());
    };
    let result = submit_redact_slash_command(state_mut(hwnd), event_id);
    match result {
        Ok(message) => set_local_command_status(hwnd, message),
        Err(message) => {
            set_local_command_status(hwnd, message.clone())?;
            Err(message)
        }
    }
}

/// Every editbox shares the EDIT class procedure, so the procedure the
/// first subclass replaced is the one every later subclass replaces.
pub(super) unsafe fn subclass_compose_edit(frame: HWND, edit: HWND) -> Result<(), String> {
    let previous_proc =
        SetWindowLongPtrW(edit, GWLP_WNDPROC, edit_window_proc as *const () as isize);
    if previous_proc == 0 {
        return Err(last_os_error("SetWindowLongPtrW edit subclass"));
    }
    let previous_proc = std::mem::transmute::<
        isize,
        unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    >(previous_proc);
    let state = state_mut(frame);
    match state.edit_window_proc {
        None => state.edit_window_proc = Some(previous_proc),
        Some(original) => assert!(
            std::ptr::fn_addr_eq(original, previous_proc),
            "every editbox must subclass the same EDIT procedure"
        ),
    }
    Ok(())
}

/// A window's editbox stays armed only with a message the window shows: a
/// redaction, or `/clear`, disarms it.
pub(super) fn disarm_vanished_compose_target(state: &mut WindowState, view_id: &ViewId) {
    let compose = state
        .views
        .get_mut(view_id)
        .expect("a refreshed view has a window")
        .compose_mut();
    if compose
        .target
        .as_ref()
        .is_some_and(|target| window_message(&state.model, view_id, target.event_id()).is_none())
    {
        compose.target = None;
    }
}

/// The indicator row of an armed editbox, if `view_id`'s is.
pub(super) fn armed_compose_target_row(
    state: &WindowState,
    view_id: &ViewId,
) -> Option<TimelineRow> {
    state.views[view_id]
        .compose()
        .target
        .as_ref()
        .map(|target| compose_target_row(&state.model, view_id, target))
}

pub(super) unsafe fn submit_compose_from_edit(hwnd: HWND) -> Result<(), String> {
    let edit = compose_edit(hwnd);
    // Commands may activate another window; the line still belongs to the
    // history of the editbox it was typed in.
    let view = state_ref(hwnd).active_view.clone();
    let input = read_window_text(edit);
    if input.is_empty() {
        return Err("message body must not be empty".to_owned());
    }

    let record = |entry: String| {
        state_mut(hwnd)
            .views
            .get_mut(&view)
            .expect("the submitting editbox's window must exist")
            .compose_mut()
            .history
            .record_submission(entry);
    };
    let empty = wide_null("");
    match parse_compose_submission(&input) {
        ComposeSubmission::Message { presentation, body } => {
            let target = state_ref(hwnd).views[&view].compose().target.clone();
            match &target {
                None => {
                    submit_compose(state_mut(hwnd), presentation, &body, None)?;
                }
                Some(ComposeTarget::Reply { event_id }) => {
                    send_reply(state_mut(hwnd), event_id, presentation, &body)?;
                }
                Some(ComposeTarget::Edit { event_id }) => {
                    send_edit(state_mut(hwnd), event_id, presentation, &body)?;
                }
            }
            record(input.clone());
            SendMessageW(edit, WM_SETTEXT, 0, empty.as_ptr() as LPARAM);
            if target.is_some() {
                state_mut(hwnd)
                    .views
                    .get_mut(&view)
                    .expect("the submitting editbox's window must exist")
                    .compose_mut()
                    .target = None;
                refresh_view_content(hwnd, &view)?;
            }
        }
        ComposeSubmission::LocalCommand(parsed) => {
            if let Some(entry) = local_command_history_entry(&input, &parsed) {
                record(entry);
            }
            // Cleared first: a command may fill the editbox, as /edit does.
            SendMessageW(edit, WM_SETTEXT, 0, empty.as_ptr() as LPARAM);
            match parsed {
                Ok(command) => run_local_command(hwnd, command)?,
                Err(error) => report_command_error(hwnd, error)?,
            }
        }
    }
    Ok(())
}

/// The history of the active window's editbox, which keys reach.
pub(super) fn active_compose_history(state: &mut WindowState) -> &mut ComposeHistory {
    &mut state
        .views
        .get_mut(&state.active_view)
        .expect("the active view must have a window")
        .compose_mut()
        .history
}

/// The active window's editbox, which compose input and commands act on.
pub(super) unsafe fn compose_edit(hwnd: HWND) -> HWND {
    let state = state_ref(hwnd);
    state
        .views
        .get(&state.active_view)
        .expect("the active view must have a window")
        .compose()
        .edit
}

pub(super) unsafe fn dump_compose_text(hwnd: HWND) -> Result<String, String> {
    let edit = compose_edit(hwnd);
    Ok(read_window_text(edit))
}

pub(super) unsafe fn set_compose_text(hwnd: HWND, text: &str) -> Result<(), String> {
    let edit = compose_edit(hwnd);
    active_compose_history(state_mut(hwnd)).reset_navigation();
    set_compose_edit_text(edit, text);
    update_color_index(hwnd, edit);
    Ok(())
}

pub(super) unsafe fn send_compose_key(hwnd: HWND, key: ComposeKey) -> Result<(), String> {
    let edit = compose_edit(hwnd);
    let virtual_key = match key {
        ComposeKey::Return => VK_RETURN_KEY,
        ComposeKey::Up => VK_UP_KEY,
        ComposeKey::Down => VK_DOWN_KEY,
        ComposeKey::Tab => VK_TAB_KEY,
        ComposeKey::Escape => VK_ESCAPE_KEY,
        // A sent key carries no Shift state, so Shift+Tab goes straight to
        // the completion the edit box runs for it.
        ComposeKey::ShiftTab => return complete_compose_nick(hwnd, true),
        // Nor Ctrl state, for the format chords.
        ComposeKey::Format { key } => {
            insert_mirc_format_code(hwnd, edit, mirc_format::mirc_format_code(key));
            return Ok(());
        }
    };
    SendMessageW(edit, WM_KEYDOWN, virtual_key, 0);
    Ok(())
}

/// Types a formatting code at the editbox's caret, as one undoable edit.
unsafe fn insert_mirc_format_code(frame: HWND, edit: HWND, code: char) {
    insert_compose_text(frame, edit, code.encode_utf8(&mut [0; 4]));
}

/// A click in the colour index: the colour's two digits replace the
/// number's digits typed so far, so a digit typed next cannot extend it.
pub(super) unsafe fn insert_color_number(frame: HWND, index: PaletteIndex) {
    let edit = compose_edit(frame);
    let code = color_code_at_caret(edit)
        .expect("the colour index shows only while a colour number is typed");
    let (_, caret) = edit_selection(edit);
    SendMessageW(edit, EM_SETSEL, caret - code.typed_digits, caret as LPARAM);
    insert_compose_text(frame, edit, &format!("{:02}", index.get()));
}

/// Types `text` at the editbox's caret, as one undoable edit.
unsafe fn insert_compose_text(frame: HWND, edit: HWND, text: &str) {
    active_compose_history(state_mut(frame)).reset_navigation();
    let text = wide_null(text);
    // wParam TRUE: the replacement can be undone.
    SendMessageW(edit, EM_REPLACESEL, 1, text.as_ptr() as LPARAM);
    update_color_index(frame, edit);
}

pub(super) unsafe fn submit_compose_from_control(
    hwnd: HWND,
    explicit_room_id: Option<&str>,
) -> Result<String, String> {
    let edit = compose_edit(hwnd);
    let input = read_window_text(edit);
    let room_id = submit_compose(
        state_mut(hwnd),
        RoomMessagePresentationKind::Text,
        &input,
        explicit_room_id,
    )?;
    let empty = wide_null("");
    SendMessageW(edit, WM_SETTEXT, 0, empty.as_ptr() as LPARAM);
    update_color_index(hwnd, edit);
    Ok(room_id)
}

/// The message an editbox line sends: its text without mIRC codes, in the
/// styles they set. Codes alone send nothing.
pub(super) fn message_text_from_input(input: &str) -> Result<(String, Vec<TextStyleSpan>), String> {
    let (body, styles) = mirc_format::parse_mirc_input(input);
    if body.is_empty() {
        return Err("message body must not be empty".to_owned());
    }
    Ok((body, styles))
}

fn submit_compose(
    state: &mut WindowState,
    presentation: RoomMessagePresentationKind,
    input: &str,
    explicit_room_id: Option<&str>,
) -> Result<String, String> {
    let (body, styles) = message_text_from_input(input)?;
    let destination = resolve_compose_destination(state, explicit_room_id)?;

    let clears_active_thread_marker = explicit_room_id.is_none() && destination.thread.is_some();
    enqueue_send_message(
        state,
        SendMessageRequest {
            room_id: destination.room_id.clone(),
            presentation,
            body,
            styles,
            thread: destination.thread,
            reply: None,
        },
    )?;
    if clears_active_thread_marker {
        clear_view_activity_marker(state, &state.active_view.clone());
    }

    Ok(destination.room_id)
}

/// Sends `input` to a joined room's main timeline, from a command naming
/// the room; returns the text sent.
pub(super) fn send_room_message(
    state: &mut WindowState,
    room_id: String,
    presentation: RoomMessagePresentationKind,
    input: &str,
) -> Result<String, String> {
    if sendable_room_by_id(&state.model.rooms, &room_id).is_none() {
        return Err(format!("room is not joined: {room_id}"));
    }
    let (body, styles) = message_text_from_input(input)?;
    enqueue_send_message(
        state,
        SendMessageRequest {
            room_id,
            presentation,
            body: body.clone(),
            styles,
            thread: None,
            reply: None,
        },
    )?;
    Ok(body)
}

pub(super) fn enqueue_send_message(
    state: &mut WindowState,
    request: SendMessageRequest,
) -> Result<(), String> {
    let command = Command::SendMessage(request);
    state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue send message command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ComposeDestination {
    pub(super) room_id: String,
    pub(super) thread: Option<SendMessageThreadTarget>,
}

/// The room `/sendfile` sends to: the active window's.
pub(super) fn send_file_room_id(state: &WindowState) -> Result<String, String> {
    match &state.active_view {
        ViewId::Room(room_id) => Ok(room_id.clone()),
        ViewId::Thread { .. } => Err("file attachments cannot target threads yet".to_owned()),
        ViewId::Status | ViewId::RoomList => Err("no room selected".to_owned()),
    }
}

/// The joined room a file goes to; an invite or a room left meanwhile
/// takes none.
fn sendable_file_room<'a>(
    state: &'a WindowState,
    room_id: &str,
) -> Result<&'a RoomSummary, String> {
    sendable_room_by_id(&state.model.rooms, room_id)
        .ok_or_else(|| format!("room is not joined: {room_id}"))
}

/// `/sendfile <path>` and the Send File dialog's OK: uploads the file to
/// the room, with the core choosing its type from the name.
pub(super) fn submit_send_file(
    state: &mut WindowState,
    room_id: String,
    path: PathBuf,
) -> Result<String, String> {
    sendable_file_room(state, &room_id)?;
    enqueue_command(
        state,
        Command::SendFile(SendFileRequest {
            room_id: room_id.clone(),
            path,
            filename: None,
            content_type: None,
        }),
    )?;
    Ok(format!("File queued for {room_id}"))
}

/// The Send File dialog for `room_id`, which the caller captured before
/// the dialog opens, so activating another window meanwhile does not
/// redirect the file. `None` when cancelled.
pub(super) unsafe fn run_send_file_dialog(
    frame: HWND,
    room_id: String,
) -> Result<Option<String>, String> {
    let room_name = sendable_file_room(state_ref(frame), &room_id)?
        .display_name
        .clone();
    // Modal: core events keep arriving, so nothing is held across it.
    match send_file::choose_file_to_send(frame, &room_name)? {
        Some(path) => submit_send_file(state_mut(frame), room_id, path).map(Some),
        None => Ok(None),
    }
}

pub(super) fn resolve_compose_destination(
    state: &WindowState,
    explicit_room_id: Option<&str>,
) -> Result<ComposeDestination, String> {
    if let Some(room_id) = explicit_room_id {
        if sendable_room_by_id(&state.model.rooms, room_id).is_some() {
            return Ok(ComposeDestination {
                room_id: room_id.to_owned(),
                thread: None,
            });
        }
        if let Some(room) = room_by_id(&state.model.rooms, room_id)
            && is_invited_room(room)
        {
            return Err(format!("room is not joined: {room_id}"));
        }
        return Err(format!("unknown room id: {room_id}"));
    }

    match &state.active_view {
        ViewId::Room(room_id) => {
            if sendable_room_by_id(&state.model.rooms, room_id).is_some() {
                Ok(ComposeDestination {
                    room_id: room_id.clone(),
                    thread: None,
                })
            } else {
                Err(format!("room is not joined: {room_id}"))
            }
        }
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => {
            projected_thread_by_id(&state.model, room_id, thread_root_event_id)
                .expect("active thread compose must target an existing projected thread");
            Ok(ComposeDestination {
                room_id: room_id.clone(),
                thread: Some(SendMessageThreadTarget {
                    thread_root_event_id: thread_root_event_id.clone(),
                    latest_event_id: latest_durable_thread_event_id(
                        &state.model,
                        room_id,
                        thread_root_event_id,
                    ),
                }),
            })
        }
        ViewId::Status | ViewId::RoomList => Err("no room selected".to_owned()),
    }
}

fn latest_durable_thread_event_id(
    model: &UiModel,
    room_id: &str,
    thread_root_event_id: &str,
) -> String {
    let room_view = model
        .room_views
        .get(room_id)
        .expect("thread compose must target an existing room view");
    let mut latest_event_id = thread_root_event_id;

    for activity in &room_view.activities {
        if room_activity_belongs_to_thread(activity, thread_root_event_id)
            && let Some(event_id) = room_activity_event_id(activity)
            && !event_id.starts_with("txn:")
        {
            latest_event_id = event_id;
        }
    }

    latest_event_id.to_owned()
}

unsafe fn navigate_compose_history(
    hwnd: HWND,
    direction: ComposeHistoryDirection,
) -> Result<(), String> {
    let edit = compose_edit(hwnd);
    let replacement = match direction {
        ComposeHistoryDirection::Previous => {
            let current_text = read_window_text(edit);
            active_compose_history(state_mut(hwnd))
                .previous(current_text)
                .map(str::to_owned)
        }
        ComposeHistoryDirection::Next => active_compose_history(state_mut(hwnd)).next(),
    };
    if let Some(text) = replacement {
        set_compose_edit_text(edit, &text);
    }
    Ok(())
}

unsafe extern "system" fn edit_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // The editbox is a child of its MDI child, which is a child of the MDI
    // client: the state lives on the frame.
    let parent = frame_window_for(hwnd);
    if message == WM_KILLFOCUS {
        hide_color_index(state_ref(parent));
    }
    // While the colour index shows, Esc only hides it (and its character
    // is swallowed below); the reply or edit target stays armed.
    if message == WM_KEYDOWN && wparam == VK_ESCAPE_KEY && color_index_shown(parent) {
        hide_color_index(state_ref(parent));
        return 0;
    }
    let result = edit_message(parent, hwnd, message, wparam, lparam);
    // Keys, clicks and edit commands go to the focused editbox, which is
    // the active window's; any of them can start or end a colour number
    // at its caret. Esc, which hides the index, is not one of them.
    let escape = matches!(message, WM_KEYDOWN | WM_CHAR) && wparam == VK_ESCAPE_KEY;
    if matches!(
        message,
        WM_KEYDOWN | WM_CHAR | WM_LBUTTONUP | WM_PASTE | WM_CUT | WM_CLEAR | WM_UNDO | EM_UNDO
    ) && !escape
    {
        update_color_index(parent, hwnd);
    }
    result
}

unsafe fn edit_message(
    parent: HWND,
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let original = state_ref(parent).edit_window_proc;

    if message == WM_SETFOCUS {
        let result = CallWindowProcW(original, hwnd, message, wparam, lparam);
        if state_ref(parent).render_state_mode {
            // Snapshots must not depend on where the blinking caret is.
            HideCaret(hwnd);
        }
        return result;
    }

    // Windows sends the wheel to the window under the pointer, Wine to the
    // focused one: an editbox passes it on to its window's timeline. Its
    // MDI child would hand it to DefMDIChildProc instead.
    if message == WM_MOUSEWHEEL {
        return SendMessageW(editbox_timeline(parent, hwnd), message, wparam, lparam);
    }

    // Keys reach only the focused editbox, and focusing an editbox
    // activates its window, so it is always the active window's.
    let is_compose_key = message == WM_KEYDOWN
        && matches!(
            wparam,
            VK_RETURN_KEY
                | VK_UP_KEY
                | VK_DOWN_KEY
                | VK_TAB_KEY
                | VK_ESCAPE_KEY
                | VK_PRIOR_KEY
                | VK_NEXT_KEY
                | VK_HOME_KEY
                | VK_END_KEY
        );
    if is_compose_key {
        assert_eq!(
            compose_edit(parent),
            hwnd,
            "compose keys must arrive at the active window's editbox"
        );
    }

    // Paging keys produce no WM_CHAR, so swallowing the key down is enough.
    if message == WM_KEYDOWN
        && let Some(gesture) = compose_scroll_gesture(
            wparam,
            GetKeyState(VK_CONTROL as i32) < 0,
            GetWindowTextLengthW(hwnd) == 0,
        )
    {
        scroll_timeline_by(editbox_timeline(parent, hwnd), gesture);
        return 0;
    }

    if message == WM_KEYDOWN && wparam == VK_RETURN_KEY {
        if let Err(message) = submit_compose_from_edit(parent) {
            let _ = set_local_command_status(parent, message);
        }
        return 0;
    }

    if message == WM_KEYDOWN && wparam == VK_UP_KEY {
        let _ = navigate_compose_history(parent, ComposeHistoryDirection::Previous);
        return 0;
    }

    if message == WM_KEYDOWN && wparam == VK_DOWN_KEY {
        let _ = navigate_compose_history(parent, ComposeHistoryDirection::Next);
        return 0;
    }

    if message == WM_KEYDOWN && wparam == VK_TAB_KEY {
        let reverse = GetKeyState(VK_SHIFT as i32) < 0;
        if let Err(message) = complete_compose_nick(parent, reverse) {
            let _ = set_local_command_status(parent, message);
        }
        return 0;
    }

    if message == WM_KEYDOWN && wparam == VK_ESCAPE_KEY {
        if let Err(message) = cancel_compose_target(parent) {
            let _ = set_local_command_status(parent, message);
        }
        return 0;
    }

    if message == WM_KEYDOWN
        && let Some(key) = mirc_format::mirc_format_key(
            GetKeyState(VK_CONTROL as i32) < 0,
            GetKeyState(VK_MENU as i32) < 0,
            wparam,
        )
    {
        assert_eq!(
            compose_edit(parent),
            hwnd,
            "format keys must arrive at the active window's editbox"
        );
        insert_mirc_format_code(parent, hwnd, mirc_format::mirc_format_code(key));
        return 0;
    }

    // TranslateMessage also turns the Tab and Esc keys, and the format
    // chords, into these characters: Ctrl+B, K, U, O, R and E, and Ctrl+I
    // is Tab's. The key down already acted, so the edit box must not see them:
    // a single-line edit box rejects them, with a beep on Windows.
    if message == WM_CHAR
        && matches!(
            wparam,
            VK_TAB_KEY | VK_ESCAPE_KEY | 0x02 | 0x0b | 0x15 | 0x0f | 0x12 | 0x05
        )
    {
        return 0;
    }

    if message == WM_CHAR
        || message == WM_PASTE
        || (message == WM_KEYDOWN && wparam == VK_DELETE_KEY)
    {
        active_compose_history(state_mut(parent)).reset_navigation();
    }

    CallWindowProcW(original, hwnd, message, wparam, lparam)
}

unsafe fn set_compose_edit_text(hwnd: HWND, text: &str) {
    set_edit_text(hwnd, text);
    let len = text.encode_utf16().count();
    SendMessageW(hwnd, EM_SETSEL, len, len as LPARAM);
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;

    #[test]
    fn paging_keys_scroll_and_ctrl_home_end_need_an_empty_editbox() {
        for edit_empty in [false, true] {
            assert_eq!(
                compose_scroll_gesture(VK_PRIOR_KEY, false, edit_empty),
                Some(ScrollGesture::Pages(-1))
            );
            assert_eq!(
                compose_scroll_gesture(VK_NEXT_KEY, false, edit_empty),
                Some(ScrollGesture::Pages(1))
            );
            assert_eq!(
                compose_scroll_gesture(VK_PRIOR_KEY, true, edit_empty),
                Some(ScrollGesture::Lines(-1))
            );
            assert_eq!(
                compose_scroll_gesture(VK_NEXT_KEY, true, edit_empty),
                Some(ScrollGesture::Lines(1))
            );
            assert_eq!(compose_scroll_gesture(VK_HOME_KEY, false, edit_empty), None);
            assert_eq!(compose_scroll_gesture(VK_END_KEY, false, edit_empty), None);
        }
        assert_eq!(
            compose_scroll_gesture(VK_HOME_KEY, true, true),
            Some(ScrollGesture::Top)
        );
        assert_eq!(
            compose_scroll_gesture(VK_END_KEY, true, true),
            Some(ScrollGesture::Bottom)
        );
        assert_eq!(compose_scroll_gesture(VK_HOME_KEY, true, false), None);
        assert_eq!(compose_scroll_gesture(VK_END_KEY, true, false), None);
    }

    unsafe fn install_test_active_timeline_event(hwnd: HWND, event_id: &str) {
        let instance = GetModuleHandleW(null());
        register_room_content_class(instance).unwrap();
        register_timeline_class(instance).unwrap();
        let content = CreateWindowExW(
            0,
            wide_null(ROOM_CONTENT_CLASS_NAME).as_ptr(),
            wide_null("").as_ptr(),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            0,
            0,
            hwnd,
            null_mut(),
            instance,
            null_mut(),
        );
        assert!(!content.is_null(), "test room content must be created");
        let active_view = state_ref(hwnd).active_view.clone();
        state_mut(hwnd)
            .views
            .get_mut(&active_view)
            .expect("the test compose window must track the active view")
            .content = content;
        set_test_timeline_font(hwnd, &active_view, content);
        set_timeline_rows(
            room_content_timeline(content),
            &[TimelineRow::log(
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                    own: false,
                },
                "[00:00] @alice: message".to_owned(),
            )
            .with_event_id(Some(event_id))],
            String::new(),
        );
    }

    #[test]
    fn latest_durable_thread_event_id_falls_back_to_root_when_root_is_only_durable_event() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root:example",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();

        assert_eq!(
            latest_durable_thread_event_id(&state.model, "!ops:example", "$thread-root:example"),
            "$thread-root:example"
        );
    }

    #[test]
    fn latest_durable_thread_event_id_ignores_newer_pending_local_thread_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root:example",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$thread-1:example",
                None,
                Some(thread_relation("$thread-root:example")),
                RoomMessagePresentationKind::Text,
                "durable reply",
                1_700_000_120_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued_in_thread("!ops:example", "thread-pending-1", "$thread-root:example"),
        )
        .unwrap();

        assert_eq!(
            latest_durable_thread_event_id(&state.model, "!ops:example", "$thread-root:example"),
            "$thread-1:example"
        );
    }

    #[test]
    fn each_window_recalls_only_what_was_typed_in_it() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .model
            .room_views
            .insert("!room:example".to_owned(), RoomViewModel::default());

        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/wat");

                // A second window, with its own editbox.
                let room_view = room_view_id("!room:example");
                let child = state_ref(hwnd).views[&ViewId::Status].child;
                let edit = CreateWindowExW(
                    0,
                    wide_null(EDIT_CLASS_NAME).as_ptr(),
                    wide_null("").as_ptr(),
                    WS_CHILD,
                    0,
                    0,
                    0,
                    0,
                    child,
                    null_mut(),
                    GetModuleHandleW(null()),
                    null_mut(),
                );
                assert!(!edit.is_null());
                state_mut(hwnd).views.insert(
                    room_view.clone(),
                    MdiViewState {
                        child,
                        content: null_mut(),
                        compose: Some(ViewCompose {
                            edit,
                            history: ComposeHistory::default(),
                            target: None,
                        }),
                    },
                );
                state_mut(hwnd).active_view = room_view.clone();
                install_test_view_content(hwnd);

                navigate_compose_history(hwnd, ComposeHistoryDirection::Previous).unwrap();
                assert_eq!(dump_compose_text(hwnd).unwrap(), "");
                type_compose_line(hwnd, "/help clear");

                state_mut(hwnd).active_view = ViewId::Status;
                navigate_compose_history(hwnd, ComposeHistoryDirection::Previous).unwrap();
                assert_eq!(dump_compose_text(hwnd).unwrap(), "/wat");
                navigate_compose_history(hwnd, ComposeHistoryDirection::Previous).unwrap();
                assert_eq!(dump_compose_text(hwnd).unwrap(), "/wat");

                state_mut(hwnd).active_view = room_view;
                navigate_compose_history(hwnd, ComposeHistoryDirection::Previous).unwrap();
                assert_eq!(dump_compose_text(hwnd).unwrap(), "/help clear");
            });
        }
    }

    #[test]
    fn invalid_e2ee_recover_form_is_not_recorded_or_recalled_in_compose_history() {
        let mut history = ComposeHistory::default();
        let input = "/e2ee recover secret phrase".to_owned();
        let ComposeSubmission::LocalCommand(command) = parse_compose_submission(&input) else {
            panic!("expected local command submission");
        };

        let history_entry = local_command_history_entry(&input, &command);
        assert_eq!(history_entry, None);
        if let Some(entry) = history_entry {
            history.record_submission(entry);
        }

        history.record_submission("safe draft".to_owned());

        assert!(!history.entries.iter().any(|entry| entry.contains("secret")));
        assert_eq!(
            history.previous("later draft".to_owned()),
            Some("safe draft")
        );
        assert_eq!(history.previous("ignored".to_owned()), Some("safe draft"));
    }

    #[test]
    fn invalid_e2ee_setup_form_is_not_recorded_or_recalled_in_compose_history() {
        let mut history = ComposeHistory::default();
        let malformed = "/e2ee setup wait passphrases secret phrase";
        let ComposeSubmission::LocalCommand(command) = parse_compose_submission(malformed) else {
            panic!("expected local command submission");
        };
        assert_eq!(local_command_history_entry(malformed, &command), None);
        history.record_submission("safe draft".to_owned());
        assert!(!history.entries.iter().any(|entry| entry.contains("secret")));
    }

    #[test]
    fn valid_e2ee_setup_passphrase_form_is_not_recorded_in_compose_history() {
        let mut history = ComposeHistory::default();
        let input = "/e2ee setup wait passphrase secret phrase";
        let ComposeSubmission::LocalCommand(command) = parse_compose_submission(input) else {
            panic!("expected local command submission");
        };

        assert_eq!(local_command_history_entry(input, &command), None);
        history.record_submission("safe draft".to_owned());
        assert!(
            !history
                .entries
                .iter()
                .any(|entry| entry.contains("secret phrase"))
        );
    }

    #[test]
    fn sendfile_goes_to_the_active_joined_room_and_lets_the_core_type_it() {
        let mut state = test_state();
        let (sender, commands) = inert_command_sender();
        std::mem::forget(commands);
        state.command_sender = Some(sender);
        state.model.rooms = vec![
            room("!ops:example", "Ops", false),
            invited_room("!new:example", "New"),
        ];

        state.active_view = ViewId::Room("!ops:example".to_owned());
        let room_id = send_file_room_id(&state).unwrap();
        assert_eq!(
            submit_send_file(&mut state, room_id, PathBuf::from("C:\\shot.png")),
            Ok("File queued for !ops:example".to_owned())
        );
        assert_eq!(
            state.recorded_commands,
            [Command::SendFile(SendFileRequest {
                room_id: "!ops:example".to_owned(),
                path: PathBuf::from("C:\\shot.png"),
                filename: None,
                content_type: None,
            })]
        );

        assert_eq!(
            submit_send_file(&mut state, "!new:example".to_owned(), PathBuf::from("a")),
            Err("room is not joined: !new:example".to_owned())
        );
        state.active_view = ViewId::Thread {
            room_id: "!ops:example".to_owned(),
            thread_root_event_id: "$root".to_owned(),
        };
        assert_eq!(
            send_file_room_id(&state),
            Err("file attachments cannot target threads yet".to_owned())
        );
        state.active_view = ViewId::Status;
        assert_eq!(
            send_file_room_id(&state),
            Err("no room selected".to_owned())
        );
        assert_eq!(state.recorded_commands.len(), 1);
    }

    fn reply_to(event_id: &str) -> ComposeTarget {
        ComposeTarget::Reply {
            event_id: event_id.to_owned(),
        }
    }

    fn edit_of(event_id: &str) -> ComposeTarget {
        ComposeTarget::Edit {
            event_id: event_id.to_owned(),
        }
    }

    #[test]
    fn the_reply_menu_arms_the_editbox_and_enter_sends_the_reply() {
        unsafe {
            with_compose_target_window(|hwnd| {
                state_mut(hwnd).selected_timeline_event = Some(SelectedTimelineEvent {
                    view_id: room_view_id("!ops:example"),
                    event_id: "$q".to_owned(),
                });
                set_compose_text(hwnd, "half typed").unwrap();
                arm_compose_target_on_selected_message(hwnd, |event_id| ComposeTarget::Reply {
                    event_id,
                })
                .unwrap();
                assert_eq!(active_compose_target(hwnd), Some(reply_to("$q")));
                assert_eq!(dump_compose_text(hwnd).unwrap(), "half typed");
                assert!(state_ref(hwnd).recorded_commands.is_empty());
                assert_eq!(
                    active_timeline_lines(hwnd).last().unwrap(),
                    "* Replying to <@alice:example> are we deploying today? (Esc cancels)"
                );

                type_compose_line(hwnd, "sure");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![reply_command(RoomMessagePresentationKind::Text, "sure")]
                );
                assert_eq!(active_compose_target(hwnd), None);
                assert_eq!(dump_compose_text(hwnd).unwrap(), "");
                assert!(
                    !active_timeline_lines(hwnd)
                        .iter()
                        .any(|line| line.starts_with("* Replying"))
                );
            });
        }
    }

    #[test]
    fn other_commands_leave_a_reply_armed_and_me_replies_with_an_emote() {
        unsafe {
            with_compose_target_window(|hwnd| {
                type_compose_line(hwnd, "/reply $q");
                assert_eq!(active_compose_target(hwnd), Some(reply_to("$q")));
                type_compose_line(hwnd, "/names");
                assert_eq!(active_compose_target(hwnd), Some(reply_to("$q")));
                type_compose_line(hwnd, "/me nods");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![reply_command(RoomMessagePresentationKind::Emote, "nods")]
                );
                assert_eq!(active_compose_target(hwnd), None);
            });
        }
    }

    #[test]
    fn editing_an_emote_prefills_it_and_keeps_it_an_emote() {
        unsafe {
            with_compose_target_window(|hwnd| {
                state_mut(hwnd).selected_timeline_event = Some(SelectedTimelineEvent {
                    view_id: room_view_id("!ops:example"),
                    event_id: "$wave".to_owned(),
                });
                assert!(active_message_action_state(state_ref(hwnd)).edit_enabled);
                arm_compose_target_on_selected_message(hwnd, |event_id| ComposeTarget::Edit {
                    event_id,
                })
                .unwrap();
                assert_eq!(active_compose_target(hwnd), Some(edit_of("$wave")));
                assert_eq!(dump_compose_text(hwnd).unwrap(), "/me waves");
                assert_eq!(
                    active_timeline_lines(hwnd).last().unwrap(),
                    "* Editing * @me:example waves (Esc cancels)"
                );

                type_compose_line(hwnd, "/me waves back");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![Command::EditMessage(EditMessageRequest {
                        room_id: "!ops:example".to_owned(),
                        target_event_id: "$wave".to_owned(),
                        presentation: RoomMessagePresentationKind::Emote,
                        body: "waves back".to_owned(),
                        styles: Vec::new(),
                    })]
                );
                assert_eq!(active_compose_target(hwnd), None);
            });
        }
    }

    #[test]
    fn an_unchanged_edit_sends_nothing_and_disarms() {
        unsafe {
            with_compose_target_window(|hwnd| {
                type_compose_line(hwnd, "/edit $mine");
                assert_eq!(dump_compose_text(hwnd).unwrap(), "deploy at 14:00");
                submit_compose_from_edit(hwnd).unwrap();
                assert!(state_ref(hwnd).recorded_commands.is_empty());
                assert_eq!(active_compose_target(hwnd), None);
                assert_eq!(dump_compose_text(hwnd).unwrap(), "");
            });
        }
    }

    #[test]
    fn edit_text_after_the_event_id_keeps_the_messages_kind() {
        unsafe {
            with_compose_target_window(|hwnd| {
                type_compose_line(hwnd, "/edit $wave waves twice");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![Command::EditMessage(EditMessageRequest {
                        room_id: "!ops:example".to_owned(),
                        target_event_id: "$wave".to_owned(),
                        presentation: RoomMessagePresentationKind::Emote,
                        body: "waves twice".to_owned(),
                        styles: Vec::new(),
                    })]
                );
                assert_eq!(active_compose_target(hwnd), None);
            });
        }
    }

    fn bold_span(start: usize, end: usize) -> TextStyleSpan {
        TextStyleSpan {
            start,
            end,
            style: bold_style(),
        }
    }

    #[test]
    fn formatting_codes_send_as_styles_and_codes_alone_send_nothing() {
        unsafe {
            with_compose_target_window(|hwnd| {
                type_compose_line(hwnd, "\x02hi\x02 there");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![Command::SendMessage(SendMessageRequest {
                        room_id: "!ops:example".to_owned(),
                        presentation: RoomMessagePresentationKind::Text,
                        body: "hi there".to_owned(),
                        styles: vec![bold_span(0, 2)],
                        thread: None,
                        reply: None,
                    })]
                );
                assert_eq!(
                    dump_model_from_state(state_ref(hwnd)).submitted_message_commands,
                    vec![DumpSubmittedMessageCommand {
                        room_id: "!ops:example".to_owned(),
                        presentation: RoomMessagePresentationKind::Text,
                        body: "hi there".to_owned(),
                        styles: vec![bold_span(0, 2)],
                        thread: None,
                    }]
                );

                set_compose_text(hwnd, "\x02\x02").unwrap();
                assert_eq!(
                    submit_compose_from_edit(hwnd),
                    Err("message body must not be empty".to_owned())
                );
                type_compose_line(hwnd, "/me \x1fwaves\x0f");
                type_compose_line(hwnd, "/reply $q \x02yes");
                let styled = |command: &Command| match command {
                    Command::SendMessage(request) => (request.body.clone(), request.styles.clone()),
                    other => panic!("unexpected command {other:?}"),
                };
                let underline = TextStyleSpan {
                    start: 0,
                    end: 5,
                    style: mirx_core::TextStyle {
                        underline: true,
                        ..mirx_core::TextStyle::default()
                    },
                };
                assert_eq!(
                    state_ref(hwnd).recorded_commands[1..]
                        .iter()
                        .map(styled)
                        .collect::<Vec<_>>(),
                    [
                        ("waves".to_owned(), vec![underline]),
                        ("yes".to_owned(), vec![bold_span(0, 3)]),
                    ]
                );
                assert_eq!(
                    dump_model_from_state(state_ref(hwnd)).submitted_reply_commands[0].styles,
                    [bold_span(0, 3)]
                );
            });
        }
    }

    #[test]
    fn editing_a_formatted_message_prefills_its_codes() {
        let mut state = compose_target_state();
        let mut activity = sent_message_activity_with_presentation(
            "!ops:example",
            "$styled",
            None,
            RoomMessagePresentationKind::Text,
            "deploy now",
            1_700_000_180_000,
        );
        let RoomActivityKind::Message(message) = &mut activity.kind else {
            unreachable!()
        };
        message.sender = user_profile("@me:example");
        message.styles = vec![bold_span(7, 10)];
        apply_core_event(&mut state, Event::RoomActivity(Box::new(activity))).unwrap();
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                refresh_view_content(hwnd, &room_view_id("!ops:example")).unwrap();
                type_compose_line(hwnd, "/edit $styled");
                assert_eq!(dump_compose_text(hwnd).unwrap(), "deploy \x02now");
                // The same text in other codes is the same message.
                set_compose_text(hwnd, "deploy \x02now\x02").unwrap();
                submit_compose_from_edit(hwnd).unwrap();
                assert!(state_ref(hwnd).recorded_commands.is_empty());

                type_compose_line(hwnd, "/edit $styled");
                set_compose_text(hwnd, "deploy now").unwrap();
                submit_compose_from_edit(hwnd).unwrap();
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![Command::EditMessage(EditMessageRequest {
                        room_id: "!ops:example".to_owned(),
                        target_event_id: "$styled".to_owned(),
                        presentation: RoomMessagePresentationKind::Text,
                        body: "deploy now".to_owned(),
                        styles: Vec::new(),
                    })]
                );
                type_compose_line(hwnd, "/edit $styled \x02deploy\x02 now");
                assert_eq!(
                    dump_model_from_state(state_ref(hwnd)).submitted_edit_commands[1].styles,
                    [bold_span(0, 6)]
                );
            });
        }
    }

    #[test]
    fn format_chords_type_codes_and_the_edit_box_never_sees_their_characters() {
        use windows_sys::Win32::UI::Controls::EM_UNDO;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardState, SetKeyboardState};
        unsafe {
            with_compose_target_window(|hwnd| {
                let edit = compose_edit(hwnd);
                subclass_compose_edit(hwnd, edit).unwrap();
                let mut keys = [0u8; 256];
                assert_ne!(GetKeyboardState(keys.as_mut_ptr()), 0);
                let saved = keys;
                // What TranslateMessage makes of each chord, after its key.
                let chord = |letter: u8, ch: usize| {
                    SendMessageW(edit, WM_KEYDOWN, usize::from(letter), 0);
                    SendMessageW(edit, WM_CHAR, ch, 0);
                };

                keys[VK_CONTROL as usize] = 0x80;
                assert_ne!(SetKeyboardState(keys.as_ptr()), 0);
                for (letter, ch) in [
                    (b'B', 0x02),
                    (b'K', 0x0b),
                    (b'U', 0x15),
                    (b'I', 0x09),
                    (b'O', 0x0f),
                    (b'R', 0x12),
                    (b'E', 0x05),
                ] {
                    chord(letter, ch);
                }
                assert_eq!(read_window_text(edit), "\x02\x03\x1f\x1d\x0f\x16\x1e");
                // The codes undo as typing does: a run of them at once.
                SendMessageW(edit, EM_UNDO, 0, 0);
                assert_eq!(read_window_text(edit), "");
                chord(b'B', 0x02);

                // AltGr is Ctrl+Alt: it types, it does not format.
                keys[VK_MENU as usize] = 0x80;
                assert_ne!(SetKeyboardState(keys.as_ptr()), 0);
                SendMessageW(edit, WM_KEYDOWN, usize::from(b'B'), 0);
                assert_eq!(read_window_text(edit), "\x02");

                keys = saved;
                keys[VK_CONTROL as usize] = 0;
                keys[VK_MENU as usize] = 0;
                assert_ne!(SetKeyboardState(keys.as_ptr()), 0);
                chord(b'B', usize::from(b'b'));
                assert_eq!(read_window_text(edit), "\x02b");
                assert_ne!(SetKeyboardState(saved.as_ptr()), 0);
            });
        }
    }

    #[test]
    fn the_automation_format_key_types_its_code() {
        unsafe {
            with_compose_target_window(|hwnd| {
                set_compose_text(hwnd, "a").unwrap();
                let edit = compose_edit(hwnd);
                SendMessageW(edit, EM_SETSEL, 1, 1);
                send_compose_key(
                    hwnd,
                    ComposeKey::Format {
                        key: crate::automation::MircFormatKey::Color,
                    },
                )
                .unwrap();
                assert_eq!(read_window_text(edit), "a\x03");
            });
        }
    }

    unsafe fn type_chars(edit: HWND, text: &str) {
        for ch in text.encode_utf16() {
            SendMessageW(edit, WM_CHAR, usize::from(ch), 0);
        }
    }

    unsafe fn press_color_key(hwnd: HWND) {
        send_compose_key(
            hwnd,
            ComposeKey::Format {
                key: crate::automation::MircFormatKey::Color,
            },
        )
        .unwrap();
    }

    unsafe fn color_index_part(hwnd: HWND) -> Option<crate::automation::UiColorCodePart> {
        color_index::dump(hwnd).map(|index| index.part)
    }

    #[test]
    fn the_colour_index_shows_while_a_colour_number_is_typed() {
        use crate::automation::UiColorCodePart::{Background, Foreground};
        unsafe {
            with_compose_target_window(|hwnd| {
                let edit = compose_edit(hwnd);
                subclass_compose_edit(hwnd, edit).unwrap();
                set_compose_text(hwnd, "hi ").unwrap();
                assert!(!color_index_shown(hwnd));
                press_color_key(hwnd);
                assert_eq!(color_index_part(hwnd), Some(Foreground));
                for (typed, part) in [
                    ("4", Some(Foreground)),
                    (",", Some(Background)),
                    ("1", Some(Background)),
                    ("2", None),
                ] {
                    type_chars(edit, typed);
                    assert_eq!(color_index_part(hwnd), part, "after {typed:?}");
                }
                assert_eq!(read_window_text(edit), "hi \x034,12");

                // A click types the colour's two digits over what was
                // typed of the number.
                press_color_key(hwnd);
                type_chars(edit, "1");
                let cell = color_index::dump(hwnd).unwrap().cells[12];
                let (x, y) = (cell.x + cell.width / 2, cell.y + cell.height / 2);
                SendMessageW(
                    state_ref(hwnd).controls.color_index,
                    WM_LBUTTONDOWN,
                    0,
                    ((y as LPARAM) << 16) | x as LPARAM,
                );
                assert_eq!(read_window_text(edit), "hi \x034,12\x0312");
                assert!(!color_index_shown(hwnd));
                type_chars(edit, ",");
                assert_eq!(color_index_part(hwnd), Some(Background));
                // Moving the caret off the code hides it.
                SendMessageW(edit, EM_SETSEL, 0, 0);
                SendMessageW(edit, WM_KEYDOWN, VK_HOME_KEY, 0);
                assert!(!color_index_shown(hwnd));
            });
        }
    }

    #[test]
    fn escape_hides_the_colour_index_before_it_cancels_a_reply() {
        unsafe {
            with_compose_target_window(|hwnd| {
                let edit = compose_edit(hwnd);
                subclass_compose_edit(hwnd, edit).unwrap();
                type_compose_line(hwnd, "/reply $q");
                set_compose_text(hwnd, "draft ").unwrap();
                press_color_key(hwnd);
                assert!(color_index_shown(hwnd));
                SendMessageW(edit, WM_KEYDOWN, VK_ESCAPE_KEY, 0);
                SendMessageW(edit, WM_CHAR, VK_ESCAPE_KEY, 0);
                assert!(!color_index_shown(hwnd));
                assert!(active_compose_target(hwnd).is_some());
                assert_eq!(read_window_text(edit), "draft \x03");

                SendMessageW(edit, WM_KEYDOWN, VK_ESCAPE_KEY, 0);
                assert_eq!(active_compose_target(hwnd), None);
                assert_eq!(read_window_text(edit), "draft \x03");
                // Losing the focus hides it too.
                type_chars(edit, "4");
                assert!(color_index_shown(hwnd));
                SendMessageW(edit, WM_KILLFOCUS, 0, 0);
                assert!(!color_index_shown(hwnd));
            });
        }
    }

    #[test]
    fn escape_cancels_an_edit_with_its_text_but_keeps_a_replys_text() {
        unsafe {
            with_compose_target_window(|hwnd| {
                type_compose_line(hwnd, "/edit $mine");
                cancel_compose_target(hwnd).unwrap();
                assert_eq!(active_compose_target(hwnd), None);
                assert_eq!(dump_compose_text(hwnd).unwrap(), "");

                type_compose_line(hwnd, "/reply $q");
                set_compose_text(hwnd, "draft").unwrap();
                cancel_compose_target(hwnd).unwrap();
                assert_eq!(active_compose_target(hwnd), None);
                assert_eq!(dump_compose_text(hwnd).unwrap(), "draft");
                assert!(state_ref(hwnd).recorded_commands.is_empty());
            });
        }
    }

    #[test]
    fn redacting_the_target_disarms_the_editbox() {
        unsafe {
            with_compose_target_window(|hwnd| {
                type_compose_line(hwnd, "/reply $q");
                apply_core_event(
                    state_mut(hwnd),
                    incoming_redacted("!ops:example", "$q", "$q-gone"),
                )
                .unwrap();
                refresh_view_content(hwnd, &room_view_id("!ops:example")).unwrap();
                assert_eq!(active_compose_target(hwnd), None);
            });
        }
    }

    #[test]
    fn react_menu_prefill_does_not_submit_a_command() {
        let mut state = test_state();
        state.active_view = room_view_id("!room:example");
        state.selected_timeline_event = Some(SelectedTimelineEvent {
            view_id: state.active_view.clone(),
            event_id: "$event-id".to_owned(),
        });

        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_active_timeline_event(hwnd, "$event-id");
                prefill_react_to_selected_message(hwnd).unwrap();
                assert_eq!(dump_compose_text(hwnd).unwrap(), "/react $event-id ");
                assert!(state_ref(hwnd).recorded_commands.is_empty());
            });
        }
    }

    #[test]
    fn redact_menu_submission_matches_slash_command_submission() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");
        state.selected_timeline_event = Some(SelectedTimelineEvent {
            view_id: state.active_view.clone(),
            event_id: "$event-id".to_owned(),
        });

        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_active_timeline_event(hwnd, "$event-id");
                submit_redact_selected_message(hwnd).unwrap();
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "Redaction queued for $event-id"
                );
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![Command::RedactEvent(RedactEventRequest {
                        room_id: "!room:example".to_owned(),
                        event_id: "$event-id".to_owned(),
                    })]
                );
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn submit_compose_enqueues_text_and_emote_messages_with_exact_requests() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!selected:example", "Selected", false)];
        state.model.selected_room = Some("!selected:example".to_owned());
        state.active_view = ViewId::Room("!selected:example".to_owned());

        let selected_room_id =
            submit_compose(&mut state, RoomMessagePresentationKind::Text, "hello", None).unwrap();
        assert_eq!(selected_room_id, "!selected:example");
        let emote_room_id = submit_compose(
            &mut state,
            RoomMessagePresentationKind::Emote,
            "waves",
            None,
        )
        .unwrap();
        assert_eq!(emote_room_id, "!selected:example");
        assert_eq!(
            state.recorded_commands,
            vec![
                Command::SendMessage(SendMessageRequest {
                    room_id: "!selected:example".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello".to_owned(),
                    styles: Vec::new(),
                    thread: None,
                    reply: None,
                }),
                Command::SendMessage(SendMessageRequest {
                    room_id: "!selected:example".to_owned(),
                    presentation: RoomMessagePresentationKind::Emote,
                    body: "waves".to_owned(),
                    styles: Vec::new(),
                    thread: None,
                    reply: None,
                }),
            ]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_compose_sends_a_literal_slash_body() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!beta:example", "Beta", false)];
        state.model.selected_room = Some("!beta:example".to_owned());
        state.active_view = ViewId::Room("!beta:example".to_owned());

        let room_id = submit_compose(
            &mut state,
            RoomMessagePresentationKind::Text,
            "/status",
            None,
        )
        .unwrap();
        assert_eq!(room_id, "!beta:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SendMessage(SendMessageRequest {
                room_id: "!beta:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "/status".to_owned(),
                styles: Vec::new(),
                thread: None,
                reply: None,
            })]
        );
        let dump = dump_model_from_state(&state);
        assert!(dump.submitted_reaction_commands.is_empty());
        assert!(dump.submitted_redaction_commands.is_empty());

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_compose_uses_active_thread_view_for_text_and_emote_messages() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root:example",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$thread-1:example",
                None,
                Some(thread_relation("$thread-root:example")),
                RoomMessagePresentationKind::Text,
                "durable reply",
                1_700_000_120_000,
            ),
        )
        .unwrap();
        state.active_view = thread_view_id("!ops:example", "$thread-root:example");
        state.room_activity.insert(
            thread_view_id("!ops:example", "$thread-root:example"),
            ViewActivityLevel::Message,
        );
        state.room_activity_read_targets.insert(
            thread_view_id("!ops:example", "$thread-root:example"),
            "$thread-1:example".to_owned(),
        );

        let room_id =
            submit_compose(&mut state, RoomMessagePresentationKind::Text, "hello", None).unwrap();
        assert_eq!(room_id, "!ops:example");
        assert!(
            !state
                .room_activity
                .contains_key(&thread_view_id("!ops:example", "$thread-root:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!ops:example", "$thread-root:example"))
        );
        let emote_room_id = submit_compose(
            &mut state,
            RoomMessagePresentationKind::Emote,
            "waves",
            None,
        )
        .unwrap();
        assert_eq!(emote_room_id, "!ops:example");
        assert_eq!(
            state.recorded_commands,
            vec![
                Command::SendMessage(SendMessageRequest {
                    room_id: "!ops:example".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello".to_owned(),
                    styles: Vec::new(),
                    thread: Some(SendMessageThreadTarget {
                        thread_root_event_id: "$thread-root:example".to_owned(),
                        latest_event_id: "$thread-1:example".to_owned(),
                    }),
                    reply: None,
                }),
                Command::SendMessage(SendMessageRequest {
                    room_id: "!ops:example".to_owned(),
                    presentation: RoomMessagePresentationKind::Emote,
                    body: "waves".to_owned(),
                    styles: Vec::new(),
                    thread: Some(SendMessageThreadTarget {
                        thread_root_event_id: "$thread-root:example".to_owned(),
                        latest_event_id: "$thread-1:example".to_owned(),
                    }),
                    reply: None,
                }),
            ]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_compose_with_explicit_room_id_ignores_active_thread_view() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$thread-1:example",
                None,
                Some(thread_relation("$thread-root:example")),
                RoomMessagePresentationKind::Text,
                "durable reply",
                1_700_000_120_000,
            ),
        )
        .unwrap();
        state.active_view = ViewId::Thread {
            room_id: "!ops:example".to_owned(),
            thread_root_event_id: "$thread-root:example".to_owned(),
        };

        let room_id = submit_compose(
            &mut state,
            RoomMessagePresentationKind::Text,
            "hello",
            Some("!ops:example"),
        )
        .unwrap();
        assert_eq!(room_id, "!ops:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SendMessage(SendMessageRequest {
                room_id: "!ops:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "hello".to_owned(),
                styles: Vec::new(),
                thread: None,
                reply: None,
            })]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_compose_rejects_text_and_emote_for_empty_body_invalid_room_and_missing_active_view() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!room:example", "Room", false),
            invited_room("!invite:example", "Invite"),
            space("!space:example", "Space"),
        ];

        assert_eq!(
            submit_compose(
                &mut state,
                RoomMessagePresentationKind::Text,
                "",
                Some("!room:example"),
            ),
            Err("message body must not be empty".to_owned())
        );
        state.model.selected_room = Some("!room:example".to_owned());
        assert_eq!(
            submit_compose(
                &mut state,
                RoomMessagePresentationKind::Emote,
                "waves",
                Some("!missing:example"),
            ),
            Err("unknown room id: !missing:example".to_owned())
        );
        assert_eq!(
            submit_compose(
                &mut state,
                RoomMessagePresentationKind::Emote,
                "waves",
                Some("!invite:example"),
            ),
            Err("room is not joined: !invite:example".to_owned())
        );
        assert_eq!(
            submit_compose(
                &mut state,
                RoomMessagePresentationKind::Emote,
                "waves",
                Some("!space:example"),
            ),
            Err("unknown room id: !space:example".to_owned())
        );
        assert_eq!(
            submit_compose(
                &mut state,
                RoomMessagePresentationKind::Emote,
                "waves",
                None,
            ),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_compose_requires_available_command_sender() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.model.selected_room = Some("!room:example".to_owned());
        state.active_view = ViewId::Room("!room:example".to_owned());

        assert_eq!(
            submit_compose(&mut state, RoomMessagePresentationKind::Text, "hello", None,),
            Err("core command sender is not available".to_owned())
        );
    }

    #[test]
    fn compose_history_navigation_is_noop_when_empty() {
        let mut history = ComposeHistory::default();

        assert_eq!(history.previous("draft".to_owned()), None);
        assert_eq!(history.next(), None);
        assert!(history.navigation.is_none());
    }

    #[test]
    fn compose_history_navigates_entries_and_restores_draft() {
        let mut history = ComposeHistory {
            entries: vec!["first".to_owned(), "second".to_owned()],
            navigation: None,
        };

        assert_eq!(history.previous("draft".to_owned()), Some("second"));
        assert_eq!(history.previous("ignored".to_owned()), Some("first"));
        assert_eq!(history.previous("ignored".to_owned()), Some("first"));
        assert_eq!(history.next(), Some("second".to_owned()));
        assert_eq!(history.next(), Some("draft".to_owned()));
        assert_eq!(history.next(), None);
        assert!(history.navigation.is_none());
    }

    #[test]
    fn compose_history_reset_navigation_restarts_from_newest_with_fresh_draft() {
        let mut history = ComposeHistory {
            entries: vec!["first".to_owned(), "second".to_owned()],
            navigation: None,
        };

        assert_eq!(history.previous("draft".to_owned()), Some("second"));
        history.reset_navigation();
        assert!(history.navigation.is_none());
        assert_eq!(
            history.previous("replacement draft".to_owned()),
            Some("second")
        );
        assert_eq!(history.next(), Some("replacement draft".to_owned()));
    }
}
