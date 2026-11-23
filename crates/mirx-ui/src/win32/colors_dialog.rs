//! The Colors dialog (Tools > Colors..., Alt+K), mIRC's: a preview of a
//! window in the scheme, where clicking a part picks its item; the item
//! drop-down; the 16 palette boxes, which give the picked item its colour;
//! Reset, OK and Cancel.
//!
//! The scheme and its rules are `colors`'; the preview's layout is pure
//! and shared by painting, clicking and the automation dump.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, COLOR_BTNFACE, CreateSolidBrush, DeleteObject, DrawFocusRect, EndPaint, FillRect,
    GetDC, GetSysColorBrush, HBRUSH, HDC, InvalidateRect, PAINTSTRUCT, ReleaseDC, SetBkMode,
    SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, SetFocus, VK_DOWN, VK_END, VK_HOME, VK_LEFT, VK_RIGHT, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CBN_SELCHANGE,
    CBS_DROPDOWNLIST, DLGC_WANTARROWS, DefWindowProcW, GetClientRect, GetDlgItem, GetParent,
    GetSystemMetrics, IDC_ARROW, IDCANCEL, IDOK, LoadCursorW, MoveWindow, RegisterClassExW,
    SM_CXEDGE, SM_CYEDGE, SendMessageW, WM_COMMAND, WM_GETDLGCODE, WM_KEYDOWN, WM_KILLFOCUS,
    WM_LBUTTONDOWN, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_SETFOCUS, WNDCLASSEXW, WNDPROC,
    WS_CHILD, WS_EX_CLIENTEDGE, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use super::colors::{
    ColorItem, ColorScheme, PALETTE_COLORS, PALETTE_GAP, PaletteIndex, PaletteKey,
    paint_palette_cells, palette_cell_at, palette_cell_rects, palette_key_move, palette_size,
};
use super::dialogs::{DialogMetrics, create_modal_dialog, dialog_text_size, end_modal_dialog};
use super::dpi::Dpi;
use super::window_data::{self, WindowData};
use super::{
    BUTTON_CLASS_NAME, COMBOBOX_CLASS_NAME, STATIC_CLASS_NAME, create_child_window, last_os_error,
    run_modal_dialog, state_ref, wide_null,
};
use crate::automation::{UiColorsDialog, UiColorsPreviewItem, UiPoint, UiRect};

pub(super) const COLORS_DIALOG_CLASS_NAME: &str = "mirxWin32ColorsDialog";
const COLORS_DIALOG_TITLE: &str = "mirx Colors";
const PREVIEW_CLASS_NAME: &str = "mirxColorsPreview";
const PALETTE_CLASS_NAME: &str = "mirxColorPalette";
pub(super) const COLORS_PREVIEW_ID: u16 = 6501;
pub(super) const COLORS_ITEM_ID: u16 = 6502;
pub(super) const COLORS_PALETTE_ID: u16 = 6503;
pub(super) const COLORS_RESET_ID: u16 = 6504;

// The layout's lengths at 96 dpi.
/// A palette box's side.
const PALETTE_CELL: i32 = 20;
const ITEM_COMBO_WIDTH: i32 = 160;
/// The drop-down's list: every item without a scroll in the dialog font.
const ITEM_COMBO_DROPPED_HEIGHT: i32 = 320;

/// The timeline's sample lines, mIRC's preview words: one per text item.
const SAMPLE_LINES: [(&str, ColorItem); 16] = [
    ("* Joins: Carol", ColorItem::Join),
    ("* Parts: Dave", ColorItem::Part),
    ("* Mod turns moderation on", ColorItem::Settings),
    ("* Eve was kicked by Mod", ColorItem::Kick),
    ("* Carol is now known as Caz", ColorItem::Nick),
    ("* Topic is 'hello'", ColorItem::Topic),
    ("* Mod invites Heidi", ColorItem::Invite),
    ("<Bob> Normal text", ColorItem::Normal),
    ("<you> Own text", ColorItem::Own),
    ("-Bob- Notice text", ColorItem::Notice),
    ("* Bob is on Matrix", ColorItem::Notify),
    ("* Bob waves", ColorItem::Action),
    ("<Bob> you: Highlight text", ColorItem::Highlight),
    ("* Info text", ColorItem::Info),
    ("* Bob sent image.png", ColorItem::Other),
    ("Bob is typing", ColorItem::Gray),
];
const TREEBAR_LINES: [&str; 2] = ["Status", "#room"];
const LISTBOX_LINES: [&str; 2] = ["@Mod", "Bob"];
const EDITBOX_TEXT: &str = "Editbox text";
/// Around the text in each part, at 96 dpi.
const PREVIEW_PADDING: i32 = 4;

