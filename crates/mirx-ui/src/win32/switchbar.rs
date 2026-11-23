//! The switchbar's geometry: mIRC's switchbar with "Lines: Auto", whose
//! fixed-width buttons wrap onto as many rows as the frame needs.
//!
//! Pure; `layout_controls` reads the toolbar's rows and button height and
//! sizes the switchbar window from them.

use super::dpi::Dpi;

/// The gap under the last row of buttons, at 96 dpi, so a checked button's
/// frame stays clear of the windows' borders below.
const SWITCHBAR_GAP: i32 = 2;

/// How tall the switchbar is at `dpi` with `rows` rows of buttons
/// `button_height` (physical pixels) tall: the rows, and the gap under them.
pub(super) fn switchbar_height(rows: u32, button_height: i32, dpi: Dpi) -> i32 {
    assert!(rows >= 1, "a toolbar always lays out at least one row");
    assert!(button_height > 0, "a toolbar's buttons have a height");
    rows as i32 * button_height + dpi.px(SWITCHBAR_GAP)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_and_the_gap_under_them() {
        assert_eq!(switchbar_height(1, 22, Dpi::STANDARD), 24);
        assert_eq!(switchbar_height(3, 22, Dpi::STANDARD), 3 * 22 + 2);
    }

    #[test]
    fn the_gap_scales_with_the_dpi() {
        assert_eq!(switchbar_height(1, 30, Dpi::new(144)), 33);
        assert_eq!(switchbar_height(2, 30, Dpi::new(144)), 2 * 30 + 3);
    }

    #[test]
    #[should_panic(expected = "at least one row")]
    fn zero_rows_is_a_bug() {
        switchbar_height(0, 22, Dpi::STANDARD);
    }
}
