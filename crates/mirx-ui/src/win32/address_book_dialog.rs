//! The Address Book window (Tools > Address Book..., Alt+B, `/abook`):
//! a tab control with mIRC's Notify, Control and Colors tabs, OK and
//! Cancel. The
//! form and its checks are `address_book`'s; this module shows them.
//!
//! Every page control is a child of the dialog, not of the tab control,
//! so their notifications come here; the tab control is created first and
//! clips its siblings, so it never paints over them. A tab shows its
//! page's controls and hides the others'.
//!
//! Add and Edit open an entry dialog over the Address Book, modal to it:
//! the Notify and Control tabs' share one, the Colors tab has its own.
//! Its OK checks the fields against a copy of the form taken when it
//! opened, and keeps it open with a message box when they do not do.

use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::rc::Rc;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_WINDOWTEXT, DEFAULT_GUI_FONT, GetStockObject, GetSysColor,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    BST_CHECKED, BST_UNCHECKED, CDDS_ITEMPREPAINT, CDDS_PREPAINT, CDDS_SUBITEM, CDRF_NEWFONT,
    CDRF_NOTIFYITEMDRAW, CDRF_NOTIFYSUBITEMDRAW, DRAWITEMSTRUCT, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW,
    LVIF_STATE, LVIS_FOCUSED, LVIS_SELECTED, LVITEMW, LVM_ENSUREVISIBLE, LVM_INSERTCOLUMNW,
    LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMSTATE, LVN_ITEMCHANGED, LVS_EX_FULLROWSELECT,
    LVS_NOCOLUMNHEADER, LVS_REPORT, LVS_SHOWSELALWAYS, LVS_SINGLESEL, MEASUREITEMSTRUCT,
    NM_CUSTOMDRAW, NM_DBLCLK, NM_RETURN, NMHDR, NMLVCUSTOMDRAW, TCIF_TEXT, TCITEMW, TCM_ADJUSTRECT,
    TCM_GETCURSEL, TCM_GETITEMRECT, TCM_INSERTITEMW, TCM_SETCURSEL, TCN_SELCHANGE,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, DM_GETDEFID,
    DefWindowProcW, EN_CHANGE, ES_READONLY, GWL_STYLE, GetDlgCtrlID, GetDlgItem, GetWindowLongPtrW,
    HWND_BOTTOM, IDCANCEL, IDOK, MB_ICONWARNING, MB_OK, MessageBoxW, MoveWindow, SW_HIDE, SW_SHOW,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SendMessageW, SetWindowPos, ShowWindow, WM_CLOSE,
    WM_COMMAND, WM_DRAWITEM, WM_MEASUREITEM, WM_NCCREATE, WM_NCDESTROY, WM_NOTIFY, WM_SETFONT,
    WNDPROC, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_CLIENTEDGE, WS_TABSTOP, WS_VISIBLE,
};

use mirx_core::Command;

use super::address_book::{
    AddressBookForm, AddressBookTab, ColorEntryFields, EntryFields, IgnoreEdit, People,
};
use super::color_drop_down::{
    COLOR_DROP_DOWN_STYLE, COLOR_DROP_DOWN_WIDTH, color_drop_down_height, draw_color_item,
    fill_color_drop_down, measure_color_item, select_color, selected_color,
};
use super::dialogs::{
    DialogColumn, DialogMetrics, create_modal_dialog, default_button, dialog_text_size,
    end_modal_dialog,
};
use super::dpi::Dpi;
use super::ignore::user_label;
use super::list_view::{LIST_VIEW_CLASS_NAME, dump_list, fill_list, selected_row};
use super::nick_colors::{NickColor, NickColorEntry, NickColorSetting};
use super::notify::NotifySetting;
use super::room_list_window::rect_center;
use super::window_data::{self, WindowData};
use super::{
    BUTTON_CLASS_NAME, STATIC_CLASS_NAME, announce_notify_changes, create_child_window, echo_view,
    enqueue_command, read_window_text, refresh_view_content, run_modal_dialog,
    set_local_command_status, show_nick_colors, state_mut, state_ref, wide_null,
};
use crate::automation::UiAddressBook;