/// A part of the preview: a background, or a sample text in its item's
/// colour over one.
#[derive(Clone)]
pub(super) struct PreviewArea {
    pub(super) rect: RECT,
    pub(super) item: ColorItem,
    pub(super) text: Option<&'static str>,
}

/// The preview as a window in mirx lies: the treebar, the text area over
/// its editbox, and the nick list, with text `line_height` apart and as
/// wide as `measure` says. The backgrounds come first, then the texts over
/// them: the order they paint in. The preview is as big as its parts.
/// `padding` is the space around each part's text, in pixels.
pub(super) fn preview_layout(
    line_height: i32,
    padding: i32,
    measure: impl Fn(&str) -> i32,
) -> Vec<PreviewArea> {
    let widest = |lines: &[&str]| {
        lines
            .iter()
            .map(|line| measure(line))
            .max()
            .expect("every part has lines")
    };
    let samples = SAMPLE_LINES.map(|(text, _)| text);
    let tree_width = 2 * padding + widest(&TREEBAR_LINES);
    let timeline_width = 2 * padding + widest(&samples);
    let list_width = 2 * padding + widest(&LISTBOX_LINES);
    let timeline_height = 2 * padding + SAMPLE_LINES.len() as i32 * line_height;
    let height = timeline_height + 2 * padding + line_height;
    let timeline_left = tree_width;
    let list_left = timeline_left + timeline_width;
    let rect = |left, top, right, bottom| RECT {
        left,
        top,
        right,
        bottom,
    };
    let background = |rect, item| PreviewArea {
        rect,
        item,
        text: None,
    };
    let text = |left: i32, top: i32, text: &'static str, item| PreviewArea {
        rect: rect(left, top, left + measure(text), top + line_height),
        item,
        text: Some(text),
    };
    let line_top = |index: usize| padding + index as i32 * line_height;
    let mut areas = vec![
        background(rect(0, 0, tree_width, height), ColorItem::Treebar),
        background(
            rect(timeline_left, 0, list_left, timeline_height),
            ColorItem::Background,
        ),
        background(
            rect(timeline_left, timeline_height, list_left, height),
            ColorItem::Editbox,
        ),
        background(
            rect(list_left, 0, list_left + list_width, height),
            ColorItem::Listbox,
        ),
    ];
    for (index, line) in TREEBAR_LINES.into_iter().enumerate() {
        areas.push(text(padding, line_top(index), line, ColorItem::TreebarText));
    }
    for (index, (line, item)) in SAMPLE_LINES.into_iter().enumerate() {
        areas.push(text(timeline_left + padding, line_top(index), line, item));
    }
    areas.push(text(
        timeline_left + padding,
        timeline_height + padding,
        EDITBOX_TEXT,
        ColorItem::EditboxText,
    ));
    for (index, line) in LISTBOX_LINES.into_iter().enumerate() {
        areas.push(text(
            list_left + padding,
            line_top(index),
            line,
            ColorItem::ListboxText,
        ));
    }
    areas
}

/// The preview's size: what its parts cover.
fn preview_size(areas: &[PreviewArea]) -> (i32, i32) {
    areas.iter().fold((0, 0), |(width, height), area| {
        (width.max(area.rect.right), height.max(area.rect.bottom))
    })
}

fn contains(rect: &RECT, x: i32, y: i32) -> bool {
    (rect.left..rect.right).contains(&x) && (rect.top..rect.bottom).contains(&y)
}

/// The item a click at `(x, y)` picks: a sample text's, else the part's
/// background's; `None` outside the preview.
pub(super) fn preview_item_at(areas: &[PreviewArea], x: i32, y: i32) -> Option<ColorItem> {
    areas
        .iter()
        .rev()
        .find(|area| contains(&area.rect, x, y))
        .map(|area| area.item)
}

