//! mIRC's logging: a window's lines appended to `logs\<name>.log` in the
//! profile, each session framed by `Session Start:`, `Session Ident:` and
//! `Session Close:` lines.
//!
//! A log records what arrives while it is on: each line once, when it first
//! shows, as mIRC can only log what it printed. Lines are kept by their
//! place in the window's arrival order (`TimelineRow::arrival`), never by
//! server time: history scrolled back to did not arrive and is never
//! written, while a late event or one from a skewed clock is. Edits and
//! redactions do not rewrite a logged line.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::mirc_format::mirc_input_from_styled;
use super::*;

const LOGS_SUBDIR: &str = "logs";

/// Where the logs go, as Options > IRC > Logging shows it: the same in
/// every profile.
pub(super) fn logs_folder_label() -> String {
    format!("%LOCALAPPDATA%\\{PROFILE_SUBDIR}\\{LOGS_SUBDIR}")
}

/// A window's log.
pub(super) enum ViewLog {
    /// The Rooms List and thread windows: no log of their own. A thread
    /// window's Logging is its room's.
    None,
    /// A room window that has not yet been through Automatically log: an
    /// invite waits until it is joined.
    Undecided,
    Off,
    On(Box<LogSession>),
}

impl ViewLog {
    /// What a new window's log starts as.
    pub(super) fn for_new_view(view_id: &ViewId) -> Self {
        match view_id {
            ViewId::Status => Self::Off,
            ViewId::Room(_) => Self::Undecided,
            ViewId::Thread { .. } | ViewId::RoomList => Self::None,
        }
    }

    pub(super) fn session(&self) -> Option<&LogSession> {
        match self {
            Self::On(session) => Some(session),
            Self::None | Self::Undecided | Self::Off => None,
        }
    }
}

/// The window whose log a window's Logging and `/log` act on: Status and a
/// room window their own, a thread window its room's.
pub(super) fn log_view(view_id: &ViewId) -> Result<ViewId, String> {
    match view_id {
        ViewId::Status | ViewId::Room(_) => Ok(view_id.clone()),
        ViewId::Thread { room_id, .. } => Ok(ViewId::Room(room_id.clone())),
        ViewId::RoomList => Err("the Rooms List has no log".to_owned()),
    }
}

/// The name a room's log goes by: a query's other user, else the room's
/// alias, else its ID. mIRC names a channel's log after the channel.
fn room_log_name(room: &RoomSummary) -> &str {
    match room.direct_user_ids.as_slice() {
        [user_id] if room.is_direct => user_id,
        _ => room.canonical_alias.as_deref().unwrap_or(&room.room_id),
    }
}

/// The log file's name without `.log`: `status`, or a room's log name as
/// Windows allows it in a file name (`#rust:matrix.org` is
/// `#rust.matrix.org`, as mIRC's `#channel.network.log`).
fn log_file_stem(model: &UiModel, view_id: &ViewId) -> String {
    let room_id = match view_id {
        ViewId::Status => return "status".to_owned(),
        ViewId::Room(room_id) => room_id,
        ViewId::Thread { .. } | ViewId::RoomList => {
            unreachable!("only Status and room windows log")
        }
    };
    let name = room_log_name(room_by_id(&model.rooms, room_id).expect("a room window's room"));
    let stem = name
        .chars()
        .map(|ch| match ch {
            ':' => '.',
            '<' | '>' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch < ' ' => '_',
            ch => ch,
        })
        .collect::<String>();
    // Windows drops trailing dots and spaces from file names.
    let stem = stem.trim_end_matches(['.', ' ']);
    // So no stem is a DOS device name such as CON or NUL.
    assert!(
        matches!(stem.chars().next(), Some('#' | '!' | '@')),
        "a room's log name is an alias, a room ID or a user ID: {name}"
    );
    stem.to_owned()
}

/// `Session Ident:`'s text: `Status`, or a room window's title and the name
/// its log goes by, `Rust (#rust:matrix.org)`.
fn session_ident(model: &UiModel, view_id: &ViewId) -> String {
    match view_id {
        ViewId::Status => "Status".to_owned(),
        ViewId::Room(room_id) => {
            let room = room_by_id(&model.rooms, room_id).expect("a room window's room");
            format!(
                "{} ({})",
                view_short_title(model, view_id),
                room_log_name(room)
            )
        }
        ViewId::Thread { .. } | ViewId::RoomList => {
            unreachable!("only Status and room windows log")
        }
    }
}

/// Which of a window's lines a log session writes, and which it wrote.
#[derive(Debug)]
struct LogDiff {
    /// The first arrival the log covers.
    from: u64,
    written: BTreeSet<TimelineRowIdentity>,
}

