//! mIRC's colour scheme (Tools > Colors, Alt+K): which of the 16 palette
//! colours each part of every window takes, and the palette boxes the
//! Colors dialog draws.
//!
//! The scheme holds palette indices, as mIRC's does: the palette itself is
//! fixed, so a colour code typed in a message means the same colour to
//! every reader whatever their scheme.

use mirx_core::TextColor;
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect, HBRUSH, HDC};

use super::mirc_format::MIRC_PALETTE;

pub(super) const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}

/// A style colour as GDI takes it.
pub(super) const fn colorref(color: TextColor) -> u32 {
    rgb(color.red, color.green, color.blue)
}

/// A GDI colour as `#rrggbb`, as the automation dump shows colours.
pub(super) fn colorref_hex(colorref: u32) -> String {
    let [red, green, blue, _] = colorref.to_le_bytes();
    format!("#{red:02x}{green:02x}{blue:02x}")
}

/// One of mIRC's 16 configurable colours, `MIRC_PALETTE[..16]`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct PaletteIndex(u8);

pub(super) const PALETTE_COLORS: usize = 16;

impl PaletteIndex {
    pub(super) const fn new(index: u8) -> Result<Self, &'static str> {
        if (index as usize) < PALETTE_COLORS {
            Ok(Self(index))
        } else {
            Err("colors must be 0-15")
        }
    }

    /// Every index, 0 to 15.
    pub(super) fn all() -> impl Iterator<Item = Self> {
        (0..PALETTE_COLORS as u8).map(Self)
    }

    pub(super) fn get(self) -> u8 {
        self.0
    }

    pub(super) fn color(self) -> TextColor {
        MIRC_PALETTE[usize::from(self.0)]
    }

    pub(super) fn colorref(self) -> u32 {
        colorref(self.color())
    }
}

const fn index(index: u8) -> PaletteIndex {
    match PaletteIndex::new(index) {
        Ok(index) => index,
        Err(_) => panic!("palette constants are 0-15"),
    }
}

/// mirx's error lines and failed sends: mIRC has no item for them. Red
/// reads on light and dark backgrounds alike.
pub(super) const ERROR_COLOR: PaletteIndex = index(4);

/// The parts of a window the scheme colours: the items of mIRC's Colors
/// dialog that mirx draws, in mIRC's order. mIRC's CTCP, Info2, Quit,
/// Wallops, Whois, Title and Inactive have nothing to colour here.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub(super) enum ColorItem {
    Background,
    Action,
    Highlight,
    Info,
    Invite,
    Join,
    Kick,
    Mode,
    Nick,
    Normal,
    Notice,
    Notify,
    Other,
    Own,
    Part,
    Topic,
    Editbox,
    EditboxText,
    Listbox,
    ListboxText,
    Gray,
    Treebar,
    TreebarText,
}

const COLOR_ITEMS: usize = 23;

impl ColorItem {
    /// In declaration order, which is mIRC's: `ALL[item as usize] == item`.
    pub(super) const ALL: [Self; COLOR_ITEMS] = [
        Self::Background,
        Self::Action,
        Self::Highlight,
        Self::Info,
        Self::Invite,
        Self::Join,
        Self::Kick,
        Self::Mode,
        Self::Nick,
        Self::Normal,
        Self::Notice,
        Self::Notify,
        Self::Other,
        Self::Own,
        Self::Part,
        Self::Topic,
        Self::Editbox,
        Self::EditboxText,
        Self::Listbox,
        Self::ListboxText,
        Self::Gray,
        Self::Treebar,
        Self::TreebarText,
    ];

