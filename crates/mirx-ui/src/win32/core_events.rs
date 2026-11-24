//! Applying core events to the window state, then refreshing the UI.

use mirx_core::SyncStopReason;

use super::open_link::open_web_link;
use super::*;

/// A reply's quote target came: it shows, with its edits applied (its
/// reactions show on no quote). An edit that synced while the fetch ran is
/// dropped, but the event cache, which the fetch reads, normally has it.
fn apply_room_event_fetch(state: &mut WindowState, fetch: RoomEventFetch) {
    // The room was left while the fetch ran: no window shows its replies.
    let Some(room_view) = state.model.room_views.get_mut(&fetch.room_id) else {
        return;
    };
    // History loaded it as a line meanwhile, which a quote follows, and
    // which brought its own edits.
    if room_view
        .find_activity_index_by_event_id(&fetch.event_id)
        .is_some()
    {
        return;
    }
    match fetch.result {
        RoomEventFetchResult::Loaded { activity, updates } => {
            room_view
                .reply_targets
                .insert(fetch.event_id, ReplyTargetState::Loaded(activity));
            for update in &updates {
                room_view.apply_update(update);
            }
        }
        RoomEventFetchResult::Failed { .. } => {
            room_view
                .reply_targets
                .insert(fetch.event_id, ReplyTargetState::Failed);
        }
    }
}

/// Returns whether the activity should flash the frame. The active view gets
/// no activity colour, but it can still want attention while mIRC is in the
/// background.
fn apply_room_activity_event(state: &mut WindowState, activity: RoomActivity) -> ActivityAttention {
    let clears_last_activity = matches!(activity.kind, RoomActivityKind::Redacted(_));
    let target_view_id = room_activity_target_view_id(&activity);
    let mut attention = ActivityAttention::default();
    if let Some(level) = activity_level_for(&activity, state.model.own_user_id())
        && let Some((room_is_direct, mode)) =
            sendable_room_by_id(&state.model.rooms, &activity.room_id)
                .map(|room| (room.is_direct, joined_room_notification_mode(room)))
    {
        // A replayed event load is not news.
        if !state.harness.synthetic_load() {
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
            raise_view_activity(state, target_view_id, level);
        }
    }
    let room_id = activity.room_id.clone();
    let last_activity = last_activity_from_room_activity(&activity);
    state
        .model
        .room_views
        .entry_or_default(room_id)
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
            (RoomActivityUpdateTarget::Event { event_id }, _) => {
                last_activity.room_id == room_id
                    && last_activity.event_id.as_deref() == Some(event_id.as_str())
            }
            (RoomActivityUpdateTarget::Transaction { .. }, _) => false,
        });
    let applied = state
        .model
        .room_views
        .entry_or_default(room_id.clone())
        .apply_update(&update);
    if applied && matches!(update.kind, RoomActivityUpdateKind::ActivityRedacted { .. }) {
        prune_decoded_images(state);
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
    let room_view = state.model.room_views.entry_or_default(room_id.clone());
    room_view.apply_history_backfill(&room_id, backfill);
    room_view.history = match end {
        Some(token) => RoomHistoryBackfillState::Idle {
            next: HistoryFrom::Token { token },
        },
        None => RoomHistoryBackfillState::Exhausted,
    };
    prune_decoded_images(state);
}

/// Lines that could not be decrypted become their messages, in place, or
/// take their current cause; decrypted edits and reactions apply. A line
/// that now names you raises its window to a highlight, but it is not
/// news: no flash, no tip. Each room view that changes is taken mutably
/// (which redraws it) only then: a big key import reports many events
/// nothing shows, and a retry many lines that stay as they are.
fn apply_room_events_decrypted(state: &mut WindowState, decrypted: RoomEventsDecrypted) {
    let room_id = decrypted.room_id;
    if state.model.room_views.get(&room_id).is_none() {
        return;
    }
    for event in decrypted.events {
        match event {
            RoomHistoryBackfillEvent::Activity(activity) => {
                assert_eq!(
                    activity.room_id, room_id,
                    "a decrypted line of another room"
                );
                let event_id = room_activity_event_id(&activity)
                    .expect("a decrypted activity is an event's")
                    .to_owned();
                // Shown as a message already (the live handler, or an
                // earlier report, decrypted it), not shown at all, or shown
                // as it is (a retry that changed nothing).
                if state.model.room_views[&room_id]
                    .shown_undecrypted(&event_id)
                    .is_none_or(|shown| *shown == *activity)
                {
                    continue;
                }
                // A line still undecryptable has no push actions to name
                // you by.
                let decrypted = !matches!(activity.kind, RoomActivityKind::UnableToDecrypt(_));
                if decrypted && activity.highlight {
                    raise_decrypted_highlight(state, &activity);
                }
                if state.model.last_activity.as_ref().is_some_and(|last| {
                    last.room_id == room_id && last.event_id.as_deref() == Some(&event_id)
                }) {
                    state.model.last_activity = last_activity_from_room_activity(&activity);
                }
                state
                    .model
                    .room_views
                    .get_mut(&room_id)
                    .expect("the room view was there")
                    .apply_decrypted(*activity);
            }
            RoomHistoryBackfillEvent::ActivityUpdate(update) => {
                assert_eq!(
                    update.room_id, room_id,
                    "a decrypted update of another room"
                );
                apply_room_activity_update_event(state, *update);
            }
        }
    }
}

fn raise_decrypted_highlight(state: &mut WindowState, activity: &RoomActivity) {
    let target_view_id = room_activity_target_view_id(activity);
    if state.active_view == target_view_id {
        return;
    }
    let Some(mode) = sendable_room_by_id(&state.model.rooms, &activity.room_id)
        .map(joined_room_notification_mode)
    else {
        return;
    };
    raise_view_activity(
        state,
        target_view_id,
        notifications::room_activity_level(ViewActivityLevel::Highlight, mode),
    );
}

/// Asks the core to read again what every room shows that could not be
/// decrypted: to decrypt it, or tell its current cause. Only a session can.
fn retry_decryption_of_shown_lines(state: &mut WindowState) -> Result<(), String> {
    if !can_fetch_room_events(state) {
        return Ok(());
    }
    let requests = state
        .model
        .room_views
        .iter()
        .filter_map(|(room_id, view)| {
            let event_ids = view.undecrypted_event_ids();
            (!event_ids.is_empty()).then(|| RetryDecryptionRequest {
                room_id: room_id.clone(),
                event_ids: event_ids.into_iter().collect(),
            })
        })
        .collect::<Vec<_>>();
    for request in requests {
        enqueue_command(state, Command::RetryDecryption(request))?;
    }
    Ok(())
}

/// Whether the session's state that tells why a line could not be
/// decrypted changed: its verification, its key backup, or whether the
/// server has one. Then the shown lines are read again, and decrypt or
/// take their current cause (no line keeps saying "/verify" once you
/// did). Lines made before the first status were told by the core with
/// the live state already. A status that did not ask the server
/// (`exists_on_server: None`) differs from one that did; the retry it
/// costs changes nothing.
fn decryption_context_changed(last: &E2eeStatus, new: &E2eeStatus) -> bool {
    last.session != new.session
        || last.backup.state != new.backup.state
        || last.backup.enabled_locally != new.backup.enabled_locally
        || last.backup.exists_on_server != new.backup.exists_on_server
}

