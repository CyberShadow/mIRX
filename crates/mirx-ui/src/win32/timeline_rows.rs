//! Building a view's timeline rows from the model.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum TimelineRowIdentity {
    ActivityPrimary {
        activity_id: String,
    },
    ActivityReactionSummary {
        activity_id: String,
    },
    /// The quote line above a reply.
    ActivityReplyQuote {
        activity_id: String,
    },
    Synthetic {
        label: TimelineSyntheticRowLabel,
        logical_text: String,
    },
    /// A Status log line; identical lines stay distinct.
    StatusLine {
        sequence: u64,
    },
    /// A command's line in a room or thread window.
    EchoLine {
        sequence: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum TimelineSyntheticRowLabel {
    NowTalking,
    Topic,
    Encryption,
    Invite,
    Typing,
    Backfill,
    /// What the editbox replies to or edits.
    ComposeTarget,
    Context,
    Plain,
}

impl TimelineRowIdentity {
    pub(super) fn activity_primary(activity_id: &str) -> Self {
        Self::ActivityPrimary {
            activity_id: activity_id.to_owned(),
        }
    }

    fn activity_reaction_summary(activity_id: &str) -> Self {
        Self::ActivityReactionSummary {
            activity_id: activity_id.to_owned(),
        }
    }

    fn activity_reply_quote(activity_id: &str) -> Self {
        Self::ActivityReplyQuote {
            activity_id: activity_id.to_owned(),
        }
    }

    pub(super) fn status_line(sequence: u64) -> Self {
        Self::StatusLine { sequence }
    }

    pub(super) fn synthetic(label: TimelineSyntheticRowLabel, logical_text: &str) -> Self {
        Self::Synthetic {
            label,
            logical_text: logical_text.to_owned(),
        }
    }

    fn default_for_row(kind: &TimelineRowKind, logical_text: &str) -> Self {
        let label = match kind {
            TimelineRowKind::Typing => TimelineSyntheticRowLabel::Typing,
            TimelineRowKind::Plain => TimelineSyntheticRowLabel::Plain,
            _ => TimelineSyntheticRowLabel::Context,
        };
        Self::synthetic(label, logical_text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TimelineRow {
    pub(super) identity: TimelineRowIdentity,
    pub(super) event_id: Option<String>,
    pub(super) kind: TimelineRowKind,
    /// The row's text, painted exactly as find and selection see it.
    pub(super) logical_text: String,
    /// How `logical_text` is formatted: a message's own styles, at their
    /// offsets in the row's text. Every other row is plain.
    pub(super) styles: Vec<TextStyleSpan>,
    pub(super) layout: TimelineRowLayout,
    pub(super) image_state: Option<TimelineImageState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TimelineRowLayout {
    /// Context rows (typing, history backfill): one painted line per text line.
    FullWidth,
    /// mIRC log lines: wrapped to the view, continuation lines indented past
    /// the timestamp.
    Log,
}

impl TimelineRow {
    pub(super) fn full_width(kind: TimelineRowKind, text: String) -> Self {
        Self {
            identity: TimelineRowIdentity::default_for_row(&kind, &text),
            event_id: None,
            kind,
            logical_text: text,
            styles: Vec::new(),
            layout: TimelineRowLayout::FullWidth,
            image_state: None,
        }
    }

    pub(super) fn log(kind: TimelineRowKind, text: String) -> Self {
        Self {
            identity: TimelineRowIdentity::default_for_row(&kind, &text),
            event_id: None,
            kind,
            logical_text: text,
            styles: Vec::new(),
            layout: TimelineRowLayout::Log,
            image_state: None,
        }
    }

    pub(super) fn with_identity(mut self, identity: TimelineRowIdentity) -> Self {
        self.identity = identity;
        self
    }

    pub(super) fn with_event_id(mut self, event_id: Option<&str>) -> Self {
        self.event_id = event_id.map(str::to_owned);
        self
    }

    pub(super) fn with_styles(mut self, styles: Vec<TextStyleSpan>) -> Self {
        assert_eq!(
            mirx_core::check_text_styles(&self.logical_text, &styles),
            Ok(()),
            "a row's styles format its text"
        );
        self.styles = styles;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Rows for lines other users wrote carry `highlight`: the user's push
/// rules highlighted the line, so it is drawn in the highlight colour.
pub(super) enum TimelineRowKind {
    /// mIRC's green event lines, such as `* Now talking in` and `* Topic is`.
    Event,
    /// mIRC's navy info lines.
    Info,
    Error,
    Typing,
    ReactionSummary,
    /// What a reply answers, above it.
    ReplyQuote,
    Message {
        presentation: RoomMessagePresentationKind,
        send_state: TimelineMessageSendState,
        highlight: bool,
    },
    Image {
        highlight: bool,
    },
    File {
        highlight: bool,
    },
    Audio {
        highlight: bool,
    },
    Video {
        highlight: bool,
    },
    Unsupported {
        highlight: bool,
    },
    UnableToDecrypt,
    StateNotice {
        highlight: bool,
    },
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TimelineMessageSendState {
    Pending,
    Sent,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TimelineImageState {
    Pending,
    Loaded {
        key: String,
        width: u32,
        height: u32,
    },
    Failed {
        message: String,
    },
}

fn timeline_message_send_state(send_state: &RoomMessageSendState) -> TimelineMessageSendState {
    match send_state {
        RoomMessageSendState::Pending => TimelineMessageSendState::Pending,
        RoomMessageSendState::Sent => TimelineMessageSendState::Sent,
        RoomMessageSendState::Failed { .. } => TimelineMessageSendState::Failed,
    }
}

fn room_context_row(
    kind: TimelineRowKind,
    label: TimelineSyntheticRowLabel,
    body: &str,
) -> TimelineRow {
    let text = context_line(body);
    TimelineRow::log(kind, text.clone()).with_identity(TimelineRowIdentity::synthetic(label, &text))
}

fn build_invite_timeline_rows(room: &RoomSummary) -> Vec<TimelineRow> {
    vec![
        room_context_row(
            TimelineRowKind::Info,
            TimelineSyntheticRowLabel::Invite,
            &invited_text(&room.display_name),
        ),
        room_context_row(
            TimelineRowKind::Info,
            TimelineSyntheticRowLabel::Invite,
            INVITE_RESPONSE_TEXT,
        ),
    ]
}

/// The lines mIRC prints when a channel window opens.
fn push_room_context_rows(
    rows: &mut Vec<TimelineRow>,
    room: &RoomSummary,
    room_view: Option<&RoomViewModel>,
) {
    rows.push(room_context_row(
        TimelineRowKind::Event,
        TimelineSyntheticRowLabel::NowTalking,
        &now_talking_text(&room.display_name, &room.room_id),
    ));
    if let Some(topic) = room_view.and_then(|view| view.topic.as_deref()) {
        rows.push(room_context_row(
            TimelineRowKind::Event,
            TimelineSyntheticRowLabel::Topic,
            &topic_is_text(topic),
        ));
    }
    if room.encrypted {
        rows.push(room_context_row(
            TimelineRowKind::Info,
            TimelineSyntheticRowLabel::Encryption,
            ENCRYPTED_ROOM_TEXT,
        ));
    }
}

/// Who is typing, below the newest line.
fn push_room_typing_row(rows: &mut Vec<TimelineRow>, room_view: &RoomViewModel) {
    if room_view.typing_user_ids.is_empty() {
        return;
    }
    let users = room_view
        .typing_user_ids
        .iter()
        .map(|user_id| room_view.member_nick(user_id))
        .collect::<Vec<_>>()
        .join(", ");
    let verb = if room_view.typing_user_ids.len() == 1 {
        "is"
    } else {
        "are"
    };
    let logical_text = context_line(&format!("{users} {verb} typing"));
    rows.push(
        TimelineRow::full_width(TimelineRowKind::Typing, logical_text.clone()).with_identity(
            TimelineRowIdentity::synthetic(TimelineSyntheticRowLabel::Typing, &logical_text),
        ),
    );
}

fn push_room_history_backfill_row(rows: &mut Vec<TimelineRow>, history: &RoomHistoryBackfillState) {
    let text = match &history.status {
        RoomHistoryBackfillStatus::Idle => return,
        RoomHistoryBackfillStatus::Loading { .. } => "* Loading older history...".to_owned(),
        RoomHistoryBackfillStatus::Failed { message, .. } => {
            format!("* Unable to load older history: {message}. Scroll to top to retry.")
        }
        RoomHistoryBackfillStatus::Exhausted => "* No older history available.".to_owned(),
    };
    rows.push(
        TimelineRow::full_width(TimelineRowKind::Plain, text.clone()).with_identity(
            TimelineRowIdentity::synthetic(TimelineSyntheticRowLabel::Backfill, &text),
        ),
    );
}

fn push_room_activity_row(
    rows: &mut Vec<TimelineRow>,
    room_view: &RoomViewModel,
    activity: &RoomActivity,
    timestamps: TimestampStyle,
) {
    push_reply_quote_row(rows, room_view, activity);
    let ActivityLine {
        kind,
        text,
        styles,
        image_state,
    } = activity_line_text(
        room_view,
        activity,
        &timestamps.prefix(activity.timestamp_ms),
    );
    let mut row = TimelineRow::log(kind, text)
        .with_styles(styles)
        .with_identity(TimelineRowIdentity::activity_primary(&activity.activity_id))
        .with_event_id(room_activity_event_id(activity));
    row.image_state = image_state;
    rows.push(row);

    let Some(event_id) = room_activity_event_id(activity) else {
        return;
    };
    if matches!(activity.kind, RoomActivityKind::Redacted(_)) {
        return;
    }
    let summaries = room_view.reaction_summaries_for_event(event_id);
    if summaries.is_empty() {
        return;
    }
    // A log row: naming reactors makes the line long, and it wraps.
    rows.push(
        TimelineRow::log(
            TimelineRowKind::ReactionSummary,
            reactions::summary_line(&summaries, |user_id| room_view.member_nick(user_id)),
        )
        .with_identity(TimelineRowIdentity::activity_reaction_summary(
            &activity.activity_id,
        )),
    );
}

/// The line above a reply quoting what it answers. It carries the reply's
/// event, so clicking it selects the reply.
fn push_reply_quote_row(
    rows: &mut Vec<TimelineRow>,
    room_view: &RoomViewModel,
    activity: &RoomActivity,
) {
    let Some(target_event_id) = activity
        .relation
        .as_ref()
        .and_then(replies::reply_quote_target)
    else {
        return;
    };
    let quoted = match room_view.reply_target(target_event_id) {
        ReplyTarget::Known(RoomActivity {
            kind: RoomActivityKind::Redacted(_),
            ..
        }) => replies::QUOTE_REDACTED_TEXT.to_owned(),
        ReplyTarget::Known(target) => quoted_line(room_view, target),
        ReplyTarget::Loading => replies::QUOTE_LOADING_TEXT.to_owned(),
        ReplyTarget::Unavailable => replies::QUOTE_UNAVAILABLE_TEXT.to_owned(),
    };
    rows.push(
        TimelineRow::full_width(TimelineRowKind::ReplyQuote, replies::quote_line(&quoted))
            .with_identity(TimelineRowIdentity::activity_reply_quote(
                &activity.activity_id,
            ))
            .with_event_id(room_activity_event_id(activity)),
    );
}

/// A message as a quote or the compose indicator shows it.
fn quoted_line(room_view: &RoomViewModel, activity: &RoomActivity) -> String {
    replies::quote_excerpt(&activity_line_text(room_view, activity, "").text)
}

/// The bottom line of a window whose editbox is armed with `target`. It
/// wraps like a log line: the excerpt alone nearly fills a default window.
pub(super) fn compose_target_row(
    model: &UiModel,
    view_id: &ViewId,
    target: &ComposeTarget,
) -> TimelineRow {
    let room_id = view_id
        .room_id()
        .expect("only room and thread windows compose replies and edits");
    let activity = window_message(model, view_id, target.event_id())
        .expect("refreshing a window disarms a target it no longer shows");
    room_context_row(
        TimelineRowKind::Info,
        TimelineSyntheticRowLabel::ComposeTarget,
        &replies::compose_target_indicator(
            target,
            &quoted_line(&model.room_views[room_id], activity),
        ),
    )
}

/// An activity's line: its row kind, text after the timestamp prefix, the
/// text's styles and the image preview state.
struct ActivityLine {
    kind: TimelineRowKind,
    text: String,
    styles: Vec<TextStyleSpan>,
    image_state: Option<TimelineImageState>,
}

impl ActivityLine {
    fn plain(kind: TimelineRowKind, text: String, image_state: Option<TimelineImageState>) -> Self {
        Self {
            kind,
            text,
            styles: Vec::new(),
            image_state,
        }
    }
}

fn activity_line_text(
    room_view: &RoomViewModel,
    activity: &RoomActivity,
    timestamp: &str,
) -> ActivityLine {
    let (kind, text, image_state) = match &activity.kind {
        RoomActivityKind::Message(message) => {
            let suffix = match &message.send_state {
                RoomMessageSendState::Pending => " (sending)",
                RoomMessageSendState::Sent
                    if message
                        .event_id
                        .as_ref()
                        .is_some_and(|event_id| room_view.edited_event_ids.contains(event_id)) =>
                {
                    " (edited)"
                }
                RoomMessageSendState::Sent => "",
                RoomMessageSendState::Failed { .. } => " (failed)",
            };
            let (text, body_offset) = message_line(
                timestamp,
                message.presentation,
                &message.sender,
                &format!("{}{suffix}", message.body),
            );
            // The suffix is mirx's note, not the sender's text: unstyled.
            let styles = message
                .styles
                .iter()
                .map(|span| TextStyleSpan {
                    start: body_offset + span.start,
                    end: body_offset + span.end,
                    style: span.style,
                })
                .collect();
            return ActivityLine {
                kind: TimelineRowKind::Message {
                    presentation: message.presentation,
                    send_state: timeline_message_send_state(&message.send_state),
                    highlight: activity.highlight,
                },
                text,
                styles,
                image_state: None,
            };
        }
        RoomActivityKind::Image(message) => {
            let display_name = message.filename.as_deref().unwrap_or(message.body.as_str());
            let metadata_suffix = format_room_image_metadata_suffix(message.info.as_ref());
            let format = timeline_thumbnail_format();
            let image_state = Some(room_view.projected_preview_state(
                &activity.activity_id,
                &message.event_id,
                &format,
            ));
            (
                TimelineRowKind::Image {
                    highlight: activity.highlight,
                },
                action_line(
                    timestamp,
                    &message.sender,
                    &format!("sent image: {display_name}{metadata_suffix}"),
                ),
                image_state,
            )
        }
        RoomActivityKind::File(message) => {
            let display_name = message.filename.as_deref().unwrap_or(message.body.as_str());
            let metadata_suffix = format_room_file_metadata_suffix(message.info.as_ref());
            (
                TimelineRowKind::File {
                    highlight: activity.highlight,
                },
                action_line(
                    timestamp,
                    &message.sender,
                    &format!("sent file: {display_name}{metadata_suffix}"),
                ),
                None,
            )
        }
        RoomActivityKind::Audio(message) => {
            let display_name = message.filename.as_deref().unwrap_or(message.body.as_str());
            let metadata_suffix = format_room_audio_metadata_suffix(message.info.as_ref());
            (
                TimelineRowKind::Audio {
                    highlight: activity.highlight,
                },
                action_line(
                    timestamp,
                    &message.sender,
                    &format!("sent audio: {display_name}{metadata_suffix}"),
                ),
                None,
            )
        }
        RoomActivityKind::Video(message) => {
            let display_name = message.filename.as_deref().unwrap_or(message.body.as_str());
            let metadata_suffix = format_room_video_metadata_suffix(message.info.as_ref());
            let format = timeline_thumbnail_format();
            let image_state = timeline_video_preview_source(message).map(|_| {
                room_view.projected_preview_state(&activity.activity_id, &message.event_id, &format)
            });
            (
                TimelineRowKind::Video {
                    highlight: activity.highlight,
                },
                action_line(
                    timestamp,
                    &message.sender,
                    &format!("sent video: {display_name}{metadata_suffix}"),
                ),
                image_state,
            )
        }
        RoomActivityKind::UnsupportedMessage(message) => (
            TimelineRowKind::Unsupported {
                highlight: activity.highlight,
            },
            action_line(
                timestamp,
                &message.sender,
                &format!("sent unsupported {}: {}", message.msgtype, message.body),
            ),
            None,
        ),
        RoomActivityKind::UnableToDecrypt(message) => (
            TimelineRowKind::UnableToDecrypt,
            action_line(
                timestamp,
                &message.sender,
                "sent an encrypted message that could not be decrypted",
            ),
            None,
        ),
        RoomActivityKind::StateNotice(notice) => {
            let body = match &notice.content {
                RoomStateNoticeContent::Topic { topic } => topic_change_text(&notice.sender, topic),
                RoomStateNoticeContent::Member {
                    change,
                    target,
                    reason,
                } => member_change_text(&notice.sender, change, target, reason.as_deref()),
                RoomStateNoticeContent::PowerLevels {
                    changes,
                    users_default,
                    other_permissions_changed,
                } => power_levels_change_text(
                    &notice.sender,
                    changes,
                    users_default.as_ref(),
                    *other_permissions_changed,
                ),
            };
            (
                TimelineRowKind::StateNotice {
                    highlight: activity.highlight,
                },
                event_line(timestamp, &body),
                None,
            )
        }
        // The redacted line no longer says what made it a highlight.
        RoomActivityKind::Redacted(_) => (
            TimelineRowKind::StateNotice { highlight: false },
            event_line(timestamp, "message redacted"),
            None,
        ),
    };
    ActivityLine::plain(kind, text, image_state)
}

pub(super) fn build_room_timeline_rows(
    model: &UiModel,
    room_id: &str,
    timestamps: TimestampStyle,
) -> Vec<TimelineRow> {
    let room = room_by_id(&model.rooms, room_id).expect("room view must target an existing room");
    let room_view = model.room_views.get(room_id);
    if is_invited_room(room) {
        let mut rows = build_invite_timeline_rows(room);
        if let Some(room_view) = room_view {
            rows.extend(
                room_view
                    .echo
                    .lines_for(None)
                    .map(|line| echo_row(line, timestamps)),
            );
        }
        return rows;
    }
    let mut rows = Vec::new();
    push_room_context_rows(&mut rows, room, room_view);

    if let Some(room_view) = room_view {
        push_room_history_backfill_row(&mut rows, &room_view.history);
        push_activity_rows_with_echo(
            &mut rows,
            room_view,
            room_view.activities.iter(),
            None,
            timestamps,
        );
        push_room_typing_row(&mut rows, room_view);
    }

    rows
}

/// The window's activity rows with its echo lines merged by time: an echo
/// line goes before the first activity newer than it, so it stays below
/// what the user saw when typing the command.
fn push_activity_rows_with_echo<'a>(
    rows: &mut Vec<TimelineRow>,
    room_view: &RoomViewModel,
    activities: impl Iterator<Item = &'a RoomActivity>,
    thread_root: Option<&str>,
    timestamps: TimestampStyle,
) {
    let mut echo = room_view.echo.lines_for(thread_root).peekable();
    for activity in activities {
        while let Some(line) = echo.next_if(|line| line.timestamp_ms < activity.timestamp_ms) {
            rows.push(echo_row(line, timestamps));
        }
        push_room_activity_row(rows, room_view, activity, timestamps);
    }
    rows.extend(echo.map(|line| echo_row(line, timestamps)));
}

fn echo_row(line: &EchoLine, timestamps: TimestampStyle) -> TimelineRow {
    let prefix = timestamps.prefix(line.timestamp_ms);
    TimelineRow::log(TimelineRowKind::Info, format!("{prefix}{}", line.text)).with_identity(
        TimelineRowIdentity::EchoLine {
            sequence: line.sequence,
        },
    )
}

pub(super) fn build_thread_timeline_rows(
    model: &UiModel,
    room_id: &str,
    thread_root_event_id: &str,
    timestamps: TimestampStyle,
) -> Vec<TimelineRow> {
    let room = room_by_id(&model.rooms, room_id).expect("thread view must target an existing room");
    let room_view = model
        .room_views
        .get(room_id)
        .expect("thread view must target an existing room view");
    let mut rows = Vec::new();
    push_room_context_rows(&mut rows, room, Some(room_view));
    push_activity_rows_with_echo(
        &mut rows,
        room_view,
        room_view
            .activities
            .iter()
            .filter(|activity| room_activity_belongs_to_thread(activity, thread_root_event_id)),
        Some(thread_root_event_id),
        timestamps,
    );
    push_room_typing_row(&mut rows, room_view);
    rows
}

pub(super) fn project_timeline_rows(rows: &[TimelineRow]) -> Vec<String> {
    rows.iter().map(|row| row.logical_text.clone()).collect()
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn render_room_lines(state: &WindowState, room_id: &str) -> Vec<String> {
    project_timeline_rows(&build_room_timeline_rows(
        &state.model,
        room_id,
        state.timestamps(),
    ))
}

fn format_room_image_metadata_suffix(info: Option<&RoomImageInfo>) -> String {
    let Some(info) = info else {
        return String::new();
    };

    let mut parts = Vec::new();
    if let Some(mimetype) = info.mimetype.as_deref() {
        parts.push(mimetype.to_owned());
    }
    if let (Some(width), Some(height)) = (info.width, info.height) {
        parts.push(format!("{width}x{height}"));
    }
    if let Some(size) = info.size {
        parts.push(format!("{size} bytes"));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

fn format_room_file_metadata_suffix(info: Option<&RoomFileInfo>) -> String {
    let Some(info) = info else {
        return String::new();
    };

    let mut parts = Vec::new();
    if let Some(mimetype) = info.mimetype.as_deref() {
        parts.push(mimetype.to_owned());
    }
    if let Some(size) = info.size {
        parts.push(format!("{size} bytes"));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

fn format_room_audio_metadata_suffix(info: Option<&RoomAudioInfo>) -> String {
    let Some(info) = info else {
        return String::new();
    };

    let mut parts = Vec::new();
    if let Some(mimetype) = info.mimetype.as_deref() {
        parts.push(mimetype.to_owned());
    }
    if let Some(size) = info.size {
        parts.push(format!("{size} bytes"));
    }
    if let Some(duration_ms) = info.duration_ms {
        parts.push(format!("{duration_ms} ms"));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

fn format_room_video_metadata_suffix(info: Option<&RoomVideoInfo>) -> String {
    let Some(info) = info else {
        return String::new();
    };

    let mut parts = Vec::new();
    if let Some(mimetype) = info.mimetype.as_deref() {
        parts.push(mimetype.to_owned());
    }
    if let (Some(width), Some(height)) = (info.width, info.height) {
        parts.push(format!("{width}x{height}"));
    }
    if let Some(size) = info.size {
        parts.push(format!("{size} bytes"));
    }
    if let Some(duration_ms) = info.duration_ms {
        parts.push(format!("{duration_ms} ms"));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

pub(super) fn build_status_timeline_rows(
    status_log: &StatusLog,
    timestamps: TimestampStyle,
) -> Vec<TimelineRow> {
    status_log
        .lines()
        .iter()
        .map(|line| {
            let kind = match line.kind {
                StatusLineKind::Info => TimelineRowKind::Info,
                StatusLineKind::Error => TimelineRowKind::Error,
            };
            let prefix = timestamps.prefix(line.timestamp_ms);
            TimelineRow::log(kind, format!("{prefix}{}", line.text))
                .with_identity(TimelineRowIdentity::status_line(line.sequence))
        })
        .collect()
}

/// The replied-to messages `activities` quote that the room view neither
/// has nor has asked for.
pub(super) fn collect_reply_target_requests<'a>(
    room_view: &RoomViewModel,
    activities: impl IntoIterator<Item = &'a RoomActivity>,
) -> BTreeSet<String> {
    activities
        .into_iter()
        .filter_map(|activity| activity.relation.as_ref())
        .filter_map(replies::reply_quote_target)
        .filter(|event_id| {
            room_view
                .find_activity_index_by_event_id(event_id)
                .is_none()
                && !room_view.reply_targets.contains_key(*event_id)
        })
        .map(str::to_owned)
        .collect()
}

/// Asks the server for replied-to messages. Only a connected session can:
/// events a synthetic load replays come from no server, and without a
/// session there is nobody to ask.
pub(super) fn dispatch_reply_target_fetches(
    state: &mut WindowState,
    room_id: &str,
    event_ids: BTreeSet<String>,
) -> Result<(), String> {
    if state.synthetic_load_in_progress
        || !matches!(
            state.model.connection,
            ConnectionState::SignedIn | ConnectionState::Syncing
        )
    {
        return Ok(());
    }
    for event_id in event_ids {
        enqueue_command(
            state,
            Command::FetchRoomEvent(FetchRoomEventRequest {
                room_id: room_id.to_owned(),
                event_id: event_id.clone(),
            }),
        )?;
        state
            .model
            .room_views
            .get_mut(room_id)
            .expect("reply target fetches require an existing room view")
            .reply_targets
            .insert(event_id, ReplyTargetState::Pending);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use mirx_core::{RoomMemberChange, RoomStateNoticeActivity};

    use super::super::test_fixtures::*;
    use super::*;

    fn incoming_reaction_removed(
        room_id: &str,
        target_event_id: &str,
        reaction_event_id: &str,
        redaction_event_id: &str,
        key: &str,
    ) -> Event {
        Event::RoomActivityUpdate(RoomActivityUpdate {
            room_id: room_id.to_owned(),
            target: RoomActivityUpdateTarget::Event {
                event_id: target_event_id.to_owned(),
            },
            kind: RoomActivityUpdateKind::ReactionRemoved {
                reaction_event_id: reaction_event_id.to_owned(),
                redaction_event_id: redaction_event_id.to_owned(),
                key: key.to_owned(),
            },
        })
    }

    #[test]
    fn synced_room_summary_topic_seeds_updates_and_clears_room_context() {
        let mut state = test_state();
        let mut summary = room("!ops:example", "Ops", false);
        summary.topic = Some("incident review".to_owned());

        apply_core_event(&mut state, Event::SyncedRooms(vec![summary.clone()])).unwrap();
        let initial = project_timeline_rows(&build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
        ));
        assert!(initial.contains(&"* Now talking in Ops (!ops:example)".to_owned()));
        assert!(initial.contains(&"* Topic is 'incident review'".to_owned()));
        assert!(
            !initial
                .iter()
                .any(|line| line.contains("changes topic to 'incident review'"))
        );

        apply_core_event(&mut state, topic_state("!ops:example", "deployment window")).unwrap();
        let updated = project_timeline_rows(&build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
        ));
        assert!(updated.contains(&"* Topic is 'deployment window'".to_owned()));
        assert!(
            updated
                .iter()
                .any(|line| line.contains("changes topic to 'deployment window'"))
        );
        assert!(!updated.contains(&"* Topic is 'incident review'".to_owned()));

        summary.topic = None;
        apply_core_event(&mut state, Event::SyncedRooms(vec![summary])).unwrap();
        let cleared = project_timeline_rows(&build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
        ));
        assert!(!cleared.iter().any(|line| line.starts_with("* Topic is ''")));
    }

    fn member_state(
        room_id: &str,
        state_key: &str,
        change: RoomMemberChange,
        display_name: Option<&str>,
    ) -> Event {
        let event_id = format!("$member:{room_id}:{state_key}:{change:?}");
        Event::RoomActivity(Box::new(RoomActivity {
            room_id: room_id.to_owned(),
            timestamp_ms: 1_700_000_120_000,
            highlight: false,
            activity_id: format!("state:{event_id}"),
            relation: None,
            kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
                event_id,
                sender: user_profile("@alice:example"),
                content: RoomStateNoticeContent::Member {
                    change,
                    target: RoomUserProfile {
                        user_id: state_key.to_owned(),
                        display_name: display_name.map(str::to_owned),
                        display_name_ambiguous: false,
                    },
                    reason: None,
                },
            }),
        }))
    }

    fn room_lines(state: &WindowState) -> Vec<String> {
        project_timeline_rows(&build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
        ))
    }

    #[test]
    fn a_reply_quotes_its_loaded_target_above_it_with_the_replys_event() {
        let state = reply_quote_state("$q");
        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        assert_eq!(
            project_timeline_rows(&rows)[2..],
            [
                "    > <@alice:example> are we deploying today?".to_owned(),
                "[22:14] <@bob:example> yes, after lunch".to_owned(),
            ]
        );
        assert_eq!(rows[2].kind, TimelineRowKind::ReplyQuote);
        assert_eq!(rows[2].event_id.as_deref(), Some("$a"));
        assert_eq!(
            rows[2].identity,
            TimelineRowIdentity::activity_reply_quote("event:$a")
        );
    }

    #[test]
    fn a_reply_quote_follows_its_targets_edits_and_redaction() {
        let mut state = reply_quote_state("$q");
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$q",
                "$q-edit",
                RoomMessagePresentationKind::Emote,
                "asks whether we deploy today, because the build looks green to me",
            ),
        )
        .unwrap();
        assert_eq!(
            room_lines(&state)[2],
            "    > * @alice:example asks whether we deploy today, because the b..."
        );
        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$q", "$q-gone"),
        )
        .unwrap();
        assert_eq!(room_lines(&state)[2], "    > message redacted");
    }

    #[test]
    fn a_reply_to_a_message_not_loaded_says_it_is_loading() {
        let state = reply_quote_state("$elsewhere");
        assert_eq!(
            room_lines(&state)[2],
            "    > (loading the replied-to message...)"
        );
    }

    #[test]
    fn an_edited_message_is_marked_until_it_is_redacted() {
        let mut state = reply_quote_state("$q");
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$a",
                "$a-edit",
                RoomMessagePresentationKind::Emote,
                "says yes",
            ),
        )
        .unwrap();
        assert_eq!(
            room_lines(&state)[3],
            "[22:14] * @bob:example says yes (edited)"
        );
        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$a", "$a-gone"),
        )
        .unwrap();
        assert_eq!(room_lines(&state)[3], "[22:14] * message redacted");
        assert!(
            state.model.room_views["!ops:example"]
                .edited_event_ids
                .is_empty()
        );
    }

    fn fetch_reply_targets(state: &mut WindowState) {
        let view = &state.model.room_views["!ops:example"];
        let requests = collect_reply_target_requests(view, view.activities.iter());
        dispatch_reply_target_fetches(state, "!ops:example", requests).unwrap();
    }

    fn reply_target_fetched(result: RoomEventFetchResult) -> Event {
        Event::RoomEvent(RoomEventFetch {
            room_id: "!ops:example".to_owned(),
            event_id: "$elsewhere".to_owned(),
            result,
        })
    }

    #[test]
    fn a_reply_target_not_loaded_is_fetched_once_while_connected() {
        let mut state = reply_quote_state("$elsewhere");
        let (sender, _commands) = inert_command_sender();
        state.command_sender = Some(sender);

        fetch_reply_targets(&mut state);
        assert!(state.recorded_commands.is_empty(), "no session to ask");

        state.model.connection = ConnectionState::Syncing;
        fetch_reply_targets(&mut state);
        fetch_reply_targets(&mut state);
        assert_eq!(
            state.recorded_commands,
            vec![Command::FetchRoomEvent(FetchRoomEventRequest {
                room_id: "!ops:example".to_owned(),
                event_id: "$elsewhere".to_owned(),
            })]
        );
        assert_eq!(
            room_lines(&state)[2],
            "    > (loading the replied-to message...)"
        );

        apply_core_event(
            &mut state,
            reply_target_fetched(RoomEventFetchResult::Loaded {
                activity: Box::new(sent_message_activity(
                    "!ops:example",
                    "$elsewhere",
                    None,
                    "from long ago",
                    1_600_000_000_000,
                )),
            }),
        )
        .unwrap();
        assert_eq!(
            room_lines(&state)[2],
            "    > <@alice:example> from long ago"
        );
    }

    #[test]
    fn a_reply_target_the_server_cannot_give_is_unavailable() {
        let mut state = reply_quote_state("$elsewhere");
        apply_core_event(
            &mut state,
            reply_target_fetched(RoomEventFetchResult::Failed {
                message: "unsupported event".to_owned(),
            }),
        )
        .unwrap();
        assert_eq!(
            room_lines(&state)[2],
            "    > (the replied-to message is unavailable)"
        );
    }

    #[test]
    fn a_fetched_reply_target_for_a_room_without_a_view_is_dropped() {
        let mut state = test_state();
        apply_core_event(
            &mut state,
            reply_target_fetched(RoomEventFetchResult::Failed {
                message: "joined room not found: !ops:example".to_owned(),
            }),
        )
        .unwrap();
        assert!(state.model.room_views.is_empty());
    }

    #[test]
    fn a_thread_reply_to_its_root_has_no_quote() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, timeline("!ops:example", "root")).unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$t1", "$!ops:example:root", "in thread"),
        )
        .unwrap();
        let rows = build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$!ops:example:root",
            state.timestamps(),
        );
        assert!(
            rows.iter()
                .all(|row| row.kind != TimelineRowKind::ReplyQuote),
            "{:?}",
            project_timeline_rows(&rows)
        );
    }

    #[test]
    fn reaction_projection_renders_single_summary_row_after_annotated_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event:example.org",
                None,
                "hello",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$event:example.org",
                "$reaction-1:example.org",
                "@bob:example.org",
                "+1",
                1_700_000_000_500,
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event,
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                TimelineRowKind::ReactionSummary,
            ]
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] <@alice:example> hello".to_owned(),
                "    reactions: +1 @bob:example.org".to_owned(),
            ]
        );
    }

    #[test]
    fn reaction_projection_aggregates_keys_in_reaction_time_into_one_summary_row() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-heart-1",
                "@alice:example.org",
                "heart",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-plus",
                "@bob:example.org",
                "+1",
                1_700_000_000_200,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-eyes",
                "@carol:example.org",
                "eyes",
                1_700_000_000_300,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-heart-2",
                "@dave:example.org",
                "heart",
                1_700_000_000_400,
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        assert_eq!(
            rows.iter()
                .filter(|row| row.kind == TimelineRowKind::ReactionSummary)
                .count(),
            1
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] <@alice:example> hello".to_owned(),
                "    reactions: heart @alice:example.org, @dave:example.org  +1 @bob:example.org  eyes @carol:example.org".to_owned(),
            ]
        );
    }

    #[test]
    fn reaction_row_names_reactors_by_member_nick_once_members_arrive() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        for (reaction_event_id, sender, timestamp_ms) in [
            ("$r1", "@alice:example", 1_700_000_000_300),
            ("$r2", "@ghost:example", 1_700_000_000_200),
        ] {
            apply_core_event(
                &mut state,
                incoming_reaction_added(
                    "!ops:example",
                    "$!ops:example:hello",
                    reaction_event_id,
                    sender,
                    "+1",
                    timestamp_ms,
                ),
            )
            .unwrap();
        }
        let reaction_row = |state: &WindowState| {
            build_room_timeline_rows(&state.model, "!ops:example", state.timestamps())
                .into_iter()
                .find(|row| row.kind == TimelineRowKind::ReactionSummary)
                .expect("the message has a reaction row")
        };
        assert_eq!(
            reaction_row(&state).logical_text,
            "    reactions: +1 @ghost:example, @alice:example"
        );

        // Members arrive after the reactions; the row is rebuilt from them.
        apply_core_event(
            &mut state,
            Event::RoomMembers(RoomMembers {
                room_id: "!ops:example".to_owned(),
                members: vec![member_summary("@alice:example", Some("Alice"), false)],
            }),
        )
        .unwrap();
        let row = reaction_row(&state);
        assert_eq!(row.logical_text, "    reactions: +1 @ghost:example, Alice");
        // It wraps like a log line instead of being clipped.
        assert_eq!(row.layout, TimelineRowLayout::Log);
    }

    #[test]
    fn same_reaction_event_id_does_not_double_count() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "@bob:example.org",
                "+1",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "@bob:example.org",
                "+1",
                1_700_000_000_100,
            ),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        assert_eq!(
            room_view.reaction_summaries_for_event("$!ops:example:hello"),
            vec![DumpRoomReactionSummary {
                key: "+1".to_owned(),
                count: 1,
                senders: vec!["@bob:example.org".to_owned()],
                reaction_event_ids: vec!["$reaction-1".to_owned()],
            }]
        );
    }

    #[test]
    fn different_reaction_event_ids_with_same_sender_and_key_count_separately() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "@bob:example.org",
                "+1",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-2",
                "@bob:example.org",
                "+1",
                1_700_000_000_200,
            ),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        assert_eq!(
            room_view.reaction_summaries_for_event("$!ops:example:hello"),
            vec![DumpRoomReactionSummary {
                key: "+1".to_owned(),
                count: 2,
                senders: vec!["@bob:example.org".to_owned(), "@bob:example.org".to_owned(),],
                reaction_event_ids: vec!["$reaction-1".to_owned(), "$reaction-2".to_owned()],
            }]
        );
    }

    #[test]
    fn reaction_removed_removes_only_matching_event_id_and_key() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "@bob:example.org",
                "+1",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-2",
                "@carol:example.org",
                "+1",
                1_700_000_000_200,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-3",
                "@dave:example.org",
                "heart",
                1_700_000_000_300,
            ),
        )
        .unwrap();

        apply_core_event(
            &mut state,
            incoming_reaction_removed(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "$redaction-wrong-key",
                "heart",
            ),
        )
        .unwrap();
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] <@alice:example> hello".to_owned(),
                "    reactions: +1 @bob:example.org, @carol:example.org  heart @dave:example.org"
                    .to_owned(),
            ]
        );

        apply_core_event(
            &mut state,
            incoming_reaction_removed(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "$redaction-right",
                "+1",
            ),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] <@alice:example> hello".to_owned(),
                "    reactions: +1 @carol:example.org  heart @dave:example.org".to_owned(),
            ]
        );
    }

    #[test]
    fn redacting_target_removes_reaction_summary_and_leaves_no_reaction_row() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event:example.org",
                None,
                "hello",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$event:example.org",
                "$reaction-1:example.org",
                "@bob:example.org",
                "+1",
                1_700_000_000_500,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$event:example.org",
                "$redaction:example.org",
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event,
                TimelineRowKind::StateNotice { highlight: false }
            ]
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] * message redacted".to_owned(),
            ]
        );
    }

    #[test]
    fn thread_timeline_projection_includes_reactions_for_root_and_reply() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "thread rep"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$thread-root",
                "$reaction-root",
                "@bob:example.org",
                "heart",
                1_700_000_060_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$thread-1",
                "$reaction-reply",
                "@carol:example.org",
                "eyes",
                1_700_000_120_100,
            ),
        )
        .unwrap();

        let rows = build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
            state.timestamps(),
        );
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event,
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                TimelineRowKind::ReactionSummary,
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                TimelineRowKind::ReactionSummary,
            ]
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:14] <@alice:example> root msg".to_owned(),
                "    reactions: heart @bob:example.org".to_owned(),
                "[22:15] <@alice:example> thread rep".to_owned(),
                "    reactions: eyes @carol:example.org".to_owned(),
            ]
        );
    }

    /// Each of `row`'s styled spans as the text it covers.
    fn styled_row_texts(row: &TimelineRow) -> Vec<(&str, mirx_core::TextStyle)> {
        row.styles
            .iter()
            .map(|span| (&row.logical_text[span.start..span.end], span.style))
            .collect()
    }

    #[test]
    fn message_rows_carry_their_styles_past_the_nick_and_never_on_the_suffix() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", true)];
        for (event_id, presentation) in [
            ("$text", RoomMessagePresentationKind::Text),
            ("$emote", RoomMessagePresentationKind::Emote),
            ("$notice", RoomMessagePresentationKind::Notice),
        ] {
            let mut activity = sent_message_activity_with_presentation(
                "!ops:example",
                event_id,
                None,
                presentation,
                "say bold",
                1_700_000_000_000,
            );
            let RoomActivityKind::Message(message) = &mut activity.kind else {
                unreachable!("a message activity");
            };
            message.styles = vec![TextStyleSpan {
                start: 4,
                end: 8,
                style: bold_style(),
            }];
            apply_core_event(&mut state, Event::RoomActivity(Box::new(activity))).unwrap();
        }
        let Event::RoomActivityUpdate(mut edit) = incoming_replaced(
            "!ops:example",
            "$text",
            "$edit",
            RoomMessagePresentationKind::Text,
            "now bold",
        ) else {
            unreachable!("a replacement");
        };
        let RoomActivityUpdateKind::MessageReplaced { styles, .. } = &mut edit.kind else {
            unreachable!("a replacement");
        };
        *styles = vec![TextStyleSpan {
            start: 0,
            end: 8,
            style: bold_style(),
        }];
        apply_core_event(&mut state, Event::RoomActivityUpdate(edit)).unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example", utc_timestamps())
            .into_iter()
            .filter(|row| row.event_id.is_some())
            .collect::<Vec<_>>();
        assert_eq!(
            rows.iter()
                .map(|row| (row.logical_text.as_str(), styled_row_texts(row)))
                .collect::<Vec<_>>(),
            vec![
                (
                    "[22:13] <@alice:example> now bold (edited)",
                    vec![("now bold", bold_style())]
                ),
                (
                    "[22:13] * @alice:example say bold",
                    vec![("bold", bold_style())]
                ),
                (
                    "[22:13] -@alice:example- say bold",
                    vec![("bold", bold_style())]
                ),
            ]
        );
    }

    #[test]
    fn build_room_timeline_rows_capture_kind_order_and_projection() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", true)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org", "@bob:example.org"]),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event-text:example.org",
                None,
                "<body>",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(sent_message_activity_with_presentation(
                "!ops:example",
                "$event-notice:example.org",
                None,
                RoomMessagePresentationKind::Notice,
                "maintenance window",
                1_700_000_030_000,
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(sent_message_activity_with_presentation(
                "!ops:example",
                "$event-emote:example.org",
                None,
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_090_000,
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-pending", "pending local echo"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            failed_message("!ops:example", "txn-failed", "failed local echo", "network"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "body fallback",
                Some("diagram.png"),
                Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: None,
                }),
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(file_activity(
                "!ops:example",
                "$file1:example.org",
                "@alice:example.org",
                1_710_000_060_000,
                "Quarterly report",
                Some("report.pdf"),
                Some(RoomFileInfo {
                    mimetype: Some("application/pdf".to_owned()),
                    size: Some(45_678),
                }),
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(audio_activity(
                "!ops:example",
                "$audio1:example.org",
                "@alice:example.org",
                1_710_000_120_000,
                "Voice note",
                Some("voice-note.ogg"),
                Some(RoomAudioInfo {
                    mimetype: Some("audio/ogg".to_owned()),
                    size: Some(23_456),
                    duration_ms: Some(12_000),
                }),
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            unsupported_message(
                "!ops:example",
                "$video1:example.org",
                "@alice:example.org",
                1_710_000_180_000,
                "com.example.custom",
                "clip.mp4",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            member_state(
                "!ops:example",
                "@carol:example",
                RoomMemberChange::Joined,
                Some("carol"),
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            member_state(
                "!ops:example",
                "@dave:example",
                RoomMemberChange::Left,
                None,
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event,
                TimelineRowKind::Event,
                TimelineRowKind::Info,
                TimelineRowKind::StateNotice { highlight: false },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Notice,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Emote,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Pending,
                    highlight: false,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Failed,
                    highlight: false,
                },
                TimelineRowKind::Image { highlight: false },
                TimelineRowKind::File { highlight: false },
                TimelineRowKind::Audio { highlight: false },
                TimelineRowKind::Unsupported { highlight: false },
                TimelineRowKind::StateNotice { highlight: false },
                TimelineRowKind::StateNotice { highlight: false },
                TimelineRowKind::Typing,
            ]
        );
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            project_timeline_rows(&rows)
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "* Topic is 'incident review'".to_owned(),
                "* Messages in this room are end-to-end encrypted".to_owned(),
                "[22:14] * @alice:example changes topic to 'incident review'".to_owned(),
                "[22:13] <@alice:example> <body>".to_owned(),
                "[22:13] -@alice:example- maintenance window".to_owned(),
                "[22:14] * @alice:example waves".to_owned(),
                "[22:13] <@alice:example> pending local echo (sending)".to_owned(),
                "[22:13] <@alice:example> failed local echo (failed)".to_owned(),
                "[16:00] * @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)"
                    .to_owned(),
                "[16:01] * @alice:example.org sent file: report.pdf (application/pdf, 45678 bytes)"
                    .to_owned(),
                "[16:02] * @alice:example.org sent audio: voice-note.ogg (audio/ogg, 23456 bytes, 12000 ms)"
                    .to_owned(),
                "[16:03] * @alice:example.org sent unsupported com.example.custom: clip.mp4"
                    .to_owned(),
                "[22:15] * Joins: carol (@carol:example)".to_owned(),
                "[22:15] * Parts: @dave:example".to_owned(),
                "* @alice:example.org, @bob:example.org are typing".to_owned(),
            ]
        );
        let (typing, log_rows) = rows.split_last().expect("rows end with the typing row");
        assert_eq!(typing.layout, TimelineRowLayout::FullWidth);
        for row in log_rows {
            assert_eq!(row.layout, TimelineRowLayout::Log, "{}", row.logical_text);
        }
    }

    #[test]
    fn typing_indicator_names_typists_by_member_nick() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            Event::RoomMembers(RoomMembers {
                room_id: "!ops:example".to_owned(),
                members: vec![
                    member_summary("@alice:example", Some("Alice"), false),
                    member_summary("@sam1:example", Some("Sam"), true),
                ],
            }),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            typing_event(
                "!ops:example",
                &["@alice:example", "@sam1:example", "@stranger:example"],
            ),
        )
        .unwrap();

        assert!(
            render_room_lines(&state, "!ops:example")
                .contains(&"* Alice, Sam (@sam1:example), @stranger:example are typing".to_owned())
        );
    }

    #[test]
    fn room_lines_show_sender_display_names() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        let mut activity =
            sent_message_activity("!ops:example", "$named", None, "hello", 1_700_000_000_000);
        let RoomActivityKind::Message(message) = &mut activity.kind else {
            unreachable!("message activity helper builds messages");
        };
        message.sender = RoomUserProfile {
            user_id: "@sam1:example".to_owned(),
            display_name: Some("Sam".to_owned()),
            display_name_ambiguous: true,
        };
        apply_core_event(&mut state, Event::RoomActivity(Box::new(activity))).unwrap();

        assert!(
            render_room_lines(&state, "!ops:example")
                .iter()
                .any(|line| line.ends_with("<Sam (@sam1:example)> hello"))
        );
    }

    #[test]
    fn render_room_lines_include_text_notice_emote_and_state_activities() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", true)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org", "@bob:example.org"]),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event-text:example.org",
                None,
                "<body>",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(sent_message_activity_with_presentation(
                "!ops:example",
                "$event-notice:example.org",
                None,
                RoomMessagePresentationKind::Notice,
                "maintenance window",
                1_700_000_030_000,
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(sent_message_activity_with_presentation(
                "!ops:example",
                "$event-emote:example.org",
                None,
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_090_000,
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            member_state(
                "!ops:example",
                "@carol:example",
                RoomMemberChange::Joined,
                Some("carol"),
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            member_state(
                "!ops:example",
                "@dave:example",
                RoomMemberChange::Left,
                None,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            member_state(
                "!ops:example",
                "@erin:example",
                RoomMemberChange::Banned,
                Some("erin"),
            ),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "* Topic is 'incident review'".to_owned(),
                "* Messages in this room are end-to-end encrypted".to_owned(),
                "[22:14] * @alice:example changes topic to 'incident review'".to_owned(),
                "[22:13] <@alice:example> <body>".to_owned(),
                "[22:13] -@alice:example- maintenance window".to_owned(),
                "[22:14] * @alice:example waves".to_owned(),
                "[22:15] * Joins: carol (@carol:example)".to_owned(),
                "[22:15] * Parts: @dave:example".to_owned(),
                "[22:15] * erin was banned by @alice:example".to_owned(),
                "* @alice:example.org, @bob:example.org are typing".to_owned(),
            ]
        );
    }

    #[test]
    fn room_timeline_rows_show_loading_history_backfill_after_context_rows() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!ops:example", "latest row")).unwrap();
        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view exists after room activity")
            .history
            .status = RoomHistoryBackfillStatus::Loading { from: None };

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());

        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event,
                TimelineRowKind::Event,
                TimelineRowKind::Plain,
                TimelineRowKind::StateNotice { highlight: false },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                TimelineRowKind::Typing,
            ]
        );
        assert_eq!(rows[2].logical_text, "* Loading older history...");
    }

    #[test]
    fn room_timeline_rows_show_failed_history_backfill_after_context_rows() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!ops:example", "latest row")).unwrap();
        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view exists after room activity")
            .history
            .status = RoomHistoryBackfillStatus::Failed {
            from: Some("token-1".to_owned()),
            message: "homeserver unavailable".to_owned(),
        };

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());

        assert_eq!(rows[2].kind, TimelineRowKind::Plain);
        assert_eq!(
            rows[2].logical_text,
            "* Unable to load older history: homeserver unavailable. Scroll to top to retry."
        );
    }

    #[test]
    fn room_timeline_rows_show_exhausted_history_backfill_after_context_rows() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!ops:example", "latest row")).unwrap();
        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view exists after room activity")
            .history
            .status = RoomHistoryBackfillStatus::Exhausted;

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());

        assert_eq!(rows[2].kind, TimelineRowKind::Plain);
        assert_eq!(rows[2].logical_text, "* No older history available.");
    }

    #[test]
    fn room_timeline_rows_skip_idle_history_backfill_row() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!ops:example", "latest row")).unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());

        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event,
                TimelineRowKind::Event,
                TimelineRowKind::StateNotice { highlight: false },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                TimelineRowKind::Typing,
            ]
        );
        assert!(
            rows.iter()
                .all(|row| !row.logical_text.contains("older history"))
        );
    }

    #[test]
    fn redaction_state_notice_renders_generic_tombstone_without_old_notice_text() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            member_state(
                "!ops:example",
                "@carol:example",
                RoomMemberChange::Joined,
                Some("carol"),
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$member:!ops:example:@carol:example:Joined",
                "$redaction:example.org",
            ),
        )
        .unwrap();

        let rendered = render_room_lines(&state, "!ops:example").join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("Joins: carol"));
    }

    #[test]
    fn room_opening_lines_leave_label_markers_to_the_caption() {
        let mut state = test_state();
        let mut direct_room = room("!direct:example", "Direct", true);
        direct_room.is_direct = true;
        direct_room.direct_user_ids = vec!["@alice:example".to_owned()];
        state.model.rooms = vec![direct_room];

        let rows = build_room_timeline_rows(&state.model, "!direct:example", state.timestamps());
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![TimelineRowKind::Event, TimelineRowKind::Info]
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "* Now talking in Direct (!direct:example)".to_owned(),
                "* Messages in this room are end-to-end encrypted".to_owned(),
            ]
        );
    }

    #[test]
    fn render_room_lines_include_unsupported_message_placeholder() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            unsupported_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "com.example.custom",
                "clip.mp4",
            ),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[16:00] * @alice:example.org sent unsupported com.example.custom: clip.mp4"
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn room_and_status_rows_follow_the_timestamp_setting() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(sent_message_activity(
                "!ops:example",
                "$hi:example",
                None,
                "hi",
                1_700_000_000_000,
            ))),
        )
        .unwrap();
        state.model.status_log.push(
            1_700_000_000_000,
            StatusLineKind::Info,
            "* ready".to_owned(),
        );

        state
            .settings
            .update(|settings| settings.timestamp.show = false)
            .unwrap();
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "<@alice:example> hi".to_owned(),
            ]
        );
        assert_eq!(
            project_timeline_rows(&build_status_timeline_rows(
                &state.model.status_log,
                state.timestamps()
            )),
            vec!["* ready".to_owned()]
        );
        assert_eq!(state.timestamps().indent_sample(), "");

        state
            .settings
            .update(|settings| {
                settings.timestamp = TimestampSetting {
                    show: true,
                    format: timestamp::TimestampFormat::parse("[HH:nn:ss]").unwrap(),
                }
            })
            .unwrap();
        assert_eq!(
            render_room_lines(&state, "!ops:example")[1],
            "[22:13:20] <@alice:example> hi"
        );
        assert_eq!(
            project_timeline_rows(&build_status_timeline_rows(
                &state.model.status_log,
                state.timestamps()
            )),
            vec!["[22:13:20] * ready".to_owned()]
        );
    }

    #[test]
    fn render_room_lines_include_unable_to_decrypt_placeholder() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", true)];
        apply_core_event(
            &mut state,
            unable_to_decrypt_activity(
                "!ops:example",
                "$encrypted1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
            ),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "* Messages in this room are end-to-end encrypted".to_owned(),
                "[16:00] * @alice:example.org sent an encrypted message that could not be decrypted"
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn redaction_unsupported_message_hides_msgtype_and_body() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            unsupported_message(
                "!ops:example",
                "$unsupported:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "com.example.custom",
                "clip.mp4",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$unsupported:example.org",
                "$redaction:example.org",
            ),
        )
        .unwrap();

        let rendered = render_room_lines(&state, "!ops:example").join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("com.example.custom"));
        assert!(!rendered.contains("clip.mp4"));
    }

    #[test]
    fn render_room_lines_include_typed_image_with_metadata() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "body fallback",
                Some("diagram.png"),
                Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: None,
                }),
            ),
        )
        .unwrap();

        let lines = render_room_lines(&state, "!ops:example");
        assert_eq!(
            lines,
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[16:00] * @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)"
                    .to_owned(),
            ]
        );
        assert!(!lines.join("\n").contains("unsupported m.image"));
    }

    #[test]
    fn render_room_lines_omit_empty_image_metadata_suffix() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[16:00] * @alice:example.org sent image: diagram.png".to_owned(),
            ]
        );
    }

    #[test]
    fn image_rows_project_pending_media_state_without_media_event() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        let image_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Image { .. }))
            .expect("image row must be present");

        assert_eq!(image_row.image_state, Some(TimelineImageState::Pending));
        assert!(
            !render_room_lines(&state, "!ops:example")
                .join("\n")
                .contains("loading image...")
        );
    }

    #[test]
    fn failed_room_preview_media_event_projects_failed_image_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "unavailable".to_owned(),
                },
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .and_then(|room_view| room_view.preview_media.get("event:$image1:example.org"))
                .cloned(),
            Some(RoomPreviewMediaState::Failed {
                event_id: "$image1:example.org".to_owned(),
                format: timeline_thumbnail_format(),
                message: "unavailable".to_owned(),
            })
        );

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        let image_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Image { .. }))
            .expect("image row must be present");
        assert_eq!(
            image_row.image_state,
            Some(TimelineImageState::Failed {
                message: "unavailable".to_owned(),
            })
        );
        assert!(
            !render_room_lines(&state, "!ops:example")
                .join("\n")
                .contains("image unavailable")
        );
    }

    #[test]
    fn video_preview_media_projects_failure_and_ignores_wrong_format() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            video_message("!ops:example", "$video1:example.org"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$video1:example.org",
                "$video1:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "unavailable".to_owned(),
                },
            ),
        )
        .unwrap();
        assert!(
            build_room_timeline_rows(&state.model, "!ops:example", state.timestamps())
                .iter()
                .any(|row| matches!(
                    row.image_state,
                    Some(TimelineImageState::Failed { ref message }) if message == "unavailable"
                ))
        );
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$video1:example.org",
                "$video1:example.org",
                RoomMediaKind::Failed {
                    format: RoomMediaFormat::File,
                    message: "wrong".to_owned(),
                },
            ),
        )
        .unwrap();
        assert_eq!(
            state.model.room_views["!ops:example"].preview_media.len(),
            1
        );
    }

    #[test]
    fn invalid_loaded_room_preview_media_event_projects_failed_image_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomMediaKind::Loaded {
                    format: timeline_thumbnail_format(),
                    bytes: vec![0, 1, 2, 3],
                },
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        let image_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Image { .. }))
            .expect("image row must be present");
        assert!(matches!(
            image_row.image_state,
            Some(TimelineImageState::Failed { .. })
        ));
    }

    #[test]
    fn loaded_room_preview_media_event_projects_loaded_image_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomMediaKind::Loaded {
                    format: timeline_thumbnail_format(),
                    bytes: tiny_png_bytes(),
                },
            ),
        )
        .unwrap();

        let image_key = room_preview_media_storage_key(
            "!ops:example",
            "event:$image1:example.org",
            "$image1:example.org",
            &timeline_thumbnail_format(),
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .and_then(|room_view| room_view.preview_media.get("event:$image1:example.org"))
                .cloned(),
            Some(RoomPreviewMediaState::Loaded {
                event_id: "$image1:example.org".to_owned(),
                format: timeline_thumbnail_format(),
                image_key: image_key.clone(),
                width: 1,
                height: 1,
            })
        );
        assert!(state.decoded_images.contains_key(&image_key));

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        let image_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Image { .. }))
            .expect("image row must be present");
        assert_eq!(
            image_row.image_state,
            Some(TimelineImageState::Loaded {
                key: image_key,
                width: 1,
                height: 1,
            })
        );
    }

    #[test]
    fn thread_image_rows_use_room_backed_preview_media_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        let mut activity = image_activity(
            "!ops:example",
            "$thread-image:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "diagram.png",
            None,
            None,
        );
        activity.relation = Some(thread_relation("$thread-root:example.org"));
        apply_core_event(&mut state, Event::RoomActivity(Box::new(activity))).unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$thread-image:example.org",
                "$thread-image:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "unavailable".to_owned(),
                },
            ),
        )
        .unwrap();

        let rows = build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root:example.org",
            state.timestamps(),
        );
        let image_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Image { .. }))
            .expect("thread image row must be present");
        assert_eq!(
            image_row.image_state,
            Some(TimelineImageState::Failed {
                message: "unavailable".to_owned(),
            })
        );
    }

    #[test]
    fn thread_rows_open_like_the_room_and_end_with_typing() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", true)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        let mut event = video_message("!ops:example", "$thread-video:example.org");
        let Event::RoomActivity(activity) = &mut event else {
            unreachable!("video message must create a room activity");
        };
        activity.relation = Some(thread_relation("$thread-root:example.org"));
        apply_core_event(&mut state, event).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();

        let rows = build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root:example.org",
            state.timestamps(),
        );
        let lines = project_timeline_rows(&rows);
        assert_eq!(
            lines[..3],
            [
                "* Now talking in Ops (!ops:example)".to_owned(),
                "* Topic is 'incident review'".to_owned(),
                "* Messages in this room are end-to-end encrypted".to_owned(),
            ]
        );
        assert!(matches!(rows[3].kind, TimelineRowKind::Video { .. }));
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[4].kind, TimelineRowKind::Typing);
        assert_eq!(lines[4], "* @alice:example.org is typing");
    }

    #[test]
    fn thread_video_rows_use_room_backed_preview_media_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        let mut event = video_message("!ops:example", "$thread-video:example.org");
        let Event::RoomActivity(activity) = &mut event else {
            unreachable!("video message must create a room activity");
        };
        activity.relation = Some(thread_relation("$thread-root:example.org"));
        apply_core_event(&mut state, event).unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$thread-video:example.org",
                "$thread-video:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "unavailable".to_owned(),
                },
            ),
        )
        .unwrap();

        let rows = build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root:example.org",
            state.timestamps(),
        );
        let video_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Video { .. }))
            .expect("thread video row must be present");
        assert_eq!(
            video_row.image_state,
            Some(TimelineImageState::Failed {
                message: "unavailable".to_owned(),
            })
        );
    }

    #[test]
    fn render_room_lines_use_singular_typing_verb_for_one_user() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "* @alice:example.org is typing".to_owned(),
            ]
        );
    }

    #[test]
    fn redaction_thread_root_keeps_thread_reachable_and_labels_it_message_redacted() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "old root",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "thread rep"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$thread-root", "$redaction:example.org"),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        assert_eq!(
            room_view
                .threads
                .get("$thread-root")
                .map(|thread| thread.label.as_str()),
            Some("Message redacted")
        );

        let lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
            state.timestamps(),
        ));
        let rendered = lines.join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("old root"));
    }

    #[test]
    fn thread_timeline_filters_room_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(&mut state, timeline("!ops:example", "lobby")).unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$reply-1",
                None,
                Some(reply_relation("$lobby-root")),
                RoomMessagePresentationKind::Text,
                "plain",
                1_700_000_090_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "thread rep"),
        )
        .unwrap();

        let lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
            state.timestamps(),
        ));

        assert!(
            lines
                .iter()
                .any(|line| line.contains("* Now talking in Ops (!ops:example)"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("* Topic is 'incident review'"))
        );
        assert!(lines.iter().any(|line| line.contains("root msg")));
        assert!(lines.iter().any(|line| line.contains("thread rep")));
        assert!(!lines.iter().any(|line| line.contains("lobby")));
        assert!(!lines.iter().any(|line| line.contains("plain")));
    }

    #[test]
    fn replacing_threaded_reply_updates_room_and_thread_timeline_projections() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "old reply"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$thread-1",
                "$thread-edit:example.org",
                RoomMessagePresentationKind::Text,
                "edited reply",
            ),
        )
        .unwrap();

        assert!(
            render_room_lines(&state, "!ops:example")
                .iter()
                .any(|line| line.contains("edited reply"))
        );
        let lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
            state.timestamps(),
        ));
        assert!(lines.iter().any(|line| line.contains("edited reply")));
        assert!(!lines.iter().any(|line| line.contains("old reply")));
    }

    #[test]
    fn redaction_threaded_reply_updates_room_and_thread_timeline_projections() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "old reply"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$thread-1", "$redaction:example.org"),
        )
        .unwrap();

        let room_rendered = render_room_lines(&state, "!ops:example").join("\n");
        assert!(room_rendered.contains("message redacted"));
        assert!(!room_rendered.contains("old reply"));

        let thread_lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
            state.timestamps(),
        ));
        let thread_rendered = thread_lines.join("\n");
        assert!(thread_rendered.contains("message redacted"));
        assert!(!thread_rendered.contains("old reply"));
    }

    #[test]
    fn highlighted_activity_builds_highlighted_rows_but_redactions_do_not() {
        let mut rows = Vec::new();
        let mention = highlighted(sent_message_activity(
            "!room:example",
            "$m",
            None,
            "ping",
            1,
        ));
        push_room_activity_row(
            &mut rows,
            &RoomViewModel::default(),
            &mention,
            utc_timestamps(),
        );
        let redacted = RoomActivity {
            kind: RoomActivityKind::Redacted(RoomRedactedActivity {
                event_id: "$m".to_owned(),
            }),
            ..mention.clone()
        };
        push_room_activity_row(
            &mut rows,
            &RoomViewModel::default(),
            &redacted,
            utc_timestamps(),
        );

        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: true,
                },
                TimelineRowKind::StateNotice { highlight: false },
            ]
        );
    }

    #[test]
    fn echo_lines_merge_into_the_room_timeline_by_time() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        for (event_id, body, timestamp_ms) in [
            ("$first", "first", 1_700_000_060_000),
            ("$second", "second", 1_700_000_180_000),
        ] {
            apply_core_event(
                &mut state,
                timeline_with_transaction_id("!ops:example", event_id, None, body, timestamp_ms),
            )
            .unwrap();
        }
        let echo = &mut state.model.room_views.get_mut("!ops:example").unwrap().echo;
        echo.push(1_700_000_000_000, None, "* before".to_owned());
        echo.push(1_700_000_120_000, None, "* between".to_owned());
        // A tie stays after the activity the user already saw.
        echo.push(1_700_000_180_000, None, "* tie".to_owned());
        echo.push(1_700_000_240_000, None, "* tie".to_owned());

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        let texts = project_timeline_rows(&rows);
        let tail = &texts[texts.len() - 6..];
        assert_eq!(
            tail,
            [
                "[22:13] * before",
                "[22:14] <@alice:example> first",
                "[22:15] * between",
                "[22:16] <@alice:example> second",
                "[22:16] * tie",
                "[22:17] * tie",
            ]
        );
        let identities = rows[rows.len() - 2..]
            .iter()
            .map(|row| row.identity.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            identities,
            vec![
                TimelineRowIdentity::EchoLine { sequence: 2 },
                TimelineRowIdentity::EchoLine { sequence: 3 },
            ]
        );
    }

    #[test]
    fn thread_echo_shows_only_in_its_thread_and_survives_room_clear() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "thread rep"),
        )
        .unwrap();
        let room_view = state.model.room_views.get_mut("!ops:example").unwrap();
        room_view.echo.push(
            1_700_000_200_000,
            Some("$thread-root".to_owned()),
            "* /clear: nothing to clear in thread view".to_owned(),
        );
        room_view
            .echo
            .push(1_700_000_200_000, None, "* room line".to_owned());

        let thread_lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
            state.timestamps(),
        ));
        assert!(
            thread_lines.contains(&"[22:16] * /clear: nothing to clear in thread view".to_owned())
        );
        assert!(!thread_lines.contains(&"[22:16] * room line".to_owned()));
        let room_lines = render_room_lines(&state, "!ops:example");
        assert!(room_lines.contains(&"[22:16] * room line".to_owned()));
        assert!(
            !room_lines
                .iter()
                .any(|line| line.contains("nothing to clear"))
        );

        state
            .model
            .room_views
            .get_mut("!ops:example")
            .unwrap()
            .clear_visible_entries();
        assert!(
            !render_room_lines(&state, "!ops:example").contains(&"[22:16] * room line".to_owned())
        );
        let thread_lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
            state.timestamps(),
        ));
        assert!(
            thread_lines.contains(&"[22:16] * /clear: nothing to clear in thread view".to_owned())
        );
    }
}
