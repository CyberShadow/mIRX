//! The dots per inch windows are laid out at.
//!
//! mirx is system-DPI-aware (its manifest): every window has the system
//! DPI. Layout lengths are written at 96 dpi ("logical pixels") and scaled
//! with [`Dpi::px`] where a window is laid out, which asks the window for
//! its DPI rather than caching one, so per-monitor awareness can later give
//! each window its own.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};

/// Dots per inch a window is laid out at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Dpi(u32);

impl Dpi {
    /// The DPI the layout lengths are written at: 100% scaling.
    #[cfg(test)]
    pub(super) const STANDARD: Dpi = Dpi(96);

    pub(super) fn new(dpi: u32) -> Self {
        assert!(dpi > 0, "a DPI is positive");
        Self(dpi)
    }

    /// The DPI `hwnd` is laid out at.
    pub(super) unsafe fn of_window(hwnd: HWND) -> Self {
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        assert!(dpi > 0, "GetDpiForWindow needs a window");
        Self(dpi)
    }

    /// The process's system DPI, for what is laid out before any window
    /// exists (the text font at startup).
    pub(super) fn system() -> Self {
        Self::new(unsafe { GetDpiForSystem() })
    }

    /// A 96-dpi length at this DPI, rounded to the nearest pixel.
    pub(super) fn px(self, logical: i32) -> i32 {
        assert!(logical >= 0, "layout lengths are not negative");
        let scaled = (i64::from(logical) * i64::from(self.0) + 48) / 96;
        i32::try_from(scaled).expect("a scaled layout length fits i32")
    }

    /// The character height of a `points` font, rounded to the nearest
    /// pixel.
    pub(super) fn points_to_px(self, points: i32) -> i32 {
        assert!(points >= 0, "font sizes are not negative");
        let scaled = (i64::from(points) * i64::from(self.0) + 36) / 72;
        i32::try_from(scaled).expect("a font height fits i32")
    }
}

#[cfg(test)]
mod tests {
    use super::Dpi;

    #[test]
    fn px_at_96_is_the_identity() {
        for logical in [0, 1, 4, 11, 28, 240, 1024] {
            assert_eq!(Dpi::STANDARD.px(logical), logical);
        }
    }

    #[test]
    fn px_rounds_to_nearest() {
        let cases = [(120, 300, 14), (144, 360, 17), (192, 480, 22)];
        for (dpi, tree, eleven) in cases {
            assert_eq!(Dpi::new(dpi).px(240), tree, "px(240) at {dpi}");
            assert_eq!(Dpi::new(dpi).px(11), eleven, "px(11) at {dpi}");
        }
    }

    #[test]
    fn points_to_px_follows_the_dpi() {
        for (dpi, height) in [(96, 12), (120, 15), (144, 18), (192, 24)] {
            assert_eq!(Dpi::new(dpi).points_to_px(9), height, "9 pt at {dpi}");
        }
    }

    #[test]
    #[should_panic(expected = "a DPI is positive")]
    fn zero_dpi_is_refused() {
        Dpi::new(0);
    }
}
