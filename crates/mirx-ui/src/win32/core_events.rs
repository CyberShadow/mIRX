//! Applying core events to the window state, then refreshing the UI.

use super::open_link::open_web_link;
use super::*;

fn apply_room_event_fetch(state: &mut WindowState, fetch: RoomEventFetch) {
    // The room was left while the fetch ran: no window shows its replies.
    let Some(room_view) = state.model.room_views.get_mut(&fetch.room_id) else {
        return;
    };
    let target = match fetch.result {
        RoomEventFetchResult::Loaded { activity } => ReplyTargetState::Loaded(activity),
        RoomEventFetchResult::Failed { .. } => ReplyTargetState::Failed,
    };
    room_view.reply_targets.insert(fetch.event_id, target);
}

/// Returns whether the activity should flash the frame. The active view gets
/// no activity colour, but it can still want attention while mIRC is in the
/// background.
fn apply_room_activity_event(state: &mut WindowState, activity: RoomActivity) -> ActivityAttention {
    let clears_last_activity = matches!(activity.kind, RoomActivityKind::Redacted(_));
    let target_view_id = room_activity_target_view_id(&activity);
    let mut attention = ActivityAttention::default();
    if let Some(level) = activity_level_for(&activity, state.model.own_user_id.as_deref())
        && let Some((room_is_direct, mode)) =
            sendable_room_by_id(&state.model.rooms, &activity.room_id)
                .map(|room| (room.is_direct, joined_room_notification_mode(room)))
    {
        // A replayed event load is not news.
        if !state.synthetic_load_in_progress {
            attention.flash = notifications::activity_wants_attention(level, room_is_direct, mode);
            attention.tip = notifications::activity_wants_tip(
                level,
                room_is_direct,
                mode,
                &state.settings.settings().tips,
            )
            .then(|| TipCandidate {
                view: target_view_id.clone(),
                activity_id: activity.activity_id.clone(),
            });
        }
        // A closed query pops up again when the other side talks, as in
        // mIRC, muted or not; the next view sync recreates its window.
        if level >= ViewActivityLevel::Message
            && let ViewId::Room(room_id) = &target_view_id
        {
            state.closed_queries.remove(room_id);
        }
        if state.active_view != target_view_id {
            // The room's mode caps its threads' activity too.
            let level = notifications::room_activity_level(level, mode);
            raise_view_activity(state, target_view_id.clone(), level);
            if !state.synthetic_load_in_progress
                && let Some(event_id) = read_target_for_room_activity(&activity)
            {
                state
                    .room_activity_read_targets
                    .insert(target_view_id, event_id.to_owned());
            }
        }
    }
    let room_id = activity.room_id.clone();
    let last_activity = last_activity_from_room_activity(&activity);
    state
        .model
        .room_views
        .entry(room_id)
        .or_default()
        .apply_activity(activity);
    if let Some(last_activity) = last_activity {
        state.model.last_activity = Some(last_activity);
    } else if clears_last_activity {
        state.model.last_activity = None;
    }
    attention
}

/// What a line asks of the user's attention: a taskbar flash, and a tip
/// about it.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct ActivityAttention {
    pub(super) flash: bool,
    pub(super) tip: Option<TipCandidate>,
}

impl ActivityAttention {
    /// A batch's: it flashes if any line does, and tips about its last
    /// line that wants one (one tip per batch; Windows queues no more).
    fn then(self, later: Self) -> Self {
        Self {
            flash: self.flash || later.flash,
            tip: later.tip.or(self.tip),
        }
    }
}

fn apply_room_activity_update_event(state: &mut WindowState, update: RoomActivityUpdate) {
    let target_view_id = room_activity_update_marks_inactive_room(&update)
        .then(|| failed_room_activity_update_view_id(state, &update));
    if let Some(target_view_id) = target_view_id
        && state.active_view != target_view_id
        && sendable_room_by_id(&state.model.rooms, &update.room_id).is_some()
    {
        raise_view_activity(state, target_view_id, ViewActivityLevel::Message);
    }
    let room_id = update.room_id.clone();
    let refresh_last_activity = state
        .model
        .last_activity
        .as_ref()
        .is_some_and(|last_activity| match (&update.target, &update.kind) {
            (
                RoomActivityUpdateTarget::Transaction { transaction_id },
                RoomActivityUpdateKind::MessageSent { event_id },
            ) => {
                last_activity.room_id == room_id
                    && (last_activity.transaction_id.as_deref() == Some(transaction_id.as_str())
                        || last_activity.event_id.as_deref() == Some(event_id.as_str()))
            }
            (
                RoomActivityUpdateTarget::Transaction { transaction_id },
                RoomActivityUpdateKind::MessageFailed { .. },
            ) => {
                last_activity.room_id == room_id
                    && last_activity.transaction_id.as_deref() == Some(transaction_id.as_str())
            }
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::MessageReplaced { .. },
            ) => {
                last_activity.room_id == room_id
                    && last_activity.event_id.as_deref() == Some(event_id.as_str())
            }
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ActivityRedacted { .. },
            ) => {
                last_activity.room_id == room_id
                    && last_activity.event_id.as_deref() == Some(event_id.as_str())
            }
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ReactionAdded { .. },
            )
            | (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ReactionRemoved { .. },
            ) => {
                last_activity.room_id == room_id
                    && last_activity.event_id.as_deref() == Some(event_id.as_str())
            }
            _ => false,
        });
    match (&update.target, &update.kind) {
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::MessageSent { .. },
        ) => state
            .model
            .room_views
            .entry(room_id.clone())
            .or_default()
            .mark_message_sent(update.clone()),
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::MessageFailed { .. },
        ) => state
            .model
            .room_views
            .entry(room_id.clone())
            .or_default()
            .mark_message_failed(update.clone()),
        (
            RoomActivityUpdateTarget::Event { event_id },
            RoomActivityUpdateKind::MessageReplaced {
                presentation,
                body,
                styles,
                user_links,
                ..
            },
        ) => {
            let _ = state
                .model
                .room_views
                .entry(room_id.clone())
                .or_default()
                .replace_message(
                    event_id,
                    MessageReplacementFields {
                        presentation: *presentation,
                        body: body.clone(),
                        styles: styles.clone(),
                        user_links: user_links.clone(),
                    },
                );
        }
        (
            RoomActivityUpdateTarget::Event { event_id },
            RoomActivityUpdateKind::ActivityRedacted { .. },
        ) => {
            let redacted = state
                .model
                .room_views
                .entry(room_id.clone())
                .or_default()
                .redact_activity(event_id);
            if redacted {
                prune_decoded_images(state);
            }
        }
        (
            RoomActivityUpdateTarget::Event { event_id },
            RoomActivityUpdateKind::ReactionAdded {
                reaction_event_id,
                sender,
                timestamp_ms,
                key,
            },
        ) => {
            let _ = state
                .model
                .room_views
                .entry(room_id.clone())
                .or_default()
                .add_reaction(event_id, reaction_event_id, sender, *timestamp_ms, key);
        }
        (
            RoomActivityUpdateTarget::Event { event_id },
            RoomActivityUpdateKind::ReactionRemoved {
                reaction_event_id,
                key,
                ..
            },
        ) => {
            let _ = state
                .model
                .room_views
                .entry(room_id.clone())
                .or_default()
                .remove_reaction(event_id, reaction_event_id, key);
        }
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::MessageReplaced { .. },
        ) => panic!("message edit requires an event target"),
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::ActivityRedacted { .. },
        ) => panic!("activity redacted update requires an event target"),
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::ReactionAdded { .. },
        ) => panic!("reaction add requires an event target"),
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::ReactionRemoved { .. },
        ) => panic!("reaction removal requires an event target"),
        (RoomActivityUpdateTarget::Event { .. }, RoomActivityUpdateKind::MessageSent { .. }) => {
            panic!("message sent update requires a transaction target")
        }
        (RoomActivityUpdateTarget::Event { .. }, RoomActivityUpdateKind::MessageFailed { .. }) => {
            panic!("message failed update requires a transaction target")
        }
    }
    if refresh_last_activity
        && let Some(updated_activity) =
            state
                .model
                .room_views
                .get(&room_id)
                .and_then(|room_view| match &update.target {
                    RoomActivityUpdateTarget::Transaction { transaction_id } => room_view
                        .find_message_activity_index(Some(transaction_id.as_str()), None)
                        .and_then(|index| room_view.activities.get(index)),
                    RoomActivityUpdateTarget::Event { event_id } => room_view
                        .find_activity_index_by_event_id(event_id.as_str())
                        .and_then(|index| room_view.activities.get(index)),
                })
    {
        state.model.last_activity = last_activity_from_room_activity(updated_activity);
    }
}

fn apply_room_history_backfill_event(state: &mut WindowState, backfill: RoomHistoryBackfill) {
    let room_id = backfill.room_id.clone();
    let end = backfill.end.clone();
    let room_view = state.model.room_views.entry(room_id.clone()).or_default();
    room_view.apply_history_backfill(&room_id, backfill);
    room_view.history.next_token = end.clone();
    room_view.history.status = match end {
        Some(_) => RoomHistoryBackfillStatus::Idle,
        None => RoomHistoryBackfillStatus::Exhausted,
    };
    prune_decoded_images(state);
}

/// The room's history load failed. An error for a room that is not loading
/// (it was left, or the user cleared it meanwhile) changes nothing.
fn apply_backfill_room_history_error(state: &mut WindowState, room_id: &str, message: String) {
    let Some(room_view) = state.model.room_views.get_mut(room_id) else {
        return;
    };
    let RoomHistoryBackfillStatus::Loading { from } = &room_view.history.status else {
        return;
    };
    let from = from.clone();
    room_view.history.status = RoomHistoryBackfillStatus::Failed {
        from: from.clone(),
        message,
    };
}

fn apply_room_read_state_event(state: &mut WindowState, read_state: RoomReadState) {
    let room_id = read_state.room_id.clone();
    let Some(mode) =
        sendable_room_by_id(&state.model.rooms, &room_id).map(joined_room_notification_mode)
    else {
        return;
    };
    state
        .model
        .room_read_states
        .insert(room_id.clone(), read_state.clone());

    let latest_active_event_id = read_state.latest_active_event_id;
    let fully_read_event_id = read_state.fully_read_event_id;
    let marked_unread = read_state.marked_unread;
    let num_unread = read_state.num_unread;
    let num_notifications = read_state.num_notifications;
    let num_mentions = read_state.num_mentions;

    let has_unread_state =
        marked_unread || num_unread > 0 || num_notifications > 0 || num_mentions > 0;
    let clear_read_target = !has_unread_state
        || state
            .room_activity_read_targets
            .get(&ViewId::Room(room_id.clone()))
            .is_some_and(|target| {
                Some(target.as_str()) == latest_active_event_id.as_deref()
                    || Some(target.as_str()) == fully_read_event_id.as_deref()
            });
    if clear_read_target {
        state
            .room_activity_read_targets
            .remove(&ViewId::Room(room_id.clone()));
    }

    // The server's counts decide: no unread state clears even event-only
    // activity, mentions are highlights.
    let room_view_id = ViewId::Room(room_id.clone());
    let room_is_active = state.active_view == room_view_id;
    if has_unread_state && !room_is_active {
        let level = if num_mentions > 0 {
            ViewActivityLevel::Highlight
        } else {
            ViewActivityLevel::Message
        };
        raise_view_activity(
            state,
            room_view_id,
            notifications::room_activity_level(level, mode),
        );
    } else {
        state.room_activity.remove(&room_view_id);
    }
}

fn apply_room_typing_event(state: &mut WindowState, typing: RoomTyping) {
    state
        .model
        .room_views
        .entry(typing.room_id.clone())
        .or_default()
        .replace_typing(typing);
}

fn apply_room_members_event(state: &mut WindowState, members: RoomMembers) {
    state
        .model
        .room_views
        .entry(members.room_id.clone())
        .or_default()
        .replace_members(members);
}

pub(super) unsafe fn handle_core_events(hwnd: HWND) {
    if let Err(message) = drain_core_events_and_refresh(hwnd) {
        state_mut(hwnd).exit_code.store(1, Ordering::Relaxed);
        eprintln!("{message}");
        DestroyWindow(hwnd);
    }
}

