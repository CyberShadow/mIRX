//! The switchbar's geometry: mIRC's switchbar with "Lines: Auto", whose
//! fixed-width buttons wrap onto as many rows as the frame needs.
//!
//! Pure; `layout_controls` reads the toolbar's rows and button height and
//! sizes the switchbar window from them.

use super::dpi::Dpi;

/// The height of a one-row switchbar, at 96 dpi.
const SWITCHBAR_ROW_HEIGHT: i32 = 28;

/// How tall the switchbar is at `dpi` with `rows` rows of buttons
/// `button_height` (physical pixels) tall: one row is `SWITCHBAR_ROW_HEIGHT`,
/// and each further row adds a button height, so the last row is the one a
/// single row shows.
pub(super) fn switchbar_height(rows: u32, button_height: i32, dpi: Dpi) -> i32 {
    assert!(rows >= 1, "a toolbar always lays out at least one row");
    dpi.px(SWITCHBAR_ROW_HEIGHT) + (rows as i32 - 1) * button_height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_row_keeps_the_row_height_and_more_rows_add_buttons() {
        assert_eq!(switchbar_height(1, 36, Dpi::STANDARD), 28);
        assert_eq!(switchbar_height(3, 36, Dpi::STANDARD), 28 + 72);
    }

    #[test]
    fn the_row_height_scales_with_the_dpi() {
        assert_eq!(switchbar_height(1, 54, Dpi::new(144)), 42);
        assert_eq!(switchbar_height(2, 54, Dpi::new(144)), 42 + 54);
    }

    #[test]
    #[should_panic(expected = "at least one row")]
    fn zero_rows_is_a_bug() {
        switchbar_height(0, 36, Dpi::STANDARD);
    }
}
