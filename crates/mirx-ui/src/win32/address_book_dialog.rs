//! The Address Book window (Tools > Address Book..., Alt+B, `/abook`):
//! a tab control with mIRC's Notify and Control tabs, OK and Cancel. The
//! form and its checks are `address_book`'s; this module shows them.
//!
//! Every page control is a child of the dialog, not of the tab control,
//! so their notifications come here; the tab control is created first and
//! clips its siblings, so it never paints over them. A tab shows its
//! page's controls and hides the other's.
//!
//! Add and Edit open an entry dialog over the Address Book, modal to it.
//! Its OK checks the fields against a copy of the form taken when it
//! opened, and keeps it open with a message box when they do not do.

use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::rc::Rc;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    BST_CHECKED, BST_UNCHECKED, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIF_TEXT, LVIR_LABEL, LVITEMW,
    LVM_DELETEALLITEMS, LVM_GETITEMCOUNT, LVM_GETITEMRECT, LVM_GETITEMTEXTW, LVM_GETNEXTITEM,
    LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMTEXTW,
    LVN_ITEMCHANGED, LVNI_SELECTED, LVS_EX_FULLROWSELECT, LVS_NOCOLUMNHEADER, LVS_REPORT,
    LVS_SHOWSELALWAYS, LVS_SINGLESEL, NM_DBLCLK, NM_RETURN, NMHDR, TCIF_TEXT, TCITEMW,
    TCM_ADJUSTRECT, TCM_GETCURSEL, TCM_GETITEMRECT, TCM_INSERTITEMW, TCM_SETCURSEL, TCN_SELCHANGE,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, DM_GETDEFID,
    DefWindowProcW, EN_CHANGE, ES_READONLY, GWL_STYLE, GetDlgCtrlID, GetDlgItem, GetWindowLongPtrW,
    HWND_BOTTOM, IDCANCEL, IDOK, MB_ICONWARNING, MB_OK, MessageBoxW, MoveWindow, SW_HIDE, SW_SHOW,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SendMessageW, SetWindowPos, ShowWindow, WM_CLOSE,
    WM_COMMAND, WM_NCCREATE, WM_NCDESTROY, WM_NOTIFY, WM_SETFONT, WNDPROC, WS_CHILD,
    WS_CLIPSIBLINGS, WS_EX_CLIENTEDGE, WS_TABSTOP, WS_VISIBLE,
};

use mirx_core::Command;

use super::address_book::{AddressBookForm, AddressBookTab, EntryFields, IgnoreEdit, People};
use super::dialogs::{
    DialogColumn, DialogMetrics, create_modal_dialog, default_button, dialog_text_size,
    end_modal_dialog,
};
use super::dpi::Dpi;
use super::ignore::user_label;
use super::notify::NotifySetting;
use super::room_list_window::{rect_center, wide_buffer_text};
use super::window_data::{self, WindowData};
use super::{
    BUTTON_CLASS_NAME, STATIC_CLASS_NAME, announce_notify_changes, create_child_window, echo_view,
    enqueue_command, read_window_text, refresh_view_content, run_modal_dialog,
    set_local_command_status, state_mut, state_ref, wide_null,
};
use crate::automation::{UiAddressBook, UiPoint};

pub(super) const ADDRESS_BOOK_DIALOG_CLASS_NAME: &str = "mirxWin32AddressBookDialog";
pub(super) const ADDRESS_ENTRY_DIALOG_CLASS_NAME: &str = "mirxWin32AddressEntryDialog";
const ADDRESS_BOOK_TITLE: &str = "Address Book";
const TAB_CONTROL_CLASS_NAME: &str = "SysTabControl32";
const LIST_VIEW_CLASS_NAME: &str = "SysListView32";

pub(super) const ADDRESS_BOOK_TAB_ID: u16 = 7001;
pub(super) const ADDRESS_BOOK_NOTIFY_LIST_ID: u16 = 7101;
pub(super) const ADDRESS_BOOK_NOTIFY_ADD_ID: u16 = 7102;
pub(super) const ADDRESS_BOOK_NOTIFY_EDIT_ID: u16 = 7103;
pub(super) const ADDRESS_BOOK_NOTIFY_DELETE_ID: u16 = 7104;
pub(super) const ADDRESS_BOOK_NOTIFY_ACTIVE_ID: u16 = 7105;
pub(super) const ADDRESS_BOOK_CONTROL_LABEL_ID: u16 = 7201;
pub(super) const ADDRESS_BOOK_CONTROL_LIST_ID: u16 = 7202;
pub(super) const ADDRESS_BOOK_CONTROL_ADD_ID: u16 = 7203;
pub(super) const ADDRESS_BOOK_CONTROL_DELETE_ID: u16 = 7204;
pub(super) const ADDRESS_BOOK_CONTROL_NOTE_ID: u16 = 7205;
pub(super) const ADDRESS_ENTRY_NICKNAME_ID: u16 = 7301;
pub(super) const ADDRESS_ENTRY_NOTE_ID: u16 = 7302;
pub(super) const ADDRESS_ENTRY_WHOIS_ID: u16 = 7303;
pub(super) const ADDRESS_ENTRY_OK_ID: usize = 7304;
pub(super) const ADDRESS_ENTRY_CANCEL_ID: usize = 7305;

