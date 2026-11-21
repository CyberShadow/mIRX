//! Lines a typed command printed in a room or thread window, like mIRC's
//! `* /kick: insufficient parameters` or `-> *Alice* hi`. They exist only in
//! this client, so they live beside the room's activities rather than among
//! them, and merge into the timeline by time.

use super::StatusLineKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EchoLine {
    /// The line's place in the room's arrival order (`RoomArrivals`):
    /// unique within the room, so identical lines stay distinct.
    pub(super) arrival: u64,
    pub(super) timestamp_ms: u64,
    /// The thread window the line was printed in; `None` for the room window.
    pub(super) thread_root: Option<String>,
    /// `Error` lines are red, as in Status.
    pub(super) kind: StatusLineKind,
    /// The line after its timestamp.
    pub(super) text: String,
}

#[derive(Debug, Default, Clone)]
pub(super) struct EchoLog {
    lines: Vec<EchoLine>,
}

impl EchoLog {
    /// Use `RoomViewModel::push_echo`, which numbers the line.
    pub(super) fn push(
        &mut self,
        arrival: u64,
        timestamp_ms: u64,
        thread_root: Option<String>,
        kind: StatusLineKind,
        text: String,
    ) {
        assert!(
            self.lines.last().is_none_or(|last| last.arrival < arrival),
            "echo lines arrive in order"
        );
        self.lines.push(EchoLine {
            arrival,
            timestamp_ms,
            thread_root,
            kind,
            text,
        });
    }

    /// The lines of the room window (`None`) or of one thread window.
    pub(super) fn lines_for<'a>(
        &'a self,
        thread_root: Option<&'a str>,
    ) -> impl Iterator<Item = &'a EchoLine> + 'a {
        self.lines
            .iter()
            .filter(move |line| line.thread_root.as_deref() == thread_root)
    }

    /// `/clear` in the room window; thread windows keep their lines.
    pub(super) fn clear_room_window(&mut self) {
        self.lines.retain(|line| line.thread_root.is_some());
    }

    /// Every window's lines, oldest first.
    pub(super) fn lines(&self) -> &[EchoLine] {
        &self.lines
    }

    /// Drops the `count` oldest lines (the window buffer).
    pub(super) fn drop_oldest(&mut self, count: usize) {
        self.lines.drain(..count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts<'a>(lines: impl Iterator<Item = &'a EchoLine>) -> Vec<(u64, &'a str)> {
        lines
            .map(|line| (line.arrival, line.text.as_str()))
            .collect()
    }

    #[test]
    fn lines_are_per_window_and_room_clear_keeps_thread_lines() {
        let mut log = EchoLog::default();
        log.push(0, 1, None, StatusLineKind::Info, "room".to_owned());
        log.push(
            1,
            2,
            Some("$root".to_owned()),
            StatusLineKind::Info,
            "thread".to_owned(),
        );
        log.push(2, 3, None, StatusLineKind::Info, "room".to_owned());

        assert_eq!(texts(log.lines_for(None)), vec![(0, "room"), (2, "room")]);
        assert_eq!(texts(log.lines_for(Some("$root"))), vec![(1, "thread")]);
        assert_eq!(texts(log.lines_for(Some("$other"))), vec![]);

        log.clear_room_window();
        assert_eq!(texts(log.lines_for(None)), vec![]);
        assert_eq!(texts(log.lines_for(Some("$root"))), vec![(1, "thread")]);
        log.push(3, 4, None, StatusLineKind::Info, "after".to_owned());
        assert_eq!(texts(log.lines_for(None)), vec![(3, "after")]);
    }
}