    /// Its name in the Colors dialog, as mIRC's.
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Background => "Background",
            Self::Action => "Action text",
            Self::Highlight => "Highlight text",
            Self::Info => "Info text",
            Self::Invite => "Invite text",
            Self::Join => "Join text",
            Self::Kick => "Kick text",
            Self::Mode => "Mode text",
            Self::Nick => "Nick text",
            Self::Normal => "Normal text",
            Self::Notice => "Notice text",
            Self::Notify => "Notify text",
            Self::Other => "Other text",
            Self::Own => "Own text",
            Self::Part => "Part text",
            Self::Topic => "Topic text",
            Self::Editbox => "Editbox",
            Self::EditboxText => "Editbox text",
            Self::Listbox => "Listbox",
            Self::ListboxText => "Listbox text",
            Self::Gray => "Gray text",
            Self::Treebar => "Treebar",
            Self::TreebarText => "Treebar text",
        }
    }

    /// Its key in `mirx.ini [colors]`, and its name in the automation dump.
    pub(super) fn ini_key(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Action => "action",
            Self::Highlight => "highlight",
            Self::Info => "info",
            Self::Invite => "invite",
            Self::Join => "join",
            Self::Kick => "kick",
            Self::Mode => "mode",
            Self::Nick => "nick",
            Self::Normal => "normal",
            Self::Notice => "notice",
            Self::Notify => "notify",
            Self::Other => "other",
            Self::Own => "own",
            Self::Part => "part",
            Self::Topic => "topic",
            Self::Editbox => "editbox",
            Self::EditboxText => "editbox_text",
            Self::Listbox => "listbox",
            Self::ListboxText => "listbox_text",
            Self::Gray => "gray",
            Self::Treebar => "treebar",
            Self::TreebarText => "treebar_text",
        }
    }

    pub(super) fn from_ini_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.ini_key() == key)
    }

    /// mIRC's default, the "mIRC Classic" scheme of mIRC 7.51:
    /// `0,6,4,5,2,3,3,3,3,3,3,1,5,7,6,1,3,2,3,5,1,0,1,0,1,14,6,0,0,1` in
    /// `$color` order, of which these are mirx's items.
    pub(super) fn classic(self) -> PaletteIndex {
        index(match self {
            Self::Background => 0,
            Self::Action => 6,
            Self::Highlight => 5,
            Self::Info => 2,
            Self::Invite => 3,
            Self::Join => 3,
            Self::Kick => 3,
            Self::Mode => 3,
            Self::Nick => 3,
            Self::Normal => 1,
            Self::Notice => 5,
            Self::Notify => 7,
            Self::Other => 6,
            Self::Own => 1,
            Self::Part => 3,
            Self::Topic => 3,
            Self::Editbox => 0,
            Self::EditboxText => 1,
            Self::Listbox => 0,
            Self::ListboxText => 1,
            Self::Gray => 14,
            Self::Treebar => 0,
            Self::TreebarText => 1,
        })
    }
}

/// The item an event line takes: a membership, mode or topic change, or a
/// notify list user coming or going.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum EventItem {
    Join,
    Part,
    Kick,
    Mode,
    Nick,
    Topic,
    Invite,
    Notify,
}

impl EventItem {
    pub(super) fn color_item(self) -> ColorItem {
        match self {
            Self::Join => ColorItem::Join,
            Self::Part => ColorItem::Part,
            Self::Kick => ColorItem::Kick,
            Self::Mode => ColorItem::Mode,
            Self::Nick => ColorItem::Nick,
            Self::Topic => ColorItem::Topic,
            Self::Invite => ColorItem::Invite,
            Self::Notify => ColorItem::Notify,
        }
    }
}

/// Each item's palette index.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct ColorScheme([PaletteIndex; COLOR_ITEMS]);

impl ColorScheme {
    pub(super) fn get(&self, item: ColorItem) -> PaletteIndex {
        self.0[item as usize]
    }

    pub(super) fn set(&mut self, item: ColorItem, index: PaletteIndex) {
        self.0[item as usize] = index;
    }

    pub(super) fn colorref(&self, item: ColorItem) -> u32 {
        self.get(item).colorref()
    }
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self(ColorItem::ALL.map(ColorItem::classic))
    }
}

/// Black or white, whichever reads on `background`: a palette box's number.
pub(super) fn contrasting_text(background: TextColor) -> TextColor {
    // Rec. 601 luma, in thousandths.
    let luma = 299 * u32::from(background.red)
        + 587 * u32::from(background.green)
        + 114 * u32::from(background.blue);
    if luma >= 128_000 {
        MIRC_PALETTE[1]
    } else {
        MIRC_PALETTE[0]
    }
}