/// Where a click picks `item`: the middle of its first sample text, or a
/// background's bottom right corner, which no text reaches.
fn preview_click_point(areas: &[PreviewArea], item: ColorItem) -> (i32, i32) {
    let area = areas
        .iter()
        .find(|area| area.item == item)
        .expect("the preview shows every item");
    let rect = &area.rect;
    match area.text {
        Some(_) => ((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2),
        None => (rect.right - 2, rect.bottom - 2),
    }
}

/// The dialog's state, owned by `ColorsDialog` and lent to the window
/// procedure while the window exists.
struct DialogState {
    /// The frame, whose text font the preview draws in.
    frame: HWND,
    /// The scheme as edited.
    scheme: ColorScheme,
    /// The item the palette colours.
    selected: ColorItem,
    preview_areas: Vec<PreviewArea>,
    preview: HWND,
    item: HWND,
    palette: HWND,
    /// What OK saved.
    saved: Option<ColorScheme>,
}

/// An open Colors dialog. `run` shows it until OK or Cancel closes it.
pub(super) struct ColorsDialog {
    hwnd: HWND,
    owner: HWND,
    state: Box<WindowData<DialogState>>,
}

impl ColorsDialog {
    /// Creates the dialog over the frame `owner`, centred on it, showing
    /// `scheme` with Normal text picked.
    pub(super) unsafe fn open(owner: HWND, scheme: &ColorScheme) -> Result<Self, String> {
        let glyphs = &state_ref(owner).text_font.glyphs;
        let screen = GetDC(null_mut());
        // The dialog lies over the frame, at its DPI.
        let padding = Dpi::of_window(owner).px(PREVIEW_PADDING);
        let preview_areas = preview_layout(glyphs.metrics().tmHeight, padding, |text| {
            glyphs.measure(screen, text)
        });
        ReleaseDC(null_mut(), screen);
        let mut state = Box::new(WindowData::new(DialogState {
            frame: owner,
            scheme: scheme.clone(),
            selected: ColorItem::Normal,
            preview_areas,
            preview: null_mut(),
            item: null_mut(),
            palette: null_mut(),
            saved: None,
        }));
        let hwnd = create_modal_dialog(
            owner,
            COLORS_DIALOG_CLASS_NAME,
            COLORS_DIALOG_TITLE,
            (&mut *state as *mut WindowData<DialogState>).cast(),
            |hwnd| create_controls(hwnd),
        )?;
        Ok(Self { hwnd, owner, state })
    }

    pub(super) fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// Shows the dialog until it closes: the scheme OK saved, or `None`
    /// for Cancel, Esc and the close box.
    pub(super) unsafe fn run(self) -> Option<ColorScheme> {
        let Self {
            hwnd,
            owner,
            mut state,
        } = self;
        SetFocus(state.palette);
        run_modal_dialog(owner, hwnd);
        state.saved.take()
    }
}

/// The preview's and the palette's classes, which paint every pixel
/// themselves. A second registration finds them registered.
pub(super) fn register_control_classes(instance: *mut core::ffi::c_void) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{ERROR_CLASS_ALREADY_EXISTS, GetLastError};
    let classes: [(&str, WNDPROC); 2] = [
        (PREVIEW_CLASS_NAME, Some(preview_window_proc)),
        (PALETTE_CLASS_NAME, Some(palette_window_proc)),
    ];
    for (name, window_proc) in classes {
        let class_name = wide_null(name);
        let window_class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: window_proc,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: null_mut(),
        };
        if unsafe { RegisterClassExW(&window_class) } == 0
            && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
        {
            return Err(last_os_error(&format!("RegisterClassExW {name}")));
        }
    }
    Ok(())
}

unsafe fn dialog_state(hwnd: HWND) -> &'static mut DialogState {
    window_data::get(hwnd).expect("the colors dialog has its state")
}

/// The state of the dialog a preview or palette is in.
unsafe fn control_dialog_state(control: HWND) -> &'static mut DialogState {
    dialog_state(GetParent(control))
}

