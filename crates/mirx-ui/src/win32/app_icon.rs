//! mirx's icon and the glyphs' image lists, from the exe's resources (`build/`).

use std::ffi::c_void;

use windows_sys::Win32::UI::Controls::{
    HIMAGELIST, ILC_COLOR32, ILC_MASK, ImageList_Create, ImageList_Destroy, ImageList_ReplaceIcon,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetSystemMetrics, HICON, IMAGE_ICON, LR_SHARED, LoadImageW, SM_CXICON,
    SM_CXSMICON, SM_CYICON, SM_CYSMICON,
};

use super::last_os_error;

/// The icon group's resource id. `build/resources.rs` has the same constant;
/// the `app_resources` tests load this id from the exe.
pub(super) const APP_ICON_ID: u16 = 1;

/// The toolbar's glyphs' icon groups. `build/glyphs.rs` gives its glyphs
/// these ids; the `app_resources` tests load each.
#[cfg(test)]
pub(super) const TOOLBAR_ICON_IDS: std::ops::RangeInclusive<u16> = 101..=113;

/// The windows' own glyphs' icon groups (`window_icons.rs` names each).
#[cfg(test)]
pub(super) const WINDOW_ICON_IDS: std::ops::RangeInclusive<u16> = 114..=118;

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

/// An image list of the exe's icon groups `ids`, in order, at `size` px
/// square (`LoadImageW` picks the group's nearest size). The caller owns
/// it: the controls it is set on do not destroy it.
pub(super) unsafe fn image_list(
    instance: *mut c_void,
    ids: &[u16],
    size: i32,
) -> Result<HIMAGELIST, String> {
    let count = i32::try_from(ids.len()).expect("an image list holds few icons");
    let images = unsafe { ImageList_Create(size, size, ILC_COLOR32 | ILC_MASK, count, 0) };
    if images == 0 {
        return Err(last_os_error("ImageList_Create"));
    }
    for &id in ids {
        let icon = unsafe {
            LoadImageW(
                instance,
                id as usize as *const u16,
                IMAGE_ICON,
                size,
                size,
                0,
            )
        };
        assert!(
            !icon.is_null(),
            "the exe embeds icon group {id} (build/main.rs links it)"
        );
        let index = unsafe { ImageList_ReplaceIcon(images, -1, icon) };
        unsafe { DestroyIcon(icon) };
        if index < 0 {
            let error = last_os_error("ImageList_ReplaceIcon");
            unsafe { ImageList_Destroy(images) };
            return Err(error);
        }
    }
    Ok(images)
}

/// Destroys an `image_list`, or nothing if the frame failed before making
/// it (a null list).
pub(super) unsafe fn destroy_image_list(images: HIMAGELIST) {
    unsafe { ImageList_Destroy(images) };
}