/// How the 16 boxes lie: two rows of eight, as in mIRC's Colors dialog.
pub(super) const PALETTE_COLUMNS: i32 = 8;
const PALETTE_ROWS: i32 = PALETTE_COLORS as i32 / PALETTE_COLUMNS;
/// The space between two boxes, at 96 dpi.
pub(super) const PALETTE_GAP: i32 = 2;

/// The area of the boxes `cell_width` by `cell_height` apart by `gap`.
pub(super) fn palette_size(cell_width: i32, cell_height: i32, gap: i32) -> (i32, i32) {
    (
        PALETTE_COLUMNS * cell_width + (PALETTE_COLUMNS - 1) * gap,
        PALETTE_ROWS * cell_height + (PALETTE_ROWS - 1) * gap,
    )
}

/// The box of each palette colour in a `width` by `height` area, in index
/// order, apart by `gap`; the remainder is spread over the last row and
/// column.
pub(super) fn palette_cell_rects(width: i32, height: i32, gap: i32) -> [RECT; PALETTE_COLORS] {
    let cell_width = (width - gap * (PALETTE_COLUMNS - 1)) / PALETTE_COLUMNS;
    let cell_height = (height - gap * (PALETTE_ROWS - 1)) / PALETTE_ROWS;
    assert!(
        cell_width > 0 && cell_height > 0,
        "a palette is big enough for its boxes"
    );
    std::array::from_fn(|index| {
        let (row, column) = (
            index as i32 / PALETTE_COLUMNS,
            index as i32 % PALETTE_COLUMNS,
        );
        let left = column * (cell_width + gap);
        let top = row * (cell_height + gap);
        RECT {
            left,
            top,
            right: left + cell_width,
            bottom: top + cell_height,
        }
    })
}

/// The palette box at `(x, y)`; `None` between boxes and outside them.
pub(super) fn palette_cell_at(
    cells: &[RECT; PALETTE_COLORS],
    x: i32,
    y: i32,
) -> Option<PaletteIndex> {
    PaletteIndex::all().find(|index| {
        let cell = &cells[usize::from(index.get())];
        (cell.left..cell.right).contains(&x) && (cell.top..cell.bottom).contains(&y)
    })
}

