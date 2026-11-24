//! Building a view's timeline rows from the model.

use std::ops::Range;

use mirx_core::RoomMemberChange;

use super::colors::PaletteIndex;
use super::hotlinks::RowUserLink;
use super::nick_colors::{NickColorer, ResolvedNickColor};
use super::timeline_text::MessageLine;
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
    /// mIRC's line marker: where the user stopped reading.
    LineMarker,
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
    /// Where `logical_text` links to users, sorted and apart: a message's
    /// sender nick and its pills. Every other row has none.
    pub(super) user_links: Vec<RowUserLink>,
    /// The nick of the user who sent the line, on the lines that start
    /// with their nick (messages, actions, media, ...), and the colour the
    /// nick colour list gives it. Never in `styles`: it is drawn, not
    /// logged or copied.
    pub(super) sender: Option<RowSender>,
    pub(super) layout: TimelineRowLayout,
    pub(super) image_state: Option<TimelineImageState>,
    /// The line's place in its window's arrival order: a live activity's
    /// room arrival, an echo line's, a Status line's sequence. `None` for
    /// context rows and for history (backfill), which did not arrive.
    pub(super) arrival: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TimelineRowLayout {
    /// Context rows (typing, history backfill): one painted line per text line.
    FullWidth,
    /// mIRC log lines: wrapped to the view, continuation lines indented by
    /// two spaces.
    Log,
    /// One line's height with a horizontal rule across it, and no text: the
    /// line marker.
    Rule,
}

impl TimelineRow {
    pub(super) fn full_width(kind: TimelineRowKind, text: String) -> Self {
        Self {
            identity: TimelineRowIdentity::default_for_row(&kind, &text),
            event_id: None,
            kind,
            logical_text: text,
            styles: Vec::new(),
            user_links: Vec::new(),
            sender: None,
            layout: TimelineRowLayout::FullWidth,
            image_state: None,
            arrival: None,
        }
    }

    pub(super) fn log(kind: TimelineRowKind, text: String) -> Self {
        Self {
            identity: TimelineRowIdentity::default_for_row(&kind, &text),
            event_id: None,
            kind,
            logical_text: text,
            styles: Vec::new(),
            user_links: Vec::new(),
            sender: None,
            layout: TimelineRowLayout::Log,
            image_state: None,
            arrival: None,
        }
    }

    /// The line marker's row: a rule, below the last line the user read.
    /// It is not text: nothing logs, saves, copies or finds it.
    pub(super) fn line_marker() -> Self {
        Self {
            identity: TimelineRowIdentity::synthetic(TimelineSyntheticRowLabel::LineMarker, ""),
            event_id: None,
            kind: TimelineRowKind::LineMarker,
            logical_text: String::new(),
            styles: Vec::new(),
            user_links: Vec::new(),
            sender: None,
            layout: TimelineRowLayout::Rule,
            image_state: None,
            arrival: None,
        }
    }

    pub(super) fn with_identity(mut self, identity: TimelineRowIdentity) -> Self {
        self.identity = identity;
        self
    }

    pub(super) fn arrived(mut self, arrival: Option<u64>) -> Self {
        self.arrival = arrival;
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

    pub(super) fn with_user_links(mut self, user_links: Vec<RowUserLink>) -> Self {
        let mut previous_end = 0;
        for link in &user_links {
            assert!(
                previous_end <= link.range.start
                    && link.range.start < link.range.end
                    && self.logical_text.get(link.range.clone()).is_some(),
                "a row's user links are sorted, apart and in its text: {user_links:?}"
            );
            previous_end = link.range.end;
        }
        self.user_links = user_links;
        self
    }

    fn with_sender(mut self, sender: Option<RowUserLink>) -> Self {
        self.sender = sender.map(|link| {
            assert!(
                link.range.start < link.range.end
                    && self.logical_text.get(link.range.clone()).is_some(),
                "a row's sender nick is in its text: {link:?}"
            );
            RowSender {
                range: link.range,
                user_id: link.user_id,
                color: None,
            }
        });
        self
    }
}

/// Where a line shows its sender's nick, and its nick colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RowSender {
    pub(super) range: Range<usize>,
    pub(super) user_id: String,
    /// `None` until `color_row_senders`, and for users no entry fits.
    pub(super) color: Option<ResolvedNickColor>,
}

