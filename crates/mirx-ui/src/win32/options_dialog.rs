//! The Options dialog (Tools > Options..., Alt+O): mIRC's category tree on
//! the left, the selected page on the right, OK and Cancel.
//!
//! The tree and the form come from `options`; this module only shows them.
//! The dialog is modal over the frame and runs its own message loop, which
//! `IsDialogMessageW` gives a dialog's keys: Tab between fields, Enter for
//! OK (Add in the Highlight page's word box) and Esc for Cancel.

use std::mem::zeroed;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{COLOR_BTNFACE, HBRUSH};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    BST_CHECKED, BST_UNCHECKED, EM_SETSEL, NMHDR, TVGN_CARET, TVGN_CHILD, TVGN_NEXT, TVGN_ROOT,
    TVM_GETNEXTITEM, TVM_SELECTITEM, TVN_SELCHANGEDW, TVS_HASBUTTONS, TVS_HASLINES,
    TVS_LINESATROOT, TVS_SHOWSELALWAYS,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, IsWindowEnabled, SetFocus,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CreateWindowExW,
    DefWindowProcW, DestroyWindow, ES_AUTOHSCROLL, ES_NUMBER, GWL_STYLE, GetClientRect,
    GetDlgCtrlID, GetWindowLongPtrW, GetWindowRect, IDC_ARROW, IDCANCEL, IDOK, IsDialogMessageW,
    LB_ADDSTRING, LB_ERR, LB_GETCOUNT, LB_GETCURSEL, LB_GETTEXT, LB_GETTEXTLEN, LB_RESETCONTENT,
    LBN_SELCHANGE, LBS_NOINTEGRALHEIGHT, LBS_NOTIFY, LoadCursorW, MB_ICONERROR, MB_OK, MessageBoxW,
    MoveWindow, RegisterClassExW, SW_HIDE, SW_SHOW, SendMessageW, ShowWindow, WM_COMMAND,
    WM_NCCREATE, WM_NCDESTROY, WM_NOTIFY, WNDCLASSEXW, WS_BORDER, WS_CAPTION, WS_CHILD,
    WS_EX_CLIENTEDGE, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use mirx_core::HighlightWord;

use super::options::{
    FIRST_PAGE, HighlightChanges, HighlightList, OPTIONS_TREE, OptionsField, OptionsForm,
    OptionsPage, page_tree_index, tree_index, tree_paths,
};
use super::settings::Settings;
use super::timestamp::format_tokens;
use super::window_data::{self, WindowData};
use super::{
    BUTTON_CLASS_NAME, EDIT_CLASS_NAME, STATIC_CLASS_NAME, TREE_CLASS_NAME, create_child_window,
    expand_tree_item, insert_tree_item, last_os_error, read_tree_item_text, read_window_text,
    run_modal_dialog, selected_tree_item, set_window_text, wide_null,
};
use crate::automation::UiOptionsDialog;

pub(super) const OPTIONS_DIALOG_CLASS_NAME: &str = "mirxWin32OptionsDialog";
const OPTIONS_DIALOG_TITLE: &str = "mirx Options";
pub(super) const OPTIONS_TREE_ID: u16 = 6001;
pub(super) const OPTIONS_CONNECT_ON_STARTUP_ID: u16 = 6101;
pub(super) const OPTIONS_TIMESTAMP_SHOW_ID: u16 = 6201;
pub(super) const OPTIONS_TIMESTAMP_FORMAT_ID: u16 = 6202;
pub(super) const OPTIONS_TIMESTAMP_TOKENS_ID: u16 = 6203;
pub(super) const OPTIONS_SWITCHBAR_BUTTONS_ID: u16 = 6301;
pub(super) const OPTIONS_HIGHLIGHT_LIST_ID: u16 = 6401;
pub(super) const OPTIONS_HIGHLIGHT_WORD_ID: u16 = 6402;
pub(super) const OPTIONS_HIGHLIGHT_ADD_ID: u16 = 6403;
pub(super) const OPTIONS_HIGHLIGHT_DELETE_ID: u16 = 6404;
pub(super) const OPTIONS_HIGHLIGHT_NOTE_ID: u16 = 6405;
const LISTBOX_CLASS_NAME: &str = "LISTBOX";
const HIGHLIGHT_NOTE: &str = "Words are matched as whole words; * and ? are wildcards. \
Highlight words are kept on your Matrix server and shared with your other clients.";
const HIGHLIGHT_SIGNED_OUT_NOTE: &str = "Connect to edit highlight words.";

