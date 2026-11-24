//! The per-room model and the facts derived from room activity.

use std::borrow::Borrow;

use super::line_edits::{Edit, EditOrder, EditedLine, LineEdits, Restore};
use super::*;

/// Where a room window's older history stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RoomHistoryBackfillState {
    /// The next page, when the window asks for one, starts at `next`.
    Idle {
        next: HistoryFrom,
    },
    Loading {
        from: HistoryFrom,
    },
    /// The room has nothing older.
    Exhausted,
    /// The page from `from` failed; asking again retries it.
    Failed {
        from: HistoryFrom,
        message: String,
    },
}

impl Default for RoomHistoryBackfillState {
    fn default() -> Self {
        Self::Idle {
            next: HistoryFrom::Latest,
        }
    }
}

#[derive(Default, Clone)]
pub(super) struct RoomViewModel {
    pub(super) topic: Option<String>,
    pub(super) activities: Vec<RoomActivity>,
    pub(super) reactions_by_target: BTreeMap<String, BTreeMap<String, RoomReactionEntry>>,
    pub(super) threads: BTreeMap<String, ThreadViewModel>,
    pub(super) preview_media: BTreeMap<String, RoomPreviewMediaState>,
    /// Messages replies answer that are not among `activities`, by event
    /// ID. Never one of a line: a line that loads takes its target's place
    /// (`take_line_from_quote`).
    pub(super) reply_targets: BTreeMap<String, ReplyTargetState>,
    /// The edits of each shown message (`shown_activity`) that was edited.
    pub(super) line_edits: LineEdits,
    pub(super) typing_user_ids: Vec<String>,
    pub(super) members: Vec<RoomMemberSummary>,
    pub(super) history: RoomHistoryBackfillState,
    pub(super) echo: EchoLog,
    pub(super) arrivals: RoomArrivals,
    /// Members whose identity changed, by user ID, as the room's identity
    /// stream last reported them (`identity.rs`).
    pub(super) identity_warnings: BTreeMap<String, IdentityWarning>,
}

/// Every room's view model, by room ID, each with a revision: the gate
/// that rebuilds a window's rows only when their inputs changed reads it.
///
/// The invariant that makes the gate safe: **every `&mut RoomViewModel`
/// handed out moves that room's revision.** A revision comes from one
/// counter for all rooms, so a room removed and created again never repeats
/// one. A mutable access that changes nothing still moves it, so loops over
/// every room must take mutable access only where they change something.
#[derive(Default, Clone)]
pub(super) struct RoomViews {
    map: BTreeMap<String, RevisedRoomView>,
    next_revision: u64,
}

#[derive(Clone)]
struct RevisedRoomView {
    view: RoomViewModel,
    revision: u64,
}

impl RoomViews {
    fn take_revision(&mut self) -> u64 {
        let revision = self.next_revision;
        self.next_revision += 1;
        revision
    }

    pub(super) fn get<Q>(&self, room_id: &Q) -> Option<&RoomViewModel>
    where
        String: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.map.get(room_id).map(|entry| &entry.view)
    }

    /// The room's revision; `None` without a view model.
    pub(super) fn revision(&self, room_id: &str) -> Option<u64> {
        self.map.get(room_id).map(|entry| entry.revision)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&String, &RoomViewModel)> {
        self.map
            .iter()
            .map(|(room_id, entry)| (room_id, &entry.view))
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &RoomViewModel> {
        self.map.values().map(|entry| &entry.view)
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub(super) fn get_mut<Q>(&mut self, room_id: &Q) -> Option<&mut RoomViewModel>
    where
        String: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let revision = self.take_revision();
        self.map.get_mut(room_id).map(|entry| {
            entry.revision = revision;
            &mut entry.view
        })
    }

    /// The room's view model, created empty if it has none.
    pub(super) fn entry_or_default(&mut self, room_id: impl Into<String>) -> &mut RoomViewModel {
        let revision = self.take_revision();
        let entry = self
            .map
            .entry(room_id.into())
            .or_insert_with(|| RevisedRoomView {
                view: RoomViewModel::default(),
                revision,
            });
        entry.revision = revision;
        &mut entry.view
    }

    pub(super) fn insert(&mut self, room_id: String, view: RoomViewModel) -> Option<RoomViewModel> {
        let revision = self.take_revision();
        self.map
            .insert(room_id, RevisedRoomView { view, revision })
            .map(|entry| entry.view)
    }

    pub(super) fn clear(&mut self) {
        self.map.clear();
    }
}

impl<Q> std::ops::Index<&Q> for RoomViews
where
    String: Borrow<Q>,
    Q: Ord + ?Sized,
{
    type Output = RoomViewModel;

    fn index(&self, room_id: &Q) -> &RoomViewModel {
        self.get(room_id).expect("the room has a view model")
    }
}

impl FromIterator<(String, RoomViewModel)> for RoomViews {
    fn from_iter<I: IntoIterator<Item = (String, RoomViewModel)>>(views: I) -> Self {
        let mut room_views = Self::default();
        for (room_id, view) in views {
            room_views.insert(room_id, view);
        }
        room_views
    }
}

/// The order in which a room's lines arrived live: its activities from
/// sync, local echoes and synthetic events, and its echo lines, numbered in
/// one sequence. History scrolled back to did not arrive, so it has no
/// number; that is what keeps it out of the logs.
#[derive(Debug, Default, Clone)]
pub(super) struct RoomArrivals {
    next: u64,
    /// The arrival of each live activity, by activity ID.
    by_activity: BTreeMap<String, u64>,
    /// The first arrival no closed window of the room has shown: where an
    /// automatic log of its next window starts.
    unshown_from: u64,
}

impl RoomArrivals {
    fn take_next(&mut self) -> u64 {
        let arrival = self.next;
        self.next += 1;
        arrival
    }

    fn stamp(&mut self, activity_id: &str) {
        let arrival = self.take_next();
        let earlier = self.by_activity.insert(activity_id.to_owned(), arrival);
        assert_eq!(earlier, None, "activity {activity_id} arrived twice");
    }

    /// An activity that took another's place keeps its arrival, if it had
    /// one: a sent message is the line the user typed.
    fn carry(&mut self, from_activity_id: &str, to_activity_id: &str) {
        if let Some(arrival) = self.by_activity.remove(from_activity_id) {
            let earlier = self.by_activity.insert(to_activity_id.to_owned(), arrival);
            assert_eq!(
                earlier, None,
                "activity {to_activity_id} took the place of {from_activity_id} but had arrived"
            );
        }
    }

    fn forget(&mut self, activity_id: &str) {
        self.by_activity.remove(activity_id);
    }

    pub(super) fn of(&self, activity_id: &str) -> Option<u64> {
        self.by_activity.get(activity_id).copied()
    }

    /// The arrival the next line will have.
    pub(super) fn next(&self) -> u64 {
        self.next
    }

    pub(super) fn unshown_from(&self) -> u64 {
        self.unshown_from
    }

    /// The room's window went away, having shown everything so far.
    pub(super) fn window_closed(&mut self) {
        self.unshown_from = self.next;
    }
}

/// The oldest lines the window buffer drops from a room: its first
/// `activities` and first `echo_lines`. Only `RoomViewModel::buffer_trim`
/// makes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BufferTrim {
    activities: usize,
    echo_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ThreadViewModel {
    pub(super) label: String,
}

/// A replied-to message fetched from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ReplyTargetState {
    Pending,
    Loaded(Box<RoomActivity>),
    Failed,
}