/// Gives every row's sender nick its colour from the nick colour list.
pub(super) fn color_row_senders(rows: &mut [TimelineRow], colorer: &NickColorer) {
    for sender in rows.iter_mut().filter_map(|row| row.sender.as_mut()) {
        sender.color = colorer.color(&sender.user_id);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Rows for lines other users wrote carry `highlight`: the user's push
/// rules highlighted the line, so it is drawn in the highlight colour.
pub(super) enum TimelineRowKind {
    /// mIRC's event lines, such as `* Now talking in` and `* Topic is`, in
    /// their item's colour.
    Event {
        item: EventItem,
    },
    /// mIRC's info lines.
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
        /// The signed-in user sent it: mIRC's Own text.
        own: bool,
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
    /// A poll's start or end.
    Poll {
        highlight: bool,
    },
    /// An in-room verification request.
    VerificationRequest {
        highlight: bool,
    },
    Unsupported {
        highlight: bool,
    },
    UnableToDecrypt,
    StateNotice {
        highlight: bool,
        item: EventItem,
    },
    /// What is left of a redacted event.
    Redacted,
    Plain,
    /// `/echo`'s line, in the Normal colour or its own.
    Echo {
        color: Option<PaletteIndex>,
    },
    /// The line marker, `TimelineRow::line_marker`.
    LineMarker,
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

/// A message waiting for the connection is still on its way: dim, and not
/// logged until it is sent.
fn timeline_message_send_state(send_state: &RoomMessageSendState) -> TimelineMessageSendState {
    match send_state {
        RoomMessageSendState::Pending
        | RoomMessageSendState::Failed {
            recoverable: true, ..
        } => TimelineMessageSendState::Pending,
        RoomMessageSendState::Sent => TimelineMessageSendState::Sent,
        RoomMessageSendState::Failed {
            recoverable: false, ..
        } => TimelineMessageSendState::Failed,
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
    let invite = TimelineRowKind::Event {
        item: EventItem::Invite,
    };
    vec![
        room_context_row(
            invite.clone(),
            TimelineSyntheticRowLabel::Invite,
            &invited_text(
                room.invite
                    .as_ref()
                    .expect("an invite's summary names its invite"),
                &room.display_name,
            ),
        ),
        room_context_row(
            invite,
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
        TimelineRowKind::Event {
            item: EventItem::Join,
        },
        TimelineSyntheticRowLabel::NowTalking,
        &now_talking_text(&room.display_name, &room.room_id),
    ));
    if let Some(topic) = room_view.and_then(|view| view.topic.as_deref()) {
        rows.push(room_context_row(
            TimelineRowKind::Event {
                item: EventItem::Topic,
            },
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
    let text = match &history {
        RoomHistoryBackfillState::Idle { .. } => return,
        RoomHistoryBackfillState::Loading { .. } => "* Loading older history...".to_owned(),
        RoomHistoryBackfillState::Failed { message, .. } => {
            format!("* Unable to load older history: {message}. Scroll to top to retry.")
        }
        RoomHistoryBackfillState::Exhausted => "* No older history available.".to_owned(),
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
    own_user_id: Option<&str>,
    rank_glyphs: bool,
) {
    push_reply_quote_row(rows, room_view, activity, own_user_id);
    let ActivityLine {
        kind,
        text,
        styles,
        user_links,
        sender,
        image_state,
    } = activity_line_text(
        room_view,
        activity,
        &timestamps.prefix(activity.timestamp_ms),
        own_user_id,
        rank_glyphs,
    );
    let mut row = TimelineRow::log(kind, text)
        .with_styles(styles)
        .with_user_links(user_links)
        .with_sender(sender)
        .with_identity(TimelineRowIdentity::activity_primary(&activity.activity_id))
        .with_event_id(room_activity_event_id(activity))
        .arrived(room_view.arrivals.of(&activity.activity_id));
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
    own_user_id: Option<&str>,
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
        ReplyTarget::Known(target) => quoted_line(room_view, target, own_user_id),
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
fn quoted_line(
    room_view: &RoomViewModel,
    activity: &RoomActivity,
    own_user_id: Option<&str>,
) -> String {
    replies::quote_excerpt(&activity_line_text(room_view, activity, "", own_user_id, false).text)
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
            &quoted_line(&model.room_views[room_id], activity, model.own_user_id()),
        ),
    )
}

/// An activity's line: its row kind, text after the timestamp prefix, the
/// text's styles and user links, and the image preview state.
struct ActivityLine {
    kind: TimelineRowKind,
    text: String,
    styles: Vec<TextStyleSpan>,
    user_links: Vec<RowUserLink>,
    /// The sender's nick: the start of a message or action line.
    sender: Option<RowUserLink>,
    image_state: Option<TimelineImageState>,
}

/// A line's text, its user links and its sender's nick.
struct LineText {
    text: String,
    user_links: Vec<RowUserLink>,
    sender: Option<RowUserLink>,
}

impl LineText {
    /// A line that names no sender, as an event line does.
    fn plain(text: String) -> Self {
        Self {
            text,
            user_links: Vec::new(),
            sender: None,
        }
    }
}

/// The link from a line's nick to its sender: a nick hotlink.
fn sender_link(line: &MessageLine, sender: &RoomUserProfile) -> RowUserLink {
    RowUserLink {
        range: line.nick.clone(),
        user_id: sender.user_id.clone(),
    }
}

/// An action line (`* Nick sent image: ...`): its nick is a hotlink.
fn action_line_with_link(timestamp: &str, sender: &RoomUserProfile, body: &str) -> LineText {
    let line = action_line(timestamp, sender, body);
    let link = sender_link(&line, sender);
    LineText {
        text: line.text,
        user_links: vec![link.clone()],
        sender: Some(link),
    }
}

/// What a poll line says after its sender's nick: a start lists its
/// answers, numbered; an end names its poll's question when the room has
/// the start loaded. mirx shows no votes: the end's text is its results.
fn poll_line_body(room_view: &RoomViewModel, poll: &RoomPollActivity) -> String {
    match &poll.content {
        RoomPollContent::Start {
            question,
            answers,
            max_selections,
            results_hidden,
        } => {
            let mut body = format!("started a poll: {question}");
            for (index, answer) in answers.iter().enumerate() {
                body.push_str(&format!(" {}) {answer}", index + 1));
            }
            if *max_selections > 1 {
                body.push_str(&format!(" (up to {max_selections} answers)"));
            }
            if *results_hidden {
                body.push_str(" (results when it ends)");
            }
            body
        }
        RoomPollContent::End {
            poll_start_event_id,
            text,
        } => match poll_question(room_view, poll_start_event_id) {
            Some(question) => format!("ended the poll \"{question}\": {text}"),
            None => format!("ended a poll: {text}"),
        },
    }
}

/// The question of the poll `poll_start_event_id` starts, among the room's
/// loaded lines.
fn poll_question<'a>(room_view: &'a RoomViewModel, poll_start_event_id: &str) -> Option<&'a str> {
    room_view
        .activities
        .iter()
        .find_map(|activity| match &activity.kind {
            RoomActivityKind::Poll(poll) if poll.event_id == poll_start_event_id => {
                match &poll.content {
                    RoomPollContent::Start { question, .. } => Some(question.as_str()),
                    RoomPollContent::End { .. } => None,
                }
            }
            _ => None,
        })
}

/// The colour item of a membership change's line.
fn member_change_item(change: &RoomMemberChange) -> EventItem {
    match change {
        RoomMemberChange::Joined | RoomMemberChange::InvitationAccepted => EventItem::Join,
        RoomMemberChange::Left
        | RoomMemberChange::InvitationRejected
        | RoomMemberChange::KnockRetracted => EventItem::Part,
        RoomMemberChange::Kicked | RoomMemberChange::KickedAndBanned => EventItem::Kick,
        // IRC's +b and -b.
        RoomMemberChange::Banned | RoomMemberChange::Unbanned => EventItem::Settings,
        RoomMemberChange::Invited
        | RoomMemberChange::InvitationRevoked
        | RoomMemberChange::Knocked
        | RoomMemberChange::KnockAccepted
        | RoomMemberChange::KnockDenied => EventItem::Invite,
        RoomMemberChange::NickChanged { .. } | RoomMemberChange::AvatarChanged => EventItem::Nick,
    }
}

/// What a line that could not be decrypted says after its sender: why,
/// as mIRC notes a kick's reason, and what can be done, when anything can.
fn unable_to_decrypt_text(cause: mirx_core::RoomUnableToDecryptCause) -> String {
    use mirx_core::RoomUnableToDecryptCause as Cause;
    let note = match cause {
        Cause::Unknown => "",
        Cause::SentBeforeYouJoined => " (sent before you joined)",
        Cause::HistoricalNoBackup => {
            " (sent before this session existed, and you have no key backup)"
        }
        Cause::HistoricalUnverified => {
            " (sent before this session existed: /verify this session or /e2ee recover to read it)"
        }
        Cause::WithheldUnverified => {
            " (the sender shares keys only with verified sessions: /verify this session)"
        }
        Cause::WithheldBySender => " (the sender withheld its key)",
        Cause::SenderIdentityChanged => " (the sender's verified identity has changed)",
        Cause::UnsignedDevice => " (sent from a session its owner has not verified)",
        Cause::UnknownDevice => " (sent from an unknown session)",
    };
    format!("sent an encrypted message that could not be decrypted{note}")
}

/// `own_user_id` is the signed-in user's, `None` while signed out.
/// `rank_glyphs`: a message's sender has their rank's glyph, as the nick
/// list ranks them now (Matrix keeps no rank at the time).
fn activity_line_text(
    room_view: &RoomViewModel,
    activity: &RoomActivity,
    timestamp: &str,
    own_user_id: Option<&str>,
    rank_glyphs: bool,
) -> ActivityLine {
    let (kind, line_text, image_state) = match &activity.kind {
        RoomActivityKind::Message(message) => {
            let suffix = match &message.send_state {
                RoomMessageSendState::Pending => " (sending)",
                RoomMessageSendState::Sent
                    if message
                        .event_id
                        .as_ref()
                        .is_some_and(|event_id| room_view.line_edits.shows_edited(event_id)) =>
                {
                    " (edited)"
                }
                RoomMessageSendState::Sent => "",
                RoomMessageSendState::Failed {
                    recoverable: true, ..
                } => " (waiting to send)",
                RoomMessageSendState::Failed {
                    recoverable: false, ..
                } => " (failed)",
            };
            let rank_glyph = if rank_glyphs {
                room_view
                    .members
                    .iter()
                    .find(|member| member.user_id == message.sender.user_id)
                    .map_or("", |member| rank_prefix(member.rank))
            } else {
                ""
            };
            let line = message_line(
                timestamp,
                message.presentation,
                rank_glyph,
                &message.sender,
                &format!("{}{suffix}", message.body),
            );
            // The suffix is mirx's note, not the sender's text: unstyled
            // and unlinked.
            let body_offset = line.body_offset;
            let styles = message
                .styles
                .iter()
                .map(|span| TextStyleSpan {
                    start: body_offset + span.start,
                    end: body_offset + span.end,
                    style: span.style,
                })
                .collect();
            let sender = sender_link(&line, &message.sender);
            let user_links = std::iter::once(sender.clone())
                .chain(message.user_links.iter().map(|link| RowUserLink {
                    range: body_offset + link.start..body_offset + link.end,
                    user_id: link.user_id.clone(),
                }))
                .collect();
            let text = line.text;
            return ActivityLine {
                kind: TimelineRowKind::Message {
                    presentation: message.presentation,
                    send_state: timeline_message_send_state(&message.send_state),
                    highlight: activity.highlight,
                    own: own_user_id == Some(message.sender.user_id.as_str()),
                },
                text,
                styles,
                user_links,
                sender: Some(sender),
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
                action_line_with_link(
                    timestamp,
                    &message.sender,
                    &format!("sent image: {display_name}{metadata_suffix}"),
                ),
                image_state,
            )
        }
        // Shown as an image is; its body describes it.
        RoomActivityKind::Sticker(message) => {
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
                action_line_with_link(
                    timestamp,
                    &message.sender,
                    &format!("sent sticker: {}", message.body),
                ),
                image_state,
            )
        }
        RoomActivityKind::Poll(poll) => (
            TimelineRowKind::Poll {
                highlight: activity.highlight,
            },
            action_line_with_link(timestamp, &poll.sender, &poll_line_body(room_view, poll)),
            None,
        ),
        RoomActivityKind::VerificationRequest(request) => {
            const WANTS: &str = "wants to verify ";
            let line = action_line(
                timestamp,
                &request.sender,
                &format!("{WANTS}{}", request.to),
            );
            let to_start = line.body_offset + WANTS.len();
            let sender = sender_link(&line, &request.sender);
            let user_links = vec![
                sender.clone(),
                RowUserLink {
                    range: to_start..to_start + request.to.len(),
                    user_id: request.to.clone(),
                },
            ];
            (
                TimelineRowKind::VerificationRequest {
                    highlight: activity.highlight,
                },
                LineText {
                    text: line.text,
                    user_links,
                    sender: Some(sender),
                },
                None,
            )
        }
        RoomActivityKind::File(message) => {
            let display_name = message.filename.as_deref().unwrap_or(message.body.as_str());
            let metadata_suffix = format_room_file_metadata_suffix(message.info.as_ref());
            (
                TimelineRowKind::File {
                    highlight: activity.highlight,
                },
                action_line_with_link(
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
                action_line_with_link(
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
                action_line_with_link(
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
            action_line_with_link(
                timestamp,
                &message.sender,
                &format!("sent unsupported {}: {}", message.msgtype, message.body),
            ),
            None,
        ),
        RoomActivityKind::UnableToDecrypt(message) => (
            TimelineRowKind::UnableToDecrypt,
            action_line_with_link(
                timestamp,
                &message.sender,
                &unable_to_decrypt_text(message.cause),
            ),
            None,
        ),
        RoomActivityKind::StateNotice(notice) => {
            let (item, body) = match &notice.content {
                RoomStateNoticeContent::Topic { topic } => {
                    (EventItem::Topic, topic_change_text(&notice.sender, topic))
                }
                RoomStateNoticeContent::Member {
                    change,
                    target,
                    reason,
                } => (
                    member_change_item(change),
                    member_change_text(&notice.sender, change, target, reason.as_deref()),
                ),
                RoomStateNoticeContent::PowerLevels {
                    changes,
                    users_default,
                    topic_by_operators,
                    moderated,
                    other_permissions_changed,
                } => (
                    EventItem::Settings,
                    power_levels_change_text(
                        &notice.sender,
                        changes,
                        users_default.as_ref(),
                        *topic_by_operators,
                        *moderated,
                        *other_permissions_changed,
                    ),
                ),
                RoomStateNoticeContent::Name { name } => (
                    EventItem::Topic,
                    name_change_text(&notice.sender, name.as_deref()),
                ),
                RoomStateNoticeContent::JoinRule { rule } => (
                    EventItem::Settings,
                    join_rule_change_text(&notice.sender, rule),
                ),
                RoomStateNoticeContent::Encryption => {
                    (EventItem::Settings, encryption_change_text(&notice.sender))
                }
            };
            (
                TimelineRowKind::StateNotice {
                    highlight: activity.highlight,
                    item,
                },
                LineText::plain(event_line(timestamp, &body)),
                None,
            )
        }
        // The redacted line no longer says what made it a highlight.
        RoomActivityKind::Redacted(_) => (
            TimelineRowKind::Redacted,
            LineText::plain(event_line(timestamp, "message redacted")),
            None,
        ),
    };
    ActivityLine {
        kind,
        text: line_text.text,
        styles: Vec::new(),
        user_links: line_text.user_links,
        sender: line_text.sender,
        image_state,
    }
}

/// A room window's rows. `line_marker`: the event the line marker follows,
/// `None` for no marker (see `room_line_marker`). `rank_prefix`: Options >
/// IRC's Show mode prefix, which a channel's lines follow.
pub(super) fn build_room_timeline_rows(
    model: &UiModel,
    room_id: &str,
    timestamps: TimestampStyle,
    line_marker: Option<&str>,
    rank_prefix: bool,
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
            LineSenders {
                own_user_id: model.own_user_id(),
                rank_glyphs: shows_rank_glyphs(room, rank_prefix),
            },
            line_marker,
        );
        push_room_typing_row(&mut rows, room_view);
    }

    rows
}

/// The window's activity rows with its echo lines merged by time: an echo
/// line goes before the first activity newer than it, so it stays below
/// what the user saw when typing the command.
///
/// The line marker goes right above the first activity after the one
/// `line_marker` names, below the echo lines before it: it parts what the
/// user read from what arrived since. With no activity after the marked
/// one there is nothing unread, and no marker.
fn push_activity_rows_with_echo<'a>(
    rows: &mut Vec<TimelineRow>,
    room_view: &RoomViewModel,
    activities: impl Iterator<Item = &'a RoomActivity>,
    thread_root: Option<&str>,
    timestamps: TimestampStyle,
    senders: LineSenders,
    line_marker: Option<&str>,
) {
    let mut echo = room_view.echo.lines_for(thread_root).peekable();
    let mut marker_due = false;
    for activity in activities {
        while let Some(line) = echo.next_if(|line| line.timestamp_ms < activity.timestamp_ms) {
            rows.push(echo_row(line, timestamps));
        }
        if std::mem::take(&mut marker_due) {
            rows.push(TimelineRow::line_marker());
        }
        push_room_activity_row(
            rows,
            room_view,
            activity,
            timestamps,
            senders.own_user_id,
            senders.rank_glyphs,
        );
        marker_due = line_marker.is_some() && room_activity_event_id(activity) == line_marker;
    }
    rows.extend(echo.map(|line| echo_row(line, timestamps)));
}

/// How a window's lines show their senders: whether each is the signed-in
/// user (`None` while signed out), and with their rank's glyph.
#[derive(Clone, Copy)]
struct LineSenders<'a> {
    own_user_id: Option<&'a str>,
    rank_glyphs: bool,
}

/// Whether `room`'s lines show their senders' rank glyphs: mIRC's Show
/// mode prefix is for channel messages, so never in a query.
fn shows_rank_glyphs(room: &RoomSummary, rank_prefix: bool) -> bool {
    rank_prefix && room_window_kind(room) == RoomWindowKind::Channel
}

fn echo_row(line: &EchoLine, timestamps: TimestampStyle) -> TimelineRow {
    log_line_row(line.kind, line.timestamp_ms, &line.text, timestamps)
        .with_identity(TimelineRowIdentity::EchoLine {
            sequence: line.arrival,
        })
        .arrived(Some(line.arrival))
}

pub(super) fn build_thread_timeline_rows(
    model: &UiModel,
    room_id: &str,
    thread_root_event_id: &str,
    timestamps: TimestampStyle,
    rank_prefix: bool,
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
        LineSenders {
            own_user_id: model.own_user_id(),
            rank_glyphs: shows_rank_glyphs(room, rank_prefix),
        },
        // Matrix keeps one read marker per room, for its room window.
        None,
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
        room_line_marker(state, room_id),
        false,
    ))
}

/// The line a room window's line marker follows: where it shows the room's
/// read marker (`m.fully_read`) as the server last said
/// (`line_marker::marker_line`), while Options > Display > Line marker is
/// on.
pub(super) fn room_line_marker<'a>(state: &'a WindowState, room_id: &str) -> Option<&'a str> {
    if !state.settings.settings().marker.show {
        return None;
    }
    line_marker::marker_line(&state.model, room_id)
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

/// A Status or echo line's row: its timestamp and text, as its `kind`
/// draws them.
fn log_line_row(
    kind: StatusLineKind,
    timestamp_ms: u64,
    text: &str,
    timestamps: TimestampStyle,
) -> TimelineRow {
    let row_kind = match kind {
        StatusLineKind::Info => TimelineRowKind::Info,
        StatusLineKind::Error => TimelineRowKind::Error,
        StatusLineKind::Notify => TimelineRowKind::Event {
            item: EventItem::Notify,
        },
        StatusLineKind::Invite => TimelineRowKind::Event {
            item: EventItem::Invite,
        },
        StatusLineKind::Echo { color, timestamp } => {
            let prefix = if timestamp {
                timestamps.prefix(timestamp_ms)
            } else {
                String::new()
            };
            return TimelineRow::log(TimelineRowKind::Echo { color }, format!("{prefix}{text}"));
        }
    };
    let prefix = timestamps.prefix(timestamp_ms);
    TimelineRow::log(row_kind, format!("{prefix}{text}"))
}

pub(super) fn build_status_timeline_rows(
    status_log: &StatusLog,
    timestamps: TimestampStyle,
) -> Vec<TimelineRow> {
    status_log
        .lines()
        .iter()
        .map(|line| {
            log_line_row(line.kind, line.timestamp_ms, &line.text, timestamps)
                .with_identity(TimelineRowIdentity::status_line(line.sequence))
                .arrived(Some(line.sequence))
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
/// Whether a refresh may ask the core for events (reply targets): only
/// for a session, and not while a synthetic load replays its events.
pub(super) fn can_fetch_room_events(state: &WindowState) -> bool {
    !state.harness.synthetic_load()
        && matches!(
            state.model.connection,
            ConnectionState::SignedIn
                | ConnectionState::WaitingToReconnect
                | ConnectionState::Syncing
        )
}

pub(super) fn dispatch_reply_target_fetches(
    state: &mut WindowState,
    room_id: &str,
    event_ids: BTreeSet<String>,
) -> Result<(), String> {
    if !can_fetch_room_events(state) {
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

    fn line_of(room_view: &RoomViewModel, activity: &RoomActivity) -> (TimelineRowKind, String) {
        let line = activity_line_text(room_view, activity, "[12:00] ", None, false);
        (line.kind, line.text)
    }

    #[test]
    fn a_verification_request_links_whom_it_asks() {
        let request =
            verification_request_activity("!room:example", "$verify", 1, "@carol:example");
        let line = activity_line_text(&RoomViewModel::default(), &request, "[12:00] ", None, false);
        assert_eq!(
            line.kind,
            TimelineRowKind::VerificationRequest { highlight: false }
        );
        assert_eq!(
            line.text,
            "[12:00] * @bob:example wants to verify @carol:example"
        );
        let linked = line
            .user_links
            .iter()
            .map(|link| (&line.text[link.range.clone()], link.user_id.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            linked,
            [
                ("@bob:example", "@bob:example"),
                ("@carol:example", "@carol:example")
            ]
        );
    }

    #[test]
    fn polls_start_with_their_answers_and_end_with_their_question() {
        let mut room_view = RoomViewModel::default();
        let start = poll_activity(
            "!room:example",
            "$poll",
            1,
            poll_start("Lunch?", &["Pizza", "Sushi"]),
        );
        let end = poll_activity("!room:example", "$end", 2, poll_end("$poll", "Pizza won"));
        let poll_row = TimelineRowKind::Poll { highlight: false };
        // The start not loaded: the end cannot name it.
        room_view.apply_activity(end.clone());
        assert_eq!(
            line_of(&room_view, &end),
            (
                poll_row.clone(),
                "[12:00] * @bob:example ended a poll: Pizza won".to_owned()
            )
        );
        room_view.apply_activity(start.clone());
        assert_eq!(
            line_of(&room_view, &start),
            (
                poll_row.clone(),
                "[12:00] * @bob:example started a poll: Lunch? 1) Pizza 2) Sushi".to_owned()
            )
        );
        assert_eq!(
            line_of(&room_view, &end).1,
            "[12:00] * @bob:example ended the poll \"Lunch?\": Pizza won"
        );

        let open_choice = poll_activity(
            "!room:example",
            "$open",
            3,
            RoomPollContent::Start {
                question: "Days?".to_owned(),
                answers: vec!["Mon".to_owned(), "Tue".to_owned(), "Wed".to_owned()],
                max_selections: 2,
                results_hidden: true,
            },
        );
        assert_eq!(
            line_of(&room_view, &open_choice).1,
            "[12:00] * @bob:example started a poll: Days? 1) Mon 2) Tue 3) Wed (up to 2 answers) \
             (results when it ends)"
        );
        assert_eq!(
            line_of(&room_view, &highlighted(start)).0,
            TimelineRowKind::Poll { highlight: true }
        );
    }

    /// A message waiting for the connection is drawn and logged as one on
    /// its way: dim, and written once it is sent.
    #[test]
    fn only_a_refused_message_is_a_failed_row() {
        let failed = |recoverable| RoomMessageSendState::Failed {
            message: "failed".to_owned(),
            recoverable,
        };
        assert_eq!(
            timeline_message_send_state(&failed(true)),
            TimelineMessageSendState::Pending
        );
        assert_eq!(
            timeline_message_send_state(&failed(false)),
            TimelineMessageSendState::Failed
        );
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
            None,
            false,
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
            None,
            false,
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
            None,
            false,
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
                        nick: display_name.map(str::to_owned),
                        nick_ambiguous: false,
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
            None,
            false,
        ))
    }

    #[test]
    fn a_reply_quotes_its_loaded_target_above_it_with_the_replys_event() {
        let state = reply_quote_state("$q");
        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );
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
            Event::RoomActivityUpdate(edit_by(
                "!ops:example",
                "$a",
                "$a-edit",
                "@bob:example",
                1_700_000_000_500,
                RoomMessagePresentationKind::Emote,
                "says yes",
            )),
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
        assert!(state.model.room_views["!ops:example"].line_edits.is_empty());
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
        assert!(state.harness.commands.is_empty(), "no session to ask");

        state.model.connection = ConnectionState::Syncing;
        fetch_reply_targets(&mut state);
        fetch_reply_targets(&mut state);
        assert_eq!(
            state.harness.commands,
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
                updates: Vec::new(),
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
            false,
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

        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event {
                    item: EventItem::Join,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                    own: false,
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

        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );
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
            ("$r3", "@dave:example", 1_700_000_000_100),
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
            build_room_timeline_rows(
                &state.model,
                "!ops:example",
                state.timestamps(),
                None,
                false,
            )
            .into_iter()
            .find(|row| row.kind == TimelineRowKind::ReactionSummary)
            .expect("the message has a reaction row")
        };
        assert_eq!(
            reaction_row(&state).logical_text,
            "    reactions: +1 @dave:example, @ghost:example, @alice:example"
        );

        // Members arrive after the reactions; the row is rebuilt from them.
        apply_core_event(
            &mut state,
            Event::RoomMembers(RoomMembers {
                room_id: "!ops:example".to_owned(),
                members: vec![
                    member_summary("@alice:example", "Alice", false),
                    // Without a display name, the core names ghost by
                    // localpart.
                    member_summary("@ghost:example", "ghost", false),
                ],
            }),
        )
        .unwrap();
        let row = reaction_row(&state);
        // Dave is no member: the full user ID.
        assert_eq!(
            row.logical_text,
            "    reactions: +1 @dave:example, ghost, Alice"
        );
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
            vec![ReactionSummary {
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
            vec![ReactionSummary {
                key: "+1".to_owned(),
                count: 2,
                senders: vec!["@bob:example.org".to_owned(), "@bob:example.org".to_owned(),],
                reaction_event_ids: vec!["$reaction-1".to_owned(), "$reaction-2".to_owned()],
            }]
        );
    }

    #[test]
    fn a_reaction_redaction_removes_only_that_reaction() {
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
            incoming_redacted("!ops:example", "$reaction-unknown", "$redaction-unknown"),
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
            incoming_redacted("!ops:example", "$reaction-1", "$redaction-right"),
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

        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event {
                    item: EventItem::Join,
                },
                TimelineRowKind::Redacted
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
            false,
        );
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event {
                    item: EventItem::Join,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                    own: false,
                },
                TimelineRowKind::ReactionSummary,
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                    own: false,
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

        let rows =
            build_room_timeline_rows(&state.model, "!ops:example", utc_timestamps(), None, false)
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

    /// Each user link's text and user ID.
    fn row_user_links(row: &TimelineRow) -> Vec<(&str, &str)> {
        row.user_links
            .iter()
            .map(|link| (&row.logical_text[link.range.clone()], link.user_id.as_str()))
            .collect()
    }

    #[test]
    fn message_rows_link_their_sender_and_pills_past_the_nick() {
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
                "ask Bob",
                1_700_000_000_000,
            );
            let RoomActivityKind::Message(message) = &mut activity.kind else {
                unreachable!("a message activity");
            };
            message.user_links = vec![UserLinkSpan {
                start: 4,
                end: 7,
                user_id: "@bob:example".to_owned(),
            }];
            apply_core_event(&mut state, Event::RoomActivity(Box::new(activity))).unwrap();
        }
        let Event::RoomActivityUpdate(mut edit) = incoming_replaced(
            "!ops:example",
            "$text",
            "$edit",
            RoomMessagePresentationKind::Text,
            "Carol, then",
        ) else {
            unreachable!("a replacement");
        };
        let RoomActivityUpdateKind::MessageReplaced { user_links, .. } = &mut edit.kind else {
            unreachable!("a replacement");
        };
        *user_links = vec![UserLinkSpan {
            start: 0,
            end: 5,
            user_id: "@carol:example".to_owned(),
        }];
        apply_core_event(&mut state, Event::RoomActivityUpdate(edit)).unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(image_activity(
                "!ops:example",
                "$image",
                "@dave:example",
                1_700_000_000_000,
                "diagram.png",
                None,
                None,
            ))),
        )
        .unwrap();

        let rows =
            build_room_timeline_rows(&state.model, "!ops:example", utc_timestamps(), None, false)
                .into_iter()
                .filter(|row| row.event_id.is_some())
                .collect::<Vec<_>>();
        assert_eq!(
            rows.iter()
                .map(|row| (row.logical_text.as_str(), row_user_links(row)))
                .collect::<Vec<_>>(),
            vec![
                (
                    "[22:13] <@alice:example> Carol, then (edited)",
                    vec![
                        ("@alice:example", "@alice:example"),
                        ("Carol", "@carol:example")
                    ]
                ),
                (
                    "[22:13] * @alice:example ask Bob",
                    vec![
                        ("@alice:example", "@alice:example"),
                        ("Bob", "@bob:example")
                    ]
                ),
                (
                    "[22:13] -@alice:example- ask Bob",
                    vec![
                        ("@alice:example", "@alice:example"),
                        ("Bob", "@bob:example")
                    ]
                ),
                (
                    "[22:13] * @dave:example sent image: diagram.png",
                    vec![("@dave:example", "@dave:example")]
                ),
            ]
        );
    }

    #[test]
    fn reply_quotes_and_other_rows_have_no_user_links() {
        let mut state = reply_quote_state("$q");
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        let rows =
            build_room_timeline_rows(&state.model, "!ops:example", utc_timestamps(), None, false);
        let linked = rows
            .iter()
            .filter(|row| !row.user_links.is_empty())
            .map(|row| row.logical_text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            linked,
            vec![
                "[22:13] <@alice:example> are we deploying today?\nsecond line",
                "[22:14] <@bob:example> yes, after lunch",
            ],
            "{rows:#?}"
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

        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event {
                    item: EventItem::Join,
                },
                TimelineRowKind::Event {
                    item: EventItem::Topic,
                },
                TimelineRowKind::Info,
                TimelineRowKind::StateNotice {
                    highlight: false,
                    item: EventItem::Topic,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                    own: false,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Notice,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                    own: false,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Emote,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                    own: false,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Pending,
                    highlight: false,
                    own: false,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Failed,
                    highlight: false,
                    own: false,
                },
                TimelineRowKind::Image { highlight: false },
                TimelineRowKind::File { highlight: false },
                TimelineRowKind::Audio { highlight: false },
                TimelineRowKind::Unsupported { highlight: false },
                TimelineRowKind::StateNotice {
                    highlight: false,
                    item: EventItem::Join,
                },
                TimelineRowKind::StateNotice {
                    highlight: false,
                    item: EventItem::Part,
                },
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
                    member_summary("@alice:example", "Alice", false),
                    member_summary("@sam1:example", "Sam", true),
                    member_summary("@ghost:example", "ghost", false),
                ],
            }),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            typing_event(
                "!ops:example",
                &[
                    "@alice:example",
                    "@sam1:example",
                    "@ghost:example",
                    "@stranger:example",
                ],
            ),
        )
        .unwrap();

        // A stranger is no member: the full user ID.
        assert!(render_room_lines(&state, "!ops:example").contains(
            &"* Alice, Sam (@sam1:example), ghost, @stranger:example are typing".to_owned()
        ));
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
            nick: Some("Sam".to_owned()),
            nick_ambiguous: true,
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
            .history = RoomHistoryBackfillState::Loading {
            from: HistoryFrom::Latest,
        };

        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );

        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event {
                    item: EventItem::Join,
                },
                TimelineRowKind::Event {
                    item: EventItem::Topic,
                },
                TimelineRowKind::Plain,
                TimelineRowKind::StateNotice {
                    highlight: false,
                    item: EventItem::Topic,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                    own: false,
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
            .history = RoomHistoryBackfillState::Failed {
            from: HistoryFrom::Token {
                token: "token-1".to_owned(),
            },
            message: "homeserver unavailable".to_owned(),
        };

        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );

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
            .history = RoomHistoryBackfillState::Exhausted;

        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );

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

        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );

        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event {
                    item: EventItem::Join,
                },
                TimelineRowKind::Event {
                    item: EventItem::Topic,
                },
                TimelineRowKind::StateNotice {
                    highlight: false,
                    item: EventItem::Topic,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                    own: false,
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

        let rows = build_room_timeline_rows(
            &state.model,
            "!direct:example",
            state.timestamps(),
            None,
            false,
        );
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event {
                    item: EventItem::Join,
                },
                TimelineRowKind::Info,
            ]
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
    fn notify_status_lines_are_event_rows_in_the_notify_colour() {
        let mut status_log = StatusLog::default();
        status_log.push(0, StatusLineKind::Notify, "* Bob is on Matrix".to_owned());
        let rows = build_status_timeline_rows(&status_log, test_state().timestamps());
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            [TimelineRowKind::Event {
                item: EventItem::Notify,
            }]
        );
        assert_eq!(EventItem::Notify.color_item(), ColorItem::Notify);
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

    /// Each cause notes why, and what to do when something can be done.
    #[test]
    fn a_line_that_could_not_be_decrypted_says_why() {
        use mirx_core::RoomUnableToDecryptCause as Cause;
        let causes = [
            (Cause::Unknown, ""),
            (Cause::SentBeforeYouJoined, " (sent before you joined)"),
            (
                Cause::HistoricalNoBackup,
                " (sent before this session existed, and you have no key backup)",
            ),
            (
                Cause::HistoricalUnverified,
                " (sent before this session existed: /verify this session or /e2ee recover to read it)",
            ),
            (
                Cause::WithheldUnverified,
                " (the sender shares keys only with verified sessions: /verify this session)",
            ),
            (Cause::WithheldBySender, " (the sender withheld its key)"),
            (
                Cause::SenderIdentityChanged,
                " (the sender's verified identity has changed)",
            ),
            (
                Cause::UnsignedDevice,
                " (sent from a session its owner has not verified)",
            ),
            (Cause::UnknownDevice, " (sent from an unknown session)"),
        ];
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", true)];
        for (index, (cause, _)) in causes.iter().enumerate() {
            let mut event = unable_to_decrypt_activity(
                "!ops:example",
                &format!("$encrypted{index}:example.org"),
                "@alice:example.org",
                1_710_000_000_000,
            );
            let Event::RoomActivity(activity) = &mut event else {
                unreachable!("a room activity");
            };
            let RoomActivityKind::UnableToDecrypt(utd) = &mut activity.kind else {
                unreachable!("a UTD");
            };
            utd.cause = *cause;
            apply_core_event(&mut state, event).unwrap();
        }

        assert_eq!(
            render_room_lines(&state, "!ops:example")[2..],
            causes
                .iter()
                .map(|(_, note)| format!(
                    "[16:00] * @alice:example.org sent an encrypted message that could not be decrypted{note}"
                ))
                .collect::<Vec<_>>()
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

        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );
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

        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );
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
            build_room_timeline_rows(
                &state.model,
                "!ops:example",
                state.timestamps(),
                None,
                false
            )
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

        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );
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

        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );
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
            false,
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
            false,
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
            false,
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
            false,
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
            false,
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
            false,
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
            false,
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
            None,
            false,
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
            None,
            false,
        );

        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: true,
                    own: false,
                },
                TimelineRowKind::Redacted,
            ]
        );
    }

    #[test]
    fn room_settings_changes_are_topic_and_mode_lines() {
        let notice = |content: RoomStateNoticeContent| RoomActivity {
            room_id: "!room:example".to_owned(),
            activity_id: "state:$s".to_owned(),
            timestamp_ms: 1_700_000_120_000,
            highlight: false,
            relation: None,
            kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
                event_id: "$s".to_owned(),
                sender: RoomUserProfile {
                    user_id: "@bob:example".to_owned(),
                    nick: Some("Bob".to_owned()),
                    nick_ambiguous: false,
                },
                content,
            }),
        };
        for (content, item, text) in [
            (
                RoomStateNoticeContent::Name {
                    name: Some("Ops".to_owned()),
                },
                EventItem::Topic,
                "* Bob changes room name to 'Ops'",
            ),
            (
                RoomStateNoticeContent::Name { name: None },
                EventItem::Topic,
                "* Bob removes room name",
            ),
            (
                RoomStateNoticeContent::JoinRule {
                    rule: mirx_core::RoomJoinRule::Invite,
                },
                EventItem::Settings,
                "* Bob makes the room invite only",
            ),
            (
                RoomStateNoticeContent::Encryption,
                EventItem::Settings,
                "* Bob turns on end-to-end encryption",
            ),
        ] {
            let mut rows = Vec::new();
            push_room_activity_row(
                &mut rows,
                &RoomViewModel::default(),
                &notice(content),
                utc_timestamps(),
                None,
                false,
            );
            assert_eq!(rows.len(), 1);
            assert_eq!(
                rows[0].kind,
                TimelineRowKind::StateNotice {
                    highlight: false,
                    item
                }
            );
            assert_eq!(rows[0].logical_text, format!("[22:15] {text}"));
        }
    }

    #[test]
    fn member_changes_take_mircs_event_items() {
        use RoomMemberChange::*;
        for (change, item) in [
            (Joined, EventItem::Join),
            (InvitationAccepted, EventItem::Join),
            (Left, EventItem::Part),
            (InvitationRejected, EventItem::Part),
            (KnockRetracted, EventItem::Part),
            (Kicked, EventItem::Kick),
            (KickedAndBanned, EventItem::Kick),
            (Banned, EventItem::Settings),
            (Unbanned, EventItem::Settings),
            (Invited, EventItem::Invite),
            (InvitationRevoked, EventItem::Invite),
            (Knocked, EventItem::Invite),
            (KnockAccepted, EventItem::Invite),
            (KnockDenied, EventItem::Invite),
            (
                NickChanged {
                    old: "carol".to_owned(),
                    new: "Caz".to_owned(),
                },
                EventItem::Nick,
            ),
            (AvatarChanged, EventItem::Nick),
        ] {
            assert_eq!(member_change_item(&change), item, "{change:?}");
        }
    }

    #[test]
    fn only_the_signed_in_users_messages_are_own() {
        let mut rows = Vec::new();
        let message = sent_message_activity("!room:example", "$m", None, "hi", 1);
        for own_user_id in [Some("@alice:example"), Some("@bob:example"), None] {
            push_room_activity_row(
                &mut rows,
                &RoomViewModel::default(),
                &message,
                utc_timestamps(),
                own_user_id,
                false,
            );
        }
        let own = rows
            .iter()
            .map(|row| match row.kind {
                TimelineRowKind::Message { own, .. } => own,
                ref kind => panic!("a message row, not {kind:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(own, [true, false, false]);
    }

    #[test]
    fn an_invites_lines_take_invite_text() {
        let rows = build_invite_timeline_rows(&invited_room("!inv:example", "Invited"));
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event {
                    item: EventItem::Invite
                };
                2
            ]
        );
    }

    #[test]
    fn an_activity_row_carries_its_arrival_and_history_none() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, timeline("!ops:example", "live")).unwrap();
        let room_view = state.model.room_views.get_mut("!ops:example").unwrap();
        room_view.apply_history_backfill(
            "!ops:example",
            RoomHistoryBackfill {
                room_id: "!ops:example".to_owned(),
                end: None,
                events: vec![RoomHistoryBackfillEvent::Activity(Box::new(
                    sent_message_activity("!ops:example", "$old", None, "old", 1_600_000_000_000),
                ))],
            },
        );
        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );
        let arrivals = rows
            .iter()
            .filter_map(|row| match &row.identity {
                TimelineRowIdentity::ActivityPrimary { activity_id } => {
                    Some((activity_id.as_str(), row.arrival))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            arrivals,
            [("event:$old", None), ("event:$!ops:example:live", Some(0))]
        );
    }

    /// Ops: Alice's `$a`, a reply `$b` to it with a reaction, an echo line,
    /// a thread reply `$t` to `$a`, and `$c`.
    fn line_marker_state() -> WindowState {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        let events = vec![
            timeline_with_transaction_id("!ops:example", "$a", None, "first", 1_700_000_060_000),
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$b",
                None,
                Some(reply_relation("$a")),
                RoomMessagePresentationKind::Text,
                "second",
                1_700_000_120_000,
            ),
            incoming_reaction_added(
                "!ops:example",
                "$b",
                "$reaction",
                "@bob:example.org",
                "heart",
                1_700_000_121_000,
            ),
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$t",
                None,
                Some(thread_relation("$a")),
                RoomMessagePresentationKind::Text,
                "in thread",
                1_700_000_170_000,
            ),
            timeline_with_transaction_id("!ops:example", "$c", None, "third", 1_700_000_180_000),
        ];
        for event in events {
            apply_core_event(&mut state, event).unwrap();
        }
        state
            .model
            .room_views
            .get_mut("!ops:example")
            .unwrap()
            .push_echo(
                1_700_000_150_000,
                None,
                StatusLineKind::Info,
                "* between".to_owned(),
            );
        state
    }

    /// The rows' texts after the opening lines, the marker as `----`.
    fn marked_texts(rows: &[TimelineRow]) -> Vec<String> {
        rows.iter()
            .skip_while(|row| row.event_id.is_none())
            .map(|row| match row.kind {
                TimelineRowKind::LineMarker => "----".to_owned(),
                _ => row.logical_text.clone(),
            })
            .collect()
    }

    fn rows_marked_at(state: &WindowState, line_marker: Option<&str>) -> Vec<TimelineRow> {
        build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            line_marker,
            false,
        )
    }

    #[test]
    fn the_line_marker_parts_the_marked_line_from_the_next_activity() {
        let state = line_marker_state();
        assert_eq!(
            marked_texts(&rows_marked_at(&state, Some("$b"))),
            [
                "[22:14] <@alice:example> first",
                "    > <@alice:example> first",
                "[22:15] <@alice:example> second",
                "    reactions: heart @bob:example.org",
                "[22:15] * between",
                "----",
                "[22:16] <@alice:example> in thread",
                "[22:16] <@alice:example> third",
            ]
        );
        // A thread reply is a line of the room window too.
        assert_eq!(
            marked_texts(&rows_marked_at(&state, Some("$t")))[5..],
            [
                "[22:16] <@alice:example> in thread",
                "----",
                "[22:16] <@alice:example> third",
            ]
        );
        let marker = rows_marked_at(&state, Some("$a"))
            .into_iter()
            .find(|row| row.kind == TimelineRowKind::LineMarker)
            .expect("a line follows $a");
        assert_eq!(marker, TimelineRow::line_marker());
        assert_eq!(
            marker.identity,
            TimelineRowIdentity::synthetic(TimelineSyntheticRowLabel::LineMarker, "")
        );
        assert_eq!((marker.event_id, marker.arrival), (None, None));
    }

    #[test]
    fn no_line_marker_without_a_line_after_it_or_a_marked_line_shown() {
        let mut state = line_marker_state();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@bob:example.org"]),
        )
        .unwrap();
        let unmarked = rows_marked_at(&state, None);
        assert!(
            unmarked
                .iter()
                .all(|row| row.kind != TimelineRowKind::LineMarker)
        );
        // The last line is read: below it come only the typing line.
        assert_eq!(rows_marked_at(&state, Some("$c")), unmarked);
        // An event the window does not show: not loaded, or no line.
        assert_eq!(rows_marked_at(&state, Some("$reaction")), unmarked);
        assert_eq!(rows_marked_at(&state, Some("$unknown")), unmarked);
    }

    #[test]
    fn thread_windows_have_no_line_marker() {
        let mut state = line_marker_state();
        state.model.room_read_states.insert(
            "!ops:example".to_owned(),
            mirx_core::RoomReadState {
                room_id: "!ops:example".to_owned(),
                latest_active_event_id: None,
                fully_read_event_id: Some("$a".to_owned()),
                fully_read_timestamp_ms: None,
                marked_unread: false,
                num_unread: 0,
                num_notifications: 0,
                num_mentions: 0,
            },
        );
        assert_eq!(room_line_marker(&state, "!ops:example"), Some("$a"));
        let rows = build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$a",
            state.timestamps(),
            false,
        );
        assert!(
            rows.iter()
                .all(|row| row.kind != TimelineRowKind::LineMarker)
        );
    }

    #[test]
    fn the_line_marker_is_the_read_marker_while_it_is_shown() {
        let mut state = line_marker_state();
        assert_eq!(
            room_line_marker(&state, "!ops:example"),
            None,
            "no read state"
        );
        let read_state = |fully_read: Option<&str>| mirx_core::RoomReadState {
            room_id: "!ops:example".to_owned(),
            latest_active_event_id: Some("$c".to_owned()),
            fully_read_event_id: fully_read.map(str::to_owned),
            fully_read_timestamp_ms: None,
            marked_unread: false,
            num_unread: 2,
            num_notifications: 0,
            num_mentions: 0,
        };
        state
            .model
            .room_read_states
            .insert("!ops:example".to_owned(), read_state(None));
        assert_eq!(room_line_marker(&state, "!ops:example"), None);
        state
            .model
            .room_read_states
            .insert("!ops:example".to_owned(), read_state(Some("$b")));
        assert_eq!(room_line_marker(&state, "!ops:example"), Some("$b"));
        assert!(
            render_room_lines(&state, "!ops:example").contains(&String::new()),
            "the marker projects as an empty line"
        );
        state
            .settings
            .update(|settings| settings.marker.show = false)
            .unwrap();
        assert_eq!(room_line_marker(&state, "!ops:example"), None);
        assert_eq!(room_line_marker(&state, "!other:example"), None);
    }

    #[test]
    fn a_line_marker_on_an_unshown_event_goes_below_the_line_before_it() {
        let mut state = line_marker_state();
        // Bob's reaction to `$b`, which shows on `$b`'s reactions line.
        state.model.room_read_states.insert(
            "!ops:example".to_owned(),
            mirx_core::RoomReadState {
                room_id: "!ops:example".to_owned(),
                latest_active_event_id: None,
                fully_read_event_id: Some("$reaction".to_owned()),
                fully_read_timestamp_ms: Some(1_700_000_121_000),
                marked_unread: false,
                num_unread: 2,
                num_notifications: 0,
                num_mentions: 0,
            },
        );
        assert_eq!(room_line_marker(&state, "!ops:example"), Some("$b"));
        assert_eq!(
            marked_texts(&rows_marked_at(
                &state,
                room_line_marker(&state, "!ops:example")
            ))[3..],
            [
                "    reactions: heart @bob:example.org",
                "[22:15] * between",
                "----",
                "[22:16] <@alice:example> in thread",
                "[22:16] <@alice:example> third",
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
        let echo = state.model.room_views.get_mut("!ops:example").unwrap();
        echo.push_echo(
            1_700_000_000_000,
            None,
            StatusLineKind::Info,
            "* before".to_owned(),
        );
        echo.push_echo(
            1_700_000_120_000,
            None,
            StatusLineKind::Info,
            "* between".to_owned(),
        );
        // A tie stays after the activity the user already saw.
        echo.push_echo(
            1_700_000_180_000,
            None,
            StatusLineKind::Info,
            "* tie".to_owned(),
        );
        echo.push_echo(
            1_700_000_240_000,
            None,
            StatusLineKind::Info,
            "* tie".to_owned(),
        );

        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );
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
                // The two messages arrived first.
                TimelineRowIdentity::EchoLine { sequence: 4 },
                TimelineRowIdentity::EchoLine { sequence: 5 },
            ]
        );
    }

    #[test]
    fn error_echo_lines_render_red_and_info_lines_stay_info() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        let room_view = state
            .model
            .room_views
            .entry_or_default("!ops:example".to_owned());
        room_view.push_echo(
            1_700_000_000_000,
            None,
            StatusLineKind::Info,
            "* info".to_owned(),
        );
        room_view.push_echo(
            1_700_000_000_000,
            None,
            StatusLineKind::Error,
            "* warning".to_owned(),
        );

        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );
        let kinds = rows[rows.len() - 2..]
            .iter()
            .map(|row| row.kind.clone())
            .collect::<Vec<_>>();
        assert_eq!(kinds, vec![TimelineRowKind::Info, TimelineRowKind::Error]);
    }

    #[test]
    fn echo_lines_are_plain_text_with_a_stamp_only_when_asked() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        let red = PaletteIndex::new(4).unwrap();
        let room_view = state
            .model
            .room_views
            .entry_or_default("!ops:example".to_owned());
        let echo = |color, timestamp| StatusLineKind::Echo { color, timestamp };
        room_view.push_echo(0, None, echo(None, false), "plain".to_owned());
        room_view.push_echo(
            1_700_000_000_000,
            None,
            echo(Some(red), true),
            "\x02**bold**\x02 and \x0312blue".to_owned(),
        );

        let rows = build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        );
        let [plain, stamped] = &rows[rows.len() - 2..] else {
            unreachable!("two echo lines")
        };
        assert_eq!(plain.logical_text, "plain");
        assert_eq!(plain.kind, TimelineRowKind::Echo { color: None });
        assert_eq!(plain.styles, []);
        // Neither control codes nor Markdown format an echo.
        assert_eq!(
            stamped.logical_text,
            "[22:13] \x02**bold**\x02 and \x0312blue"
        );
        assert_eq!(stamped.kind, TimelineRowKind::Echo { color: Some(red) });
        assert_eq!(stamped.styles, []);

        // Status rows are built the same way.
        let mut log = StatusLog::default();
        log.push(0, echo(None, false), "\x1fs\x1f".to_owned());
        let rows = build_status_timeline_rows(&log, state.timestamps());
        assert_eq!(rows[0].logical_text, "\x1fs\x1f");
        assert_eq!(rows[0].styles, []);
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
        room_view.push_echo(
            1_700_000_200_000,
            Some("$thread-root".to_owned()),
            StatusLineKind::Info,
            "* /clear: nothing to clear in thread view".to_owned(),
        );
        room_view.push_echo(
            1_700_000_200_000,
            None,
            StatusLineKind::Info,
            "* room line".to_owned(),
        );

        let thread_lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
            state.timestamps(),
            false,
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
            false,
        ));
        assert!(
            thread_lines.contains(&"[22:16] * /clear: nothing to clear in thread view".to_owned())
        );
    }

    /// Options > Chat's Show rank prefix: a channel's message, action and
    /// notice lines put the sender's rank glyph before the nick, outside
    /// the nick's range; queries never do, and off, nothing does.
    #[test]
    fn show_rank_prefix_puts_the_senders_rank_before_the_nick_in_rooms() {
        use mirx_core::RoomMemberRank;
        let mut state = test_state();
        state.model.rooms = vec![
            room("!ops:example", "Ops", false),
            query("!dm:example", "Mod", "@mod:example"),
        ];
        let ranked = |user_id: &str, nick: &str, rank| RoomMemberSummary {
            rank,
            ..member_summary(user_id, nick, false)
        };
        let members = vec![
            ranked("@owner:example", "Owner", RoomMemberRank::Creator),
            ranked("@mod:example", "Mod", RoomMemberRank::Moderator),
            ranked("@voice:example", "Voice", RoomMemberRank::Voiced),
            ranked("@sam:example", "Sam", RoomMemberRank::Member),
        ];
        for (second, (sender, presentation, body)) in [
            ("@owner:example", RoomMessagePresentationKind::Text, "hi"),
            ("@mod:example", RoomMessagePresentationKind::Emote, "waves"),
            (
                "@voice:example",
                RoomMessagePresentationKind::Notice,
                "note",
            ),
            ("@sam:example", RoomMessagePresentationKind::Text, "yo"),
        ]
        .into_iter()
        .enumerate()
        {
            for room_id in ["!ops:example", "!dm:example"] {
                let mut activity = sent_message_activity_with_presentation(
                    room_id,
                    &format!("${second}"),
                    None,
                    presentation,
                    body,
                    1_700_000_000_000 + second as u64 * 1000,
                );
                let RoomActivityKind::Message(message) = &mut activity.kind else {
                    unreachable!("a message activity");
                };
                message.sender = RoomUserProfile {
                    user_id: sender.to_owned(),
                    nick: members
                        .iter()
                        .find(|member| member.user_id == sender)
                        .map(|member| member.nick.clone()),
                    nick_ambiguous: false,
                };
                apply_core_event(&mut state, Event::RoomActivity(Box::new(activity))).unwrap();
            }
        }
        for room_id in ["!ops:example", "!dm:example"] {
            state.model.room_views.entry_or_default(room_id).members = members.clone();
        }
        let lines = |state: &WindowState, room_id: &str, rank_prefix: bool| {
            build_room_timeline_rows(&state.model, room_id, utc_timestamps(), None, rank_prefix)
                .into_iter()
                .filter_map(|row| {
                    let sender = row.sender?;
                    Some((
                        row.logical_text.clone(),
                        row.logical_text[sender.range].to_owned(),
                    ))
                })
                .collect::<Vec<_>>()
        };
        let line = |text: &str, nick: &str| (text.to_owned(), nick.to_owned());
        assert_eq!(
            lines(&state, "!ops:example", true),
            [
                line("[22:13] <~Owner> hi", "Owner"),
                line("[22:13] * @Mod waves", "Mod"),
                line("[22:13] -+Voice- note", "Voice"),
                line("[22:13] <Sam> yo", "Sam"),
            ]
        );
        let plain = [
            line("[22:13] <Owner> hi", "Owner"),
            line("[22:13] * Mod waves", "Mod"),
            line("[22:13] -Voice- note", "Voice"),
            line("[22:13] <Sam> yo", "Sam"),
        ];
        assert_eq!(lines(&state, "!ops:example", false), plain);
        assert_eq!(lines(&state, "!dm:example", true), plain);

        // The rank now, not then: an op voiced shows `+` on old lines too.
        state
            .model
            .room_views
            .get_mut("!ops:example")
            .unwrap()
            .members[1]
            .rank = RoomMemberRank::Voiced;
        assert_eq!(
            lines(&state, "!ops:example", true)[1],
            line("[22:13] * +Mod waves", "Mod")
        );
    }

    #[test]
    fn lines_that_start_with_their_senders_nick_carry_it_and_take_its_colour() {
        use super::super::nick_colors::{NickColorEntry, NickColorer};
        use super::super::settings::Settings;
        use mirx_core::RoomMemberRank;
        let room_id = "!ops:example";
        let mut state = test_state();
        state.model.rooms = vec![room(room_id, "Ops", false)];
        let at = |second: u64| 1_700_000_000_000 + second * 1000;
        let message = |event_id, presentation, body, second| {
            sent_message_activity_with_presentation(
                room_id,
                event_id,
                None,
                presentation,
                body,
                at(second),
            )
        };
        let bob = "@bob:example";
        let mut reply = message("$reply", RoomMessagePresentationKind::Text, "yes", 9);
        reply.relation = Some(reply_relation("$text"));
        for activity in [
            message("$text", RoomMessagePresentationKind::Text, "hello", 0),
            message("$emote", RoomMessagePresentationKind::Emote, "waves", 1),
            message("$notice", RoomMessagePresentationKind::Notice, "note", 2),
            image_activity(room_id, "$image", bob, at(3), "diagram.png", None, None),
            sticker_activity(room_id, "$sticker", bob, at(4), "a cat"),
            file_activity(room_id, "$file", bob, at(5), "report.pdf", None, None),
            poll_activity(room_id, "$poll", at(6), poll_start("Lunch?", &["Pizza"])),
            verification_request_activity(room_id, "$verify", at(7), "@carol:example"),
            unsupported_message_activity(room_id, "$where", bob, at(8), "m.location", "geo"),
            reply,
        ] {
            apply_core_event(&mut state, Event::RoomActivity(Box::new(activity))).unwrap();
        }
        apply_core_event(
            &mut state,
            unable_to_decrypt_activity(room_id, "$utd", bob, at(10)),
        )
        .unwrap();
        apply_core_event(&mut state, topic_state(room_id, "incident")).unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(room_id, "$text", "$thumbs", bob, "+1", at(11)),
        )
        .unwrap();
        state.model.room_views.entry_or_default(room_id).members = vec![RoomMemberSummary {
            rank: RoomMemberRank::Moderator,
            ..member_summary("@alice:example", "Alice", false)
        }];
        let rows = build_room_timeline_rows(&state.model, room_id, utc_timestamps(), None, false);
        let senders = rows
            .iter()
            .map(|row| {
                row.sender.as_ref().map(|sender| {
                    assert_eq!(sender.color, None);
                    (
                        &row.logical_text[sender.range.clone()],
                        sender.user_id.as_str(),
                    )
                })
            })
            .collect::<Vec<_>>();
        let alice = Some(("@alice:example", "@alice:example"));
        let bob_nick = Some((bob, bob));
        let expected = [
            // Now talking, Topic is; the text and its reactions.
            vec![None, None, alice, None],
            // The emote, the notice, Bob's six.
            vec![
                alice, alice, bob_nick, bob_nick, bob_nick, bob_nick, bob_nick, bob_nick,
            ],
            // The reply's quote, then the reply.
            vec![None, alice],
            // The undecryptable line, the topic change.
            vec![bob_nick, None],
        ]
        .concat();
        assert_eq!(senders, expected, "{:#?}", project_timeline_rows(&rows));

        let mut settings = Settings::default();
        for value in ["* 12 ranks=@", "@bob:example 4"] {
            settings
                .nick_colors
                .list
                .push(NickColorEntry::parse_ini_value(value).unwrap());
        }
        let mut colored = rows.clone();
        color_row_senders(
            &mut colored,
            &NickColorer::new(&settings, &state.model.room_views[room_id].members),
        );
        let palette = |index| {
            Some(ResolvedNickColor::Palette(
                PaletteIndex::new(index).unwrap(),
            ))
        };
        for (row, colored) in rows.iter().zip(&colored) {
            // Only the sender's colour changes: text, styles and links
            // (what logs, copies and hotlinks read) stay.
            assert_eq!(colored.logical_text, row.logical_text);
            assert_eq!(colored.styles, row.styles);
            assert_eq!(colored.user_links, row.user_links);
            let expected = row
                .sender
                .as_ref()
                .map(|sender| match sender.user_id.as_str() {
                    "@alice:example" => palette(12),
                    _ => palette(4),
                });
            assert_eq!(colored.sender.as_ref().map(|sender| sender.color), expected);
        }
        settings.nick_colors.active = false;
        color_row_senders(
            &mut colored,
            &NickColorer::new(&settings, &state.model.room_views[room_id].members),
        );
        assert_eq!(colored, rows);
    }
}
