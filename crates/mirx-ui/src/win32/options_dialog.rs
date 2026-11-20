//! The Options dialog (Tools > Options..., Alt+O): mIRC's category tree on
//! the left, the selected page on the right, OK and Cancel.
//!
//! The tree and the form come from `options`; this module only shows them.
//! The dialog is modal over the frame and runs its own message loop, which
//! `IsDialogMessageW` gives a dialog's keys: Tab between fields, Enter for
//! OK (Add in the Highlight page's word box) and Esc for Cancel.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_HIGHLIGHT, COLOR_WINDOW, DrawFocusRect, FillRect, GetSysColorBrush, HBRUSH, SetBkMode,
    TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    BST_CHECKED, BST_UNCHECKED, DRAWITEMSTRUCT, EM_SETSEL, MEASUREITEMSTRUCT, NMHDR, ODS_FOCUS,
    ODS_SELECTED, ODT_COMBOBOX, TVGN_CARET, TVGN_CHILD, TVGN_NEXT, TVGN_ROOT, TVM_GETNEXTITEM,
    TVM_SELECTITEM, TVN_SELCHANGEDW, TVS_HASBUTTONS, TVS_HASLINES, TVS_LINESATROOT,
    TVS_SHOWSELALWAYS,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, IsWindowEnabled, SetFocus,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CB_ADDSTRING,
    CB_GETCURSEL, CB_SETCURSEL, CBS_DROPDOWNLIST, CBS_OWNERDRAWFIXED, DefWindowProcW,
    DestroyWindow, ES_NUMBER, GWL_STYLE, GetDlgCtrlID, GetWindowLongPtrW, IDCANCEL, IDOK,
    LB_ADDSTRING, LB_ERR, LB_GETCOUNT, LB_GETCURSEL, LB_GETTEXT, LB_GETTEXTLEN, LB_RESETCONTENT,
    LBN_SELCHANGE, LBS_NOINTEGRALHEIGHT, LBS_NOTIFY, MB_ICONERROR, MB_OK, MessageBoxW, MoveWindow,
    SW_HIDE, SW_SHOW, SendMessageW, ShowWindow, WM_COMMAND, WM_DRAWITEM, WM_MEASUREITEM,
    WM_NCCREATE, WM_NCDESTROY, WM_NOTIFY, WS_CHILD, WS_EX_CLIENTEDGE, WS_TABSTOP, WS_VISIBLE,
    WS_VSCROLL,
};

use mirx_core::HighlightWord;