/// What a reply answers, as far as the window knows.
pub(super) enum ReplyTarget<'a> {
    Known(&'a RoomActivity),
    Loading,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RoomPreviewMediaState {
    Pending {
        event_id: String,
        format: RoomMediaFormat,
    },
    Loaded {
        event_id: String,
        format: RoomMediaFormat,
        image_key: String,
        width: u32,
        height: u32,
    },
    Failed {
        event_id: String,
        format: RoomMediaFormat,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LastMessageActivity {
    pub(super) room_id: String,
    pub(super) event_id: Option<String>,
    pub(super) transaction_id: Option<String>,
    pub(super) sender: String,
    pub(super) timestamp_ms: u64,
    pub(super) presentation: RoomMessagePresentationKind,
    pub(super) body: String,
    pub(super) send_state: RoomMessageSendState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MessageReplacementFields {
    pub(super) presentation: RoomMessagePresentationKind,
    pub(super) body: String,
    pub(super) styles: Vec<TextStyleSpan>,
    pub(super) user_links: Vec<UserLinkSpan>,
    pub(super) source_body: Option<String>,
}

impl MessageReplacementFields {
    /// What `message` shows that an edit replaces.
    pub(super) fn of(message: &RoomMessageActivity) -> Self {
        Self {
            presentation: message.presentation,
            body: message.body.clone(),
            styles: message.styles.clone(),
            user_links: message.user_links.clone(),
            source_body: message.source_body.clone(),
        }
    }

    fn apply_to(self, message: &mut RoomMessageActivity) {
        message.presentation = self.presentation;
        message.body = self.body;
        message.styles = self.styles;
        message.user_links = self.user_links;
        message.source_body = self.source_body;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RoomReactionEntry {
    reaction_event_id: String,
    sender: String,
    timestamp_ms: u64,
    key: String,
}

impl RoomViewModel {
    /// `/clear` in the room window: every line goes but the messages
    /// still queued to send. Thread windows keep their echo lines.
    pub(super) fn clear_visible_entries(&mut self) {
        let (queued, cleared) = std::mem::take(&mut self.activities)
            .into_iter()
            .partition(room_activity_is_queued);
        self.activities = queued;
        self.echo.clear_room_window();
        self.forget_activities(cleared);
    }

    /// The lines the window buffer counts: every activity, and every echo
    /// line of the room's windows. A message's quote and reaction lines
    /// belong to it, and the lines about the room ("Now talking in", its
    /// topic, who is typing) are not lines of the buffer.
    pub(super) fn buffer_lines(&self) -> usize {
        self.activities.len() + self.echo.lines().len()
    }

    /// mIRC's window buffer: which of the oldest lines, in the order the
    /// room window shows them, to drop so that at most `cap` remain; `None`
    /// when nothing goes. What stays is always the newest part of the room,
    /// without a gap, so older history loads back from just before it. A
    /// line that must stay ends the trim there: a message still queued to
    /// send, and a line of a thread whose window is open (`open_threads`,
    /// by root), which that window shows.
    pub(super) fn buffer_trim(
        &self,
        cap: usize,
        open_threads: &BTreeSet<String>,
    ) -> Option<BufferTrim> {
        let excess = self.buffer_lines().saturating_sub(cap);
        let in_open_thread =
            |root: Option<&str>| root.is_some_and(|root| open_threads.contains(root));
        let echo = self.echo.lines();
        let mut trim = BufferTrim {
            activities: 0,
            echo_lines: 0,
        };
        while trim.activities + trim.echo_lines < excess {
            // The room window's merge order (`push_activity_rows_with_echo`):
            // an echo line goes before the first activity newer than it.
            let echo_next = match (
                self.activities.get(trim.activities),
                echo.get(trim.echo_lines),
            ) {
                (Some(activity), Some(line)) => line.timestamp_ms < activity.timestamp_ms,
                (None, Some(_)) => true,
                (Some(_), None) => false,
                (None, None) => unreachable!("the excess lines are lines"),
            };
            if echo_next {
                if in_open_thread(echo[trim.echo_lines].thread_root.as_deref()) {
                    break;
                }
                trim.echo_lines += 1;
            } else {
                let activity = &self.activities[trim.activities];
                if room_activity_is_queued(activity)
                    || in_open_thread(room_activity_event_id(activity))
                    || in_open_thread(room_activity_thread_root_event_id(activity))
                {
                    break;
                }
                trim.activities += 1;
            }
        }
        (trim.activities + trim.echo_lines > 0).then_some(trim)
    }

    /// Drops the lines `buffer_trim` chose. Dropping activities gives the
    /// window older history again: from just before its oldest line, or,
    /// if no line it kept came from the server, the newest page, which goes
    /// above them. Not while a page is loading, which would go above a gap.
    pub(super) fn trim_buffer(&mut self, trim: BufferTrim) {
        assert!(
            !matches!(self.history, RoomHistoryBackfillState::Loading { .. }),
            "a room trims only between history pages: a page in flight would go above a gap"
        );
        self.echo.drop_oldest(trim.echo_lines);
        if trim.activities == 0 {
            return;
        }
        let dropped = self.activities.drain(..trim.activities).collect();
        self.forget_activities(dropped);
        self.history = RoomHistoryBackfillState::Idle {
            next: self
                .activities
                .iter()
                .find_map(room_activity_event_id)
                .map_or(HistoryFrom::Latest, |event_id| HistoryFrom::BeforeEvent {
                    event_id: event_id.to_owned(),
                }),
        };
    }

    /// Forgets what the model holds for activities that left it. One that
    /// a remaining reply quotes stays as that reply's target, with its
    /// edits, so the quote keeps its text without asking the server;
    /// targets nothing quotes any more go, with theirs.
    fn forget_activities(&mut self, dropped: Vec<RoomActivity>) {
        let quoted = self
            .activities
            .iter()
            .filter_map(|activity| activity.relation.as_ref())
            .filter_map(replies::reply_quote_target)
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        for activity in &dropped {
            self.arrivals.forget(&activity.activity_id);
            if let Some(event_id) = room_activity_event_id(activity) {
                self.reactions_by_target.remove(event_id);
                if !quoted.contains(event_id) {
                    self.line_edits.forget(event_id);
                }
            }
        }
        for activity in dropped {
            if let Some(event_id) =
                room_activity_event_id(&activity).filter(|event_id| quoted.contains(*event_id))
            {
                self.reply_targets.insert(
                    event_id.to_owned(),
                    ReplyTargetState::Loaded(Box::new(activity)),
                );
            }
        }
        let line_edits = &mut self.line_edits;
        self.reply_targets.retain(|event_id, _| {
            let kept = quoted.contains(event_id);
            if !kept {
                line_edits.forget(event_id);
            }
            kept
        });
        self.retain_preview_media_for_visible_activities();
        self.rebuild_threads();
    }

    pub(super) fn apply_activity(&mut self, activity: RoomActivity) {
        match &activity.kind {
            RoomActivityKind::Message(message) => {
                let transaction_id = message.transaction_id.clone();
                let event_id = message.event_id.clone();
                match &message.send_state {
                    RoomMessageSendState::Pending => {
                        let transaction_id = transaction_id
                            .as_deref()
                            .expect("pending room activity requires a transaction id");
                        self.upsert_message_activity(activity, Some(transaction_id), None);
                    }
                    RoomMessageSendState::Sent => {
                        let event_id = event_id
                            .as_deref()
                            .expect("sent room activity requires an event id");
                        self.upsert_message_activity(
                            activity,
                            transaction_id.as_deref(),
                            Some(event_id),
                        );
                    }
                    RoomMessageSendState::Failed { .. } => {
                        self.upsert_message_activity(
                            activity,
                            transaction_id.as_deref(),
                            event_id.as_deref(),
                        );
                    }
                }
            }
            RoomActivityKind::Image(_)
            | RoomActivityKind::File(_)
            | RoomActivityKind::Audio(_)
            | RoomActivityKind::Video(_)
            | RoomActivityKind::Sticker(_)
            | RoomActivityKind::Poll(_)
            | RoomActivityKind::VerificationRequest(_)
            | RoomActivityKind::UnsupportedMessage(_)
            | RoomActivityKind::UnableToDecrypt(_) => self.push_live(activity),
            RoomActivityKind::StateNotice(notice) => {
                if let RoomStateNoticeContent::Topic { topic } = &notice.content {
                    self.topic = Some(topic.clone());
                }
                self.push_live(activity);
            }
            RoomActivityKind::Redacted(redacted) => {
                self.remove_reactions_for_target(&redacted.event_id);
                self.push_live(activity);
            }
        }
        self.rebuild_threads();
    }

    fn push_live(&mut self, activity: RoomActivity) {
        if let Some(event_id) = room_activity_event_id(&activity) {
            self.take_line_from_quote(event_id);
        }
        self.arrivals.stamp(&activity.activity_id);
        self.activities.push(activity);
    }

    /// `event_id` becomes a line: a reply's fetched copy of it, if any,
    /// goes with the edits it held, since the line brings its own (with
    /// its page, or live) and must keep them from its own text.
    fn take_line_from_quote(&mut self, event_id: &str) {
        if self.reply_targets.remove(event_id).is_some() {
            self.line_edits.forget(event_id);
        }
    }

    /// Prints a command's line in the room window (`None`) or a thread
    /// window: it arrives like any other line.
    pub(super) fn push_echo(
        &mut self,
        timestamp_ms: u64,
        thread_root: Option<String>,
        kind: StatusLineKind,
        text: String,
    ) {
        let arrival = self.arrivals.take_next();
        self.echo
            .push(arrival, timestamp_ms, thread_root, kind, text);
    }

    pub(super) fn apply_history_backfill(&mut self, room_id: &str, backfill: RoomHistoryBackfill) {
        assert_eq!(
            backfill.room_id, room_id,
            "room history backfill must target the mutated room view"
        );

        let mut prepended_count = 0usize;
        let mut changed = false;
        for event in backfill.events {
            match event {
                RoomHistoryBackfillEvent::Activity(activity) => {
                    assert_eq!(
                        activity.room_id, room_id,
                        "room history activity must target the mutated room view"
                    );
                    if let Some(event_id) = room_activity_event_id(&activity) {
                        if self.find_activity_index_by_event_id(event_id).is_some() {
                            continue;
                        }
                        self.take_line_from_quote(event_id);
                    }

                    // History did not arrive: no arrival.
                    self.activities.insert(prepended_count, *activity);
                    prepended_count += 1;
                    changed = true;
                }
                RoomHistoryBackfillEvent::ActivityUpdate(update) => {
                    assert_eq!(
                        update.room_id, room_id,
                        "room history activity update must target the mutated room view"
                    );
                    if self.validate_update(&update).is_err() {
                        continue;
                    }

                    changed |= self.apply_update(&update);
                }
            }
        }

        if changed {
            self.rebuild_threads();
            self.retain_preview_media_for_visible_activities();
        }
    }

    pub(super) fn replace_typing(&mut self, typing: RoomTyping) {
        self.typing_user_ids = typing.user_ids;
    }

    pub(super) fn replace_members(&mut self, members: RoomMembers) {
        self.members = members.members;
    }

    /// A user's nick from the joined-member snapshot; users outside it, such
    /// as typing notifications racing a membership change, show by user ID.
    pub(super) fn member_nick(&self, user_id: &str) -> String {
        self.members
            .iter()
            .find(|member| member.user_id == user_id)
            .map_or_else(|| user_id.to_owned(), room_member_nick)
    }

    pub(super) fn projected_preview_state(
        &self,
        activity_id: &str,
        event_id: &str,
        format: &RoomMediaFormat,
    ) -> TimelineImageState {
        match self.preview_media.get(activity_id) {
            Some(RoomPreviewMediaState::Pending {
                event_id: stored_event_id,
                format: stored_format,
            }) if stored_event_id == event_id && stored_format == format => {
                TimelineImageState::Pending
            }
            Some(RoomPreviewMediaState::Loaded {
                event_id: stored_event_id,
                format: stored_format,
                image_key,
                width,
                height,
            }) if stored_event_id == event_id && stored_format == format => {
                TimelineImageState::Loaded {
                    key: image_key.clone(),
                    width: *width,
                    height: *height,
                }
            }
            Some(RoomPreviewMediaState::Failed {
                event_id: stored_event_id,
                format: stored_format,
                message,
            }) if stored_event_id == event_id && stored_format == format => {
                TimelineImageState::Failed {
                    message: message.clone(),
                }
            }
            _ => TimelineImageState::Pending,
        }
    }

    pub(super) fn should_request_preview_media(
        &self,
        activity_id: &str,
        event_id: &str,
        format: &RoomMediaFormat,
    ) -> bool {
        match self.preview_media.get(activity_id) {
            Some(RoomPreviewMediaState::Pending {
                event_id: stored_event_id,
                format: stored_format,
            })
            | Some(RoomPreviewMediaState::Loaded {
                event_id: stored_event_id,
                format: stored_format,
                ..
            })
            | Some(RoomPreviewMediaState::Failed {
                event_id: stored_event_id,
                format: stored_format,
                ..
            }) => stored_event_id != event_id || stored_format != format,
            None => true,
        }
    }

    pub(super) fn mark_preview_media_pending(
        &mut self,
        activity_id: String,
        event_id: String,
        format: RoomMediaFormat,
    ) -> Option<RoomPreviewMediaState> {
        self.preview_media.insert(
            activity_id,
            RoomPreviewMediaState::Pending { event_id, format },
        )
    }

    pub(super) fn remove_preview_media(
        &mut self,
        activity_id: &str,
    ) -> Option<RoomPreviewMediaState> {
        self.preview_media.remove(activity_id)
    }

    pub(super) fn insert_preview_media(
        &mut self,
        activity_id: String,
        preview_media: RoomPreviewMediaState,
    ) -> Option<RoomPreviewMediaState> {
        self.preview_media.insert(activity_id, preview_media)
    }

    fn upsert_message_activity(
        &mut self,
        activity: RoomActivity,
        transaction_id: Option<&str>,
        event_id: Option<&str>,
    ) {
        if let Some(index) = self.find_message_activity_index(transaction_id, event_id) {
            // A queued line that synced gains its event ID.
            if let Some(event_id) = event_id {
                self.take_line_from_quote(event_id);
            }
            self.arrivals
                .carry(&self.activities[index].activity_id, &activity.activity_id);
            self.activities[index] = activity;
        } else {
            self.push_live(activity);
        }
    }

    fn mark_message_sent(&mut self, transaction_id: &str, event_id: &str) {
        self.take_line_from_quote(event_id);
        let transaction_index = self
            .find_pending_message_activity_index(transaction_id)
            .expect("outgoing sent requires an existing queued timeline row");
        let duplicate_event_index = self
            .find_message_activity_index(None, Some(event_id))
            .filter(|event_index| *event_index != transaction_index);
        let duplicate_event =
            duplicate_event_index.map(|event_index| self.activities[event_index].clone());
        let duplicate_event = duplicate_event.and_then(|activity| {
            let RoomActivityKind::Message(message) = activity.kind else {
                return None;
            };
            Some((
                activity.activity_id,
                activity.timestamp_ms,
                activity.relation,
                message.sender,
                message.presentation,
                message.body,
            ))
        });
        let new_activity_id = duplicate_event
            .as_ref()
            .map(|(activity_id, _, _, _, _, _)| activity_id.clone())
            .unwrap_or_else(|| format!("event:{event_id}"));
        let synced_timestamp_ms = duplicate_event
            .as_ref()
            .map(|(_, timestamp_ms, _, _, _, _)| *timestamp_ms);
        let synced_relation = duplicate_event
            .as_ref()
            .and_then(|(_, _, relation, _, _, _)| relation.clone());
        let synced_sender = duplicate_event
            .as_ref()
            .map(|(_, _, _, sender, _, _)| sender.clone());
        let synced_presentation = duplicate_event
            .as_ref()
            .map(|(_, _, _, _, presentation, _)| *presentation);
        let synced_body = duplicate_event
            .as_ref()
            .map(|(_, _, _, _, _, body)| body.clone());

        // The synced copy goes; the line the user typed stays, arrived when
        // it was typed.
        if let Some((duplicate_activity_id, ..)) = &duplicate_event {
            self.arrivals.forget(duplicate_activity_id);
        }
        self.arrivals.carry(
            &self.activities[transaction_index].activity_id,
            &new_activity_id,
        );
        let activity = self
            .activities
            .get_mut(transaction_index)
            .expect("transaction row index must resolve");
        activity.activity_id = new_activity_id;
        if let Some(synced_timestamp_ms) = synced_timestamp_ms {
            activity.timestamp_ms = synced_timestamp_ms;
        }
        if let Some(synced_relation) = synced_relation {
            activity.relation = Some(synced_relation);
        }
        let RoomActivityKind::Message(message) = &mut activity.kind else {
            panic!("outgoing sent requires a message room activity");
        };

        message.event_id = Some(event_id.to_owned());
        if let Some(synced_sender) = synced_sender {
            message.sender = synced_sender;
        }
        if let Some(synced_presentation) = synced_presentation {
            message.presentation = synced_presentation;
        }
        if let Some(synced_body) = synced_body {
            message.body = synced_body;
        }
        message.send_state = RoomMessageSendState::Sent;

        if let Some(duplicate_event_index) = duplicate_event_index {
            self.activities.remove(duplicate_event_index);
        }
        self.rebuild_threads();
    }

    fn mark_message_failed(&mut self, transaction_id: &str, message: &str, recoverable: bool) {
        let activity = self
            .find_pending_message_activity_mut(transaction_id)
            .expect("outgoing failed requires an existing queued timeline row");
        let RoomActivityKind::Message(room_message) = &mut activity.kind else {
            panic!("outgoing failed requires a message room activity");
        };

        room_message.send_state = RoomMessageSendState::Failed {
            message: message.to_owned(),
            recoverable,
        };
        self.rebuild_threads();
    }

    /// Keeps an edit of a shown message (`shown_activity`: a line, or a
    /// reply's quote) from its sender, and shows it if it is the latest:
    /// only a message's sender can edit it (the spec's validity of
    /// replacement events), and anyone else's edit is ignored, as Element
    /// does. A message that could not be decrypted holds the edit until it
    /// decrypts. Whether the room changed.
    fn replace_message(&mut self, target_event_id: &str, order: EditOrder, edit: Edit) -> bool {
        let Some(shown) = shown_activity_in(&self.activities, &self.reply_targets, target_event_id)
        else {
            return false;
        };
        // The envelope's sender: the server vouches for it, not the
        // ciphertext, and decryption keeps it.
        let line_sender = match &shown.kind {
            RoomActivityKind::Message(message) => &message.sender.user_id,
            RoomActivityKind::UnableToDecrypt(utd) => &utd.sender.user_id,
            // An edit replaces a text line's text; no other line has any.
            RoomActivityKind::Image(_)
            | RoomActivityKind::File(_)
            | RoomActivityKind::Audio(_)
            | RoomActivityKind::Video(_)
            | RoomActivityKind::Sticker(_)
            | RoomActivityKind::Poll(_)
            | RoomActivityKind::VerificationRequest(_)
            | RoomActivityKind::UnsupportedMessage(_)
            | RoomActivityKind::StateNotice(_)
            | RoomActivityKind::Redacted(_) => return false,
        };
        if *line_sender != edit.sender {
            return false;
        }
        let line = match &shown.kind {
            RoomActivityKind::Message(message) => EditedLine::Message(message),
            RoomActivityKind::UnableToDecrypt(_) => EditedLine::Undecrypted,
            _ => unreachable!("the line's sender was read above"),
        };
        let undecrypted = matches!(line, EditedLine::Undecrypted);
        let Some(fields) = self.line_edits.edit(target_event_id, order, edit, line) else {
            // Held, or older than the edit the line shows: kept to fall
            // back to.
            return undecrypted;
        };
        self.show_edit(target_event_id, fields);
        true
    }

    /// The shown message `event_id` shows `fields`, from an edit or its
    /// own. Only lines are in threads, but a quote's is no rebuild worth
    /// telling apart.
    fn show_edit(&mut self, event_id: &str, fields: MessageReplacementFields) {
        let shown = self
            .shown_activity_mut(event_id)
            .expect("an edited message is shown");
        let RoomActivityKind::Message(message) = &mut shown.kind else {
            panic!("only a message shows an edit");
        };
        fields.apply_to(message);
        self.rebuild_threads();
    }

    /// The edit `replacement_event_id` was redacted: its message shows the
    /// latest edit left, or its own text. Whether a shown message had it
    /// (its edits go with it, so it still is).
    fn redact_edit(&mut self, replacement_event_id: &str) -> bool {
        let Some((target, restore)) = self.line_edits.redact_edit(replacement_event_id) else {
            return false;
        };
        let fields = match restore {
            Restore::Edit(fields) | Restore::Original(fields) => fields,
            Restore::Unchanged => return true,
        };
        self.show_edit(&target, fields);
        true
    }

    /// Redacts the event a redaction names, whatever the room shows it as:
    /// a line (its reactions and edits go with it), a reply's quote of a
    /// message that is no line (its edits go), a reaction, or an edit
    /// (`redact_edit`). Whether the room showed it.
    fn redact_activity(&mut self, target_event_id: &str) -> bool {
        let Some(index) = self.find_activity_index_by_event_id(target_event_id) else {
            if let Some(ReplyTargetState::Loaded(target)) =
                self.reply_targets.get_mut(target_event_id)
            {
                // The quote reads `message redacted`, as one of a redacted
                // line does.
                self.line_edits.forget(target_event_id);
                target.kind = RoomActivityKind::Redacted(RoomRedactedActivity {
                    event_id: target_event_id.to_owned(),
                });
                return true;
            }
            return self.remove_reaction(target_event_id) || self.redact_edit(target_event_id);
        };
        let activity_id = self.activities[index].activity_id.clone();
        self.remove_reactions_for_target(target_event_id);
        // A redacted line says only that; what it was, edited or not, is gone.
        self.line_edits.forget(target_event_id);
        self.activities[index].kind = RoomActivityKind::Redacted(RoomRedactedActivity {
            event_id: target_event_id.to_owned(),
        });
        self.remove_preview_media(&activity_id);
        self.rebuild_threads();
        true
    }

    pub(super) fn add_reaction(
        &mut self,
        target_event_id: &str,
        reaction_event_id: &str,
        sender: &str,
        timestamp_ms: u64,
        key: &str,
    ) -> bool {
        let Some(index) = self.find_activity_index_by_event_id(target_event_id) else {
            return false;
        };
        if matches!(self.activities[index].kind, RoomActivityKind::Redacted(_)) {
            return false;
        }
        self.reactions_by_target
            .entry(target_event_id.to_owned())
            .or_default()
            .insert(
                reaction_event_id.to_owned(),
                RoomReactionEntry {
                    reaction_event_id: reaction_event_id.to_owned(),
                    sender: sender.to_owned(),
                    timestamp_ms,
                    key: key.to_owned(),
                },
            );
        true
    }

    /// The line a shown reaction reacts to.
    fn reaction_target(&self, reaction_event_id: &str) -> Option<&str> {
        self.reactions_by_target
            .iter()
            .find(|(_, reactions)| reactions.contains_key(reaction_event_id))
            .map(|(target_event_id, _)| target_event_id.as_str())
    }

    fn remove_reaction(&mut self, reaction_event_id: &str) -> bool {
        let Some(target_event_id) = self.reaction_target(reaction_event_id).map(str::to_owned)
        else {
            return false;
        };
        let reactions = self
            .reactions_by_target
            .get_mut(&target_event_id)
            .expect("reaction_target found it");
        reactions.remove(reaction_event_id);
        if reactions.is_empty() {
            self.reactions_by_target.remove(&target_event_id);
        }
        true
    }

    fn remove_reactions_for_target(&mut self, target_event_id: &str) {
        self.reactions_by_target.remove(target_event_id);
    }

    pub(super) fn reaction_summaries_for_event(
        &self,
        target_event_id: &str,
    ) -> Vec<ReactionSummary> {
        let Some(reactions) = self.reactions_by_target.get(target_event_id) else {
            return Vec::new();
        };

        // In reaction time, so the first reaction stays first as more
        // arrive: reactors within a group, and groups by their first.
        let mut entries = reactions.values().collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            (left.timestamp_ms, &left.reaction_event_id)
                .cmp(&(right.timestamp_ms, &right.reaction_event_id))
        });
        let mut grouped = Vec::<(String, Vec<&RoomReactionEntry>)>::new();
        for entry in entries {
            match grouped.iter_mut().find(|(key, _)| *key == entry.key) {
                Some((_, group)) => group.push(entry),
                None => grouped.push((entry.key.clone(), vec![entry])),
            }
        }

        grouped
            .into_iter()
            .map(|(key, entries)| ReactionSummary {
                key,
                count: entries.len() as u64,
                senders: entries.iter().map(|entry| entry.sender.clone()).collect(),
                reaction_event_ids: entries
                    .iter()
                    .map(|entry| entry.reaction_event_id.clone())
                    .collect(),
            })
            .collect()
    }

    pub(super) fn find_message_activity_index(
        &self,
        transaction_id: Option<&str>,
        event_id: Option<&str>,
    ) -> Option<usize> {
        if let Some(transaction_id) = transaction_id
            && let Some(index) = self.activities.iter().position(|activity| {
                matches!(
                    &activity.kind,
                    RoomActivityKind::Message(RoomMessageActivity {
                        transaction_id: Some(existing_transaction_id),
                        ..
                    }) if existing_transaction_id == transaction_id
                )
            })
        {
            return Some(index);
        }

        if let Some(event_id) = event_id
            && let Some(index) = self.activities.iter().position(|activity| {
                matches!(
                    &activity.kind,
                    RoomActivityKind::Message(RoomMessageActivity {
                        event_id: Some(existing_event_id),
                        ..
                    }) if existing_event_id == event_id
                )
            })
        {
            return Some(index);
        }

        None
    }

    /// A loaded message wins over a fetched one: it is live, with its
    /// edits and redaction.
    pub(super) fn reply_target(&self, event_id: &str) -> ReplyTarget<'_> {
        if let Some(index) = self.find_activity_index_by_event_id(event_id) {
            return ReplyTarget::Known(&self.activities[index]);
        }
        match self.reply_targets.get(event_id) {
            Some(ReplyTargetState::Loaded(activity)) => ReplyTarget::Known(activity),
            Some(ReplyTargetState::Failed) => ReplyTarget::Unavailable,
            Some(ReplyTargetState::Pending) | None => ReplyTarget::Loading,
        }
    }

    /// The message the window shows for the event: its line, or else the
    /// message a reply quotes, fetched. Both follow the message's edits
    /// and redaction.
    fn shown_activity(&self, event_id: &str) -> Option<&RoomActivity> {
        shown_activity_in(&self.activities, &self.reply_targets, event_id)
    }

    fn shown_activity_mut(&mut self, event_id: &str) -> Option<&mut RoomActivity> {
        match self.find_activity_index_by_event_id(event_id) {
            Some(index) => Some(&mut self.activities[index]),
            None => match self.reply_targets.get_mut(event_id) {
                Some(ReplyTargetState::Loaded(target)) => Some(&mut **target),
                Some(ReplyTargetState::Pending | ReplyTargetState::Failed) | None => None,
            },
        }
    }

    /// What the window shows of the event (`shown_activity`), if it shows
    /// it as one that could not be decrypted.
    pub(super) fn shown_undecrypted(&self, event_id: &str) -> Option<&RoomActivity> {
        self.shown_activity(event_id)
            .filter(|shown| matches!(shown.kind, RoomActivityKind::UnableToDecrypt(_)))
    }

    /// An event that could not be decrypted, read again
    /// (`shown_undecrypted`): its line, or a reply's quote of it, becomes
    /// what it is now in place, with the same arrival. Decrypted, it is
    /// the message, showing the latest edit it held of its sender; still
    /// undecryptable, its cause may have changed, and its edits wait on.
    pub(super) fn apply_decrypted(&mut self, mut activity: RoomActivity) {
        let event_id = room_activity_event_id(&activity)
            .expect("a decrypted activity is an event's")
            .to_owned();
        assert!(
            self.shown_undecrypted(&event_id).is_some(),
            "{event_id} is not shown as undecrypted"
        );
        let decrypted = match &mut activity.kind {
            RoomActivityKind::UnableToDecrypt(_) => false,
            RoomActivityKind::Message(message) => {
                if let Some(fields) = self.line_edits.decrypted(&event_id, message) {
                    fields.apply_to(message);
                }
                true
            }
            // A message of another kind has no text to edit.
            _ => {
                self.line_edits.forget(&event_id);
                true
            }
        };
        let shown = self
            .shown_activity_mut(&event_id)
            .expect("shown_undecrypted found it");
        // The core names both after the event: the arrival stays valid.
        assert_eq!(
            shown.activity_id, activity.activity_id,
            "a decrypted line keeps its activity"
        );
        *shown = activity;
        // Its cause is in no thread's name; decrypted, it may be a
        // thread's message, or a thread's root.
        if decrypted {
            self.rebuild_threads();
        }
    }

    /// The events the window shows that could not be decrypted
    /// (`shown_undecrypted`), with a Megolm session or without (an Olm
    /// event's): what a retry reads again.
    pub(super) fn undecrypted_event_ids(&self) -> BTreeSet<String> {
        let reply_targets = self
            .reply_targets
            .values()
            .filter_map(|target| match target {
                ReplyTargetState::Loaded(activity) => Some(&**activity),
                ReplyTargetState::Pending | ReplyTargetState::Failed => None,
            });
        self.activities
            .iter()
            .chain(reply_targets)
            .filter_map(|activity| match &activity.kind {
                RoomActivityKind::UnableToDecrypt(utd) => Some(utd.event_id.clone()),
                _ => None,
            })
            .collect()
    }

    pub(super) fn find_activity_index_by_event_id(&self, event_id: &str) -> Option<usize> {
        self.activities
            .iter()
            .position(|activity| room_activity_event_id(activity) == Some(event_id))
    }

    /// The row of a message still in the core's send queue (`is_queued`):
    /// a `MessageSent` or `MessageFailed` comes for it.
    fn find_pending_message_activity_index(&self, transaction_id: &str) -> Option<usize> {
        self.activities.iter().position(|activity| {
            matches!(
                &activity.kind,
                RoomActivityKind::Message(RoomMessageActivity {
                    transaction_id: Some(existing_transaction_id),
                    send_state,
                    ..
                }) if existing_transaction_id == transaction_id && is_queued(send_state)
            )
        })
    }

    fn find_pending_message_activity_mut(
        &mut self,
        transaction_id: &str,
    ) -> Option<&mut RoomActivity> {
        let index = self.find_pending_message_activity_index(transaction_id)?;
        self.activities.get_mut(index)
    }

    fn rebuild_threads(&mut self) {
        let root_labels = self
            .activities
            .iter()
            .filter_map(|activity| {
                Some((
                    room_activity_event_id(activity)?.to_owned(),
                    room_activity_thread_root_label(activity)?,
                ))
            })
            .collect::<BTreeMap<_, _>>();

        // A thread whose root is no longer loaded (cleared, or dropped by
        // the window buffer) keeps the name it had.
        self.threads = self
            .activities
            .iter()
            .filter_map(|activity| room_activity_thread_root_event_id(activity))
            .map(|thread_root_event_id| {
                let label = root_labels
                    .get(thread_root_event_id)
                    .cloned()
                    .or_else(|| {
                        self.threads
                            .get(thread_root_event_id)
                            .map(|thread| thread.label.clone())
                    })
                    .unwrap_or_else(|| thread_root_event_id.to_owned());
                (thread_root_event_id.to_owned(), ThreadViewModel { label })
            })
            .collect();
    }

    /// Applies an update to what the room shows; whether anything changed.
    /// A target the room does not show (any more, or yet) changes nothing,
    /// but a send-state update's queued line must be there.
    pub(super) fn apply_update(&mut self, update: &RoomActivityUpdate) -> bool {
        match (&update.target, &update.kind) {
            (
                RoomActivityUpdateTarget::Transaction { transaction_id },
                RoomActivityUpdateKind::MessageSent { event_id },
            ) => {
                self.mark_message_sent(transaction_id, event_id);
                true
            }
            (
                RoomActivityUpdateTarget::Transaction { transaction_id },
                RoomActivityUpdateKind::MessageFailed {
                    message,
                    recoverable,
                },
            ) => {
                self.mark_message_failed(transaction_id, message, *recoverable);
                true
            }
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::MessageReplaced {
                    replacement_event_id,
                    sender,
                    timestamp_ms,
                    presentation,
                    body,
                    styles,
                    user_links,
                    source_body,
                },
            ) => self.replace_message(
                event_id,
                EditOrder {
                    timestamp_ms: *timestamp_ms,
                    replacement_event_id: replacement_event_id.clone(),
                },
                Edit {
                    sender: sender.clone(),
                    fields: MessageReplacementFields {
                        presentation: *presentation,
                        body: body.clone(),
                        styles: styles.clone(),
                        user_links: user_links.clone(),
                        source_body: source_body.clone(),
                    },
                },
            ),
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ActivityRedacted { .. },
            ) => self.redact_activity(event_id),
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ReactionAdded {
                    reaction_event_id,
                    sender,
                    timestamp_ms,
                    key,
                },
            ) => self.add_reaction(event_id, reaction_event_id, sender, *timestamp_ms, key),
            (
                RoomActivityUpdateTarget::Event { .. },
                RoomActivityUpdateKind::MessageSent { .. }
                | RoomActivityUpdateKind::MessageFailed { .. },
            ) => panic!("a send-state update requires a transaction target: {update:?}"),
            (RoomActivityUpdateTarget::Transaction { .. }, _) => {
                panic!("an event's update requires an event target: {update:?}")
            }
        }
    }

    fn validate_update(&self, update: &RoomActivityUpdate) -> Result<(), String> {
        match (&update.target, &update.kind) {
            (
                RoomActivityUpdateTarget::Transaction { transaction_id },
                RoomActivityUpdateKind::MessageSent { .. },
            ) => self
                .find_pending_message_activity_index(transaction_id)
                .map(|_| ())
                .ok_or_else(|| "outgoing sent requires an existing queued timeline row".to_owned()),
            (
                RoomActivityUpdateTarget::Transaction { transaction_id },
                RoomActivityUpdateKind::MessageFailed { .. },
            ) => self
                .find_pending_message_activity_index(transaction_id)
                .map(|_| ())
                .ok_or_else(|| {
                    "outgoing failed requires an existing queued timeline row".to_owned()
                }),
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::MessageReplaced { .. },
            ) => self
                .shown_activity(event_id)
                .filter(|shown| {
                    matches!(
                        shown.kind,
                        RoomActivityKind::Message(_) | RoomActivityKind::UnableToDecrypt(_)
                    )
                })
                .map(|_| ())
                .ok_or_else(|| "message edit requires an existing visible target event".to_owned()),
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ActivityRedacted { .. },
            ) => (self.shown_activity(event_id).is_some()
                || self.reaction_target(event_id).is_some()
                || self.line_edits.has_edit(event_id))
            .then_some(())
            .ok_or_else(|| {
                "activity redaction requires an existing visible target event".to_owned()
            }),
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ReactionAdded { .. },
            ) => {
                let Some(index) = self.find_activity_index_by_event_id(event_id) else {
                    return Err("reaction add requires an existing visible target event".to_owned());
                };
                if matches!(self.activities[index].kind, RoomActivityKind::Redacted(_)) {
                    return Err("reaction add requires an existing visible target event".to_owned());
                }
                Ok(())
            }
            (
                RoomActivityUpdateTarget::Event { .. },
                RoomActivityUpdateKind::MessageSent { .. },
            ) => Err("message sent update requires a transaction target".to_owned()),
            (
                RoomActivityUpdateTarget::Event { .. },
                RoomActivityUpdateKind::MessageFailed { .. },
            ) => Err("message failed update requires a transaction target".to_owned()),
            (
                RoomActivityUpdateTarget::Transaction { .. },
                RoomActivityUpdateKind::MessageReplaced { .. },
            ) => Err("message edit requires an event target".to_owned()),
            (
                RoomActivityUpdateTarget::Transaction { .. },
                RoomActivityUpdateKind::ActivityRedacted { .. },
            ) => Err("activity redacted update requires an event target".to_owned()),
            (
                RoomActivityUpdateTarget::Transaction { .. },
                RoomActivityUpdateKind::ReactionAdded { .. },
            ) => Err("reaction add requires an event target".to_owned()),
        }
    }

    /// Every arrival is of an activity the room has, and was handed out.
    #[cfg(test)]
    pub(super) fn assert_arrivals_consistent(&self) {
        for (activity_id, arrival) in &self.arrivals.by_activity {
            assert!(
                self.activities
                    .iter()
                    .any(|activity| activity.activity_id == *activity_id),
                "arrival of a missing activity {activity_id}"
            );
            assert!(
                *arrival < self.arrivals.next,
                "arrival {arrival} not handed out"
            );
        }
    }

    fn retain_preview_media_for_visible_activities(&mut self) {
        self.preview_media.retain(|activity_id, _| {
            self.activities
                .iter()
                .any(|activity| activity.activity_id == *activity_id)
        });
    }
}

