//! The Options dialog (Tools > Options..., Alt+O): mIRC's category tree on
//! the left, the selected page on the right, OK and Cancel.
//!
//! The tree and the form come from `options`; this module only shows them.
//! The dialog is modal over the frame and runs its own message loop, which
//! `IsDialogMessageW` gives a dialog's keys: Tab between fields, Enter for
//! OK (Add in the Highlight page's word box) and Esc for Cancel.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    BST_CHECKED, BST_UNCHECKED, DRAWITEMSTRUCT, EM_SETSEL, MEASUREITEMSTRUCT, NMHDR, TVGN_CARET,
    TVGN_CHILD, TVGN_NEXT, TVGN_ROOT, TVM_GETNEXTITEM, TVM_SELECTITEM, TVN_SELCHANGEDW,
    TVS_HASBUTTONS, TVS_HASLINES, TVS_LINESATROOT, TVS_SHOWSELALWAYS,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, IsWindowEnabled, SetFocus,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_PUSHBUTTON,
    DefWindowProcW, ES_NUMBER, GWL_STYLE, GetDlgCtrlID, GetWindowLongPtrW, IDCANCEL, IDOK,
    LB_ADDSTRING, LB_ERR, LB_GETCOUNT, LB_GETCURSEL, LB_GETTEXT, LB_GETTEXTLEN, LB_RESETCONTENT,
    LBN_SELCHANGE, LBS_NOINTEGRALHEIGHT, LBS_NOTIFY, MB_ICONERROR, MB_OK, MessageBoxW, MoveWindow,
    SW_HIDE, SW_SHOW, SendMessageW, ShowWindow, WM_COMMAND, WM_DRAWITEM, WM_MEASUREITEM,
    WM_NCCREATE, WM_NCDESTROY, WM_NOTIFY, WS_CHILD, WS_EX_CLIENTEDGE, WS_TABSTOP, WS_VISIBLE,
    WS_VSCROLL,
};

use mirx_core::HighlightWord;

use super::automation_dump::dump_activity_colors;
use super::color_drop_down::{
    COLOR_DROP_DOWN_STYLE, COLOR_DROP_DOWN_WIDTH, color_drop_down_height, draw_color_item,
    fill_color_drop_down, measure_color_item, select_color, selected_color,
};
use super::colors::PaletteIndex;
use super::dialogs::{
    DIALOG_EDIT_EX_STYLE, DIALOG_EDIT_STYLE, DialogMetrics, controls_extent, create_modal_dialog,
    dialog_text_size, end_modal_dialog,
};
use super::dpi::Dpi;
use super::logging::logs_folder_label;
use super::options::{
    FIRST_PAGE, HighlightChanges, HighlightList, OPTIONS_TREE, OptionsField, OptionsForm,
    OptionsPage, page_tree_index, tree_index, tree_paths,
};
use super::perform::PerformSetting;
use super::perform_dialog::run_perform_dialog;
use super::settings::{
    ActivityColors, IrcSetting, LoggingSetting, MarkerSetting, Settings, TipsSetting, TraySetting,
};
use super::timestamp::format_tokens;
use super::window_data::{self, WindowData};
use super::{
    BUTTON_CLASS_NAME, COMBOBOX_CLASS_NAME, EDIT_CLASS_NAME, STATIC_CLASS_NAME, TREE_CLASS_NAME,
    create_child_window, expand_tree_item, insert_tree_item, read_tree_item_text, read_window_text,
    run_modal_dialog, selected_tree_item, set_window_text, wide_null,
};
use crate::automation::{
    UiConnectOptions, UiIrcOptions, UiLoggingOptions, UiMarkerOptions, UiOptionsDialog,
    UiTipsOptions, UiTrayOptions,
};