const DIALOG_WIDTH: i32 = 520;
const DIALOG_HEIGHT: i32 = 380;
const MARGIN: i32 = 8;
const TREE_WIDTH: i32 = 150;
const PAGE_X: i32 = MARGIN + TREE_WIDTH + 12;
const BUTTON_WIDTH: i32 = 80;
const BUTTON_HEIGHT: i32 = 26;
const LABEL_HEIGHT: i32 = 16;

pub(super) fn register_class(instance: *mut std::ffi::c_void) -> Result<(), String> {
    let class_name = wide_null(OPTIONS_DIALOG_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_BTNFACE as isize + 1) as HBRUSH,
        lpszClassName: class_name.as_ptr(),
        ..unsafe { zeroed() }
    };
    // A second registration, as each unit test makes, finds it registered.
    if unsafe { RegisterClassExW(&window_class) } == 0
        && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
    {
        return Err(last_os_error("RegisterClassExW options dialog"));
    }
    Ok(())
}

/// The form's controls.
struct Fields {
    connect_on_startup: HWND,
    timestamp_show: HWND,
    timestamp_format: HWND,
    switchbar_buttons: HWND,
}

impl Fields {
    unsafe fn read(&self) -> OptionsForm {
        let checked = |button| SendMessageW(button, BM_GETCHECK, 0, 0) == BST_CHECKED as LRESULT;
        OptionsForm {
            connect_on_startup: checked(self.connect_on_startup),
            timestamp_show: checked(self.timestamp_show),
            timestamp_format: read_window_text(self.timestamp_format),
            switchbar_buttons: read_window_text(self.switchbar_buttons),
        }
    }

    fn field(&self, field: OptionsField) -> HWND {
        match field {
            OptionsField::TimestampFormat => self.timestamp_format,
            OptionsField::SwitchbarButtons => self.switchbar_buttons,
        }
    }
}

/// The Highlight page's controls.
struct HighlightControls {
    list: HWND,
    word: HWND,
    add: HWND,
    delete: HWND,
}

/// What OK saved: the settings, and the highlight words to send.
pub(super) struct OptionsResult {
    pub(super) settings: Settings,
    pub(super) highlight_changes: HighlightChanges,
}

/// The dialog's state, owned by `OptionsDialog` and lent to the window
/// procedure while the window exists.
struct DialogState {
    current: Settings,
    /// The Highlight page's words as edited; `None` while signed out, when
    /// the page is greyed. A snapshot: a list arriving while the dialog is
    /// open does not change it.
    highlight: Option<HighlightList>,
    highlight_controls: HighlightControls,
    tree: HWND,
    /// The tree's items, in `OPTIONS_TREE` order.
    tree_items: Vec<isize>,
    /// Every page's controls; only the shown page's are visible.
    page_controls: Vec<(OptionsPage, HWND)>,
    fields: Fields,
    /// What OK saved.
    saved: Option<OptionsResult>,
}

/// An open Options dialog. `run` shows it until OK or Cancel closes it.
pub(super) struct OptionsDialog {
    hwnd: HWND,
    owner: HWND,
    state: Box<WindowData<DialogState>>,
}

