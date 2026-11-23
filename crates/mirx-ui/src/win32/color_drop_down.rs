//! The colour drop-down: an owner-drawn combo box of the 16 palette
//! colours, each a numbered palette box, as the Options dialog's switchbar
//! and line marker colours and the Address Book's nick colour entry pick
//! them. The nick colour's has an Auto item first.
//!
//! Without `CBS_HASSTRINGS` an item is its data: the palette index, or
//! `AUTO_ITEM` for Auto.

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT, DT_CENTER, DT_SINGLELINE,
    DT_VCENTER, DrawFocusRect, DrawTextW, FillRect, GetSysColor, GetSysColorBrush, HBRUSH,
    SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::UI::Controls::{
    DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_FOCUS, ODS_SELECTED, ODT_COMBOBOX,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CB_ADDSTRING, CB_GETCURSEL, CB_GETITEMDATA, CB_SETCURSEL, CBS_DROPDOWNLIST, CBS_OWNERDRAWFIXED,
    SendMessageW, WS_TABSTOP, WS_VSCROLL,
};

use super::colors::{PALETTE_COLORS, PaletteIndex, paint_palette_cells};
use super::dpi::Dpi;

/// A drop-down's style, besides `WS_CHILD` and its visibility.
pub(super) const COLOR_DROP_DOWN_STYLE: u32 =
    WS_TABSTOP | WS_VSCROLL | (CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED) as u32;
/// Its width at 96 dpi.
pub(super) const COLOR_DROP_DOWN_WIDTH: i32 = 60;
/// A closed drop-down's height around its item: its edges, in pixels at
/// every DPI.
const DROP_DOWN_EDGES: i32 = 6;
/// The open list shows half the palette.
const DROPPED_ITEMS: i32 = 8;
/// The Auto item's data: past the palette.
const AUTO_ITEM: usize = PALETTE_COLORS;
const AUTO_TEXT: &str = "Auto";

/// The height to create a drop-down with: one that includes its open
/// list.
pub(super) fn color_drop_down_height(edit_height: i32) -> i32 {
    edit_height + DROPPED_ITEMS * item_height(edit_height) + 2
}

/// An item's height: a closed drop-down is as tall as an edit field.
fn item_height(edit_height: i32) -> i32 {
    edit_height - DROP_DOWN_EDGES
}

/// Fills a new drop-down: Auto first when `auto`, then the palette.
pub(super) unsafe fn fill_color_drop_down(combo: HWND, auto: bool) {
    let palette = 0..PALETTE_COLORS;
    for data in auto.then_some(AUTO_ITEM).into_iter().chain(palette) {
        SendMessageW(combo, CB_ADDSTRING, 0, data as LPARAM);
    }
}

/// Picks `color`, `None` for Auto, in a drop-down `fill_color_drop_down`
/// filled, with Auto when `auto`.
pub(super) unsafe fn select_color(combo: HWND, auto: bool, color: Option<PaletteIndex>) {
    let index = match color {
        Some(index) => usize::from(auto) + usize::from(index.get()),
        None => {
            assert!(auto, "only a drop-down with Auto picks it");
            0
        }
    };
    SendMessageW(combo, CB_SETCURSEL, index, 0);
}

/// The drop-down's pick: `None` for Auto.
pub(super) unsafe fn selected_color(combo: HWND) -> Option<PaletteIndex> {
    let index = SendMessageW(combo, CB_GETCURSEL, 0, 0);
    let index = usize::try_from(index).expect("a colour drop-down always has a pick");
    item_color(SendMessageW(combo, CB_GETITEMDATA, index, 0) as usize)
}

fn item_color(data: usize) -> Option<PaletteIndex> {
    match data {
        AUTO_ITEM => None,
        index => Some(
            PaletteIndex::new(u8::try_from(index).expect("an item is a palette index"))
                .expect("an item is a palette index"),
        ),
    }
}

/// `WM_MEASUREITEM` for a dialog whose owner-drawn controls are colour
/// drop-downs.
pub(super) unsafe fn measure_color_item(item: &mut MEASUREITEMSTRUCT, edit_height: i32) {
    assert_eq!(
        item.CtlType, ODT_COMBOBOX,
        "only the colour drop-downs are owner-drawn"
    );
    item.itemHeight = item_height(edit_height) as u32;
}

/// `WM_DRAWITEM`: an item's numbered palette box, or Auto in the list's
/// text, on the list's background or the selection's.
pub(super) unsafe fn draw_color_item(item: &DRAWITEMSTRUCT) {
    assert_eq!(
        item.CtlType, ODT_COMBOBOX,
        "only the colour drop-downs are owner-drawn"
    );
    let (background, text) = match item.itemState & ODS_SELECTED {
        0 => (COLOR_WINDOW, COLOR_WINDOWTEXT),
        _ => (COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT),
    };
    FillRect(
        item.hDC,
        &item.rcItem,
        GetSysColorBrush(background) as HBRUSH,
    );
    let mode = SetBkMode(item.hDC, TRANSPARENT as i32);
    match item_color(item.itemData) {
        Some(index) => {
            let dpi = Dpi::of_window(item.hwndItem);
            let (inset_x, inset_y) = (dpi.px(2), dpi.px(1));
            let cell = RECT {
                left: item.rcItem.left + inset_x,
                top: item.rcItem.top + inset_y,
                right: item.rcItem.right - inset_x,
                bottom: item.rcItem.bottom - inset_y,
            };
            paint_palette_cells(item.hDC, &[cell], index, None, true);
        }
        None => {
            // The combo box has its font in the DC.
            let mut label = AUTO_TEXT.encode_utf16().collect::<Vec<_>>();
            let mut rect = item.rcItem;
            SetTextColor(item.hDC, GetSysColor(text));
            DrawTextW(
                item.hDC,
                label.as_mut_ptr(),
                label.len() as i32,
                &mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }
    }
    SetBkMode(item.hDC, mode);
    if item.itemState & ODS_FOCUS != 0 {
        DrawFocusRect(item.hDC, &item.rcItem);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_is_a_palette_index_or_auto_past_the_palette() {
        assert_eq!(item_color(0), Some(PaletteIndex::new(0).unwrap()));
        assert_eq!(item_color(15), Some(PaletteIndex::new(15).unwrap()));
        assert_eq!(item_color(AUTO_ITEM), None);
    }
}