pub(super) const OPTIONS_DIALOG_CLASS_NAME: &str = "mirxWin32OptionsDialog";
const OPTIONS_DIALOG_TITLE: &str = "mirx Options";
pub(super) const OPTIONS_TREE_ID: u16 = 6001;
pub(super) const OPTIONS_CONNECT_ON_STARTUP_ID: u16 = 6101;
pub(super) const OPTIONS_RECONNECT_ID: u16 = 6102;
pub(super) const OPTIONS_RETRY_ID: u16 = 6103;
pub(super) const OPTIONS_RETRY_TIMES_ID: u16 = 6104;
pub(super) const OPTIONS_RETRY_DELAY_ID: u16 = 6105;
pub(super) const OPTIONS_PERFORM_ID: u16 = 6106;
pub(super) const OPTIONS_TIMESTAMP_SHOW_ID: u16 = 6201;
pub(super) const OPTIONS_TIMESTAMP_FORMAT_ID: u16 = 6202;
pub(super) const OPTIONS_TIMESTAMP_TOKENS_ID: u16 = 6203;
pub(super) const OPTIONS_SEND_TYPING_ID: u16 = 6204;
pub(super) const OPTIONS_SWITCHBAR_BUTTONS_ID: u16 = 6301;
/// The switchbar's Event, Message and Highlight colours: 6302-6304.
pub(super) const OPTIONS_ACTIVITY_COLOR_IDS: [u16; 3] = [6302, 6303, 6304];
pub(super) const OPTIONS_MARKER_SHOW_ID: u16 = 6305;
pub(super) const OPTIONS_MARKER_COLOR_ID: u16 = 6306;
pub(super) const OPTIONS_HIGHLIGHT_LIST_ID: u16 = 6401;
pub(super) const OPTIONS_HIGHLIGHT_WORD_ID: u16 = 6402;
pub(super) const OPTIONS_HIGHLIGHT_ADD_ID: u16 = 6403;
pub(super) const OPTIONS_HIGHLIGHT_DELETE_ID: u16 = 6404;
pub(super) const OPTIONS_HIGHLIGHT_NOTE_ID: u16 = 6405;
pub(super) const OPTIONS_LOG_CHANNELS_ID: u16 = 6501;
pub(super) const OPTIONS_LOG_QUERIES_ID: u16 = 6502;
pub(super) const OPTIONS_LOG_STRIP_CODES_ID: u16 = 6503;
pub(super) const OPTIONS_LOG_TIMESTAMP_ID: u16 = 6504;
pub(super) const OPTIONS_LOG_FOLDER_ID: u16 = 6505;
pub(super) const OPTIONS_TRAY_ALWAYS_ID: u16 = 6601;
pub(super) const OPTIONS_TRAY_STARTUP_ID: u16 = 6602;
pub(super) const OPTIONS_TRAY_MINIMIZE_ID: u16 = 6603;
pub(super) const OPTIONS_TRAY_SINGLE_CLICK_ID: u16 = 6604;
pub(super) const OPTIONS_TIPS_HIGHLIGHTS_ID: u16 = 6701;
pub(super) const OPTIONS_TIPS_QUERIES_ID: u16 = 6702;
pub(super) const OPTIONS_TIPS_CHANNELS_ID: u16 = 6703;
pub(super) const OPTIONS_TIPS_NOTE_ID: u16 = 6704;
pub(super) const OPTIONS_WINDOW_BUFFER_ID: u16 = 6801;
pub(super) const OPTIONS_IRC_MODE_PREFIX_ID: u16 = 6901;
pub(super) const OPTIONS_IRC_AUTO_JOIN_INVITE_ID: u16 = 6902;
pub(super) const OPTIONS_IRC_ACTIVE_AWAY_ID: u16 = 6903;
pub(super) const OPTIONS_IRC_ACTIVE_INVITES_ID: u16 = 6904;
pub(super) const OPTIONS_IRC_ACTIVE_WHOIS_ID: u16 = 6905;
const LISTBOX_CLASS_NAME: &str = "LISTBOX";
const HIGHLIGHT_NOTE: &str = "Words are matched as whole words; * and ? are wildcards. \
Highlight words are kept on your Matrix server and shared with your other clients.";
const HIGHLIGHT_SIGNED_OUT_NOTE: &str = "Connect to edit highlight words.";
const TIPS_NOTE: &str =
    "Muted rooms never show tips; Mentions and keywords rooms only for highlights.";

// The layout's lengths at 96 dpi.
const TREE_WIDTH: i32 = 150;
const TREE_HEIGHT: i32 = 260;
const PAGE_WIDTH: i32 = 320;
const TIMESTAMP_FORMAT_WIDTH: i32 = 120;
/// A number field: up to four digits.
const NUMBER_FIELD_WIDTH: i32 = 40;
/// The window buffer's field: up to five digits.
const WINDOW_BUFFER_FIELD_WIDTH: i32 = 48;
const HIGHLIGHT_LIST_HEIGHT: i32 = 130;

/// The form's controls.
struct Fields {
    connect_on_startup: HWND,
    reconnect: HWND,
    retry: HWND,
    retry_times: HWND,
    retry_delay: HWND,
    /// The Retry and Delay rows, greyed while retry is off.
    retry_rows: Vec<HWND>,
    timestamp_show: HWND,
    timestamp_format: HWND,
    send_typing: HWND,
    irc_mode_prefix: HWND,
    irc_auto_join_invite: HWND,
    irc_active_away: HWND,
    irc_active_invites: HWND,
    irc_active_whois: HWND,
    switchbar_buttons: HWND,
    /// Event, Message, Highlight.
    activity_colors: [HWND; 3],
    marker_show: HWND,
    marker_color: HWND,
    log_channels: HWND,
    log_queries: HWND,
    log_strip_codes: HWND,
    log_timestamp: HWND,
    tray_always: HWND,
    tray_startup: HWND,
    tray_minimize: HWND,
    tray_single_click: HWND,
    tips_highlights: HWND,
    tips_queries: HWND,
    tips_channels: HWND,
    window_buffer: HWND,
}

