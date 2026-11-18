//! The mIRC window model: every channel, query and status window is an MDI
//! child holding its own content above its own editbox.

/// A child control's rectangle in its MDI child's client coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ChildRect {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) width: i32,
    pub(super) height: i32,
}

/// Where an MDI child's controls go: the content fills the window above the
/// editbox, which spans the full width at the bottom, as in mIRC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ChildLayout {
    pub(super) content: ChildRect,
    pub(super) edit: ChildRect,
}

pub(super) fn child_layout(client_width: i32, client_height: i32, edit_height: i32) -> ChildLayout {
    let edit_height = edit_height.min(client_height);
    let content_height = client_height - edit_height;
    ChildLayout {
        content: ChildRect {
            x: 0,
            y: 0,
            width: client_width,
            height: content_height,
        },
        edit: ChildRect {
            x: 0,
            y: content_height,
            width: client_width,
            height: edit_height,
        },
    }
}

const MEMBER_LIST_WIDTH: i32 = 180;
const MEMBER_LIST_GAP: i32 = 4;

/// How a channel window's content splits between the text area and the nick
/// list, in the content's client coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RoomContentLayout {
    pub(super) timeline_width: i32,
    pub(super) members_x: i32,
    pub(super) members_width: i32,
}

/// mIRC always keeps the nick list in a channel window and shrinks the text
/// area; in a narrow window the list takes at most a third.
pub(super) fn room_content_layout(width: i32) -> RoomContentLayout {
    let members_width = MEMBER_LIST_WIDTH.min(width / 3);
    let members_x = width - members_width;
    RoomContentLayout {
        timeline_width: (members_x - MEMBER_LIST_GAP).max(0),
        members_x,
        members_width,
    }
}

/// A room window's caption: its label, then `: topic` as in mIRC's
/// `#chan [+modes]: topic`. mIRC omits the suffix when there is no topic, and
/// a caption is a single line.
pub(super) fn room_caption(label: &str, topic: Option<&str>) -> String {
    let topic = topic
        .unwrap_or_default()
        .split(['\r', '\n'])
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if topic.is_empty() {
        label.to_owned()
    } else {
        format!("{label}: {topic}")
    }
}

/// The Status window's caption. mIRC shows `Status: nick [+modes] on
/// Network`; a Matrix user ID names both the account and, by its server part,
/// the network.
pub(super) fn status_caption(own_user_id: Option<&str>) -> String {
    match own_user_id {
        Some(user_id) => format!("Status: {user_id}"),
        None => "Status".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_content_layout_keeps_the_nick_list_at_every_width() {
        assert_eq!(
            room_content_layout(800),
            RoomContentLayout {
                timeline_width: 616,
                members_x: 620,
                members_width: 180,
            }
        );
        // A tiled window: the list shrinks to a third.
        assert_eq!(
            room_content_layout(250),
            RoomContentLayout {
                timeline_width: 163,
                members_x: 167,
                members_width: 83,
            }
        );
        assert_eq!(
            room_content_layout(0),
            RoomContentLayout {
                timeline_width: 0,
                members_x: 0,
                members_width: 0,
            }
        );
    }

    #[test]
    fn status_caption_names_the_signed_in_account() {
        assert_eq!(status_caption(None), "Status");
        assert_eq!(
            status_caption(Some("@me:example.org")),
            "Status: @me:example.org"
        );
    }

    #[test]
    fn room_caption_appends_the_topic() {
        assert_eq!(
            room_caption("Ops [e2ee]", Some("incident review")),
            "Ops [e2ee]: incident review"
        );
    }

    #[test]
    fn room_caption_omits_a_missing_or_cleared_topic() {
        assert_eq!(room_caption("Ops", None), "Ops");
        assert_eq!(room_caption("Ops", Some("")), "Ops");
        assert_eq!(room_caption("Ops", Some("\n")), "Ops");
    }

    #[test]
    fn room_caption_flattens_a_multiline_topic() {
        assert_eq!(
            room_caption("Ops", Some("incident review\r\nsee the doc\nthanks")),
            "Ops: incident review see the doc thanks"
        );
    }

    #[test]
    fn child_layout_puts_the_editbox_under_the_content() {
        assert_eq!(
            child_layout(600, 400, 28),
            ChildLayout {
                content: ChildRect {
                    x: 0,
                    y: 0,
                    width: 600,
                    height: 372,
                },
                edit: ChildRect {
                    x: 0,
                    y: 372,
                    width: 600,
                    height: 28,
                },
            }
        );
    }

    #[test]
    fn child_layout_gives_a_short_window_to_the_editbox() {
        let layout = child_layout(600, 20, 28);
        assert_eq!(layout.content.height, 0);
        assert_eq!(layout.edit.y, 0);
        assert_eq!(layout.edit.height, 20);
    }

    #[test]
    fn child_layout_handles_an_empty_client_area() {
        let layout = child_layout(0, 0, 28);
        assert_eq!(
            layout.content,
            ChildRect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            }
        );
        assert_eq!(
            layout.edit,
            ChildRect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            }
        );
    }
}