const CONTROL_LABEL: &str = "Ignore these users:";
const CONTROL_NOTE: &str = "Your ignore list is kept on your Matrix server and shared with \
your other clients. The server stops sending an ignored user's messages and invites.";
const CONTROL_SIGNED_OUT_NOTE: &str = "Connect to edit the ignore list.";

// The layout's lengths at 96 dpi.
const LIST_WIDTH: i32 = 320;
const LIST_HEIGHT: i32 = 150;
const NICKNAME_COLUMN_WIDTH: i32 = 200;
const ENTRY_WIDTH: i32 = 260;
/// What `IsDialogMessageW` sends for Esc.
const ESC_CANCEL_ID: usize = IDCANCEL as usize;

/// A page's controls.
struct NotifyPage {
    list: HWND,
    add: HWND,
    edit: HWND,
    delete: HWND,
    active: HWND,
}

struct ControlPage {
    label: HWND,
    list: HWND,
    add: HWND,
    delete: HWND,
    note: HWND,
}

/// The Address Book's state, owned by `run_address_book_dialog` and lent
/// to the window procedure while the window exists.
struct DialogState {
    form: AddressBookForm,
    tab_control: HWND,
    notify: NotifyPage,
    control: ControlPage,
    /// The form OK accepted.
    saved: Option<AddressBookForm>,
}

impl DialogState {
    fn page_controls(&self) -> [(AddressBookTab, HWND); 10] {
        let NotifyPage {
            list,
            add,
            edit,
            delete,
            active,
        } = self.notify;
        let notify = AddressBookTab::Notify;
        let ControlPage {
            label,
            list: control_list,
            add: control_add,
            delete: control_delete,
            note,
        } = self.control;
        let control = AddressBookTab::Control;
        [
            (notify, list),
            (notify, add),
            (notify, edit),
            (notify, delete),
            (notify, active),
            (control, label),
            (control, control_list),
            (control, control_add),
            (control, control_delete),
            (control, note),
        ]
    }
}

unsafe fn dialog_state(hwnd: HWND) -> &'static mut DialogState {
    window_data::get(hwnd).expect("the address book has its state")
}

/// Tools > Address Book... and `/abook`: shows the notify list and the
/// ignore list at `tab` until OK or Cancel. OK saves the notify list and
/// its option at once and sends the ignore list's changes to the server.
pub(super) unsafe fn run_address_book_dialog(
    hwnd: HWND,
    tab: AddressBookTab,
) -> Result<(), String> {
    let state = state_ref(hwnd);
    let model = &state.model;
    let form = AddressBookForm {
        notify: state.settings.settings().notify.clone(),
        ignore: model.ignored_users.clone().map(IgnoreEdit::new),
        people: People::new(
            model
                .room_views
                .values()
                .flat_map(|view| view.members.iter().cloned())
                .collect(),
            model.own_user_id().map(str::to_owned),
        ),
    };
    let mut dialog_state = Box::new(WindowData::new(DialogState {
        form,
        tab_control: null_mut(),
        notify: NotifyPage {
            list: null_mut(),
            add: null_mut(),
            edit: null_mut(),
            delete: null_mut(),
            active: null_mut(),
        },
        control: ControlPage {
            label: null_mut(),
            list: null_mut(),
            add: null_mut(),
            delete: null_mut(),
            note: null_mut(),
        },
        saved: None,
    }));
    let dialog = create_modal_dialog(
        hwnd,
        ADDRESS_BOOK_DIALOG_CLASS_NAME,
        ADDRESS_BOOK_TITLE,
        (&mut *dialog_state as *mut WindowData<DialogState>).cast(),
        |dialog| create_controls(dialog),
    )?;
    show_tab(dialog, tab);
    state_mut(hwnd).address_book_dialog = Some(dialog);
    run_modal_dialog(hwnd, dialog);
    state_mut(hwnd).address_book_dialog = None;
    let Some(form) = dialog_state.saved.take() else {
        return Ok(());
    };
    apply(hwnd, form)
}

