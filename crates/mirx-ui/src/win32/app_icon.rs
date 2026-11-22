//! mirx's icon, from the exe's resources (`build/`).

use std::ffi::c_void;

use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, HICON, IMAGE_ICON, LR_SHARED, LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON,
    SM_CYSMICON,
};

/// The icon group's resource id. `build/resources.rs` has the same constant;
/// the `app_resources` tests load this id from the exe.
pub(super) const APP_ICON_ID: u16 = 1;

/// The toolbar's glyphs' icon groups. `build/toolbar_icons.rs` gives its
/// glyphs these ids; the `app_resources` tests load each.
#[cfg(test)]
pub(super) const TOOLBAR_ICON_IDS: std::ops::RangeInclusive<u16> = 101..=113;

#[derive(Clone, Copy)]
pub(super) enum IconSize {
    /// Captions, the maximized child's menubar button, the tray.
    Small,
    /// The taskbar and Alt+Tab.
    Large,
}

/// The icon at the system's metric for `size`. Shared (`LR_SHARED`): it is
/// never destroyed, as class and tray icons live as long as the process.
pub(super) fn app_icon(instance: *mut c_void, size: IconSize) -> HICON {
    let (cx, cy) = match size {
        IconSize::Small => (SM_CXSMICON, SM_CYSMICON),
        IconSize::Large => (SM_CXICON, SM_CYICON),
    };
    let icon = unsafe {
        LoadImageW(
            instance,
            APP_ICON_ID as usize as *const u16,
            IMAGE_ICON,
            GetSystemMetrics(cx),
            GetSystemMetrics(cy),
            LR_SHARED,
        )
    };
    assert!(
        !icon.is_null(),
        "the exe embeds mirx's icon (build/main.rs links it)"
    );
    icon
}