/// Returns whether the event should flash the frame.
pub(super) fn apply_core_event(
    state: &mut WindowState,
    event: Event,
) -> Result<ActivityAttention, String> {
    state.model.connection = connection::after_event(state.model.connection, &event);
    match event {
        Event::Started => {
            if matches!(
                state.model.lifecycle_state,
                LifecycleState::Offline | LifecycleState::CoreStarted
            ) {
                state.model.status = "core started".to_owned();
                state.model.lifecycle_state = LifecycleState::CoreStarted;
            }
        }
        Event::LoggedIn {
            session,
            homeserver_url,
        } => {
            if let Some(own_user_id) = &state.model.own_user_id
                && *own_user_id != session.user_id
            {
                return Err(format!(
                    "received LoggedIn for {} while signed in as {own_user_id}",
                    session.user_id
                ));
            }
            // A replayed event load must not overwrite the saved session.
            if !state.synthetic_load_in_progress {
                save_session(&homeserver_url, &session)?;
                state.current_session_homeserver_url = Some(homeserver_url);
            }
            log_status(
                state,
                StatusLineKind::Info,
                status_log::logged_in(&session.user_id, &session.device_id),
            );
            state.model.own_user_id = Some(session.user_id);
            state.model.status = "authenticated".to_owned();
            state.model.lifecycle_state = LifecycleState::Authenticated;
        }
        Event::SignInUrl { url } => {
            log_status(
                state,
                StatusLineKind::Info,
                status_log::SIGN_IN_WITH_BROWSER.to_owned(),
            );
            log_status(
                state,
                StatusLineKind::Info,
                status_log::sign_in_address(&url),
            );
            state.model.status = "waiting for browser sign-in".to_owned();
        }
        Event::LoginCancelled => {
            log_status(
                state,
                StatusLineKind::Info,
                status_log::SIGN_IN_CANCELLED.to_owned(),
            );
            state.model.status = "sign-in cancelled".to_owned();
            state.model.lifecycle_state = LifecycleState::LoggedOut;
        }
        Event::SessionUpdated { session } => {
            let homeserver_url = state
                .current_session_homeserver_url
                .as_ref()
                .expect("session updates require an active homeserver URL");
            save_session(homeserver_url, &session)?;
        }
        Event::RestoredSession => {
            log_status(
                state,
                StatusLineKind::Info,
                status_log::SESSION_RESTORED.to_owned(),
            );
            state.model.status = "restored saved session".to_owned();
            state.model.lifecycle_state = LifecycleState::Authenticated;
        }
        Event::SyncedRooms(rooms) => {
            // The core re-sends the room list on every change; only the
            // first one after connecting is news.
            if state.model.lifecycle_state != LifecycleState::Ready {
                let invited = rooms.iter().filter(|room| is_invited_room(room)).count();
                log_status(
                    state,
                    StatusLineKind::Info,
                    status_log::synced(rooms.len() - invited, invited),
                );
            }
            state.model.status = format!("synced {} room(s)", rooms.len());
            state.model.lifecycle_state = LifecycleState::Ready;
            if state
                .model
                .selected_room
                .as_deref()
                .is_some_and(|room_id| navigable_room_by_id(&rooms, room_id).is_none())
            {
                state.model.selected_room = None;
            }
            state.room_activity.retain(|view_id, _| {
                view_id_is_valid_for_rooms(&rooms, &state.model.room_views, view_id)
            });
            state.closed_queries.retain(|room_id| {
                navigable_room_by_id(&rooms, room_id)
                    .is_some_and(|room| room_window_kind(room) == RoomWindowKind::Query)
            });
            let tracked_room_activity = state.room_activity.clone();
            state
                .room_activity_read_targets
                .retain(|view_id, _| tracked_room_activity.contains_key(view_id));
            for (room_id, room_view) in &mut state.model.room_views {
                if sendable_room_by_id(&rooms, room_id).is_none() {
                    room_view.typing_user_ids.clear();
                    room_view.members.clear();
                }
            }
            state
                .model
                .room_read_states
                .retain(|room_id, _| sendable_room_by_id(&rooms, room_id).is_some());
            for room in rooms
                .iter()
                .filter(|room| room.membership == RoomMembership::Join && !room.is_space)
            {
                state
                    .model
                    .room_views
                    .entry(room.room_id.clone())
                    .or_default()
                    .topic = room.topic.clone();
            }
            state.model.rooms = rooms;
        }
        Event::RoomActivity(activity) => return Ok(apply_room_activity_event(state, *activity)),
        Event::RoomActivityUpdate(update) => apply_room_activity_update_event(state, update),
        Event::RoomHistoryBackfill(backfill) => apply_room_history_backfill_event(state, backfill),
        Event::RoomMedia(media) => apply_room_media_event(state, media),
        Event::RoomEvent(fetch) => apply_room_event_fetch(state, fetch),
        Event::PublicRooms(page) => apply_public_rooms_page(state, page)?,
        Event::RoomJoined(joined) => apply_room_joined_event(state, &joined.room_id)?,
        Event::RoomTyping(typing) => apply_room_typing_event(state, typing),
        Event::RoomMembers(members) => apply_room_members_event(state, members),
        Event::RoomReadState(read_state) => apply_room_read_state_event(state, read_state),
        Event::HighlightWords(words) => state.model.highlight_words = Some(words),
        Event::IgnoredUsers(ignored_users) => {
            // The first list after signing in is no change.
            if let Some(old) = &state.model.ignored_users {
                let lines = ignore::change_lines(old, &ignored_users, |user_id| {
                    known_nick(&state.model, user_id)
                });
                echo_lines_in_active_view(state, lines);
            }
            state.model.ignored_users = Some(ignored_users);
        }
        Event::UserProfile(profile) => {
            let lines = whois_lines(&state.model, &profile);
            echo_lines_in_active_view(state, lines);
        }
        Event::E2eeStatus(status) => {
            for line in status_log::e2ee_status(&status) {
                log_status(state, StatusLineKind::Info, line);
            }
            state.model.last_e2ee_status = Some(status);
            state.model.status = "e2ee status updated".to_owned();
        }
        Event::E2eeRecoverySucceeded(_) => {
            log_status(
                state,
                StatusLineKind::Info,
                status_log::E2EE_RECOVERY_COMPLETE.to_owned(),
            );
            state.model.status = "e2ee recovery complete".to_owned();
        }
        Event::E2eeRecoverySetupSucceeded(_) => {
            log_status(
                state,
                StatusLineKind::Info,
                status_log::E2EE_RECOVERY_SET_UP.to_owned(),
            );
            state.model.last_e2ee_recovery_setup_result = true;
            state.model.status = "e2ee recovery setup complete".to_owned();
        }
        Event::VerificationRequest(request) => {
            apply_verification_event(state, verification_state_event_from_request(request));
        }
        Event::VerificationState(event) => apply_verification_event(state, event),
        Event::LoggedOut => {
            delete_saved_session()?;
            state.current_session_homeserver_url = None;
            state.model.own_user_id = None;
            state.model.status = "logged out".to_owned();
            state.model.lifecycle_state = LifecycleState::LoggedOut;
            state.model.rooms.clear();
            state.closed_queries.clear();
            state.model.selected_room = None;
            state.model.room_views.clear();
            state.model.last_activity = None;
            state.model.last_e2ee_status = None;
            state.model.last_e2ee_recovery_setup_result = false;
            state.model.last_verification = None;
            state.model.last_error = None;
            state.model.room_read_states.clear();
            state.model.room_list = None;
            state.model.highlight_words = None;
            state.model.ignored_users = None;
            state.room_activity.clear();
            state.room_activity_read_targets.clear();
            state.find_states.clear();
            state.decoded_images.clear();
            log_status(
                state,
                StatusLineKind::Info,
                status_log::LOGGED_OUT.to_owned(),
            );
        }
        Event::Stopped => {
            log_status(state, StatusLineKind::Info, status_log::STOPPED.to_owned());
            state.model.status = "core stopped".to_owned();
            state.model.lifecycle_state = LifecycleState::Stopped;
        }
        Event::SyncStarted => {
            log_status(
                state,
                StatusLineKind::Info,
                status_log::SYNC_STARTED.to_owned(),
            );
            state.model.status = "continuous sync started".to_owned();
            state.model.lifecycle_state = LifecycleState::Syncing;
        }
        Event::SyncStopped(reason) => {
            let (kind, line) = status_log::disconnected(&reason);
            log_status(state, kind, line);
            state.model.status = "continuous sync stopped".to_owned();
        }
        Event::Error(error) => {
            if error.operation == Operation::BackfillRoomHistory {
                let room_id = error
                    .room_id
                    .as_deref()
                    .expect("a backfill error names its room");
                apply_backfill_room_history_error(state, room_id, error.message.clone());
            }
            let line = status_log::error(operation_name(error.operation), &error.message);
            let now = state.clock.now_ms();
            match error_destination(&error) {
                ErrorDestination::ActiveWindow { command } => {
                    let error = CommandError::new(command, error.message.clone());
                    echo_lines_in_active_view(state, [error.to_string()]);
                }
                ErrorDestination::Room(room_id) => match state.model.room_views.get_mut(room_id) {
                    Some(room_view) => room_view.push_echo(now, None, context_line(&line)),
                    // The room was left, or never synced.
                    None => log_status(state, StatusLineKind::Error, line),
                },
                ErrorDestination::Status => log_status(state, StatusLineKind::Error, line),
            }
            if !preserves_submission_status(&state.model.status, error.operation) {
                state.model.status = format!("error during {}", operation_name(error.operation));
            }
            state.model.lifecycle_state = LifecycleState::Error;
            state.model.last_error = Some(error);
        }
    }
    Ok(ActivityAttention::default())
}

/// A join the user asked for opens the room's window and makes it active,
/// as `/join` does in mIRC; the next view sync realizes it. A space has no
/// window, so the Status window notes the join instead.
fn apply_room_joined_event(state: &mut WindowState, room_id: &str) -> Result<(), String> {
    // The core sends the snapshot listing the room right before.
    let room = state
        .model
        .rooms
        .iter()
        .find(|room| room.room_id == room_id)
        .ok_or_else(|| format!("joined room {room_id} is not in the room list"))?;
    if room.is_space {
        let line = status_log::joined_space(&room.display_name);
        log_status(state, StatusLineKind::Info, line);
        return Ok(());
    }
    let view_id = ViewId::Room(room_id.to_owned());
    state.closed_queries.remove(room_id);
    state.model.selected_room = Some(room_id.to_owned());
    if state.active_view != view_id {
        state.selected_timeline_event = None;
        state.active_view = view_id;
    }
    Ok(())
}

fn preserves_submission_status(status: &str, operation: Operation) -> bool {
    match operation {
        Operation::JoinRoom => status.starts_with("join submitted: "),
        Operation::AcceptRoomInvite => status.starts_with("accept submitted: "),
        Operation::CreateDirectRoom => status.starts_with("query submitted: "),
        Operation::CreateRoom => status.starts_with("create submitted: "),
        Operation::LeaveRoom => status.starts_with("part submitted: "),
        Operation::RejectRoomInvite => status.starts_with("reject submitted: "),
        Operation::SetRoomTopic => status.starts_with("topic change submitted for "),
        _ => false,
    }
}

unsafe fn drain_core_events(hwnd: HWND) -> Vec<Event> {
    let queue = &state_ref(hwnd).event_queue;
    let mut queue = queue.lock().expect("core event queue poisoned");
    queue.drain(..).collect()
}

pub(super) unsafe fn apply_events_and_refresh(
    hwnd: HWND,
    events: Vec<Event>,
) -> Result<(), String> {
    if events.is_empty() {
        return Ok(());
    }

    let mut history_backfill_anchor = None;
    let mut history_backfill_views = Vec::new();
    let mut backfill_result = false;
    let mut failed_backfill_room_id = None;
    let mut attention = ActivityAttention::default();
    for event in events {
        if let Event::E2eeRecoverySetupSucceeded(result) = &event {
            show_e2ee_recovery_key(hwnd, &result.recovery_key)?;
        }
        let state = state_mut(hwnd);
        if history_backfill_anchor.is_none()
            && let Event::RoomHistoryBackfill(backfill) = &event
        {
            history_backfill_anchor =
                capture_room_history_backfill_anchor(state, &backfill.room_id);
        }
        let backfilled_room_id = match &event {
            Event::RoomHistoryBackfill(backfill) => Some(backfill.room_id.clone()),
            Event::Error(error) if error.operation == Operation::BackfillRoomHistory => {
                let room_id = error
                    .room_id
                    .clone()
                    .expect("a backfill error names its room");
                failed_backfill_room_id = Some(room_id.clone());
                Some(room_id)
            }
            _ => None,
        };
        if let Some(backfilled_room_id) = &backfilled_room_id {
            history_backfill_views.extend(
                pending_timeline_find_history_views(state)
                    .into_iter()
                    .filter(|view_id| match view_id {
                        ViewId::Room(room_id) | ViewId::Thread { room_id, .. } => {
                            room_id == backfilled_room_id
                        }
                        ViewId::Status | ViewId::RoomList => false,
                    }),
            );
        }
        backfill_result |= backfilled_room_id.is_some();
        // A replayed event load opens nothing.
        let sign_in_url = match &event {
            Event::SignInUrl { url } if !state.synthetic_load_in_progress => Some(url.clone()),
            _ => None,
        };
        attention = attention.then(apply_core_event(state, event)?);
        state.event_generation += 1;
        if let Some(url) = sign_in_url {
            open_web_link(hwnd, url)?;
        }
    }

    sync_views_and_tree(hwnd)?;
    if attention.flash {
        request_frame_attention(hwnd);
    }
    // The rows exist now, so the tip can show the line as the window does.
    if let Some(tip) = attention.tip {
        show_tip(hwnd, tip)?;
    }
    if let Some(anchor) = history_backfill_anchor {
        restore_room_history_backfill_anchor(hwnd, anchor);
    }
    history_backfill_views.sort();
    history_backfill_views.dedup();
    for view_id in history_backfill_views {
        continue_timeline_find_history_search_after_refresh(hwnd, &view_id)?;
    }
    // After the anchor, so the jump's scroll wins.
    if backfill_result {
        continue_original_jump(hwnd, failed_backfill_room_id.as_deref())?;
    }
    Ok(())
}

