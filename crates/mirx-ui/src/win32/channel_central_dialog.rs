//! The Channel Central window (`/channel`, a double-click in a channel
//! window, Room > Channel Central...): a room's name, Topic History, Bans
//! List and modes, OK and Cancel. The form and its request are
//! `channel_central`'s; this module shows them.
//!
//! It opens greyed and asks the core for the room's settings; the answer
//! (or the error) comes while its modal loop runs, through
//! `take_core_answer`, and fills and enables the controls the user's power
//! level allows.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    BST_CHECKED, BST_INDETERMINATE, BST_UNCHECKED, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW,
    LVM_INSERTCOLUMNW, LVM_SETEXTENDEDLISTVIEWSTYLE, LVN_ITEMCHANGED, LVS_EX_FULLROWSELECT,
    LVS_REPORT, LVS_SHOWSELALWAYS, LVS_SINGLESEL, NMHDR,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BM_SETSTYLE, BS_AUTO3STATE, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON,
    BS_PUSHBUTTON, CB_ADDSTRING, CB_GETCOUNT, CB_GETLBTEXT, CB_GETLBTEXTLEN, CB_RESETCONTENT,
    CBS_AUTOHSCROLL, CBS_DROPDOWN, DefWindowProcW, ES_AUTOHSCROLL, IDCANCEL, IDOK, MoveWindow,
    SendMessageW, SetWindowTextW, WM_CLOSE, WM_COMMAND, WM_NCCREATE, WM_NCDESTROY, WM_NOTIFY,
    WNDPROC, WS_CHILD, WS_EX_CLIENTEDGE, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use mirx_core::{
    Command, Event, Operation, RoomActivityKind, RoomMemberSummary, RoomStateNoticeContent,
    UpdateRoomSettingsRequest,
};

use super::channel_central::{
    ChannelCentralEdits, ChannelCentralForm, READING_TEXT, invite_only, status_text, topic_history,
};
use super::dialogs::{DialogMetrics, create_modal_dialog, dialog_text_size, end_modal_dialog};
use super::dpi::Dpi;
use super::list_view::{LIST_VIEW_CLASS_NAME, dump_list, fill_list, selected_row};
use super::room_model::room_by_id;
use super::timestamp::LocalTimeFn;
use super::window_data::{self, WindowData};
use super::{
    BUTTON_CLASS_NAME, COMBOBOX_CLASS_NAME, EDIT_CLASS_NAME, STATIC_CLASS_NAME,
    create_child_window, enqueue_command, read_window_text, run_modal_dialog,
    set_local_command_status, state_mut, state_ref, wide_null,
};
use crate::automation::{UiChannelCentral, UiCheckBox, UiCheckState};

pub(super) const CHANNEL_CENTRAL_DIALOG_CLASS_NAME: &str = "mirxWin32ChannelCentralDialog";

pub(super) const CHANNEL_CENTRAL_NAME_ID: u16 = 7601;
pub(super) const CHANNEL_CENTRAL_TOPIC_ID: u16 = 7602;
pub(super) const CHANNEL_CENTRAL_BANS_ID: u16 = 7603;
pub(super) const CHANNEL_CENTRAL_REMOVE_ID: u16 = 7604;
pub(super) const CHANNEL_CENTRAL_TOPIC_BY_OPERATORS_ID: u16 = 7605;
pub(super) const CHANNEL_CENTRAL_INVITE_ONLY_ID: u16 = 7606;
pub(super) const CHANNEL_CENTRAL_MODERATED_ID: u16 = 7607;
pub(super) const CHANNEL_CENTRAL_PRIVATE_ID: u16 = 7608;
const CHANNEL_CENTRAL_STATUS_ID: u16 = 7609;

const NAME_LABEL: &str = "Name:";
const TOPIC_LABEL: &str = "Topic History:";
const BANS_LABEL: &str = "Bans List:";