impl Fields {
    /// The form the page controls show, with the Perform dialog's
    /// `perform`.
    unsafe fn read(&self, perform: &PerformSetting) -> OptionsForm {
        let checked = |button| SendMessageW(button, BM_GETCHECK, 0, 0) == BST_CHECKED as LRESULT;
        OptionsForm {
            connect_on_startup: checked(self.connect_on_startup),
            reconnect: checked(self.reconnect),
            retry: checked(self.retry),
            retry_times: read_window_text(self.retry_times),
            retry_delay: read_window_text(self.retry_delay),
            timestamp_show: checked(self.timestamp_show),
            timestamp_format: read_window_text(self.timestamp_format),
            send_typing: checked(self.send_typing),
            irc: IrcSetting {
                mode_prefix: checked(self.irc_mode_prefix),
                auto_join_invite: checked(self.irc_auto_join_invite),
                active_away: checked(self.irc_active_away),
                active_invites: checked(self.irc_active_invites),
                active_whois: checked(self.irc_active_whois),
            },
            switchbar_buttons: read_window_text(self.switchbar_buttons),
            activity_colors: self.read_activity_colors(),
            marker: MarkerSetting {
                show: checked(self.marker_show),
                color: read_color(self.marker_color),
            },
            logging: LoggingSetting {
                channels: checked(self.log_channels),
                queries: checked(self.log_queries),
                strip_codes: checked(self.log_strip_codes),
                timestamp: checked(self.log_timestamp),
            },
            tray: TraySetting {
                always: checked(self.tray_always),
                startup: checked(self.tray_startup),
                minimize: checked(self.tray_minimize),
                single_click: checked(self.tray_single_click),
            },
            tips: TipsSetting {
                highlights: checked(self.tips_highlights),
                queries: checked(self.tips_queries),
                channels: checked(self.tips_channels),
            },
            window_buffer: read_window_text(self.window_buffer),
            perform: perform.clone(),
        }
    }

    unsafe fn read_activity_colors(&self) -> ActivityColors {
        let [event, message, highlight] = self.activity_colors.map(|combo| read_color(combo));
        ActivityColors {
            event,
            message,
            highlight,
        }
    }

