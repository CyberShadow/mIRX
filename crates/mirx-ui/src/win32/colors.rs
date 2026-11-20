//! mIRC's colour scheme: which of the 16 palette colours each part of
//! every window takes.
//!
//! The scheme holds palette indices, as mIRC's does: the palette itself is
//! fixed, so a colour code typed in a message means the same colour to
//! every reader whatever their scheme.

use mirx_core::TextColor;
use windows_sys::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, HBRUSH};

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
/// dialog that mirx draws, in mIRC's order. mIRC's CTCP, Info2, Notify,
/// Quit, Wallops, Whois, Title and Inactive have nothing to colour here.
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

const COLOR_ITEMS: usize = 22;

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

/// The item an event line takes: a membership, mode or topic change.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum EventItem {
    Join,
    Part,
    Kick,
    Mode,
    Nick,
    Topic,
    Invite,
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
            0, 1, 3, 4, 6, 7, 8, 9, 10, 11, 12, 14, 15, 16, 18, 21, 22, 23, 24, 25, 28, 29,
        ];
        for (item, position) in ColorItem::ALL.into_iter().zip(positions) {
            assert_eq!(item.classic().get(), classic[position], "{item:?}");
        }
        let scheme = ColorScheme::default();
        assert_eq!(scheme.get(ColorItem::Highlight), index(5));
        assert_eq!(scheme.get(ColorItem::Gray), index(14));
    }

    #[test]
    fn palette_indices_are_0_to_15() {
        assert_eq!(PaletteIndex::new(0).map(PaletteIndex::get), Ok(0));
        assert_eq!(PaletteIndex::new(15).map(PaletteIndex::get), Ok(15));
        assert_eq!(PaletteIndex::new(16), Err("colors must be 0-15"));
        assert_eq!(index(4).colorref(), rgb(0xff, 0, 0));
        assert_eq!(index(12).colorref(), rgb(0, 0, 0xfc));
    }

    #[test]
    fn items_are_in_declaration_order_with_unique_keys() {
        for (position, item) in ColorItem::ALL.into_iter().enumerate() {
            assert_eq!(item as usize, position);
            assert_eq!(ColorItem::from_ini_key(item.ini_key()), Some(item));
        }
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
    fn hex_is_lowercase_rrggbb() {
        assert_eq!(colorref_hex(index(13).colorref()), "#ff00ff");
        assert_eq!(colorref_hex(index(2).colorref()), "#00007f");
        assert_eq!(colorref_hex(rgb(0x12, 0xab, 0x0c)), "#12ab0c");
    }
}