// The layout's lengths at 96 dpi.
const LIST_WIDTH: i32 = 420;
const LIST_HEIGHT: i32 = 110;
/// The Topic History's dropped list.
const TOPIC_LIST_HEIGHT: i32 = 120;
const USER_COLUMN_WIDTH: i32 = 160;
const SET_BY_COLUMN_WIDTH: i32 = 80;
const DATE_COLUMN_WIDTH: i32 = 100;

struct Controls {
    name: HWND,
    topic: HWND,
    bans: HWND,
    remove: HWND,
    topic_by_operators: HWND,
    invite_only: HWND,
    moderated: HWND,
    private: HWND,
    status: HWND,
    ok: HWND,
}

impl Controls {
    /// What stays greyed until the settings are in.
    fn settings_controls(&self) -> [HWND; 9] {
        [
            self.name,
            self.topic,
            self.bans,
            self.remove,
            self.topic_by_operators,
            self.invite_only,
            self.moderated,
            self.private,
            self.ok,
        ]
    }
}

/// The dialog's state, owned by `run_channel_central_dialog` and lent to
/// the window procedure while the window exists.
struct DialogState {
    room_id: String,
    /// The topics the room's window saw set, newest first.
    window_topics: Vec<String>,
    /// The room's members, for the banners' nicks.
    members: Vec<RoomMemberSummary>,
    local_time: LocalTimeFn,
    controls: Controls,
    /// `None` until the settings are in.
    form: Option<ChannelCentralForm>,
    /// What OK sends.
    request: Option<UpdateRoomSettingsRequest>,
}

unsafe fn dialog_state(hwnd: HWND) -> &'static mut DialogState {
    window_data::get(hwnd).expect("channel central has its state")
}