/// `shown_activity`, borrowing only what it reads.
fn shown_activity_in<'a>(
    activities: &'a [RoomActivity],
    reply_targets: &'a BTreeMap<String, ReplyTargetState>,
    event_id: &str,
) -> Option<&'a RoomActivity> {
    match activities
        .iter()
        .find(|activity| room_activity_event_id(activity) == Some(event_id))
    {
        Some(line) => Some(line),
        None => match reply_targets.get(event_id) {
            Some(ReplyTargetState::Loaded(target)) => Some(&**target),
            Some(ReplyTargetState::Pending | ReplyTargetState::Failed) | None => None,
        },
    }
}

pub(super) fn validate_synthetic_room_activity_events(
    room_views: &RoomViews,
    events: &[Event],
) -> Result<(), String> {
    let mut room_views = room_views.clone();

    for event in events {
        match event {
            Event::RoomActivity(activity) => room_views
                .entry_or_default(activity.room_id.clone())
                .apply_activity(activity.as_ref().clone()),
            Event::RoomActivityUpdate(update) => {
                let room_view = room_views.entry_or_default(update.room_id.clone());
                room_view.validate_update(update)?;
                room_view.apply_update(update);
            }
            _ => {}
        }
    }

    Ok(())
}

pub(super) fn last_activity_from_room_activity(
    activity: &RoomActivity,
) -> Option<LastMessageActivity> {
    match &activity.kind {
        RoomActivityKind::Message(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: message.event_id.clone(),
            transaction_id: message.transaction_id.clone(),
            sender: message.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: message.presentation,
            body: message.body.clone(),
            send_state: message.send_state.clone(),
        }),
        RoomActivityKind::UnsupportedMessage(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: format!("[unsupported {}] {}", message.msgtype, message.body),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::UnableToDecrypt(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: "[encrypted] Unable to decrypt message".to_owned(),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::Image(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: format!(
                "[image] {}",
                message.filename.as_deref().unwrap_or(message.body.as_str())
            ),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::File(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: format!(
                "[file] {}",
                message.filename.as_deref().unwrap_or(message.body.as_str())
            ),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::Audio(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: format!(
                "[audio] {}",
                message.filename.as_deref().unwrap_or(message.body.as_str())
            ),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::Video(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: format!(
                "[video] {}",
                message.filename.as_deref().unwrap_or(message.body.as_str())
            ),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::Sticker(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: format!("[sticker] {}", message.body),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::Poll(poll) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(poll.event_id.clone()),
            transaction_id: None,
            sender: poll.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: match &poll.content {
                RoomPollContent::Start { question, .. } => format!("[poll] {question}"),
                RoomPollContent::End { text, .. } => format!("[poll ended] {text}"),
            },
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::VerificationRequest(request) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(request.event_id.clone()),
            transaction_id: None,
            sender: request.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: "[verification request]".to_owned(),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::StateNotice(_) => None,
        RoomActivityKind::Redacted(_) => None,
    }
}

/// Who a line comes from. A redaction placeholder has no sender: the core
/// does not report who removed the event.
pub(super) fn room_activity_sender_user_id(activity: &RoomActivity) -> Option<&str> {
    match &activity.kind {
        RoomActivityKind::Message(message) => Some(&message.sender.user_id),
        RoomActivityKind::Image(message) => Some(&message.sender.user_id),
        RoomActivityKind::File(message) => Some(&message.sender.user_id),
        RoomActivityKind::Audio(message) => Some(&message.sender.user_id),
        RoomActivityKind::Video(message) => Some(&message.sender.user_id),
        RoomActivityKind::Sticker(message) => Some(&message.sender.user_id),
        RoomActivityKind::Poll(poll) => Some(&poll.sender.user_id),
        RoomActivityKind::VerificationRequest(request) => Some(&request.sender.user_id),
        RoomActivityKind::UnsupportedMessage(message) => Some(&message.sender.user_id),
        RoomActivityKind::UnableToDecrypt(message) => Some(&message.sender.user_id),
        RoomActivityKind::StateNotice(notice) => Some(&notice.sender.user_id),
        RoomActivityKind::Redacted(RoomRedactedActivity { event_id: _ }) => None,
    }
}

/// The activity a line marks its view with while the user is elsewhere.
/// `None` for our own lines, as in mIRC: our pending echo, its sync echo
/// and what we sent from another device. Without a signed-in user
/// (`own_user_id` is `None`) no line is ours. The login flow delivers the
/// initial sync's lines before `LoggedIn`, but their rooms are not in the
/// room list yet, so they colour nothing either.
pub(super) fn activity_level_for(
    activity: &RoomActivity,
    own_user_id: Option<&str>,
) -> Option<ViewActivityLevel> {
    if own_user_id.is_some_and(|me| room_activity_sender_user_id(activity) == Some(me)) {
        return None;
    }
    let level = match &activity.kind {
        RoomActivityKind::Message(_)
        | RoomActivityKind::Image(_)
        | RoomActivityKind::File(_)
        | RoomActivityKind::Audio(_)
        | RoomActivityKind::Video(_)
        | RoomActivityKind::Sticker(_)
        | RoomActivityKind::Poll(_)
        | RoomActivityKind::VerificationRequest(_)
        | RoomActivityKind::UnsupportedMessage(_)
        | RoomActivityKind::UnableToDecrypt(_) => ViewActivityLevel::Message,
        RoomActivityKind::StateNotice(_) | RoomActivityKind::Redacted(_) => {
            ViewActivityLevel::Event
        }
    };
    Some(if activity.highlight {
        ViewActivityLevel::Highlight
    } else {
        level
    })
}

/// Records activity in a view, keeping the higher level: mIRC never lowers
/// a window's colour until the user looks at it.
pub(super) fn raise_view_activity(
    state: &mut WindowState,
    view_id: ViewId,
    level: ViewActivityLevel,
) {
    let recorded = state.room_activity.entry(view_id).or_insert(level);
    *recorded = (*recorded).max(level);
}

pub(super) fn room_activity_event_id(activity: &RoomActivity) -> Option<&str> {
    match &activity.kind {
        RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some(event_id),
            ..
        }) => Some(event_id.as_str()),
        RoomActivityKind::Image(message) => Some(message.event_id.as_str()),
        RoomActivityKind::File(message) => Some(message.event_id.as_str()),
        RoomActivityKind::Audio(message) => Some(message.event_id.as_str()),
        RoomActivityKind::Video(message) => Some(message.event_id.as_str()),
        RoomActivityKind::Sticker(message) => Some(message.event_id.as_str()),
        RoomActivityKind::Poll(poll) => Some(poll.event_id.as_str()),
        RoomActivityKind::VerificationRequest(request) => Some(request.event_id.as_str()),
        RoomActivityKind::UnsupportedMessage(message) => Some(message.event_id.as_str()),
        RoomActivityKind::UnableToDecrypt(message) => Some(message.event_id.as_str()),
        RoomActivityKind::StateNotice(notice) => Some(notice.event_id.as_str()),
        RoomActivityKind::Redacted(activity) => Some(activity.event_id.as_str()),
        // A pending send has no event yet.
        RoomActivityKind::Message(RoomMessageActivity { event_id: None, .. }) => None,
    }
}

fn room_activity_thread_root_event_id(activity: &RoomActivity) -> Option<&str> {
    match &activity.relation {
        Some(RoomActivityRelation::Thread {
            thread_root_event_id,
            ..
        }) => Some(thread_root_event_id.as_str()),
        _ => None,
    }
}

fn room_activity_thread_root_label(activity: &RoomActivity) -> Option<String> {
    match &activity.kind {
        RoomActivityKind::Message(message) => Some(message.body.clone()),
        RoomActivityKind::Image(message) => Some(
            message
                .filename
                .as_deref()
                .unwrap_or(message.body.as_str())
                .to_owned(),
        ),
        RoomActivityKind::File(message) => Some(
            message
                .filename
                .as_deref()
                .unwrap_or(message.body.as_str())
                .to_owned(),
        ),
        RoomActivityKind::Audio(message) => Some(
            message
                .filename
                .as_deref()
                .unwrap_or(message.body.as_str())
                .to_owned(),
        ),
        RoomActivityKind::Video(message) => Some(
            message
                .filename
                .as_deref()
                .unwrap_or(message.body.as_str())
                .to_owned(),
        ),
        RoomActivityKind::Sticker(message) => Some(message.body.clone()),
        RoomActivityKind::Poll(poll) => Some(match &poll.content {
            RoomPollContent::Start { question, .. } => question.clone(),
            RoomPollContent::End { text, .. } => text.clone(),
        }),
        RoomActivityKind::VerificationRequest(_) => Some("Verification request".to_owned()),
        RoomActivityKind::UnsupportedMessage(message) => Some(message.body.clone()),
        RoomActivityKind::UnableToDecrypt(_) => Some("Unable to decrypt message".to_owned()),
        RoomActivityKind::StateNotice(_) => None,
        RoomActivityKind::Redacted(_) => Some("Message redacted".to_owned()),
    }
}

pub(super) fn room_activity_belongs_to_thread(
    activity: &RoomActivity,
    thread_root_event_id: &str,
) -> bool {
    room_activity_event_id(activity) == Some(thread_root_event_id)
        || room_activity_thread_root_event_id(activity) == Some(thread_root_event_id)
}

fn room_activity_view_id(room_id: &str, relation: Option<&RoomActivityRelation>) -> ViewId {
    match relation {
        Some(RoomActivityRelation::Thread {
            thread_root_event_id,
            ..
        }) => ViewId::Thread {
            room_id: room_id.to_owned(),
            thread_root_event_id: thread_root_event_id.clone(),
        },
        _ => ViewId::Room(room_id.to_owned()),
    }
}

pub(super) fn room_activity_target_view_id(activity: &RoomActivity) -> ViewId {
    room_activity_view_id(&activity.room_id, activity.relation.as_ref())
}

/// A message the server refused wants the user's attention; one waiting
/// for the connection is no news yet.
pub(super) fn room_activity_update_marks_inactive_room(update: &RoomActivityUpdate) -> bool {
    matches!(
        (&update.target, &update.kind),
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::MessageFailed {
                recoverable: false,
                ..
            }
        )
    )
}

/// Whether the message is still in the core's send queue: on its way, or
/// waiting for the connection to come back after a recoverable failure.
fn room_activity_is_queued(activity: &RoomActivity) -> bool {
    matches!(
        &activity.kind,
        RoomActivityKind::Message(RoomMessageActivity { send_state, .. }) if is_queued(send_state)
    )
}

pub(super) fn is_queued(send_state: &RoomMessageSendState) -> bool {
    matches!(
        send_state,
        RoomMessageSendState::Pending
            | RoomMessageSendState::Failed {
                recoverable: true,
                ..
            }
    )
}

pub(super) fn failed_room_activity_update_view_id(
    state: &WindowState,
    update: &RoomActivityUpdate,
) -> ViewId {
    let (
        RoomActivityUpdateTarget::Transaction { transaction_id },
        RoomActivityUpdateKind::MessageFailed { .. },
    ) = (&update.target, &update.kind)
    else {
        panic!("failed room activity marker target requires a failed activity update");
    };
    let room_view = state
        .model
        .room_views
        .get(&update.room_id)
        .expect("outgoing failed requires an existing queued timeline row");
    let pending_index = room_view
        .find_pending_message_activity_index(transaction_id)
        .expect("outgoing failed requires an existing queued timeline row");
    let pending_activity = room_view
        .activities
        .get(pending_index)
        .expect("pending room activity index must resolve");
    room_activity_target_view_id(pending_activity)
}

pub(super) fn read_target_for_room_activity(activity: &RoomActivity) -> Option<&str> {
    match &activity.kind {
        RoomActivityKind::Message(RoomMessageActivity { send_state, .. })
            if !matches!(send_state, RoomMessageSendState::Pending) =>
        {
            room_activity_event_id(activity)
        }
        RoomActivityKind::Message(_) => None,
        RoomActivityKind::Image(_)
        | RoomActivityKind::File(_)
        | RoomActivityKind::Audio(_)
        | RoomActivityKind::Video(_)
        | RoomActivityKind::Sticker(_)
        | RoomActivityKind::Poll(_)
        | RoomActivityKind::VerificationRequest(_)
        | RoomActivityKind::UnsupportedMessage(_)
        | RoomActivityKind::UnableToDecrypt(_)
        | RoomActivityKind::StateNotice(_)
        | RoomActivityKind::Redacted(_) => room_activity_event_id(activity),
    }
}

/// The message `event_id` names among the lines of the window `view_id`.
/// A redacted message is no longer one.
pub(super) fn window_message<'a>(
    model: &'a UiModel,
    view_id: &ViewId,
    event_id: &str,
) -> Option<&'a RoomActivity> {
    let room_view = model.room_views.get(view_id.room_id()?)?;
    let activity = &room_view.activities[room_view.find_activity_index_by_event_id(event_id)?];
    let in_window = view_id
        .thread_key()
        .is_none_or(|(_, thread_root_event_id)| {
            room_activity_belongs_to_thread(activity, thread_root_event_id)
        });
    (in_window && !matches!(activity.kind, RoomActivityKind::Redacted(_))).then_some(activity)
}

pub(super) fn is_joined_message_room(room: &RoomSummary) -> bool {
    room.membership == RoomMembership::Join && !room.is_space
}

pub(super) fn is_invited_room(room: &RoomSummary) -> bool {
    room.membership == RoomMembership::Invite && !room.is_space
}

pub(super) fn is_navigable_room(room: &RoomSummary) -> bool {
    is_joined_message_room(room) || is_invited_room(room)
}

/// What kind of mIRC window a navigable room is. The order is mIRC's
/// window order: channels, then queries; invites, which mIRC lacks, last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum RoomWindowKind {
    Channel,
    Query,
    Invite,
}