impl LogDiff {
    fn new(from: u64) -> Self {
        Self {
            from,
            written: BTreeSet::new(),
        }
    }

    /// The text lines of `rows` the log has not written: lines that arrived
    /// (messages, events, echo and Status lines) from `from` on, once each.
    /// A message waiting to be sent is not said yet, and is written once it
    /// is.
    fn lines(&mut self, rows: &[TimelineRow], strip_codes: bool) -> Vec<String> {
        let mut lines = Vec::new();
        for row in rows {
            let Some(arrival) = row.arrival else {
                continue;
            };
            let unsent = matches!(
                row.kind,
                TimelineRowKind::Message {
                    send_state: TimelineMessageSendState::Pending
                        | TimelineMessageSendState::Failed,
                    ..
                }
            );
            if arrival < self.from || unsent || !self.written.insert(row.identity.clone()) {
                continue;
            }
            let text = if strip_codes {
                row.logical_text.clone()
            } else {
                mirc_input_from_styled(&row.logical_text, &row.styles)
            };
            lines.extend(
                text.split('\n')
                    .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned()),
            );
        }
        lines
    }
}

/// One window's open log file. Dropping it ends the session: closing the
/// window, leaving the room, `/log off` and exiting all close the log.
pub(super) struct LogSession {
    path: PathBuf,
    file: File,
    diff: LogDiff,
    clock: UiClock,
    local_time: LocalTimeFn,
}

