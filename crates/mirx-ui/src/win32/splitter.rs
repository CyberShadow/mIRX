//! The treebar's splitter, as mIRC's: the band between the treebar and the
//! MDI client. Over it the cursor is a left-right arrow; dragging it resizes
//! the treebar live, and the release saves the width.
//!
//! The band is bare frame client area, which the frame's class brush
//! paints. The frame gets the band's mouse messages; the treebar and the
//! MDI client cover the rest.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, HTCLIENT, IDC_SIZEWE, SetCursor};

use super::dpi::Dpi;
use super::settings::TreebarWidth;
use super::*;

/// The band's width, at 96 dpi.
pub(super) const SPLITTER_WIDTH: i32 = 4;

/// What the MDI client keeps of a frame too narrow for the treebar, at
/// 96 dpi.
const MDI_MIN_WIDTH: i32 = 100;

/// How wide the treebar is laid out in a frame `frame_width` wide: `wanted`
/// (physical pixels), squeezed so the band and the MDI client's minimum
/// still fit. The squeeze is the layout's only; the setting keeps its width.
pub(super) fn tree_width(wanted: i32, frame_width: i32, dpi: Dpi) -> i32 {
    wanted.min(widest_tree(frame_width, dpi)).max(0)
}

fn widest_tree(frame_width: i32, dpi: Dpi) -> i32 {
    frame_width - dpi.px(SPLITTER_WIDTH) - dpi.px(MDI_MIN_WIDTH)
}

/// The band, in frame client coordinates: right and bottom exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Band {
    pub(super) left: i32,
    pub(super) right: i32,
    pub(super) top: i32,
    pub(super) bottom: i32,
}

impl Band {
    pub(super) fn contains(self, x: i32, y: i32) -> bool {
        (self.left..self.right).contains(&x) && (self.top..self.bottom).contains(&y)
    }
}

/// A drag of the band: the treebar width it shows, not saved until the
/// release. `WindowState::treebar_drag` is `Some` exactly while the frame
/// has the mouse captured for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TreebarDrag {
    /// Where in the band the button went down, from the treebar's right
    /// edge: the band stays under the cursor at that spot.
    grab_offset: i32,
    width: i32,
}

impl TreebarDrag {
    /// The button went down at `x` with the treebar `width` wide.
    pub(super) fn grab(width: i32, x: i32) -> Self {
        Self {
            grab_offset: x - width,
            width,
        }
    }

    /// The treebar width shown, in physical pixels.
    pub(super) fn width(self) -> i32 {
        self.width
    }

    /// The cursor moved to `x`: the treebar follows, within the setting's
    /// range at `dpi` and leaving the MDI client its minimum. Whether the
    /// width changed.
    pub(super) fn drag_to(&mut self, x: i32, frame_width: i32, dpi: Dpi) -> bool {
        let width = (x - self.grab_offset)
            .min(widest_tree(frame_width, dpi))
            .clamp(
                dpi.px(TreebarWidth::MIN.pixels()),
                dpi.px(TreebarWidth::MAX.pixels()),
            );
        let changed = width != self.width;
        self.width = width;
        changed
    }
}

/// The band as the frame's children are laid out; `None` while the treebar
/// is hidden, which leaves no band.
unsafe fn band(hwnd: HWND) -> Option<Band> {
    let controls = &state_ref(hwnd).controls;
    let tree = frame_child_rect(hwnd, controls.tree)?;
    let mdi_client =
        frame_child_rect(hwnd, controls.mdi_client).expect("the MDI client is always shown");
    Some(Band {
        left: tree.right,
        right: mdi_client.left,
        top: tree.top,
        bottom: tree.bottom,
    })
}

/// A mouse message's point, in the receiving window's client coordinates;
/// negative left of or above it while the mouse is captured.
fn mouse_point(lparam: LPARAM) -> (i32, i32) {
    (
        i32::from(lparam as u16 as i16),
        i32::from((lparam >> 16) as u16 as i16),
    )
}

unsafe fn frame_width(hwnd: HWND) -> i32 {
    let mut client = RECT::default();
    GetClientRect(hwnd, &mut client);
    client.right - client.left
}