pub(super) fn room_window_kind(room: &RoomSummary) -> RoomWindowKind {
    assert!(
        is_navigable_room(room),
        "only navigable rooms have windows: {}",
        room.room_id
    );
    match room.membership {
        RoomMembership::Invite => RoomWindowKind::Invite,
        RoomMembership::Join if room.is_direct => RoomWindowKind::Query,
        RoomMembership::Join => RoomWindowKind::Channel,
    }
}

/// The navigable rooms in window order, each kind in model order.
pub(super) fn rooms_in_window_order(rooms: &[RoomSummary]) -> Vec<&RoomSummary> {
    let mut ordered = rooms
        .iter()
        .filter(|room| is_navigable_room(room))
        .collect::<Vec<_>>();
    ordered.sort_by_key(|room| room_window_kind(room));
    ordered
}

pub(super) fn is_sendable_room(room: &RoomSummary) -> bool {
    is_joined_message_room(room)
}

pub(super) fn is_invite_command_target(room: &RoomSummary) -> bool {
    is_invited_room(room)
}

pub(super) fn room_by_id<'a>(rooms: &'a [RoomSummary], room_id: &str) -> Option<&'a RoomSummary> {
    rooms.iter().find(|room| room.room_id == room_id)
}

pub(super) fn navigable_room_by_id<'a>(
    rooms: &'a [RoomSummary],
    room_id: &str,
) -> Option<&'a RoomSummary> {
    room_by_id(rooms, room_id).filter(|room| is_navigable_room(room))
}