pub(super) const ADDRESS_BOOK_DIALOG_CLASS_NAME: &str = "mirxWin32AddressBookDialog";
pub(super) const ADDRESS_ENTRY_DIALOG_CLASS_NAME: &str = "mirxWin32AddressEntryDialog";
pub(super) const ADDRESS_COLOR_ENTRY_DIALOG_CLASS_NAME: &str = "mirxWin32AddressColorEntryDialog";
const ADDRESS_BOOK_TITLE: &str = "Address Book";
const TAB_CONTROL_CLASS_NAME: &str = "SysTabControl32";

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
pub(super) const ADDRESS_BOOK_COLORS_ACTIVE_ID: u16 = 7401;
pub(super) const ADDRESS_BOOK_COLORS_LIST_ID: u16 = 7402;
pub(super) const ADDRESS_BOOK_COLORS_ADD_ID: u16 = 7403;
pub(super) const ADDRESS_BOOK_COLORS_EDIT_ID: u16 = 7404;
pub(super) const ADDRESS_BOOK_COLORS_DELETE_ID: u16 = 7405;
pub(super) const ADDRESS_BOOK_COLORS_UP_ID: u16 = 7406;
pub(super) const ADDRESS_BOOK_COLORS_DOWN_ID: u16 = 7407;
pub(super) const COLOR_ENTRY_PATTERN_ID: u16 = 7501;
pub(super) const COLOR_ENTRY_COLOR_ID: u16 = 7502;
pub(super) const COLOR_ENTRY_MODES_ID: u16 = 7503;
pub(super) const COLOR_ENTRY_ANY_MODE_ID: u16 = 7504;
pub(super) const COLOR_ENTRY_NO_MODE_ID: u16 = 7505;
pub(super) const COLOR_ENTRY_NOTIFY_ID: u16 = 7506;
pub(super) const COLOR_ENTRY_OK_ID: usize = 7507;
pub(super) const COLOR_ENTRY_CANCEL_ID: usize = 7508;

const CONTROL_LABEL: &str = "Ignore these users:";
const CONTROL_NOTE: &str = "Your ignore list is kept on your Matrix server and shared with \
your other clients. The server stops sending an ignored user's messages and invites.";
const CONTROL_SIGNED_OUT_NOTE: &str = "Connect to edit the ignore list.";

// The layout's lengths at 96 dpi.
const LIST_WIDTH: i32 = 320;
const LIST_HEIGHT: i32 = 150;
const NICKNAME_COLUMN_WIDTH: i32 = 200;
/// The Colors list's: its Nickname column is narrower, for Color and
/// Apply.
const COLORS_NICKNAME_COLUMN_WIDTH: i32 = 150;
const COLORS_COLOR_COLUMN_WIDTH: i32 = 50;
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

struct ColorsPage {
    list: HWND,
    add: HWND,
    edit: HWND,
    delete: HWND,
    up: HWND,
    down: HWND,
    active: HWND,
}

/// The Address Book's state, owned by `run_address_book_dialog` and lent
/// to the window procedure while the window exists.
struct DialogState {
    form: AddressBookForm,
    tab_control: HWND,
    notify: NotifyPage,
    control: ControlPage,
    colors: ColorsPage,
    /// The form OK accepted.
    saved: Option<AddressBookForm>,
}

impl DialogState {
    fn page_controls(&self) -> [(AddressBookTab, HWND); 17] {
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
        let ColorsPage {
            list: colors_list,
            add: colors_add,
            edit: colors_edit,
            delete: colors_delete,
            up,
            down,
            active: colors_active,
        } = self.colors;
        let colors = AddressBookTab::Colors;
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
            (colors, colors_list),
            (colors, colors_add),
            (colors, colors_edit),
            (colors, colors_delete),
            (colors, up),
            (colors, down),
            (colors, colors_active),
        ]
    }
}