/// Flashes the taskbar button until the user brings mIRC to the foreground;
/// Windows stops the flash on activation.
unsafe fn request_frame_attention(hwnd: HWND) {
    if GetForegroundWindow() == hwnd {
        return;
    }
    let info = FLASHWINFO {
        cbSize: size_of::<FLASHWINFO>() as u32,
        hwnd,
        dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
        uCount: 0,
        dwTimeout: 0,
    };
    // The return value is the window's previous active state, not an error.
    FlashWindowEx(&info);
}

unsafe fn apply_synthetic_events_and_refresh(hwnd: HWND, events: Vec<Event>) -> Result<(), String> {
    state_mut(hwnd).synthetic_load_in_progress = true;
    let result = apply_events_and_refresh(hwnd, events);
    state_mut(hwnd).synthetic_load_in_progress = false;
    result
}

pub(super) unsafe fn apply_event_load(
    hwnd: HWND,
    events: Vec<Event>,
) -> Result<(usize, usize), String> {
    {
        let state = state_mut(hwnd);
        state.current_session_homeserver_url = None;
        state.room_activity.clear();
        state.closed_queries.clear();
        state.room_activity_read_targets.clear();
        state.find_states.clear();
        state.selected_timeline_event = None;
        state.model = initial_model();
        state.decoded_images.clear();
    }

    apply_synthetic_events_and_refresh(hwnd, events)?;

    let (rooms, timeline_events, active_view) = {
        let state = state_mut(hwnd);
        let rooms = state.model.rooms.len();
        let timeline_events = state
            .model
            .room_views
            .values()
            .map(|room_view| room_view.activities.len())
            .sum();
        let selected_room = state
            .model
            .rooms
            .iter()
            .find(|room| is_navigable_room(room))
            .map(|room| room.room_id.clone());

        let active_view = match selected_room {
            Some(room_id) => {
                state.model.selected_room = Some(room_id.clone());
                ViewId::Room(room_id)
            }
            None => {
                state.model.selected_room = None;
                ViewId::Status
            }
        };
        state.active_view = active_view.clone();

        (rooms, timeline_events, active_view)
    };

    sync_views_and_tree(hwnd)?;
    activate_view(hwnd, &active_view, None)?;

    Ok((rooms, timeline_events))
}

pub(super) unsafe fn drain_core_events_and_refresh(hwnd: HWND) -> Result<usize, String> {
    let events = drain_core_events(hwnd);
    let event_count = events.len();
    apply_events_and_refresh(hwnd, events).map(|()| event_count)
}

fn submit_mark_room_read(
    state: &mut WindowState,
    room_id: &str,
    event_id: String,
) -> Result<(), String> {
    let command = Command::MarkRoomRead(MarkRoomReadRequest {
        room_id: room_id.to_owned(),
        event_id,
    });
    let Some(sender) = state.command_sender.as_ref() else {
        return Ok(());
    };
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue mark room read command: {err}"))?;
    #[cfg(test)]
    state.recorded_commands.push(command);
    Ok(())
}

pub(super) fn clear_view_activity_marker(
    state: &mut WindowState,
    view_id: &ViewId,
) -> (bool, Option<String>) {
    let read_target = state.room_activity_read_targets.remove(view_id);
    let cleared_view_activity = state.room_activity.remove(view_id).is_some();
    let read_target = if cleared_view_activity {
        read_target
    } else {
        None
    };
    (cleared_view_activity, read_target)
}

pub(super) fn handle_view_activation(
    state: &mut WindowState,
    view_id: &ViewId,
) -> Result<bool, String> {
    let (cleared_view_activity, read_target) = clear_view_activity_marker(state, view_id);
    if let Some(event_id) = read_target {
        let room_id = view_id
            .room_id()
            .expect("room activity markers must target room or thread views");
        submit_mark_room_read(state, room_id, event_id)?;
    }
    Ok(cleared_view_activity)
}

pub(super) fn resolve_timeline_room_id(
    state: &WindowState,
    explicit_room_id: Option<&str>,
) -> Result<String, String> {
    let room_id = match explicit_room_id {
        Some(room_id) => room_id.to_owned(),
        None => state
            .model
            .selected_room
            .clone()
            .ok_or_else(|| "no room selected".to_owned())?,
    };

    if navigable_room_by_id(&state.model.rooms, &room_id).is_some() {
        Ok(room_id)
    } else {
        Err(format!("unknown room id: {room_id}"))
    }
}