pub(super) fn sendable_room_by_id<'a>(
    rooms: &'a [RoomSummary],
    room_id: &str,
) -> Option<&'a RoomSummary> {
    room_by_id(rooms, room_id).filter(|room| is_sendable_room(room))
}

/// A joined room's notification mode, which the core reports for every
/// joined room.
pub(super) fn joined_room_notification_mode(room: &RoomSummary) -> RoomNotificationMode {
    room.notification_mode
        .expect("a joined room has a notification mode")
}

pub(super) fn view_id_is_valid_for_rooms(
    rooms: &[RoomSummary],
    room_views: &RoomViews,
    view_id: &ViewId,
) -> bool {
    match view_id {
        ViewId::Status => true,
        ViewId::RoomList => unreachable!("the Rooms List has no activity"),
        ViewId::Room(room_id) => navigable_room_by_id(rooms, room_id).is_some(),
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => {
            sendable_room_by_id(rooms, room_id).is_some()
                && room_views
                    .get(room_id)
                    .is_some_and(|room_view| room_view.threads.contains_key(thread_root_event_id))
        }
    }
}

pub(super) fn projected_thread_by_id<'a>(
    model: &'a UiModel,
    room_id: &str,
    thread_root_event_id: &str,
) -> Option<&'a ThreadViewModel> {
    sendable_room_by_id(&model.rooms, room_id)?;
    model
        .room_views
        .get(room_id)?
        .threads
        .get(thread_root_event_id)
}