/// The palette index an arrow, Home or End key moves `from` to, in the
/// two rows of eight; the keys stop at the first and last box.
pub(super) fn palette_key_move(from: PaletteIndex, key: PaletteKey) -> PaletteIndex {
    let from = i32::from(from.get());
    let last = PALETTE_COLORS as i32 - 1;
    let to = match key {
        PaletteKey::Left => from - 1,
        PaletteKey::Right => from + 1,
        PaletteKey::Up => from - PALETTE_COLUMNS,
        PaletteKey::Down => from + PALETTE_COLUMNS,
        PaletteKey::Home => 0,
        PaletteKey::End => last,
    };
    index(to.clamp(0, last) as u8)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum PaletteKey {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

unsafe fn fill_rect(hdc: HDC, rect: &RECT, color: u32) {
    let brush = CreateSolidBrush(color);
    assert!(!brush.is_null(), "CreateSolidBrush must succeed");
    FillRect(hdc, rect, brush);
    DeleteObject(brush);
}

/// Paints the palette boxes `cells` holds, each with a thin black frame.
/// `framed` gets mIRC's thick black-and-white frame: the selected colour.
/// `numbered` writes each box's index in it, in black or white, as the
/// Ctrl+K colour index does; the DC's font and background mode are the
/// caller's.
pub(super) unsafe fn paint_palette_cells(
    hdc: HDC,
    cells: &[RECT],
    first: PaletteIndex,
    framed: Option<PaletteIndex>,
    numbered: bool,
) {
    use windows_sys::Win32::Graphics::Gdi::{
        DT_CENTER, DT_SINGLELINE, DT_VCENTER, DrawTextW, SetTextColor,
    };
    for (offset, cell) in cells.iter().enumerate() {
        let index = index(first.get() + offset as u8);
        fill_rect(hdc, cell, rgb(0, 0, 0));
        let inner = inset(cell, 1);
        fill_rect(hdc, &inner, index.colorref());
        if framed == Some(index) {
            let white = inset(cell, 1);
            frame(hdc, &white, rgb(0xff, 0xff, 0xff));
            frame(hdc, &inset(cell, 2), rgb(0, 0, 0));
        }
        if numbered {
            let mut text = index.get().to_string().encode_utf16().collect::<Vec<_>>();
            let mut rect = *cell;
            SetTextColor(hdc, colorref(contrasting_text(index.color())));
            DrawTextW(
                hdc,
                text.as_mut_ptr(),
                text.len() as i32,
                &mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }
    }
}

fn inset(rect: &RECT, by: i32) -> RECT {
    RECT {
        left: rect.left + by,
        top: rect.top + by,
        right: rect.right - by,
        bottom: rect.bottom - by,
    }
}

/// A one-pixel frame just inside `rect`.
unsafe fn frame(hdc: HDC, rect: &RECT, color: u32) {
    use windows_sys::Win32::Graphics::Gdi::FrameRect;
    let brush = CreateSolidBrush(color);
    assert!(!brush.is_null(), "CreateSolidBrush must succeed");
    FrameRect(hdc, rect, brush);
    DeleteObject(brush);
}

/// The brushes the scheme's background items paint with: what the
/// timeline fills with, and what `WM_CTLCOLOREDIT`/`WM_CTLCOLORLISTBOX`
/// hand the editboxes and nick lists. Made from one scheme, deleted on
/// drop; a new scheme makes new ones, swapped in before the windows
/// repaint.
pub(super) struct SchemeBrushes {
    pub(super) background: HBRUSH,
    pub(super) editbox: HBRUSH,
    pub(super) listbox: HBRUSH,
}

impl SchemeBrushes {
    pub(super) unsafe fn create(scheme: &ColorScheme) -> Self {
        let brush = |item| {
            let brush = CreateSolidBrush(scheme.colorref(item));
            assert!(!brush.is_null(), "CreateSolidBrush must succeed");
            brush
        };
        Self {
            background: brush(ColorItem::Background),
            editbox: brush(ColorItem::Editbox),
            listbox: brush(ColorItem::Listbox),
        }
    }
}

impl Drop for SchemeBrushes {
    fn drop(&mut self) {
        for brush in [self.background, self.editbox, self.listbox] {
            unsafe {
                DeleteObject(brush);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_defaults_are_mirc_7_51s_for_mirxs_items() {
        // $color order: Background, Action, CTCP, Highlight, Info, Info2,
        // Invite, Join, Kick, Mode, Nick, Normal, Notice, Notify, Other,
        // Own, Part, Quit, Topic, Wallops, Whois, Editbox, Editbox text,
        // Listbox, Listbox text, Gray, Title, Inactive, Treebar, Treebar
        // text.
        let classic = [
            0, 6, 4, 5, 2, 3, 3, 3, 3, 3, 3, 1, 5, 7, 6, 1, 3, 2, 3, 5, 1, 0, 1, 0, 1, 14, 6, 0, 0,
            1,
        ];
        // Where each of mirx's items is in that order.
        let positions = [
            0, 1, 3, 4, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 21, 22, 23, 24, 25, 28, 29,
        ];
        for (item, position) in ColorItem::ALL.into_iter().zip(positions) {
            assert_eq!(item.classic().get(), classic[position], "{item:?}");
        }
        let scheme = ColorScheme::default();
        assert_eq!(scheme.get(ColorItem::Highlight), index(5));
        assert_eq!(scheme.get(ColorItem::Gray), index(14));
        assert_eq!(scheme.get(ColorItem::Notify), index(7));
    }

    #[test]
    fn palette_indices_are_0_to_15() {
        assert_eq!(PaletteIndex::new(0).map(PaletteIndex::get), Ok(0));
        assert_eq!(PaletteIndex::new(15).map(PaletteIndex::get), Ok(15));
        assert_eq!(PaletteIndex::new(16), Err("colors must be 0-15"));
        assert_eq!(PaletteIndex::all().count(), 16);
        assert_eq!(index(4).colorref(), rgb(0xff, 0, 0));
        assert_eq!(index(12).colorref(), rgb(0, 0, 0xfc));
    }

    #[test]
    fn items_are_in_declaration_order_with_unique_names() {
        for (position, item) in ColorItem::ALL.into_iter().enumerate() {
            assert_eq!(item as usize, position);
            assert_eq!(ColorItem::from_ini_key(item.ini_key()), Some(item));
        }
        let mut labels = ColorItem::ALL.map(ColorItem::label).to_vec();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), COLOR_ITEMS);
        let mut keys = ColorItem::ALL.map(ColorItem::ini_key).to_vec();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), COLOR_ITEMS);
        assert_eq!(ColorItem::from_ini_key("ctcp"), None);
    }

    #[test]
    fn setting_an_item_changes_only_that_item() {
        let mut scheme = ColorScheme::default();
        scheme.set(ColorItem::Join, index(9));
        for item in ColorItem::ALL {
            let expected = if item == ColorItem::Join {
                index(9)
            } else {
                item.classic()
            };
            assert_eq!(scheme.get(item), expected);
        }
        assert_eq!(scheme.colorref(ColorItem::Join), rgb(0, 0xfc, 0));
    }

    #[test]
    fn every_cell_centre_hits_its_cell_and_gaps_hit_none() {
        let (width, height) = palette_size(20, 20, 2);
        assert_eq!((width, height), (8 * 20 + 7 * 2, 2 * 20 + 2));
        let cells = palette_cell_rects(width, height, 2);
        let cell = cells[9];
        assert_eq!(
            (cell.left, cell.top, cell.right, cell.bottom),
            (22, 22, 42, 42)
        );
        for index in PaletteIndex::all() {
            let cell = cells[usize::from(index.get())];
            let (x, y) = ((cell.left + cell.right) / 2, (cell.top + cell.bottom) / 2);
            assert_eq!(palette_cell_at(&cells, x, y), Some(index));
        }
        // The gap after box 0, below row one, and outside.
        assert_eq!(palette_cell_at(&cells, 20, 5), None);
        assert_eq!(palette_cell_at(&cells, 5, 21), None);
        assert_eq!(palette_cell_at(&cells, -1, 5), None);
        assert_eq!(palette_cell_at(&cells, 5, 42), None);
    }

    #[test]
    fn palette_keys_move_within_two_rows_of_eight() {
        let at = |from, key| palette_key_move(index(from), key).get();
        assert_eq!(at(3, PaletteKey::Right), 4);
        assert_eq!(at(3, PaletteKey::Left), 2);
        assert_eq!(at(3, PaletteKey::Down), 11);
        assert_eq!(at(11, PaletteKey::Up), 3);
        assert_eq!(at(3, PaletteKey::Up), 0);
        assert_eq!(at(0, PaletteKey::Left), 0);
        assert_eq!(at(15, PaletteKey::Right), 15);
        assert_eq!(at(12, PaletteKey::Down), 15);
        assert_eq!(at(9, PaletteKey::Home), 0);
        assert_eq!(at(2, PaletteKey::End), 15);
    }

    #[test]
    fn numbers_are_white_on_dark_boxes_and_black_on_light_ones() {
        for dark in [1, 2, 5, 12] {
            assert_eq!(
                contrasting_text(index(dark).color()),
                MIRC_PALETTE[0],
                "{dark}"
            );
        }
        for light in [0, 8, 15] {
            assert_eq!(
                contrasting_text(index(light).color()),
                MIRC_PALETTE[1],
                "{light}"
            );
        }
    }

    #[test]
    fn hex_is_lowercase_rrggbb() {
        assert_eq!(colorref_hex(index(13).colorref()), "#ff00ff");
        assert_eq!(colorref_hex(index(2).colorref()), "#00007f");
        assert_eq!(colorref_hex(rgb(0x12, 0xab, 0x0c)), "#12ab0c");
    }
}
