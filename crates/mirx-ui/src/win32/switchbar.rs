//! The switchbar's geometry: mIRC's switchbar with "Lines: Auto", whose
//! fixed-width buttons wrap onto as many rows as the frame needs.
//!
//! Pure; `layout_controls` reads the toolbar's rows and button height and
//! sizes the switchbar window from them.

/// The height of a one-row switchbar.
pub(super) const SWITCHBAR_ROW_HEIGHT: i32 = 28;

/// The width of every switchbar button, mIRC's Display "Buttons" size.
pub(super) const SWITCHBAR_BUTTON_WIDTH: i32 = 120;

/// How tall the switchbar is with `rows` rows of buttons `button_height`
/// tall: one row is `SWITCHBAR_ROW_HEIGHT`, and each further row adds a
/// button height, so the last row is the one a single row shows.
pub(super) fn switchbar_height(rows: u32, button_height: i32) -> i32 {
    assert!(rows >= 1, "a toolbar always lays out at least one row");
    SWITCHBAR_ROW_HEIGHT + (rows as i32 - 1) * button_height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_row_keeps_the_row_height_and_more_rows_add_buttons() {
        assert_eq!(switchbar_height(1, 36), SWITCHBAR_ROW_HEIGHT);
        assert_eq!(switchbar_height(3, 36), SWITCHBAR_ROW_HEIGHT + 72);
    }

    #[test]
    #[should_panic(expected = "at least one row")]
    fn zero_rows_is_a_bug() {
        switchbar_height(0, 36);
    }
}