/// `WM_SETCURSOR`: whether the cursor is over the band, where it is set to
/// the left-right arrow. Children's `WM_SETCURSOR`s pass through the frame
/// too; only the frame's own client area is the band's.
pub(super) unsafe fn set_cursor(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> bool {
    if wparam as HWND != hwnd || u32::from(lparam as u16) != HTCLIENT {
        return false;
    }
    let mut cursor = POINT::default();
    GetCursorPos(&mut cursor);
    ScreenToClient(hwnd, &mut cursor);
    if !band(hwnd).is_some_and(|band| band.contains(cursor.x, cursor.y)) {
        return false;
    }
    SetCursor(LoadCursorW(null_mut(), IDC_SIZEWE));
    true
}

/// `WM_LBUTTONDOWN`: whether it went down on the band, which starts a drag.
pub(super) unsafe fn begin_drag(hwnd: HWND, lparam: LPARAM) -> bool {
    let (x, y) = mouse_point(lparam);
    let Some(band) = band(hwnd).filter(|band| band.contains(x, y)) else {
        return false;
    };
    state_mut(hwnd).treebar_drag = Some(TreebarDrag::grab(band.left, x));
    SetCapture(hwnd);
    true
}

/// `WM_MOUSEMOVE`: whether a drag is on, which the move continues.
pub(super) unsafe fn continue_drag(hwnd: HWND, lparam: LPARAM) -> bool {
    let (frame_width, dpi) = (frame_width(hwnd), Dpi::of_window(hwnd));
    let Some(drag) = state_mut(hwnd).treebar_drag.as_mut() else {
        return false;
    };
    if drag.drag_to(mouse_point(lparam).0, frame_width, dpi) {
        layout_controls(hwnd);
    }
    true
}

/// `WM_LBUTTONUP`: whether a drag is on, which the release ends. The
/// treebar keeps the width the release is at, saved at 96 dpi.
pub(super) unsafe fn end_drag(hwnd: HWND, lparam: LPARAM) -> Option<Result<(), String>> {
    let (frame_width, dpi) = (frame_width(hwnd), Dpi::of_window(hwnd));
    let state = state_mut(hwnd);
    let mut drag = state.treebar_drag.take()?;
    // Taken first: the release's `WM_CAPTURECHANGED` must find no drag to
    // cancel.
    ReleaseCapture();
    drag.drag_to(mouse_point(lparam).0, frame_width, dpi);
    let logical = u16::try_from(dpi.logical(drag.width())).expect("a treebar width fits u16");
    let width = TreebarWidth::new(logical).expect("a dragged width is in the setting's range");
    let saved = state
        .settings
        .update(|settings| settings.treebar.width = width);
    layout_controls(hwnd);
    Some(saved)
}

/// `WM_CAPTURECHANGED`: whether a drag was on, which losing the mouse
/// (another window took it) cancels. The treebar goes back to its saved
/// width.
pub(super) unsafe fn cancel_drag(hwnd: HWND) -> bool {
    if state_mut(hwnd).treebar_drag.take().is_none() {
        return false;
    }
    layout_controls(hwnd);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_band_holds_its_left_and_top_edges_but_not_its_right_and_bottom() {
        let band = Band {
            left: 240,
            right: 244,
            top: 28,
            bottom: 600,
        };
        assert!(band.contains(240, 28));
        assert!(band.contains(243, 599));
        assert!(!band.contains(239, 100));
        assert!(!band.contains(244, 100));
        assert!(!band.contains(241, 27));
        assert!(!band.contains(241, 600));
    }

    #[test]
    fn a_narrow_frame_squeezes_the_treebar_for_the_windows() {
        let dpi = Dpi::STANDARD;
        assert_eq!(tree_width(240, 1024, dpi), 240);
        // 4 px of band and 100 of windows.
        assert_eq!(tree_width(240, 300, dpi), 196);
        assert_eq!(tree_width(240, 50, dpi), 0);
        assert_eq!(tree_width(360, 600, Dpi::new(144)), 360);
        assert_eq!(tree_width(360, 400, Dpi::new(144)), 400 - 6 - 150);
    }

    #[test]
    fn the_band_stays_under_the_cursor_where_it_was_grabbed() {
        let dpi = Dpi::STANDARD;
        let mut drag = TreebarDrag::grab(240, 242);
        assert_eq!(drag.width(), 240);
        assert!(!drag.drag_to(242, 1024, dpi));
        assert!(drag.drag_to(322, 1024, dpi));
        assert_eq!(drag.width(), 320);
        assert!(drag.drag_to(200, 1024, dpi));
        assert_eq!(drag.width(), 198);
    }

    #[test]
    fn a_drag_stops_at_the_setting_range_and_the_windows_minimum() {
        let dpi = Dpi::STANDARD;
        let mut drag = TreebarDrag::grab(240, 240);
        drag.drag_to(-50, 1024, dpi);
        assert_eq!(drag.width(), 40);
        drag.drag_to(2000, 2000, dpi);
        assert_eq!(drag.width(), 800);
        drag.drag_to(1000, 600, dpi);
        assert_eq!(drag.width(), 600 - 4 - 100);
        // A frame too narrow for even the minimum keeps the minimum; the
        // layout squeezes it.
        drag.drag_to(100, 120, dpi);
        assert_eq!(drag.width(), 40);

        let dpi = Dpi::new(144);
        drag.drag_to(-50, 2000, dpi);
        assert_eq!(drag.width(), 60);
        drag.drag_to(5000, 2000, dpi);
        assert_eq!(drag.width(), 1200);
    }
}
