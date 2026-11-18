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

#[cfg(test)]
mod tests {
    use super::*;

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