pub(super) fn operation_name(operation: Operation) -> &'static str {
    match operation {
        Operation::Login => "login",
        Operation::RestoreSession => "restore session",
        Operation::SessionUpdate => "session update",
        Operation::StartSync => "start sync",
        Operation::StopSync => "stop sync",
        Operation::JoinRoom => "join room",
        Operation::AcceptRoomInvite => "accept room invitation",
        Operation::CreateDirectRoom => "create direct room",
        Operation::CreateRoom => "create room",
        Operation::LeaveRoom => "leave room",
        Operation::RejectRoomInvite => "reject room invitation",
        Operation::KickRoomMember => "kick room member",
        Operation::BanRoomMember => "ban room member",
        Operation::UnbanRoomMember => "unban room member",
        Operation::InviteRoomMember => "invite room member",
        Operation::SetRoomMemberPowerLevel => "set room member power level",
        Operation::SetDisplayName => "set display name",
        Operation::SetRoomTopic => "set room topic",
        Operation::SetRoomNotificationMode => "set room notifications",
        Operation::HighlightWords => "highlight words",
        Operation::AddHighlightWord => "add highlight word",
        Operation::RemoveHighlightWord => "remove highlight word",
        Operation::IgnoredUsers => "ignore list",
        Operation::IgnoreUser => "ignore user",
        Operation::UnignoreUser => "unignore user",
        Operation::FetchUserProfile => "fetch user profile",
        Operation::SendMessage => "send text",
        Operation::SendFile => "send file",
        Operation::EditMessage => "edit message",
        Operation::SendReaction => "send reaction",
        Operation::RedactEvent => "redact event",
        Operation::MarkRoomRead => "mark room read",
        Operation::BackfillRoomHistory => "backfill room history",
        Operation::FetchRoomMedia => "fetch room image media",
        Operation::FetchRoomEvent => "fetch message",
        Operation::ListPublicRooms => "list rooms",
        Operation::RoomList => "room list",
        Operation::RoomReadState => "room read state",
        Operation::SendQueue => "send queue",
        Operation::E2eeStatus => "e2ee status",
        Operation::RecoverE2ee => "e2ee recovery",
        Operation::SetupE2eeRecovery => "e2ee recovery setup",
        Operation::Verification => "verification",
        Operation::Logout => "logout",
        Operation::Shutdown => "shutdown",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use mirx_core::SyncStopReason;

    use super::super::test_fixtures::*;
    use super::*;

    const SIGN_IN_URL: &str = "https://account.example/oauth2/authorize?state=x";

    #[test]
    fn a_browser_sign_in_logs_its_page_and_can_be_cancelled() {
        let mut state = test_state();
        state.model.connection = ConnectionState::SigningIn;

        apply_core_event(
            &mut state,
            Event::SignInUrl {
                url: SIGN_IN_URL.to_owned(),
            },
        )
        .unwrap();
        assert_eq!(state.model.connection, ConnectionState::WaitingForBrowser);
        assert_eq!(state.model.status, "waiting for browser sign-in");
        assert_eq!(
            status_log_lines(&state)[..],
            [
                "[22:12] * Sign in on the page your browser opened; File > Disconnect cancels"
                    .to_owned(),
                format!("[22:12] * If no browser opened, go to {SIGN_IN_URL}"),
            ]
        );

        apply_core_event(&mut state, Event::LoginCancelled).unwrap();
        assert_eq!(state.model.connection, ConnectionState::SignedOut);
        assert_eq!(state.model.status, "sign-in cancelled");
        assert_eq!(
            status_log_lines(&state).last().map(String::as_str),
            Some("[22:12] * Sign-in cancelled")
        );
    }

    #[test]
    fn a_replayed_sign_in_page_is_not_opened() {
        unsafe {
            with_test_frame(600, 400, |hwnd| {
                apply_event_load(
                    hwnd,
                    vec![
                        Event::Started,
                        Event::SignInUrl {
                            url: SIGN_IN_URL.to_owned(),
                        },
                    ],
                )
                .unwrap();
                let state = state_ref(hwnd);
                assert_eq!(state.model.connection, ConnectionState::WaitingForBrowser);
                assert_eq!(state.opened_links, Vec::<String>::new());
            });
        }
    }

    #[test]
    fn core_events_move_the_connection_state() {
        with_local_app_data(|_| {
            let mut state = test_state();
            let (sender, _commands) = inert_command_sender();
            state.command_sender = Some(sender);
            submit_restore_session(
                &mut state,
                "https://matrix.example/".to_owned(),
                stored_session(),
                true,
            )
            .unwrap();
            assert_eq!(state.model.connection, ConnectionState::SigningIn);
            for (event, connection) in [
                (Event::RestoredSession, ConnectionState::SignedIn),
                (
                    Event::Error(CoreErrorEvent {
                        operation: Operation::RestoreSession,
                        room_id: None,
                        message: "initial Matrix sync failed".to_owned(),
                    }),
                    ConnectionState::SignedIn,
                ),
                (Event::SyncStarted, ConnectionState::Syncing),
                (
                    Event::Error(CoreErrorEvent {
                        operation: Operation::StartSync,
                        room_id: None,
                        message: "continuous sync is already running".to_owned(),
                    }),
                    ConnectionState::Syncing,
                ),
                (
                    Event::SyncStopped(SyncStopReason::Requested),
                    ConnectionState::SignedIn,
                ),
                (Event::LoggedOut, ConnectionState::SignedOut),
            ] {
                apply_core_event(&mut state, event.clone()).unwrap();
                assert_eq!(state.model.connection, connection, "{event:?}");
            }
        });
    }

    fn outgoing_queued_with_presentation(
        room_id: &str,
        transaction_id: &str,
        presentation: RoomMessagePresentationKind,
        body: &str,
    ) -> Event {
        Event::RoomActivity(Box::new(pending_message_activity_with_presentation(
            room_id,
            transaction_id,
            presentation,
            body,
        )))
    }

    fn outgoing_failed(room_id: &str, transaction_id: &str, message: &str) -> Event {
        Event::RoomActivityUpdate(RoomActivityUpdate {
            room_id: room_id.to_owned(),
            target: RoomActivityUpdateTarget::Transaction {
                transaction_id: transaction_id.to_owned(),
            },
            kind: RoomActivityUpdateKind::MessageFailed {
                message: message.to_owned(),
                recoverable: true,
            },
        })
    }

    #[test]
    fn room_activity_events_accumulate_per_room_in_order() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!one:example", "One", false),
                room("!two:example", "Two", true),
            ]),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!one:example", "first")).unwrap();
        apply_core_event(&mut state, timeline("!one:example", "second")).unwrap();
        apply_core_event(&mut state, timeline("!two:example", "third")).unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!one:example")
                .map(|view| &view.activities),
            Some(&vec![
                sent_message_activity(
                    "!one:example",
                    "$!one:example:first",
                    None,
                    "first",
                    1_700_000_000_000,
                ),
                sent_message_activity(
                    "!one:example",
                    "$!one:example:second",
                    None,
                    "second",
                    1_700_000_000_000,
                ),
            ])
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!two:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity(
                "!two:example",
                "$!two:example:third",
                None,
                "third",
                1_700_000_000_000,
            )])
        );
        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!two:example".to_owned(),
                event_id: Some("$!two:example:third".to_owned()),
                transaction_id: None,
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "third".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn pending_room_activity_creates_one_pending_row_keyed_by_transaction_id() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![pending_message_activity_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves"
            )])
        );
        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: None,
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                presentation: RoomMessagePresentationKind::Emote,
                body: "waves".to_owned(),
                send_state: RoomMessageSendState::Pending,
            })
        );
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] * @alice:example waves (sending)".to_owned(),
            ]
        );
    }

    #[test]
    fn room_activity_update_sent_updates_existing_row_and_clears_pending_marker() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                "hello",
                1_700_000_000_000,
            )])
        );
        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: Some("$event:example.org".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "hello".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] <@alice:example> hello".to_owned(),
            ]
        );
    }

    #[test]
    fn room_activity_update_replaced_updates_existing_row_in_place() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                None,
                RoomMessagePresentationKind::Text,
                "original body",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$event:example.org",
                "$edit:example.org",
                RoomMessagePresentationKind::Emote,
                "edited body",
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity_with_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                RoomMessagePresentationKind::Emote,
                "edited body",
                1_700_000_000_000,
            )])
        );
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] * @alice:example edited body (edited)".to_owned(),
            ]
        );
    }

    #[test]
    fn room_activity_update_replaced_refreshes_last_activity_only_for_target() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$older:example.org",
                None,
                None,
                RoomMessagePresentationKind::Text,
                "older body",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$latest:example.org",
                None,
                None,
                RoomMessagePresentationKind::Text,
                "latest body",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$older:example.org",
                "$older-edit:example.org",
                RoomMessagePresentationKind::Text,
                "older edited",
            ),
        )
        .unwrap();

        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: Some("$latest:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                presentation: RoomMessagePresentationKind::Text,
                body: "latest body".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );

        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$latest:example.org",
                "$latest-edit:example.org",
                RoomMessagePresentationKind::Notice,
                "latest edited",
            ),
        )
        .unwrap();

        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: Some("$latest:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                presentation: RoomMessagePresentationKind::Notice,
                body: "latest edited".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn redaction_room_activity_update_replaces_text_message_with_tombstone() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                None,
                RoomMessagePresentationKind::Text,
                "secret body",
                1_700_000_000_000,
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

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        assert!(matches!(
            room_view.activities.as_slice(),
            [RoomActivity {
                activity_id,
                kind: RoomActivityKind::Redacted(RoomRedactedActivity { event_id }),
                ..
            }] if activity_id == "event:$event:example.org" && event_id == "$event:example.org"
        ));
        assert_eq!(state.model.last_activity, None);

        let rendered = render_room_lines(&state, "!ops:example").join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("secret body"));
    }

    #[test]
    fn redaction_room_activity_update_overrides_edited_message_and_blocks_late_replacement() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                None,
                RoomMessagePresentationKind::Text,
                "original body",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$event:example.org",
                "$edit:example.org",
                RoomMessagePresentationKind::Emote,
                "edited body",
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
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$event:example.org",
                "$late-edit:example.org",
                RoomMessagePresentationKind::Text,
                "late body",
            ),
        )
        .unwrap();

        let rendered = render_room_lines(&state, "!ops:example").join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("original body"));
        assert!(!rendered.contains("edited body"));
        assert!(!rendered.contains("late body"));
    }

    #[test]
    fn direct_message_edit_without_target_leaves_visible_timeline_unchanged() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        let before = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist")
            .activities
            .clone();

        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$missing:example.org",
                "$edit:example.org",
                RoomMessagePresentationKind::Text,
                "edited",
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&before)
        );
    }

    #[test]
    fn redaction_runtime_unknown_event_target_leaves_visible_timeline_unchanged() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        let before_activities = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist")
            .activities
            .clone();
        let before_last_activity = state.model.last_activity.clone();

        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$missing:example.org",
                "$redaction:example.org",
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&before_activities)
        );
        assert_eq!(state.model.last_activity, before_last_activity);
    }

    #[test]
    fn typed_sent_message_reconciles_by_transaction_id_without_duplicate_rows() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id_with_presentation(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_000_100,
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity_with_presentation(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_000_100,
            )])
        );
    }

    #[test]
    fn synced_message_before_sent_update_removes_duplicate_by_event_id_fallback() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id_with_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| view.activities.len()),
            Some(1)
        );
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] * @alice:example waves".to_owned(),
            ]
        );
        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: Some("$event:example.org".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                presentation: RoomMessagePresentationKind::Emote,
                body: "waves".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn synced_message_without_transaction_id_collapses_into_outgoing_row_after_sent_update() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id_with_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity_with_presentation(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_000_100,
            )])
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| view.activities.len()),
            Some(1)
        );
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] * @alice:example waves".to_owned(),
            ]
        );
    }

    #[test]
    fn failed_message_update_keeps_visible_failed_row_and_error_details() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_failed("!ops:example", "txn-1", "send failed"),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![failed_message_activity_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
                "send failed",
            )])
        );
        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: None,
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                presentation: RoomMessagePresentationKind::Emote,
                body: "waves".to_owned(),
                send_state: RoomMessageSendState::Failed {
                    message: "send failed".to_owned(),
                    recoverable: true,
                },
            })
        );
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] * @alice:example waves (failed)".to_owned(),
            ]
        );
    }

    #[test]
    fn failed_room_activity_loaded_directly_renders_failed_row() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            failed_message_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
                "send failed",
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![failed_message_activity_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
                "send failed",
            )])
        );
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] * @alice:example waves (failed)".to_owned(),
            ]
        );
    }

    #[test]
    fn clear_room_timeline_preserves_pending_room_activity_for_sent_update() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "first")).unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();

        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view must exist after timeline update")
            .clear_visible_entries();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![pending_message_activity(
                "!ops:example",
                "txn-1",
                "hello"
            )])
        );

        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                "hello",
                1_700_000_000_000,
            )])
        );
    }

    #[test]
    fn clear_room_timeline_removes_unsupported_placeholders() {
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
        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();

        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view must exist after timeline update")
            .clear_visible_entries();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![pending_message_activity(
                "!ops:example",
                "txn-1",
                "hello"
            )])
        );
    }

    #[test]
    fn clear_room_timeline_removes_image_rows() {
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
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();

        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view must exist after timeline update")
            .clear_visible_entries();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![pending_message_activity(
                "!ops:example",
                "txn-1",
                "hello"
            )])
        );
    }

    #[test]
    #[should_panic(expected = "outgoing sent requires an existing queued timeline row")]
    fn room_activity_update_sent_without_existing_queued_row_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "outgoing sent requires an existing queued timeline row")]
    fn room_activity_update_sent_after_sent_row_with_same_transaction_id_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                "hello",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "outgoing sent requires an existing queued timeline row")]
    fn room_activity_update_sent_after_failed_row_with_same_transaction_id_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            failed_message("!ops:example", "txn-1", "hello", "send failed"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "outgoing failed requires an existing queued timeline row")]
    fn room_activity_update_failed_without_existing_queued_row_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_failed("!ops:example", "txn-1", "send failed"),
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "outgoing failed requires an existing queued timeline row")]
    fn room_activity_update_failed_after_sent_row_with_same_transaction_id_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                "hello",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_failed("!ops:example", "txn-1", "send failed"),
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "outgoing failed requires an existing queued timeline row")]
    fn room_activity_update_failed_after_failed_row_with_same_transaction_id_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            failed_message("!ops:example", "txn-1", "hello", "send failed"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_failed("!ops:example", "txn-1", "send failed again"),
        )
        .unwrap();
    }

    #[test]
    fn room_typing_replaces_prior_user_list_and_empty_list_clears_it() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();
        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| view.typing_user_ids.clone()),
            Some(vec!["@alice:example.org".to_owned()])
        );

        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org", "@bob:example.org"]),
        )
        .unwrap();
        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| view.typing_user_ids.clone()),
            Some(vec![
                "@alice:example.org".to_owned(),
                "@bob:example.org".to_owned(),
            ])
        );

        apply_core_event(&mut state, typing_event("!ops:example", &[])).unwrap();
        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| view.typing_user_ids.clone()),
            Some(Vec::new())
        );
    }

    #[test]
    fn room_typing_does_not_change_last_activity_or_room_activity_markers() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        state.active_view = ViewId::Status;

        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();

        assert_eq!(state.model.last_activity, None);
        assert!(state.room_activity.is_empty());
        assert_eq!(state.model.status, "offline");
    }

    #[test]
    fn verification_request_event_stores_requested_state_and_logs_actions() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            Event::VerificationRequest(VerificationRequest {
                flow_id: "flow-verify-123".to_owned(),
                user_id: "@alice:example".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(
            state.model.last_verification,
            Some(VerificationStateEvent {
                flow_id: "flow-verify-123".to_owned(),
                user_id: "@alice:example".to_owned(),
                state: VerificationState::Requested,
            })
        );
        assert_eq!(
            status_log_lines(&state),
            vec![
                "[22:12] * Verification requested by @alice:example (flow flow-verify-123)"
                    .to_owned(),
                "[22:12] * Actions: Tools > Accept Verification, Tools > Cancel Verification, /verify accept, /verify cancel"
                    .to_owned(),
            ]
        );
    }
    #[test]
    fn verification_sas_ready_and_errors_are_logged_to_status() {
        let mut state = test_state();
        apply_core_event(
            &mut state,
            Event::VerificationState(VerificationStateEvent {
                flow_id: "flow-verify-123".to_owned(),
                user_id: "@alice:example".to_owned(),
                state: VerificationState::SasReady {
                    emojis: vec![
                        VerificationEmoji {
                            symbol: "🐶".to_owned(),
                            description: "dog".to_owned(),
                        },
                        VerificationEmoji {
                            symbol: "🔥".to_owned(),
                            description: "fire".to_owned(),
                        },
                    ],
                    decimals: mirx_core::VerificationDecimals {
                        first: 1234,
                        second: 5678,
                        third: 9012,
                    },
                },
            }),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::SendMessage,
                room_id: Some("!left:example".to_owned()),
                message: "message send failed".to_owned(),
            }),
        )
        .unwrap();

        let rows = build_status_timeline_rows(&state.model.status_log, state.timestamps());
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "[22:12] * Verification with @alice:example: compare the emoji or the numbers"
                    .to_owned(),
                "[22:12] * Emoji: 🐶 dog | 🔥 fire".to_owned(),
                "[22:12] * Decimals: 1234-5678-9012".to_owned(),
                "[22:12] * Actions: Tools > Confirm SAS, Tools > Reject SAS, Tools > Cancel Verification, /verify confirm, /verify reject, /verify cancel"
                    .to_owned(),
                "[22:12] * Error during send text: message send failed".to_owned(),
            ]
        );
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            [vec![TimelineRowKind::Info; 4], vec![TimelineRowKind::Error],].concat()
        );
    }
    #[test]
    fn e2ee_status_event_updates_model_and_logs_status() {
        let mut state = test_state();
        let status = e2ee_status(
            E2eeRecoveryState::Incomplete,
            E2eeBackupState::Enabled,
            false,
            Some(true),
            Some(false),
        );

        apply_core_event(&mut state, Event::E2eeStatus(status.clone())).unwrap();

        assert_eq!(state.model.last_e2ee_status, Some(status));
        assert_eq!(state.model.status, "e2ee status updated");
        assert_eq!(
            status_log_lines(&state),
            vec![
                "[22:12] * E2EE: recovery incomplete, local backup disabled, server backup exists, cross-signing incomplete"
                    .to_owned(),
                "[22:12] * Use /e2ee refresh to check server backup, /e2ee setup to enable recovery, /e2ee recover to restore keys"
                    .to_owned(),
            ]
        );
    }
    #[test]
    fn e2ee_recovery_succeeded_event_sets_status_message() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            Event::E2eeRecoverySucceeded(mirx_core::E2eeRecoverySucceeded {
                operation: mirx_core::E2eeRecoveryOperation::Recover,
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "e2ee recovery complete");
    }

    #[test]
    fn e2ee_recovery_setup_succeeded_event_only_sets_non_secret_status() {
        let mut state = test_state();
        let recovery_key = "recovery-key-must-not-persist";

        apply_core_event(
            &mut state,
            Event::E2eeRecoverySetupSucceeded(mirx_core::E2eeRecoverySetupSucceeded {
                recovery_key: recovery_key.to_owned(),
            }),
        )
        .unwrap();

        let status_lines = status_log_lines(&state);
        assert_eq!(state.model.status, "e2ee recovery setup complete");
        assert!(!state.model.status.contains(recovery_key));
        assert_eq!(
            status_lines,
            vec![
                "[22:12] * E2EE recovery set up; the recovery key was shown in a dialog".to_owned()
            ]
        );
        assert!(!status_lines.iter().any(|line| line.contains(recovery_key)));
        assert!(!format!("{:?}", dump_model_from_state(&state)).contains(recovery_key));
        assert!(
            !state
                .views
                .values()
                .flat_map(|view| &view.compose().history.entries)
                .any(|entry| entry.contains(recovery_key))
        );
    }

    #[test]
    fn submission_status_survives_matching_error_event() {
        let mut state = test_state();
        state.model.status = "join submitted: #ops:example.org".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::JoinRoom,
                room_id: None,
                message: "join room requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "join submitted: #ops:example.org");
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::JoinRoom,
                room_id: None,
                message: "join room requires an authenticated session".to_owned(),
            })
        );

        state.model.status = "accept submitted: !invite:example".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::AcceptRoomInvite,
                room_id: Some("!invite:example".to_owned()),
                message: "accept room invitation requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "accept submitted: !invite:example");
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::AcceptRoomInvite,
                room_id: Some("!invite:example".to_owned()),
                message: "accept room invitation requires an authenticated session".to_owned(),
            })
        );

        state.model.status = "part submitted: !beta:example".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::LeaveRoom,
                room_id: Some("!beta:example".to_owned()),
                message: "leave room requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "part submitted: !beta:example");
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::LeaveRoom,
                room_id: Some("!beta:example".to_owned()),
                message: "leave room requires an authenticated session".to_owned(),
            })
        );

        state.model.status = "reject submitted: !invite:example".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::RejectRoomInvite,
                room_id: Some("!invite:example".to_owned()),
                message: "reject room invitation requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "reject submitted: !invite:example");
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::RejectRoomInvite,
                room_id: Some("!invite:example".to_owned()),
                message: "reject room invitation requires an authenticated session".to_owned(),
            })
        );

        state.model.status = "query submitted: @alice:example.org".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::CreateDirectRoom,
                room_id: None,
                message: "create direct room requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "query submitted: @alice:example.org");
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::CreateDirectRoom,
                room_id: None,
                message: "create direct room requires an authenticated session".to_owned(),
            })
        );

        state.model.status = "topic change submitted for !ops:example.org".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::SetRoomTopic,
                room_id: Some("!ops:example.org".to_owned()),
                message: "set room topic requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(
            state.model.status,
            "topic change submitted for !ops:example.org"
        );
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::SetRoomTopic,
                room_id: Some("!ops:example.org".to_owned()),
                message: "set room topic requires an authenticated session".to_owned(),
            })
        );
    }

    #[test]
    fn verification_done_logs_terminal_text_without_actions() {
        let mut state = test_state();
        apply_core_event(
            &mut state,
            Event::VerificationState(VerificationStateEvent {
                flow_id: "flow-verify-123".to_owned(),
                user_id: "@alice:example".to_owned(),
                state: VerificationState::Done,
            }),
        )
        .unwrap();

        assert_eq!(
            status_log_lines(&state),
            vec!["[22:12] * Verification done with @alice:example: verified".to_owned()]
        );
    }
    #[test]
    fn verification_cancelled_logs_reason_code_without_actions() {
        let mut state = test_state();
        apply_core_event(
            &mut state,
            Event::VerificationState(VerificationStateEvent {
                flow_id: "flow-verify-123".to_owned(),
                user_id: "@alice:example".to_owned(),
                state: VerificationState::Cancelled {
                    reason: "timed out".to_owned(),
                    code: "m.timeout".to_owned(),
                    cancelled_by_us: false,
                },
            }),
        )
        .unwrap();

        assert_eq!(
            status_log_lines(&state),
            vec![
                "[22:12] * Verification cancelled with @alice:example: timed out (m.timeout)"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn synced_rooms_preserves_only_valid_selection() {
        let mut state = test_state();
        state.model.selected_room = Some("!keep:example".to_owned());

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!keep:example", "Keep", false),
                room("!other:example", "Other", false),
            ]),
        )
        .unwrap();
        assert_eq!(state.model.selected_room, Some("!keep:example".to_owned()));

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!other:example", "Other", false)]),
        )
        .unwrap();
        assert_eq!(state.model.selected_room, None);
    }

    #[test]
    fn synced_rooms_clears_typing_for_non_actionable_rooms() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            typing_event("!keep:example", &["@alice:example.org"]),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            typing_event("!drop:example", &["@bob:example.org"]),
        )
        .unwrap();

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!keep:example", "Keep", false)]),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!keep:example")
                .map(|view| view.typing_user_ids.clone()),
            Some(vec!["@alice:example.org".to_owned()])
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!drop:example")
                .map(|view| view.typing_user_ids.clone()),
            Some(Vec::new())
        );
    }

    #[test]
    fn synced_rooms_clear_selected_room_when_room_becomes_space() {
        let mut state = test_state();
        state.model.selected_room = Some("!room:example".to_owned());

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![space("!room:example", "Room Space")]),
        )
        .unwrap();

        assert_eq!(state.model.selected_room, None);
    }

    #[test]
    fn logged_out_clears_selection_timelines_and_diagnostics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.model.selected_room = Some("!room:example".to_owned());
        state.active_view = ViewId::Room("!room:example".to_owned());
        state.model.room_views.insert(
            "!room:example".to_owned(),
            RoomViewModel {
                topic: Some("incident review".to_owned()),
                activities: vec![sent_message_activity(
                    "!room:example",
                    "$!room:example:hello",
                    None,
                    "hello",
                    1_700_000_000_000,
                )],
                reactions_by_target: BTreeMap::new(),
                threads: BTreeMap::new(),
                preview_media: BTreeMap::new(),
                reply_targets: BTreeMap::new(),
                edited_event_ids: BTreeSet::new(),
                typing_user_ids: vec!["@alice:example.org".to_owned()],
                members: Vec::new(),
                history: RoomHistoryBackfillState::default(),
                echo: EchoLog::default(),
                arrivals: RoomArrivals::default(),
            },
        );
        state.model.last_activity = Some(LastMessageActivity {
            room_id: "!room:example".to_owned(),
            event_id: Some("$!room:example:hello".to_owned()),
            transaction_id: None,
            sender: "@alice:example".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            presentation: RoomMessagePresentationKind::Text,
            body: "hello".to_owned(),
            send_state: RoomMessageSendState::Sent,
        });
        state.model.last_e2ee_status = Some(e2ee_status(
            E2eeRecoveryState::Enabled,
            E2eeBackupState::Enabled,
            true,
            Some(true),
            Some(true),
        ));
        state.model.last_verification = Some(VerificationStateEvent {
            flow_id: "flow".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::Requested,
        });
        state.model.last_error = Some(CoreErrorEvent {
            operation: Operation::SendMessage,
            room_id: Some("!room:example".to_owned()),
            message: "failed".to_owned(),
        });
        state.find_states.insert(
            ViewId::Room("!room:example".to_owned()),
            build_view_find_state("hello", "hello", Vec::new(), None),
        );
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(ViewId::Room("!room:example".to_owned()), tracked_view(20));
        state.hwnd_views.insert(10, ViewId::Status);
        state
            .hwnd_views
            .insert(20, ViewId::Room("!room:example".to_owned()));
        let words = vec![HighlightWord {
            word: "deploy".to_owned(),
            highlights: true,
        }];
        apply_core_event(&mut state, Event::HighlightWords(words.clone()))
            .expect("highlight words apply");
        assert_eq!(state.model.highlight_words, Some(words));

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut)
                .expect("logged out event must apply successfully");
        });
        let removed = take_stale_room_views(&mut state);

        assert!(state.model.rooms.is_empty());
        assert_eq!(state.model.highlight_words, None);
        assert_eq!(state.model.selected_room, None);
        assert!(state.model.room_views.is_empty());
        assert_eq!(state.model.last_activity, None);
        assert_eq!(state.model.last_e2ee_status, None);
        assert_eq!(state.model.last_verification, None);
        assert_eq!(state.model.last_error, None);
        assert!(state.find_states.is_empty());
        assert_eq!(state.active_view, ViewId::Status);
        assert_eq!(removed.len(), 1);
        assert!(state.views.contains_key(&ViewId::Status));
        assert!(
            !state
                .views
                .contains_key(&ViewId::Room("!room:example".to_owned()))
        );
    }

    #[test]
    fn logged_out_clears_last_e2ee_status() {
        let mut state = test_state();
        state.model.last_e2ee_status = Some(e2ee_status(
            E2eeRecoveryState::Incomplete,
            E2eeBackupState::Enabled,
            true,
            Some(true),
            Some(false),
        ));

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut)
                .expect("logged out event must apply successfully");
        });

        assert_eq!(state.model.last_e2ee_status, None);
    }

    #[test]
    fn closed_query_reopens_on_message_activity_from_others() {
        let mut state = test_state();
        state.model.rooms = vec![query("!dm:example", "Alice", "@alice:example")];
        state.model.own_user_id = Some("@me:example".to_owned());
        state.closed_queries.insert("!dm:example".to_owned());
        let message_from = |event_id: &str, user_id: &str| {
            let mut activity = sent_message_activity("!dm:example", event_id, None, "hi", 1);
            let RoomActivityKind::Message(message) = &mut activity.kind else {
                unreachable!("a sent message activity is a message");
            };
            message.sender = user_profile(user_id);
            activity
        };

        apply_room_activity_event(&mut state, message_from("$own", "@me:example"));
        apply_core_event(&mut state, topic_state("!dm:example", "new topic")).unwrap();
        assert!(state.closed_queries.contains("!dm:example"));

        apply_room_activity_event(&mut state, message_from("$theirs", "@alice:example"));
        assert!(state.closed_queries.is_empty());
    }

    fn room_joined(room_id: &str) -> Event {
        Event::RoomJoined(mirx_core::RoomJoined {
            room_id: room_id.to_owned(),
        })
    }

    #[test]
    fn room_joined_makes_the_joined_room_the_active_view() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!ops:example", "Ops", false),
            room("!new:example", "New", false),
        ];
        state.active_view = room_view_id("!ops:example");
        state.model.selected_room = Some("!ops:example".to_owned());

        apply_core_event(&mut state, room_joined("!new:example")).unwrap();

        assert_eq!(state.active_view, room_view_id("!new:example"));
        assert_eq!(state.model.selected_room.as_deref(), Some("!new:example"));
        assert_eq!(
            resolve_active_view(
                &state.active_view,
                state.model.selected_room.as_deref(),
                &state.model,
                &state.closed_queries,
            ),
            room_view_id("!new:example"),
            "the view sync keeps the joined room active"
        );
    }

    #[test]
    fn room_joined_reopens_a_closed_query() {
        let mut state = test_state();
        state.model.rooms = vec![query("!dm:example", "Alice", "@alice:example")];
        state.closed_queries.insert("!dm:example".to_owned());
        state.active_view = ViewId::Status;

        apply_core_event(&mut state, room_joined("!dm:example")).unwrap();

        assert!(state.closed_queries.is_empty());
        assert_eq!(state.active_view, room_view_id("!dm:example"));
    }

    #[test]
    fn room_joined_for_a_space_logs_it_and_keeps_the_active_view() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!ops:example", "Ops", false),
            space("!space:example", "Rust Space"),
        ];
        state.active_view = room_view_id("!ops:example");

        apply_core_event(&mut state, room_joined("!space:example")).unwrap();

        assert_eq!(state.active_view, room_view_id("!ops:example"));
        assert_eq!(
            state
                .model
                .status_log
                .lines()
                .last()
                .map(|line| line.text.as_str()),
            Some("* Joined space Rust Space")
        );
    }

    #[test]
    fn room_joined_for_a_room_missing_from_the_room_list_is_an_error() {
        let mut state = test_state();

        assert_eq!(
            apply_core_event(&mut state, room_joined("!gone:example")),
            Err("joined room !gone:example is not in the room list".to_owned())
        );
    }

    #[test]
    fn closed_queries_drop_rooms_that_stop_being_queries() {
        let mut state = test_state();
        state.closed_queries = BTreeSet::from([
            "!dm:example".to_owned(),
            "!gone:example".to_owned(),
            "!now-channel:example".to_owned(),
        ]);

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                query("!dm:example", "Alice", "@alice:example"),
                room("!now-channel:example", "Channel", false),
            ]),
        )
        .unwrap();

        assert_eq!(
            state.closed_queries,
            BTreeSet::from(["!dm:example".to_owned()])
        );
    }

    #[test]
    fn own_direct_message_neither_colours_nor_flashes() {
        let mut state = test_state();
        let mut direct_room = room("!dm:example", "Dm", false);
        direct_room.is_direct = true;
        direct_room.direct_user_ids = vec!["@alice:example".to_owned()];
        state.model.rooms = vec![direct_room];
        state.model.own_user_id = Some("@me:example".to_owned());
        let message_from = |event_id: &str, user_id: &str| {
            let mut activity = sent_message_activity("!dm:example", event_id, None, "hi", 1);
            let RoomActivityKind::Message(message) = &mut activity.kind else {
                unreachable!("a sent message activity is a message");
            };
            message.sender = user_profile(user_id);
            activity
        };
        assert_ne!(state.active_view, room_view_id("!dm:example"));

        assert_eq!(
            apply_room_activity_event(&mut state, message_from("$own", "@me:example")),
            ActivityAttention::default()
        );
        assert_eq!(state.room_activity.get(&room_view_id("!dm:example")), None);
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!dm:example")),
            None
        );

        assert!(
            apply_room_activity_event(&mut state, message_from("$theirs", "@alice:example")).flash
        );
        assert_eq!(
            state.room_activity.get(&room_view_id("!dm:example")),
            Some(&ViewActivityLevel::Message)
        );
    }

    #[test]
    fn view_activity_only_escalates_until_the_view_is_activated() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = room_view_id("!active:example");

        apply_core_event(&mut state, timeline("!room:example", "hello")).unwrap();
        apply_core_event(&mut state, topic_state("!room:example", "later topic")).unwrap();
        assert_eq!(
            state.room_activity.get(&room_view_id("!room:example")),
            Some(&ViewActivityLevel::Message)
        );

        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(highlighted(sent_message_activity(
                "!room:example",
                "$ping",
                None,
                "Alice: ping",
                2,
            )))),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!room:example", "after the ping")).unwrap();
        assert_eq!(
            state.room_activity.get(&room_view_id("!room:example")),
            Some(&ViewActivityLevel::Highlight)
        );

        handle_view_activation(&mut state, &room_view_id("!room:example")).unwrap();
        assert!(state.room_activity.is_empty());
    }

    #[test]
    fn room_activity_wants_attention_even_in_the_active_view_but_not_during_loads() {
        let mut state = test_state();
        let mut query = room("!query:example", "Casey", false);
        query.is_direct = true;
        state.model.rooms = vec![room("!room:example", "Room", false), query];
        state.active_view = room_view_id("!query:example");
        let ping = || {
            Event::RoomActivity(Box::new(highlighted(sent_message_activity(
                "!room:example",
                "$ping",
                None,
                "Alice: ping",
                1,
            ))))
        };

        let mut apply = |event| apply_core_event(&mut state, event).unwrap();
        let tip = |room_id: &str, activity_id: &str| {
            Some(TipCandidate {
                view: room_view_id(room_id),
                activity_id: activity_id.to_owned(),
            })
        };
        let quiet = ActivityAttention::default();
        assert_eq!(apply(timeline("!room:example", "hello")), quiet);
        assert_eq!(apply(topic_state("!query:example", "topic")), quiet);
        assert_eq!(
            apply(timeline("!query:example", "hi")),
            ActivityAttention {
                flash: true,
                tip: tip("!query:example", "event:$!query:example:hi"),
            }
        );
        assert_eq!(
            apply(ping()),
            ActivityAttention {
                flash: true,
                tip: tip("!room:example", "event:$ping"),
            }
        );
        assert_eq!(apply(Event::SyncStarted), quiet);
        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!query:example"))
        );

        state.synthetic_load_in_progress = true;
        assert_eq!(apply_core_event(&mut state, ping()).unwrap(), quiet);
        assert_eq!(
            apply_core_event(&mut state, timeline("!query:example", "loaded")).unwrap(),
            quiet
        );
    }

    #[test]
    fn a_batch_flashes_if_any_line_does_and_tips_about_its_last_tip() {
        let tip = |activity_id: &str| {
            Some(TipCandidate {
                view: room_view_id("!room:example"),
                activity_id: activity_id.to_owned(),
            })
        };
        let first = ActivityAttention {
            flash: true,
            tip: tip("first"),
        };
        let quiet = ActivityAttention::default();
        assert_eq!(
            first.then(quiet).then(ActivityAttention {
                flash: false,
                tip: tip("last"),
            }),
            ActivityAttention {
                flash: true,
                tip: tip("last"),
            }
        );
        assert_eq!(
            ActivityAttention {
                flash: false,
                tip: tip("only"),
            }
            .then(ActivityAttention::default()),
            ActivityAttention {
                flash: false,
                tip: tip("only"),
            }
        );
    }

    fn with_mode(mut room: RoomSummary, mode: RoomNotificationMode) -> RoomSummary {
        room.notification_mode = Some(mode);
        room
    }

    #[test]
    fn a_muted_rooms_activity_stays_event_coloured_and_never_flashes() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            with_mode(
                room("!muted:example", "Muted", false),
                RoomNotificationMode::Mute,
            ),
            with_mode(
                query("!quiet-dm:example", "Casey", "@casey:example"),
                RoomNotificationMode::Mute,
            ),
        ];
        state.active_view = room_view_id("!active:example");

        assert_eq!(
            apply_core_event(&mut state, timeline("!muted:example", "hello")).unwrap(),
            ActivityAttention::default()
        );
        let ping = highlighted(sent_message_activity(
            "!muted:example",
            "$ping",
            None,
            "Alice: ping",
            1,
        ));
        assert_eq!(
            apply_core_event(&mut state, Event::RoomActivity(Box::new(ping))).unwrap(),
            ActivityAttention::default()
        );
        assert_eq!(
            apply_core_event(&mut state, timeline("!quiet-dm:example", "hi")).unwrap(),
            ActivityAttention::default()
        );
        assert_eq!(
            state.room_activity.get(&room_view_id("!muted:example")),
            Some(&ViewActivityLevel::Event)
        );
        assert_eq!(
            state.room_activity.get(&room_view_id("!quiet-dm:example")),
            Some(&ViewActivityLevel::Event)
        );
    }

    #[test]
    fn a_mentions_only_query_message_is_red_but_does_not_flash() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            with_mode(
                query("!dm:example", "Casey", "@casey:example"),
                RoomNotificationMode::MentionsAndKeywordsOnly,
            ),
        ];
        state.active_view = room_view_id("!active:example");

        assert_eq!(
            apply_core_event(&mut state, timeline("!dm:example", "hi")).unwrap(),
            ActivityAttention::default()
        );
        assert_eq!(
            state.room_activity.get(&room_view_id("!dm:example")),
            Some(&ViewActivityLevel::Message)
        );
        let ping = highlighted(sent_message_activity(
            "!dm:example",
            "$ping",
            None,
            "Casey: ping",
            1,
        ));
        let attention = apply_core_event(&mut state, Event::RoomActivity(Box::new(ping))).unwrap();
        assert!(attention.flash && attention.tip.is_some(), "{attention:?}");
    }

    #[test]
    fn a_muted_closed_query_still_reopens_when_the_other_side_talks() {
        let mut state = test_state();
        state.model.rooms = vec![with_mode(
            query("!dm:example", "Alice", "@alice:example"),
            RoomNotificationMode::Mute,
        )];
        state.closed_queries.insert("!dm:example".to_owned());

        apply_core_event(&mut state, timeline("!dm:example", "hi")).unwrap();
        assert!(state.closed_queries.is_empty());
    }

    #[test]
    fn a_muted_rooms_unread_counts_are_event_coloured() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            with_mode(
                room("!muted:example", "Muted", false),
                RoomNotificationMode::Mute,
            ),
        ];
        state.active_view = room_view_id("!active:example");

        apply_core_event(
            &mut state,
            room_read_state("!muted:example", None, None, false, 3, 0, 1),
        )
        .unwrap();
        assert_eq!(
            state.room_activity.get(&room_view_id("!muted:example")),
            Some(&ViewActivityLevel::Event)
        );
    }

    #[test]
    fn read_state_marks_mentions_as_highlights_and_unread_as_messages() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!mentions:example", "Mentions", false),
            room("!unread:example", "Unread", false),
            room("!events:example", "Events", false),
        ];
        state.active_view = room_view_id("!active:example");

        apply_core_event(
            &mut state,
            room_read_state("!mentions:example", None, None, false, 2, 2, 1),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_read_state("!unread:example", None, None, false, 2, 0, 0),
        )
        .unwrap();
        apply_core_event(&mut state, topic_state("!events:example", "new topic")).unwrap();
        assert_eq!(
            state.room_activity.get(&room_view_id("!events:example")),
            Some(&ViewActivityLevel::Event)
        );
        // Zero counts clear the view, event-only activity included.
        apply_core_event(
            &mut state,
            room_read_state("!events:example", None, None, false, 0, 0, 0),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_read_state("!active:example", None, None, false, 4, 4, 4),
        )
        .unwrap();

        assert_eq!(
            state.room_activity,
            BTreeMap::from([
                (
                    room_view_id("!mentions:example"),
                    ViewActivityLevel::Highlight
                ),
                (room_view_id("!unread:example"), ViewActivityLevel::Message),
            ])
        );
    }

    #[test]
    fn inactive_synced_messages_failed_updates_and_state_notices_mark_room_activity() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(&mut state, timeline("!inactive:example", "hello")).unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued("!inactive:example", "txn-1", "draft"),
        )
        .unwrap();
        assert_eq!(
            state.room_activity.get(&room_view_id("!inactive:example")),
            Some(&ViewActivityLevel::Message)
        );

        state.room_activity.clear();

        apply_core_event(
            &mut state,
            outgoing_sent("!inactive:example", "txn-1", "$event:sent"),
        )
        .unwrap();
        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );

        apply_core_event(
            &mut state,
            outgoing_queued("!inactive:example", "txn-2", "retry"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_failed("!inactive:example", "txn-2", "send failed"),
        )
        .unwrap();
        assert_eq!(
            state.room_activity.get(&room_view_id("!inactive:example")),
            Some(&ViewActivityLevel::Message)
        );

        state.room_activity.clear();

        apply_core_event(
            &mut state,
            topic_state("!inactive:example", "incident review"),
        )
        .unwrap();

        assert_eq!(
            state.room_activity.get(&room_view_id("!inactive:example")),
            Some(&ViewActivityLevel::Event)
        );
    }

    #[test]
    fn inactive_event_backed_messages_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(&mut state, timeline("!inactive:example", "hello")).unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$!inactive:example:hello")
        );
    }

    #[test]
    fn synthetic_inactive_messages_mark_activity_without_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());
        state.synthetic_load_in_progress = true;

        apply_core_event(&mut state, timeline("!inactive:example", "hello")).unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            None
        );
    }

    #[test]
    fn inactive_unsupported_messages_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            unsupported_message(
                "!inactive:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "com.example.custom",
                "clip.mp4",
            ),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$image1:example.org")
        );
    }

    #[test]
    fn inactive_image_messages_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            image_message(
                "!inactive:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
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

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$image1:example.org")
        );
    }

    #[test]
    fn inactive_file_messages_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(file_activity(
                "!inactive:example",
                "$file1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "Quarterly report",
                Some("report.pdf"),
                Some(RoomFileInfo {
                    mimetype: Some("application/pdf".to_owned()),
                    size: Some(45_678),
                }),
            ))),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$file1:example.org")
        );
    }

    #[test]
    fn inactive_audio_messages_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(audio_activity(
                "!inactive:example",
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
            ))),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$audio1:example.org")
        );
    }

    #[test]
    fn inactive_state_notices_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            topic_state("!inactive:example", "incident review"),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$topic:!inactive:example:incident review")
        );
    }

    #[test]
    fn pending_eventless_and_failed_send_activity_do_not_replace_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(&mut state, timeline("!inactive:example", "hello")).unwrap();
        let original_target = state
            .room_activity_read_targets
            .get(&room_view_id("!inactive:example"))
            .cloned();

        apply_core_event(
            &mut state,
            outgoing_queued("!inactive:example", "txn-1", "draft"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_failed("!inactive:example", "txn-1", "send failed"),
        )
        .unwrap();

        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example")),
            original_target.as_ref()
        );
    }

    #[test]
    fn active_room_events_do_not_mark_room_view_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = ViewId::Room("!room:example".to_owned());

        apply_core_event(&mut state, timeline("!room:example", "hello")).unwrap();
        apply_core_event(&mut state, topic_state("!room:example", "incident review")).unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
    }

    #[test]
    fn thread_view_is_not_treated_as_active_room_for_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = thread_view_id("!room:example", "$thread-root");

        apply_core_event(&mut state, timeline("!room:example", "hello")).unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!room:example"))
                .map(String::as_str),
            Some("$!room:example:hello")
        );
    }

    #[test]
    fn threaded_activity_marks_thread_view_and_not_parent_room() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        apply_core_event(
            &mut state,
            thread_timeline("!room:example", "$thread-1", "$thread-root", "thread reply"),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&thread_view_id("!room:example", "$thread-root"))
                .map(String::as_str),
            Some("$thread-1")
        );
    }

    #[test]
    fn active_matching_thread_view_does_not_mark_thread_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = thread_view_id("!room:example", "$thread-root");

        apply_core_event(
            &mut state,
            thread_timeline("!room:example", "$thread-1", "$thread-root", "thread reply"),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
    }

    #[test]
    fn active_parent_room_does_not_suppress_thread_activity_marker() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");

        apply_core_event(
            &mut state,
            thread_timeline("!room:example", "$thread-1", "$thread-root", "thread reply"),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
    }

    #[test]
    fn failed_thread_send_marks_inactive_thread_view() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = room_view_id("!active:example");

        apply_core_event(
            &mut state,
            outgoing_queued_in_thread("!room:example", "thread-pending-1", "$thread-root"),
        )
        .unwrap();
        state.room_activity.clear();

        apply_core_event(
            &mut state,
            outgoing_failed("!room:example", "thread-pending-1", "send failed"),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
    }

    #[test]
    fn server_read_state_clears_matching_activity_read_target_without_command() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);
        state.room_activity.insert(
            thread_view_id("!room:example", "$thread-root"),
            ViewActivityLevel::Message,
        );
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event".to_owned());
        state.room_activity_read_targets.insert(
            thread_view_id("!room:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        apply_core_event(
            &mut state,
            room_read_state("!room:example", Some("$event"), None, false, 0, 0, 0),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(
            state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert_eq!(
            state.model.room_read_states.get("!room:example"),
            Some(&RoomReadState {
                room_id: "!room:example".to_owned(),
                latest_active_event_id: Some("$event".to_owned()),
                fully_read_event_id: None,
                marked_unread: false,
                num_unread: 0,
                num_notifications: 0,
                num_mentions: 0,
            })
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn server_fully_read_marker_clears_matching_activity_read_target_without_command() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);
        state.room_activity.insert(
            thread_view_id("!room:example", "$thread-root"),
            ViewActivityLevel::Message,
        );
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event".to_owned());
        state.room_activity_read_targets.insert(
            thread_view_id("!room:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        apply_core_event(
            &mut state,
            room_read_state("!room:example", None, Some("$event"), false, 0, 0, 0),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(
            state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn server_zero_unread_state_clears_only_room_activity() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);
        state.room_activity.insert(
            thread_view_id("!room:example", "$thread-root"),
            ViewActivityLevel::Message,
        );
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event".to_owned());
        state.room_activity_read_targets.insert(
            thread_view_id("!room:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        apply_core_event(
            &mut state,
            room_read_state("!room:example", None, None, false, 0, 0, 0),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
    }

    #[test]
    fn server_unread_state_marks_inactive_room_without_local_timeline_event() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            room_read_state("!room:example", None, None, false, 1, 0, 0),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
    }

    #[test]
    fn server_unread_state_does_not_mark_active_room() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = ViewId::Room("!room:example".to_owned());

        apply_core_event(
            &mut state,
            room_read_state("!room:example", None, None, false, 1, 0, 0),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
    }

    #[test]
    fn server_read_state_ignores_unknown_room() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            room_read_state("!missing:example", None, None, false, 1, 0, 0),
        )
        .unwrap();

        assert!(state.room_activity.is_empty());
        assert!(state.room_activity_read_targets.is_empty());
        assert!(state.model.rooms.is_empty());
        assert!(state.model.room_read_states.is_empty());
        assert!(state.model.room_views.is_empty());
        assert!(state.views.is_empty());
    }

    #[test]
    fn synced_rooms_prunes_stale_room_activity() {
        let mut state = test_state();
        // Thread activity stays tracked only while the thread is projected.
        apply_core_event(
            &mut state,
            thread_timeline("!keep:example", "$thread-1", "$thread-root", "thread reply"),
        )
        .unwrap();
        state
            .room_activity
            .insert(room_view_id("!keep:example"), ViewActivityLevel::Message);
        state
            .room_activity
            .insert(room_view_id("!stale:example"), ViewActivityLevel::Message);
        state.room_activity.insert(
            thread_view_id("!keep:example", "$thread-root"),
            ViewActivityLevel::Message,
        );

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!keep:example", "Keep", false)]),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!keep:example"))
        );
        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!stale:example"))
        );
        assert!(
            state
                .room_activity
                .contains_key(&thread_view_id("!keep:example", "$thread-root"))
        );
    }

    #[test]
    fn synced_rooms_prune_room_activity_read_targets() {
        let mut state = test_state();
        // Thread activity stays tracked only while the thread is projected.
        apply_core_event(
            &mut state,
            thread_timeline("!keep:example", "$thread-1", "$thread-root", "thread reply"),
        )
        .unwrap();
        state
            .room_activity
            .insert(room_view_id("!keep:example"), ViewActivityLevel::Message);
        state
            .room_activity
            .insert(room_view_id("!stale:example"), ViewActivityLevel::Message);
        state.room_activity.insert(
            thread_view_id("!keep:example", "$thread-root"),
            ViewActivityLevel::Message,
        );
        state
            .room_activity_read_targets
            .insert(room_view_id("!keep:example"), "$keep:example".to_owned());
        state
            .room_activity_read_targets
            .insert(room_view_id("!stale:example"), "$stale:example".to_owned());
        state.room_activity_read_targets.insert(
            thread_view_id("!keep:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!keep:example", "Keep", false)]),
        )
        .unwrap();

        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!keep:example"))
                .map(String::as_str),
            Some("$keep:example")
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&thread_view_id("!keep:example", "$thread-root"))
                .map(String::as_str),
            Some("$thread-1")
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!stale:example"))
        );
    }

    #[test]
    fn synced_rooms_prune_stale_room_read_states() {
        let mut state = test_state();
        state.model.room_read_states.insert(
            "!keep:example".to_owned(),
            RoomReadState {
                room_id: "!keep:example".to_owned(),
                latest_active_event_id: Some("$keep:example".to_owned()),
                fully_read_event_id: None,
                marked_unread: true,
                num_unread: 1,
                num_notifications: 0,
                num_mentions: 0,
            },
        );
        state.model.room_read_states.insert(
            "!stale:example".to_owned(),
            RoomReadState {
                room_id: "!stale:example".to_owned(),
                latest_active_event_id: Some("$stale:example".to_owned()),
                fully_read_event_id: None,
                marked_unread: false,
                num_unread: 0,
                num_notifications: 0,
                num_mentions: 0,
            },
        );

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!keep:example", "Keep", false),
                space("!space:example", "Space"),
            ]),
        )
        .unwrap();

        assert!(state.model.room_read_states.contains_key("!keep:example"));
        assert!(!state.model.room_read_states.contains_key("!stale:example"));
    }

    #[test]
    fn synced_rooms_prune_room_read_states_when_room_becomes_space() {
        let mut state = test_state();
        state.model.room_read_states.insert(
            "!room:example".to_owned(),
            RoomReadState {
                room_id: "!room:example".to_owned(),
                latest_active_event_id: Some("$room:example".to_owned()),
                fully_read_event_id: None,
                marked_unread: true,
                num_unread: 1,
                num_notifications: 0,
                num_mentions: 0,
            },
        );

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![space("!room:example", "Room Space")]),
        )
        .unwrap();

        assert!(!state.model.room_read_states.contains_key("!room:example"));
    }

    #[test]
    fn synced_rooms_prune_space_room_activity() {
        let mut state = test_state();
        state
            .room_activity
            .insert(room_view_id("!keep:example"), ViewActivityLevel::Message);
        state
            .room_activity
            .insert(room_view_id("!space:example"), ViewActivityLevel::Message);
        state.room_activity.insert(
            thread_view_id("!space:example", "$thread-root"),
            ViewActivityLevel::Message,
        );

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!keep:example", "Keep", false),
                space("!space:example", "Space"),
            ]),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!keep:example"))
        );
        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!space:example"))
        );
        assert!(
            !state
                .room_activity
                .contains_key(&thread_view_id("!space:example", "$thread-root"))
        );
    }

    #[test]
    fn synced_rooms_prune_space_room_activity_read_targets() {
        let mut state = test_state();
        state
            .room_activity
            .insert(room_view_id("!keep:example"), ViewActivityLevel::Message);
        state
            .room_activity
            .insert(room_view_id("!space:example"), ViewActivityLevel::Message);
        state
            .room_activity_read_targets
            .insert(room_view_id("!keep:example"), "$keep:example".to_owned());
        state
            .room_activity_read_targets
            .insert(room_view_id("!space:example"), "$space:example".to_owned());
        state.room_activity_read_targets.insert(
            thread_view_id("!space:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!keep:example", "Keep", false),
                space("!space:example", "Space"),
            ]),
        )
        .unwrap();

        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!keep:example"))
                .map(String::as_str),
            Some("$keep:example")
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!space:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!space:example", "$thread-root"))
        );
    }

    #[test]
    fn logged_out_clears_room_activity_read_targets() {
        let mut state = test_state();
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event:example".to_owned());

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut)
                .expect("logged out event must apply successfully");
        });

        assert!(state.room_activity_read_targets.is_empty());
    }

    #[test]
    fn logged_out_clears_room_activity() {
        let mut state = test_state();
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut)
                .expect("logged out event must apply successfully");
        });

        assert!(state.room_activity.is_empty());
    }

    #[test]
    fn logged_out_clears_room_read_states_before_same_room_id_returns() {
        let mut state = test_state();
        state.model.room_read_states.insert(
            "!room:example".to_owned(),
            RoomReadState {
                room_id: "!room:example".to_owned(),
                latest_active_event_id: Some("$room:example".to_owned()),
                fully_read_event_id: Some("$fully-read:example".to_owned()),
                marked_unread: true,
                num_unread: 1,
                num_notifications: 2,
                num_mentions: 1,
            },
        );

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut)
                .expect("logged out event must apply successfully");
        });
        apply_core_event(&mut state, Event::Started).unwrap();
        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!room:example", "Room", false)]),
        )
        .unwrap();

        assert!(!state.model.room_read_states.contains_key("!room:example"));
    }

    #[test]
    fn room_activation_with_read_target_enqueues_mark_room_read() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event:example".to_owned());

        assert_eq!(
            handle_view_activation(&mut state, &room_view_id("!room:example")),
            Ok(true)
        );
        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::MarkRoomRead(MarkRoomReadRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$event:example".to_owned(),
            })]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn room_activation_without_room_activity_discards_stale_read_target_without_command() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event:example".to_owned());

        assert_eq!(
            handle_view_activation(&mut state, &room_view_id("!room:example")),
            Ok(false)
        );
        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(state.recorded_commands.is_empty());

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn room_activation_without_read_target_clears_room_activity_without_command() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);

        assert_eq!(
            handle_view_activation(&mut state, &room_view_id("!room:example")),
            Ok(true)
        );
        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(state.recorded_commands.is_empty());

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn thread_activation_with_read_target_clears_only_thread_activity_and_enqueues_mark_room_read()
    {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);
        state.room_activity.insert(
            thread_view_id("!room:example", "$thread-root"),
            ViewActivityLevel::Message,
        );
        state.room_activity_read_targets.insert(
            thread_view_id("!room:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        assert_eq!(
            handle_view_activation(&mut state, &thread_view_id("!room:example", "$thread-root")),
            Ok(true)
        );
        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            !state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::MarkRoomRead(MarkRoomReadRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$thread-1".to_owned(),
            })]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn room_activation_leaves_thread_activity_intact() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);
        state.room_activity.insert(
            thread_view_id("!room:example", "$thread-root"),
            ViewActivityLevel::Message,
        );

        assert_eq!(
            handle_view_activation(&mut state, &room_view_id("!room:example")),
            Ok(true)
        );
        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(state.recorded_commands.is_empty());

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn create_room_errors_keep_the_submission_status() {
        assert!(preserves_submission_status(
            "create submitted: Team room",
            Operation::CreateRoom
        ));
        assert_eq!(operation_name(Operation::CreateRoom), "create room");
    }

    fn status_log_texts(state: &WindowState) -> Vec<&str> {
        state
            .model
            .status_log
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect()
    }

    fn room_echo_texts<'s>(state: &'s WindowState, room_id: &str) -> Vec<&'s str> {
        state.model.room_views[room_id]
            .echo
            .lines_for(None)
            .map(|line| line.text.as_str())
            .collect()
    }

    fn room_error(operation: Operation, room_id: &str, message: &str) -> Event {
        Event::Error(CoreErrorEvent {
            operation,
            room_id: Some(room_id.to_owned()),
            message: message.to_owned(),
        })
    }

    #[test]
    fn a_rooms_error_prints_in_its_window_or_status_when_it_has_none() {
        let mut state = test_state();
        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!ops:example", "Ops", false),
                room("!lobby:example", "Lobby", false),
            ]),
        )
        .unwrap();
        let status_lines = state.model.status_log.lines().len();

        apply_core_event(
            &mut state,
            room_error(Operation::KickRoomMember, "!lobby:example", "not an op"),
        )
        .unwrap();
        assert_eq!(
            room_echo_texts(&state, "!lobby:example"),
            vec!["* Error during kick room member: not an op"]
        );
        assert!(room_echo_texts(&state, "!ops:example").is_empty());
        assert_eq!(state.model.status_log.lines().len(), status_lines);
        assert_eq!(state.model.status, "error during kick room member");

        apply_core_event(
            &mut state,
            room_error(Operation::SendMessage, "!left:example", "gone"),
        )
        .unwrap();
        assert_eq!(
            status_log_texts(&state).last(),
            Some(&"* Error during send text: gone")
        );

        // Room loads show their failure in the window themselves.
        apply_core_event(
            &mut state,
            room_error(Operation::FetchRoomEvent, "!ops:example", "offline"),
        )
        .unwrap();
        assert!(room_echo_texts(&state, "!ops:example").is_empty());
        assert_eq!(
            status_log_texts(&state).last(),
            Some(&"* Error during fetch message: offline")
        );
    }

    #[test]
    fn ignore_list_changes_and_user_command_errors_print_in_the_active_window() {
        let mut state = test_state();
        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!ops:example", "Ops", false)]),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomMembers(RoomMembers {
                room_id: "!ops:example".to_owned(),
                members: vec![member_summary("@bob:example", Some("Bob"), false)],
            }),
        )
        .unwrap();
        state.active_view = ViewId::Room("!ops:example".to_owned());

        // The list after signing in is no change.
        apply_core_event(
            &mut state,
            Event::IgnoredUsers(vec!["@spam:remote".to_owned()]),
        )
        .unwrap();
        assert!(room_echo_texts(&state, "!ops:example").is_empty());
        assert_eq!(
            state.model.ignored_users,
            Some(vec!["@spam:remote".to_owned()])
        );

        apply_core_event(
            &mut state,
            Event::IgnoredUsers(vec!["@bob:example".to_owned()]),
        )
        .unwrap();
        assert_eq!(
            room_echo_texts(&state, "!ops:example"),
            vec![
                "* Added Bob (@bob:example) to ignore list",
                "* Removed @spam:remote from ignore list",
            ]
        );

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::UnignoreUser,
                room_id: None,
                message: "failed to unignore @bob:example: offline".to_owned(),
            }),
        )
        .unwrap();
        assert_eq!(
            room_echo_texts(&state, "!ops:example").last(),
            Some(&"* /unignore: failed to unignore @bob:example: offline")
        );
    }

    #[test]
    fn a_backfill_error_fails_the_named_rooms_load_only() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![
            room("!room:example", "Room", false),
            room("!other:example", "Other", false),
        ];
        for room_id in ["!room:example", "!other:example"] {
            state
                .model
                .room_views
                .entry(room_id.to_owned())
                .or_default()
                .history
                .next_token = Some("token-2".to_owned());
        }
        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );

        // A late error for a room that is not loading changes nothing.
        apply_core_event(
            &mut state,
            room_error(Operation::BackfillRoomHistory, "!other:example", "late"),
        )
        .unwrap();
        assert_eq!(
            state.model.room_views["!other:example"].history.status,
            RoomHistoryBackfillStatus::Idle
        );
        assert!(matches!(
            state.model.room_views["!room:example"].history.status,
            RoomHistoryBackfillStatus::Loading { .. }
        ));

        apply_core_event(
            &mut state,
            room_error(Operation::BackfillRoomHistory, "!room:example", "offline"),
        )
        .unwrap();
        assert_eq!(
            state.model.room_views["!room:example"].history.status,
            RoomHistoryBackfillStatus::Failed {
                from: Some("token-2".to_owned()),
                message: "offline".to_owned(),
            }
        );

        stop_test_core(handle);
    }

    #[test]
    fn resolve_timeline_room_id_rejects_spaces() {
        let mut state = test_state();
        state.model.rooms = vec![
            space("!space:example", "Space"),
            room("!room:example", "Room", false),
        ];
        state.model.selected_room = Some("!room:example".to_owned());

        assert_eq!(
            resolve_timeline_room_id(&state, Some("!space:example")),
            Err("unknown room id: !space:example".to_owned())
        );
        assert_eq!(
            resolve_timeline_room_id(&state, Some("!room:example")),
            Ok("!room:example".to_owned())
        );
    }

    #[test]
    fn lifecycle_events_update_model_and_persist_login_session() {
        with_local_app_data(|root| {
            let mut state = test_state();

            apply_core_event(&mut state, Event::Started).unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::CoreStarted);

            apply_core_event(
                &mut state,
                Event::LoggedIn {
                    session: stored_session(),
                    homeserver_url: "https://matrix.example".to_owned(),
                },
            )
            .unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Authenticated);
            assert_eq!(state.model.own_user_id.as_deref(), Some("@alice:example"));
            assert_eq!(
                state.current_session_homeserver_url,
                Some("https://matrix.example".to_owned())
            );

            let session_path = session_file_path(&root);
            let session_bytes = fs::read(&session_path).expect("saved session file must exist");
            assert_eq!(load_saved_session().unwrap(), saved_session_payload());
            assert_bytes_do_not_contain(&session_bytes, "secret-token");
            assert_bytes_do_not_contain(&session_bytes, "refresh-token");
            assert_bytes_do_not_contain(&session_bytes, "@alice:example");
            assert_bytes_do_not_contain(&session_bytes, "DEVICE");
            assert_bytes_do_not_contain(&session_bytes, "\"access_token\"");

            apply_core_event(&mut state, Event::SyncStarted).unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Syncing);

            apply_core_event(
                &mut state,
                Event::SyncedRooms(vec![room("!room:example", "Room", false)]),
            )
            .unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Ready);

            apply_core_event(&mut state, Event::RestoredSession).unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Authenticated);

            apply_core_event(
                &mut state,
                Event::Error(CoreErrorEvent {
                    operation: Operation::RestoreSession,
                    room_id: None,
                    message: "restore failed".to_owned(),
                }),
            )
            .unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
            assert_eq!(
                state.model.last_error,
                Some(CoreErrorEvent {
                    operation: Operation::RestoreSession,
                    room_id: None,
                    message: "restore failed".to_owned(),
                })
            );

            apply_core_event(&mut state, Event::LoggedOut).unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::LoggedOut);
            assert_eq!(state.model.own_user_id, None);
            assert_eq!(state.current_session_homeserver_url, None);
            assert!(!session_path.exists());

            apply_core_event(&mut state, Event::Stopped).unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Stopped);
        });
    }

    #[test]
    fn connection_events_are_logged_to_status_and_survive_logout() {
        with_local_app_data(|_| {
            let mut state = test_state();
            state.synthetic_load_in_progress = true;

            apply_core_event(&mut state, Event::Started).unwrap();
            apply_core_event(
                &mut state,
                Event::LoggedIn {
                    session: stored_session(),
                    homeserver_url: "https://matrix.example".to_owned(),
                },
            )
            .unwrap();
            apply_core_event(&mut state, Event::SyncStarted).unwrap();
            let rooms = vec![
                room("!room:example", "Room", false),
                invited_room("!invite:example", "Invite"),
            ];
            apply_core_event(&mut state, Event::SyncedRooms(rooms.clone())).unwrap();
            // The core re-sends the room list on every change.
            apply_core_event(&mut state, Event::SyncedRooms(rooms)).unwrap();
            apply_core_event(&mut state, Event::SyncStopped(SyncStopReason::Requested)).unwrap();
            apply_core_event(&mut state, Event::SyncStarted).unwrap();
            apply_core_event(
                &mut state,
                Event::SyncStopped(SyncStopReason::Failed(
                    "continuous Matrix sync failed: boom".to_owned(),
                )),
            )
            .unwrap();
            apply_core_event(&mut state, Event::LoggedOut).unwrap();
            apply_core_event(&mut state, Event::Stopped).unwrap();

            assert_eq!(state.model.own_user_id, None);
            assert_eq!(
                status_log_lines(&state),
                vec![
                    "[22:12] * Logged in as @alice:example (device DEVICE)".to_owned(),
                    "[22:12] * Syncing with the homeserver".to_owned(),
                    "[22:12] * Synced rooms: 1 joined, 1 invited".to_owned(),
                    "[22:12] * Disconnected".to_owned(),
                    "[22:12] * Syncing with the homeserver".to_owned(),
                    "[22:12] * Disconnected: continuous Matrix sync failed: boom".to_owned(),
                    "[22:12] * Logged out".to_owned(),
                    "[22:12] * Disconnected".to_owned(),
                ]
            );
        });
    }

    #[test]
    fn status_lines_mark_an_inactive_status_window() {
        let mut state = test_state();
        apply_core_event(&mut state, Event::SyncStarted).unwrap();
        assert_eq!(state.room_activity.get(&ViewId::Status), None);

        state.active_view = room_view_id("!room:example");
        apply_core_event(&mut state, Event::SyncStarted).unwrap();
        assert_eq!(
            state.room_activity.get(&ViewId::Status),
            Some(&ViewActivityLevel::Event)
        );
        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::SendMessage,
                room_id: Some("!left:example".to_owned()),
                message: "message send failed".to_owned(),
            }),
        )
        .unwrap();
        assert_eq!(
            state.room_activity.get(&ViewId::Status),
            Some(&ViewActivityLevel::Message)
        );
        let rows = build_status_timeline_rows(&state.model.status_log, state.timestamps());
        assert_eq!(rows.last().unwrap().kind, TimelineRowKind::Error);
    }

    #[test]
    fn logged_in_event_for_another_account_is_rejected() {
        let mut state = test_state();
        state.synthetic_load_in_progress = true;
        state.model.own_user_id = Some("@me:example".to_owned());

        assert_eq!(
            apply_core_event(
                &mut state,
                Event::LoggedIn {
                    session: stored_session(),
                    homeserver_url: "https://matrix.example".to_owned(),
                },
            ),
            Err("received LoggedIn for @alice:example while signed in as @me:example".to_owned())
        );
        assert_eq!(state.model.own_user_id.as_deref(), Some("@me:example"));
    }

    #[test]
    fn synthetic_logged_in_event_updates_model_without_persisting_session() {
        with_local_app_data(|root| {
            let mut state = test_state();
            state.synthetic_load_in_progress = true;

            apply_core_event(
                &mut state,
                Event::LoggedIn {
                    session: stored_session(),
                    homeserver_url: "https://matrix.example".to_owned(),
                },
            )
            .unwrap();

            assert_eq!(state.model.status, "authenticated");
            assert_eq!(state.model.lifecycle_state, LifecycleState::Authenticated);
            assert_eq!(state.current_session_homeserver_url, None);
            assert!(!root.join(PROFILE_SUBDIR).join(SESSION_FILE_NAME).exists());
        });
    }

    #[test]
    fn session_updated_event_persists_refreshed_tokens_without_leaking_them() {
        with_local_app_data(|root| {
            save_session("https://matrix.example", &stored_session()).unwrap();
            let mut state = test_state();
            state.current_session_homeserver_url = Some("https://matrix.example".to_owned());

            let updated_session = StoredSession {
                access_token: "updated-access-token".to_owned(),
                refresh_token: Some("updated-refresh-token".to_owned()),
                ..stored_session()
            };

            apply_core_event(
                &mut state,
                Event::SessionUpdated {
                    session: updated_session.clone(),
                },
            )
            .unwrap();

            assert_eq!(
                load_saved_session().unwrap(),
                SavedSession {
                    session: updated_session.clone(),
                    ..saved_session_payload()
                }
            );
            let session_bytes = fs::read(session_file_path(&root)).unwrap();
            assert_bytes_do_not_contain(&session_bytes, "updated-access-token");
            assert_bytes_do_not_contain(&session_bytes, "updated-refresh-token");
            assert!(
                !serde_json::to_string(&dump_model_from_state(&state))
                    .unwrap()
                    .contains("updated-access-token")
            );
            assert!(
                !serde_json::to_string(&state.model.status)
                    .unwrap()
                    .contains("updated-access-token")
            );
        });
    }

    #[test]
    fn startup_restore_keeps_restoring_state_when_started_arrives_after_submit() {
        with_local_app_data(|_| {
            let mut state = test_state();
            let (sender, _commands) = inert_command_sender();
            state.command_sender = Some(sender);
            save_session("https://matrix.example", &stored_session()).unwrap();

            submit_restore_saved_session(&mut state, false).unwrap();
            assert_eq!(state.model.status, "restoring saved session");
            assert_eq!(state.model.lifecycle_state, LifecycleState::Restoring);

            apply_core_event(&mut state, Event::Started).unwrap();
            assert_eq!(state.model.status, "restoring saved session");
            assert_eq!(state.model.lifecycle_state, LifecycleState::Restoring);
        });
    }
}