    fn field(&self, field: OptionsField) -> HWND {
        match field {
            OptionsField::RetryTimes => self.retry_times,
            OptionsField::RetryDelay => self.retry_delay,
            OptionsField::TimestampFormat => self.timestamp_format,
            OptionsField::SwitchbarButtons => self.switchbar_buttons,
            OptionsField::WindowBuffer => self.window_buffer,
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
    /// Perform on connect as its dialog's OK left it, which OK saves.
    perform: PerformSetting,
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
            perform: current.perform.clone(),
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
                reconnect: null_mut(),
                retry: null_mut(),
                retry_times: null_mut(),
                retry_delay: null_mut(),
                retry_rows: Vec::new(),
                timestamp_show: null_mut(),
                timestamp_format: null_mut(),
                send_typing: null_mut(),
                irc_mode_prefix: null_mut(),
                irc_auto_join_invite: null_mut(),
                irc_active_away: null_mut(),
                irc_active_invites: null_mut(),
                irc_active_whois: null_mut(),
                switchbar_buttons: null_mut(),
                activity_colors: [null_mut(); 3],
                marker_show: null_mut(),
                marker_color: null_mut(),
                log_channels: null_mut(),
                log_queries: null_mut(),
                log_strip_codes: null_mut(),
                log_timestamp: null_mut(),
                tray_always: null_mut(),
                tray_startup: null_mut(),
                tray_minimize: null_mut(),
                tray_single_click: null_mut(),
                tips_highlights: null_mut(),
                tips_queries: null_mut(),
                tips_channels: null_mut(),
                window_buffer: null_mut(),
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
    let dpi = Dpi::of_window(hwnd);
    let DialogMetrics {
        margin,
        gap,
        label_gap,
        button_width,
        button_height,
        edit_height,
        checkbox_height,
    } = DialogMetrics::at(dpi);
    let (tree_width, page_width) = (dpi.px(TREE_WIDTH), dpi.px(PAGE_WIDTH));
    let page_x = margin + tree_width + margin;
    let number_field_width = dpi.px(NUMBER_FIELD_WIDTH);
    let window_buffer_field_width = dpi.px(WINDOW_BUFFER_FIELD_WIDTH);
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
        let (width, height) = dialog_text_size(text, page_width);
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
        let (_, height) = dialog_text_size(text, page_width);
        let (label, (width, _)) = label(text, 0, 0, x, row_y + (edit_height - height) / 2)?;
        Ok::<_, String>((label, width))
    };
    let state = dialog_state(hwnd);

    // mIRC labels its tree, which names the shown page; there is no
    // page heading.
    let (_, (_, category_height)) = label("Category:", 0, WS_VISIBLE, margin, margin)?;
    let tree_top = margin + category_height + label_gap;
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
        margin,
        tree_top,
        tree_width,
        dpi.px(TREE_HEIGHT),
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

    // mIRC's Connect > Options, with its Retry... dialog's two fields on
    // the page: OK checks them with the rest.
    let mut row_y = page_top;
    for (field, text, id) in [
        (
            &mut state.fields.connect_on_startup,
            "Connect on startup",
            OPTIONS_CONNECT_ON_STARTUP_ID,
        ),
        (
            &mut state.fields.reconnect,
            "Reconnect on disconnection",
            OPTIONS_RECONNECT_ID,
        ),
        (
            &mut state.fields.retry,
            "Enable connect retry",
            OPTIONS_RETRY_ID,
        ),
    ] {
        *field = place(
            control(BUTTON_CLASS_NAME, text, checkbox, 0, id)?,
            page_x,
            row_y,
            page_width,
            checkbox_height,
        );
        state
            .page_controls
            .push((OptionsPage::ConnectOptions, *field));
        row_y += checkbox_height + gap;
    }
    // The Retry and Delay rows sit under Enable connect retry, their
    // fields lined up.
    let retry_x = page_x + margin;
    let retry_labels = ["Retry:", "Delay:"];
    let retry_field_x = retry_x
        + retry_labels
            .iter()
            .map(|text| dialog_text_size(text, page_width).0)
            .max()
            .expect("there are two labels")
        + gap;
    for ((text, unit), (field, id)) in retry_labels.into_iter().zip(["times", "seconds"]).zip([
        (&mut state.fields.retry_times, OPTIONS_RETRY_TIMES_ID),
        (&mut state.fields.retry_delay, OPTIONS_RETRY_DELAY_ID),
    ]) {
        let (row_label, _) = label_beside_field(text, retry_x, row_y)?;
        *field = place(
            control(
                EDIT_CLASS_NAME,
                "",
                DIALOG_EDIT_STYLE | ES_NUMBER as u32,
                DIALOG_EDIT_EX_STYLE,
                id,
            )?,
            retry_field_x,
            row_y,
            number_field_width,
            edit_height,
        );
        let (unit_label, _) =
            label_beside_field(unit, retry_field_x + number_field_width + gap, row_y)?;
        for control in [row_label, *field, unit_label] {
            state
                .page_controls
                .push((OptionsPage::ConnectOptions, control));
            state.fields.retry_rows.push(control);
        }
        row_y += edit_height + gap;
    }
    // mIRC's Perform... button, under the Retry rows, opens its dialog.
    let perform = place(
        control(
            BUTTON_CLASS_NAME,
            "Perform...",
            WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            OPTIONS_PERFORM_ID,
        )?,
        page_x,
        row_y,
        button_width,
        button_height,
    );
    state
        .page_controls
        .push((OptionsPage::ConnectOptions, perform));

    state.fields.timestamp_show = place(
        control(
            BUTTON_CLASS_NAME,
            "Timestamp events",
            checkbox,
            0,
            OPTIONS_TIMESTAMP_SHOW_ID,
        )?,
        page_x,
        page_top,
        page_width,
        checkbox_height,
    );
    let format_y = page_top + checkbox_height + gap;
    let (format_label, format_label_width) = label_beside_field("Format:", page_x, format_y)?;
    state.fields.timestamp_format = place(
        control(
            EDIT_CLASS_NAME,
            "",
            DIALOG_EDIT_STYLE,
            DIALOG_EDIT_EX_STYLE,
            OPTIONS_TIMESTAMP_FORMAT_ID,
        )?,
        page_x + format_label_width + gap,
        format_y,
        dpi.px(TIMESTAMP_FORMAT_WIDTH),
        edit_height,
    );
    let tokens_y = format_y + edit_height + gap;
    let (tokens, (_, tokens_height)) = label(
        &format!("Tokens: {}", format_tokens()),
        OPTIONS_TIMESTAMP_TOKENS_ID,
        0,
        page_x,
        tokens_y,
    )?;
    state.fields.send_typing = place(
        control(
            BUTTON_CLASS_NAME,
            "Send typing notifications",
            checkbox,
            0,
            OPTIONS_SEND_TYPING_ID,
        )?,
        page_x,
        tokens_y + tokens_height + 2 * gap,
        page_width,
        checkbox_height,
    );
    for control in [
        state.fields.timestamp_show,
        format_label,
        state.fields.timestamp_format,
        tokens,
        state.fields.send_typing,
    ] {
        state
            .page_controls
            .push((OptionsPage::IrcMessages, control));
    }

    let (buttons_label, buttons_label_width) =
        label_beside_field("Switchbar buttons:", page_x, page_top)?;
    let buttons_x = page_x + buttons_label_width + gap;
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
        number_field_width,
        edit_height,
    );
    let (pixels_label, _) = label_beside_field(
        "pixels wide (40-400)",
        buttons_x + number_field_width + gap,
        page_top,
    )?;
    for control in [buttons_label, state.fields.switchbar_buttons, pixels_label] {
        state.page_controls.push((OptionsPage::Display, control));
    }
    // mIRC's switchbar colours, one row each under the buttons' width,
    // then its line marker's checkbox and colour. The colours share a
    // column.
    let color_labels = ["Event:", "Message:", "Highlight:", "Line marker:"];
    let color_x = page_x
        + color_labels
            .iter()
            .map(|text| dialog_text_size(text, page_width).0)
            .max()
            .expect("there are four labels")
        + gap;
    let row_y = |row: i32| page_top + row * (edit_height + gap);
    // A labelled colour drop-down on the row at `row_y`.
    let color_row = |text: &str, id: u16, row_y: i32| {
        let (row_label, _) = label_beside_field(text, page_x, row_y)?;
        let combo = place(
            control(COMBOBOX_CLASS_NAME, "", COLOR_DROP_DOWN_STYLE, 0, id)?,
            color_x,
            row_y,
            dpi.px(COLOR_DROP_DOWN_WIDTH),
            color_drop_down_height(edit_height),
        );
        fill_color_drop_down(combo, false);
        Ok::<_, String>((row_label, combo))
    };
    for (row, (text, id)) in color_labels[..3]
        .iter()
        .zip(OPTIONS_ACTIVITY_COLOR_IDS)
        .enumerate()
    {
        let (row_label, combo) = color_row(text, id, row_y(row as i32 + 1))?;
        state.fields.activity_colors[row] = combo;
        for control in [row_label, combo] {
            state.page_controls.push((OptionsPage::Display, control));
        }
    }
    state.fields.marker_show = place(
        control(
            BUTTON_CLASS_NAME,
            "Show line marker",
            checkbox,
            0,
            OPTIONS_MARKER_SHOW_ID,
        )?,
        page_x,
        row_y(4) + (edit_height - checkbox_height) / 2,
        page_width,
        checkbox_height,
    );
    let (marker_label, marker_color) =
        color_row(color_labels[3], OPTIONS_MARKER_COLOR_ID, row_y(5))?;
    state.fields.marker_color = marker_color;
    for control in [state.fields.marker_show, marker_label, marker_color] {
        state.page_controls.push((OptionsPage::Display, control));
    }