use super::automation_dump::dump_activity_colors;
use super::colors::{PALETTE_COLORS, PaletteIndex, paint_palette_cells};
use super::dialogs::{
    DIALOG_BUTTON_HEIGHT, DIALOG_BUTTON_WIDTH, DIALOG_CHECKBOX_HEIGHT, DIALOG_EDIT_EX_STYLE,
    DIALOG_EDIT_HEIGHT, DIALOG_EDIT_STYLE, DIALOG_GAP, DIALOG_LABEL_GAP, DIALOG_MARGIN,
    controls_extent, create_modal_dialog, dialog_text_size,
};
use super::options::{
    FIRST_PAGE, HighlightChanges, HighlightList, OPTIONS_TREE, OptionsField, OptionsForm,
    OptionsPage, page_tree_index, tree_index, tree_paths,
};
use super::settings::{ActivityColors, Settings};
use super::timestamp::format_tokens;
use super::window_data::{self, WindowData};
use super::{
    BUTTON_CLASS_NAME, COMBOBOX_CLASS_NAME, EDIT_CLASS_NAME, STATIC_CLASS_NAME, TREE_CLASS_NAME,
    create_child_window, expand_tree_item, insert_tree_item, read_tree_item_text, read_window_text,
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
/// The switchbar's Event, Message and Highlight colours: 6302-6304.
pub(super) const OPTIONS_ACTIVITY_COLOR_IDS: [u16; 3] = [6302, 6303, 6304];
pub(super) const OPTIONS_HIGHLIGHT_LIST_ID: u16 = 6401;
pub(super) const OPTIONS_HIGHLIGHT_WORD_ID: u16 = 6402;
pub(super) const OPTIONS_HIGHLIGHT_ADD_ID: u16 = 6403;
pub(super) const OPTIONS_HIGHLIGHT_DELETE_ID: u16 = 6404;
pub(super) const OPTIONS_HIGHLIGHT_NOTE_ID: u16 = 6405;
const LISTBOX_CLASS_NAME: &str = "LISTBOX";
const HIGHLIGHT_NOTE: &str = "Words are matched as whole words; * and ? are wildcards. \
Highlight words are kept on your Matrix server and shared with your other clients.";
const HIGHLIGHT_SIGNED_OUT_NOTE: &str = "Connect to edit highlight words.";

const TREE_WIDTH: i32 = 150;
const TREE_HEIGHT: i32 = 260;
const PAGE_X: i32 = DIALOG_MARGIN + TREE_WIDTH + DIALOG_MARGIN;
const PAGE_WIDTH: i32 = 320;
const TIMESTAMP_FORMAT_WIDTH: i32 = 120;
/// Three digits.
const SWITCHBAR_BUTTONS_WIDTH: i32 = 40;
const HIGHLIGHT_LIST_HEIGHT: i32 = 130;
/// A colour drop-down's item: a numbered palette box, as tall as the
/// closed drop-down is with an edit field's height.
const ACTIVITY_COLOR_ITEM_HEIGHT: i32 = 15;
const ACTIVITY_COLOR_WIDTH: i32 = 60;
/// The open list shows half the palette.
const ACTIVITY_COLOR_DROPPED_ITEMS: i32 = 8;

/// The form's controls.
struct Fields {
    connect_on_startup: HWND,
    timestamp_show: HWND,
    timestamp_format: HWND,
    switchbar_buttons: HWND,
    /// Event, Message, Highlight.
    activity_colors: [HWND; 3],
}

impl Fields {
    unsafe fn read(&self) -> OptionsForm {
        let checked = |button| SendMessageW(button, BM_GETCHECK, 0, 0) == BST_CHECKED as LRESULT;
        OptionsForm {
            connect_on_startup: checked(self.connect_on_startup),
            timestamp_show: checked(self.timestamp_show),
            timestamp_format: read_window_text(self.timestamp_format),
            switchbar_buttons: read_window_text(self.switchbar_buttons),
            activity_colors: self.read_activity_colors(),
        }
    }

    unsafe fn read_activity_colors(&self) -> ActivityColors {
        let [event, message, highlight] = self.activity_colors.map(|combo| {
            let index = SendMessageW(combo, CB_GETCURSEL, 0, 0);
            palette_index(usize::try_from(index).expect("a colour drop-down always has a pick"))
        });
        ActivityColors {
            event,
            message,
            highlight,
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
                activity_colors: [null_mut(); 3],
            },
            saved: None,
        }));
        let hwnd = create_modal_dialog(
            owner,
            OPTIONS_DIALOG_CLASS_NAME,
            OPTIONS_DIALOG_TITLE,
            (&mut *state as *mut WindowData<DialogState>).cast(),
            |hwnd| create_controls(hwnd),
        )?;
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
        SetFocus(state.tree);
        run_modal_dialog(owner, hwnd);
        state.saved.take()
    }
}

unsafe fn dialog_state(hwnd: HWND) -> &'static mut DialogState {
    window_data::get(hwnd).expect("the options dialog has its state")
}