pub(super) fn view_id_is_valid(model: &UiModel, view_id: &ViewId) -> bool {
    match view_id {
        ViewId::Status => true,
        ViewId::RoomList => model.room_list.is_some(),
        ViewId::Room(room_id) => navigable_room_by_id(&model.rooms, room_id).is_some(),
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => projected_thread_by_id(model, room_id, thread_root_event_id).is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;

    #[test]
    fn room_views_bump_a_revision_only_on_mutable_access() {
        let mut views = RoomViews::default();
        views.insert("!a:example".to_owned(), RoomViewModel::default());
        views.entry_or_default("!b:example");
        let a = views.revision("!a:example").unwrap();
        let b = views.revision("!b:example").unwrap();
        assert_ne!(a, b);
        assert_eq!(views.revision("!c:example"), None);

        assert!(views.get("!a:example").is_some());
        let _ = views.iter().count() + views.values().count();
        assert_eq!(views.revision("!a:example"), Some(a));

        views.get_mut("!a:example").unwrap();
        let a2 = views.revision("!a:example").unwrap();
        assert!(a2 > a);
        views.entry_or_default("!a:example");
        let a3 = views.revision("!a:example").unwrap();
        assert!(a3 > a2);
        views.insert("!a:example".to_owned(), RoomViewModel::default());
        let a4 = views.revision("!a:example").unwrap();
        assert!(a4 > a3);
        assert_eq!(views.revision("!b:example"), Some(b));
        // A missing room moves no revision of another.
        assert!(views.get_mut("!c:example").is_none());
        assert_eq!(views.revision("!a:example"), Some(a4));

        // A room gone and back never repeats an earlier revision.
        views.clear();
        views.entry_or_default("!a:example");
        assert!(views.revision("!a:example").unwrap() > a4);
    }

    #[test]
    fn synthetic_event_validation_rejects_invalid_message_sent_sequence() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &RoomViews::default(),
                &[outgoing_sent("!ops:example", "txn-1", "$event:example.org")]
            ),
            Err("outgoing sent requires an existing queued timeline row".to_owned())
        );
    }

    #[test]
    fn synthetic_event_validation_accepts_pending_then_sent_sequence() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &RoomViews::default(),
                &[
                    outgoing_queued("!ops:example", "txn-1", "hello"),
                    outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
                ]
            ),
            Ok(())
        );
    }

    #[test]
    fn synthetic_event_validation_rejects_message_edit_with_transaction_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &RoomViews::default(),
                &[Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id: "!ops:example".to_owned(),
                    target: RoomActivityUpdateTarget::Transaction {
                        transaction_id: "txn-1".to_owned(),
                    },
                    kind: RoomActivityUpdateKind::MessageReplaced {
                        replacement_event_id: "$edit:example.org".to_owned(),
                        sender: "@alice:example".to_owned(),
                        timestamp_ms: 1_700_000_000_500,
                        presentation: RoomMessagePresentationKind::Text,
                        body: "edited".to_owned(),
                        styles: Vec::new(),
                        user_links: Vec::new(),
                        source_body: None,
                    },
                })]
            ),
            Err("message edit requires an event target".to_owned())
        );
    }

    #[test]
    fn synthetic_event_validation_rejects_message_edit_without_visible_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &RoomViews::default(),
                &[incoming_replaced(
                    "!ops:example",
                    "$missing:example.org",
                    "$edit:example.org",
                    RoomMessagePresentationKind::Text,
                    "edited"
                )]
            ),
            Err("message edit requires an existing visible target event".to_owned())
        );
    }

    #[test]
    fn redaction_synthetic_event_validation_rejects_missing_event_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &RoomViews::default(),
                &[incoming_redacted(
                    "!ops:example",
                    "$missing:example.org",
                    "$redaction:example.org",
                )]
            ),
            Err("activity redaction requires an existing visible target event".to_owned())
        );
    }

    #[test]
    fn redaction_synthetic_event_validation_rejects_transaction_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &RoomViews::default(),
                &[Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id: "!ops:example".to_owned(),
                    target: RoomActivityUpdateTarget::Transaction {
                        transaction_id: "txn-1".to_owned(),
                    },
                    kind: RoomActivityUpdateKind::ActivityRedacted {
                        redaction_event_id: "$redaction:example.org".to_owned(),
                    },
                })]
            ),
            Err("activity redacted update requires an event target".to_owned())
        );
    }

    #[test]
    fn reaction_add_synthetic_event_validation_rejects_transaction_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &RoomViews::default(),
                &[Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id: "!ops:example".to_owned(),
                    target: RoomActivityUpdateTarget::Transaction {
                        transaction_id: "txn-1".to_owned(),
                    },
                    kind: RoomActivityUpdateKind::ReactionAdded {
                        reaction_event_id: "$reaction:example.org".to_owned(),
                        sender: "@alice:example".to_owned(),
                        timestamp_ms: 1_700_000_000_500,
                        key: "+1".to_owned(),
                    },
                })]
            ),
            Err("reaction add requires an event target".to_owned())
        );
    }

    #[test]
    fn thread_labels_use_observed_root_body_when_available_and_fall_back_to_root_event_id() {
        let mut room_view = RoomViewModel::default();

        room_view.apply_activity(sent_message_activity_with_relation_and_presentation(
            "!ops:example",
            "$thread-1",
            None,
            Some(thread_relation("$thread-root")),
            RoomMessagePresentationKind::Text,
            "thread rep",
            1_700_000_120_000,
        ));
        assert_eq!(
            room_view
                .threads
                .get("$thread-root")
                .map(|thread| thread.label.as_str()),
            Some("$thread-root")
        );

        room_view.apply_activity(sent_message_activity(
            "!ops:example",
            "$thread-root",
            None,
            "root msg",
            1_700_000_060_000,
        ));
        assert_eq!(
            room_view
                .threads
                .get("$thread-root")
                .map(|thread| thread.label.as_str()),
            Some("root msg")
        );

        let mut room_view = RoomViewModel::default();
        room_view.apply_activity(sent_message_activity(
            "!ops:example",
            "$thread-root",
            None,
            "root msg",
            1_700_000_060_000,
        ));
        room_view.apply_activity(sent_message_activity_with_relation_and_presentation(
            "!ops:example",
            "$thread-2",
            None,
            Some(thread_relation("$thread-root")),
            RoomMessagePresentationKind::Text,
            "thread rep",
            1_700_000_120_000,
        ));
        assert_eq!(
            room_view
                .threads
                .get("$thread-root")
                .map(|thread| thread.label.as_str()),
            Some("root msg")
        );
    }

    #[test]
    fn replacing_thread_root_updates_thread_label() {
        let mut room_view = RoomViewModel::default();
        room_view.apply_activity(sent_message_activity(
            "!ops:example",
            "$thread-root",
            None,
            "old root",
            1_700_000_060_000,
        ));
        room_view.apply_activity(sent_message_activity_with_relation_and_presentation(
            "!ops:example",
            "$thread-1",
            None,
            Some(thread_relation("$thread-root")),
            RoomMessagePresentationKind::Text,
            "thread rep",
            1_700_000_120_000,
        ));

        assert!(room_view.apply_update(&edit_by(
            "!ops:example",
            "$thread-root",
            "$root-edit",
            "@alice:example",
            1_700_000_180_000,
            RoomMessagePresentationKind::Text,
            "edited root",
        )));
        assert_eq!(
            room_view
                .threads
                .get("$thread-root")
                .map(|thread| thread.label.as_str()),
            Some("edited root")
        );
    }

    fn event_room_activity(event: Event) -> RoomActivity {
        let Event::RoomActivity(activity) = event else {
            panic!("expected a room activity event, got {event:?}");
        };
        *activity
    }

    #[test]
    fn activity_level_for_maps_every_kind_and_highlight_overrides() {
        let message = sent_message_activity("!room:example", "$m", None, "hi", 1);
        let image = image_activity("!room:example", "$i", "@bob:example", 1, "i", None, None);
        let file = file_activity("!room:example", "$f", "@bob:example", 1, "f", None, None);
        let audio = audio_activity("!room:example", "$a", "@bob:example", 1, "a", None, None);
        let video = event_room_activity(video_message("!room:example", "$v"));
        let sticker = sticker_activity("!room:example", "$s", "@bob:example", 1, "s");
        let poll = poll_activity("!room:example", "$p", 1, poll_start("q", &["a", "b"]));
        let poll_ended = poll_activity("!room:example", "$pe", 1, poll_end("$p", "a won"));
        let verification = verification_request_activity("!room:example", "$v", 1, "@me:example");
        let unsupported = unsupported_message_activity(
            "!room:example",
            "$u",
            "@bob:example",
            1,
            "org.example.custom",
            "u",
        );
        let undecryptable = event_room_activity(unable_to_decrypt_activity(
            "!room:example",
            "$e",
            "@bob:example",
            1,
        ));
        let state_notice = event_room_activity(topic_state("!room:example", "topic"));
        let redacted = RoomActivity {
            kind: RoomActivityKind::Redacted(RoomRedactedActivity {
                event_id: "$r".to_owned(),
            }),
            ..message.clone()
        };

        for activity in [
            &message,
            &image,
            &file,
            &audio,
            &video,
            &sticker,
            &poll,
            &poll_ended,
            &verification,
            &unsupported,
            &undecryptable,
        ] {
            assert_eq!(
                activity_level_for(activity, None),
                Some(ViewActivityLevel::Message),
                "{activity:?}"
            );
        }
        for activity in [&state_notice, &redacted] {
            assert_eq!(
                activity_level_for(activity, None),
                Some(ViewActivityLevel::Event),
                "{activity:?}"
            );
        }
        for activity in [message, image, sticker, poll, state_notice, redacted] {
            assert_eq!(
                activity_level_for(&highlighted(activity), None),
                Some(ViewActivityLevel::Highlight)
            );
        }
    }

    #[test]
    fn the_last_activity_names_what_a_line_without_text_is() {
        let body = |activity: RoomActivity| {
            last_activity_from_room_activity(&activity)
                .expect("a line from someone")
                .body
        };
        assert_eq!(
            body(sticker_activity(
                "!room:example",
                "$s",
                "@bob:example",
                1,
                "a waving cat"
            )),
            "[sticker] a waving cat"
        );
        assert_eq!(
            body(poll_activity(
                "!room:example",
                "$p",
                1,
                poll_start("Lunch?", &["Pizza", "Sushi"])
            )),
            "[poll] Lunch?"
        );
        assert_eq!(
            body(poll_activity(
                "!room:example",
                "$pe",
                1,
                poll_end("$p", "Pizza won")
            )),
            "[poll ended] Pizza won"
        );
        assert_eq!(
            body(verification_request_activity(
                "!room:example",
                "$v",
                1,
                "@carol:example"
            )),
            "[verification request]"
        );
    }

    #[test]
    fn activity_level_for_ignores_our_own_lines() {
        let me = Some("@alice:example");
        let own_message = sent_message_activity("!room:example", "$m", None, "hi", 1);
        let own_pending = pending_message_activity("!room:example", "txn", "draft");
        let own_image = image_activity("!room:example", "$i", "@alice:example", 1, "i", None, None);
        let own_notice = event_room_activity(topic_state("!room:example", "topic"));
        let others_image =
            image_activity("!room:example", "$o", "@bob:example", 1, "o", None, None);
        let redacted = RoomActivity {
            kind: RoomActivityKind::Redacted(RoomRedactedActivity {
                event_id: "$r".to_owned(),
            }),
            ..own_message.clone()
        };

        for activity in [&own_message, &own_pending, &own_image, &own_notice] {
            assert_eq!(activity_level_for(activity, me), None, "{activity:?}");
        }
        // Sender first: the core never highlights our own line, but a
        // highlight must not make it ours to look at.
        assert_eq!(
            activity_level_for(&highlighted(own_message.clone()), me),
            None
        );
        assert_eq!(
            activity_level_for(&others_image, me),
            Some(ViewActivityLevel::Message)
        );
        assert_eq!(
            activity_level_for(&redacted, me),
            Some(ViewActivityLevel::Event)
        );
        // Not signed in: nothing is ours.
        assert_eq!(
            activity_level_for(&own_message, None),
            Some(ViewActivityLevel::Message)
        );
    }

    #[test]
    fn file_and_audio_room_activities_expose_event_ids_read_targets_and_thread_labels() {
        let file = file_activity(
            "!media:example.org",
            "$file1:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "Quarterly report",
            Some("report.pdf"),
            Some(RoomFileInfo {
                mimetype: Some("application/pdf".to_owned()),
                size: Some(45_678),
            }),
        );
        let audio = audio_activity(
            "!media:example.org",
            "$audio1:example.org",
            "@alice:example.org",
            1_710_000_060_000,
            "Voice note",
            Some("voice-note.ogg"),
            Some(RoomAudioInfo {
                mimetype: Some("audio/ogg".to_owned()),
                size: Some(23_456),
                duration_ms: Some(12_000),
            }),
        );

        assert_eq!(room_activity_event_id(&file), Some("$file1:example.org"));
        assert_eq!(
            read_target_for_room_activity(&file),
            Some("$file1:example.org")
        );
        assert_eq!(
            room_activity_thread_root_label(&file),
            Some("report.pdf".to_owned())
        );

        assert_eq!(room_activity_event_id(&audio), Some("$audio1:example.org"));
        assert_eq!(
            read_target_for_room_activity(&audio),
            Some("$audio1:example.org")
        );
        assert_eq!(
            room_activity_thread_root_label(&audio),
            Some("voice-note.ogg".to_owned())
        );
    }

    #[test]
    fn unsupported_room_activity_updates_last_activity_preview() {
        let activity = unsupported_message_activity(
            "!media:example.org",
            "$image1:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "com.example.custom",
            "clip.mp4",
        );

        assert_eq!(
            last_activity_from_room_activity(&activity),
            Some(LastMessageActivity {
                room_id: "!media:example.org".to_owned(),
                event_id: Some("$image1:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example.org".to_owned(),
                timestamp_ms: 1_710_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "[unsupported com.example.custom] clip.mp4".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn image_room_activity_updates_last_activity_preview() {
        let activity = image_activity(
            "!media:example.org",
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
        );

        assert_eq!(
            last_activity_from_room_activity(&activity),
            Some(LastMessageActivity {
                room_id: "!media:example.org".to_owned(),
                event_id: Some("$image1:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example.org".to_owned(),
                timestamp_ms: 1_710_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "[image] diagram.png".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn file_room_activity_updates_last_activity_preview() {
        let activity = file_activity(
            "!media:example.org",
            "$file1:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "Quarterly report",
            Some("report.pdf"),
            Some(RoomFileInfo {
                mimetype: Some("application/pdf".to_owned()),
                size: Some(45_678),
            }),
        );

        assert_eq!(
            last_activity_from_room_activity(&activity),
            Some(LastMessageActivity {
                room_id: "!media:example.org".to_owned(),
                event_id: Some("$file1:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example.org".to_owned(),
                timestamp_ms: 1_710_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "[file] report.pdf".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn audio_room_activity_updates_last_activity_preview() {
        let activity = audio_activity(
            "!media:example.org",
            "$audio1:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "Voice note",
            Some("voice-note.ogg"),
            Some(RoomAudioInfo {
                mimetype: Some("audio/ogg".to_owned()),
                size: Some(23_456),
                duration_ms: Some(12_000),
            }),
        );

        assert_eq!(
            last_activity_from_room_activity(&activity),
            Some(LastMessageActivity {
                room_id: "!media:example.org".to_owned(),
                event_id: Some("$audio1:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example.org".to_owned(),
                timestamp_ms: 1_710_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "[audio] voice-note.ogg".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    fn arrivals(room_view: &RoomViewModel) -> Vec<(&str, Option<u64>)> {
        room_view.assert_arrivals_consistent();
        room_view
            .activities
            .iter()
            .map(|activity| {
                (
                    activity.activity_id.as_str(),
                    room_view.arrivals.of(&activity.activity_id),
                )
            })
            .collect()
    }

    fn utd(event_id: &str) -> RoomActivity {
        let Event::RoomActivity(activity) =
            unable_to_decrypt_activity("!ops:example", event_id, "@bob:example", 1_700_000_000_000)
        else {
            unreachable!()
        };
        *activity
    }

    fn sent(event_id: &str, body: &str) -> RoomActivity {
        sent_message_activity("!ops:example", event_id, None, body, 1_700_000_000_000)
    }

    #[test]
    fn live_activities_arrive_in_order_and_history_does_not_arrive() {
        let mut room_view = RoomViewModel::default();
        room_view.apply_activity(sent("$a", "a"));
        room_view.apply_activity(utd("$b"));
        room_view.apply_history_backfill(
            "!ops:example",
            RoomHistoryBackfill {
                room_id: "!ops:example".to_owned(),
                end: None,
                events: vec![RoomHistoryBackfillEvent::Activity(Box::new(sent(
                    "$old", "old",
                )))],
            },
        );
        assert_eq!(
            arrivals(&room_view),
            [
                ("event:$old", None),
                ("event:$a", Some(0)),
                ("event:$b", Some(1))
            ]
        );
        assert_eq!(room_view.arrivals.next(), 2);
    }

    #[test]
    #[should_panic(expected = "arrived twice")]
    fn an_activity_arrives_once() {
        let mut room_view = RoomViewModel::default();
        room_view.apply_activity(utd("$b"));
        room_view.apply_activity(utd("$b"));
    }

    #[test]
    fn a_sent_message_keeps_the_arrival_of_its_pending_echo() {
        // Sent by update: the pending line becomes the event.
        let mut room_view = RoomViewModel::default();
        room_view.apply_activity(pending_message_activity("!ops:example", "t1", "hi"));
        room_view.apply_activity(sent("$other", "other"));
        let Event::RoomActivityUpdate(update) = outgoing_sent("!ops:example", "t1", "$hi") else {
            unreachable!()
        };
        assert!(room_view.apply_update(&update));
        assert_eq!(
            arrivals(&room_view),
            [("event:$hi", Some(0)), ("event:$other", Some(1))]
        );

        // Sent by update after its synced copy arrived: the copy goes.
        let mut room_view = RoomViewModel::default();
        room_view.apply_activity(pending_message_activity("!ops:example", "t1", "hi"));
        room_view.apply_activity(sent("$hi", "hi"));
        let Event::RoomActivityUpdate(update) = outgoing_sent("!ops:example", "t1", "$hi") else {
            unreachable!()
        };
        assert!(room_view.apply_update(&update));
        assert_eq!(arrivals(&room_view), [("event:$hi", Some(0))]);

        // Replaced by the synced event carrying its transaction.
        let mut room_view = RoomViewModel::default();
        room_view.apply_activity(pending_message_activity("!ops:example", "t1", "hi"));
        room_view.apply_activity(sent("$other", "other"));
        room_view.apply_activity(sent_message_activity(
            "!ops:example",
            "$hi",
            Some("t1"),
            "hi",
            1_700_000_000_000,
        ));
        assert_eq!(
            arrivals(&room_view),
            [("event:$hi", Some(0)), ("event:$other", Some(1))]
        );
        assert_eq!(room_view.arrivals.next(), 2);
    }

    #[test]
    fn clear_keeps_only_the_pending_arrivals_and_echo_shares_the_count() {
        let mut room_view = RoomViewModel::default();
        room_view.apply_activity(sent("$a", "a"));
        room_view.push_echo(
            1_700_000_000_000,
            None,
            StatusLineKind::Info,
            "* echo".to_owned(),
        );
        room_view.apply_activity(pending_message_activity("!ops:example", "t1", "hi"));
        room_view.clear_visible_entries();
        assert_eq!(arrivals(&room_view), [("txn:t1", Some(2))]);
        room_view.push_echo(
            1_700_000_000_000,
            None,
            StatusLineKind::Info,
            "* after".to_owned(),
        );
        assert_eq!(
            room_view
                .echo
                .lines_for(None)
                .map(|line| line.arrival)
                .collect::<Vec<_>>(),
            [3]
        );
        assert_eq!(room_view.arrivals.next(), 4);
    }

    /// Message `n`, sent at minute `n`.
    fn line(n: u64) -> RoomActivity {
        sent_message_activity(
            "!ops:example",
            &format!("$m{n}"),
            None,
            &format!("line {n}"),
            1_700_000_000_000 + n * 60_000,
        )
    }

    fn lines_of(room_view: &RoomViewModel) -> Vec<String> {
        room_view.assert_arrivals_consistent();
        let mut lines = room_view
            .activities
            .iter()
            .map(|activity| (activity.timestamp_ms, activity.activity_id.clone()))
            .chain(
                room_view
                    .echo
                    .lines()
                    .iter()
                    .map(|line| (line.timestamp_ms, line.text.clone())),
            )
            .collect::<Vec<_>>();
        lines.sort();
        lines.into_iter().map(|(_, line)| line).collect()
    }

    fn room_with_lines(count: u64) -> RoomViewModel {
        let mut room_view = RoomViewModel::default();
        for n in 0..count {
            room_view.apply_activity(line(n));
        }
        room_view
    }

    /// Trims as the window buffer does: whether anything went.
    fn trim(room_view: &mut RoomViewModel, cap: usize, open_threads: &BTreeSet<String>) -> bool {
        let trim = room_view.buffer_trim(cap, open_threads);
        if let Some(trim) = trim {
            room_view.trim_buffer(trim);
        }
        trim.is_some()
    }

    fn before_event(event_id: &str) -> RoomHistoryBackfillState {
        RoomHistoryBackfillState::Idle {
            next: HistoryFrom::BeforeEvent {
                event_id: event_id.to_owned(),
            },
        }
    }

    #[test]
    fn the_buffer_drops_the_oldest_lines_with_echo_lines_in_window_order() {
        let mut room_view = room_with_lines(4);
        // Between lines 1 and 2, and after 3.
        room_view.push_echo(
            1_700_000_000_000 + 90_000,
            None,
            StatusLineKind::Info,
            "* echo a".to_owned(),
        );
        room_view.push_echo(
            1_700_000_000_000 + 200_000,
            None,
            StatusLineKind::Info,
            "* echo b".to_owned(),
        );
        assert_eq!(room_view.buffer_lines(), 6);

        assert!(!trim(&mut room_view, 6, &BTreeSet::new()));
        assert_eq!(room_view.history, RoomHistoryBackfillState::default());

        assert!(trim(&mut room_view, 3, &BTreeSet::new()));
        assert_eq!(room_view.buffer_lines(), 3);
        assert_eq!(lines_of(&room_view), ["event:$m2", "event:$m3", "* echo b"]);
        assert_eq!(room_view.history, before_event("$m2"));
        assert_eq!(room_view.arrivals.of("event:$m1"), None);
        assert_eq!(room_view.arrivals.of("event:$m2"), Some(2));

        // Only echo lines go: history stays where it was.
        let mut room_view = RoomViewModel {
            history: RoomHistoryBackfillState::Exhausted,
            ..RoomViewModel::default()
        };
        room_view.push_echo(1, None, StatusLineKind::Info, "* echo".to_owned());
        assert!(trim(&mut room_view, 0, &BTreeSet::new()));
        assert_eq!(room_view.buffer_lines(), 0);
        assert_eq!(room_view.history, RoomHistoryBackfillState::Exhausted);
    }

    #[test]
    fn a_queued_message_or_an_open_threads_line_ends_the_trim() {
        let mut room_view = room_with_lines(2);
        room_view.apply_activity(pending_message_activity("!ops:example", "t1", "queued"));
        for n in 2..5 {
            room_view.apply_activity(line(n));
        }
        assert!(trim(&mut room_view, 1, &BTreeSet::new()));
        assert_eq!(
            lines_of(&room_view)[..2],
            ["txn:t1".to_owned(), "event:$m2".to_owned()]
        );
        assert_eq!(room_view.buffer_lines(), 4);

        let mut room_view = room_with_lines(2);
        let mut reply = line(2);
        reply.relation = Some(thread_relation("$m1"));
        room_view.apply_activity(reply);
        room_view.push_echo(
            1_700_000_000_000 + 30_000,
            Some("$m1".to_owned()),
            StatusLineKind::Info,
            "* thread echo".to_owned(),
        );
        for n in 3..6 {
            room_view.apply_activity(line(n));
        }
        let open = BTreeSet::from(["$m1".to_owned()]);
        // The thread's echo line comes right after line 0.
        assert!(trim(&mut room_view, 2, &open));
        assert_eq!(lines_of(&room_view)[0], "* thread echo");
        assert_eq!(room_view.history, before_event("$m1"));
        room_view.echo.drop_oldest(1);
        // Its root stays, and its reply.
        assert!(!trim(&mut room_view, 2, &open));
        assert_eq!(lines_of(&room_view)[..2], ["event:$m1", "event:$m2"]);
        // Closed, the thread's lines go too.
        assert!(trim(&mut room_view, 2, &BTreeSet::new()));
        assert_eq!(lines_of(&room_view), ["event:$m4", "event:$m5"]);
        assert!(room_view.threads.is_empty());
    }

    fn redaction_of(event_id: &str) -> RoomActivityUpdate {
        let Event::RoomActivityUpdate(update) =
            incoming_redacted("!ops:example", event_id, "$redaction")
        else {
            unreachable!()
        };
        update
    }

    #[test]
    fn a_redaction_of_an_unknown_event_changes_nothing() {
        let mut room_view = room_with_lines(2);
        assert!(room_view.add_reaction("$m0", "$r0", "@bob:example", 1, "+1"));
        let before = room_view.clone();

        assert_eq!(
            room_view.validate_update(&redaction_of("$elsewhere")),
            Err("activity redaction requires an existing visible target event".to_owned())
        );
        assert!(!room_view.apply_update(&redaction_of("$elsewhere")));

        assert_eq!(lines_of(&room_view), lines_of(&before));
        assert_eq!(room_view.reactions_by_target, before.reactions_by_target);
    }

    #[test]
    fn a_redaction_validates_against_a_shown_line_or_reaction() {
        let mut room_view = room_with_lines(1);
        assert!(room_view.add_reaction("$m0", "$r0", "@bob:example", 1, "+1"));
        assert_eq!(room_view.validate_update(&redaction_of("$m0")), Ok(()));
        assert_eq!(room_view.validate_update(&redaction_of("$r0")), Ok(()));

        // A redacted line takes its reactions along.
        assert!(room_view.apply_update(&redaction_of("$m0")));
        assert!(room_view.reactions_by_target.is_empty());
        assert!(room_view.validate_update(&redaction_of("$r0")).is_err());
    }

    #[test]
    fn a_trimmed_line_takes_its_reactions_edits_and_media_along() {
        let mut room_view = room_with_lines(3);
        assert!(room_view.add_reaction("$m0", "$r0", "@bob:example", 1, "+1"));
        assert!(room_view.add_reaction("$m2", "$r2", "@bob:example", 1, "+1"));
        assert!(room_view.apply_update(&edit_by(
            "!ops:example",
            "$m0",
            "$e0",
            "@alice:example",
            1,
            RoomMessagePresentationKind::Text,
            "edited",
        )));
        for activity_id in ["event:$m0", "event:$m2"] {
            room_view.insert_preview_media(
                activity_id.to_owned(),
                RoomPreviewMediaState::Failed {
                    event_id: "$x".to_owned(),
                    format: RoomMediaFormat::File,
                    message: "gone".to_owned(),
                },
            );
        }
        assert!(trim(&mut room_view, 2, &BTreeSet::new()));
        assert_eq!(
            room_view.reactions_by_target.keys().collect::<Vec<_>>(),
            ["$m2"]
        );
        assert!(room_view.line_edits.is_empty());
        assert_eq!(
            room_view.preview_media.keys().collect::<Vec<_>>(),
            ["event:$m2"]
        );
    }

    fn quoted_body(room_view: &RoomViewModel, event_id: &str) -> String {
        let ReplyTarget::Known(activity) = room_view.reply_target(event_id) else {
            panic!("{event_id} is not known");
        };
        let RoomActivityKind::Message(message) = &activity.kind else {
            panic!("{event_id} is no message: {activity:?}");
        };
        message.body.clone()
    }

    /// A room whose reply (`$m2`) quotes `$m0`, which the window buffer
    /// dropped after alice edited it (`$e0`): its quote keeps the edit.
    fn room_quoting_its_trimmed_edited_line() -> RoomViewModel {
        let mut room_view = room_with_lines(2);
        let mut reply = line(2);
        reply.relation = Some(reply_relation("$m0"));
        room_view.apply_activity(reply);
        room_view.apply_activity(line(3));
        assert!(room_view.apply_update(&edit_by(
            "!ops:example",
            "$m0",
            "$e0",
            "@alice:example",
            1,
            RoomMessagePresentationKind::Text,
            "edited",
        )));
        assert!(trim(&mut room_view, 2, &BTreeSet::new()));
        assert_eq!(quoted_body(&room_view, "$m0"), "edited");
        room_view
    }

    #[test]
    fn a_trimmed_line_kept_as_a_quote_keeps_its_edits() {
        let mut room_view = room_quoting_its_trimmed_edited_line();
        assert!(room_view.line_edits.shows_edited("$m0"));

        // Deleting the edit gives the quote back the message's own text.
        assert!(room_view.apply_update(&redaction_of("$e0")));
        assert_eq!(quoted_body(&room_view, "$m0"), "line 0");
        assert!(room_view.line_edits.is_empty());

        // A target nothing quotes any more goes with its edits.
        let mut room_view = room_quoting_its_trimmed_edited_line();
        assert!(trim(&mut room_view, 1, &BTreeSet::new()));
        assert!(room_view.reply_targets.is_empty());
        assert!(room_view.line_edits.is_empty());
    }

    /// A quote follows its message's edits and deletion, as a line does;
    /// it shows no reactions.
    #[test]
    fn edits_and_redactions_of_a_quote_target_validate_but_reactions_do_not() {
        let room_view = room_quoting_its_trimmed_edited_line();
        let Event::RoomActivityUpdate(reaction) =
            incoming_reaction_added("!ops:example", "$m0", "$r0", "@bob:example", "+1", 1)
        else {
            unreachable!()
        };
        assert_eq!(
            room_view.validate_update(&edit_by(
                "!ops:example",
                "$m0",
                "$e1",
                "@alice:example",
                2,
                RoomMessagePresentationKind::Text,
                "again",
            )),
            Ok(())
        );
        assert_eq!(room_view.validate_update(&redaction_of("$m0")), Ok(()));
        assert_eq!(room_view.validate_update(&redaction_of("$e0")), Ok(()));
        assert!(room_view.validate_update(&reaction).is_err());
    }

    #[test]
    fn a_kept_reply_keeps_quoting_its_trimmed_target_until_it_goes_too() {
        let mut room_view = room_with_lines(2);
        let mut reply = line(2);
        reply.relation = Some(reply_relation("$m0"));
        room_view.apply_activity(reply);
        room_view.apply_activity(line(3));
        room_view
            .reply_targets
            .insert("$elsewhere".to_owned(), ReplyTargetState::Failed);

        assert!(trim(&mut room_view, 2, &BTreeSet::new()));
        assert!(matches!(
            room_view.reply_target("$m0"),
            ReplyTarget::Known(activity) if *activity == line(0)
        ));
        // Nothing quotes the other target, nor line 1.
        assert_eq!(room_view.reply_targets.keys().collect::<Vec<_>>(), ["$m0"]);

        assert!(trim(&mut room_view, 1, &BTreeSet::new()));
        assert!(room_view.reply_targets.is_empty());
    }

    #[test]
    fn a_thread_whose_root_was_trimmed_keeps_its_name() {
        let mut room_view = room_with_lines(1);
        let mut reply = line(1);
        reply.relation = Some(thread_relation("$m0"));
        room_view.apply_activity(reply);
        assert_eq!(room_view.threads["$m0"].label, "line 0");

        assert!(trim(&mut room_view, 1, &BTreeSet::new()));
        assert_eq!(room_view.threads["$m0"].label, "line 0");
        // And so it does after /clear.
        room_view.clear_visible_entries();
        assert!(room_view.threads.is_empty());
    }

    #[test]
    fn a_window_left_with_no_server_line_loads_the_newest_page() {
        let mut room_view = room_with_lines(2);
        room_view.apply_activity(pending_message_activity("!ops:example", "t1", "queued"));
        assert!(trim(&mut room_view, 1, &BTreeSet::new()));
        assert_eq!(lines_of(&room_view), ["txn:t1"]);
        assert_eq!(room_view.history, RoomHistoryBackfillState::default());
    }

    #[test]
    #[should_panic(expected = "a room trims only between history pages")]
    fn a_room_loading_history_is_not_trimmed() {
        let mut room_view = room_with_lines(3);
        room_view.history = RoomHistoryBackfillState::Loading {
            from: HistoryFrom::Latest,
        };
        room_view.trim_buffer(room_view.buffer_trim(1, &BTreeSet::new()).unwrap());
    }

    #[test]
    fn a_closed_window_has_shown_everything_that_arrived() {
        let mut room_view = RoomViewModel::default();
        assert_eq!(room_view.arrivals.unshown_from(), 0);
        room_view.apply_activity(sent("$a", "a"));
        room_view.push_echo(
            1_700_000_000_000,
            None,
            StatusLineKind::Info,
            "* echo".to_owned(),
        );
        room_view.arrivals.window_closed();
        assert_eq!(room_view.arrivals.unshown_from(), 2);
        room_view.apply_activity(sent("$b", "b"));
        assert_eq!(room_view.arrivals.unshown_from(), 2);
        assert_eq!(room_view.arrivals.of("event:$b"), Some(2));
    }
}