    let (highlight_label, (_, highlight_label_height)) = label(
        "Highlight messages that contain these words:",
        0,
        0,
        page_x,
        page_top,
    )?;
    let list_width = page_width - button_width - gap;
    let side_x = page_x + list_width + gap;
    let list_top = page_top + highlight_label_height + label_gap;
    let word_top = list_top + dpi.px(HIGHLIGHT_LIST_HEIGHT) + gap;
    let controls = &mut state.highlight_controls;
    controls.list = place(
        control(
            LISTBOX_CLASS_NAME,
            "",
            WS_TABSTOP | WS_VSCROLL | (LBS_NOTIFY | LBS_NOINTEGRALHEIGHT) as u32,
            WS_EX_CLIENTEDGE,
            OPTIONS_HIGHLIGHT_LIST_ID,
        )?,
        page_x,
        list_top,
        list_width,
        dpi.px(HIGHLIGHT_LIST_HEIGHT),
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
        button_width,
        button_height,
    );
    controls.word = place(
        control(
            EDIT_CLASS_NAME,
            "",
            DIALOG_EDIT_STYLE,
            DIALOG_EDIT_EX_STYLE,
            OPTIONS_HIGHLIGHT_WORD_ID,
        )?,
        page_x,
        word_top,
        list_width,
        edit_height,
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
        word_top + (edit_height - button_height) / 2,
        button_width,
        button_height,
    );
    let (highlight_note, _) = label(
        match state.highlight {
            Some(_) => HIGHLIGHT_NOTE,
            None => HIGHLIGHT_SIGNED_OUT_NOTE,
        },
        OPTIONS_HIGHLIGHT_NOTE_ID,
        0,
        page_x,
        word_top + edit_height + gap,
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

    // mIRC's Logging page: what logs by itself, and how lines are written.
    let (auto_label, (_, auto_label_height)) = label("Automatically log:", 0, 0, page_x, page_top)?;
    state
        .page_controls
        .push((OptionsPage::IrcLogging, auto_label));
    // A checkbox on the row at `row_y`, which moves to the next row.
    let check_box_row = |text: &str, id: u16, indent: i32, row_y: &mut i32| {
        let check_box = place(
            control(BUTTON_CLASS_NAME, text, checkbox, 0, id)?,
            page_x + indent,
            *row_y,
            page_width - indent,
            checkbox_height,
        );
        *row_y += checkbox_height + gap;
        Ok::<_, String>(check_box)
    };
    let mut row_y = page_top + auto_label_height + label_gap;
    // The kinds of window sit under "Automatically log:".
    state.fields.log_channels =
        check_box_row("Channels", OPTIONS_LOG_CHANNELS_ID, margin, &mut row_y)?;
    state.fields.log_queries =
        check_box_row("Queries", OPTIONS_LOG_QUERIES_ID, margin, &mut row_y)?;
    state.fields.log_strip_codes =
        check_box_row("Strip codes", OPTIONS_LOG_STRIP_CODES_ID, 0, &mut row_y)?;
    state.fields.log_timestamp =
        check_box_row("Timestamp logs", OPTIONS_LOG_TIMESTAMP_ID, 0, &mut row_y)?;
    for control in [
        state.fields.log_channels,
        state.fields.log_queries,
        state.fields.log_strip_codes,
        state.fields.log_timestamp,
    ] {
        state.page_controls.push((OptionsPage::IrcLogging, control));
    }
    let (folder, _) = label(
        &format!("Folder: {}", logs_folder_label()),
        OPTIONS_LOG_FOLDER_ID,
        0,
        page_x,
        row_y,
    )?;
    state.page_controls.push((OptionsPage::IrcLogging, folder));

    // mIRC's IRC page, the options of it Matrix has, in its order. mIRC
    // puts the Show in active boxes in a column at the right; here they
    // sit under their label.
    let mut row_y = page_top;
    state.fields.irc_mode_prefix = check_box_row(
        "Show mode &prefix",
        OPTIONS_IRC_MODE_PREFIX_ID,
        0,
        &mut row_y,
    )?;
    state.fields.irc_auto_join_invite = check_box_row(
        "Auto-join ch&annel on invite",
        OPTIONS_IRC_AUTO_JOIN_INVITE_ID,
        0,
        &mut row_y,
    )?;
    let (active_label, (_, active_label_height)) =
        label("Show in active:", 0, 0, page_x, row_y + gap)?;
    let mut row_y = row_y + gap + active_label_height + label_gap;
    state.fields.irc_active_away =
        check_box_row("Awa&y", OPTIONS_IRC_ACTIVE_AWAY_ID, margin, &mut row_y)?;
    state.fields.irc_active_invites = check_box_row(
        "&Invites",
        OPTIONS_IRC_ACTIVE_INVITES_ID,
        margin,
        &mut row_y,
    )?;
    state.fields.irc_active_whois =
        check_box_row("Whoi&s", OPTIONS_IRC_ACTIVE_WHOIS_ID, margin, &mut row_y)?;
    for control in [
        state.fields.irc_mode_prefix,
        state.fields.irc_auto_join_invite,
        active_label,
        state.fields.irc_active_away,
        state.fields.irc_active_invites,
        state.fields.irc_active_whois,
    ] {
        state.page_controls.push((OptionsPage::Irc, control));
    }

    // mIRC's Tray page: when the icon shows, and what opens mirx from it.
    let mut row_y = page_top;
    state.fields.tray_always = check_box_row(
        "Always show mirx icon in tray",
        OPTIONS_TRAY_ALWAYS_ID,
        0,
        &mut row_y,
    )?;
    state.fields.tray_startup = check_box_row(
        "On startup minimize mirx to tray",
        OPTIONS_TRAY_STARTUP_ID,
        0,
        &mut row_y,
    )?;
    state.fields.tray_minimize = check_box_row(
        "Place mirx in tray when minimized",
        OPTIONS_TRAY_MINIMIZE_ID,
        0,
        &mut row_y,
    )?;
    state.fields.tray_single_click = check_box_row(
        "Single click on tray icon to open",
        OPTIONS_TRAY_SINGLE_CLICK_ID,
        0,
        &mut row_y,
    )?;
    for control in [
        state.fields.tray_always,
        state.fields.tray_startup,
        state.fields.tray_minimize,
        state.fields.tray_single_click,
    ] {
        state
            .page_controls
            .push((OptionsPage::DisplayTray, control));
    }

    // mIRC's Tips page, without its queue size and display time: Windows
    // keeps the notifications' queue and decides how long each shows.
    let (tips_label, (_, tips_label_height)) = label(
        "Show tips when mirx is not the active application for:",
        0,
        0,
        page_x,
        page_top,
    )?;
    let mut row_y = page_top + tips_label_height + label_gap;
    state.fields.tips_highlights =
        check_box_row("Highlights", OPTIONS_TIPS_HIGHLIGHTS_ID, margin, &mut row_y)?;
    state.fields.tips_queries = check_box_row(
        "Query messages",
        OPTIONS_TIPS_QUERIES_ID,
        margin,
        &mut row_y,
    )?;
    state.fields.tips_channels = check_box_row(
        "Channel messages",
        OPTIONS_TIPS_CHANNELS_ID,
        margin,
        &mut row_y,
    )?;
    let (tips_note, _) = label(TIPS_NOTE, OPTIONS_TIPS_NOTE_ID, 0, page_x, row_y)?;
    for control in [
        tips_label,
        state.fields.tips_highlights,
        state.fields.tips_queries,
        state.fields.tips_channels,
        tips_note,
    ] {
        state
            .page_controls
            .push((OptionsPage::DisplayTips, control));
    }

    // mIRC's Options > Other: its Window buffer row.
    let (buffer_label, buffer_label_width) =
        label_beside_field("Window buffer:", page_x, page_top)?;
    let buffer_x = page_x + buffer_label_width + gap;
    state.fields.window_buffer = place(
        control(
            EDIT_CLASS_NAME,
            "",
            DIALOG_EDIT_STYLE | ES_NUMBER as u32,
            DIALOG_EDIT_EX_STYLE,
            OPTIONS_WINDOW_BUFFER_ID,
        )?,
        buffer_x,
        page_top,
        window_buffer_field_width,
        edit_height,
    );
    let (lines_label, _) = label_beside_field(
        "lines (100-50000)",
        buffer_x + window_buffer_field_width + gap,
        page_top,
    )?;
    for control in [buffer_label, state.fields.window_buffer, lines_label] {
        state.page_controls.push((OptionsPage::Other, control));
    }

    // OK is `IDOK` and Cancel `IDCANCEL`: what `IsDialogMessageW` sends
    // for Enter and Esc. They sit a margin under the tree and every page,
    // right-aligned with the pages.
    let (_, pages_bottom) = controls_extent(hwnd);
    let buttons_y = pages_bottom + margin;
    let cancel_x = page_x + page_width - button_width;
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

    let form = OptionsForm::from_settings(&state.current);
    let check = |checked: bool| (if checked { BST_CHECKED } else { BST_UNCHECKED }) as WPARAM;
    for (check_box, checked) in [
        (state.fields.connect_on_startup, form.connect_on_startup),
        (state.fields.reconnect, form.reconnect),
        (state.fields.retry, form.retry),
        (state.fields.timestamp_show, form.timestamp_show),
        (state.fields.send_typing, form.send_typing),
        (state.fields.irc_mode_prefix, form.irc.mode_prefix),
        (state.fields.irc_auto_join_invite, form.irc.auto_join_invite),
        (state.fields.irc_active_away, form.irc.active_away),
        (state.fields.irc_active_invites, form.irc.active_invites),
        (state.fields.irc_active_whois, form.irc.active_whois),
        (state.fields.marker_show, form.marker.show),
        (state.fields.log_channels, form.logging.channels),
        (state.fields.log_queries, form.logging.queries),
        (state.fields.log_strip_codes, form.logging.strip_codes),
        (state.fields.log_timestamp, form.logging.timestamp),
        (state.fields.tray_always, form.tray.always),
        (state.fields.tray_startup, form.tray.startup),
        (state.fields.tray_minimize, form.tray.minimize),
        (state.fields.tray_single_click, form.tray.single_click),
        (state.fields.tips_highlights, form.tips.highlights),
        (state.fields.tips_queries, form.tips.queries),
        (state.fields.tips_channels, form.tips.channels),
    ] {
        SendMessageW(check_box, BM_SETCHECK, check(checked), 0);
    }
    set_window_text(state.fields.retry_times, &form.retry_times);
    set_window_text(state.fields.retry_delay, &form.retry_delay);
    grey_retry_rows(&state.fields);
    set_window_text(state.fields.timestamp_format, &form.timestamp_format);
    set_window_text(state.fields.switchbar_buttons, &form.switchbar_buttons);
    set_window_text(state.fields.window_buffer, &form.window_buffer);
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
        .chain([(state.fields.marker_color, form.marker.color)])
    {
        select_color(combo, false, Some(index));
    }

    // Selecting the page's item shows it, through TVN_SELCHANGEDW.
    select_page(hwnd, FIRST_PAGE);
    Ok(())
}

/// The Retry and Delay rows are greyed while Enable connect retry is off.
unsafe fn grey_retry_rows(fields: &Fields) {
    let retry = SendMessageW(fields.retry, BM_GETCHECK, 0, 0) == BST_CHECKED as LRESULT;
    for control in &fields.retry_rows {
        EnableWindow(*control, retry.into());
    }
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

/// A colour drop-down's pick; the options' have no Auto.
unsafe fn read_color(combo: HWND) -> PaletteIndex {
    selected_color(combo).expect("the options' colour drop-downs have no Auto")
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
    match state.fields.read(&state.perform).apply(&state.current) {
        Ok(settings) => {
            state.saved = Some(OptionsResult {
                settings,
                highlight_changes: state
                    .highlight
                    .as_ref()
                    .map(HighlightList::changes)
                    .unwrap_or_default(),
            });
            end_modal_dialog(hwnd);
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

/// Perform...: the Perform dialog over this one; its OK changes what this
/// dialog's OK saves.
unsafe fn edit_perform(hwnd: HWND) {
    let current = dialog_state(hwnd).perform.clone();
    match run_perform_dialog(hwnd, &current) {
        Ok(Some(perform)) => dialog_state(hwnd).perform = perform,
        Ok(None) => {}
        Err(message) => show_error(hwnd, &message),
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
        WM_COMMAND if wparam & 0xffff == usize::from(OPTIONS_PERFORM_ID) => {
            edit_perform(hwnd);
            0
        }
        WM_COMMAND
            if wparam & 0xffff == usize::from(OPTIONS_RETRY_ID)
                && (wparam >> 16) as u32 == BN_CLICKED =>
        {
            grey_retry_rows(&dialog_state(hwnd).fields);
            0
        }
        WM_COMMAND if wparam & 0xffff == IDCANCEL as usize => {
            end_modal_dialog(hwnd);
            0
        }
        WM_MEASUREITEM => {
            let edit_height = DialogMetrics::of(hwnd).edit_height;
            measure_color_item(&mut *(lparam as *mut MEASUREITEMSTRUCT), edit_height);
            1
        }
        WM_DRAWITEM => {
            draw_color_item(&*(lparam as *const DRAWITEMSTRUCT));
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
    let retry_rows_enabled: Vec<bool> = state
        .fields
        .retry_rows
        .iter()
        .map(|control| IsWindowEnabled(*control) != 0)
        .collect();
    let retry_fields_enabled = retry_rows_enabled[0];
    if retry_rows_enabled.contains(&!retry_fields_enabled) {
        return Err(format!(
            "the Retry and Delay rows are half greyed: {retry_rows_enabled:?}"
        ));
    }
    let form = state.fields.read(&state.perform);
    Ok(UiOptionsDialog {
        connect: UiConnectOptions {
            startup: form.connect_on_startup,
            reconnect: form.reconnect,
            retry: form.retry,
            retry_times: form.retry_times.clone(),
            retry_delay: form.retry_delay.clone(),
            retry_fields_enabled,
            perform_enabled: form.perform.enabled,
            perform_lines: form
                .perform
                .lines()
                .iter()
                .map(|line| line.text().to_owned())
                .collect(),
        },
        tree: paths,
        selected: selected.ok_or_else(|| "the options tree has no selection".to_owned())?,
        visible_controls,
        highlight_items,
        highlight_controls_enabled,
        highlight_delete_enabled: IsWindowEnabled(controls.delete) != 0,
        activity_colors: dump_activity_colors(&form.activity_colors),
        send_typing: form.send_typing,
        irc: UiIrcOptions {
            mode_prefix: form.irc.mode_prefix,
            auto_join_invite: form.irc.auto_join_invite,
            active_away: form.irc.active_away,
            active_invites: form.irc.active_invites,
            active_whois: form.irc.active_whois,
        },
        marker: UiMarkerOptions {
            show: form.marker.show,
            color: form.marker.color.get(),
        },
        logging: dump_logging_options(&form.logging),
        tray: UiTrayOptions {
            always: form.tray.always,
            startup: form.tray.startup,
            minimize: form.tray.minimize,
            single_click: form.tray.single_click,
        },
        tips: UiTipsOptions {
            highlights: form.tips.highlights,
            queries: form.tips.queries,
            channels: form.tips.channels,
        },
    })
}

fn dump_logging_options(logging: &LoggingSetting) -> UiLoggingOptions {
    UiLoggingOptions {
        channels: logging.channels,
        queries: logging.queries,
        strip_codes: logging.strip_codes,
        timestamp: logging.timestamp,
    }
}