/// Shows the joined room's settings until OK or Cancel. OK sends what
/// changed, in one request whose echo prints the lines.
pub(super) unsafe fn run_channel_central_dialog(hwnd: HWND, room_id: String) -> Result<(), String> {
    let state = state_ref(hwnd);
    let room =
        room_by_id(&state.model.rooms, &room_id).expect("channel central opens for a known room");
    let caption = format!("Channel Central - {}", room.display_name);
    let room_view = state.model.room_views.get(&room_id);
    let window_topics = room_view
        .map(|view| {
            view.activities
                .iter()
                .rev()
                .filter_map(|activity| match &activity.kind {
                    RoomActivityKind::StateNotice(notice) => match &notice.content {
                        RoomStateNoticeContent::Topic { topic } => Some(topic.clone()),
                        _ => None,
                    },
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let members = room_view
        .map(|view| view.members.clone())
        .unwrap_or_default();
    let local_time = state.local_time;
    enqueue_command(state_mut(hwnd), Command::FetchRoomSettings(room_id.clone()))?;
    let mut dialog_state = Box::new(WindowData::new(DialogState {
        room_id,
        window_topics,
        members,
        local_time,
        controls: Controls {
            name: null_mut(),
            topic: null_mut(),
            bans: null_mut(),
            remove: null_mut(),
            topic_by_operators: null_mut(),
            invite_only: null_mut(),
            moderated: null_mut(),
            private: null_mut(),
            status: null_mut(),
            ok: null_mut(),
        },
        form: None,
        request: None,
    }));
    let dialog = create_modal_dialog(
        hwnd,
        CHANNEL_CENTRAL_DIALOG_CLASS_NAME,
        &caption,
        (&mut *dialog_state as *mut WindowData<DialogState>).cast(),
        |dialog| create_controls(dialog),
    )?;
    state_mut(hwnd).channel_central_dialog = Some(dialog);
    run_modal_dialog(hwnd, dialog);
    state_mut(hwnd).channel_central_dialog = None;
    let Some(request) = dialog_state.request.take() else {
        return Ok(());
    };
    enqueue_command(state_mut(hwnd), Command::UpdateRoomSettings(request))?;
    set_local_command_status(hwnd, "room settings sent".to_owned())
}

unsafe fn create_controls(dialog: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let dpi = Dpi::of_window(dialog);
    let DialogMetrics {
        margin,
        gap,
        label_gap,
        button_width,
        button_height,
        edit_height,
        checkbox_height,
    } = DialogMetrics::at(dpi);
    let control = |class: &str, text: &str, style: u32, ex_style: u32, id: u16| {
        create_child_window(
            dialog,
            class,
            text,
            WS_CHILD | WS_VISIBLE | style,
            ex_style,
            id,
            instance,
            null_mut(),
        )
    };
    let place = |control: HWND, x: i32, y: i32, width: i32, height: i32| {
        MoveWindow(control, x, y, width, height, 0);
        control
    };
    let state = dialog_state(dialog);
    let (list_width, list_height) = (dpi.px(LIST_WIDTH), dpi.px(LIST_HEIGHT));
    let content_width = list_width + gap + button_width;
    let (label_width, label_height) = [NAME_LABEL, TOPIC_LABEL]
        .map(|label| dialog_text_size(label, content_width))
        .into_iter()
        .fold((0, 0), |(width, height), (w, h)| {
            (width.max(w), height.max(h))
        });
    let field_x = margin + label_width + gap;
    let field_width = margin + content_width - field_x;
    // A label sits level with its field's text.
    let label_offset = (edit_height - label_height) / 2;
    let label = |text: &str, y: i32| {
        control(STATIC_CLASS_NAME, text, 0, 0, 0)
            .map(|label| place(label, margin, y, label_width, label_height))
    };

    let mut y = margin;
    label(NAME_LABEL, y + label_offset)?;
    state.controls.name = place(
        control(
            EDIT_CLASS_NAME,
            "",
            WS_TABSTOP | ES_AUTOHSCROLL as u32,
            WS_EX_CLIENTEDGE,
            CHANNEL_CENTRAL_NAME_ID,
        )?,
        field_x,
        y,
        field_width,
        edit_height,
    );
    y += edit_height + gap;
    label(TOPIC_LABEL, y + label_offset)?;
    // A combo box's height includes its dropped list.
    state.controls.topic = place(
        control(
            COMBOBOX_CLASS_NAME,
            "",
            WS_TABSTOP | WS_VSCROLL | (CBS_DROPDOWN | CBS_AUTOHSCROLL) as u32,
            0,
            CHANNEL_CENTRAL_TOPIC_ID,
        )?,
        field_x,
        y,
        field_width,
        edit_height + dpi.px(TOPIC_LIST_HEIGHT),
    );
    y += edit_height + gap;

    let (_, bans_label_height) = dialog_text_size(BANS_LABEL, content_width);
    label(BANS_LABEL, y)?;
    y += bans_label_height + label_gap;
    state.controls.bans = place(
        control(
            LIST_VIEW_CLASS_NAME,
            "",
            WS_TABSTOP | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS,
            WS_EX_CLIENTEDGE,
            CHANNEL_CENTRAL_BANS_ID,
        )?,
        margin,
        y,
        list_width,
        list_height,
    );
    SendMessageW(
        state.controls.bans,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        LVS_EX_FULLROWSELECT as WPARAM,
        LVS_EX_FULLROWSELECT as LPARAM,
    );
    // The list's inside, less its sunken edges and a scroll bar.
    let inner_width = list_width - dpi.px(24);
    let (user, set_by, date) = (
        dpi.px(USER_COLUMN_WIDTH),
        dpi.px(SET_BY_COLUMN_WIDTH),
        dpi.px(DATE_COLUMN_WIDTH),
    );
    for (index, (title, width)) in [
        ("User", user),
        ("Set by", set_by),
        ("Date", date),
        ("Reason", inner_width - user - set_by - date),
    ]
    .into_iter()
    .enumerate()
    {
        let mut title = wide_null(title);
        let column = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH,
            cx: width,
            pszText: title.as_mut_ptr(),
            ..LVCOLUMNW::default()
        };
        if SendMessageW(
            state.controls.bans,
            LVM_INSERTCOLUMNW,
            index,
            &column as *const LVCOLUMNW as LPARAM,
        ) != index as LRESULT
        {
            return Err(format!("LVM_INSERTCOLUMNW {index} failed"));
        }
    }
    state.controls.remove = place(
        control(
            BUTTON_CLASS_NAME,
            "Remove",
            WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            CHANNEL_CENTRAL_REMOVE_ID,
        )?,
        margin + list_width + gap,
        y,
        button_width,
        button_height,
    );
    y += list_height + gap;

    // The modes, two columns of check boxes.
    let column_width = content_width / 2;
    let check = |text: &str, id: u16, column: i32, row: i32| {
        control(
            BUTTON_CLASS_NAME,
            text,
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            0,
            id,
        )
        .map(|check| {
            place(
                check,
                margin + column * column_width,
                y + row * (checkbox_height + gap),
                column_width,
                checkbox_height,
            )
        })
    };
    state.controls.topic_by_operators = check(
        "Only ops set topic",
        CHANNEL_CENTRAL_TOPIC_BY_OPERATORS_ID,
        0,
        0,
    )?;
    state.controls.invite_only = check("Invite only", CHANNEL_CENTRAL_INVITE_ONLY_ID, 1, 0)?;
    state.controls.moderated = check("Moderated", CHANNEL_CENTRAL_MODERATED_ID, 0, 1)?;
    state.controls.private = check(
        "Private (not in the Rooms List)",
        CHANNEL_CENTRAL_PRIVATE_ID,
        1,
        1,
    )?;
    y += 2 * checkbox_height + 2 * gap;

    // The status: two lines, as an error may need.
    let (_, line_height) = dialog_text_size("Xg", content_width);
    state.controls.status = place(
        control(
            STATIC_CLASS_NAME,
            READING_TEXT,
            0,
            0,
            CHANNEL_CENTRAL_STATUS_ID,
        )?,
        margin,
        y,
        content_width,
        2 * line_height,
    );
    y += 2 * line_height + gap;

    // OK is `IDOK` and Cancel `IDCANCEL`, which `IsDialogMessageW` sends
    // for Enter and Esc.
    let cancel_x = margin + content_width - button_width;
    state.controls.ok = place(
        control(
            BUTTON_CLASS_NAME,
            "OK",
            WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
            0,
            IDOK as u16,
        )?,
        cancel_x - gap - button_width,
        y,
        button_width,
        button_height,
    );
    place(
        control(
            BUTTON_CLASS_NAME,
            "Cancel",
            WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            IDCANCEL as u16,
        )?,
        cancel_x,
        y,
        button_width,
        button_height,
    );

    for control in state.controls.settings_controls() {
        EnableWindow(control, 0);
    }
    Ok(())
}

/// The core's answer to the open dialog's read, if `event` is it: the
/// room's settings fill the controls, an error goes in the status line.
/// Everything else, another room's answer included, is not for it.
pub(super) unsafe fn take_core_answer(dialog: HWND, event: &Event) {
    let state = dialog_state(dialog);
    match event {
        Event::RoomSettings(settings) if settings.room_id == state.room_id => {
            let (form, edits) = match state.form.take() {
                None => {
                    let form = ChannelCentralForm::new(settings.clone());
                    let edits = form.loaded_edits();
                    (form, edits)
                }
                // A later answer (the room's lane answers its reads in
                // order, so it is the newest) keeps the user's edits.
                Some(form) => form.rebase(settings.clone(), &read_edits(&state.controls)),
            };
            fill(state, form, &edits);
        }
        Event::Error(error)
            if error.operation == Operation::FetchRoomSettings
                && error.room_id.as_deref() == Some(state.room_id.as_str()) =>
        {
            set_text(state.controls.status, &error.message);
        }
        _ => {}
    }
}

/// Shows `form`'s settings with `edits` in the controls, enabling what the
/// user may change.
unsafe fn fill(state: &mut DialogState, form: ChannelCentralForm, edits: &ChannelCentralEdits) {
    let controls = &state.controls;
    let settings = form.settings();
    let allowed = settings.allowed;

    set_text(controls.name, &edits.name);
    SendMessageW(controls.topic, CB_RESETCONTENT, 0, 0);
    let window_topics = state.window_topics.iter().map(String::as_str);
    for topic in topic_history(settings.topic.as_deref(), window_topics) {
        let topic = wide_null(&topic);
        SendMessageW(controls.topic, CB_ADDSTRING, 0, topic.as_ptr() as LPARAM);
    }
    set_text(controls.topic, &edits.topic);

    set_check(controls.topic_by_operators, Some(edits.topic_by_operators));
    // Indeterminate only for a rule neither state is.
    let invite_only_style = match invite_only(&settings.join_rule) {
        Some(_) => BS_AUTOCHECKBOX,
        None => BS_AUTO3STATE,
    };
    SendMessageW(
        controls.invite_only,
        BM_SETSTYLE,
        invite_only_style as WPARAM,
        1,
    );
    set_check(controls.invite_only, edits.invite_only);
    set_check(controls.moderated, Some(edits.moderated));
    set_check(controls.private, Some(edits.private));

    for (control, enabled) in [
        (controls.name, allowed.name),
        (controls.topic, allowed.topic),
        (controls.bans, true),
        (controls.topic_by_operators, allowed.power_levels),
        (controls.moderated, allowed.power_levels),
        (controls.invite_only, allowed.join_rule),
        (controls.private, allowed.directory),
        (controls.ok, true),
    ] {
        EnableWindow(control, enabled.into());
    }
    set_text(controls.status, status_text(&allowed));
    state.form = Some(form);
    refill_bans(state);
}

unsafe fn refill_bans(state: &DialogState) {
    let form = state
        .form
        .as_ref()
        .expect("the bans list shows loaded settings");
    let members = &state.members;
    let nick_of = |user_id: &str| {
        members
            .iter()
            .find(|member| member.user_id == user_id)
            .map(|member| member.nick.clone())
    };
    let rows = form
        .ban_rows(&nick_of, state.local_time)
        .into_iter()
        .map(Vec::from)
        .collect::<Vec<_>>();
    fill_list(state.controls.bans, &rows);
    grey_remove(state);
}

/// Remove lifts the selected ban, where the user may unban.
unsafe fn grey_remove(state: &DialogState) {
    let unban = state
        .form
        .as_ref()
        .is_some_and(|form| form.settings().allowed.unban);
    let selected = selected_row(state.controls.bans).is_some();
    EnableWindow(state.controls.remove, (unban && selected).into());
}

unsafe fn handle_remove(dialog: HWND) {
    let state = dialog_state(dialog);
    let row = selected_row(state.controls.bans).expect("Remove is greyed without a selection");
    state
        .form
        .as_mut()
        .expect("Remove is greyed until the settings are in")
        .remove_ban(row);
    refill_bans(state);
}

/// The controls as the user left them.
unsafe fn read_edits(controls: &Controls) -> ChannelCentralEdits {
    ChannelCentralEdits {
        name: read_window_text(controls.name),
        topic: read_window_text(controls.topic),
        topic_by_operators: check_state(controls.topic_by_operators) == Some(true),
        invite_only: check_state(controls.invite_only),
        moderated: check_state(controls.moderated) == Some(true),
        private: check_state(controls.private) == Some(true),
    }
}

unsafe fn handle_ok(dialog: HWND) {
    let state = dialog_state(dialog);
    let edits = read_edits(&state.controls);
    state.request = state
        .form
        .as_ref()
        .expect("OK is greyed until the settings are in")
        .request(&edits);
    end_modal_dialog(dialog);
}

unsafe fn set_text(control: HWND, text: &str) {
    let text = wide_null(text);
    SetWindowTextW(control, text.as_ptr());
}

/// `None` is the indeterminate state.
unsafe fn set_check(checkbox: HWND, checked: Option<bool>) {
    let check = match checked {
        Some(true) => BST_CHECKED,
        Some(false) => BST_UNCHECKED,
        None => BST_INDETERMINATE,
    };
    SendMessageW(checkbox, BM_SETCHECK, check as WPARAM, 0);
}

unsafe fn check_state(checkbox: HWND) -> Option<bool> {
    match SendMessageW(checkbox, BM_GETCHECK, 0, 0) as u32 {
        BST_CHECKED => Some(true),
        BST_UNCHECKED => Some(false),
        BST_INDETERMINATE => None,
        state => panic!("BM_GETCHECK answered {state}"),
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            window_data::borrow_create_param::<DialogState>(hwnd, lparam);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_COMMAND => match (wparam & 0xffff) as u16 {
            id if id == IDOK as u16 => {
                handle_ok(hwnd);
                0
            }
            id if id == IDCANCEL as u16 => {
                end_modal_dialog(hwnd);
                0
            }
            CHANNEL_CENTRAL_REMOVE_ID => {
                handle_remove(hwnd);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        },
        WM_NOTIFY => {
            let header = &*(lparam as *const NMHDR);
            if header.idFrom as u16 == CHANNEL_CENTRAL_BANS_ID && header.code == LVN_ITEMCHANGED {
                grey_remove(dialog_state(hwnd));
            }
            0
        }
        // The close box: Cancel.
        WM_CLOSE => {
            end_modal_dialog(hwnd);
            0
        }
        WM_NCDESTROY => {
            window_data::detach::<DialogState>(hwnd);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// Channel Central as automation sees it, read back from its controls.
pub(super) unsafe fn dump(dialog: HWND) -> UiChannelCentral {
    let state = dialog_state(dialog);
    let controls = &state.controls;
    let enabled = |control| IsWindowEnabled(control) != 0;
    let checkbox = |control| UiCheckBox {
        state: match check_state(control) {
            Some(true) => UiCheckState::Checked,
            Some(false) => UiCheckState::Unchecked,
            None => UiCheckState::Indeterminate,
        },
        enabled: enabled(control),
    };
    let topic_items = (0..SendMessageW(controls.topic, CB_GETCOUNT, 0, 0) as usize)
        .map(|index| {
            let length = SendMessageW(controls.topic, CB_GETLBTEXTLEN, index, 0) as usize;
            let mut buffer = vec![0u16; length + 1];
            SendMessageW(
                controls.topic,
                CB_GETLBTEXT,
                index,
                buffer.as_mut_ptr() as LPARAM,
            );
            String::from_utf16_lossy(&buffer[..length])
        })
        .collect();
    let (ban_rows, ban_row_click_points) = dump_list(controls.bans, 4);
    UiChannelCentral {
        caption: read_window_text(dialog),
        room_id: state.room_id.clone(),
        status: read_window_text(controls.status),
        name: read_window_text(controls.name),
        name_enabled: enabled(controls.name),
        topic: read_window_text(controls.topic),
        topic_items,
        topic_enabled: enabled(controls.topic),
        ban_rows,
        ban_row_click_points,
        remove_enabled: enabled(controls.remove),
        topic_by_operators: checkbox(controls.topic_by_operators),
        invite_only: checkbox(controls.invite_only),
        moderated: checkbox(controls.moderated),
        private: checkbox(controls.private),
        ok_enabled: enabled(controls.ok),
    }
}

/// The dialog's class and procedure, registered with the other dialogs'.
pub(super) const DIALOG_CLASS: (&str, WNDPROC) =
    (CHANNEL_CENTRAL_DIALOG_CLASS_NAME, Some(window_proc));