unsafe fn create_controls(hwnd: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
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
    // A label as large as its text wrapped to the page.
    let label = |text: &str, id: u16, style: u32, x: i32, y: i32| {
        let (width, height) = dialog_text_size(text, PAGE_WIDTH);
        let label = place(
            control(STATIC_CLASS_NAME, text, style, 0, id)?,
            x,
            y,
            width,
            height,
        );
        Ok::<_, String>((label, (width, height)))
    };
    // A label left of the field on the row at `row_y`, centred on it.
    let label_beside_field = |text: &str, x: i32, row_y: i32| {
        let (_, height) = dialog_text_size(text, PAGE_WIDTH);
        let (label, (width, _)) = label(text, 0, 0, x, row_y + (DIALOG_EDIT_HEIGHT - height) / 2)?;
        Ok::<_, String>((label, width))
    };
    let state = dialog_state(hwnd);

    // mIRC labels its tree, which names the shown page; there is no
    // page heading.
    let (_, (_, category_height)) =
        label("Category:", 0, WS_VISIBLE, DIALOG_MARGIN, DIALOG_MARGIN)?;
    let tree_top = DIALOG_MARGIN + category_height + DIALOG_LABEL_GAP;
    state.tree = place(
        control(
            TREE_CLASS_NAME,
            "",
            WS_VISIBLE
                | WS_TABSTOP
                | TVS_HASLINES
                | TVS_HASBUTTONS
                | TVS_LINESATROOT
                | TVS_SHOWSELALWAYS,
            WS_EX_CLIENTEDGE,
            OPTIONS_TREE_ID,
        )?,
        DIALOG_MARGIN,
        tree_top,
        TREE_WIDTH,
        TREE_HEIGHT,
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
        PAGE_WIDTH,
        DIALOG_CHECKBOX_HEIGHT,
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
        PAGE_WIDTH,
        DIALOG_CHECKBOX_HEIGHT,
    );
    let format_y = page_top + DIALOG_CHECKBOX_HEIGHT + DIALOG_GAP;
    let (format_label, format_label_width) = label_beside_field("Format:", PAGE_X, format_y)?;
    state.fields.timestamp_format = place(
        control(
            EDIT_CLASS_NAME,
            "",
            DIALOG_EDIT_STYLE,
            DIALOG_EDIT_EX_STYLE,
            OPTIONS_TIMESTAMP_FORMAT_ID,
        )?,
        PAGE_X + format_label_width + DIALOG_GAP,
        format_y,
        TIMESTAMP_FORMAT_WIDTH,
        DIALOG_EDIT_HEIGHT,
    );
    let (tokens, _) = label(
        &format!("Tokens: {}", format_tokens()),
        OPTIONS_TIMESTAMP_TOKENS_ID,
        0,
        PAGE_X,
        format_y + DIALOG_EDIT_HEIGHT + DIALOG_GAP,
    )?;
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

    let (buttons_label, buttons_label_width) =
        label_beside_field("Switchbar buttons:", PAGE_X, page_top)?;
    let buttons_x = PAGE_X + buttons_label_width + DIALOG_GAP;
    state.fields.switchbar_buttons = place(
        control(
            EDIT_CLASS_NAME,
            "",
            DIALOG_EDIT_STYLE | ES_NUMBER as u32,
            DIALOG_EDIT_EX_STYLE,
            OPTIONS_SWITCHBAR_BUTTONS_ID,
        )?,
        buttons_x,
        page_top,
        SWITCHBAR_BUTTONS_WIDTH,
        DIALOG_EDIT_HEIGHT,
    );
    let (pixels_label, _) = label_beside_field(
        "pixels wide (40-400)",
        buttons_x + SWITCHBAR_BUTTONS_WIDTH + DIALOG_GAP,
        page_top,
    )?;
    for control in [buttons_label, state.fields.switchbar_buttons, pixels_label] {
        state.page_controls.push((OptionsPage::Display, control));
    }
    // mIRC's switchbar colours, one row each under the buttons' width.
    let activity_labels = ["Event:", "Message:", "Highlight:"];
    let activity_x = PAGE_X
        + activity_labels
            .iter()
            .map(|text| dialog_text_size(text, PAGE_WIDTH).0)
            .max()
            .expect("there are three labels")
        + DIALOG_GAP;
    for (row, (text, id)) in activity_labels
        .into_iter()
        .zip(OPTIONS_ACTIVITY_COLOR_IDS)
        .enumerate()
    {
        let row_y = page_top + (row as i32 + 1) * (DIALOG_EDIT_HEIGHT + DIALOG_GAP);
        let (row_label, _) = label_beside_field(text, PAGE_X, row_y)?;
        let combo = place(
            control(
                COMBOBOX_CLASS_NAME,
                "",
                WS_TABSTOP | WS_VSCROLL | (CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED) as u32,
                0,
                id,
            )?,
            activity_x,
            row_y,
            ACTIVITY_COLOR_WIDTH,
            // A drop-down's height includes its open list.
            DIALOG_EDIT_HEIGHT + ACTIVITY_COLOR_DROPPED_ITEMS * ACTIVITY_COLOR_ITEM_HEIGHT + 2,
        );
        // Without CBS_HASSTRINGS an item is its data: the palette index.
        for index in 0..PALETTE_COLORS {
            SendMessageW(combo, CB_ADDSTRING, 0, index as LPARAM);
        }
        state.fields.activity_colors[row] = combo;
        for control in [row_label, combo] {
            state.page_controls.push((OptionsPage::Display, control));
        }
    }

    let (highlight_label, (_, highlight_label_height)) = label(
        "Highlight messages that contain these words:",
        0,
        0,
        PAGE_X,
        page_top,
    )?;
    let list_width = PAGE_WIDTH - DIALOG_BUTTON_WIDTH - DIALOG_GAP;
    let side_x = PAGE_X + list_width + DIALOG_GAP;
    let list_top = page_top + highlight_label_height + DIALOG_LABEL_GAP;
    let word_top = list_top + HIGHLIGHT_LIST_HEIGHT + DIALOG_GAP;
    let controls = &mut state.highlight_controls;
    controls.list = place(
        control(
            LISTBOX_CLASS_NAME,
            "",
            WS_TABSTOP | WS_VSCROLL | (LBS_NOTIFY | LBS_NOINTEGRALHEIGHT) as u32,
            WS_EX_CLIENTEDGE,
            OPTIONS_HIGHLIGHT_LIST_ID,
        )?,
        PAGE_X,
        list_top,
        list_width,
        HIGHLIGHT_LIST_HEIGHT,
    );
    controls.delete = place(
        control(
            BUTTON_CLASS_NAME,
            "&Delete",
            WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            OPTIONS_HIGHLIGHT_DELETE_ID,
        )?,
        side_x,
        list_top,
        DIALOG_BUTTON_WIDTH,
        DIALOG_BUTTON_HEIGHT,
    );
    controls.word = place(
        control(
            EDIT_CLASS_NAME,
            "",
            DIALOG_EDIT_STYLE,
            DIALOG_EDIT_EX_STYLE,
            OPTIONS_HIGHLIGHT_WORD_ID,
        )?,
        PAGE_X,
        word_top,
        list_width,
        DIALOG_EDIT_HEIGHT,
    );
    controls.add = place(
        control(
            BUTTON_CLASS_NAME,
            "&Add",
            WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            OPTIONS_HIGHLIGHT_ADD_ID,
        )?,
        side_x,
        word_top + (DIALOG_EDIT_HEIGHT - DIALOG_BUTTON_HEIGHT) / 2,
        DIALOG_BUTTON_WIDTH,
        DIALOG_BUTTON_HEIGHT,
    );
    let (highlight_note, _) = label(
        match state.highlight {
            Some(_) => HIGHLIGHT_NOTE,
            None => HIGHLIGHT_SIGNED_OUT_NOTE,
        },
        OPTIONS_HIGHLIGHT_NOTE_ID,
        0,
        PAGE_X,
        word_top + DIALOG_EDIT_HEIGHT + DIALOG_GAP,
    )?;
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
    // for Enter and Esc. They sit a margin under the tree and every page,
    // right-aligned with the pages.
    let (_, pages_bottom) = controls_extent(hwnd);
    let buttons_y = pages_bottom + DIALOG_MARGIN;
    let cancel_x = PAGE_X + PAGE_WIDTH - DIALOG_BUTTON_WIDTH;
    place(
        control(
            BUTTON_CLASS_NAME,
            "OK",
            WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
            0,
            IDOK as u16,
        )?,
        cancel_x - DIALOG_GAP - DIALOG_BUTTON_WIDTH,
        buttons_y,
        DIALOG_BUTTON_WIDTH,
        DIALOG_BUTTON_HEIGHT,
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
        DIALOG_BUTTON_WIDTH,
        DIALOG_BUTTON_HEIGHT,
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
    let ActivityColors {
        event,
        message,
        highlight,
    } = form.activity_colors;
    for (combo, index) in state
        .fields
        .activity_colors
        .into_iter()
        .zip([event, message, highlight])
    {
        SendMessageW(combo, CB_SETCURSEL, usize::from(index.get()), 0);
    }

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

fn palette_index(index: usize) -> PaletteIndex {
    PaletteIndex::new(u8::try_from(index).expect("a colour drop-down holds the palette"))
        .expect("a colour drop-down holds the palette")
}

/// A colour drop-down's item: its numbered palette box on the list's
/// background, or the selection's.
unsafe fn draw_activity_color(item: &DRAWITEMSTRUCT) {
    assert_eq!(
        item.CtlType, ODT_COMBOBOX,
        "only the colour drop-downs are owner-drawn"
    );
    let background = match item.itemState & ODS_SELECTED {
        0 => COLOR_WINDOW,
        _ => COLOR_HIGHLIGHT,
    };
    FillRect(
        item.hDC,
        &item.rcItem,
        GetSysColorBrush(background) as HBRUSH,
    );
    let cell = RECT {
        left: item.rcItem.left + 2,
        top: item.rcItem.top + 1,
        right: item.rcItem.right - 2,
        bottom: item.rcItem.bottom - 1,
    };
    let index = palette_index(item.itemID as usize);
    let mode = SetBkMode(item.hDC, TRANSPARENT as i32);
    paint_palette_cells(item.hDC, &[cell], index, None, true);
    SetBkMode(item.hDC, mode);
    if item.itemState & ODS_FOCUS != 0 {
        DrawFocusRect(item.hDC, &item.rcItem);
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

pub(super) unsafe extern "system" fn window_proc(
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
        WM_MEASUREITEM => {
            let item = &mut *(lparam as *mut MEASUREITEMSTRUCT);
            assert_eq!(
                item.CtlType, ODT_COMBOBOX,
                "only the colour drop-downs are owner-drawn"
            );
            item.itemHeight = ACTIVITY_COLOR_ITEM_HEIGHT as u32;
            1
        }
        WM_DRAWITEM => {
            draw_activity_color(&*(lparam as *const DRAWITEMSTRUCT));
            1
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
        activity_colors: dump_activity_colors(&state.fields.read_activity_colors()),
    })
}