impl OptionsDialog {
    /// Creates the dialog over `owner`, centred on it, showing `current`
    /// and the server's `highlight_words` (`None` while signed out).
    pub(super) unsafe fn open(
        owner: HWND,
        current: &Settings,
        highlight_words: Option<&[HighlightWord]>,
    ) -> Result<Self, String> {
        let mut state = Box::new(WindowData::new(DialogState {
            current: current.clone(),
            highlight: highlight_words.map(|words| HighlightList::new(words.to_vec())),
            highlight_controls: HighlightControls {
                list: null_mut(),
                word: null_mut(),
                add: null_mut(),
                delete: null_mut(),
            },
            tree: null_mut(),
            tree_items: Vec::new(),
            page_controls: Vec::new(),
            fields: Fields {
                connect_on_startup: null_mut(),
                timestamp_show: null_mut(),
                timestamp_format: null_mut(),
                switchbar_buttons: null_mut(),
            },
            saved: None,
        }));
        let mut owner_rect = RECT::default();
        GetWindowRect(owner, &mut owner_rect);
        let class_name = wide_null(OPTIONS_DIALOG_CLASS_NAME);
        let title = wide_null(OPTIONS_DIALOG_TITLE);
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            owner_rect.left + (owner_rect.right - owner_rect.left - DIALOG_WIDTH) / 2,
            owner_rect.top + (owner_rect.bottom - owner_rect.top - DIALOG_HEIGHT) / 2,
            DIALOG_WIDTH,
            DIALOG_HEIGHT,
            owner,
            null_mut(),
            GetModuleHandleW(null()),
            (&mut *state as *mut WindowData<DialogState>).cast(),
        );
        if hwnd.is_null() {
            return Err(last_os_error("CreateWindowExW options dialog"));
        }
        if let Err(error) = create_controls(hwnd) {
            DestroyWindow(hwnd);
            return Err(error);
        }
        Ok(Self { hwnd, owner, state })
    }

    pub(super) fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// Shows the dialog until it closes: what OK saved, or `None` for
    /// Cancel, Esc and the close box.
    pub(super) unsafe fn run(self) -> Option<OptionsResult> {
        let Self {
            hwnd,
            owner,
            mut state,
        } = self;
        ShowWindow(hwnd, SW_SHOW);
        SetFocus(state.tree);
        run_modal_dialog(owner, hwnd, |message| IsDialogMessageW(hwnd, message) != 0);
        state.saved.take()
    }
}

unsafe fn dialog_state(hwnd: HWND) -> &'static mut DialogState {
    window_data::get(hwnd).expect("the options dialog has its state")
}