unsafe fn dialog_state(hwnd: HWND) -> &'static mut DialogState {
    window_data::get(hwnd).expect("the address book has its state")
}

/// Tools > Address Book... and `/abook`: shows the notify list, the
/// ignore list and the nick colours at `tab` until OK or Cancel. OK saves
/// the notify list, the nick colours and their options at once and sends
/// the ignore list's changes to the server.
pub(super) unsafe fn run_address_book_dialog(
    hwnd: HWND,
    tab: AddressBookTab,
) -> Result<(), String> {
    let state = state_ref(hwnd);
    let model = &state.model;
    let form = AddressBookForm {
        notify: state.settings.settings().notify.clone(),
        ignore: model.ignored_users.clone().map(IgnoreEdit::new),
        nick_colors: state.settings.settings().nick_colors.clone(),
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
        colors: ColorsPage {
            list: null_mut(),
            add: null_mut(),
            edit: null_mut(),
            delete: null_mut(),
            up: null_mut(),
            down: null_mut(),
            active: null_mut(),
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

/// What OK does with the form: the notify list, the nick colours and
/// their options are saved, and a user listed while on is news, as with
/// `/notify`; the nicks take their colours; the ignore list's changes go
/// to the server, whose echo prints them.
unsafe fn apply(hwnd: HWND, form: AddressBookForm) -> Result<(), String> {
    let AddressBookForm {
        notify,
        ignore,
        nick_colors,
        ..
    } = form;
    let state = state_mut(hwnd);
    state.settings.update(|settings| {
        settings.notify = notify.clone();
        settings.nick_colors = nick_colors.clone();
    })?;
    announce_notify_changes(state)?;
    let view = echo_view(&state.active_view);
    refresh_view_content(hwnd, &view)?;
    show_nick_colors(hwnd)?;
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

    // Colors: the list, its five buttons beside it, Enable under it.
    let colors_nickname_width = dpi.px(COLORS_NICKNAME_COLUMN_WIDTH);
    let colors_color_width = dpi.px(COLORS_COLOR_COLUMN_WIDTH);
    state.colors.list = list(
        ADDRESS_BOOK_COLORS_LIST_ID,
        page_y,
        &[
            ("Nickname", colors_nickname_width),
            ("Color", colors_color_width),
            (
                "Apply",
                inner_width - colors_nickname_width - colors_color_width,
            ),
        ],
        true,
    )?;
    state.colors.add = button("Add...", ADDRESS_BOOK_COLORS_ADD_ID, 0, page_y)?;
    state.colors.edit = button("Edit...", ADDRESS_BOOK_COLORS_EDIT_ID, 1, page_y)?;
    state.colors.delete = button("Delete", ADDRESS_BOOK_COLORS_DELETE_ID, 2, page_y)?;
    state.colors.up = button("Up", ADDRESS_BOOK_COLORS_UP_ID, 3, page_y)?;
    state.colors.down = button("Down", ADDRESS_BOOK_COLORS_DOWN_ID, 4, page_y)?;
    state.colors.active = place(
        control(
            BUTTON_CLASS_NAME,
            "Enable nick colors",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            0,
            ADDRESS_BOOK_COLORS_ACTIVE_ID,
        )?,
        page_x,
        page_y + list_height + gap,
        page_width,
        checkbox_height,
    );

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

    set_check(state.notify.active, state.form.notify.active);
    set_check(state.colors.active, state.form.nick_colors.active);
    refill_notify_list(state);
    refill_control_list(state);
    refill_colors_list(state);
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
        AddressBookTab::Colors => state.colors.list,
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

unsafe fn refill_colors_list(state: &DialogState) {
    let rows = state
        .form
        .color_rows()
        .into_iter()
        .map(Vec::from)
        .collect::<Vec<_>>();
    fill_list(state.colors.list, &rows);
    grey_row_buttons(state);
}

/// Selects (and focuses) `row`, as a click on it does.
unsafe fn select_row(list: HWND, row: usize) {
    let state = LVIS_SELECTED | LVIS_FOCUSED;
    let item = LVITEMW {
        mask: LVIF_STATE,
        state,
        stateMask: state,
        ..LVITEMW::default()
    };
    SendMessageW(
        list,
        LVM_SETITEMSTATE,
        row,
        &item as *const LVITEMW as LPARAM,
    );
    SendMessageW(list, LVM_ENSUREVISIBLE, row, 0);
}

/// Edit and Delete act on the selected row: greyed without one.
unsafe fn grey_row_buttons(state: &DialogState) {
    let notify_selected = selected_row(state.notify.list).is_some();
    EnableWindow(state.notify.edit, notify_selected.into());
    EnableWindow(state.notify.delete, notify_selected.into());
    let control_selected = selected_row(state.control.list).is_some();
    EnableWindow(state.control.delete, control_selected.into());
    let colors_selected = selected_row(state.colors.list);
    let last = state.form.nick_colors.list.entries().len().checked_sub(1);
    EnableWindow(state.colors.edit, colors_selected.is_some().into());
    EnableWindow(state.colors.delete, colors_selected.is_some().into());
    EnableWindow(
        state.colors.up,
        colors_selected.is_some_and(|row| row > 0).into(),
    );
    EnableWindow(
        state.colors.down,
        colors_selected.is_some_and(|row| Some(row) != last).into(),
    );
}

unsafe fn set_check(checkbox: HWND, checked: bool) {
    let check = if checked { BST_CHECKED } else { BST_UNCHECKED };
    SendMessageW(checkbox, BM_SETCHECK, check as WPARAM, 0);
}

unsafe fn is_checked(checkbox: HWND) -> bool {
    SendMessageW(checkbox, BM_GETCHECK, 0, 0) == BST_CHECKED as LRESULT
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

unsafe fn handle_colors_add(dialog: HWND) -> Result<(), String> {
    let form = dialog_state(dialog).form.clone();
    let entry = run_color_entry_dialog(
        dialog,
        "Add Nick Color",
        ColorEntryFields::default(),
        move |fields| form.check_color_entry(fields),
    )?;
    if let Some(entry) = entry {
        let state = dialog_state(dialog);
        state.form.put_color(None, entry);
        refill_colors_list(state);
    }
    Ok(())
}

unsafe fn handle_colors_edit(dialog: HWND) -> Result<(), String> {
    let state = dialog_state(dialog);
    let index = selected_row(state.colors.list).expect("Edit is greyed without a selection");
    let form = state.form.clone();
    let entry = run_color_entry_dialog(
        dialog,
        "Edit Nick Color",
        form.color_fields(index),
        move |fields| form.check_color_entry(fields),
    )?;
    if let Some(entry) = entry {
        let state = dialog_state(dialog);
        state.form.put_color(Some(index), entry);
        refill_colors_list(state);
        select_row(state.colors.list, index);
    }
    Ok(())
}

unsafe fn handle_colors_delete(dialog: HWND) {
    let state = dialog_state(dialog);
    let index = selected_row(state.colors.list).expect("Delete is greyed without a selection");
    state.form.remove_color(index);
    refill_colors_list(state);
}

/// Up and Down: the selection moves with its entry.
unsafe fn handle_colors_move(dialog: HWND, up: bool) {
    let state = dialog_state(dialog);
    let index =
        selected_row(state.colors.list).expect("Up and Down are greyed without a selection");
    let moved = state.form.move_color(index, up);
    refill_colors_list(state);
    select_row(state.colors.list, moved);
}

/// The Colors list paints each Nickname cell in its entry's colour, and
/// an auto colour's (which differs per user) in the list's text colour.
unsafe fn colors_list_custom_draw(state: &DialogState, draw: &mut NMLVCUSTOMDRAW) -> LRESULT {
    match draw.nmcd.dwDrawStage {
        CDDS_PREPAINT => CDRF_NOTIFYITEMDRAW as LRESULT,
        CDDS_ITEMPREPAINT => CDRF_NOTIFYSUBITEMDRAW as LRESULT,
        stage if stage == CDDS_ITEMPREPAINT | CDDS_SUBITEM => {
            let entry = &state.form.nick_colors.list.entries()[draw.nmcd.dwItemSpec];
            draw.clrText = match (draw.iSubItem, entry.color) {
                (0, NickColor::Palette(index)) => index.colorref(),
                _ => GetSysColor(COLOR_WINDOWTEXT),
            };
            CDRF_NEWFONT as LRESULT
        }
        // CDRF_DODEFAULT
        _ => 0,
    }
}

unsafe fn handle_ok(dialog: HWND) {
    let state = dialog_state(dialog);
    let mut form = state.form.clone();
    form.notify = NotifySetting {
        active: is_checked(state.notify.active),
        ..form.notify
    };
    form.nick_colors = NickColorSetting {
        active: is_checked(state.colors.active),
        ..form.nick_colors
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
            ADDRESS_BOOK_COLORS_ADD_ID => handled(handle_colors_add(hwnd)),
            ADDRESS_BOOK_COLORS_EDIT_ID => handled(handle_colors_edit(hwnd)),
            ADDRESS_BOOK_COLORS_DELETE_ID => {
                handle_colors_delete(hwnd);
                0
            }
            ADDRESS_BOOK_COLORS_UP_ID | ADDRESS_BOOK_COLORS_DOWN_ID => {
                handle_colors_move(hwnd, (wparam & 0xffff) as u16 == ADDRESS_BOOK_COLORS_UP_ID);
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
                (
                    ADDRESS_BOOK_NOTIFY_LIST_ID
                    | ADDRESS_BOOK_CONTROL_LIST_ID
                    | ADDRESS_BOOK_COLORS_LIST_ID,
                    LVN_ITEMCHANGED,
                ) => {
                    grey_row_buttons(dialog_state(hwnd));
                }
                // A row's double-click or Enter is Edit.
                (ADDRESS_BOOK_NOTIFY_LIST_ID, NM_DBLCLK | NM_RETURN)
                    if selected_row(dialog_state(hwnd).notify.list).is_some() =>
                {
                    return handled(handle_notify_edit(hwnd));
                }
                (ADDRESS_BOOK_COLORS_LIST_ID, NM_DBLCLK | NM_RETURN)
                    if selected_row(dialog_state(hwnd).colors.list).is_some() =>
                {
                    return handled(handle_colors_edit(hwnd));
                }
                (ADDRESS_BOOK_COLORS_LIST_ID, NM_CUSTOMDRAW) => {
                    return colors_list_custom_draw(
                        dialog_state(hwnd),
                        &mut *(lparam as *mut NMLVCUSTOMDRAW),
                    );
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
    let (color_rows, color_row_click_points) = dump_list(state.colors.list, 3);
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
        notify_active: is_checked(state.notify.active),
        notify_edit_enabled: enabled(state.notify.edit),
        notify_delete_enabled: enabled(state.notify.delete),
        control_rows: control_rows.into_iter().flatten().collect(),
        control_row_click_points,
        control_enabled,
        control_delete_enabled: enabled(state.control.delete),
        control_note: read_window_text(state.control.note),
        colors_active: is_checked(state.colors.active),
        color_rows,
        color_row_click_points,
        color_edit_enabled: enabled(state.colors.edit),
        color_delete_enabled: enabled(state.colors.delete),
        color_up_enabled: enabled(state.colors.up),
        color_down_enabled: enabled(state.colors.down),
    }
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
                set_check(whois, fields.whois);
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
            whois: is_checked(GetDlgItem(dialog, ADDRESS_ENTRY_WHOIS_ID.into())),
        }
    } else {
        EntryFields {
            nickname: text(ADDRESS_ENTRY_NICKNAME_ID),
            ..EntryFields::default()
        }
    };
    accept_or_refuse(dialog, (state.accept)(&fields));
}

/// An entry dialog's OK: closes it when its check passed, else says why
/// and stays open.
unsafe fn accept_or_refuse(dialog: HWND, checked: Result<(), String>) {
    match checked {
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

/// The colour entry dialog's OK: checks the fields, and keeps the entry
/// it accepts.
type ColorAccept = Box<dyn FnMut(&ColorEntryFields) -> Result<(), String>>;

/// Add Nick Color and Edit Nick Color: whom the entry names (editable in
/// Edit too: the pattern is what the entry is, not its key), its colour,
/// and its other Apply items, over the Address Book until OK passes
/// `check` or Cancel, as `run_entry_dialog`.
unsafe fn run_color_entry_dialog(
    owner: HWND,
    title: &str,
    fields: ColorEntryFields,
    check: impl Fn(&ColorEntryFields) -> Result<NickColorEntry, String> + 'static,
) -> Result<Option<NickColorEntry>, String> {
    let accepted = Rc::new(RefCell::new(None));
    let slot = Rc::clone(&accepted);
    let accept: ColorAccept = Box::new(move |fields| {
        *slot.borrow_mut() = Some(check(fields)?);
        Ok(())
    });
    let mut state = Box::new(WindowData::new(accept));
    let dialog = create_modal_dialog(
        owner,
        ADDRESS_COLOR_ENTRY_DIALOG_CLASS_NAME,
        title,
        (&mut *state as *mut WindowData<ColorAccept>).cast(),
        |dialog| {
            let dpi = Dpi::of_window(dialog);
            let mut column = DialogColumn::new(dialog, dpi.px(ENTRY_WIDTH));
            column.label("Nick, user ID or pattern (* and ?):", 0, 0)?;
            column.edit(COLOR_ENTRY_PATTERN_ID, &fields.pattern, 0)?;
            column.label("Color:", 0, 0)?;
            let edit_height = column.metrics.edit_height;
            let color = column.drop_down(
                COLOR_ENTRY_COLOR_ID,
                COLOR_DROP_DOWN_STYLE,
                dpi.px(COLOR_DROP_DOWN_WIDTH),
                color_drop_down_height(edit_height),
            )?;
            fill_color_drop_down(color, true);
            select_color(
                color,
                true,
                match fields.color {
                    NickColor::Palette(index) => Some(index),
                    NickColor::Auto => None,
                },
            );
            column.label("Modes (some of ~&@+; empty for any):", 0, 0)?;
            column.edit(COLOR_ENTRY_MODES_ID, &fields.modes, 0)?;
            for (id, text, checked) in [
                (COLOR_ENTRY_ANY_MODE_ID, "Any mode", fields.any_mode),
                (COLOR_ENTRY_NO_MODE_ID, "No mode", fields.no_mode),
                (COLOR_ENTRY_NOTIFY_ID, "On notify list", fields.notify),
            ] {
                set_check(column.checkbox(id, text)?, checked);
            }
            column.buttons(&[(COLOR_ENTRY_OK_ID, "OK"), (COLOR_ENTRY_CANCEL_ID, "Cancel")])
        },
    )?;
    grey_color_entry_ok(dialog);
    SetFocus(GetDlgItem(dialog, COLOR_ENTRY_PATTERN_ID.into()));
    run_modal_dialog(owner, dialog);
    drop(state);
    Ok(accepted.take())
}

/// OK needs whom the entry names.
unsafe fn grey_color_entry_ok(dialog: HWND) {
    let pattern = read_window_text(GetDlgItem(dialog, COLOR_ENTRY_PATTERN_ID.into()));
    EnableWindow(
        GetDlgItem(dialog, COLOR_ENTRY_OK_ID as i32),
        (!pattern.trim().is_empty()).into(),
    );
}

unsafe fn color_entry_accept(dialog: HWND) {
    let accept =
        window_data::get::<ColorAccept>(dialog).expect("the colour entry dialog has its state");
    let item = |id: u16| GetDlgItem(dialog, id.into());
    let fields = ColorEntryFields {
        pattern: read_window_text(item(COLOR_ENTRY_PATTERN_ID)),
        color: selected_color(item(COLOR_ENTRY_COLOR_ID))
            .map_or(NickColor::Auto, NickColor::Palette),
        modes: read_window_text(item(COLOR_ENTRY_MODES_ID)),
        any_mode: is_checked(item(COLOR_ENTRY_ANY_MODE_ID)),
        no_mode: is_checked(item(COLOR_ENTRY_NO_MODE_ID)),
        notify: is_checked(item(COLOR_ENTRY_NOTIFY_ID)),
    };
    accept_or_refuse(dialog, accept(&fields));
}

unsafe extern "system" fn color_entry_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            window_data::borrow_create_param::<ColorAccept>(hwnd, lparam);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_COMMAND => match wparam & 0xffff {
            COLOR_ENTRY_OK_ID => {
                color_entry_accept(hwnd);
                0
            }
            COLOR_ENTRY_CANCEL_ID | ESC_CANCEL_ID => {
                end_modal_dialog(hwnd);
                0
            }
            id if id == usize::from(COLOR_ENTRY_PATTERN_ID)
                && (wparam >> 16) as u32 == EN_CHANGE =>
            {
                grey_color_entry_ok(hwnd);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        },
        // The colour drop-down, while it is created too.
        WM_MEASUREITEM => {
            let edit_height = DialogMetrics::of(hwnd).edit_height;
            measure_color_item(&mut *(lparam as *mut MEASUREITEMSTRUCT), edit_height);
            1
        }
        WM_DRAWITEM => {
            draw_color_item(&*(lparam as *const DRAWITEMSTRUCT));
            1
        }
        DM_GETDEFID => default_button(COLOR_ENTRY_OK_ID),
        WM_CLOSE => {
            end_modal_dialog(hwnd);
            0
        }
        WM_NCDESTROY => {
            window_data::detach::<ColorAccept>(hwnd);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// The dialogs' classes and procedures, registered with the other dialogs'.
pub(super) const DIALOG_CLASSES: [(&str, WNDPROC); 3] = [
    (ADDRESS_BOOK_DIALOG_CLASS_NAME, Some(window_proc)),
    (ADDRESS_ENTRY_DIALOG_CLASS_NAME, Some(entry_window_proc)),
    (
        ADDRESS_COLOR_ENTRY_DIALOG_CLASS_NAME,
        Some(color_entry_window_proc),
    ),
];

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;

    fn form(ignore: Option<IgnoreEdit>) -> AddressBookForm {
        AddressBookForm {
            notify: NotifySetting::default(),
            ignore,
            nick_colors: NickColorSetting::default(),
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
    fn ok_saves_the_nick_colors_with_the_notify_list() {
        unsafe {
            with_test_frame(800, 600, |hwnd| {
                let mut form = form(None);
                form.nick_colors.active = false;
                form.put_color(
                    None,
                    NickColorEntry::parse_ini_value("*:example 4 notify").unwrap(),
                );
                form.notify.list.add("@bob:example".to_owned(), None, false);
                apply(hwnd, form.clone()).unwrap();
                let state = state_ref(hwnd);
                assert_eq!(state.settings.settings().nick_colors, form.nick_colors);
                assert_eq!(state.settings.settings().notify, form.notify);
                assert_eq!(state.model.status, "address book saved");
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
