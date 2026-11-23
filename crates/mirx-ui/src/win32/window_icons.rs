//! The icons the treebar and the switchbar show beside each window's name,
//! as mIRC's do: one per kind of window, in Matrix's kinds. The glyphs are
//! the exe's icon groups (`build/glyphs.rs`), in one image list at the
//! frame's DPI that both bars share.

use windows_sys::Win32::UI::Controls::HIMAGELIST;

use super::*;

/// The icons' size at 96 dpi.
pub(super) const WINDOW_ICON_SIZE: i32 = 16;

/// What kind of window an item or button stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WindowIcon {
    Status,
    Room,
    Direct,
    Invite,
    Thread,
    RoomList,
    /// A treebar folder: a space has no window.
    Space,
}

impl WindowIcon {
    /// Every icon, in image-list order: the declaration order.
    const ALL: [WindowIcon; 7] = [
        WindowIcon::Status,
        WindowIcon::Room,
        WindowIcon::Direct,
        WindowIcon::Invite,
        WindowIcon::Thread,
        WindowIcon::RoomList,
        WindowIcon::Space,
    ];

    /// Its glyph's icon group. The Rooms List and a space share the
    /// toolbar's List Rooms page and Join Room folder.
    fn resource_id(self) -> u16 {
        match self {
            WindowIcon::Status => 114,
            WindowIcon::Room => 115,
            WindowIcon::Direct => 116,
            WindowIcon::Invite => 117,
            WindowIcon::Thread => 118,
            WindowIcon::RoomList => 104,
            WindowIcon::Space => 105,
        }
    }

    /// Its index in the image list.
    pub(super) fn image(self) -> i32 {
        self as i32
    }

    /// The icon of `view_id`'s window.
    pub(super) fn of_view(model: &UiModel, view_id: &ViewId) -> Self {
        match view_id {
            ViewId::Status => WindowIcon::Status,
            ViewId::RoomList => WindowIcon::RoomList,
            ViewId::Thread { .. } => WindowIcon::Thread,
            ViewId::Room(room_id) => WindowIcon::of_room(
                room_by_id(&model.rooms, room_id).expect("a room window's room is in the model"),
            ),
        }
    }

    /// A navigable room's window's icon, by its kind.
    pub(super) fn of_room(room: &RoomSummary) -> Self {
        match room_window_kind(room) {
            RoomWindowKind::Channel => WindowIcon::Room,
            RoomWindowKind::Query => WindowIcon::Direct,
            RoomWindowKind::Invite => WindowIcon::Invite,
        }
    }
}

/// The bars' image list: every icon at the frame's DPI, `WindowIcon::image`
/// indexing it. The frame owns it.
pub(super) unsafe fn create_window_icons(
    frame: HWND,
    instance: *mut c_void,
) -> Result<HIMAGELIST, String> {
    unsafe {
        app_icon::image_list(
            instance,
            &WindowIcon::ALL.map(WindowIcon::resource_id),
            Dpi::of_window(frame).px(WINDOW_ICON_SIZE),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::app_icon::{TOOLBAR_ICON_IDS, WINDOW_ICON_IDS};
    use super::*;

    #[test]
    fn the_image_list_order_is_the_declaration_order() {
        for (index, icon) in WindowIcon::ALL.iter().enumerate() {
            assert_eq!(icon.image(), index as i32, "{icon:?}");
        }
        assert_eq!(
            WindowIcon::ALL.len(),
            WindowIcon::Space.image() as usize + 1
        );
    }

    #[test]
    fn every_window_glyph_is_used_once_and_the_rest_are_the_toolbars() {
        let ids = WindowIcon::ALL.map(WindowIcon::resource_id);
        for id in WINDOW_ICON_IDS {
            assert_eq!(ids.iter().filter(|&&used| used == id).count(), 1, "{id}");
        }
        for id in ids {
            assert!(
                WINDOW_ICON_IDS.contains(&id) || TOOLBAR_ICON_IDS.contains(&id),
                "{id}"
            );
        }
    }
}