unsafe fn create_controls(hwnd: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let mut client = RECT::default();
    GetClientRect(hwnd, &mut client);
    let control = |class: &str, text: &str, style: u32, ex_style: u32, id: u16| {
        create_child_window(
            hwnd,
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
    let buttons_y = client.bottom - MARGIN - BUTTON_HEIGHT;
    let page_width = client.right - PAGE_X - MARGIN;
    let state = dialog_state(hwnd);

    // mIRC labels its tree, which names the shown page; there is no
    // page heading.
    place(
        control(STATIC_CLASS_NAME, "Category:", WS_VISIBLE, 0, 0)?,
        MARGIN,
        MARGIN,
        TREE_WIDTH,
        LABEL_HEIGHT,
    );
    let tree_top = MARGIN + LABEL_HEIGHT + 2;
    state.tree = place(
        control(
            TREE_CLASS_NAME,
            "",
            WS_VISIBLE
                | WS_BORDER
                | WS_TABSTOP
                | TVS_HASLINES
                | TVS_HASBUTTONS
                | TVS_LINESATROOT
                | TVS_SHOWSELALWAYS,
            0,
            OPTIONS_TREE_ID,
        )?,
        MARGIN,
        tree_top,
        TREE_WIDTH,
        buttons_y - MARGIN - tree_top,
    );
    let mut category = 0;
    for (label, depth, _) in OPTIONS_TREE {
        let parent = match depth {
            0 => 0,
            1 => category,
            _ => unreachable!("the tree is two levels deep"),
        };
        let item = insert_tree_item(state.tree, parent, label);
        if item == 0 {
            return Err(format!("failed to add the options tree item {label}"));
        }
        if *depth == 0 {
            category = item;
        }
        state.tree_items.push(item);
    }
    for (index, (_, depth, _)) in OPTIONS_TREE.iter().enumerate() {
        if *depth == 0 {
            expand_tree_item(state.tree, state.tree_items[index]);
        }
    }

    let checkbox = WS_TABSTOP | BS_AUTOCHECKBOX as u32;
    let edit = WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32;
    let page_top = tree_top;

    state.fields.connect_on_startup = place(
        control(
            BUTTON_CLASS_NAME,
            "Connect on startup",
            checkbox,
            0,
            OPTIONS_CONNECT_ON_STARTUP_ID,
        )?,
        PAGE_X,
        page_top,
        page_width,
        20,
    );
    state
        .page_controls
        .push((OptionsPage::ConnectOptions, state.fields.connect_on_startup));

    state.fields.timestamp_show = place(
        control(
            BUTTON_CLASS_NAME,
            "Timestamp events",
            checkbox,
            0,
            OPTIONS_TIMESTAMP_SHOW_ID,
        )?,
        PAGE_X,
        page_top,
        page_width,
        20,
    );
    let format_label = place(
        control(STATIC_CLASS_NAME, "Format:", 0, 0, 0)?,
        PAGE_X,
        page_top + 32,
        64,
        20,
    );
    state.fields.timestamp_format = place(
        control(
            EDIT_CLASS_NAME,
            "",
            edit,
            WS_EX_CLIENTEDGE,
            OPTIONS_TIMESTAMP_FORMAT_ID,
        )?,
        PAGE_X + 68,
        page_top + 30,
        160,
        24,
    );
    let tokens = place(
        control(
            STATIC_CLASS_NAME,
            &format!("Tokens: {}", format_tokens()),
            0,
            0,
            OPTIONS_TIMESTAMP_TOKENS_ID,
        )?,
        PAGE_X,
        page_top + 64,
        page_width,
        40,
    );
    for control in [
        state.fields.timestamp_show,
        format_label,
        state.fields.timestamp_format,
        tokens,
    ] {
        state
            .page_controls
            .push((OptionsPage::IrcMessages, control));
    }

    let buttons_label = place(
        control(STATIC_CLASS_NAME, "Switchbar buttons:", 0, 0, 0)?,
        PAGE_X,
        page_top + 2,
        140,
        20,
    );
    state.fields.switchbar_buttons = place(
        control(
            EDIT_CLASS_NAME,
            "",
            edit | ES_NUMBER as u32,
            WS_EX_CLIENTEDGE,
            OPTIONS_SWITCHBAR_BUTTONS_ID,
        )?,
        PAGE_X + 144,
        page_top,
        56,
        24,
    );
    let pixels_label = place(
        control(STATIC_CLASS_NAME, "pixels wide (40-400)", 0, 0, 0)?,
        PAGE_X + 208,
        page_top + 2,
        page_width - 208,
        20,
    );
    for control in [buttons_label, state.fields.switchbar_buttons, pixels_label] {
        state.page_controls.push((OptionsPage::Display, control));
    }

    let list_width = page_width - BUTTON_WIDTH - MARGIN;
    let list_top = page_top + 24;
    let list_height = 150;
    let word_top = list_top + list_height + MARGIN;
    let highlight_label = place(
        control(
            STATIC_CLASS_NAME,
            "Highlight messages that contain these words:",
            0,
            0,
            0,
        )?,
        PAGE_X,
        page_top + 2,
        page_width,
        20,
    );
    let controls = &mut state.highlight_controls;
    controls.list = place(
        control(
            LISTBOX_CLASS_NAME,
            "",
            WS_BORDER | WS_TABSTOP | WS_VSCROLL | (LBS_NOTIFY | LBS_NOINTEGRALHEIGHT) as u32,
            0,
            OPTIONS_HIGHLIGHT_LIST_ID,
        )?,
        PAGE_X,
        list_top,
        list_width,
        list_height,
    );
    controls.delete = place(
        control(
            BUTTON_CLASS_NAME,
            "&Delete",
            WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            OPTIONS_HIGHLIGHT_DELETE_ID,
        )?,
        PAGE_X + list_width + MARGIN,
        list_top,
        BUTTON_WIDTH,
        BUTTON_HEIGHT,
    );
    controls.word = place(
        control(
            EDIT_CLASS_NAME,
            "",
            edit,
            WS_EX_CLIENTEDGE,
            OPTIONS_HIGHLIGHT_WORD_ID,
        )?,
        PAGE_X,
        word_top,
        list_width,
        24,
    );
    controls.add = place(
        control(
            BUTTON_CLASS_NAME,
            "&Add",
            WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            OPTIONS_HIGHLIGHT_ADD_ID,
        )?,
        PAGE_X + list_width + MARGIN,
        word_top - 1,
        BUTTON_WIDTH,
        BUTTON_HEIGHT,
    );
    let highlight_note = place(
        control(
            STATIC_CLASS_NAME,
            match state.highlight {
                Some(_) => HIGHLIGHT_NOTE,
                None => HIGHLIGHT_SIGNED_OUT_NOTE,
            },
            0,
            0,
            OPTIONS_HIGHLIGHT_NOTE_ID,
        )?,
        PAGE_X,
        word_top + 32,
        page_width,
        48,
    );
    let controls = &state.highlight_controls;
    for control in [
        highlight_label,
        controls.list,
        controls.delete,
        controls.word,
        controls.add,
        highlight_note,
    ] {
        state
            .page_controls
            .push((OptionsPage::IrcHighlight, control));
    }
    for control in [controls.list, controls.word, controls.add] {
        EnableWindow(control, state.highlight.is_some().into());
    }
    refill_highlight_list(state);

    // OK is `IDOK` and Cancel `IDCANCEL`: what `IsDialogMessageW` sends
    // for Enter and Esc.
    place(
        control(
            BUTTON_CLASS_NAME,
            "OK",
            WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
            0,
            IDOK as u16,
        )?,
        client.right - MARGIN - 2 * BUTTON_WIDTH - MARGIN,
        buttons_y,
        BUTTON_WIDTH,
        BUTTON_HEIGHT,
    );
    place(
        control(
            BUTTON_CLASS_NAME,
            "Cancel",
            WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            IDCANCEL as u16,
        )?,
        client.right - MARGIN - BUTTON_WIDTH,
        buttons_y,
        BUTTON_WIDTH,
        BUTTON_HEIGHT,
    );

    let form = OptionsForm::from_settings(&state.current);
    let check = |checked: bool| (if checked { BST_CHECKED } else { BST_UNCHECKED }) as WPARAM;
    SendMessageW(
        state.fields.connect_on_startup,
        BM_SETCHECK,
        check(form.connect_on_startup),
        0,
    );
    SendMessageW(
        state.fields.timestamp_show,
        BM_SETCHECK,
        check(form.timestamp_show),
        0,
    );
    set_window_text(state.fields.timestamp_format, &form.timestamp_format);
    set_window_text(state.fields.switchbar_buttons, &form.switchbar_buttons);

    // Selecting the page's item shows it, through TVN_SELCHANGEDW.
    select_page(hwnd, FIRST_PAGE);
    Ok(())
}

unsafe fn select_page(hwnd: HWND, page: OptionsPage) {
    select_tree_index(hwnd, page_tree_index(page));
}

/// Selects the tree item at `path` (`IRC/Messages`), as a click on it does.
pub(super) unsafe fn select_path(hwnd: HWND, path: &str) -> Result<(), String> {
    let index = tree_index(path).ok_or_else(|| {
        format!(
            "the Options tree has no {path:?}; its items are {:?}",
            tree_paths()
        )
    })?;
    select_tree_index(hwnd, index);
    Ok(())
}

/// Selecting an item shows its page, through TVN_SELCHANGEDW.
unsafe fn select_tree_index(hwnd: HWND, index: usize) {
    let state = dialog_state(hwnd);
    SendMessageW(
        state.tree,
        TVM_SELECTITEM,
        TVGN_CARET as WPARAM,
        state.tree_items[index],
    );
}

/// The tree's selection moved: a category without a page of its own
/// passes it on to its page, anything else shows its page.
unsafe fn handle_selection(hwnd: HWND) {
    let state = dialog_state(hwnd);
    let item = selected_tree_item(state.tree).expect("a selection change selects an item");
    let index = state
        .tree_items
        .iter()
        .position(|tree_item| *tree_item == item)
        .expect("the options tree holds only its own items");
    let page = OPTIONS_TREE[index].2;
    if index != page_tree_index(page) {
        select_page(hwnd, page);
        return;
    }
    for (control_page, control) in &state.page_controls {
        ShowWindow(
            *control,
            if *control_page == page {
                SW_SHOW
            } else {
                SW_HIDE
            },
        );
    }
}

unsafe fn show_error(hwnd: HWND, message: &str) {
    let text = wide_null(message);
    let title = wide_null(OPTIONS_DIALOG_TITLE);
    MessageBoxW(hwnd, text.as_ptr(), title.as_ptr(), MB_ICONERROR | MB_OK);
}

/// OK: saves the form when every field is right; otherwise shows the first
/// wrong field and says what is wrong with it.
unsafe fn handle_ok(hwnd: HWND) {
    let state = dialog_state(hwnd);
    match state.fields.read().apply(&state.current) {
        Ok(settings) => {
            state.saved = Some(OptionsResult {
                settings,
                highlight_changes: state
                    .highlight
                    .as_ref()
                    .map(HighlightList::changes)
                    .unwrap_or_default(),
            });
            DestroyWindow(hwnd);
        }
        Err(error) => {
            select_page(hwnd, error.field.page());
            let field = state.fields.field(error.field);
            SetFocus(field);
            SendMessageW(field, EM_SETSEL, 0, -1);
            show_error(hwnd, &error.message);
        }
    }
}

/// Fills the list from the edited words. Nothing is selected afterwards,
/// so Delete is greyed until something is.
unsafe fn refill_highlight_list(state: &DialogState) {
    let controls = &state.highlight_controls;
    SendMessageW(controls.list, LB_RESETCONTENT, 0, 0);
    for item in state.highlight.iter().flat_map(HighlightList::items) {
        let text = wide_null(&item);
        SendMessageW(controls.list, LB_ADDSTRING, 0, text.as_ptr() as LPARAM);
    }
    EnableWindow(controls.delete, 0);
}

fn highlight_list(state: &mut DialogState) -> &mut HighlightList {
    state
        .highlight
        .as_mut()
        .expect("the Highlight page's controls work only while signed in")
}

/// Add (or Enter in the word box): adds the typed words, or says what is
/// wrong and keeps the text to fix.
unsafe fn handle_highlight_add(hwnd: HWND) {
    let state = dialog_state(hwnd);
    let word = state.highlight_controls.word;
    match highlight_list(state).add(&read_window_text(word)) {
        Ok(()) => {
            refill_highlight_list(state);
            set_window_text(word, "");
            SetFocus(word);
        }
        Err(message) => {
            show_error(hwnd, &message);
            SetFocus(word);
        }
    }
}

unsafe fn selected_highlight_index(state: &DialogState) -> Option<usize> {
    let index = SendMessageW(state.highlight_controls.list, LB_GETCURSEL, 0, 0);
    (index != LB_ERR as LRESULT).then_some(index as usize)
}

/// Delete, which is greyed without a selection.
unsafe fn handle_highlight_delete(hwnd: HWND) {
    let state = dialog_state(hwnd);
    let index =
        selected_highlight_index(state).expect("Delete is greyed while nothing is selected");
    highlight_list(state).delete(index);
    refill_highlight_list(state);
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
        // Enter in the word box is Add, not OK.
        WM_COMMAND
            if wparam & 0xffff == IDOK as usize
                && GetFocus() == dialog_state(hwnd).highlight_controls.word =>
        {
            handle_highlight_add(hwnd);
            0
        }
        WM_COMMAND if wparam & 0xffff == IDOK as usize => {
            handle_ok(hwnd);
            0
        }
        WM_COMMAND if wparam & 0xffff == usize::from(OPTIONS_HIGHLIGHT_ADD_ID) => {
            handle_highlight_add(hwnd);
            0
        }
        WM_COMMAND if wparam & 0xffff == usize::from(OPTIONS_HIGHLIGHT_DELETE_ID) => {
            handle_highlight_delete(hwnd);
            0
        }
        WM_COMMAND
            if wparam & 0xffff == usize::from(OPTIONS_HIGHLIGHT_LIST_ID)
                && (wparam >> 16) as u32 == LBN_SELCHANGE =>
        {
            let state = dialog_state(hwnd);
            EnableWindow(
                state.highlight_controls.delete,
                selected_highlight_index(state).is_some().into(),
            );
            0
        }
        WM_COMMAND if wparam & 0xffff == IDCANCEL as usize => {
            DestroyWindow(hwnd);
            0
        }
        WM_NOTIFY => {
            let header = &*(lparam as *const NMHDR);
            if header.idFrom == usize::from(OPTIONS_TREE_ID) && header.code == TVN_SELCHANGEDW {
                handle_selection(hwnd);
            }
            0
        }
        WM_NCDESTROY => {
            window_data::detach::<DialogState>(hwnd);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// The dialog as automation sees it, read back from its tree and controls.
pub(super) unsafe fn dump(hwnd: HWND) -> Result<UiOptionsDialog, String> {
    let state = dialog_state(hwnd);
    let tree = state.tree;
    let next = |relation, item: isize| {
        SendMessageW(tree, TVM_GETNEXTITEM, relation as WPARAM, item) as isize
    };
    let selected_item = selected_tree_item(tree);
    let mut paths = Vec::new();
    let mut selected = None;
    let mut category = next(TVGN_ROOT, 0);
    while category != 0 {
        let category_path = read_tree_item_text(tree, category)?;
        let mut child = next(TVGN_CHILD, category);
        if selected_item == Some(category) {
            selected = Some(category_path.clone());
        }
        paths.push(category_path.clone());
        while child != 0 {
            let path = format!("{category_path}/{}", read_tree_item_text(tree, child)?);
            if selected_item == Some(child) {
                selected = Some(path.clone());
            }
            paths.push(path);
            child = next(TVGN_NEXT, child);
        }
        category = next(TVGN_NEXT, category);
    }
    let visible_controls = state
        .page_controls
        .iter()
        .filter(|(_, control)| {
            GetWindowLongPtrW(*control, GWL_STYLE) as u32 & WS_VISIBLE != 0
                && GetDlgCtrlID(*control) != 0
        })
        .map(|(_, control)| GetDlgCtrlID(*control) as u16)
        .collect();
    let controls = &state.highlight_controls;
    let highlight_controls_enabled = match [controls.list, controls.word, controls.add]
        .map(|control| IsWindowEnabled(control) != 0)
    {
        [true, true, true] => true,
        [false, false, false] => false,
        enabled => return Err(format!("the Highlight page is half greyed: {enabled:?}")),
    };
    let highlight_items = (0..SendMessageW(controls.list, LB_GETCOUNT, 0, 0) as usize)
        .map(|index| {
            let length = SendMessageW(controls.list, LB_GETTEXTLEN, index, 0) as usize;
            let mut text = vec![0u16; length + 1];
            SendMessageW(
                controls.list,
                LB_GETTEXT,
                index,
                text.as_mut_ptr() as LPARAM,
            );
            String::from_utf16_lossy(&text[..length])
        })
        .collect();
    Ok(UiOptionsDialog {
        tree: paths,
        selected: selected.ok_or_else(|| "the options tree has no selection".to_owned())?,
        visible_controls,
        highlight_items,
        highlight_controls_enabled,
        highlight_delete_enabled: IsWindowEnabled(controls.delete) != 0,
    })
}