impl LogSession {
    /// Opens `path` for appending, creating it and its folder, and writes
    /// the session's start; a blank line parts it from the one before. The
    /// session writes the lines that arrive from `from` on.
    fn start(
        path: PathBuf,
        ident: &str,
        from: u64,
        clock: UiClock,
        local_time: LocalTimeFn,
    ) -> io::Result<Self> {
        fs::create_dir_all(path.parent().expect("a log file is in the logs folder"))?;
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        let started_ms = clock.now_ms();
        let separator = if file.metadata()?.len() > 0 {
            "\r\n"
        } else {
            ""
        };
        file.write_all(
            format!(
                "{separator}Session Start: {}\r\nSession Ident: {ident}\r\n",
                local_time(started_ms).asctime()
            )
            .as_bytes(),
        )?;
        Ok(Self {
            path,
            file,
            diff: LogDiff::new(from),
            clock,
            local_time,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// Appends the lines of `rows` not yet logged, unbuffered: the file is
    /// whole after each batch.
    fn write_new_lines(&mut self, rows: &[TimelineRow], strip_codes: bool) -> io::Result<()> {
        let text = self
            .diff
            .lines(rows, strip_codes)
            .into_iter()
            .map(|line| line + "\r\n")
            .collect::<String>();
        self.file.write_all(text.as_bytes())
    }
}

impl Drop for LogSession {
    fn drop(&mut self) {
        let close = format!(
            "Session Close: {}\r\n",
            (self.local_time)(self.clock.now_ms()).asctime()
        );
        // The one error that is only printed: the window is going, or the
        // log already failed, so there is no window left to tell.
        if let Err(error) = self.file.write_all(close.as_bytes()) {
            eprintln!("failed to close the log {}: {error}", self.path.display());
        }
    }
}

/// The rows a window's log writes from: the window's lines, stamped with
/// the log's timestamp setting rather than the window's.
fn log_rows(state: &WindowState, view_id: &ViewId) -> Vec<TimelineRow> {
    let settings = state.settings.settings();
    let timestamp = TimestampSetting {
        show: settings.logging.timestamp,
        format: settings.timestamp.format.clone(),
    };
    let timestamps = TimestampStyle::new(&timestamp, state.local_time);
    match view_id {
        ViewId::Status => build_status_timeline_rows(&state.model.status_log, timestamps),
        ViewId::Room(room_id) => build_room_timeline_rows(&state.model, room_id, timestamps),
        ViewId::Thread { .. } | ViewId::RoomList => {
            unreachable!("only Status and room windows log")
        }
    }
}

/// Starts logging Status or a room window, from the line that arrives as
/// `from` on.
fn start_view_log(state: &mut WindowState, view_id: &ViewId, from: u64) -> Result<(), String> {
    if state.render_state_mode {
        return Err("logging is unavailable in render-state mode".to_owned());
    }
    let path = profile_dir()?
        .join(LOGS_SUBDIR)
        .join(format!("{}.log", log_file_stem(&state.model, view_id)));
    let ident = session_ident(&state.model, view_id);
    let session = LogSession::start(path.clone(), &ident, from, state.clock, state.local_time)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    state
        .views
        .get_mut(view_id)
        .expect("a logged window exists")
        .log = ViewLog::On(Box::new(session));
    Ok(())
}

/// Turns Status's or a room window's log on or off, and says so: the line
/// to print.
pub(super) fn set_view_logging(
    state: &mut WindowState,
    view_id: &ViewId,
    on: bool,
) -> Result<String, String> {
    let logged_path = state.views[view_id]
        .log
        .session()
        .map(|session| session.path().display().to_string());
    let line = match (on, logged_path) {
        (true, Some(path)) => return Ok(format!("* Logging is already on: {path}")),
        (true, None) => {
            // From now on: the next line.
            let from = match view_id {
                ViewId::Status => state.model.status_log.next_sequence(),
                ViewId::Room(room_id) => state
                    .model
                    .room_views
                    .get(room_id)
                    .map_or(0, |room_view| room_view.arrivals.next()),
                ViewId::Thread { .. } | ViewId::RoomList => {
                    unreachable!("only Status and room windows log")
                }
            };
            start_view_log(state, view_id, from)?;
            format!(
                "* Logging on: {}",
                view_log_path(state, view_id).expect("the log just started")
            )
        }
        (false, logged_path) => {
            // An undecided window is decided too: off.
            state
                .views
                .get_mut(view_id)
                .expect("a logged window exists")
                .log = ViewLog::Off;
            match logged_path {
                Some(path) => format!("* Logging off: {path}"),
                None => "* Logging is already off".to_owned(),
            }
        }
    };
    check_logging_items(state, view_id);
    Ok(line)
}

/// `/log`'s answer for a window.
pub(super) fn logging_is_line(state: &WindowState, view_id: &ViewId) -> String {
    match state.views[view_id].log.session() {
        Some(session) => format!("* Logging is on: {}", session.path().display()),
        None => "* Logging is off".to_owned(),
    }
}

/// mIRC's Automatically log: a room window decides once, when it is first
/// shown as a joined channel or query, from the Options of that moment. Its
/// log starts where the room's last window stopped showing lines: the
/// message that reopens a closed query, the lines of the batch that joined
/// the room and those received while mirx was closed are all in it.
pub(super) fn decide_auto_log(state: &mut WindowState, view_id: &ViewId) {
    let ViewId::Room(room_id) = view_id else {
        unreachable!("only room windows log automatically")
    };
    if !matches!(state.views[view_id].log, ViewLog::Undecided) {
        return;
    }
    let room = room_by_id(&state.model.rooms, room_id).expect("a room window's room");
    let logging = &state.settings.settings().logging;
    let on = match room_window_kind(room) {
        RoomWindowKind::Invite => return,
        RoomWindowKind::Channel => logging.channels,
        RoomWindowKind::Query => logging.queries,
    };
    let view = state
        .views
        .get_mut(view_id)
        .expect("the room window exists");
    view.log = ViewLog::Off;
    let from = state
        .model
        .room_views
        .get(room_id)
        .map_or(0, |room_view| room_view.arrivals.unshown_from());
    if on && let Err(error) = start_view_log(state, view_id, from) {
        echo_lines_in_view(state, view_id, [format!("* Logging: {error}")]);
    }
    check_logging_items(state, view_id);
}

/// Appends the window's new lines to its log, if it logs. A failed write
/// stops the log and prints why in the window, without an error: a full
/// disk must not stop every refresh. The line goes in before the caller
/// builds the window's rows, so it shows in this refresh.
pub(super) fn write_view_log(state: &mut WindowState, view_id: &ViewId) {
    if state.views[view_id].log.session().is_none() {
        return;
    }
    let rows = log_rows(state, view_id);
    let strip_codes = state.settings.settings().logging.strip_codes;
    let view = state
        .views
        .get_mut(view_id)
        .expect("the logged window exists");
    let ViewLog::On(session) = &mut view.log else {
        unreachable!("the window logs")
    };
    if let Err(error) = session.write_new_lines(&rows, strip_codes) {
        let line = format!("* Logging stopped: {}: {error}", session.path().display());
        view.log = ViewLog::Off;
        echo_lines_in_view(state, view_id, [line]);
        check_logging_items(state, view_id);
    }
}

/// Checks or clears the Logging item of `view_id`'s window menu and its
/// thread windows'.
fn check_logging_items(state: &WindowState, view_id: &ViewId) {
    let check = if state.views[view_id].log.session().is_some() {
        MF_CHECKED
    } else {
        MF_UNCHECKED
    };
    for (other, view) in &state.views {
        if log_view(other).as_ref() == Ok(view_id) {
            unsafe { check_window_menu_logging(view.child, check) };
        }
    }
}

/// Whether a new window's Logging item starts checked: a thread window's
/// room may already log.
pub(super) fn logging_item_checked(state: &WindowState, view_id: &ViewId) -> bool {
    log_view(view_id)
        .ok()
        .and_then(|log_view| state.views.get(&log_view))
        .is_some_and(|view| view.log.session().is_some())
}

/// The log file a window writes to, for the automation dump: a thread
/// window's is its room's.
pub(super) fn view_log_path(state: &WindowState, view_id: &ViewId) -> Option<String> {
    let log_view = log_view(view_id).ok()?;
    let session = state.views.get(&log_view)?.log.session()?;
    Some(session.path().display().to_string())
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::super::timestamp::utc_local_time;
    use super::*;
    use mirx_core::TextStyle;

    fn summary(room_id: &str) -> RoomSummary {
        room(room_id, "Rust", true)
    }

    fn model_with(room: RoomSummary) -> UiModel {
        UiModel {
            rooms: vec![room],
            ..UiModel::default()
        }
    }

    fn stem(room: RoomSummary) -> String {
        let room_id = room.room_id.clone();
        log_file_stem(&model_with(room), &ViewId::Room(room_id))
    }

    #[test]
    fn a_log_is_named_after_its_query_user_alias_or_room_id() {
        assert_eq!(
            log_file_stem(&UiModel::default(), &ViewId::Status),
            "status"
        );
        let mut room = summary("!abc:matrix.org");
        assert_eq!(stem(room.clone()), "!abc.matrix.org");
        room.canonical_alias = Some("#rust:matrix.org".to_owned());
        assert_eq!(stem(room.clone()), "#rust.matrix.org");
        room.is_direct = true;
        room.direct_user_ids = vec!["@bob:example.org".to_owned()];
        assert_eq!(stem(room.clone()), "@bob.example.org");
        // A DM with several users is named like a room.
        room.direct_user_ids.push("@carol:example.org".to_owned());
        assert_eq!(stem(room.clone()), "#rust.matrix.org");
        room.canonical_alias = None;
        assert_eq!(stem(room), "!abc.matrix.org");
    }

    #[test]
    fn a_log_name_holds_only_what_windows_allows() {
        let mut room = summary("!abc:matrix.org");
        room.canonical_alias = Some("#a<b>c\"d/e\\f|g?h*i\u{7}j:k. .".to_owned());
        assert_eq!(stem(room), "#a_b_c_d_e_f_g_h_i_j.k");
    }

    #[test]
    #[should_panic(expected = "a room's log name is an alias, a room ID or a user ID")]
    fn a_log_name_must_start_like_a_matrix_name() {
        let mut room = summary("!abc:matrix.org");
        room.canonical_alias = Some("CON".to_owned());
        stem(room);
    }

    #[test]
    fn the_logs_folder_label_is_where_the_logs_go() {
        assert_eq!(logs_folder_label(), r"%LOCALAPPDATA%\mirx\ui-shell\logs");
    }

    #[test]
    fn the_session_ident_names_the_window_and_its_log_name() {
        let model = UiModel::default();
        assert_eq!(session_ident(&model, &ViewId::Status), "Status");
        let mut room = summary("!abc:matrix.org");
        room.canonical_alias = Some("#rust:matrix.org".to_owned());
        let view_id = ViewId::Room(room.room_id.clone());
        assert_eq!(
            session_ident(&model_with(room.clone()), &view_id),
            "Rust (#rust:matrix.org)"
        );
        room.is_direct = true;
        room.direct_user_ids = vec!["@bob:example.org".to_owned()];
        room.display_name = "Bob".to_owned();
        assert_eq!(
            session_ident(&model_with(room), &view_id),
            "Bob (@bob:example.org)"
        );
    }

    #[test]
    fn a_thread_window_logs_through_its_room_and_the_rooms_list_not_at_all() {
        assert_eq!(log_view(&ViewId::Status), Ok(ViewId::Status));
        let room = ViewId::Room("!a:x".to_owned());
        assert_eq!(log_view(&room), Ok(room.clone()));
        assert_eq!(
            log_view(&ViewId::Thread {
                room_id: "!a:x".to_owned(),
                thread_root_event_id: "$root".to_owned(),
            }),
            Ok(room)
        );
        assert_eq!(
            log_view(&ViewId::RoomList),
            Err("the Rooms List has no log".to_owned())
        );
    }

    fn temp_log(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mirx-logging-{name}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir.join(LOGS_SUBDIR).join("#rust.matrix.org.log")
    }

    #[test]
    fn a_session_is_framed_and_sessions_are_parted_by_a_blank_line() {
        let path = temp_log("frame");
        let clock = UiClock::Fixed(1_700_000_000_000);
        let rows = [
            TimelineRow::log(TimelineRowKind::Info, "* héllo wörld".to_owned())
                .with_identity(TimelineRowIdentity::status_line(1))
                .arrived(Some(1)),
        ];
        let mut session =
            LogSession::start(path.clone(), "Rust (#rust:x)", 0, clock, utc_local_time).unwrap();
        session.write_new_lines(&rows, false).unwrap();
        drop(session);
        let session =
            LogSession::start(path.clone(), "Rust (#rust:x)", 2, clock, utc_local_time).unwrap();
        drop(session);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Session Start: Tue Nov 14 22:13:20 2023\r\n\
             Session Ident: Rust (#rust:x)\r\n\
             * héllo wörld\r\n\
             Session Close: Tue Nov 14 22:13:20 2023\r\n\
             \r\n\
             Session Start: Tue Nov 14 22:13:20 2023\r\n\
             Session Ident: Rust (#rust:x)\r\n\
             Session Close: Tue Nov 14 22:13:20 2023\r\n"
        );
        fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
    }

    const FROM: u64 = 10;

    fn line(arrival: u64, text: &str) -> TimelineRow {
        TimelineRow::log(TimelineRowKind::Info, text.to_owned())
            .with_identity(TimelineRowIdentity::EchoLine { sequence: arrival })
            .arrived(Some(arrival))
    }

    #[test]
    fn a_line_is_written_once_and_only_from_the_session_on() {
        let mut diff = LogDiff::new(FROM);
        let mut rows = vec![
            line(FROM - 1, "before"),
            TimelineRow::full_width(TimelineRowKind::Plain, "context".to_owned()),
            line(FROM, "at the start"),
        ];
        assert_eq!(diff.lines(&rows, false), ["at the start"]);
        assert!(diff.lines(&rows, false).is_empty());
        rows.push(line(FROM + 5, "two\r\nlines"));
        // The same line again, restamped: still logged once.
        rows[2].logical_text = "[10:00] at the start".to_owned();
        assert_eq!(diff.lines(&rows, false), ["two", "lines"]);
    }

    #[test]
    fn history_did_not_arrive_and_is_never_written() {
        let mut diff = LogDiff::new(0);
        let history = message_row(
            "event:$old",
            TimelineMessageSendState::Sent,
            "<bob> old",
            Vec::new(),
        )
        .arrived(None);
        assert!(diff.lines(&[history], false).is_empty());
    }

    fn message_row(
        activity_id: &str,
        send_state: TimelineMessageSendState,
        text: &str,
        styles: Vec<TextStyleSpan>,
    ) -> TimelineRow {
        TimelineRow::log(
            TimelineRowKind::Message {
                presentation: RoomMessagePresentationKind::Text,
                send_state,
                highlight: false,
                own: true,
            },
            text.to_owned(),
        )
        .with_styles(styles)
        .with_identity(TimelineRowIdentity::activity_primary(activity_id))
        .arrived(Some(FROM + 1))
    }

    #[test]
    fn a_message_is_logged_once_it_is_sent() {
        let mut diff = LogDiff::new(FROM);
        let pending = message_row(
            "local:1",
            TimelineMessageSendState::Pending,
            "<me> hi",
            Vec::new(),
        );
        let failed = message_row(
            "local:2",
            TimelineMessageSendState::Failed,
            "<me> lost",
            Vec::new(),
        );
        assert!(diff.lines(&[pending, failed], false).is_empty());
        let sent = message_row(
            "event:$1",
            TimelineMessageSendState::Sent,
            "<me> hi",
            Vec::new(),
        );
        assert_eq!(diff.lines(std::slice::from_ref(&sent), false), ["<me> hi"]);
        assert!(diff.lines(&[sent], false).is_empty());
    }

    #[test]
    fn formatting_is_written_as_codes_unless_stripped() {
        let bold = vec![TextStyleSpan {
            start: 5,
            end: 9,
            style: TextStyle {
                bold: true,
                ..TextStyle::default()
            },
        }];
        let row = message_row(
            "event:$1",
            TimelineMessageSendState::Sent,
            "<me> bold",
            bold,
        );
        assert_eq!(
            LogDiff::new(FROM).lines(std::slice::from_ref(&row), false),
            ["<me> \u{2}bold"]
        );
        assert_eq!(LogDiff::new(FROM).lines(&[row], true), ["<me> bold"]);
    }
}
