//! The switchbar's geometry: mIRC's switchbar with "Lines: Auto", whose
//! fixed-width buttons wrap onto as many rows as the frame needs, but no
//! more rows than leave the windows under it their least height.
//!
//! Pure; `layout_controls` reads the toolbar's rows and button height and
//! sizes the switchbar window from them.

use super::dpi::Dpi;

/// The gap under the last row of buttons, at 96 dpi, so a checked button's
/// frame stays clear of the windows' borders below.
const SWITCHBAR_GAP: i32 = 2;

/// The least height the switchbar leaves the windows, at 96 dpi, as the
/// treebar leaves them their least width.
const MDI_MIN_HEIGHT: i32 = 100;

/// How tall the switchbar is at `dpi` with `rows` rows of buttons
/// `button_height` (physical pixels) tall, when the switchbar and the
/// windows share `height` between them: the rows, and the gap under them.
/// Rows that would leave the windows less than their least height are cut
/// (a big account's thousand buttons take more rows than any screen is
/// high); one row always shows.
pub(super) fn switchbar_height(rows: u32, button_height: i32, height: i32, dpi: Dpi) -> i32 {
    assert!(rows >= 1, "a toolbar always lays out at least one row");
    assert!(button_height > 0, "a toolbar's buttons have a height");
    let gap = dpi.px(SWITCHBAR_GAP);
    let fitting_rows = (height - dpi.px(MDI_MIN_HEIGHT) - gap) / button_height;
    let shown_rows = (rows as i32).min(fitting_rows).max(1);
    shown_rows * button_height + gap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_and_the_gap_under_them() {
        assert_eq!(switchbar_height(1, 22, 600, Dpi::STANDARD), 24);
        assert_eq!(switchbar_height(3, 22, 600, Dpi::STANDARD), 3 * 22 + 2);
    }

    #[test]
    fn the_gap_scales_with_the_dpi() {
        assert_eq!(switchbar_height(1, 30, 900, Dpi::new(144)), 33);
        assert_eq!(switchbar_height(2, 30, 900, Dpi::new(144)), 2 * 30 + 3);
    }

    #[test]
    fn rows_that_would_leave_the_windows_less_than_their_least_height_are_cut() {
        // 600 - 100 - 2 = 498 px: 22 rows of 22 px.
        assert_eq!(switchbar_height(125, 22, 600, Dpi::STANDARD), 22 * 22 + 2);
        assert_eq!(switchbar_height(22, 22, 600, Dpi::STANDARD), 22 * 22 + 2);
        assert_eq!(switchbar_height(21, 22, 600, Dpi::STANDARD), 21 * 22 + 2);
        // 150 px of least height at 144 dpi.
        assert_eq!(
            switchbar_height(125, 30, 900, Dpi::new(144)),
            (900 - 150 - 3) / 30 * 30 + 3
        );
    }

    #[test]
    fn one_row_shows_however_low_the_frame() {
        assert_eq!(switchbar_height(5, 22, 50, Dpi::STANDARD), 24);
        assert_eq!(switchbar_height(5, 22, 0, Dpi::STANDARD), 24);
    }

    #[test]
    #[should_panic(expected = "at least one row")]
    fn zero_rows_is_a_bug() {
        switchbar_height(0, 22, 600, Dpi::STANDARD);
    }
}