unsafe fn create_controls(hwnd: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let control = |class: &str, text: &str, style: u32, ex_style: u32, id: u16| {
        create_child_window(
            hwnd,
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
    let state = dialog_state(hwnd);
    let dpi = Dpi::of_window(hwnd);
    let DialogMetrics {
        margin,
        gap,
        button_width,
        button_height,
        edit_height,
        ..
    } = DialogMetrics::at(dpi);

    // The preview's client area is its layout; the sunken edge is around it.
    let (preview_width, preview_height) = preview_size(&state.preview_areas);
    let (edge_x, edge_y) = (GetSystemMetrics(SM_CXEDGE), GetSystemMetrics(SM_CYEDGE));
    state.preview = place(
        control(
            PREVIEW_CLASS_NAME,
            "",
            0,
            WS_EX_CLIENTEDGE,
            COLORS_PREVIEW_ID,
        )?,
        margin,
        margin,
        preview_width + 2 * edge_x,
        preview_height + 2 * edge_y,
    );
    let label = "Item:";
    let (label_width, label_height) = dialog_text_size(label, i32::MAX);
    let combo_width = dpi.px(ITEM_COMBO_WIDTH);
    let cell = dpi.px(PALETTE_CELL);
    let (palette_width, palette_height) = palette_size(cell, cell, dpi.px(PALETTE_GAP));
    // As wide as its widest row: the preview is as wide as the text font
    // makes it, which may be narrower than the rest.
    let dialog_width = [
        preview_width + 2 * edge_x,
        label_width + gap + combo_width,
        palette_width,
        3 * button_width + 2 * gap,
    ]
    .into_iter()
    .max()
    .expect("the dialog has rows");

    let item_y = margin + preview_height + 2 * edge_y + gap;
    place(
        control(STATIC_CLASS_NAME, label, 0, 0, 0)?,
        margin,
        item_y + (edit_height - label_height) / 2,
        label_width,
        label_height,
    );
    // A drop-down list's height is its dropped list's; its window is the
    // closed height, which is what the dialog fits.
    state.item = place(
        control(
            COMBOBOX_CLASS_NAME,
            "",
            WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32,
            0,
            COLORS_ITEM_ID,
        )?,
        margin + label_width + gap,
        item_y,
        combo_width,
        dpi.px(ITEM_COMBO_DROPPED_HEIGHT),
    );
    for item in ColorItem::ALL {
        let label = wide_null(item.label());
        SendMessageW(state.item, CB_ADDSTRING, 0, label.as_ptr() as LPARAM);
    }

    let palette_y = item_y + edit_height + gap;
    state.palette = place(
        control(PALETTE_CLASS_NAME, "", WS_TABSTOP, 0, COLORS_PALETTE_ID)?,
        margin,
        palette_y,
        palette_width,
        palette_height,
    );

    // Reset left, OK and Cancel right, under the palette.
    let buttons_y = palette_y + palette_height + margin;
    let right = margin + dialog_width;
    for (id, text, style, x) in [
        (COLORS_RESET_ID as usize, "&Reset", BS_PUSHBUTTON, margin),
        (
            IDOK as usize,
            "OK",
            BS_DEFPUSHBUTTON,
            right - 2 * button_width - gap,
        ),
        (
            IDCANCEL as usize,
            "Cancel",
            BS_PUSHBUTTON,
            right - button_width,
        ),
    ] {
        place(
            control(
                BUTTON_CLASS_NAME,
                text,
                WS_TABSTOP | style as u32,
                0,
                id as u16,
            )?,
            x,
            buttons_y,
            button_width,
            button_height,
        );
    }

    select_item(state, state.selected);
    Ok(())
}

/// Picks `item`: the drop-down names it, the preview frames it and the
/// palette frames its colour.
unsafe fn select_item(state: &mut DialogState, item: ColorItem) {
    state.selected = item;
    SendMessageW(state.item, CB_SETCURSEL, item as usize, 0);
    redraw(state);
}

/// Gives the picked item `index`.
unsafe fn set_index(state: &mut DialogState, index: PaletteIndex) {
    state.scheme.set(state.selected, index);
    redraw(state);
}

unsafe fn redraw(state: &DialogState) {
    InvalidateRect(state.preview, null(), 0);
    InvalidateRect(state.palette, null(), 0);
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
        WM_COMMAND => {
            let id = wparam & 0xffff;
            let notification = (wparam >> 16) as u32;
            let state = dialog_state(hwnd);
            match id {
                _ if id == IDOK as usize => {
                    state.saved = Some(state.scheme.clone());
                    end_modal_dialog(hwnd);
                }
                _ if id == IDCANCEL as usize => {
                    end_modal_dialog(hwnd);
                }
                // mIRC's default colours; nothing is saved until OK.
                _ if id == usize::from(COLORS_RESET_ID) => {
                    state.scheme = ColorScheme::default();
                    redraw(state);
                }
                _ if id == usize::from(COLORS_ITEM_ID) && notification == CBN_SELCHANGE => {
                    let index = SendMessageW(state.item, CB_GETCURSEL, 0, 0);
                    let item = ColorItem::ALL
                        [usize::try_from(index).expect("a drop-down list always has a selection")];
                    select_item(state, item);
                }
                _ => return DefWindowProcW(hwnd, message, wparam, lparam),
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

/// The dialog's class and procedure, registered with the other dialogs'.
pub(super) const DIALOG_CLASS: (&str, WNDPROC) = (COLORS_DIALOG_CLASS_NAME, Some(window_proc));

unsafe fn fill(hdc: HDC, rect: &RECT, color: u32) {
    let brush = CreateSolidBrush(color);
    assert!(!brush.is_null(), "CreateSolidBrush must succeed");
    FillRect(hdc, rect, brush);
    DeleteObject(brush);
}

/// Paints the preview in the edited scheme and the frame's text font, and
/// frames the picked item's parts.
unsafe fn paint_preview(hwnd: HWND) {
    let state = control_dialog_state(hwnd);
    let glyphs = &state_ref(state.frame).text_font.glyphs;
    let mut paint = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint);
    SetBkMode(hdc, TRANSPARENT as i32);
    for area in &state.preview_areas {
        let color = state.scheme.colorref(area.item);
        match area.text {
            None => fill(hdc, &area.rect, color),
            Some(text) => {
                SetTextColor(hdc, color);
                glyphs.draw(
                    hdc,
                    area.rect.left,
                    area.rect.top + glyphs.metrics().tmAscent,
                    &area.rect,
                    text,
                    false,
                );
            }
        }
    }
    for area in state
        .preview_areas
        .iter()
        .filter(|area| area.item == state.selected)
    {
        // Around a text, and just inside a background.
        let by = if area.text.is_some() { -1 } else { 1 };
        let rect = RECT {
            left: area.rect.left + by,
            top: area.rect.top + by,
            right: area.rect.right - by,
            bottom: area.rect.bottom - by,
        };
        DrawFocusRect(hdc, &rect);
    }
    EndPaint(hwnd, &paint);
}

fn mouse_point(lparam: LPARAM) -> (i32, i32) {
    (
        i32::from(lparam as u16 as i16),
        i32::from((lparam >> 16) as u16 as i16),
    )
}

unsafe extern "system" fn preview_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_PAINT => {
            paint_preview(hwnd);
            0
        }
        // "Click on it and you will see its name appear" (mIRC's help).
        WM_LBUTTONDOWN => {
            let state = control_dialog_state(hwnd);
            let (x, y) = mouse_point(lparam);
            let item = preview_item_at(&state.preview_areas, x, y)
                .expect("the preview's parts cover its client area");
            select_item(state, item);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn palette_cells(hwnd: HWND) -> [RECT; PALETTE_COLORS] {
    let mut client = RECT::default();
    GetClientRect(hwnd, &mut client);
    palette_cell_rects(
        client.right,
        client.bottom,
        Dpi::of_window(hwnd).px(PALETTE_GAP),
    )
}

unsafe fn paint_palette(hwnd: HWND) {
    let state = control_dialog_state(hwnd);
    let cells = palette_cells(hwnd);
    let current = state.scheme.get(state.selected);
    let mut paint = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint);
    let mut client = RECT::default();
    GetClientRect(hwnd, &mut client);
    FillRect(hdc, &client, GetSysColorBrush(COLOR_BTNFACE) as HBRUSH);
    let first = PaletteIndex::new(0).expect("0 is a palette index");
    paint_palette_cells(hdc, &cells, first, Some(current), false);
    if GetFocus() == hwnd {
        let cell = cells[usize::from(current.get())];
        let rect = RECT {
            left: cell.left - 1,
            top: cell.top - 1,
            right: cell.right + 1,
            bottom: cell.bottom + 1,
        };
        DrawFocusRect(hdc, &rect);
    }
    EndPaint(hwnd, &paint);
}

fn palette_key(key: u16) -> Option<PaletteKey> {
    Some(match key {
        VK_LEFT => PaletteKey::Left,
        VK_RIGHT => PaletteKey::Right,
        VK_UP => PaletteKey::Up,
        VK_DOWN => PaletteKey::Down,
        VK_HOME => PaletteKey::Home,
        VK_END => PaletteKey::End,
        _ => return None,
    })
}

unsafe extern "system" fn palette_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_PAINT => {
            paint_palette(hwnd);
            0
        }
        WM_LBUTTONDOWN => {
            SetFocus(hwnd);
            let (x, y) = mouse_point(lparam);
            // A click between boxes picks nothing.
            if let Some(index) = palette_cell_at(&palette_cells(hwnd), x, y) {
                set_index(control_dialog_state(hwnd), index);
            }
            0
        }
        // The arrows move the colour; Tab and Enter stay the dialog's.
        WM_GETDLGCODE => DLGC_WANTARROWS as LRESULT,
        WM_KEYDOWN => match palette_key(wparam as u16) {
            Some(key) => {
                let state = control_dialog_state(hwnd);
                let index = palette_key_move(state.scheme.get(state.selected), key);
                set_index(state, index);
                0
            }
            None => DefWindowProcW(hwnd, message, wparam, lparam),
        },
        WM_SETFOCUS | WM_KILLFOCUS => {
            InvalidateRect(hwnd, null(), 0);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// The dialog as automation sees it: the picked item, the edited scheme,
/// and where to click each palette box and each item in the preview.
pub(super) unsafe fn dump(hwnd: HWND) -> UiColorsDialog {
    let state = dialog_state(hwnd);
    let palette = GetDlgItem(hwnd, i32::from(COLORS_PALETTE_ID));
    assert_eq!(palette, state.palette);
    UiColorsDialog {
        selected: state.selected.ini_key().to_owned(),
        items: super::automation_dump::dump_color_items(&state.scheme),
        palette_cells: palette_cells(palette)
            .iter()
            .map(|cell| UiRect {
                x: cell.left,
                y: cell.top,
                width: cell.right - cell.left,
                height: cell.bottom - cell.top,
            })
            .collect(),
        preview_items: ColorItem::ALL
            .into_iter()
            .map(|item| {
                let (x, y) = preview_click_point(&state.preview_areas, item);
                UiColorsPreviewItem {
                    item: item.ini_key().to_owned(),
                    click_point: UiPoint { x, y },
                }
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ten pixels a character.
    fn mono(text: &str) -> i32 {
        text.chars().count() as i32 * 10
    }

    #[test]
    fn the_preview_shows_every_item_and_clicks_pick_them() {
        let areas = preview_layout(15, PREVIEW_PADDING, mono);
        for item in ColorItem::ALL {
            let (x, y) = preview_click_point(&areas, item);
            assert_eq!(preview_item_at(&areas, x, y), Some(item), "{item:?}");
        }
        // Treebar | text area over the editbox | nick list.
        let size = preview_size(&areas);
        let widest_sample =
            mono("<Bob> you: Highlight text").max(mono("* Carol is now known as Caz"));
        assert_eq!(
            size,
            (
                (8 + 60) + (8 + widest_sample) + (8 + mono("@Mod")),
                (8 + 16 * 15) + (8 + 15)
            )
        );
        // The text area's empty right, below its last line, and the gap
        // left of a line are its background.
        assert_eq!(
            preview_item_at(&areas, 68 + 4 + mono("* Joins: Carol") + 3, 5),
            Some(ColorItem::Background)
        );
        assert_eq!(preview_item_at(&areas, 70, 50), Some(ColorItem::Background));
        assert_eq!(preview_item_at(&areas, 75, 4), Some(ColorItem::Join));
        assert_eq!(
            preview_item_at(&areas, 75, 4 + 12 * 15),
            Some(ColorItem::Highlight)
        );
        assert_eq!(preview_item_at(&areas, 5, 5), Some(ColorItem::TreebarText));
        assert_eq!(preview_item_at(&areas, 5, 100), Some(ColorItem::Treebar));
        assert_eq!(preview_item_at(&areas, size.0, 5), None);
        assert_eq!(preview_item_at(&areas, 5, size.1), None);
    }

    #[test]
    fn every_sample_line_is_a_different_text_item() {
        let mut items = SAMPLE_LINES.map(|(_, item)| item).to_vec();
        items.sort();
        items.dedup();
        assert_eq!(items.len(), SAMPLE_LINES.len());
    }
}
