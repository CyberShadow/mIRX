//! Lines a typed command printed in a room or thread window, like mIRC's
//! `* /kick: insufficient parameters` or `-> *Alice* hi`. They exist only in
//! this client, so they live beside the room's activities rather than among
//! them, and merge into the timeline by time.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EchoLine {
    /// Unique within the room's echo, so identical lines stay distinct.
    pub(super) sequence: u64,
    pub(super) timestamp_ms: u64,
    /// The thread window the line was printed in; `None` for the room window.
    pub(super) thread_root: Option<String>,
    /// The line after its timestamp.
    pub(super) text: String,
}

#[derive(Debug, Default, Clone)]
pub(super) struct EchoLog {
    lines: Vec<EchoLine>,
    next_sequence: u64,
}

impl EchoLog {
    pub(super) fn push(&mut self, timestamp_ms: u64, thread_root: Option<String>, text: String) {
        self.lines.push(EchoLine {
            sequence: self.next_sequence,
            timestamp_ms,
            thread_root,
            text,
        });
        self.next_sequence += 1;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts<'a>(lines: impl Iterator<Item = &'a EchoLine>) -> Vec<(u64, &'a str)> {
        lines
            .map(|line| (line.sequence, line.text.as_str()))
            .collect()
    }

    #[test]
    fn lines_are_per_window_and_room_clear_keeps_thread_lines() {
        let mut log = EchoLog::default();
        log.push(1, None, "room".to_owned());
        log.push(2, Some("$root".to_owned()), "thread".to_owned());
        log.push(3, None, "room".to_owned());

        assert_eq!(texts(log.lines_for(None)), vec![(0, "room"), (2, "room")]);
        assert_eq!(texts(log.lines_for(Some("$root"))), vec![(1, "thread")]);
        assert_eq!(texts(log.lines_for(Some("$other"))), vec![]);

        log.clear_room_window();
        assert_eq!(texts(log.lines_for(None)), vec![]);
        assert_eq!(texts(log.lines_for(Some("$root"))), vec![(1, "thread")]);
        log.push(4, None, "after".to_owned());
        assert_eq!(texts(log.lines_for(None)), vec![(3, "after")]);
    }
}