/// The room's history load failed. An error for a room that is not loading
/// (it was left, or the user cleared it meanwhile) changes nothing.
fn apply_backfill_room_history_error(state: &mut WindowState, room_id: &str, message: String) {
    let Some(room_view) = state.model.room_views.get_mut(room_id) else {
        return;
    };
    let RoomHistoryBackfillState::Loading { from } = &room_view.history else {
        return;
    };
    room_view.history = RoomHistoryBackfillState::Failed {
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

    let marked_unread = read_state.marked_unread;
    let num_unread = read_state.num_unread;
    let num_notifications = read_state.num_notifications;
    let num_mentions = read_state.num_mentions;

    let has_unread_state =
        marked_unread || num_unread > 0 || num_notifications > 0 || num_mentions > 0;

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
        .entry_or_default(typing.room_id.clone())
        .replace_typing(typing);
}

fn apply_room_members_event(state: &mut WindowState, members: RoomMembers) {
    state
        .model
        .room_views
        .entry_or_default(members.room_id.clone())
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
    // The open Sessions dialog asked, and shows it
    // (`apply_events_and_refresh`): the status bar and the windows say
    // nothing.
    if state.sessions_dialog.is_some()
        && sessions::is_dialog_answer(state.model.own_user_id(), &event)
    {
        return Ok(ActivityAttention::default());
    }
    match event {
        Event::CommandHandled(id) => state.core_answers.answer(id),
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
            if let Some(own_user_id) = state.model.own_user_id()
                && own_user_id != session.user_id
            {
                return Err(format!(
                    "received LoggedIn for {} while signed in as {own_user_id}",
                    session.user_id
                ));
            }
            // A replayed event load must not overwrite the saved session.
            if !state.harness.synthetic_load() {
                save_session(&homeserver_url, &session)?;
                state.current_session_homeserver_url = Some(homeserver_url);
            }
            log_status(
                state,
                StatusLineKind::Info,
                status_log::logged_in(&session.user_id, &session.device_id),
            );
            state.model.own_account = Some(OwnAccount {
                user_id: session.user_id,
                auth: session.auth,
            });
            state.model.status = "authenticated".to_owned();
            state.model.lifecycle_state = LifecycleState::Authenticated;
            state.room_count_due = true;
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
            state.room_count_due = true;
        }
        Event::SyncedRooms(rooms) => {
            // The core re-sends the room list on every change; only the
            // session's first is news.
            if std::mem::take(&mut state.room_count_due) {
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
            state
                .sent_receipts
                .retain_rooms(|room_id| sendable_room_by_id(&rooms, room_id).is_some());
            // Mutable access moves a room's revision, which rebuilds its
            // window: take it only for rooms that change.
            let unsendable_with_people = state
                .model
                .room_views
                .iter()
                .filter(|(room_id, room_view)| {
                    sendable_room_by_id(&rooms, room_id).is_none()
                        && !(room_view.typing_user_ids.is_empty() && room_view.members.is_empty())
                })
                .map(|(room_id, _)| room_id.clone())
                .collect::<Vec<_>>();
            for room_id in unsendable_with_people {
                let room_view = state
                    .model
                    .room_views
                    .get_mut(&room_id)
                    .expect("the room was just listed");
                room_view.typing_user_ids.clear();
                room_view.members.clear();
            }
            state
                .model
                .room_read_states
                .retain(|room_id, _| sendable_room_by_id(&rooms, room_id).is_some());
            let previous = state
                .model
                .rooms_listed
                .then_some(state.model.rooms.as_slice());
            let arrived = new_invites(previous, &rooms)
                .into_iter()
                .cloned()
                .collect::<Vec<_>>();
            for invite in arrived {
                announce_invite(state, &invite)?;
            }
            state.model.rooms_listed = true;
            for room in rooms
                .iter()
                .filter(|room| room.membership == RoomMembership::Join && !room.is_space)
            {
                if state
                    .model
                    .room_views
                    .get(&room.room_id)
                    .map(|view| &view.topic)
                    != Some(&room.topic)
                {
                    state
                        .model
                        .room_views
                        .entry_or_default(room.room_id.clone())
                        .topic = room.topic.clone();
                }
            }
            state.model.rooms = rooms;
        }
        Event::RoomActivity(activity) => return Ok(apply_room_activity_event(state, *activity)),
        Event::RoomActivityUpdate(update) => apply_room_activity_update_event(state, update),
        Event::RoomHistoryBackfill(backfill) => apply_room_history_backfill_event(state, backfill),
        Event::RoomEventsDecrypted(decrypted) => apply_room_events_decrypted(state, decrypted),
        Event::DecryptionRetryWanted => retry_decryption_of_shown_lines(state)?,
        Event::RoomMedia(media) => apply_room_media_event(state, media),
        Event::RoomEvent(fetch) => apply_room_event_fetch(state, fetch),
        Event::PublicRooms(page) => apply_public_rooms_page(state, page)?,
        Event::RoomJoined(joined) => apply_room_joined_event(state, joined)?,
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
                echo_lines_in_front_window(state, lines);
            }
            state.model.ignored_users = Some(ignored_users);
        }
        // Channel Central shows it (`apply_events_and_refresh`); with no
        // dialog open for the room, nothing awaits it.
        Event::RoomSettings(_) => {}
        Event::UserProfile(profile) => {
            let lines = whois_lines(&state.model, &profile);
            let view = whois_view(state);
            echo_lines_in_view(state, &view, lines);
        }
        Event::AwayChanged(changed) => {
            echo_lines_in_front_window(state, commands::away_lines(&changed));
            state.model.status = commands::away_status(&changed);
        }
        Event::PresenceChanged(changes) => {
            state.model.presence.apply(changes);
            announce_notify_changes(state)?;
        }
        Event::UserDevices(devices) => {
            let lines = devices_lines(&state.model, &devices, state.local_time);
            echo_lines_in_front_window(state, lines);
        }
        Event::RoomIdentityChanges(changes) => {
            identity::apply_room_identity_changes(state, changes);
        }
        Event::UserIdentityAccepted(user_id) => {
            let nick = known_nick(&state.model, &user_id).unwrap_or(user_id);
            echo_lines_in_front_window(state, [identity::accepted_line(&nick)]);
        }
        Event::DeviceSignedOut(device_id) => {
            echo_lines_in_front_window(state, [commands::device_signed_out_line(&device_id)]);
            state.model.status = "session signed out".to_owned();
        }
        Event::DeviceSignOutApproval { device_id, url } => {
            echo_lines_in_front_window(
                state,
                [
                    commands::device_sign_out_approval_line(&device_id),
                    format!("* {}", status_log::sign_in_address(&url)),
                ],
            );
            state.model.status = "waiting for session sign-out".to_owned();
        }
        Event::DeviceRenamed {
            device_id,
            display_name,
        } => {
            echo_lines_in_front_window(
                state,
                [commands::device_renamed_line(&device_id, &display_name)],
            );
            state.model.status = "session renamed".to_owned();
        }
        Event::E2eeStatus(status) => {
            for (kind, line) in status_log::e2ee_status(&status) {
                log_status(state, kind, line);
            }
            let context_changed = state
                .model
                .last_e2ee_status
                .as_ref()
                .is_some_and(|last| decryption_context_changed(last, &status));
            state.model.last_e2ee_status = Some(status);
            state.model.status = "e2ee status updated".to_owned();
            if context_changed {
                retry_decryption_of_shown_lines(state)?;
            }
        }
        Event::RoomKeysExported { path } => {
            log_status(
                state,
                StatusLineKind::Info,
                status_log::room_keys_exported(&path),
            );
            state.model.status = "room keys exported".to_owned();
        }
        Event::RoomKeysImported {
            path,
            imported,
            total,
        } => {
            log_status(
                state,
                StatusLineKind::Info,
                status_log::room_keys_imported(&path, imported, total),
            );
            state.model.status = "room keys imported".to_owned();
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
        Event::E2eeIdentityResetApproval { url } => {
            log_status(
                state,
                StatusLineKind::Info,
                status_log::E2EE_RESET_APPROVAL.to_owned(),
            );
            log_status(
                state,
                StatusLineKind::Info,
                status_log::sign_in_address(&url),
            );
            state.model.status = "waiting for identity reset approval".to_owned();
        }
        Event::E2eeIdentityReset(outcome) => {
            for (kind, line) in status_log::e2ee_identity_reset(outcome) {
                log_status(state, kind, line);
            }
            state.model.status = match outcome {
                E2eeIdentityResetOutcome::Done => "e2ee identity reset",
                E2eeIdentityResetOutcome::Cancelled => "e2ee identity reset cancelled",
            }
            .to_owned();
        }
        Event::VerificationRequest(request) => apply_verification_request(state, request),
        Event::VerificationState(event) => apply_verification_event(state, event),
        Event::LogoutRefused { request, reason } => {
            state.model.status = match reason {
                LogoutRefusal::RoomKeysAtRisk(_) => "log out: room keys at risk",
                LogoutRefusal::ServerKeepsSession { .. } => "log out: the server kept the session",
            }
            .to_owned();
            state.log_out_warning = Some((request, reason));
        }
        Event::LoggingOut => {
            log_status(
                state,
                StatusLineKind::Info,
                status_log::LOGGING_OUT.to_owned(),
            );
            state.model.status = "logging out".to_owned();
        }
        Event::LogoutCancelled => {
            // The user's Disconnect: as after it, retries start over.
            state.reconnect.reset();
            log_status(
                state,
                StatusLineKind::Info,
                status_log::LOG_OUT_CANCELLED.to_owned(),
            );
            state.model.status = "log out cancelled".to_owned();
        }
        Event::LoggedOut(scope) => {
            let line = match scope {
                LogoutScope::Everywhere => status_log::LOGGED_OUT.to_owned(),
                LogoutScope::ThisComputerOnly { device_id } => {
                    // The connection is `SignedOut` already: `own_server`
                    // refuses.
                    let account = state.model.own_account.as_ref();
                    let (_, server) = account
                        .expect("a session logs out")
                        .user_id
                        .split_once(':')
                        .expect("a Matrix user ID names its server");
                    status_log::logged_out_on_this_computer_only(server, &device_id)
                }
            };
            state.reconnect.reset();
            // A warning not shown yet was about this session.
            state.log_out_warning = None;
            delete_saved_session()?;
            state.current_session_homeserver_url = None;
            state.model.own_account = None;
            state.room_count_due = false;
            state.model.status = "logged out".to_owned();
            state.model.lifecycle_state = LifecycleState::LoggedOut;
            state.model.rooms.clear();
            state.model.rooms_listed = false;
            state.closed_queries.clear();
            state.quiet_joins.clear();
            state.model.selected_room = None;
            state.model.room_views.clear();
            state.model.last_activity = None;
            state.model.last_e2ee_status = None;
            state.model.last_e2ee_recovery_setup_result = false;
            state.model.verifications.clear();
            state.model.last_error = None;
            state.model.room_read_states.clear();
            state.model.room_list = None;
            state.model.highlight_words = None;
            state.model.ignored_users = None;
            state.model.presence.clear();
            state.model.joined_aliases.clear();
            state.notify_watch.disconnected();
            state.room_activity.clear();
            state.sent_receipts.clear();
            state.find_states.clear();
            state.decoded_images.clear();
            log_status(state, StatusLineKind::Info, line);
        }
        Event::Stopped => {
            log_status(state, StatusLineKind::Info, status_log::STOPPED.to_owned());
            state.model.status = "core stopped".to_owned();
            state.model.lifecycle_state = LifecycleState::Stopped;
        }
        Event::SyncStarted => {
            if !state.reconnect.reconnecting() {
                log_status(
                    state,
                    StatusLineKind::Info,
                    status_log::SYNC_STARTED.to_owned(),
                );
            }
            state.model.status = "continuous sync started".to_owned();
            state.model.lifecycle_state = LifecycleState::Syncing;
            state.reconnect.sync_started();
        }
        Event::SyncConnected => {
            if state.reconnect.sync_connected() {
                let line = status_log::reconnected(session_server(state));
                log_status(state, StatusLineKind::Info, line);
            }
            state.model.status = format!("connected to {}", session_server(state));
            // Connected now: an earlier error (the offline start's restore,
            // a /join before the drop) is past, and stays in Status or its
            // window. The bar's error segment says what is wrong now.
            state.model.last_error = None;
            // mIRC's notify on connect: who on the list is on.
            state.notify_watch.connected();
            announce_notify_changes(state)?;
        }
        Event::SyncStopped(reason) => {
            let (kind, line) = match reason {
                SyncStopReason::Requested => (StatusLineKind::Info, status_log::STOPPED.to_owned()),
                SyncStopReason::Lost(message) => {
                    (StatusLineKind::Error, status_log::connection_lost(&message))
                }
                SyncStopReason::SessionEnded(message) => (
                    StatusLineKind::Error,
                    status_log::session_ended(session_server(state), &message),
                ),
            };
            log_status(state, kind, line);
            state.model.status = "continuous sync stopped".to_owned();
            state.notify_watch.disconnected();
        }
        Event::Error(error) => {
            // A refused accept leaves the invite; a later one is the user's.
            if error.operation == Operation::AcceptRoomInvite {
                let room_id = error
                    .room_id
                    .as_deref()
                    .expect("an accept error names its room");
                state.quiet_joins.remove(room_id);
            }
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
                // Channel Central shows it (`apply_events_and_refresh`):
                // the status bar and the windows say nothing.
                ErrorDestination::AskingDialog => return Ok(ActivityAttention::default()),
                ErrorDestination::ActiveWindow { command } => {
                    let error = CommandError::new(command, error.message.clone());
                    echo_lines_in_front_window(state, [error.to_string()]);
                }
                ErrorDestination::Whois => {
                    let error = CommandError::new("whois", error.message.clone());
                    let view = whois_view(state);
                    echo_lines_in_view(state, &view, [error.to_string()]);
                }
                ErrorDestination::Room(room_id) => match state.model.room_views.get_mut(room_id) {
                    Some(room_view) => {
                        room_view.push_echo(now, None, StatusLineKind::Info, context_line(&line))
                    }
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

/// The invites in `rooms` that `previous`, the session's last room list,
/// did not have: none without one, as the first list's are no news.
fn new_invites<'r>(
    previous: Option<&[RoomSummary]>,
    rooms: &'r [RoomSummary],
) -> Vec<&'r RoomSummary> {
    let Some(previous) = previous else {
        return Vec::new();
    };
    rooms
        .iter()
        .filter(|room| {
            room.membership == RoomMembership::Invite
                && !previous.iter().any(|old| {
                    old.room_id == room.room_id && old.membership == RoomMembership::Invite
                })
        })
        .collect()
}

/// An invite arrived: its line prints where Show in active: Invites says
/// (its window opens behind, at the next view sync). With Auto-join
/// channel on invite, an invite to a group room is accepted at once, its
/// window to open without becoming active.
fn announce_invite(state: &mut WindowState, room: &RoomSummary) -> Result<(), String> {
    let invite = room
        .invite
        .as_ref()
        .expect("an invite's summary names its invite");
    let text = invited_text(invite, &room.display_name);
    let chat = state.settings.settings().chat;
    match answer_view(&state.active_view, chat.active_invites) {
        ViewId::Status => log_status(state, StatusLineKind::Invite, text),
        view => push_lines_in_view(state, &view, StatusLineKind::Invite, [context_line(&text)]),
    }
    if chat.join_on_invite && !invite.direct && !room.is_space {
        submit_accept_room_invite(state, room.room_id.clone())?;
        state.quiet_joins.insert(room.room_id.clone());
    }
    Ok(())
}

/// Where `/whois` answers print: Options > Chat's Show in active: Whois.
fn whois_view(state: &WindowState) -> ViewId {
    answer_view(
        &state.active_view,
        state.settings.settings().chat.active_whois,
    )
}

/// The server the signed-in session is on, as connection lines name it.
/// Says in Status which notify list users came or went since the last
/// time, by the presence the server shared; nothing while not connected.
/// With `Show notifies in active window` the active window says it too
/// (once, when that is Status). An entry that performs `/whois` asks the
/// server for the profile of a user who came, which the active window
/// prints as a typed `/whois`. The caller refreshes the windows.
pub(super) fn announce_notify_changes(state: &mut WindowState) -> Result<(), String> {
    let notify = state.settings.settings().notify.clone();
    let presence = &state.model.presence;
    let changes = state.notify_watch.update(&notify.list, |user_id| {
        presence.is_on(user_id) == Some(true)
    });
    let active_view = echo_view(&state.active_view);
    for (user_id, on) in changes {
        let name = known_nick(&state.model, &user_id).unwrap_or_else(|| user_id.clone());
        let text = notify::change_text(&name, on);
        if notify.active && active_view != ViewId::Status {
            let line = context_line(&text);
            push_lines_in_view(state, &active_view, StatusLineKind::Notify, [line]);
        }
        log_status(state, StatusLineKind::Notify, text);
        let entry = notify
            .list
            .get(&user_id)
            .expect("a change is of a listed user");
        if on && entry.whois {
            enqueue_command(state, Command::FetchUserProfile(user_id))?;
        }
    }
    Ok(())
}

fn session_server(state: &WindowState) -> &str {
    own_server(&state.model).expect("a session that syncs is signed in")
}

/// A join the user asked for opens the room's window and makes it active,
/// as `/join` does in mIRC: the next view sync creates the window and
/// activates it (`pending_activation`). A space has no window, so the
/// Status window notes the join instead. The alias the join named, if
/// any, names the room from now on (`UiModel::joined_aliases`).
fn apply_room_joined_event(
    state: &mut WindowState,
    joined: mirx_core::RoomJoined,
) -> Result<(), String> {
    let mirx_core::RoomJoined { room_id, alias } = joined;
    if let Some(alias) = alias {
        state.model.joined_aliases.insert(alias, room_id.clone());
    }
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
    state.closed_queries.remove(&room_id);
    // An auto-join's window opens behind the active one.
    if !state.quiet_joins.remove(&room_id) {
        state.pending_activation = Some(ViewId::Room(room_id));
    }
    Ok(())
}

fn preserves_submission_status(status: &str, operation: Operation) -> bool {
    match operation {
        Operation::JoinRoom => status.starts_with("join submitted: "),
        Operation::AcceptRoomInvite => status.starts_with("accept submitted: "),
        Operation::CreateDirectRoom => status.starts_with("direct messages requested: "),
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

    // A warning already waiting for its dialog has its message posted.
    let log_out_warning_posted = state_ref(hwnd).log_out_warning.is_some();
    let mut history_backfill_anchor = None;
    let mut history_backfill_views = Vec::new();
    let mut backfill_result = false;
    // A connect in the batch: perform runs after it.
    let mut connected = false;
    let mut failed_backfill_room_id = None;
    let mut attention = ActivityAttention::default();
    for event in events {
        if let Event::E2eeRecoverySetupSucceeded(result) = &event {
            show_e2ee_recovery_key(hwnd, &result.recovery_key)?;
        }
        if let Some(dialog) = state_ref(hwnd).channel_central_dialog {
            channel_central_dialog::take_core_answer(dialog, &event);
        }
        if let Some(dialog) = state_ref(hwnd).sessions_dialog {
            sessions_dialog::take_core_answer(dialog, &event)?;
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
        // A replayed event load opens nothing, and reconnects nowhere.
        let browser_url = match &event {
            Event::SignInUrl { url }
            | Event::E2eeIdentityResetApproval { url }
            | Event::DeviceSignOutApproval { url, .. }
                if !state.harness.synthetic_load() =>
            {
                Some(url.clone())
            }
            _ => None,
        };
        let connection_lost = !state.harness.synthetic_load()
            && matches!(event, Event::SyncStopped(SyncStopReason::Lost(_)));
        connected |= !state.harness.synthetic_load() && matches!(event, Event::SyncConnected);
        attention = attention.then(apply_core_event(state, event)?);
        state.event_generation += 1;
        if let Some(url) = browser_url {
            let front = state_ref(hwnd).active_view.clone();
            open_web_link(hwnd, &front, url)?;
        }
        if connection_lost {
            reconnect::after_sync_lost(hwnd)?;
        }
    }

    sync_views_and_tree(hwnd)?;
    read_receipts::read_active_window(hwnd)?;
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
        continue_event_jump(hwnd, failed_backfill_room_id.as_deref())?;
    }
    // Posted, as the log-out warning: perform's commands may open modal
    // dialogs, which must not run inside the batch.
    if connected {
        PostMessageW(hwnd, WM_PERFORM, 0, 0);
    }
    script::post_resume_if_ready(hwnd);
    if !log_out_warning_posted && state_ref(hwnd).log_out_warning.is_some() {
        PostMessageW(hwnd, WM_LOG_OUT_WARNING, 0, 0);
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

pub(super) unsafe fn drain_core_events_and_refresh(hwnd: HWND) -> Result<usize, String> {
    let events = drain_core_events(hwnd);
    let event_count = events.len();
    apply_events_and_refresh(hwnd, events).map(|()| event_count)
}

/// Sends the read receipt for `event_id`, and records it as in flight.
pub(super) fn submit_mark_room_read(
    state: &mut WindowState,
    room_id: &str,
    event_id: String,
) -> Result<(), String> {
    let Some(sender) = state.command_sender.as_ref() else {
        return Ok(());
    };
    let command = Command::MarkRoomRead(MarkRoomReadRequest {
        room_id: room_id.to_owned(),
        event_id: event_id.clone(),
    });
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue mark room read command: {err}"))?;
    state.sent_receipts.record(room_id, &event_id);
    state.harness.record_command(command);
    Ok(())
}

/// Moves the room's read marker to `event_id`; the server's echo moves
/// the line marker.
pub(super) fn submit_set_read_marker(
    state: &mut WindowState,
    room_id: &str,
    event_id: String,
) -> Result<(), String> {
    let command = Command::SetReadMarker(SetReadMarkerRequest {
        room_id: room_id.to_owned(),
        event_id,
    });
    let Some(sender) = state.command_sender.as_ref() else {
        return Ok(());
    };
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue set read marker command: {err}"))?;
    state.harness.record_command(command);
    Ok(())
}

/// A window becomes active: its activity clears, and if it is unread, its
/// newest line goes to the server as the read receipt (`MarkRoomRead`,
/// `read_receipts::activation_receipt`), which clears the room's unread
/// counts. The read marker stays: it moves when the user leaves the window
/// (`line_marker::leave_window`). Only while connected, as the marker's
/// move: offline the receipt could only fail, with an error about something
/// the user did not ask for. Returns whether the window had activity.
pub(super) fn handle_view_activation(
    state: &mut WindowState,
    view_id: &ViewId,
) -> Result<bool, String> {
    let had_activity = state.room_activity.remove(view_id).is_some();
    if let ViewId::Room(_) | ViewId::Thread { .. } = view_id
        && state.model.connection == ConnectionState::Syncing
        && let Some(event_id) = read_receipts::view_activation_receipt(state, view_id, had_activity)
    {
        let event_id = event_id.to_owned();
        let room_id = view_id
            .room_id()
            .expect("a room or thread window")
            .to_owned();
        submit_mark_room_read(state, &room_id, event_id)?;
    }
    Ok(had_activity)
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
        Operation::SetDisplayName => "set display name",
        Operation::SetPresence => "set presence",
        Operation::SetRoomTopic => "set room topic",
        Operation::FetchRoomSettings => "fetch room settings",
        Operation::UpdateRoomSettings => "change room settings",
        Operation::SetRoomNotificationMode => "set room notifications",
        Operation::HighlightWords => "highlight words",
        Operation::AddHighlightWord => "add highlight word",
        Operation::RemoveHighlightWord => "remove highlight word",
        Operation::IgnoredUsers => "ignore list",
        Operation::IgnoreUser => "ignore user",
        Operation::UnignoreUser => "unignore user",
        Operation::FetchUserProfile => "fetch user profile",
        Operation::ListUserDevices => "list devices",
        Operation::AcceptUserIdentity => "accept identity",
        Operation::SignOutDevice => "session sign-out",
        Operation::RenameDevice => "session rename",
        Operation::SendMessage => "send text",
        Operation::SendFile => "send file",
        Operation::EditMessage => "edit message",
        Operation::SendReaction => "send reaction",
        Operation::RedactEvent => "redact event",
        Operation::MarkRoomRead => "mark room read",
        Operation::SetReadMarker => "set read marker",
        Operation::SetTyping => "send typing notice",
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
        Operation::ResetE2eeIdentity => "e2ee identity reset",
        Operation::RetryDecryption => "late decryption",
        Operation::ExportRoomKeys => "room key export",
        Operation::ImportRoomKeys => "room key import",
        Operation::Verification => "verification",
        Operation::RequestVerification => "verification request",
        Operation::Logout => "logout",
        Operation::Shutdown => "shutdown",
    }
}

/// The automation harness's part of this module: what it reads and does
/// here for the protocol.
#[cfg(any(test, feature = "automation"))]
pub(super) mod automation {
    use super::*;

    unsafe fn apply_synthetic_events_and_refresh(
        hwnd: HWND,
        events: Vec<Event>,
    ) -> Result<(), String> {
        state_mut(hwnd).harness.synthetic_load = true;
        let result = apply_events_and_refresh(hwnd, events);
        state_mut(hwnd).harness.synthetic_load = false;
        result
    }

    pub(in crate::win32) unsafe fn apply_event_load(
        hwnd: HWND,
        events: Vec<Event>,
    ) -> Result<(usize, usize), String> {
        {
            let state = state_mut(hwnd);
            state.current_session_homeserver_url = None;
            state.room_activity.clear();
            state.closed_queries.clear();
            state.quiet_joins.clear();
            state.sent_receipts.clear();
            state.find_states.clear();
            state.selected_timeline_event = None;
            state.model = initial_model();
            state.decoded_images.clear();
        }

        apply_synthetic_events_and_refresh(hwnd, events)?;

        let (rooms, timeline_events, active_view) = {
            let state = state_ref(hwnd);
            let rooms = state.model.rooms.len();
            let timeline_events = state
                .model
                .room_views
                .values()
                .map(|room_view| room_view.activities.len())
                .sum();
            // The first room's window, as after signing in.
            let active_view = state
                .model
                .rooms
                .iter()
                .find(|room| is_navigable_room(room))
                .map_or(ViewId::Status, |room| ViewId::Room(room.room_id.clone()));
            (rooms, timeline_events, active_view)
        };

        activate_view(hwnd, &active_view, None)?;

        Ok((rooms, timeline_events))
    }

    pub(in crate::win32) fn resolve_timeline_room_id(
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
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use mirx_core::{PresenceChange, PresenceKind};

    use super::super::connection::AfterSyncLost;
    use super::super::test_fixtures::*;
    use super::super::verification::verification_state_label;
    use super::*;

    #[test]
    fn synced_rooms_with_the_same_rooms_touches_no_room_view() {
        let mut state = test_state();
        let mut rooms = vec![
            room("!a:example", "A", false),
            room("!b:example", "B", false),
            invited_room("!i:example", "I"),
        ];
        rooms[0].topic = Some("alpha".to_owned());
        apply_core_event(&mut state, Event::SyncedRooms(rooms.clone())).unwrap();
        apply_core_event(&mut state, typing_event("!b:example", &["@bob:example"])).unwrap();
        let revisions = |state: &WindowState| {
            ["!a:example", "!b:example", "!i:example"].map(|id| state.model.room_views.revision(id))
        };
        let before = revisions(&state);

        apply_core_event(&mut state, Event::SyncedRooms(rooms.clone())).unwrap();
        assert_eq!(revisions(&state), before);

        rooms[1].topic = Some("beta".to_owned());
        apply_core_event(&mut state, Event::SyncedRooms(rooms.clone())).unwrap();
        let after = revisions(&state);
        assert_eq!(after[0], before[0]);
        assert_ne!(after[1], before[1]);
        assert_eq!(after[2], before[2]);
        assert_eq!(
            state.model.room_views["!b:example"].topic.as_deref(),
            Some("beta")
        );

        // Leaving a room clears its typing once.
        rooms.remove(1);
        apply_core_event(&mut state, Event::SyncedRooms(rooms.clone())).unwrap();
        let left = state.model.room_views.revision("!b:example");
        assert_ne!(left, after[1]);
        assert!(
            state.model.room_views["!b:example"]
                .typing_user_ids
                .is_empty()
        );
        apply_core_event(&mut state, Event::SyncedRooms(rooms)).unwrap();
        assert_eq!(state.model.room_views.revision("!b:example"), left);
    }

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
                assert_eq!(state.harness.opened_links, Vec::<String>::new());
            });
        }
    }

    const RESET_APPROVAL_URL: &str =
        "https://account.example/account?action=org.matrix.cross_signing_reset";

    fn status_lines_with_kinds(state: &WindowState) -> Vec<(StatusLineKind, String)> {
        state
            .model
            .status_log
            .lines()
            .iter()
            .map(|line| (line.kind, line.text.clone()))
            .collect()
    }

    /// Options > Chat's Show in active: Whois decides where `/whois` answers
    /// and their errors print: the room or thread window in front, or
    /// Status; with Status in front, Status either way.
    #[test]
    fn whois_prints_in_status_when_show_in_active_whois_is_off() {
        let whois_texts = |state: &WindowState| -> (Vec<String>, Vec<String>) {
            let in_room = render_room_lines(state, "!ops:example")
                .into_iter()
                .filter(|line| line.contains("bob"))
                .collect();
            let in_status = state
                .model
                .status_log
                .lines()
                .iter()
                .map(|line| line.text.clone())
                .filter(|text| text.contains("bob"))
                .collect();
            (in_room, in_status)
        };
        let answer_and_error = |state: &mut WindowState| {
            apply_core_event(
                state,
                Event::UserProfile(UserProfile {
                    user_id: "@bob:example".to_owned(),
                    display_name: None,
                    presence: None,
                }),
            )
            .unwrap();
            apply_core_event(
                state,
                Event::Error(CoreErrorEvent {
                    operation: Operation::FetchUserProfile,
                    room_id: None,
                    message: "no profile for @bob:example".to_owned(),
                }),
            )
            .unwrap();
        };
        for (active_whois, active, in_room) in [
            (true, ViewId::Room("!ops:example".to_owned()), true),
            (false, ViewId::Room("!ops:example".to_owned()), false),
            (true, ViewId::Status, false),
        ] {
            let mut state = test_state();
            state.model.rooms = vec![room("!ops:example", "Ops", false)];
            state
                .settings
                .update(|settings| settings.chat.active_whois = active_whois)
                .unwrap();
            state.active_view = active.clone();
            answer_and_error(&mut state);
            let (room_lines, status_lines) = whois_texts(&state);
            let (expected_room, expected_status) = if in_room { (3, 0) } else { (0, 3) };
            assert_eq!(
                (room_lines.len(), status_lines.len()),
                (expected_room, expected_status),
                "{active_whois} {active:?}: {room_lines:?} {status_lines:?}"
            );
            let lines = if in_room { room_lines } else { status_lines };
            assert!(
                lines[2].ends_with("* /whois: no profile for @bob:example"),
                "{lines:?}"
            );
        }
    }

    #[test]
    fn an_identity_reset_approval_opens_its_page_and_says_how_to_cancel() {
        unsafe {
            with_test_frame(600, 400, |hwnd| {
                apply_events_and_refresh(
                    hwnd,
                    vec![Event::E2eeIdentityResetApproval {
                        url: RESET_APPROVAL_URL.to_owned(),
                    }],
                )
                .unwrap();
                let state = state_ref(hwnd);
                assert_eq!(
                    state.harness.opened_links,
                    vec![RESET_APPROVAL_URL.to_owned()]
                );
                let lines = status_lines_with_kinds(state);
                assert_eq!(
                    lines[..2],
                    [
                        (
                            StatusLineKind::Info,
                            "* To reset your E2EE identity, approve it on the page your browser \
                             opened; mirx waits 2 minutes, /e2ee reset cancel stops it"
                                .to_owned()
                        ),
                        (
                            StatusLineKind::Info,
                            format!("* If no browser opened, go to {RESET_APPROVAL_URL}")
                        ),
                    ]
                );
            });
        }
    }

    #[test]
    fn session_answers_print_in_the_active_window_and_open_the_account_page() {
        const ACCOUNT_URL: &str =
            "https://account.example/?action=org.matrix.session_end&device_id=PH0NE";
        unsafe {
            with_test_frame(600, 400, |hwnd| {
                apply_event_load(
                    hwnd,
                    vec![
                        Event::Started,
                        Event::DeviceSignOutApproval {
                            device_id: "PH0NE".to_owned(),
                            url: ACCOUNT_URL.to_owned(),
                        },
                    ],
                )
                .unwrap();
                // A replayed load opens nothing.
                assert_eq!(state_ref(hwnd).harness.opened_links, Vec::<String>::new());
                assert_eq!(state_ref(hwnd).model.status, "waiting for session sign-out");

                apply_events_and_refresh(
                    hwnd,
                    vec![
                        Event::DeviceSignedOut("L4PT0P".to_owned()),
                        Event::DeviceRenamed {
                            device_id: "PH0NE".to_owned(),
                            display_name: "Work phone".to_owned(),
                        },
                        Event::DeviceSignOutApproval {
                            device_id: "PH0NE".to_owned(),
                            url: ACCOUNT_URL.to_owned(),
                        },
                    ],
                )
                .unwrap();
                let state = state_ref(hwnd);
                assert_eq!(state.active_view, ViewId::Status);
                assert_eq!(state.harness.opened_links, vec![ACCOUNT_URL.to_owned()]);
                let approval = [
                    "* To sign out PH0NE, confirm on the page your browser opened; /devices \
                     lists your sessions afterwards"
                        .to_owned(),
                    format!("* If no browser opened, go to {ACCOUNT_URL}"),
                ];
                let lines: Vec<_> = status_lines_with_kinds(state)
                    .into_iter()
                    .map(|(kind, text)| {
                        assert_eq!(kind, StatusLineKind::Info, "{text}");
                        text
                    })
                    .collect();
                let load = lines
                    .iter()
                    .position(|line| *line == approval[0])
                    .expect("the replayed approval's lines");
                assert_eq!(lines[load + 1], approval[1]);
                let answers = lines
                    .iter()
                    .position(|line| line == "* Session L4PT0P signed out")
                    .expect("the sign-out's line");
                assert_eq!(
                    lines[answers..answers + 4],
                    [
                        "* Session L4PT0P signed out".to_owned(),
                        "* Session PH0NE renamed \"Work phone\"".to_owned(),
                        approval[0].clone(),
                        approval[1].clone(),
                    ]
                );
            });
        }
    }

    #[test]
    fn a_replayed_identity_reset_approval_is_not_opened() {
        unsafe {
            with_test_frame(600, 400, |hwnd| {
                apply_event_load(
                    hwnd,
                    vec![
                        Event::Started,
                        Event::E2eeIdentityResetApproval {
                            url: RESET_APPROVAL_URL.to_owned(),
                        },
                    ],
                )
                .unwrap();
                let state = state_ref(hwnd);
                assert_eq!(state.harness.opened_links, Vec::<String>::new());
                assert_eq!(state.model.status, "waiting for identity reset approval");
            });
        }
    }

    #[test]
    fn an_identity_reset_ends_done_or_cancelled_with_recovery_off_in_red() {
        let mut state = test_state();
        apply_core_event(
            &mut state,
            Event::E2eeIdentityReset(E2eeIdentityResetOutcome::Done),
        )
        .unwrap();
        assert_eq!(state.model.status, "e2ee identity reset");
        apply_core_event(
            &mut state,
            Event::E2eeIdentityReset(E2eeIdentityResetOutcome::Cancelled),
        )
        .unwrap();
        assert_eq!(state.model.status, "e2ee identity reset cancelled");
        let recovery_off = (
            StatusLineKind::Error,
            "* Recovery is off: /e2ee setup to set it up again".to_owned(),
        );
        assert_eq!(
            status_lines_with_kinds(&state),
            [
                (
                    StatusLineKind::Info,
                    "* Your E2EE identity was reset; this session is verified with the new one"
                        .to_owned()
                ),
                recovery_off.clone(),
                (
                    StatusLineKind::Info,
                    "* E2EE identity reset cancelled: your identity is unchanged, but your key \
                     backup was deleted"
                        .to_owned()
                ),
                recovery_off,
            ]
        );
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
                        operation: Operation::RoomList,
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
                    Event::LogoutRefused {
                        request: keep_everything(),
                        reason: LogoutRefusal::RoomKeysAtRisk(RoomKeysAtRisk::RecoveryOff),
                    },
                    ConnectionState::Syncing,
                ),
                // A Log Out the server refuses: the session goes on syncing.
                (Event::LoggingOut, ConnectionState::LoggingOut),
                (
                    Event::LogoutRefused {
                        request: keep_everything(),
                        reason: LogoutRefusal::ServerKeepsSession {
                            device_id: "MIRXDEVICE".to_owned(),
                            reason: "busy".to_owned(),
                        },
                    },
                    ConnectionState::SignedIn,
                ),
                (Event::SyncStarted, ConnectionState::Syncing),
                (
                    Event::SyncStopped(SyncStopReason::Requested),
                    ConnectionState::SignedIn,
                ),
                // Disconnect stops a Log Out: still signed in.
                (Event::LoggingOut, ConnectionState::LoggingOut),
                (Event::LogoutCancelled, ConnectionState::SignedIn),
                (Event::LoggingOut, ConnectionState::LoggingOut),
                (
                    Event::LoggedOut(LogoutScope::Everywhere),
                    ConnectionState::SignedOut,
                ),
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
                recoverable: false,
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

    fn outgoing_waiting(room_id: &str, transaction_id: &str) -> Event {
        Event::RoomActivityUpdate(RoomActivityUpdate {
            room_id: room_id.to_owned(),
            target: RoomActivityUpdateTarget::Transaction {
                transaction_id: transaction_id.to_owned(),
            },
            kind: RoomActivityUpdateKind::MessageFailed {
                message: "error sending request".to_owned(),
                recoverable: true,
            },
        })
    }

    /// A send the connection stopped stays in the core's queue: the row
    /// waits, unmarked, through further failures and a /clear, and becomes
    /// the sent line.
    #[test]
    fn a_message_waiting_for_the_connection_becomes_the_sent_line() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();
        state.room_activity.clear();
        for _ in 0..2 {
            apply_core_event(&mut state, outgoing_waiting("!ops:example", "txn-1")).unwrap();
        }
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] <@alice:example> hello (waiting to send)".to_owned(),
            ]
        );
        assert_eq!(state.room_activity.get(&room_view_id("!ops:example")), None);
        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("the room's view")
            .clear_visible_entries();

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
                    recoverable: false,
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

    fn alice_asks(flow_id: &str) -> Event {
        Event::VerificationRequest(VerificationRequest {
            flow_id: flow_id.to_owned(),
            user_id: "@alice:example".to_owned(),
            direction: VerificationDirection::Incoming {
                from_device: "ALICEDEVICE".to_owned(),
            },
        })
    }

    fn verification_state(flow_id: &str, user_id: &str, state: VerificationState) -> Event {
        Event::VerificationState(VerificationStateEvent {
            flow_id: flow_id.to_owned(),
            user_id: user_id.to_owned(),
            device_id: None,
            state,
        })
    }

    fn flows(state: &WindowState) -> Vec<(String, &'static str)> {
        state
            .model
            .verifications
            .iter()
            .map(|verification| {
                (
                    verification.flow_id.clone(),
                    verification_state_label(&verification.state),
                )
            })
            .collect()
    }

    #[test]
    fn verification_request_event_stores_requested_state_and_logs_actions() {
        let mut state = test_state();

        apply_core_event(&mut state, alice_asks("flow-verify-123")).unwrap();

        assert_eq!(flows(&state), [("flow-verify-123".to_owned(), "requested")]);
        assert_eq!(
            status_log_lines(&state),
            vec![
                "[22:12] * Verification requested by @alice:example (session ALICEDEVICE)"
                    .to_owned(),
                "[22:12] * Actions: Tools > Accept Verification, Tools > Cancel Verification, /verify accept, /verify cancel"
                    .to_owned(),
            ]
        );
    }

    /// Until iter-77 the second flow replaced the first, and every event of
    /// either flow made its flow the current one.
    #[test]
    fn a_second_flow_joins_the_first_and_the_firsts_events_stay_its_own() {
        let mut state = test_state();
        apply_core_event(&mut state, alice_asks("flow-alice")).unwrap();
        apply_core_event(
            &mut state,
            Event::VerificationRequest(VerificationRequest {
                flow_id: "flow-bob".to_owned(),
                user_id: "@bob:example".to_owned(),
                direction: VerificationDirection::Incoming {
                    from_device: "B0B".to_owned(),
                },
            }),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            verification_state("flow-alice", "@alice:example", VerificationState::Ready),
        )
        .unwrap();

        assert_eq!(
            flows(&state),
            [
                ("flow-alice".to_owned(), "ready"),
                ("flow-bob".to_owned(), "requested")
            ]
        );
        assert_eq!(
            state
                .model
                .verifications
                .latest()
                .map(|latest| latest.flow_id.as_str()),
            Some("flow-bob")
        );
        assert_eq!(
            status_log_lines(&state)[2..],
            [
                "[22:12] * Verification requested by @bob:example (session B0B)",
                "[22:12] * Actions: Tools > Accept Verification, Tools > Cancel Verification, /verify accept, /verify cancel",
                "[22:12] * Also under way: @alice:example ALICEDEVICE (requested); /verify <action> <user> [session] answers one of them",
                "[22:12] * Verification ready with @alice:example (session ALICEDEVICE)",
                "[22:12] * Actions: /verify cancel @alice:example ALICEDEVICE",
            ]
        );
    }

    #[test]
    fn outgoing_verification_request_is_sent_with_only_cancel() {
        let mut state = test_state();
        state.model.own_account = Some(OwnAccount::matrix("@me:example".to_owned()));

        apply_core_event(
            &mut state,
            Event::VerificationRequest(VerificationRequest {
                flow_id: "flow-out".to_owned(),
                user_id: "@me:example".to_owned(),
                direction: VerificationDirection::Outgoing { to_device: None },
            }),
        )
        .unwrap();

        assert_eq!(flows(&state), [("flow-out".to_owned(), "sent")]);
        assert_eq!(
            state.model.status,
            "verification request sent to your other sessions"
        );
        assert_eq!(
            status_log_lines(&state),
            vec![
                "[22:12] * Verification request sent to your other sessions; accept it on one of them"
                    .to_owned(),
                "[22:12] * Actions: Tools > Cancel Verification, /verify cancel".to_owned(),
            ]
        );
    }

    #[test]
    fn verification_sas_ready_and_errors_are_logged_to_status() {
        let mut state = test_state();
        apply_core_event(&mut state, alice_asks("flow-verify-123")).unwrap();
        state.model.status_log = StatusLog::default();
        apply_core_event(
            &mut state,
            verification_state(
                "flow-verify-123",
                "@alice:example",
                VerificationState::SasReady {
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
            ),
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
                "[22:12] * Verification with @alice:example (session ALICEDEVICE): compare the emoji or the numbers"
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
        // A new session: the account's keys are on the server, not here.
        let status = E2eeStatus {
            session: E2eeSessionState::Unverified,
            ..e2ee_status(
                E2eeRecoveryState::Incomplete,
                E2eeBackupState::Enabled,
                false,
                Some(true),
                Some(false),
            )
        };

        apply_core_event(&mut state, Event::E2eeStatus(status.clone())).unwrap();

        assert_eq!(state.model.last_e2ee_status, Some(status));
        assert_eq!(state.model.status, "e2ee status updated");
        assert_eq!(
            status_log_lines(&state),
            vec![
                "[22:12] * E2EE: recovery incomplete, local backup disabled, server backup exists, cross-signing incomplete, this session not verified"
                    .to_owned(),
                "[22:12] * Use /e2ee recover to restore keys"
                    .to_owned(),
                format!("[22:12] * {}", status_log::SESSION_UNVERIFIED_WARNING),
            ]
        );
        assert_eq!(
            state
                .model
                .status_log
                .lines()
                .iter()
                .map(|line| line.kind)
                .collect::<Vec<_>>(),
            [
                StatusLineKind::Info,
                StatusLineKind::Info,
                StatusLineKind::Error
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

        state.model.status = "direct messages requested: @alice:example.org".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::CreateDirectRoom,
                room_id: None,
                message: "create direct room requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(
            state.model.status,
            "direct messages requested: @alice:example.org"
        );
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
    fn verification_done_logs_terminal_text_without_actions_and_ends_the_flow() {
        let mut state = test_state();
        apply_core_event(&mut state, alice_asks("flow-verify-123")).unwrap();
        apply_core_event(
            &mut state,
            verification_state("flow-verify-123", "@alice:example", VerificationState::Done),
        )
        .unwrap();

        assert_eq!(
            status_log_lines(&state)[2..],
            ["[22:12] * Verification done with @alice:example (session ALICEDEVICE): verified"]
        );
        assert!(flows(&state).is_empty());
    }
    #[test]
    fn verification_cancelled_logs_reason_code_without_actions_and_ends_the_flow() {
        let mut state = test_state();
        apply_core_event(&mut state, alice_asks("flow-verify-123")).unwrap();
        apply_core_event(
            &mut state,
            verification_state(
                "flow-verify-123",
                "@alice:example",
                VerificationState::Cancelled {
                    reason: "timed out".to_owned(),
                    code: "m.timeout".to_owned(),
                    cancelled_by_us: false,
                },
            ),
        )
        .unwrap();

        assert_eq!(
            status_log_lines(&state)[2..],
            [
                "[22:12] * Verification cancelled with @alice:example (session ALICEDEVICE): timed out (m.timeout)"
            ]
        );
        assert!(flows(&state).is_empty());
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
                line_edits: Default::default(),
                typing_user_ids: vec!["@alice:example.org".to_owned()],
                members: Vec::new(),
                history: RoomHistoryBackfillState::default(),
                echo: EchoLog::default(),
                arrivals: RoomArrivals::default(),
                identity_warnings: BTreeMap::new(),
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
        apply_core_event(&mut state, alice_asks("flow")).unwrap();
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
            apply_core_event(&mut state, Event::LoggedOut(LogoutScope::Everywhere))
                .expect("logged out event must apply successfully");
        });
        let removed = take_stale_room_views(&mut state);

        assert!(state.model.rooms.is_empty());
        assert_eq!(state.model.highlight_words, None);
        assert_eq!(state.model.selected_room, None);
        assert!(state.model.room_views.is_empty());
        assert_eq!(state.model.last_activity, None);
        assert_eq!(state.model.last_e2ee_status, None);
        assert_eq!(state.model.verifications.len(), 0);
        assert_eq!(state.model.last_error, None);
        assert!(state.find_states.is_empty());
        assert_eq!(removed.len(), 1);
        assert!(state.views.contains_key(&ViewId::Status));
        assert!(
            !state
                .views
                .contains_key(&ViewId::Room("!room:example".to_owned()))
        );
    }

    /// A File > Log Out's request: nothing allowed yet.
    fn keep_everything() -> LogoutRequest {
        LogoutRequest {
            discard_room_keys: false,
            this_computer_only: false,
        }
    }

    /// Log Out's refusal waits for its dialog, with the request it refused,
    /// and says nothing else: the session goes on as it was.
    #[test]
    fn a_logout_refusal_waits_for_its_dialog_until_logged_out() {
        for (reason, status) in [
            (
                LogoutRefusal::RoomKeysAtRisk(RoomKeysAtRisk::RecoveryOff),
                "log out: room keys at risk",
            ),
            (
                LogoutRefusal::ServerKeepsSession {
                    device_id: "MIRXDEVICE".to_owned(),
                    reason: "unreachable".to_owned(),
                },
                "log out: the server kept the session",
            ),
        ] {
            let mut state = test_state();
            state.model.own_account = Some(OwnAccount::matrix("@me:example".to_owned()));
            state.model.connection = ConnectionState::Syncing;
            state.model.lifecycle_state = LifecycleState::Syncing;
            let lines = status_log_lines(&state);

            apply_core_event(
                &mut state,
                Event::LogoutRefused {
                    request: keep_everything(),
                    reason: reason.clone(),
                },
            )
            .unwrap();

            assert_eq!(state.log_out_warning, Some((keep_everything(), reason)));
            assert_eq!(state.model.status, status);
            assert_eq!(state.model.connection, ConnectionState::Syncing);
            assert_eq!(state.model.lifecycle_state, LifecycleState::Syncing);
            assert_eq!(status_log_lines(&state), lines);

            with_local_app_data(|_| {
                apply_core_event(&mut state, Event::LoggedOut(LogoutScope::Everywhere)).unwrap()
            });
            assert_eq!(state.log_out_warning, None);
        }
    }

    /// A Log Out from this computer only says what it left on the server,
    /// and where to finish it; a whole one just says it logged out.
    #[test]
    fn logging_out_says_whether_the_server_still_lists_the_session() {
        for (scope, line) in [
            (LogoutScope::Everywhere, "* Logged out".to_owned()),
            (
                LogoutScope::ThisComputerOnly {
                    device_id: "MIRXDEVICE".to_owned(),
                },
                "* Logged out on this computer only: example still lists this session \
                 (MIRXDEVICE) until you sign it out from another session (Tools > Sessions... \
                 there)"
                    .to_owned(),
            ),
        ] {
            let mut state = test_state();
            state.model.own_account = Some(OwnAccount::matrix("@me:example".to_owned()));
            state.model.connection = ConnectionState::SignedIn;

            with_local_app_data(|_| apply_core_event(&mut state, Event::LoggedOut(scope)).unwrap());

            assert_eq!(state.model.connection, ConnectionState::SignedOut);
            assert_eq!(state.model.status, "logged out");
            let lines = status_log_lines(&state);
            assert!(
                lines.last().is_some_and(|last| last.ends_with(&line)),
                "{lines:?}"
            );
        }
    }

    /// An invite from Bob, as the fixtures' `invited_room`, of `kind`.
    fn invite_of(room_id: &str, name: &str, direct: bool, is_space: bool) -> RoomSummary {
        let mut room = invited_room(room_id, name);
        room.invite.as_mut().expect("an invite").direct = direct;
        room.is_space = is_space;
        room
    }

    fn invite_lines(lines: impl IntoIterator<Item = String>) -> Vec<String> {
        lines
            .into_iter()
            .filter(|line| line.contains("invites you"))
            .collect()
    }

    fn status_invite_lines(state: &WindowState) -> Vec<(StatusLineKind, String)> {
        status_lines_with_kinds(state)
            .into_iter()
            .filter(|(_, text)| text.contains("invites you"))
            .collect()
    }

    fn ops_invite_lines(state: &WindowState) -> Vec<(StatusLineKind, String)> {
        state.model.room_views["!ops:example"]
            .echo
            .lines_for(None)
            .filter(|line| line.text.contains("invites you"))
            .map(|line| (line.kind, line.text.clone()))
            .collect()
    }

    /// Show in active: Invites: an invite that a room list brings prints
    /// in the active window, or Status; the session's first list's
    /// invites are no news, and neither is one listed before.
    #[test]
    fn invites_are_announced_once_and_not_on_the_first_room_list() {
        let (mut state, handle) = test_state_with_command_sender();
        let ops = room("!ops:example", "Ops", false);
        let listed = |state: &mut WindowState, rooms: Vec<RoomSummary>| {
            apply_core_event(state, Event::SyncedRooms(rooms)).unwrap();
        };
        state.active_view = room_view_id("!ops:example");
        state
            .model
            .room_views
            .entry_or_default("!ops:example".to_owned());

        let x = invited_room("!x:example", "Xray");
        listed(&mut state, vec![ops.clone(), x.clone()]);
        assert_eq!(ops_invite_lines(&state), []);
        assert_eq!(status_invite_lines(&state), []);

        let y = invited_room("!y:example", "Yankee");
        for _ in 0..2 {
            listed(&mut state, vec![ops.clone(), x.clone(), y.clone()]);
        }
        let line = "* Bob (@bob:example) invites you to join Yankee".to_owned();
        assert_eq!(
            ops_invite_lines(&state),
            [(StatusLineKind::Invite, line.clone())]
        );
        assert_eq!(status_invite_lines(&state), []);
        // Its own window shows the same line.
        assert!(
            invite_lines(render_room_lines(&state, "!y:example")).contains(&line),
            "{:?}",
            render_room_lines(&state, "!y:example")
        );

        state
            .settings
            .update(|settings| settings.chat.active_invites = false)
            .unwrap();
        let z = invite_of("!z:example", "Zulu", true, false);
        listed(&mut state, vec![ops.clone(), x.clone(), y.clone(), z]);
        assert_eq!(ops_invite_lines(&state).len(), 1);
        assert_eq!(
            status_invite_lines(&state),
            [(
                StatusLineKind::Invite,
                "* Bob (@bob:example) invites you to direct messages".to_owned()
            )]
        );
        // Nothing was accepted: auto-join is off.
        assert_eq!(state.harness.commands, []);

        // A new session's first list is no news either.
        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut(LogoutScope::Everywhere)).unwrap()
        });
        let before = status_invite_lines(&state).len();
        listed(&mut state, vec![invited_room("!w:example", "Whiskey")]);
        assert_eq!(status_invite_lines(&state).len(), before);
        stop_test_core(handle);
    }

    /// Auto-join channel on invite: an arriving invite to a group room is
    /// accepted, and its join opens the window behind the active one; a
    /// query or a space invite is not; the user's own accept activates.
    #[test]
    fn auto_join_accepts_channel_invites_without_taking_the_focus() {
        let (mut state, handle) = test_state_with_command_sender();
        state
            .settings
            .update(|settings| settings.chat.join_on_invite = true)
            .unwrap();
        let ops = room("!ops:example", "Ops", false);
        state.active_view = room_view_id("!ops:example");
        apply_core_event(&mut state, Event::SyncedRooms(vec![ops.clone()])).unwrap();
        assert_eq!(state.harness.commands, []);

        let y = invited_room("!y:example", "Yankee");
        let z = invite_of("!z:example", "Zulu", true, false);
        let space = invite_of("!s:example", "Space", false, true);
        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![ops.clone(), y, z.clone(), space]),
        )
        .unwrap();
        let accept = |room_id: &str| {
            Command::AcceptRoomInvite(AcceptRoomInviteRequest {
                room_id: room_id.to_owned(),
            })
        };
        assert_eq!(state.harness.commands, [accept("!y:example")]);
        // The lines print first, all three.
        assert_eq!(ops_invite_lines(&state).len(), 3);

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                ops.clone(),
                room("!y:example", "Yankee", false),
                z.clone(),
            ]),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomJoined(mirx_core::RoomJoined {
                room_id: "!y:example".to_owned(),
                alias: None,
            }),
        )
        .unwrap();
        assert_eq!(state.pending_activation, None);
        assert!(state.quiet_joins.is_empty());

        // The user's own accept of the query activates it on its join.
        let window = room_view_id("!z:example");
        submit_accept_slash_command(&mut state, &window, None).unwrap();
        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                ops.clone(),
                room("!y:example", "Yankee", false),
                query("!z:example", "Zulu", "@bob:example"),
            ]),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomJoined(mirx_core::RoomJoined {
                room_id: "!z:example".to_owned(),
                alias: None,
            }),
        )
        .unwrap();
        assert_eq!(state.pending_activation, Some(window));

        // A refused auto-join leaves the invite to the user.
        let v = invited_room("!v:example", "Victor");
        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                ops.clone(),
                room("!y:example", "Yankee", false),
                query("!z:example", "Zulu", "@bob:example"),
                v,
            ]),
        )
        .unwrap();
        assert!(state.quiet_joins.contains("!v:example"));
        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::AcceptRoomInvite,
                room_id: Some("!v:example".to_owned()),
                message: "forbidden".to_owned(),
            }),
        )
        .unwrap();
        assert!(state.quiet_joins.is_empty());
        stop_test_core(handle);
    }

    /// A Log Out under way says so, in Status and the status bar.
    #[test]
    fn a_log_out_under_way_says_so() {
        let mut state = test_state();
        state.model.connection = ConnectionState::Syncing;
        let lines = status_log_lines(&state);

        apply_core_event(&mut state, Event::LoggingOut).unwrap();

        assert_eq!(state.model.connection, ConnectionState::LoggingOut);
        assert_eq!(state.model.status, "logging out");
        let new_lines = &status_log_lines(&state)[lines.len()..];
        assert_eq!(new_lines.len(), 1, "{new_lines:?}");
        assert!(
            new_lines[0].ends_with(status_log::LOGGING_OUT),
            "{new_lines:?}"
        );

        // Disconnect stopped it: signed in, and retries start over.
        let settings = state.settings.settings().connect.clone();
        state.reconnect.sync_started();
        state.reconnect.sync_lost(&settings);
        state.reconnect.retry();
        apply_core_event(&mut state, Event::LogoutCancelled).unwrap();

        assert_eq!(state.model.connection, ConnectionState::SignedIn);
        assert_eq!(state.model.status, "log out cancelled");
        assert_eq!(state.reconnect.attempts(), 0);
        let new_lines = &status_log_lines(&state)[lines.len() + 1..];
        assert_eq!(new_lines.len(), 1, "{new_lines:?}");
        assert!(
            new_lines[0].ends_with(status_log::LOG_OUT_CANCELLED),
            "{new_lines:?}"
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
            apply_core_event(&mut state, Event::LoggedOut(LogoutScope::Everywhere))
                .expect("logged out event must apply successfully");
        });

        assert_eq!(state.model.last_e2ee_status, None);
    }

    #[test]
    fn closed_query_reopens_on_message_activity_from_others() {
        let mut state = test_state();
        state.model.rooms = vec![query("!dm:example", "Alice", "@alice:example")];
        state.model.own_account = Some(OwnAccount::matrix("@me:example".to_owned()));
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
            alias: None,
        })
    }

    #[test]
    fn room_joined_asks_the_view_sync_to_activate_the_joined_room() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!ops:example", "Ops", false),
            room("!new:example", "New", false),
        ];
        state.active_view = room_view_id("!ops:example");
        state.model.selected_room = Some("!ops:example".to_owned());

        apply_core_event(&mut state, room_joined("!new:example")).unwrap();

        // Only the activation makes it the active view, entering it.
        assert_eq!(state.pending_activation, Some(room_view_id("!new:example")));
        assert_eq!(state.active_view, room_view_id("!ops:example"));
        assert_eq!(state.model.selected_room.as_deref(), Some("!ops:example"));
    }

    #[test]
    fn apply_room_joined_records_the_alias() {
        let mut state = test_state();
        state.model.rooms = vec![room("!new:example", "!new:example", false)];

        apply_core_event(
            &mut state,
            Event::RoomJoined(mirx_core::RoomJoined {
                room_id: "!new:example".to_owned(),
                alias: Some("#new:example".to_owned()),
            }),
        )
        .unwrap();

        assert_eq!(
            state.model.joined_aliases,
            BTreeMap::from([("#new:example".to_owned(), "!new:example".to_owned())])
        );
        // The session's: a log out forgets it.
        apply_core_event(&mut state, Event::LoggedOut(LogoutScope::Everywhere)).unwrap();
        assert!(state.model.joined_aliases.is_empty());
    }

    #[test]
    fn room_joined_reopens_a_closed_query() {
        let mut state = test_state();
        state.model.rooms = vec![query("!dm:example", "Alice", "@alice:example")];
        state.closed_queries.insert("!dm:example".to_owned());
        state.active_view = ViewId::Status;

        apply_core_event(&mut state, room_joined("!dm:example")).unwrap();

        assert!(state.closed_queries.is_empty());
        assert_eq!(state.pending_activation, Some(room_view_id("!dm:example")));
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
        assert_eq!(state.pending_activation, None);
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
        state.model.own_account = Some(OwnAccount::matrix("@me:example".to_owned()));
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

        state.harness.synthetic_load = true;
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
    fn inactive_event_backed_messages_mark_room_activity() {
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
    }

    #[test]
    fn synthetic_inactive_messages_mark_activity() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());
        state.harness.synthetic_load = true;

        apply_core_event(&mut state, timeline("!inactive:example", "hello")).unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );
    }

    #[test]
    fn inactive_unsupported_messages_mark_room_activity() {
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
    }

    #[test]
    fn inactive_image_messages_mark_room_activity() {
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
    }

    #[test]
    fn inactive_file_messages_mark_room_activity() {
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
    }

    #[test]
    fn inactive_audio_messages_mark_room_activity() {
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
    }

    #[test]
    fn inactive_state_notices_mark_room_activity() {
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
    fn thread_view_is_not_treated_as_active_room_for_room_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = thread_view_id("!room:example", "$thread-root");

        apply_core_event(&mut state, timeline("!room:example", "hello")).unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
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
    fn server_read_state_is_kept_and_clears_room_activity_without_command() {
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
            state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert_eq!(
            state.model.room_read_states.get("!room:example"),
            Some(&RoomReadState {
                room_id: "!room:example".to_owned(),
                latest_active_event_id: Some("$event".to_owned()),
                fully_read_event_id: None,
                fully_read_timestamp_ms: None,
                marked_unread: false,
                num_unread: 0,
                num_notifications: 0,
                num_mentions: 0,
            })
        );
        assert!(state.harness.commands.is_empty());
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
    fn synced_rooms_prune_stale_room_read_states() {
        let mut state = test_state();
        state.model.room_read_states.insert(
            "!keep:example".to_owned(),
            RoomReadState {
                room_id: "!keep:example".to_owned(),
                latest_active_event_id: Some("$keep:example".to_owned()),
                fully_read_event_id: None,
                fully_read_timestamp_ms: None,
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
                fully_read_timestamp_ms: None,
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
                fully_read_timestamp_ms: None,
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
    fn logged_out_clears_room_activity() {
        let mut state = test_state();
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut(LogoutScope::Everywhere))
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
                fully_read_timestamp_ms: None,
                marked_unread: true,
                num_unread: 1,
                num_notifications: 2,
                num_mentions: 1,
            },
        );

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut(LogoutScope::Everywhere))
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
    fn activation_of_a_room_unread_from_the_server_sends_its_newest_line() {
        let mut state = test_state();
        state.model.connection = ConnectionState::Syncing;
        let (sender, _commands) = inert_command_sender();
        state.command_sender = Some(sender);
        // The initial sync's lines come before the rooms are known, so
        // they raise no activity.
        for (event_id, timestamp_ms) in [("$a", 1_700_000_000_000), ("$b", 1_700_000_001_000)] {
            apply_core_event(
                &mut state,
                Event::RoomActivity(Box::new(sent_message_activity(
                    "!room:example",
                    event_id,
                    None,
                    "hi",
                    timestamp_ms,
                ))),
            )
            .unwrap();
        }
        apply_core_event(
            &mut state,
            outgoing_queued("!room:example", "txn1", "sending"),
        )
        .unwrap();
        assert!(state.room_activity.is_empty());
        state.model.rooms = vec![room("!room:example", "Room", false)];
        // The colour comes from the server's counts.
        apply_core_event(
            &mut state,
            room_read_state("!room:example", None, None, false, 2, 0, 0),
        )
        .unwrap();
        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );

        assert_eq!(
            handle_view_activation(&mut state, &room_view_id("!room:example")),
            Ok(true)
        );
        assert_eq!(
            state.harness.commands,
            vec![Command::MarkRoomRead(MarkRoomReadRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$b".to_owned(),
            })]
        );
    }

    #[test]
    fn activation_while_not_syncing_sends_no_receipt() {
        let mut state = test_state();
        let (sender, _commands) = inert_command_sender();
        state.command_sender = Some(sender);
        state.model.rooms = vec![room("!room:example", "Room", false)];
        apply_core_event(
            &mut state,
            thread_timeline("!room:example", "$thread-1", "$thread-root", "thread reply"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_read_state("!room:example", None, None, false, 2, 0, 0),
        )
        .unwrap();
        assert_ne!(state.model.connection, ConnectionState::Syncing);

        for view_id in [
            room_view_id("!room:example"),
            thread_view_id("!room:example", "$thread-root"),
        ] {
            assert_eq!(handle_view_activation(&mut state, &view_id), Ok(true));
        }
        assert!(state.harness.commands.is_empty());
    }

    #[test]
    fn room_activation_of_a_read_room_sends_nothing() {
        let mut state = test_state();
        let (sender, _commands) = inert_command_sender();
        state.command_sender = Some(sender);
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .model
            .room_views
            .entry_or_default("!room:example")
            .apply_activity(sent_message_activity("!room:example", "$a", None, "hi", 1));
        apply_core_event(
            &mut state,
            room_read_state("!room:example", Some("$a"), None, false, 0, 0, 0),
        )
        .unwrap();

        assert_eq!(
            handle_view_activation(&mut state, &room_view_id("!room:example")),
            Ok(false)
        );
        assert!(state.harness.commands.is_empty());
    }

    #[test]
    fn room_activation_without_lines_clears_room_activity_without_command() {
        let mut state = test_state();
        let (sender, _commands) = inert_command_sender();
        state.command_sender = Some(sender);
        state.model.rooms = vec![room("!room:example", "Room", false)];
        apply_core_event(
            &mut state,
            room_read_state("!room:example", None, None, true, 1, 0, 0),
        )
        .unwrap();
        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
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
        assert!(state.harness.commands.is_empty());
    }

    #[test]
    fn thread_activation_after_activity_clears_only_thread_activity_and_enqueues_mark_room_read() {
        let mut state = test_state();
        state.model.connection = ConnectionState::Syncing;
        let (sender, _commands) = inert_command_sender();
        state.command_sender = Some(sender);
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);
        apply_core_event(
            &mut state,
            thread_timeline("!room:example", "$thread-1", "$thread-root", "thread reply"),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!room:example", "main line")).unwrap();

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
        assert_eq!(
            state.harness.commands,
            vec![Command::MarkRoomRead(MarkRoomReadRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$thread-1".to_owned(),
            })]
        );
        // Again, without activity since: nothing.
        state.harness.commands.clear();
        handle_view_activation(&mut state, &thread_view_id("!room:example", "$thread-root"))
            .unwrap();
        assert!(state.harness.commands.is_empty());
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
        assert!(state.harness.commands.is_empty());

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

    /// Presence prints nothing; Log Out forgets it with the session.
    #[test]
    fn presence_changes_fill_the_book_until_logout() {
        with_local_app_data(|_| {
            let mut state = test_state();
            state.active_view = ViewId::Status;
            let status_lines = status_log_texts(&state).len();
            apply_core_event(
                &mut state,
                Event::PresenceChanged(vec![PresenceChange {
                    user_id: "@bob:example".to_owned(),
                    kind: PresenceKind::Unavailable,
                    status_msg: Some("lunch".to_owned()),
                }]),
            )
            .unwrap();
            assert_eq!(
                state.model.presence.kind("@bob:example"),
                Some(PresenceKind::Unavailable)
            );
            assert_eq!(status_log_texts(&state).len(), status_lines);

            apply_core_event(&mut state, Event::LoggedOut(LogoutScope::Everywhere)).unwrap();
            assert_eq!(state.model.presence.kind("@bob:example"), None);
        });
    }

    fn presence(user_id: &str, kind: PresenceKind) -> Event {
        Event::PresenceChanged(vec![PresenceChange {
            user_id: user_id.to_owned(),
            kind,
            status_msg: None,
        }])
    }

    /// The Status lines after the first `from`, with their kinds.
    fn status_lines_since(state: &WindowState, from: usize) -> Vec<(StatusLineKind, &str)> {
        state.model.status_log.lines()[from..]
            .iter()
            .map(|line| (line.kind, line.text.as_str()))
            .collect()
    }

    /// Listed users' comings and goings print in Status, by state, only
    /// while connected: the stored presence before the connection says
    /// nothing, the connection calls the roll, a disconnect stops them
    /// and a reconnect calls the roll again.
    #[test]
    fn notify_lines_follow_presence_while_connected() {
        with_local_app_data(|_| {
            let mut state = test_state();
            state
                .settings
                .update(|settings| {
                    for user_id in ["@carol:example", "@bob:example", "@dave:example"] {
                        settings.notify.list.add(user_id.to_owned(), None, false);
                    }
                })
                .unwrap();
            apply_core_event(
                &mut state,
                Event::LoggedIn {
                    session: stored_session(),
                    homeserver_url: "https://matrix.example".to_owned(),
                },
            )
            .unwrap();
            apply_core_event(
                &mut state,
                Event::RoomMembers(RoomMembers {
                    room_id: "!ops:example".to_owned(),
                    members: vec![member_summary("@bob:example", "Bob", false)],
                }),
            )
            .unwrap();
            let start = state.model.status_log.lines().len();
            // The store's presence of the last run.
            apply_core_event(&mut state, presence("@bob:example", PresenceKind::Online)).unwrap();
            apply_core_event(
                &mut state,
                presence("@carol:example", PresenceKind::Unavailable),
            )
            .unwrap();
            apply_core_event(&mut state, presence("@dave:example", PresenceKind::Offline)).unwrap();
            apply_core_event(&mut state, Event::SyncStarted).unwrap();
            assert_eq!(status_lines_since(&state, start).len(), 1);

            let connected = state.model.status_log.lines().len();
            apply_core_event(&mut state, Event::SyncConnected).unwrap();
            // Repeats, and online <-> away, say nothing.
            apply_core_event(&mut state, presence("@bob:example", PresenceKind::Online)).unwrap();
            apply_core_event(&mut state, presence("@carol:example", PresenceKind::Online)).unwrap();
            apply_core_event(&mut state, presence("@bob:example", PresenceKind::Offline)).unwrap();
            apply_core_event(&mut state, presence("@dave:example", PresenceKind::Online)).unwrap();
            // Not listed.
            apply_core_event(&mut state, presence("@erin:example", PresenceKind::Online)).unwrap();
            assert_eq!(
                status_lines_since(&state, connected),
                [
                    (StatusLineKind::Notify, "* @carol:example is on Matrix"),
                    (StatusLineKind::Notify, "* Bob is on Matrix"),
                    (StatusLineKind::Notify, "* Bob has left Matrix"),
                    (StatusLineKind::Notify, "* @dave:example is on Matrix"),
                ]
            );

            apply_core_event(&mut state, Event::SyncStopped(SyncStopReason::Requested)).unwrap();
            let stopped = state.model.status_log.lines().len();
            apply_core_event(&mut state, presence("@bob:example", PresenceKind::Online)).unwrap();
            apply_core_event(&mut state, Event::SyncStarted).unwrap();
            apply_core_event(&mut state, Event::SyncConnected).unwrap();
            assert_eq!(
                status_lines_since(&state, stopped),
                [
                    (StatusLineKind::Info, "* Syncing with the homeserver"),
                    (StatusLineKind::Notify, "* @carol:example is on Matrix"),
                    (StatusLineKind::Notify, "* Bob is on Matrix"),
                    (StatusLineKind::Notify, "* @dave:example is on Matrix"),
                ]
            );

            apply_core_event(&mut state, Event::LoggedOut(LogoutScope::Everywhere)).unwrap();
            let logged_out = state.model.status_log.lines().len();
            apply_core_event(&mut state, presence("@bob:example", PresenceKind::Offline)).unwrap();
            assert_eq!(status_lines_since(&state, logged_out), []);
            // The list is the user's: it stays.
            assert_eq!(state.settings.settings().notify.list.entries().len(), 3);
        });
    }

    /// With `Show notifies in active window` the active room says it too,
    /// Status once when it is active; an entry that performs `/whois`
    /// asks for the profile of a user who came, not of one who went.
    #[test]
    fn notifies_in_the_active_window_and_whois_on_notify() {
        with_local_app_data(|_| {
            let mut state = test_state();
            let (sender, commands) = inert_command_sender();
            state.command_sender = Some(sender);
            state
                .settings
                .update(|settings| {
                    settings.notify.active = true;
                    settings
                        .notify
                        .list
                        .add("@bob:example".to_owned(), None, true);
                    settings
                        .notify
                        .list
                        .add("@carol:example".to_owned(), None, false);
                })
                .unwrap();
            apply_core_event(
                &mut state,
                Event::LoggedIn {
                    session: stored_session(),
                    homeserver_url: "https://matrix.example".to_owned(),
                },
            )
            .unwrap();
            apply_core_event(
                &mut state,
                Event::SyncedRooms(vec![room("!ops:example", "Ops", false)]),
            )
            .unwrap();
            state.active_view = ViewId::Room("!ops:example".to_owned());
            apply_core_event(&mut state, Event::SyncConnected).unwrap();
            let start = state.model.status_log.lines().len();
            apply_core_event(&mut state, presence("@bob:example", PresenceKind::Online)).unwrap();
            apply_core_event(&mut state, presence("@carol:example", PresenceKind::Online)).unwrap();
            apply_core_event(&mut state, presence("@bob:example", PresenceKind::Offline)).unwrap();
            let changes = [
                "* @bob:example is on Matrix",
                "* @carol:example is on Matrix",
                "* @bob:example has left Matrix",
            ];
            assert_eq!(
                status_lines_since(&state, start),
                changes.map(|text| (StatusLineKind::Notify, text))
            );
            let room_lines = render_room_lines(&state, "!ops:example");
            assert_eq!(
                &room_lines[room_lines.len() - 3..],
                changes.map(|text| format!("[22:12] {text}"))
            );
            assert_eq!(
                state.harness.commands,
                [Command::FetchUserProfile("@bob:example".to_owned())]
            );

            // Status active: once.
            state.active_view = ViewId::Status;
            let start = state.model.status_log.lines().len();
            apply_core_event(&mut state, presence("@bob:example", PresenceKind::Online)).unwrap();
            assert_eq!(
                status_lines_since(&state, start),
                [(StatusLineKind::Notify, "* @bob:example is on Matrix")]
            );
            assert_eq!(state.harness.commands.len(), 2);
            drop(commands);
        });
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
                members: vec![member_summary("@bob:example", "Bob", false)],
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
                .entry_or_default(room_id.to_owned())
                .history = RoomHistoryBackfillState::Idle {
                next: HistoryFrom::Token {
                    token: "token-2".to_owned(),
                },
            };
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
            state.model.room_views["!other:example"].history,
            RoomHistoryBackfillState::Idle {
                next: HistoryFrom::Token {
                    token: "token-2".to_owned(),
                },
            }
        );
        assert!(matches!(
            state.model.room_views["!room:example"].history,
            RoomHistoryBackfillState::Loading { .. }
        ));

        apply_core_event(
            &mut state,
            room_error(Operation::BackfillRoomHistory, "!room:example", "offline"),
        )
        .unwrap();
        assert_eq!(
            state.model.room_views["!room:example"].history,
            RoomHistoryBackfillState::Failed {
                from: HistoryFrom::Token {
                    token: "token-2".to_owned(),
                },
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
            assert_eq!(state.model.own_user_id(), Some("@alice:example"));
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

            apply_core_event(&mut state, Event::LoggedOut(LogoutScope::Everywhere)).unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::LoggedOut);
            assert_eq!(state.model.own_account, None);
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
            state.harness.synthetic_load = true;

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
            apply_core_event(&mut state, Event::SyncConnected).unwrap();
            assert_eq!(state.model.status, "connected to example");
            apply_core_event(
                &mut state,
                Event::SyncStopped(SyncStopReason::Lost("boom".to_owned())),
            )
            .unwrap();
            apply_core_event(&mut state, Event::SyncStarted).unwrap();
            apply_core_event(
                &mut state,
                Event::SyncStopped(SyncStopReason::SessionEnded("token gone".to_owned())),
            )
            .unwrap();
            apply_core_event(&mut state, Event::LoggedOut(LogoutScope::Everywhere)).unwrap();
            apply_core_event(&mut state, Event::Stopped).unwrap();

            assert_eq!(state.model.own_account, None);
            assert_eq!(
                status_log_lines(&state),
                vec![
                    "[22:12] * Logged in as @alice:example (session DEVICE)".to_owned(),
                    "[22:12] * Syncing with the homeserver".to_owned(),
                    "[22:12] * Synced rooms: 1 joined, 1 invited".to_owned(),
                    "[22:12] * Disconnected".to_owned(),
                    "[22:12] * Syncing with the homeserver".to_owned(),
                    "[22:12] * Disconnected: boom".to_owned(),
                    "[22:12] * Syncing with the homeserver".to_owned(),
                    "[22:12] * Disconnected: example signed this session out (token gone). /e2ee export saves its room keys; File > Log Out ends it, then sign in again.".to_owned(),
                    "[22:12] * Logged out".to_owned(),
                    "[22:12] * Disconnected".to_owned(),
                ]
            );
        });
    }

    /// The connection's return is news only after it was lost; the
    /// retries count from then until it is back.
    #[test]
    fn a_connection_that_comes_back_says_so_and_ends_the_retries() {
        with_local_app_data(|_| {
            let mut state = test_state();
            state.harness.synthetic_load = true;
            apply_core_event(
                &mut state,
                Event::LoggedIn {
                    session: stored_session(),
                    homeserver_url: "https://matrix.example".to_owned(),
                },
            )
            .unwrap();
            apply_core_event(&mut state, Event::SyncStarted).unwrap();
            apply_core_event(&mut state, Event::SyncConnected).unwrap();
            // As the frame does when the core reports the loss.
            let settings = state.settings.settings().connect.clone();
            apply_core_event(
                &mut state,
                Event::SyncStopped(SyncStopReason::Lost("reset".to_owned())),
            )
            .unwrap();
            assert_eq!(
                state.reconnect.sync_lost(&settings),
                AfterSyncLost::ReconnectNow
            );
            apply_core_event(&mut state, Event::SyncStarted).unwrap();
            apply_core_event(
                &mut state,
                Event::SyncStopped(SyncStopReason::Lost("refused".to_owned())),
            )
            .unwrap();
            assert_eq!(
                state.reconnect.sync_lost(&settings),
                AfterSyncLost::RetryAfter(Duration::from_secs(10))
            );
            assert_eq!(state.reconnect.retry(), 1);
            apply_core_event(&mut state, Event::SyncStarted).unwrap();
            apply_core_event(&mut state, Event::SyncConnected).unwrap();

            assert_eq!(state.reconnect.attempts(), 0);
            assert_eq!(state.model.status, "connected to example");
            assert_eq!(
                status_log_lines(&state),
                vec![
                    "[22:12] * Logged in as @alice:example (session DEVICE)".to_owned(),
                    "[22:12] * Syncing with the homeserver".to_owned(),
                    // The reconnect's and the retry's own lines (the
                    // frame's) say it syncs again.
                    "[22:12] * Disconnected: reset".to_owned(),
                    "[22:12] * Disconnected: refused".to_owned(),
                    "[22:12] * Reconnected to example".to_owned(),
                ]
            );

            // Back: the user's next Connect says it syncs.
            apply_core_event(&mut state, Event::SyncStopped(SyncStopReason::Requested)).unwrap();
            state.reconnect.reset();
            let stopped = state.model.status_log.lines().len();
            apply_core_event(&mut state, Event::SyncStarted).unwrap();
            assert_eq!(
                status_lines_since(&state, stopped),
                [(StatusLineKind::Info, "* Syncing with the homeserver")]
            );
        });
    }

    /// The core lists the rooms again on every change; Status counts the
    /// session's first list only, whatever came between.
    #[test]
    fn synced_rooms_is_logged_once_per_session() {
        with_local_app_data(|_| {
            let mut state = test_state();
            state.harness.synthetic_load = true;
            let rooms = || vec![room("!room:example", "Room", false)];
            let counted = |state: &WindowState| {
                status_log_lines(state)
                    .into_iter()
                    .filter(|line| line.contains("* Synced rooms: "))
                    .count()
            };

            apply_core_event(&mut state, Event::RestoredSession).unwrap();
            apply_core_event(&mut state, Event::SyncedRooms(rooms())).unwrap();
            assert_eq!(counted(&state), 1);
            // The first sync's list, a /join's, after an error line.
            apply_core_event(&mut state, Event::SyncStarted).unwrap();
            apply_core_event(&mut state, Event::SyncedRooms(rooms())).unwrap();
            apply_core_event(
                &mut state,
                Event::Error(CoreErrorEvent {
                    operation: Operation::JoinRoom,
                    room_id: None,
                    message: "no such room".to_owned(),
                }),
            )
            .unwrap();
            apply_core_event(&mut state, Event::SyncedRooms(rooms())).unwrap();
            assert_eq!(counted(&state), 1);

            // The next session's first list is news again.
            apply_core_event(&mut state, Event::LoggedOut(LogoutScope::Everywhere)).unwrap();
            apply_core_event(
                &mut state,
                Event::LoggedIn {
                    session: stored_session(),
                    homeserver_url: "https://matrix.example".to_owned(),
                },
            )
            .unwrap();
            apply_core_event(&mut state, Event::SyncedRooms(rooms())).unwrap();
            apply_core_event(&mut state, Event::SyncedRooms(rooms())).unwrap();
            assert_eq!(counted(&state), 2);
        });
    }

    #[test]
    fn connecting_clears_an_error_from_before_but_not_one_after() {
        with_local_app_data(|_| {
            let mut state = test_state();
            state.harness.synthetic_load = true;
            apply_core_event(
                &mut state,
                Event::LoggedIn {
                    session: stored_session(),
                    homeserver_url: "https://matrix.example".to_owned(),
                },
            )
            .unwrap();
            apply_core_event(
                &mut state,
                Event::Error(CoreErrorEvent {
                    operation: Operation::RestoreSession,
                    room_id: None,
                    message: "offline".to_owned(),
                }),
            )
            .unwrap();
            assert!(frame_status_text(&state).contains(" | error: "));
            apply_core_event(&mut state, Event::SyncStarted).unwrap();
            apply_core_event(&mut state, Event::SyncConnected).unwrap();
            assert_eq!(state.model.last_error, None);
            assert!(
                !frame_status_text(&state).contains("| error:"),
                "{}",
                frame_status_text(&state)
            );

            let join_failed = CoreErrorEvent {
                operation: Operation::JoinRoom,
                room_id: None,
                message: "forbidden".to_owned(),
            };
            apply_core_event(&mut state, Event::Error(join_failed.clone())).unwrap();
            assert_eq!(state.model.last_error, Some(join_failed));
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
        state.harness.synthetic_load = true;
        state.model.own_account = Some(OwnAccount::matrix("@me:example".to_owned()));

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
        assert_eq!(state.model.own_user_id(), Some("@me:example"));
    }

    #[test]
    fn synthetic_logged_in_event_updates_model_without_persisting_session() {
        with_local_app_data(|root| {
            let mut state = test_state();
            state.harness.synthetic_load = true;

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

    fn decrypted(room_id: &str, events: Vec<RoomHistoryBackfillEvent>) -> Event {
        Event::RoomEventsDecrypted(RoomEventsDecrypted {
            room_id: room_id.to_owned(),
            events,
        })
    }

    fn decrypted_line(activity: RoomActivity) -> RoomHistoryBackfillEvent {
        RoomHistoryBackfillEvent::Activity(Box::new(activity))
    }

    fn ops_lines(state: &WindowState) -> Vec<String> {
        project_timeline_rows(&build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
            None,
            false,
        ))
    }

    /// Ops, with alice's `$secret` that could not be decrypted between two
    /// messages.
    fn undecrypted_state() -> WindowState {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", true)];
        for event in [
            timeline_with_transaction_id(
                "!ops:example",
                "$before",
                None,
                "before",
                1_700_000_000_000,
            ),
            unable_to_decrypt_activity(
                "!ops:example",
                "$secret",
                "@alice:example",
                1_700_000_060_000,
            ),
            timeline_with_transaction_id(
                "!ops:example",
                "$after",
                None,
                "after",
                1_700_000_120_000,
            ),
        ] {
            apply_core_event(&mut state, event).unwrap();
        }
        state
    }

    fn alices_secret(body: &str) -> RoomActivity {
        let mut activity =
            sent_message_activity("!ops:example", "$secret", None, body, 1_700_000_060_000);
        let RoomActivityKind::Message(message) = &mut activity.kind else {
            unreachable!()
        };
        message.sender = user_profile("@alice:example");
        activity
    }

    #[test]
    fn a_decrypted_line_takes_the_place_of_the_one_that_could_not_be_decrypted() {
        let mut state = undecrypted_state();
        let before = ops_lines(&state);
        let line = before
            .iter()
            .position(|line| line.contains("sent an encrypted message that could not be decrypted"))
            .expect("the line that could not be decrypted");
        let arrival = state.model.room_views["!ops:example"]
            .arrivals
            .of("event:$secret")
            .expect("the line arrived");
        let revision = state.model.room_views.revision("!ops:example");

        apply_core_event(
            &mut state,
            decrypted(
                "!ops:example",
                vec![decrypted_line(alices_secret("the plan"))],
            ),
        )
        .unwrap();

        let after = ops_lines(&state);
        assert_eq!(after.len(), before.len(), "{after:#?}");
        for (index, (after, before)) in after.iter().zip(&before).enumerate() {
            if index == line {
                assert!(after.ends_with("<@alice:example> the plan"), "{after}");
            } else {
                assert_eq!(after, before);
            }
        }
        let view = &state.model.room_views["!ops:example"];
        assert_eq!(view.arrivals.of("event:$secret"), Some(arrival));
        view.assert_arrivals_consistent();
        assert_ne!(state.model.room_views.revision("!ops:example"), revision);

        // Reported again (the SDK refreshed its encryption info): the
        // message stays, and the window is not redrawn.
        let revision = state.model.room_views.revision("!ops:example");
        apply_core_event(
            &mut state,
            decrypted(
                "!ops:example",
                vec![decrypted_line(alices_secret("the plan"))],
            ),
        )
        .unwrap();
        assert_eq!(ops_lines(&state), after);
        assert_eq!(state.model.room_views.revision("!ops:example"), revision);
    }

    /// `$secret` that could not be decrypted, as a retry reports it now:
    /// still undecryptable, for `cause`.
    fn alices_secret_undecryptable(cause: mirx_core::RoomUnableToDecryptCause) -> RoomActivity {
        let Event::RoomActivity(mut activity) = unable_to_decrypt_activity(
            "!ops:example",
            "$secret",
            "@alice:example",
            1_700_000_060_000,
        ) else {
            unreachable!()
        };
        let RoomActivityKind::UnableToDecrypt(utd) = &mut activity.kind else {
            unreachable!()
        };
        utd.cause = cause;
        *activity
    }

    /// A retry tells a line still undecryptable its current cause: the line
    /// changes in place, raises nothing, and keeps the edit it holds for
    /// when it decrypts. The same report again changes nothing.
    #[test]
    fn a_retried_line_still_undecryptable_takes_its_new_cause_in_place() {
        use mirx_core::RoomUnableToDecryptCause as Cause;
        let mut state = undecrypted_state();
        apply_core_event(
            &mut state,
            edit("$secret", "$fix", "@alice:example", 1, "the plan, fixed"),
        )
        .unwrap();
        let before = ops_lines(&state);
        let line = before
            .iter()
            .position(|line| line.contains("sent an encrypted message that could not be decrypted"))
            .expect("the line that could not be decrypted");
        let activity = state.room_activity.clone();
        let revision = state.model.room_views.revision("!ops:example");

        let historical = alices_secret_undecryptable(Cause::HistoricalUnverified);
        apply_core_event(
            &mut state,
            decrypted("!ops:example", vec![decrypted_line(historical.clone())]),
        )
        .unwrap();

        let after = ops_lines(&state);
        assert_eq!(after.len(), before.len(), "{after:#?}");
        for (index, (after, before)) in after.iter().zip(&before).enumerate() {
            if index == line {
                assert!(
                    after.ends_with(
                        "* @alice:example sent an encrypted message that could not be decrypted \
                         (sent before this session existed: /verify this session or /e2ee recover to read it)"
                    ),
                    "{after}"
                );
            } else {
                assert_eq!(after, before);
            }
        }
        assert_eq!(state.room_activity, activity);
        assert_ne!(state.model.room_views.revision("!ops:example"), revision);
        state.model.room_views["!ops:example"].assert_arrivals_consistent();

        let revision = state.model.room_views.revision("!ops:example");
        apply_core_event(
            &mut state,
            decrypted("!ops:example", vec![decrypted_line(historical)]),
        )
        .unwrap();
        assert_eq!(ops_lines(&state), after);
        assert_eq!(state.model.room_views.revision("!ops:example"), revision);

        apply_core_event(
            &mut state,
            decrypted(
                "!ops:example",
                vec![decrypted_line(alices_secret("the plna"))],
            ),
        )
        .unwrap();
        assert_eq!(
            ops_lines(&state)[line],
            "[22:14] <@alice:example> the plan, fixed (edited)"
        );
    }

    #[test]
    fn a_decrypted_thread_message_moves_into_its_thread() {
        let mut state = undecrypted_state();
        assert!(state.model.room_views["!ops:example"].threads.is_empty());
        let mut in_thread = alices_secret("in the thread");
        in_thread.relation = Some(thread_relation("$before"));

        apply_core_event(
            &mut state,
            decrypted("!ops:example", vec![decrypted_line(in_thread)]),
        )
        .unwrap();

        let view = &state.model.room_views["!ops:example"];
        assert!(view.threads.contains_key("$before"), "{:#?}", view.threads);
        view.assert_arrivals_consistent();
    }

    #[test]
    fn a_quoted_message_that_could_not_be_decrypted_is_quoted_once_decrypted() {
        let mut state = reply_quote_state("$secret");
        let Event::RoomActivity(utd) = unable_to_decrypt_activity(
            "!ops:example",
            "$secret",
            "@alice:example",
            1_600_000_000_000,
        ) else {
            unreachable!()
        };
        state
            .model
            .room_views
            .get_mut("!ops:example")
            .unwrap()
            .reply_targets
            .insert("$secret".to_owned(), ReplyTargetState::Loaded(utd));
        assert_eq!(
            state.model.room_views["!ops:example"].undecrypted_event_ids(),
            BTreeSet::from(["$secret".to_owned()])
        );

        let mut target = alices_secret("from long ago");
        target.timestamp_ms = 1_600_000_000_000;
        apply_core_event(
            &mut state,
            decrypted("!ops:example", vec![decrypted_line(target)]),
        )
        .unwrap();

        let lines = ops_lines(&state);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("> <@alice:example> from long ago")),
            "{lines:#?}"
        );
        assert!(
            state.model.room_views["!ops:example"]
                .undecrypted_event_ids()
                .is_empty()
        );
    }

    #[test]
    fn decrypted_events_of_lines_not_shown_change_nothing() {
        let mut state = undecrypted_state();
        let revision = state.model.room_views.revision("!ops:example");
        let lines = ops_lines(&state);

        let mut elsewhere = alices_secret("trimmed long ago");
        let RoomActivityKind::Message(message) = &mut elsewhere.kind else {
            unreachable!()
        };
        message.event_id = Some("$gone".to_owned());
        elsewhere.activity_id = "event:$gone".to_owned();
        apply_core_event(
            &mut state,
            decrypted("!ops:example", vec![decrypted_line(elsewhere)]),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            decrypted("!unknown:example", vec![decrypted_line(alices_secret("x"))]),
        )
        .unwrap();

        assert_eq!(ops_lines(&state), lines);
        assert_eq!(state.model.room_views.revision("!ops:example"), revision);
        assert!(state.model.room_views.get("!unknown:example").is_none());
    }

    #[test]
    fn a_decrypted_highlight_raises_its_inactive_window() {
        let mut state = undecrypted_state();
        state.active_view = ViewId::Status;
        state.room_activity.clear();

        apply_core_event(
            &mut state,
            decrypted(
                "!ops:example",
                vec![decrypted_line(highlighted(alices_secret("you there?")))],
            ),
        )
        .unwrap();

        assert_eq!(
            state.room_activity.get(&room_view_id("!ops:example")),
            Some(&ViewActivityLevel::Highlight)
        );
    }

    #[test]
    fn decrypted_reactions_and_edits_apply_once() {
        let mut state = undecrypted_state();
        apply_core_event(
            &mut state,
            decrypted("!ops:example", vec![decrypted_line(alices_secret("tpyo"))]),
        )
        .unwrap();
        let Event::RoomActivityUpdate(reaction) = incoming_reaction_added(
            "!ops:example",
            "$secret",
            "$reaction",
            "@bob:example",
            "+1",
            1_700_000_300_000,
        ) else {
            unreachable!()
        };
        let Event::RoomActivityUpdate(edit) = incoming_replaced(
            "!ops:example",
            "$secret",
            "$edit",
            RoomMessagePresentationKind::Text,
            "typo",
        ) else {
            unreachable!()
        };
        let updates = vec![
            RoomHistoryBackfillEvent::ActivityUpdate(Box::new(edit)),
            RoomHistoryBackfillEvent::ActivityUpdate(Box::new(reaction)),
        ];

        apply_core_event(&mut state, decrypted("!ops:example", updates.clone())).unwrap();
        let once = ops_lines(&state);
        apply_core_event(&mut state, decrypted("!ops:example", updates)).unwrap();

        assert_eq!(ops_lines(&state), once);
        assert!(
            once.contains(&"[22:14] <@alice:example> typo (edited)".to_owned()),
            "{once:#?}"
        );
        let view = &state.model.room_views["!ops:example"];
        assert_eq!(view.reactions_by_target["$secret"].len(), 1);
    }

    /// However a reaction came (live, in a history page, or decrypted
    /// late), its redaction takes it back: the redaction names the reaction.
    #[test]
    fn a_redaction_of_a_shown_reaction_removes_it() {
        let mut state = undecrypted_state();
        apply_core_event(
            &mut state,
            decrypted("!ops:example", vec![decrypted_line(alices_secret("plan"))]),
        )
        .unwrap();
        let reaction = |target: &str, reaction_event_id: &str| {
            let Event::RoomActivityUpdate(update) = incoming_reaction_added(
                "!ops:example",
                target,
                reaction_event_id,
                "@bob:example",
                "+1",
                1_700_000_300_000,
            ) else {
                unreachable!()
            };
            update
        };
        apply_core_event(
            &mut state,
            Event::RoomActivityUpdate(reaction("$before", "$live")),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_history_backfill(
                "!ops:example",
                None,
                vec![RoomHistoryBackfillEvent::ActivityUpdate(Box::new(
                    reaction("$after", "$history"),
                ))],
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            decrypted(
                "!ops:example",
                vec![RoomHistoryBackfillEvent::ActivityUpdate(Box::new(
                    reaction("$secret", "$decrypted"),
                ))],
            ),
        )
        .unwrap();
        assert_eq!(
            state.model.room_views["!ops:example"]
                .reactions_by_target
                .len(),
            3
        );

        for reaction_event_id in ["$live", "$history", "$decrypted"] {
            apply_core_event(
                &mut state,
                incoming_redacted("!ops:example", reaction_event_id, "$taken-back"),
            )
            .unwrap();
        }

        let view = &state.model.room_views["!ops:example"];
        assert!(
            view.reactions_by_target.is_empty(),
            "{:#?}",
            view.reactions_by_target
        );
        let lines = ops_lines(&state);
        assert!(
            !lines.iter().any(|line| line.contains("reactions:")),
            "{lines:#?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.ends_with("<@alice:example> plan")),
            "{lines:#?}"
        );
    }

    /// `sender`'s edit of `target`, at `at` ms past the test epoch.
    fn edit(target: &str, replacement: &str, sender: &str, at: u64, body: &str) -> Event {
        Event::RoomActivityUpdate(edit_by(
            "!ops:example",
            target,
            replacement,
            sender,
            1_700_000_200_000 + at,
            RoomMessagePresentationKind::Text,
            body,
        ))
    }

    fn line_of<'a>(lines: &'a [String], event_line_end: &str) -> &'a str {
        lines
            .iter()
            .find(|line| line.contains(event_line_end))
            .unwrap_or_else(|| panic!("no line with {event_line_end}: {lines:#?}"))
    }

    /// Deleting an edit shows what it replaced, as Element does: the latest
    /// edit left, then the message's own text, no longer `(edited)`. The
    /// deleted text shows nowhere.
    #[test]
    fn a_redacted_edit_restores_the_previous_text() {
        let mut state = undecrypted_state();
        for event in [
            edit("$before", "$e2", "@alice:example", 2, "second"),
            edit("$before", "$e3", "@alice:example", 3, "third"),
        ] {
            apply_core_event(&mut state, event).unwrap();
        }
        assert_eq!(
            line_of(&ops_lines(&state), "<@alice:example> "),
            "[22:13] <@alice:example> third (edited)"
        );

        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$e3", "$unedit3"),
        )
        .unwrap();
        assert_eq!(
            line_of(&ops_lines(&state), "<@alice:example> "),
            "[22:13] <@alice:example> second (edited)"
        );
        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$e2", "$unedit2"),
        )
        .unwrap();
        let lines = ops_lines(&state);
        assert_eq!(
            line_of(&lines, "<@alice:example> "),
            "[22:13] <@alice:example> before"
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("second") || line.contains("third")),
            "{lines:#?}"
        );
        assert!(state.model.room_views["!ops:example"].line_edits.is_empty());
    }

    /// A page of history brings a message with the server's latest edit
    /// (bundled): deleting that edit shows the message's own text.
    #[test]
    fn a_redacted_bundled_edit_restores_the_original() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", true)];
        let mut old = alices_secret("tpyo");
        old.activity_id = "event:$old".to_owned();
        let RoomActivityKind::Message(message) = &mut old.kind else {
            unreachable!()
        };
        message.event_id = Some("$old".to_owned());
        let Event::RoomActivityUpdate(bundled) = edit("$old", "$fix", "@alice:example", 1, "typo")
        else {
            unreachable!()
        };
        apply_core_event(
            &mut state,
            room_history_backfill(
                "!ops:example",
                None,
                vec![
                    RoomHistoryBackfillEvent::Activity(Box::new(old)),
                    RoomHistoryBackfillEvent::ActivityUpdate(Box::new(bundled)),
                ],
            ),
        )
        .unwrap();
        assert!(
            ops_lines(&state).contains(&"[22:14] <@alice:example> typo (edited)".to_owned()),
            "{:#?}",
            ops_lines(&state)
        );

        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$fix", "$unfix"),
        )
        .unwrap();

        let lines = ops_lines(&state);
        assert!(
            lines.contains(&"[22:14] <@alice:example> tpyo".to_owned()),
            "{lines:#?}"
        );
    }

    /// A reply's quote of a line whose edit was deleted quotes the text the
    /// line shows again.
    #[test]
    fn a_reply_quote_follows_the_restored_text() {
        let mut state = reply_quote_state("$q");
        apply_core_event(
            &mut state,
            edit("$q", "$fix", "@alice:example", 1, "secret"),
        )
        .unwrap();
        assert!(
            ops_lines(&state)
                .iter()
                .any(|line| line.contains("> ") && line.contains("secret")),
            "{:#?}",
            ops_lines(&state)
        );

        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$fix", "$unfix"),
        )
        .unwrap();

        let lines = ops_lines(&state);
        assert!(
            !lines.iter().any(|line| line.contains("secret")),
            "{lines:#?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("> <") && line.contains("are we deploying today?")),
            "{lines:#?}"
        );
    }

    /// The reply quotes `$elsewhere`, which is no line; the fetch brings it
    /// (alice's `from long ago`) with `updates`.
    fn fetched_quote_state(target: RoomActivity, updates: Vec<RoomActivityUpdate>) -> WindowState {
        let mut state = reply_quote_state("$elsewhere");
        apply_core_event(
            &mut state,
            Event::RoomEvent(RoomEventFetch {
                room_id: "!ops:example".to_owned(),
                event_id: "$elsewhere".to_owned(),
                result: RoomEventFetchResult::Loaded {
                    activity: Box::new(target),
                    updates,
                },
            }),
        )
        .unwrap();
        state
    }

    fn from_long_ago() -> RoomActivity {
        sent_message_activity(
            "!ops:example",
            "$elsewhere",
            None,
            "from long ago",
            1_600_000_000_000,
        )
    }

    fn edit_update(
        target: &str,
        replacement: &str,
        sender: &str,
        at: u64,
        body: &str,
    ) -> RoomActivityUpdate {
        let Event::RoomActivityUpdate(update) = edit(target, replacement, sender, at, body) else {
            unreachable!()
        };
        update
    }

    fn quote_line(state: &WindowState) -> String {
        line_of(&ops_lines(state), "    > ").to_owned()
    }

    #[test]
    fn a_fetched_quote_shows_its_latest_edit() {
        let state = fetched_quote_state(
            from_long_ago(),
            vec![edit_update(
                "$elsewhere",
                "$fix",
                "@alice:example",
                1,
                "fixed",
            )],
        );
        assert_eq!(quote_line(&state), "    > <@alice:example> fixed (edited)");
    }

    #[test]
    fn a_quote_follows_a_live_edit_and_its_redaction() {
        let mut state = fetched_quote_state(from_long_ago(), Vec::new());
        for event in [
            edit("$elsewhere", "$fix", "@alice:example", 1, "fixed"),
            edit("$elsewhere", "$forged", "@bob:example", 2, "bob was here"),
        ] {
            apply_core_event(&mut state, event).unwrap();
        }
        assert_eq!(quote_line(&state), "    > <@alice:example> fixed (edited)");

        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$fix", "$unfix"),
        )
        .unwrap();
        assert_eq!(quote_line(&state), "    > <@alice:example> from long ago");
        assert!(state.model.room_views["!ops:example"].line_edits.is_empty());
    }

    /// Deleting a message deletes it from the quotes of it too, also when
    /// no line shows it.
    #[test]
    fn a_quote_of_a_message_redacted_while_not_loaded_reads_message_redacted() {
        let mut state = fetched_quote_state(
            from_long_ago(),
            vec![edit_update(
                "$elsewhere",
                "$fix",
                "@alice:example",
                1,
                "fixed",
            )],
        );
        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$elsewhere", "$delete"),
        )
        .unwrap();

        let lines = ops_lines(&state);
        assert_eq!(quote_line(&state), "    > message redacted");
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("long ago") || line.contains("fixed")),
            "{lines:#?}"
        );
        assert!(state.model.room_views["!ops:example"].line_edits.is_empty());
    }

    /// The fetched copy and its edits go when history loads the message as
    /// a line, which brings its own: the same latest edit shows on it.
    #[test]
    fn a_line_loaded_after_its_quote_shows_its_own_latest_edit() {
        let bundled = edit_update("$elsewhere", "$fix", "@alice:example", 1, "fixed");
        let mut state = fetched_quote_state(from_long_ago(), vec![bundled.clone()]);

        apply_core_event(
            &mut state,
            room_history_backfill(
                "!ops:example",
                None,
                vec![
                    RoomHistoryBackfillEvent::Activity(Box::new(from_long_ago())),
                    RoomHistoryBackfillEvent::ActivityUpdate(Box::new(bundled)),
                ],
            ),
        )
        .unwrap();

        let room_view = &state.model.room_views["!ops:example"];
        assert!(!room_view.reply_targets.contains_key("$elsewhere"));
        let lines = ops_lines(&state);
        assert!(
            lines.contains(&"[12:26] <@alice:example> fixed (edited)".to_owned()),
            "{lines:#?}"
        );
        assert_eq!(quote_line(&state), "    > <@alice:example> fixed (edited)");

        // A fetch that answers after the line came changes nothing.
        let before = ops_lines(&state);
        let state = {
            let mut state = state;
            apply_core_event(
                &mut state,
                Event::RoomEvent(RoomEventFetch {
                    room_id: "!ops:example".to_owned(),
                    event_id: "$elsewhere".to_owned(),
                    result: RoomEventFetchResult::Loaded {
                        activity: Box::new(from_long_ago()),
                        updates: Vec::new(),
                    },
                }),
            )
            .unwrap();
            state
        };
        assert_eq!(ops_lines(&state), before);
        assert!(
            !state.model.room_views["!ops:example"]
                .reply_targets
                .contains_key("$elsewhere")
        );
    }

    /// A quote of a message that could not be decrypted holds its edits,
    /// as a line does, and shows the latest of its sender's once it
    /// decrypts.
    #[test]
    fn a_quote_target_that_could_not_be_decrypted_applies_its_held_edit_when_it_does() {
        let Event::RoomActivity(utd) = unable_to_decrypt_activity(
            "!ops:example",
            "$elsewhere",
            "@alice:example",
            1_600_000_000_000,
        ) else {
            unreachable!()
        };
        let mut state = fetched_quote_state(
            *utd,
            vec![edit_update(
                "$elsewhere",
                "$fix",
                "@alice:example",
                1,
                "fixed",
            )],
        );

        apply_core_event(
            &mut state,
            decrypted("!ops:example", vec![decrypted_line(from_long_ago())]),
        )
        .unwrap();
        assert_eq!(quote_line(&state), "    > <@alice:example> fixed (edited)");
    }

    /// Only a message's sender can edit it (the spec's validity of
    /// replacement events): anyone else's edit is ignored, as Element
    /// does, also while the line waits for its key.
    #[test]
    fn an_edit_by_another_user_is_ignored() {
        let mut state = undecrypted_state();
        let before = ops_lines(&state);
        for event in [
            edit("$before", "$forged", "@bob:example", 1, "bob was here"),
            edit("$secret", "$forged-too", "@bob:example", 2, "bob again"),
        ] {
            apply_core_event(&mut state, event).unwrap();
        }
        assert_eq!(ops_lines(&state), before);

        apply_core_event(
            &mut state,
            decrypted("!ops:example", vec![decrypted_line(alices_secret("plan"))]),
        )
        .unwrap();
        let lines = ops_lines(&state);
        assert!(
            line_of(&lines, "<@alice:example> plan").ends_with("plan"),
            "{lines:#?}"
        );
        assert!(!lines.iter().any(|line| line.contains("bob")), "{lines:#?}");
    }

    /// The latest edit shows (by server time, then by event ID), whatever
    /// order the edits come in; the same edit again changes nothing.
    #[test]
    fn an_older_edit_never_overwrites_a_newer_one() {
        let mut state = undecrypted_state();
        for event in [
            edit("$before", "$e2", "@alice:example", 2, "second"),
            edit("$before", "$e1", "@alice:example", 1, "first"),
        ] {
            apply_core_event(&mut state, event).unwrap();
        }
        assert_eq!(
            line_of(&ops_lines(&state), "<@alice:example> "),
            "[22:13] <@alice:example> second (edited)"
        );

        // A tie in time goes to the greater event ID.
        let Event::RoomActivityUpdate(tie) = edit("$before", "$e3", "@alice:example", 2, "third")
        else {
            unreachable!()
        };
        let Event::RoomActivityUpdate(tie_lost) =
            edit("$before", "$e0", "@alice:example", 2, "zeroth")
        else {
            unreachable!()
        };
        let view = state.model.room_views.get_mut("!ops:example").unwrap();
        assert!(view.apply_update(&tie));
        assert!(!view.apply_update(&tie_lost));
        assert!(!view.apply_update(&tie), "the same edit again");
        assert_eq!(
            line_of(&ops_lines(&state), "<@alice:example> "),
            "[22:13] <@alice:example> third (edited)"
        );
    }

    /// An edit that came while its line could not be decrypted waits for
    /// the line; once the line decrypts, it shows edited.
    #[test]
    fn an_edit_of_an_undecrypted_line_applies_when_it_decrypts() {
        let mut state = undecrypted_state();
        let before = ops_lines(&state);
        apply_core_event(
            &mut state,
            edit("$secret", "$fix", "@alice:example", 1, "the plan, fixed"),
        )
        .unwrap();
        assert_eq!(ops_lines(&state), before, "the line still waits");

        apply_core_event(
            &mut state,
            decrypted(
                "!ops:example",
                vec![decrypted_line(alices_secret("the plna"))],
            ),
        )
        .unwrap();
        let lines = ops_lines(&state);
        assert!(
            lines.contains(&"[22:14] <@alice:example> the plan, fixed (edited)".to_owned()),
            "{lines:#?}"
        );
    }

    #[test]
    fn an_edit_held_for_a_line_that_decrypts_to_another_kind_is_dropped() {
        let mut state = undecrypted_state();
        apply_core_event(
            &mut state,
            edit("$secret", "$fix", "@alice:example", 1, "fixed"),
        )
        .unwrap();
        let image = image_activity(
            "!ops:example",
            "$secret",
            "@alice:example",
            1_700_000_060_000,
            "plan.png",
            None,
            None,
        );
        apply_core_event(
            &mut state,
            decrypted("!ops:example", vec![decrypted_line(image)]),
        )
        .unwrap();

        let lines = ops_lines(&state);
        assert!(
            !lines.iter().any(|line| line.contains("fixed")),
            "{lines:#?}"
        );
        assert!(state.model.room_views["!ops:example"].line_edits.is_empty());
    }

    #[test]
    fn a_held_edit_redacted_before_its_line_decrypts_is_dropped() {
        let mut state = undecrypted_state();
        apply_core_event(
            &mut state,
            edit("$secret", "$fix", "@alice:example", 1, "fixed"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$fix", "$unfix"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            decrypted("!ops:example", vec![decrypted_line(alices_secret("plan"))]),
        )
        .unwrap();

        let lines = ops_lines(&state);
        assert!(
            lines.contains(&"[22:14] <@alice:example> plan".to_owned()),
            "{lines:#?}"
        );
    }

    /// An edit that decrypts late, older than the edit already shown, does
    /// not bring back the older text.
    #[test]
    fn a_decrypted_older_edit_after_a_live_newer_one_is_ignored() {
        let mut state = undecrypted_state();
        apply_core_event(
            &mut state,
            edit("$before", "$e2", "@alice:example", 2, "second"),
        )
        .unwrap();
        let Event::RoomActivityUpdate(older) = edit("$before", "$e1", "@alice:example", 1, "first")
        else {
            unreachable!()
        };
        apply_core_event(
            &mut state,
            decrypted(
                "!ops:example",
                vec![RoomHistoryBackfillEvent::ActivityUpdate(Box::new(older))],
            ),
        )
        .unwrap();
        assert_eq!(
            line_of(&ops_lines(&state), "<@alice:example> "),
            "[22:13] <@alice:example> second (edited)"
        );
    }

    #[test]
    fn a_retry_asks_for_the_events_each_room_shows_undecrypted() {
        let mut state = undecrypted_state();
        let (sender, _commands) = inert_command_sender();
        state.command_sender = Some(sender);
        state
            .model
            .rooms
            .push(room("!quiet:example", "Quiet", true));
        apply_core_event(&mut state, timeline("!quiet:example", "plain")).unwrap();
        // A second line of the same session, and one with none: each is
        // read again.
        apply_core_event(
            &mut state,
            unable_to_decrypt_activity(
                "!ops:example",
                "$again",
                "@alice:example",
                1_700_000_180_000,
            ),
        )
        .unwrap();
        let Event::RoomActivity(mut no_session) =
            unable_to_decrypt_activity("!ops:example", "$olm", "@alice:example", 1_700_000_240_000)
        else {
            unreachable!()
        };
        let RoomActivityKind::UnableToDecrypt(utd) = &mut no_session.kind else {
            unreachable!()
        };
        utd.session_id = None;
        apply_core_event(&mut state, Event::RoomActivity(no_session)).unwrap();

        apply_core_event(&mut state, Event::DecryptionRetryWanted).unwrap();
        assert!(state.harness.commands.is_empty(), "no session to ask");

        state.model.connection = ConnectionState::Syncing;
        apply_core_event(&mut state, Event::DecryptionRetryWanted).unwrap();
        assert_eq!(
            state.harness.commands,
            vec![Command::RetryDecryption(RetryDecryptionRequest {
                room_id: "!ops:example".to_owned(),
                event_ids: vec!["$again".to_owned(), "$olm".to_owned(), "$secret".to_owned()],
            })]
        );
    }

    /// Why a line could not be decrypted depends on this session's
    /// verification and key backup: when either changes, the shown lines
    /// are read again. The first status, an equal one, or one that differs
    /// elsewhere (recovery, cross-signing) asks nothing.
    #[test]
    fn a_changed_session_state_retries_the_shown_undecrypted_lines() {
        let mut state = undecrypted_state();
        let (sender, _commands) = inert_command_sender();
        state.command_sender = Some(sender);
        state.model.connection = ConnectionState::Syncing;
        let unverified = E2eeStatus {
            session: E2eeSessionState::Unverified,
            ..e2ee_status(
                E2eeRecoveryState::Disabled,
                E2eeBackupState::Unknown,
                false,
                Some(true),
                Some(true),
            )
        };
        let retry = Command::RetryDecryption(RetryDecryptionRequest {
            room_id: "!ops:example".to_owned(),
            event_ids: vec!["$secret".to_owned()],
        });
        let status_then = |state: &mut WindowState, status: &E2eeStatus| {
            state.harness.commands.clear();
            apply_core_event(state, Event::E2eeStatus(status.clone())).unwrap();
            std::mem::take(&mut state.harness.commands)
        };

        assert_eq!(status_then(&mut state, &unverified), [], "the first");
        assert_eq!(status_then(&mut state, &unverified), [], "the same");
        let recovery = E2eeStatus {
            recovery: E2eeRecoveryState::Incomplete,
            cross_signing: None,
            ..unverified.clone()
        };
        assert_eq!(status_then(&mut state, &recovery), [], "nothing it reads");
        let verified = E2eeStatus {
            session: E2eeSessionState::Verified,
            ..recovery.clone()
        };
        assert_eq!(status_then(&mut state, &verified), vec![retry.clone()]);
        let no_backup = E2eeStatus {
            backup: mirx_core::E2eeBackupStatus {
                exists_on_server: Some(false),
                ..verified.backup.clone()
            },
            ..verified.clone()
        };
        assert_eq!(status_then(&mut state, &no_backup), vec![retry.clone()]);
        let enabled = E2eeStatus {
            backup: mirx_core::E2eeBackupStatus {
                state: E2eeBackupState::Enabled,
                enabled_locally: true,
                exists_on_server: Some(true),
            },
            ..no_backup.clone()
        };
        assert_eq!(status_then(&mut state, &enabled), vec![retry]);
    }

    #[test]
    fn room_key_file_answers_are_status_lines() {
        let mut state = test_state();
        for event in [
            Event::RoomKeysExported {
                path: r"C:\keys.txt".to_owned(),
            },
            Event::RoomKeysImported {
                path: r"C:\keys.txt".to_owned(),
                imported: 3,
                total: 3,
            },
            Event::RoomKeysImported {
                path: r"C:\keys.txt".to_owned(),
                imported: 1,
                total: 3,
            },
            Event::Error(CoreErrorEvent {
                operation: Operation::ImportRoomKeys,
                room_id: None,
                message: "wrong passphrase, or the file is damaged".to_owned(),
            }),
        ] {
            apply_core_event(&mut state, event).unwrap();
        }
        assert_eq!(
            status_lines_with_kinds(&state)[..],
            [
                (
                    StatusLineKind::Info,
                    r"* Room keys exported to C:\keys.txt. Keep the file and its passphrase safe."
                        .to_owned()
                ),
                (
                    StatusLineKind::Info,
                    r"* Imported 3 of 3 room keys from C:\keys.txt".to_owned()
                ),
                (
                    StatusLineKind::Info,
                    r"* Imported 1 of 3 room keys from C:\keys.txt (2 already known)".to_owned()
                ),
                (
                    StatusLineKind::Error,
                    "* Error during room key import: wrong passphrase, or the file is damaged"
                        .to_owned()
                ),
            ]
        );
    }
}