/// What OK does with the form: the notify list and its option are saved,
/// and a user listed while on is news, as with `/notify`; the ignore
/// list's changes go to the server, whose echo prints them.
unsafe fn apply(hwnd: HWND, form: AddressBookForm) -> Result<(), String> {
    let AddressBookForm { notify, ignore, .. } = form;
    let state = state_mut(hwnd);
    state
        .settings
        .update(|settings| settings.notify = notify.clone())?;
    announce_notify_changes(state)?;
    let view = echo_view(&state.active_view);
    refresh_view_content(hwnd, &view)?;
    let changes = ignore.map(|edit| edit.changes()).unwrap_or_default();
    let commands = changes
        .ignore
        .into_iter()
        .map(Command::IgnoreUser)
        .chain(changes.unignore.into_iter().map(Command::UnignoreUser))
        .collect::<Vec<_>>();
    let status = if commands.is_empty() {
        "address book saved"
    } else {
        "address book saved; ignore list sent to the server"
    };
    for command in commands {
        enqueue_command(state_mut(hwnd), command)?;
    }
    set_local_command_status(hwnd, status.to_owned())
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
        checkbox_height,
        ..
    } = DialogMetrics::at(dpi);
    let control = |class: &str, text: &str, style: u32, ex_style: u32, id: u16| {
        create_child_window(
            dialog,
            class,
            text,
            WS_CHILD | style,
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
    let page_width = list_width + gap + button_width;
    let (_, label_height) = dialog_text_size(CONTROL_LABEL, page_width);
    let (_, note_height) = dialog_text_size(CONTROL_NOTE, page_width);
    let page_height = (list_height + gap + checkbox_height)
        .max(label_height + label_gap + list_height + gap + note_height);

    // The tab control first: the pages are above it.
    state.tab_control = control(
        TAB_CONTROL_CLASS_NAME,
        "",
        WS_VISIBLE | WS_TABSTOP | WS_CLIPSIBLINGS,
        0,
        ADDRESS_BOOK_TAB_ID,
    )?;
    for tab in AddressBookTab::ALL {
        let mut label = wide_null(tab.label());
        let item = TCITEMW {
            mask: TCIF_TEXT,
            pszText: label.as_mut_ptr(),
            ..TCITEMW::default()
        };
        if SendMessageW(
            state.tab_control,
            TCM_INSERTITEMW,
            tab.index(),
            &item as *const TCITEMW as LPARAM,
        ) != tab.index() as LRESULT
        {
            return Err(format!("TCM_INSERTITEMW {} failed", tab.label()));
        }
    }
    // The tabs' row is as tall as the dialog font's: set it before the
    // control says how much room its border and tabs take.
    SendMessageW(
        state.tab_control,
        WM_SETFONT,
        GetStockObject(DEFAULT_GUI_FONT) as WPARAM,
        0,
    );
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: page_width + 2 * gap,
        bottom: page_height + 2 * gap,
    };
    SendMessageW(
        state.tab_control,
        TCM_ADJUSTRECT,
        1,
        &mut rect as *mut RECT as LPARAM,
    );
    place(
        state.tab_control,
        margin,
        margin,
        rect.right - rect.left,
        rect.bottom - rect.top,
    );
    // The pages' backdrop: the tab control's body is lighter than the
    // dialog's face under a theme, and the labels and check box on it
    // paint the face; over the backdrop, every page control is on the
    // face. Above the tab control, below the page controls (see the end).
    let backdrop = place(
        control(STATIC_CLASS_NAME, "", WS_VISIBLE | WS_CLIPSIBLINGS, 0, 0)?,
        margin - rect.left,
        margin - rect.top,
        page_width + 2 * gap,
        page_height + 2 * gap,
    );
    let (page_x, page_y) = (margin - rect.left + gap, margin - rect.top + gap);
    let buttons_x = page_x + list_width + gap;
    let button = |text: &str, id: u16, row: i32, y: i32| {
        Ok::<_, String>(place(
            control(
                BUTTON_CLASS_NAME,
                text,
                WS_TABSTOP | BS_PUSHBUTTON as u32,
                0,
                id,
            )?,
            buttons_x,
            y + row * (button_height + gap),
            button_width,
            button_height,
        ))
    };
    let list = |id: u16, y: i32, columns: &[(&str, i32)], header: bool| {
        let header_style = if header { 0 } else { LVS_NOCOLUMNHEADER };
        let list = place(
            control(
                LIST_VIEW_CLASS_NAME,
                "",
                WS_TABSTOP | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS | header_style,
                WS_EX_CLIENTEDGE,
                id,
            )?,
            page_x,
            y,
            list_width,
            list_height,
        );
        SendMessageW(
            list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            LVS_EX_FULLROWSELECT as WPARAM,
            LVS_EX_FULLROWSELECT as LPARAM,
        );
        for (index, (title, width)) in columns.iter().enumerate() {
            let mut title = wide_null(title);
            let column = LVCOLUMNW {
                mask: LVCF_TEXT | LVCF_WIDTH,
                cx: *width,
                pszText: title.as_mut_ptr(),
                ..LVCOLUMNW::default()
            };
            if SendMessageW(
                list,
                LVM_INSERTCOLUMNW,
                index,
                &column as *const LVCOLUMNW as LPARAM,
            ) != index as LRESULT
            {
                return Err(format!("LVM_INSERTCOLUMNW {} failed", title.len()));
            }
        }
        Ok::<_, String>(list)
    };
    // The list's inside, less its sunken edges and a scroll bar.
    let inner_width = list_width - dpi.px(24);
    let nickname_width = dpi.px(NICKNAME_COLUMN_WIDTH);

    // Notify: the list, its buttons beside it, the option under it.
    state.notify.list = list(
        ADDRESS_BOOK_NOTIFY_LIST_ID,
        page_y,
        &[
            ("Nickname", nickname_width),
            ("Note", inner_width - nickname_width),
        ],
        true,
    )?;
    state.notify.add = button("Add...", ADDRESS_BOOK_NOTIFY_ADD_ID, 0, page_y)?;
    state.notify.edit = button("Edit...", ADDRESS_BOOK_NOTIFY_EDIT_ID, 1, page_y)?;
    state.notify.delete = button("Delete", ADDRESS_BOOK_NOTIFY_DELETE_ID, 2, page_y)?;
    state.notify.active = place(
        control(
            BUTTON_CLASS_NAME,
            "Show notifies in active window",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            0,
            ADDRESS_BOOK_NOTIFY_ACTIVE_ID,
        )?,
        page_x,
        page_y + list_height + gap,
        page_width,
        checkbox_height,
    );

    // Control: the ignore list under its label, the note under it.
    state.control.label = place(
        control(
            STATIC_CLASS_NAME,
            CONTROL_LABEL,
            0,
            0,
            ADDRESS_BOOK_CONTROL_LABEL_ID,
        )?,
        page_x,
        page_y,
        page_width,
        label_height,
    );
    let control_list_y = page_y + label_height + label_gap;
    state.control.list = list(
        ADDRESS_BOOK_CONTROL_LIST_ID,
        control_list_y,
        &[("User", inner_width)],
        false,
    )?;
    state.control.add = button("Add...", ADDRESS_BOOK_CONTROL_ADD_ID, 0, control_list_y)?;
    state.control.delete = button("Delete", ADDRESS_BOOK_CONTROL_DELETE_ID, 1, control_list_y)?;
    let known = state.form.ignore.is_some();
    state.control.note = place(
        control(
            STATIC_CLASS_NAME,
            if known {
                CONTROL_NOTE
            } else {
                CONTROL_SIGNED_OUT_NOTE
            },
            0,
            0,
            ADDRESS_BOOK_CONTROL_NOTE_ID,
        )?,
        page_x,
        control_list_y + list_height + gap,
        page_width,
        note_height,
    );
    for control in [state.control.list, state.control.add] {
        EnableWindow(control, known.into());
    }

    // OK is `IDOK` and Cancel `IDCANCEL`, which `IsDialogMessageW` sends
    // for Enter and Esc, right-aligned a margin under the tabs.
    let buttons_y = margin + (rect.bottom - rect.top) + margin;
    let cancel_x = margin + (rect.right - rect.left) - button_width;
    place(
        control(
            BUTTON_CLASS_NAME,
            "OK",
            WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
            0,
            IDOK as u16,
        )?,
        cancel_x - gap - button_width,
        buttons_y,
        button_width,
        button_height,
    );
    place(
        control(
            BUTTON_CLASS_NAME,
            "Cancel",
            WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            IDCANCEL as u16,
        )?,
        cancel_x,
        buttons_y,
        button_width,
        button_height,
    );

    // A child is created below its siblings: the tab control and the
    // backdrop go to the bottom, the tab control lowest. Both clip their
    // siblings, so neither paints over a page control.
    for window in [backdrop, state.tab_control] {
        SetWindowPos(
            window,
            HWND_BOTTOM,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }

    let check = if state.form.notify.active {
        BST_CHECKED
    } else {
        BST_UNCHECKED
    };
    SendMessageW(state.notify.active, BM_SETCHECK, check as WPARAM, 0);
    refill_notify_list(state);
    refill_control_list(state);
    Ok(())
}

/// Shows `tab`'s page and hides the other's, as a click on the tab does.
unsafe fn show_tab(dialog: HWND, tab: AddressBookTab) {
    let state = dialog_state(dialog);
    SendMessageW(state.tab_control, TCM_SETCURSEL, tab.index(), 0);
    show_page(state, tab);
    let first = match tab {
        AddressBookTab::Notify => state.notify.list,
        AddressBookTab::Control => state.control.list,
    };
    SetFocus(first);
}

unsafe fn show_page(state: &DialogState, tab: AddressBookTab) {
    for (page, control) in state.page_controls() {
        ShowWindow(control, if page == tab { SW_SHOW } else { SW_HIDE });
    }
}

unsafe fn selected_tab(state: &DialogState) -> AddressBookTab {
    let index = SendMessageW(state.tab_control, TCM_GETCURSEL, 0, 0);
    AddressBookTab::ALL[usize::try_from(index).expect("a tab is always selected")]
}

/// Fills a list's rows; nothing is selected afterwards, so its Edit and
/// Delete are greyed until something is.
unsafe fn fill_list(list: HWND, rows: &[Vec<String>]) {
    SendMessageW(list, LVM_DELETEALLITEMS, 0, 0);
    for (row, cells) in rows.iter().enumerate() {
        for (column, cell) in cells.iter().enumerate() {
            let mut text = wide_null(cell);
            let item = LVITEMW {
                mask: LVIF_TEXT,
                iItem: row as i32,
                iSubItem: column as i32,
                pszText: text.as_mut_ptr(),
                ..LVITEMW::default()
            };
            let message = if column == 0 {
                LVM_INSERTITEMW
            } else {
                LVM_SETITEMTEXTW
            };
            SendMessageW(list, message, row, &item as *const LVITEMW as LPARAM);
        }
    }
}

unsafe fn refill_notify_list(state: &DialogState) {
    let rows = state
        .form
        .notify_rows()
        .into_iter()
        .map(Vec::from)
        .collect::<Vec<_>>();
    fill_list(state.notify.list, &rows);
    grey_row_buttons(state);
}

unsafe fn refill_control_list(state: &DialogState) {
    let rows = state
        .form
        .ignore_rows()
        .into_iter()
        .map(|row| vec![row])
        .collect::<Vec<_>>();
    fill_list(state.control.list, &rows);
    grey_row_buttons(state);
}

unsafe fn selected_row(list: HWND) -> Option<usize> {
    let row = SendMessageW(list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as LPARAM);
    usize::try_from(row).ok()
}

/// Edit and Delete act on the selected row: greyed without one.
unsafe fn grey_row_buttons(state: &DialogState) {
    let notify_selected = selected_row(state.notify.list).is_some();
    EnableWindow(state.notify.edit, notify_selected.into());
    EnableWindow(state.notify.delete, notify_selected.into());
    let control_selected = selected_row(state.control.list).is_some();
    EnableWindow(state.control.delete, control_selected.into());
}

unsafe fn handle_notify_add(dialog: HWND) -> Result<(), String> {
    let form = dialog_state(dialog).form.clone();
    let entry = run_entry_dialog(
        dialog,
        EntrySpec {
            title: "Add Notify",
            fields: EntryFields::default(),
            nickname_read_only: false,
            notify: true,
        },
        move |fields| form.check_new_notify(fields),
    )?;
    if let Some(entry) = entry {
        let state = dialog_state(dialog);
        state.form.put_notify(entry);
        refill_notify_list(state);
    }
    Ok(())
}

unsafe fn handle_notify_edit(dialog: HWND) -> Result<(), String> {
    let state = dialog_state(dialog);
    let index = selected_row(state.notify.list).expect("Edit is greyed without a selection");
    let mut fields = state.form.notify_fields(index);
    let user_id = std::mem::replace(
        &mut fields.nickname,
        user_label(
            &state.form.notify.list.entries()[index].user_id,
            &|user_id: &str| state.form.people.nick_of(user_id),
        ),
    );
    let entry = run_entry_dialog(
        dialog,
        EntrySpec {
            title: "Edit Notify",
            fields,
            nickname_read_only: true,
            notify: true,
        },
        move |fields| Ok::<_, String>(super::address_book::notify_entry(user_id.clone(), fields)),
    )?;
    if let Some(entry) = entry {
        let state = dialog_state(dialog);
        state.form.put_notify(entry);
        refill_notify_list(state);
    }
    Ok(())
}

unsafe fn handle_notify_delete(dialog: HWND) {
    let state = dialog_state(dialog);
    let index = selected_row(state.notify.list).expect("Delete is greyed without a selection");
    state.form.remove_notify(index);
    refill_notify_list(state);
}

fn ignore_edit(state: &mut DialogState) -> &mut IgnoreEdit {
    state
        .form
        .ignore
        .as_mut()
        .expect("the Control tab works only while the ignore list is known")
}

unsafe fn handle_control_add(dialog: HWND) -> Result<(), String> {
    let state = dialog_state(dialog);
    let edit = ignore_edit(state).clone();
    let people = state.form.people.clone();
    let user_id = run_entry_dialog(
        dialog,
        EntrySpec {
            title: "Add Ignore",
            fields: EntryFields::default(),
            nickname_read_only: false,
            notify: false,
        },
        move |fields| edit.check_add(&people, &fields.nickname),
    )?;
    if let Some(user_id) = user_id {
        let state = dialog_state(dialog);
        ignore_edit(state).add(user_id);
        refill_control_list(state);
    }
    Ok(())
}

unsafe fn handle_control_delete(dialog: HWND) {
    let state = dialog_state(dialog);
    let index = selected_row(state.control.list).expect("Delete is greyed without a selection");
    ignore_edit(state).remove(index);
    refill_control_list(state);
}

unsafe fn handle_ok(dialog: HWND) {
    let state = dialog_state(dialog);
    let mut form = state.form.clone();
    form.notify = NotifySetting {
        active: SendMessageW(state.notify.active, BM_GETCHECK, 0, 0) == BST_CHECKED as LRESULT,
        ..form.notify
    };
    state.saved = Some(form);
    end_modal_dialog(dialog);
}

/// A handler's error: the dialog cannot go on with a failed control.
fn handled(result: Result<(), String>) -> LRESULT {
    if let Err(error) = result {
        panic!("the address book failed: {error}");
    }
    0
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
            ADDRESS_BOOK_NOTIFY_ADD_ID => handled(handle_notify_add(hwnd)),
            ADDRESS_BOOK_NOTIFY_EDIT_ID => handled(handle_notify_edit(hwnd)),
            ADDRESS_BOOK_NOTIFY_DELETE_ID => {
                handle_notify_delete(hwnd);
                0
            }
            ADDRESS_BOOK_CONTROL_ADD_ID => handled(handle_control_add(hwnd)),
            ADDRESS_BOOK_CONTROL_DELETE_ID => {
                handle_control_delete(hwnd);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        },
        WM_NOTIFY => {
            let header = &*(lparam as *const NMHDR);
            let id = header.idFrom as u16;
            match (id, header.code) {
                (ADDRESS_BOOK_TAB_ID, TCN_SELCHANGE) => {
                    let state = dialog_state(hwnd);
                    show_page(state, selected_tab(state));
                }
                (ADDRESS_BOOK_NOTIFY_LIST_ID | ADDRESS_BOOK_CONTROL_LIST_ID, LVN_ITEMCHANGED) => {
                    grey_row_buttons(dialog_state(hwnd));
                }
                // A row's double-click or Enter is Edit.
                (ADDRESS_BOOK_NOTIFY_LIST_ID, NM_DBLCLK | NM_RETURN)
                    if selected_row(dialog_state(hwnd).notify.list).is_some() =>
                {
                    return handled(handle_notify_edit(hwnd));
                }
                _ => {}
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

/// The Address Book as automation sees it, read back from its controls.
pub(super) unsafe fn dump(dialog: HWND) -> UiAddressBook {
    let state = dialog_state(dialog);
    let tab_click_points = AddressBookTab::ALL
        .into_iter()
        .map(|tab| {
            let mut rect = RECT::default();
            assert_ne!(
                SendMessageW(
                    state.tab_control,
                    TCM_GETITEMRECT,
                    tab.index(),
                    &mut rect as *mut RECT as LPARAM,
                ),
                0,
                "TCM_GETITEMRECT must place the {} tab",
                tab.label()
            );
            rect_center(&rect)
        })
        .collect();
    let visible_controls = state
        .page_controls()
        .into_iter()
        .filter(|(_, control)| GetWindowLongPtrW(*control, GWL_STYLE) as u32 & WS_VISIBLE != 0)
        .map(|(_, control)| GetDlgCtrlID(control) as u16)
        .collect();
    let (notify_rows, notify_row_click_points) = dump_list(state.notify.list, 2);
    let (control_rows, control_row_click_points) = dump_list(state.control.list, 1);
    let control_enabled = match [state.control.list, state.control.add]
        .map(|control| IsWindowEnabled(control) != 0)
    {
        [true, true] => true,
        [false, false] => false,
        enabled => panic!("the Control tab is half greyed: {enabled:?}"),
    };
    let enabled = |control| IsWindowEnabled(control) != 0;
    UiAddressBook {
        tab: selected_tab(state).label().to_owned(),
        tab_click_points,
        visible_controls,
        notify_rows,
        notify_row_click_points,
        notify_active: SendMessageW(state.notify.active, BM_GETCHECK, 0, 0)
            == BST_CHECKED as LRESULT,
        notify_edit_enabled: enabled(state.notify.edit),
        notify_delete_enabled: enabled(state.notify.delete),
        control_rows: control_rows.into_iter().flatten().collect(),
        control_row_click_points,
        control_enabled,
        control_delete_enabled: enabled(state.control.delete),
        control_note: read_window_text(state.control.note),
    }
}

/// A list's rows, `columns` cells each, and where a click selects each, in
/// the list's client coordinates.
unsafe fn dump_list(list: HWND, columns: usize) -> (Vec<Vec<String>>, Vec<UiPoint>) {
    let mut buffer = [0u16; 512];
    let rows = usize::try_from(SendMessageW(list, LVM_GETITEMCOUNT, 0, 0))
        .expect("LVM_GETITEMCOUNT succeeds");
    (0..rows)
        .map(|row| {
            let cells = (0..columns)
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
                .collect();
            let mut rect = RECT {
                left: LVIR_LABEL as i32,
                ..RECT::default()
            };
            assert_ne!(
                SendMessageW(list, LVM_GETITEMRECT, row, &mut rect as *mut _ as LPARAM),
                0,
                "LVM_GETITEMRECT must place row {row}"
            );
            (cells, rect_center(&rect))
        })
        .unzip()
}

/// What an entry dialog shows.
struct EntrySpec {
    title: &'static str,
    fields: EntryFields,
    /// Edit's nickname names the entry, which it does not change.
    nickname_read_only: bool,
    /// Note and Perform /whois: the Notify tab's entries.
    notify: bool,
}

/// An entry dialog's OK: checks the fields, and keeps what it accepts.
type Accept = Box<dyn FnMut(&EntryFields) -> Result<(), String>>;

/// The entry dialog's state.
struct EntryState {
    accept: Accept,
    notify: bool,
}

/// Add Notify, Edit Notify, Add Ignore: the entry dialog over the Address
/// Book, until OK passes `check` or Cancel. OK with fields `check` refuses
/// keeps it open, and a message box says why.
unsafe fn run_entry_dialog<T: 'static>(
    owner: HWND,
    spec: EntrySpec,
    check: impl Fn(&EntryFields) -> Result<T, String> + 'static,
) -> Result<Option<T>, String> {
    let accepted = Rc::new(RefCell::new(None));
    let slot = Rc::clone(&accepted);
    let mut state = Box::new(WindowData::new(EntryState {
        accept: Box::new(move |fields| {
            *slot.borrow_mut() = Some(check(fields)?);
            Ok(())
        }),
        notify: spec.notify,
    }));
    let EntrySpec {
        title,
        fields,
        nickname_read_only,
        notify,
    } = spec;
    let dialog = create_modal_dialog(
        owner,
        ADDRESS_ENTRY_DIALOG_CLASS_NAME,
        title,
        (&mut *state as *mut WindowData<EntryState>).cast(),
        |dialog| {
            let dpi = Dpi::of_window(dialog);
            let mut column = DialogColumn::new(dialog, dpi.px(ENTRY_WIDTH));
            let read_only = if nickname_read_only {
                ES_READONLY as u32
            } else {
                0
            };
            column.label("Nickname:", 0, 0)?;
            column.edit(ADDRESS_ENTRY_NICKNAME_ID, &fields.nickname, read_only)?;
            if notify {
                column.label("Note:", 0, 0)?;
                column.edit(ADDRESS_ENTRY_NOTE_ID, &fields.note, 0)?;
                let whois = column.checkbox(ADDRESS_ENTRY_WHOIS_ID, "Perform /whois")?;
                let check = if fields.whois {
                    BST_CHECKED
                } else {
                    BST_UNCHECKED
                };
                SendMessageW(whois, BM_SETCHECK, check as WPARAM, 0);
            }
            column.buttons(&[
                (ADDRESS_ENTRY_OK_ID, "OK"),
                (ADDRESS_ENTRY_CANCEL_ID, "Cancel"),
            ])
        },
    )?;
    grey_entry_ok(dialog);
    let first = if nickname_read_only {
        ADDRESS_ENTRY_NOTE_ID
    } else {
        ADDRESS_ENTRY_NICKNAME_ID
    };
    SetFocus(GetDlgItem(dialog, first.into()));
    run_modal_dialog(owner, dialog);
    drop(state);
    Ok(accepted.take())
}

/// OK needs a nickname.
unsafe fn grey_entry_ok(dialog: HWND) {
    let nickname = read_window_text(GetDlgItem(dialog, ADDRESS_ENTRY_NICKNAME_ID.into()));
    EnableWindow(
        GetDlgItem(dialog, ADDRESS_ENTRY_OK_ID as i32),
        (!nickname.trim().is_empty()).into(),
    );
}

unsafe fn entry_accept(dialog: HWND) {
    let state = window_data::get::<EntryState>(dialog).expect("the entry dialog has its state");
    let text = |id: u16| read_window_text(GetDlgItem(dialog, id.into()));
    let fields = if state.notify {
        EntryFields {
            nickname: text(ADDRESS_ENTRY_NICKNAME_ID),
            note: text(ADDRESS_ENTRY_NOTE_ID),
            whois: SendMessageW(
                GetDlgItem(dialog, ADDRESS_ENTRY_WHOIS_ID.into()),
                BM_GETCHECK,
                0,
                0,
            ) == BST_CHECKED as LRESULT,
        }
    } else {
        EntryFields {
            nickname: text(ADDRESS_ENTRY_NICKNAME_ID),
            ..EntryFields::default()
        }
    };
    match (state.accept)(&fields) {
        Ok(()) => end_modal_dialog(dialog),
        Err(reason) => {
            let message = wide_null(&reason);
            let title = wide_null(ADDRESS_BOOK_TITLE);
            MessageBoxW(
                dialog,
                message.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONWARNING,
            );
        }
    }
}

unsafe extern "system" fn entry_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            window_data::borrow_create_param::<EntryState>(hwnd, lparam);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_COMMAND => match wparam & 0xffff {
            ADDRESS_ENTRY_OK_ID => {
                entry_accept(hwnd);
                0
            }
            ADDRESS_ENTRY_CANCEL_ID | ESC_CANCEL_ID => {
                end_modal_dialog(hwnd);
                0
            }
            id if id == usize::from(ADDRESS_ENTRY_NICKNAME_ID)
                && (wparam >> 16) as u32 == EN_CHANGE =>
            {
                grey_entry_ok(hwnd);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        },
        DM_GETDEFID => default_button(ADDRESS_ENTRY_OK_ID),
        WM_CLOSE => {
            end_modal_dialog(hwnd);
            0
        }
        WM_NCDESTROY => {
            window_data::detach::<EntryState>(hwnd);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// The dialogs' classes and procedures, registered with the other dialogs'.
pub(super) const DIALOG_CLASSES: [(&str, WNDPROC); 2] = [
    (ADDRESS_BOOK_DIALOG_CLASS_NAME, Some(window_proc)),
    (ADDRESS_ENTRY_DIALOG_CLASS_NAME, Some(entry_window_proc)),
];

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;

    fn form(ignore: Option<IgnoreEdit>) -> AddressBookForm {
        AddressBookForm {
            notify: NotifySetting::default(),
            ignore,
            people: People::new(Vec::new(), Some("@me:example".to_owned())),
        }
    }

    #[test]
    fn ok_sends_the_ignore_changes_adds_first() {
        unsafe {
            with_test_frame(800, 600, |hwnd| {
                let (sender, commands) = inert_command_sender();
                std::mem::forget(commands);
                state_mut(hwnd).command_sender = Some(sender);
                let mut edit = IgnoreEdit::new(vec!["@spam:remote".to_owned()]);
                edit.remove(0);
                edit.add("@bob:example".to_owned());
                apply(hwnd, form(Some(edit))).unwrap();
                let state = state_ref(hwnd);
                assert_eq!(
                    state.recorded_commands,
                    [
                        Command::IgnoreUser("@bob:example".to_owned()),
                        Command::UnignoreUser("@spam:remote".to_owned()),
                    ]
                );
                assert_eq!(
                    state.model.status,
                    "address book saved; ignore list sent to the server"
                );
            });
        }
    }

    #[test]
    fn ok_without_ignore_changes_sends_nothing() {
        unsafe {
            with_test_frame(800, 600, |hwnd| {
                // Signed out: no list to edit.
                apply(hwnd, form(None)).unwrap();
                // Opened and left as it was.
                let edit = IgnoreEdit::new(vec!["@spam:remote".to_owned()]);
                apply(hwnd, form(Some(edit))).unwrap();
                let state = state_ref(hwnd);
                assert!(state.recorded_commands.is_empty());
                assert_eq!(state.model.status, "address book saved");
            });
        }
    }
}
