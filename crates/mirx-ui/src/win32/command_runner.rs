//! Running parsed slash commands; the parser is `commands.rs`.

use super::aliases::Aliases;
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
enum QueryTargetResolution {
    None,
    Single(String),
    Ambiguous,
}

/// Runs a command given in `window` and reports how it went: its status,
/// or its error in that window.
pub(super) unsafe fn run_local_command(
    hwnd: HWND,
    window: &ViewId,
    command: SlashCommand,
) -> Result<(), String> {
    match execute_slash_command(hwnd, window, command) {
        Ok(Some(status)) => set_local_command_status(hwnd, status),
        Ok(None) => Ok(()),
        Err(error) => report_command_error(hwnd, window, error),
    }
}

/// Runs a command given in `window`. `Ok(Some(status))` is feedback for the
/// status bar; `Ok(None)` means the command reported (or needs) nothing.
pub(super) unsafe fn execute_slash_command(
    hwnd: HWND,
    window: &ViewId,
    command: SlashCommand,
) -> Result<Option<String>, CommandError> {
    let name = command.name();
    run_slash_command(hwnd, window, command).map_err(|reason| CommandError::new(name, reason))
}

/// `/clear`, and the window menu's Buffer > Clear: every line of the
/// window goes, but a room's messages still queued to send. A thread
/// window has nothing of its own to clear. Returns the status bar's
/// feedback.
pub(super) unsafe fn clear_window(hwnd: HWND, view_id: &ViewId) -> Result<String, String> {
    match view_id {
        ViewId::Room(room_id) => {
            {
                let state = state_mut(hwnd);
                if let Some(view) = state.model.room_views.get_mut(room_id) {
                    view.clear_visible_entries();
                }
                prune_decoded_images(state);
            }
            refresh_view_content(hwnd, view_id)?;
            Ok(format!("cleared window: {room_id}"))
        }
        ViewId::Thread { .. } => Err("nothing to clear in thread view".to_owned()),
        ViewId::Status => {
            state_mut(hwnd).model.status_log.clear();
            refresh_view_content(hwnd, view_id)?;
            Ok("cleared window: status".to_owned())
        }
        ViewId::RoomList => unreachable!("the Rooms List has no buffer"),
    }
}

/// Runs a command given in `window`, the window it is about: the room a
/// command without one acts on, and where its lines print. It was the
/// active one when the command was given; it need not be now (a prompt's
/// modal loop may have activated another).
pub(super) unsafe fn run_slash_command(
    hwnd: HWND,
    window: &ViewId,
    command: SlashCommand,
) -> Result<Option<String>, String> {
    match command {
        SlashCommand::Status => {
            activate_view(hwnd, &ViewId::Status, None)?;
            Ok(Some("status window active".to_owned()))
        }
        SlashCommand::Find { query } => {
            execute_view_find(hwnd, window, &query)?;
            Ok(None)
        }
        SlashCommand::Window { target } => {
            let model = &state_ref(hwnd).model;
            let room_id = match resolve_room(
                &model.rooms,
                &model.joined_aliases,
                is_navigable_room,
                &target,
            ) {
                Ok(room) => room.room_id.clone(),
                Err(RoomTargetError::Unknown) => {
                    return Err(format!("unknown window target: {target}"));
                }
                Err(RoomTargetError::Ambiguous) => {
                    return Err(format!("ambiguous window target: {target}"));
                }
            };
            activate_view(hwnd, &ViewId::Room(room_id), None)?;
            Ok(Some(format!("window active: {target}")))
        }
        SlashCommand::Join { target } => open_room_link(hwnd, target, Vec::new()).map(Some),
        SlashCommand::List { server, filter } => {
            start_room_list(hwnd, RoomListQuery { server, filter }).map(Some)
        }
        SlashCommand::Create(args) => submit_create_room(state_mut(hwnd), args).map(Some),
        SlashCommand::Query { target, message } => {
            let user_id =
                commands::resolve_user(all_room_members(&state_ref(hwnd).model), &target)?.user_id;
            match resolve_direct_room_query_target(&state_ref(hwnd).model, &user_id) {
                QueryTargetResolution::Single(room_id) => {
                    activate_view(hwnd, &ViewId::Room(room_id.clone()), None)?;
                    if let Some(input) = message {
                        send_room_message(
                            state_mut(hwnd),
                            room_id.clone(),
                            RoomMessagePresentationKind::Text,
                            &input,
                        )?;
                        if let Some(notice) = away_notice(state_mut(hwnd), &room_id) {
                            let view = away_view(state_ref(hwnd), &room_id);
                            echo_in_window(hwnd, &view, [notice])?;
                        }
                    }
                    Ok(Some(format!("query opened: {user_id}")))
                }
                QueryTargetResolution::Ambiguous => {
                    Err(format!("ambiguous query target: {user_id}"))
                }
                QueryTargetResolution::None => {
                    let status = submit_query_slash_command(state_mut(hwnd), user_id.clone())?;
                    if message.is_some() {
                        echo_in_window(
                            hwnd,
                            window,
                            [format!(
                                "* /query: creating a query with {user_id}; message not sent"
                            )],
                        )?;
                    }
                    Ok(Some(status))
                }
            }
        }
        SlashCommand::Msg {
            target,
            body,
            presentation,
        } => {
            let room_id = msg_target_room_id(&state_ref(hwnd).model, &target)?;
            let sent = send_room_message(state_mut(hwnd), room_id.clone(), presentation, &body)?;
            let echo = commands::msg_echo(presentation, &target, &sent);
            echo_in_window(hwnd, window, [echo])?;
            if let Some(notice) = away_notice(state_mut(hwnd), &room_id) {
                let view = away_view(state_ref(hwnd), &room_id);
                echo_in_window(hwnd, &view, [notice])?;
            }
            Ok(None)
        }
        SlashCommand::Names { target } => {
            let line = {
                let state = state_ref(hwnd);
                let room_id = resolve_joined_room_target(state, window, target.as_deref())?;
                let room = room_by_id(&state.model.rooms, &room_id)
                    .expect("a joined room target must exist");
                commands::names_line(&room.display_name, room_members(&state.model, &room_id))
            };
            echo_in_window(hwnd, window, [line])?;
            Ok(None)
        }
        SlashCommand::Whois { nick } => {
            let state = state_mut(hwnd);
            let user_id = commands::resolve_user(all_room_members(&state.model), &nick)?.user_id;
            enqueue_command(state, Command::FetchUserProfile(user_id.clone()))?;
            Ok(Some(format!("whois requested: {user_id}")))
        }
        SlashCommand::Away { message } => {
            let status = match &message {
                Some(_) => "away submitted",
                None => "back submitted",
            };
            enqueue_command(state_mut(hwnd), Command::SetAway(message))?;
            Ok(Some(status.to_owned()))
        }
        SlashCommand::Devices { target } => {
            let state = state_mut(hwnd);
            let user_id = match target {
                Some(target) => {
                    commands::resolve_user(all_room_members(&state.model), &target)?.user_id
                }
                None => own_user_id(&state.model)?.to_owned(),
            };
            enqueue_command(state, Command::ListUserDevices(user_id.clone()))?;
            Ok(Some(format!("devices requested: {user_id}")))
        }
        SlashCommand::DevicesDelete { device_id } => {
            let status = format!("session sign-out submitted: {device_id}");
            Ok(sign_out_session(hwnd, hwnd, device_id)?.then_some(status))
        }
        SlashCommand::DevicesRename {
            device_id,
            display_name,
        } => {
            let status = format!("session rename submitted: {device_id}");
            submit_device_rename(state_mut(hwnd), device_id, display_name)?;
            Ok(Some(status))
        }
        SlashCommand::IdentityAccept { target } => {
            let state = state_mut(hwnd);
            let user_id = commands::resolve_user(all_room_members(&state.model), &target)?.user_id;
            if identity::user_identity_warning(&state.model, &user_id).is_none() {
                let nick = known_nick(&state.model, &user_id).unwrap_or_else(|| user_id.clone());
                return Err(format!("{nick}'s identity has not changed"));
            }
            enqueue_command(state, Command::AcceptUserIdentity(user_id.clone()))?;
            Ok(Some(format!("identity accept submitted: {user_id}")))
        }
        SlashCommand::Ignore { target: None } => {
            let lines = {
                let model = &state_ref(hwnd).model;
                ignore::list_lines(model.ignored_users.as_deref(), |user_id| {
                    known_nick(model, user_id)
                })?
            };
            echo_in_window(hwnd, window, lines)?;
            Ok(None)
        }
        SlashCommand::Ignore {
            target: Some(target),
        } => {
            let state = state_mut(hwnd);
            let user_id = user_target(&state.model, &target)?;
            ignore::check_ignore(
                state.model.ignored_users.as_deref(),
                state.model.own_user_id(),
                &user_id,
                |user_id| known_nick(&state.model, user_id),
            )?;
            enqueue_command(state, Command::IgnoreUser(user_id.clone()))?;
            Ok(Some(format!("ignore submitted: {user_id}")))
        }
        SlashCommand::Unignore { target } => {
            let state = state_mut(hwnd);
            let user_id = user_target(&state.model, &target)?;
            ignore::check_unignore(state.model.ignored_users.as_deref(), &user_id, |user_id| {
                known_nick(&state.model, user_id)
            })?;
            enqueue_command(state, Command::UnignoreUser(user_id.clone()))?;
            Ok(Some(format!("unignore submitted: {user_id}")))
        }
        SlashCommand::Notify(NotifyCommand::List) => {
            let lines = {
                let state = state_ref(hwnd);
                notify::list_lines(
                    &state.settings.settings().notify.list,
                    |user_id| {
                        state
                            .notify_watch
                            .is_connected()
                            .then(|| state.model.presence.is_on(user_id))
                            .flatten()
                    },
                    |user_id| known_nick(&state.model, user_id),
                )
            };
            echo_in_window(hwnd, window, lines)?;
            Ok(None)
        }
        SlashCommand::Notify(NotifyCommand::Add {
            target,
            note,
            whois,
        }) => {
            let line = {
                let state = state_mut(hwnd);
                let user_id = user_target(&state.model, &target)?;
                let mut list = state.settings.settings().notify.list.clone();
                let added = list.add(user_id.clone(), note, whois);
                let line = notify::added_line(
                    added,
                    list.get(&user_id).expect("the user was just listed"),
                    |user_id| known_nick(&state.model, user_id),
                );
                state
                    .settings
                    .update(|settings| settings.notify.list = list)?;
                line
            };
            echo_in_window(hwnd, window, [line])?;
            show_nick_colors(hwnd)?;
            // As mIRC asks the server at once: a user on now is news.
            announce_notify_changes(state_mut(hwnd))?;
            refresh_view_content(hwnd, &echo_view(window))?;
            Ok(Some("notify list saved".to_owned()))
        }
        SlashCommand::Notify(NotifyCommand::Remove { target }) => {
            let line = {
                let state = state_mut(hwnd);
                let user_id = user_target(&state.model, &target)?;
                let nick_of = |user_id: &str| known_nick(&state.model, user_id);
                let mut list = state.settings.settings().notify.list.clone();
                if !list.remove(&user_id) {
                    return Err(format!(
                        "{} is not on the notify list",
                        ignore::user_name(&user_id, &nick_of)
                    ));
                }
                let line = notify::removed_line(&user_id, nick_of);
                state
                    .settings
                    .update(|settings| settings.notify.list = list)?;
                line
            };
            echo_in_window(hwnd, window, [line])?;
            show_nick_colors(hwnd)?;
            Ok(Some("notify list saved".to_owned()))
        }
        SlashCommand::NickColor(NickColorCommand::List) => {
            let lines = {
                let state = state_ref(hwnd);
                nick_colors::list_lines(&state.settings.settings().nick_colors, |user_id| {
                    known_nick(&state.model, user_id)
                })
            };
            echo_in_window(hwnd, window, lines)?;
            Ok(None)
        }
        SlashCommand::NickColor(NickColorCommand::Active(active)) => {
            let mut setting = state_ref(hwnd).settings.settings().nick_colors.clone();
            setting.active = active;
            apply_nick_colors(hwnd, setting)?;
            echo_in_window(hwnd, window, [nick_colors::state_line(active)])?;
            Ok(Some("nick colors saved".to_owned()))
        }
        SlashCommand::NickColor(NickColorCommand::Add {
            target,
            color,
            ranks,
            notify,
            force,
        }) => {
            let (setting, line) = {
                let state = state_ref(hwnd);
                let entry = nick_colors::NickColorEntry {
                    pattern: nick_color_pattern(&state.model, &target)?,
                    color,
                    ranks,
                    notify,
                };
                let mut setting = state.settings.settings().nick_colors.clone();
                let line = nick_colors::added_line(
                    setting.list.add(entry.clone(), force),
                    &entry,
                    |user_id| known_nick(&state.model, user_id),
                );
                (setting, line)
            };
            apply_nick_colors(hwnd, setting)?;
            echo_in_window(hwnd, window, [line])?;
            Ok(Some("nick colors saved".to_owned()))
        }
        SlashCommand::NickColor(NickColorCommand::Remove(target)) => {
            let (setting, line) = {
                let state = state_ref(hwnd);
                let nick_of = |user_id: &str| known_nick(&state.model, user_id);
                let mut setting = state.settings.settings().nick_colors.clone();
                let index = match target {
                    NickColorTarget::Number(number) => number
                        .checked_sub(1)
                        .filter(|&index| index < setting.list.entries().len())
                        .ok_or_else(|| format!("no nick color entry {number}"))?,
                    NickColorTarget::Entry(target) => {
                        let pattern = nick_color_pattern(&state.model, &target)?;
                        setting.list.find(&pattern).ok_or_else(|| {
                            format!("{} is not on the nick color list", pattern.label(&nick_of))
                        })?
                    }
                };
                let line = nick_colors::removed_line(&setting.list.remove(index), nick_of);
                (setting, line)
            };
            apply_nick_colors(hwnd, setting)?;
            echo_in_window(hwnd, window, [line])?;
            Ok(Some("nick colors saved".to_owned()))
        }
        SlashCommand::Channel { target } => {
            let room_id = channel_central_room(state_ref(hwnd), window, target.as_deref())?;
            // Modal: it says what it sent itself.
            channel_central_dialog::run_channel_central_dialog(hwnd, room_id)?;
            Ok(None)
        }
        SlashCommand::AddressBook { tab } => {
            // Modal: it says what it saved itself.
            address_book_dialog::run_address_book_dialog(hwnd, tab)?;
            Ok(None)
        }
        SlashCommand::Close => {
            let state = state_ref(hwnd);
            if close_action(&state.model, window) == CloseAction::Refuse {
                return Err("the Status window cannot be closed".to_owned());
            }
            // Posted: closing may ask first, and destroys this editbox.
            let child = state.views[window].child;
            if PostMessageW(child, WM_CLOSE, 0, 0) == 0 {
                return Err(last_os_error("PostMessageW WM_CLOSE"));
            }
            Ok(None)
        }
        SlashCommand::Kick { nick, reason } => {
            let state = state_mut(hwnd);
            let (room_id, member) = room_member_target(state, window, &nick)?;
            if !member.can_kick {
                return Err(format!("you cannot kick {nick}"));
            }
            let user_id = member.user_id.clone();
            enqueue_command(
                state,
                Command::KickRoomMember(KickRoomMemberRequest {
                    room_id,
                    user_id: user_id.clone(),
                    reason,
                }),
            )?;
            Ok(Some(format!("kick submitted: {user_id}")))
        }
        SlashCommand::Ban { nick, reason } => {
            let state = state_mut(hwnd);
            let (room_id, member) = room_member_target(state, window, &nick)?;
            if !member.can_ban {
                return Err(format!("you cannot ban {nick}"));
            }
            let user_id = member.user_id.clone();
            enqueue_command(
                state,
                Command::BanRoomMember(BanRoomMemberRequest {
                    room_id,
                    user_id: user_id.clone(),
                    reason,
                }),
            )?;
            Ok(Some(format!("ban submitted: {user_id}")))
        }
        SlashCommand::Unban { target, reason } => {
            let state = state_mut(hwnd);
            let room_id = resolve_joined_room_target(state, window, None)?;
            let user_id = commands::resolve_user(all_room_members(&state.model), &target)?.user_id;
            enqueue_command(
                state,
                Command::UnbanRoomMember(UnbanRoomMemberRequest {
                    room_id,
                    user_id: user_id.clone(),
                    reason,
                }),
            )?;
            Ok(Some(format!("unban submitted: {user_id}")))
        }
        SlashCommand::Invite { nick, room } => {
            let state = state_mut(hwnd);
            let room_id = resolve_joined_room_target(state, window, room.as_deref())?;
            let user_id = commands::resolve_user(all_room_members(&state.model), &nick)?.user_id;
            if room_members(&state.model, &room_id)
                .iter()
                .any(|member| member.user_id == user_id)
            {
                return Err(format!("{nick} is already in this room"));
            }
            enqueue_command(
                state,
                Command::InviteRoomMember(InviteRoomMemberRequest {
                    room_id,
                    user_id: user_id.clone(),
                }),
            )?;
            Ok(Some(format!("invite submitted: {user_id}")))
        }
        SlashCommand::SetRank { nicks, change } => {
            let state = state_mut(hwnd);
            let room_id = resolve_joined_room_target(state, window, None)?;
            // One write carries every change or none: the first refusal
            // sends nothing.
            let changes = commands::member_role_changes(
                room_members(&state.model, &room_id),
                nicks.iter().map(|nick| (nick.as_str(), change)),
            )?;
            let user_ids = changes
                .iter()
                .map(|change| change.user_id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            enqueue_command(
                state,
                Command::UpdateRoomSettings(UpdateRoomSettingsRequest {
                    room_id,
                    topic_by_operators: None,
                    moderated: None,
                    member_roles: changes,
                    invite_only: None,
                    name: None,
                    topic: None,
                    listed_in_directory: None,
                    unban: Vec::new(),
                }),
            )?;
            Ok(Some(format!("power level change submitted: {user_ids}")))
        }
        SlashCommand::Nick { display_name } => {
            enqueue_command(
                state_mut(hwnd),
                Command::SetDisplayName(SetDisplayNameRequest {
                    display_name: display_name.clone(),
                }),
            )?;
            Ok(Some(format!("nick change submitted: {display_name}")))
        }
        SlashCommand::Server { server } => {
            connect(hwnd, server.as_deref())?;
            Ok(None)
        }
        SlashCommand::Quit => {
            disconnect(hwnd)?;
            Ok(None)
        }
        SlashCommand::Exit => {
            if PostMessageW(hwnd, WM_CLOSE, 0, 0) == 0 {
                return Err(last_os_error("PostMessageW WM_CLOSE"));
            }
            Ok(None)
        }
        SlashCommand::Accept { target } => {
            submit_accept_slash_command(state_mut(hwnd), window, target.as_deref()).map(Some)
        }
        SlashCommand::Reject { target } => {
            submit_reject_slash_command(state_mut(hwnd), window, target.as_deref()).map(Some)
        }
        SlashCommand::Part { target } => {
            submit_part_slash_command(state_mut(hwnd), window, target.as_deref()).map(Some)
        }
        SlashCommand::React {
            target_event_id,
            key,
        } => submit_react_slash_command(state_mut(hwnd), window, target_event_id, key).map(Some),
        SlashCommand::Reply {
            target_event_id,
            body: None,
        } => {
            arm_compose_target(
                hwnd,
                window,
                ComposeTarget::Reply {
                    event_id: target_event_id,
                },
            )?;
            Ok(None)
        }
        SlashCommand::Reply {
            target_event_id,
            body: Some(body),
        } => {
            send_reply(
                state_mut(hwnd),
                window,
                &target_event_id,
                RoomMessagePresentationKind::Text,
                &body,
            )?;
            Ok(Some(format!("Reply queued for {target_event_id}")))
        }
        SlashCommand::SendFile { path } => {
            let room_id = send_file_room_id(window)?;
            match path {
                Some(path) => {
                    submit_send_file(state_mut(hwnd), room_id, PathBuf::from(path)).map(Some)
                }
                None => run_send_file_dialog(hwnd, room_id),
            }
        }
        SlashCommand::Edit {
            target_event_id,
            body: None,
        } => {
            arm_compose_target(
                hwnd,
                window,
                ComposeTarget::Edit {
                    event_id: target_event_id,
                },
            )?;
            Ok(None)
        }
        SlashCommand::Edit {
            target_event_id,
            body: Some(body),
        } => {
            // Typed after the event ID, the text keeps the message's kind:
            // an emote stays an emote.
            let presentation = {
                let state = state_ref(hwnd);
                let activity = window_message(&state.model, window, &target_event_id)
                    .ok_or_else(|| "no such message in this window".to_owned())?;
                replies::editable_message(activity, state.model.own_user_id())?.presentation
            };
            send_edit(
                state_mut(hwnd),
                window,
                &target_event_id,
                presentation,
                &body,
            )?;
            Ok(Some(format!("Edit queued for {target_event_id}")))
        }
        SlashCommand::Redact { event_id } => {
            submit_redact_slash_command(state_mut(hwnd), window, event_id).map(Some)
        }
        SlashCommand::Unreact { reaction_event_id } => {
            submit_unreact_slash_command(state_mut(hwnd), window, reaction_event_id).map(Some)
        }
        SlashCommand::Topic {
            room,
            topic: Some(topic),
        } => submit_topic_slash_command(state_mut(hwnd), window, room.as_deref(), topic).map(Some),
        SlashCommand::Topic { room, topic: None } => {
            let line = room_topic_line(state_ref(hwnd), window, room.as_deref())?;
            echo_in_window(hwnd, window, [line])?;
            Ok(None)
        }
        SlashCommand::Clear => clear_window(hwnd, window).map(Some),
        SlashCommand::ClearAll(switches) => {
            let views = clearall_views(&state_ref(hwnd).model, switches);
            for view_id in &views {
                clear_window(hwnd, view_id)?;
            }
            Ok(Some(match views.len() {
                1 => "cleared 1 window".to_owned(),
                count => format!("cleared {count} windows"),
            }))
        }
        SlashCommand::AllChannels { presentation, body } => {
            let state = state_mut(hwnd);
            // Checked before the first send: a refusal sends nowhere.
            message_text_from_input(&body)?;
            let room_ids = channel_room_ids(&state.model);
            if room_ids.is_empty() {
                return Err("not in any channel".to_owned());
            }
            for room_id in &room_ids {
                send_room_message(state, room_id.clone(), presentation, &body)?;
            }
            Ok(Some(match room_ids.len() {
                1 => "sent to 1 channel".to_owned(),
                count => format!("sent to {count} channels"),
            }))
        }
        SlashCommand::SaveBuf { path } => match path {
            Some(path) => save_window_buffer(hwnd, window, Path::new(&path)).map(Some),
            None => run_save_buffer_dialog(hwnd, window),
        },
        SlashCommand::E2ee(command) => match command {
            E2eeSlashCommand::ShowStatus => {
                let state = state_mut(hwnd);
                let lines = match &state.model.last_e2ee_status {
                    Some(status) => status_log::e2ee_status(status),
                    None => vec![(
                        StatusLineKind::Info,
                        status_log::E2EE_STATUS_UNKNOWN.to_owned(),
                    )],
                };
                for (kind, line) in lines {
                    log_status(state, kind, line);
                }
                activate_view(hwnd, &ViewId::Status, None)?;
                Ok(Some("e2ee status shown".to_owned()))
            }
            E2eeSlashCommand::Refresh => {
                let state = state_mut(hwnd);
                let command = Command::RefreshE2eeStatus;
                state
                    .command_sender
                    .as_ref()
                    .ok_or_else(|| "core command sender is not available".to_owned())?
                    .send(command.clone())
                    .map_err(|err| {
                        format!("failed to enqueue refresh e2ee status command: {err}")
                    })?;
                #[cfg(test)]
                state.recorded_commands.push(command);
                Ok(Some("e2ee refresh submitted".to_owned()))
            }
            E2eeSlashCommand::RecoverDialog => {
                show_recover_e2ee_menu_dialog(hwnd)?;
                Ok(None)
            }
            E2eeSlashCommand::ResetDialog => {
                show_reset_e2ee_menu_dialog(hwnd)?;
                Ok(None)
            }
            E2eeSlashCommand::ResetCancel => {
                submit_cancel_identity_reset(state_mut(hwnd))?;
                refresh_status_bar(hwnd)?;
                Ok(None)
            }
            E2eeSlashCommand::Setup {
                passphrase,
                wait_for_backups_to_upload,
            } => {
                submit_setup_e2ee_recovery(
                    state_mut(hwnd),
                    passphrase,
                    wait_for_backups_to_upload,
                )?;
                refresh_status_bar(hwnd)?;
                Ok(None)
            }
            E2eeSlashCommand::RoomKeys { transfer, path } => {
                run_room_keys_dialogs(hwnd, transfer, path)
            }
        },
        SlashCommand::Help { topic } => {
            echo_in_window(hwnd, window, commands::help_lines(topic.as_deref())?)?;
            Ok(None)
        }
        SlashCommand::Echo(echo) => {
            print_echo(hwnd, window, echo)?;
            Ok(None)
        }
        SlashCommand::Alias(AliasCommand::List) => {
            echo_in_window(
                hwnd,
                window,
                alias_list_lines(state_ref(hwnd).aliases.aliases()),
            )?;
            Ok(None)
        }
        SlashCommand::Alias(AliasCommand::Remove(name)) => {
            let aliases = state_ref(hwnd).aliases.aliases().remove(&name)?;
            state_mut(hwnd).aliases.replace(aliases)?;
            Ok(Some(format!("alias removed: /{name}")))
        }
        SlashCommand::Alias(AliasCommand::Set { name, command }) => {
            let aliases = state_ref(hwnd).aliases.aliases().set(&name, &command)?;
            state_mut(hwnd).aliases.replace(aliases)?;
            Ok(Some(format!("alias set: /{name}")))
        }
        SlashCommand::Log(command) => {
            let (target, on) = match command {
                LogCommand::Show => (None, None),
                LogCommand::On { target } => (target, Some(true)),
                LogCommand::Off { target } => (target, Some(false)),
            };
            let view_id = log_command_view(state_ref(hwnd), window, target.as_deref())?;
            let line = match on {
                None => logging_is_line(state_ref(hwnd), &view_id),
                Some(on) => set_view_logging(state_mut(hwnd), &view_id, on)?,
            };
            echo_in_window(hwnd, window, [line])?;
            Ok(None)
        }
        SlashCommand::Font(FontCommand::Dialog) => run_font_dialog(hwnd),
        SlashCommand::Font(FontCommand::Set(setting)) => {
            apply_font_setting(hwnd, setting).map(Some)
        }
        SlashCommand::Timestamp(command) => match command {
            TimestampCommand::Show => {
                let timestamp = &state_ref(hwnd).settings.settings().timestamp;
                let line = format!(
                    "* Timestamp is {} ({})",
                    on_off(timestamp.show),
                    timestamp.format
                );
                echo_in_window(hwnd, window, [line])?;
                Ok(None)
            }
            TimestampCommand::On | TimestampCommand::Off => {
                let show = command == TimestampCommand::On;
                change_timestamp_setting(hwnd, |timestamp| timestamp.show = show)?;
                Ok(Some(format!("timestamp: {}", on_off(show))))
            }
            TimestampCommand::Format(format) => {
                let status = format!("timestamp format: {format}");
                change_timestamp_setting(hwnd, |timestamp| timestamp.format = format)?;
                Ok(Some(status))
            }
        },
        SlashCommand::Bar { bar, command } => {
            let show = match command {
                BarCommand::Show => {
                    let shown = state_ref(hwnd).settings.settings().bar_shown(bar);
                    echo_in_window(hwnd, window, [bar_is_line(bar, shown)])?;
                    return Ok(None);
                }
                BarCommand::On => true,
                BarCommand::Off => false,
            };
            state_mut(hwnd)
                .settings
                .update(|settings| settings.set_bar_shown(bar, show))?;
            layout_controls(hwnd);
            Ok(Some(format!("{}: {}", bar.name(), on_off(show))))
        }
        SlashCommand::VerifyStart { target, device_id } => {
            let state = state_mut(hwnd);
            let user_id = match target {
                Some(target) => {
                    commands::resolve_user(all_room_members(&state.model), &target)?.user_id
                }
                None => own_user_id(&state.model)?.to_owned(),
            };
            let status = match &device_id {
                Some(device_id) => format!("verification requested: {user_id} {device_id}"),
                None => format!("verification requested: {user_id}"),
            };
            enqueue_command(
                state,
                Command::RequestVerification(RequestVerificationRequest { user_id, device_id }),
            )?;
            Ok(Some(status))
        }
        SlashCommand::Verify { action } => {
            let state = state_mut(hwnd);
            match action {
                VerificationAction::Accept => submit_accept_verification(state),
                VerificationAction::Cancel => submit_cancel_verification(state),
                VerificationAction::Confirm => submit_confirm_verification_sas(state),
                VerificationAction::Reject => submit_reject_verification_sas(state),
            }?;
            Ok(Some(format!(
                "verification {} submitted",
                verification_action_label(action)
            )))
        }
    }
}

/// `/echo` given in `window`: its line goes in its window, `window` itself
/// without one. A window the user is not looking at gets mIRC's event
/// activity, as `/echo` without `-n` gives it.
unsafe fn print_echo(hwnd: HWND, window: &ViewId, echo: EchoCommand) -> Result<(), String> {
    let EchoCommand {
        color,
        place,
        timestamp,
        text,
    } = echo;
    let state = state_mut(hwnd);
    let view = match place {
        EchoPlace::Active => echo_view(window),
        EchoPlace::Status => ViewId::Status,
        EchoPlace::Window(target) => {
            let view = match resolve_room(
                &state.model.rooms,
                &state.model.joined_aliases,
                is_navigable_room,
                &target,
            ) {
                Ok(room) => ViewId::Room(room.room_id.clone()),
                Err(RoomTargetError::Unknown) => return Err(format!("no such window: {target}")),
                Err(RoomTargetError::Ambiguous) => {
                    return Err(format!("ambiguous window: {target}"));
                }
            };
            // A query window the user closed.
            if !state.views.contains_key(&view) {
                return Err(format!("no such window: {target}"));
            }
            view
        }
    };
    push_lines_in_view(
        state,
        &view,
        StatusLineKind::Echo { color, timestamp },
        [text],
    );
    if view != state.active_view {
        raise_view_activity(state, view, ViewActivityLevel::Event);
    }
    sync_views_and_tree(hwnd)
}

/// What `/alias` alone prints: every alias's lines, as written.
fn alias_list_lines(aliases: &Aliases) -> Vec<String> {
    let lines = aliases.list_lines();
    if lines.is_empty() {
        return vec!["* No aliases".to_owned()];
    }
    std::iter::once("* Aliases:".to_owned())
        .chain(lines.into_iter().map(|line| format!("*   {line}")))
        .collect()
}

/// The window `/log` given in `window` acts on: that one (a thread
/// window's room), or the room `target` names, whose window must be open.
fn log_command_view(
    state: &WindowState,
    window: &ViewId,
    target: Option<&str>,
) -> Result<ViewId, String> {
    let Some(target) = target else {
        return log_view(window);
    };
    let view_id = match resolve_room(
        &state.model.rooms,
        &state.model.joined_aliases,
        is_navigable_room,
        target,
    ) {
        Ok(room) => ViewId::Room(room.room_id.clone()),
        Err(RoomTargetError::Unknown) => return Err(format!("unknown room: {target}")),
        Err(RoomTargetError::Ambiguous) => return Err(format!("ambiguous room: {target}")),
    };
    if !state.views.contains_key(&view_id) {
        return Err(format!("{target} has no open window"));
    }
    Ok(view_id)
}

/// Saves a timestamp change and restamps every window's lines, which keep
/// their times rather than stamped text.
unsafe fn change_timestamp_setting(
    hwnd: HWND,
    change: impl FnOnce(&mut timestamp::TimestampSetting),
) -> Result<(), String> {
    state_mut(hwnd)
        .settings
        .update(|settings| change(&mut settings.timestamp))?;
    refresh_every_view_content(hwnd)
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// `* Switchbar is on`, what `/switchbar` alone prints.
fn bar_is_line(bar: Bar, shown: bool) -> String {
    format!("* {} is {}", bar.title(), on_off(shown))
}

/// A command's error, printed in the window it was given in; the status
/// bar says the same.
pub(super) unsafe fn report_command_error(
    hwnd: HWND,
    window: &ViewId,
    error: CommandError,
) -> Result<(), String> {
    report_error_line(hwnd, window, error.to_string())
}

/// An error line, `* ...`, printed in `window` as a command's errors are;
/// the status bar says the same.
pub(super) unsafe fn report_error_line(
    hwnd: HWND,
    window: &ViewId,
    text: String,
) -> Result<(), String> {
    echo_in_window(hwnd, window, [text.clone()])?;
    set_local_command_status(hwnd, text)
}

/// The window the lines of a command given in `window` print in: that one.
/// The Rooms List has no text of its own: what a Commands menu item run
/// there prints goes to Status.
pub(super) fn echo_view(window: &ViewId) -> ViewId {
    match window {
        ViewId::RoomList => ViewId::Status,
        view => view.clone(),
    }
}

/// Where a core answer of a mIRC "Show in active" kind prints: the window
/// in front when the option is on and it is a room or thread window, else
/// Status.
pub(super) fn answer_view(active: &ViewId, show_in_active: bool) -> ViewId {
    if show_in_active {
        echo_view(active)
    } else {
        ViewId::Status
    }
}

/// Prints the lines of a core answer in the window in front now, where the
/// user looks when it arrives. They raise no activity. The caller
/// refreshes the window.
pub(super) fn echo_lines_in_front_window(
    state: &mut WindowState,
    lines: impl IntoIterator<Item = String>,
) {
    let view = echo_view(&state.active_view);
    echo_lines_in_view(state, &view, lines);
}

/// Prints lines in a window with text: Status, a room or a thread window.
/// They raise no activity. The caller refreshes the window.
pub(super) fn echo_lines_in_view(
    state: &mut WindowState,
    view: &ViewId,
    lines: impl IntoIterator<Item = String>,
) {
    push_lines_in_view(state, view, StatusLineKind::Info, lines);
}

/// Prints lines of `kind` in a window with text. The caller refreshes the
/// window.
pub(super) fn push_lines_in_view(
    state: &mut WindowState,
    view: &ViewId,
    kind: StatusLineKind,
    lines: impl IntoIterator<Item = String>,
) {
    let now = state.clock.now_ms();
    for text in lines {
        match view {
            ViewId::Status => state.model.status_log.push(now, kind, text),
            ViewId::RoomList => unreachable!("Rooms List output goes to Status"),
            ViewId::Room(room_id) => state
                .model
                .room_views
                .entry_or_default(room_id.clone())
                .push_echo(now, None, kind, text),
            ViewId::Thread {
                room_id,
                thread_root_event_id,
            } => state
                .model
                .room_views
                .get_mut(room_id)
                .expect("a thread window's room must have a view")
                .push_echo(now, Some(thread_root_event_id.clone()), kind, text),
        }
    }
}

/// Prints the lines of a command given in `window` there, and shows them.
pub(super) unsafe fn echo_in_window(
    hwnd: HWND,
    window: &ViewId,
    lines: impl IntoIterator<Item = String>,
) -> Result<(), String> {
    let view = echo_view(window);
    echo_lines_in_view(state_mut(hwnd), &view, lines);
    refresh_view_content(hwnd, &view)
}

pub(super) unsafe fn set_local_command_status(hwnd: HWND, message: String) -> Result<(), String> {
    let has_status_view = {
        let state = state_mut(hwnd);
        state.model.status = message;
        state.views.contains_key(&ViewId::Status)
    };

    refresh_status_bar(hwnd)?;
    if has_status_view {
        refresh_view_content(hwnd, &ViewId::Status)?;
    }
    Ok(())
}

fn resolve_direct_room_query_target(model: &UiModel, user_id: &str) -> QueryTargetResolution {
    let mut matches = model
        .rooms
        .iter()
        .filter(|room| room.is_direct && room.direct_user_ids.iter().any(|id| id == user_id));
    let Some(room) = matches.next() else {
        return QueryTargetResolution::None;
    };
    if matches.next().is_some() {
        return QueryTargetResolution::Ambiguous;
    }
    QueryTargetResolution::Single(room.room_id.clone())
}

/// The members the client knows in a room, in nick list order.
fn room_members<'m>(model: &'m UiModel, room_id: &str) -> &'m [RoomMemberSummary] {
    model
        .room_views
        .get(room_id)
        .map(|view| view.members.as_slice())
        .unwrap_or_default()
}

/// The joined room a member command given in `window` acts on, and the
/// member the nick names there.
fn room_member_target<'s>(
    state: &'s WindowState,
    window: &ViewId,
    nick: &str,
) -> Result<(String, &'s RoomMemberSummary), String> {
    let room_id = resolve_joined_room_target(state, window, None)?;
    let member = commands::resolve_nick(room_members(&state.model, &room_id), nick)?;
    Ok((room_id, member))
}

/// Enqueues a command for the core, recording it for the automation dump.
pub(super) fn enqueue_command(state: &mut WindowState, command: Command) -> Result<(), String> {
    state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(())
}

/// Every known member of every room; a user in several rooms is listed
/// once per room.
fn all_room_members(model: &UiModel) -> impl Iterator<Item = &RoomMemberSummary> {
    model
        .room_views
        .values()
        .flat_map(|view| view.members.iter())
}

/// The nick a member of a known room goes by. `None` for a user no known
/// room has.
pub(super) fn known_nick(model: &UiModel, user_id: &str) -> Option<String> {
    all_room_members(model)
        .find(|member| member.user_id == user_id)
        .map(|member| member.nick.clone())
}

/// The user `/ignore`, `/unignore` and `/notify` name: a user ID, which
/// need not be a member of any known room, or a known member's nick.
/// Ignored and listed users stay nameable by ID once they leave every room.
fn user_target(model: &UiModel, target: &str) -> Result<String, String> {
    Ok(commands::resolve_user(all_room_members(model), target)?.user_id)
}

/// Whom `/cnick` names: a pattern with `*` or `?` as typed, else a user
/// as `/notify` names one.
fn nick_color_pattern(model: &UiModel, target: &str) -> Result<nick_colors::UserPattern, String> {
    if target.contains(['*', '?']) {
        nick_colors::UserPattern::new(target)
    } else {
        nick_colors::UserPattern::new(&user_target(model, target)?)
    }
}

/// Where `/msg` sends: a room by ID, alias or display name, else the query
/// with a user named by ID or nick. Like mIRC's `/msg`, it does not open a
/// query.
/// The channel windows' rooms, in window order (`Ctrl+N`'s): `/amsg`'s
/// and `/ame`'s targets.
fn channel_room_ids(model: &UiModel) -> Vec<String> {
    rooms_in_window_order(&model.rooms)
        .into_iter()
        .filter(|room| room_window_kind(room) == RoomWindowKind::Channel)
        .map(|room| room.room_id.clone())
        .collect()
}

/// The windows `/clearall` clears: Status first, then the rooms' in
/// window order. Thread windows have no buffer of their own.
fn clearall_views(model: &UiModel, switches: commands::ClearAll) -> Vec<ViewId> {
    let status = switches.status.then_some(ViewId::Status);
    let rooms = rooms_in_window_order(&model.rooms)
        .into_iter()
        .filter(move |room| match room_window_kind(room) {
            RoomWindowKind::Channel => switches.channels,
            RoomWindowKind::Query => switches.queries,
            RoomWindowKind::Invite => switches.invites,
        })
        .map(|room| ViewId::Room(room.room_id.clone()));
    status.into_iter().chain(rooms).collect()
}

fn msg_target_room_id(model: &UiModel, target: &str) -> Result<String, String> {
    match resolve_room(
        &model.rooms,
        &model.joined_aliases,
        is_navigable_room,
        target,
    ) {
        Ok(room) => return Ok(room.room_id.clone()),
        Err(RoomTargetError::Ambiguous) => return Err(format!("ambiguous target: {target}")),
        Err(RoomTargetError::Unknown) => {}
    }
    let user_id = commands::resolve_user(all_room_members(model), target)?.user_id;
    match resolve_direct_room_query_target(model, &user_id) {
        QueryTargetResolution::Single(room_id) => Ok(room_id),
        QueryTargetResolution::Ambiguous => Err(format!("ambiguous query target: {user_id}")),
        QueryTargetResolution::None => Err(format!("no query with {target}; use /query {target}")),
    }
}

/// `/whois`'s answer: the server's profile, with the nick and the joined
/// rooms shared with the user from what the client knows.
pub(super) fn whois_lines(model: &UiModel, profile: &UserProfile) -> Vec<String> {
    let shared_rooms = model
        .rooms
        .iter()
        .filter(|room| is_sendable_room(room))
        .filter_map(|room| {
            let member = model
                .room_views
                .get(&room.room_id)?
                .members
                .iter()
                .find(|member| member.user_id == profile.user_id)?;
            Some((member.rank, room.display_name.as_str()))
        })
        .collect::<Vec<_>>();
    let room_nick = known_nick(model, &profile.user_id);
    commands::whois_lines(profile, room_nick.as_deref(), &shared_rooms)
}

/// `/devices`' answer, with the nick a room knows the user by.
pub(super) fn devices_lines(
    model: &UiModel,
    devices: &UserDevices,
    local_time: LocalTimeFn,
) -> Vec<String> {
    let nick = known_nick(model, &devices.user_id).unwrap_or_else(|| devices.user_id.clone());
    let own_user = model.own_user_id() == Some(devices.user_id.as_str());
    let identity_changed = identity::user_identity_warning(model, &devices.user_id).is_some();
    commands::devices_lines(devices, &nick, own_user, identity_changed, local_time)
}

/// The joined room a command given in `window` acts on: `target`, else
/// `window`'s room (a thread window's too).
pub(super) fn resolve_joined_room_target(
    state: &WindowState,
    window: &ViewId,
    target: Option<&str>,
) -> Result<String, String> {
    let Some(target) = target else {
        return match window {
            ViewId::Room(room_id) => {
                if sendable_room_by_id(&state.model.rooms, room_id).is_some() {
                    Ok(room_id.clone())
                } else {
                    Err(format!("room is not joined: {room_id}"))
                }
            }
            ViewId::Thread { room_id, .. } => Ok(room_id.clone()),
            ViewId::Status | ViewId::RoomList => Err("no room selected".to_owned()),
        };
    };

    match resolve_room(
        &state.model.rooms,
        &state.model.joined_aliases,
        is_navigable_room,
        target,
    ) {
        Ok(room) if is_sendable_room(room) => Ok(room.room_id.clone()),
        Ok(_) => Err(format!("room is not joined: {target}")),
        Err(RoomTargetError::Unknown) => Err(format!("unknown room: {target}")),
        Err(RoomTargetError::Ambiguous) => Err(format!("ambiguous room: {target}")),
    }
}

/// The room Channel Central opens for: `target`, else `window`'s room. It
/// reads the server, so it needs the session; a query is no channel.
pub(super) fn channel_central_room(
    state: &WindowState,
    window: &ViewId,
    target: Option<&str>,
) -> Result<String, String> {
    own_user_id(&state.model)?;
    let room_id = resolve_joined_room_target(state, window, target)?;
    let room = room_by_id(&state.model.rooms, &room_id).expect("a joined room target must exist");
    if room.is_direct {
        return Err(format!(
            "{} is a direct message; Room Settings are for rooms",
            room.display_name
        ));
    }
    Ok(room_id)
}

/// The invite `/accept` or `/reject` given in `window` answers: `target`,
/// else `window`'s room.
pub(super) fn resolve_invite_command_room_id(
    state: &WindowState,
    window: &ViewId,
    action: &str,
    target: Option<&str>,
) -> Result<String, String> {
    let Some(target) = target else {
        return match window {
            ViewId::Room(room_id)
                if room_by_id(&state.model.rooms, room_id)
                    .is_some_and(is_invite_command_target) =>
            {
                Ok(room_id.clone())
            }
            _ => Err(format!("{action} failed: no invited room selected")),
        };
    };

    let (rooms, joined_aliases) = (&state.model.rooms, &state.model.joined_aliases);
    match resolve_room(rooms, joined_aliases, is_invite_command_target, target) {
        Ok(room) => Ok(room.room_id.clone()),
        Err(RoomTargetError::Ambiguous) => Err(format!(
            "{action} failed: ambiguous invited room target: {target}"
        )),
        Err(RoomTargetError::Unknown) => {
            match resolve_room(rooms, joined_aliases, is_joined_message_room, target) {
                Ok(_) | Err(RoomTargetError::Ambiguous) => {
                    Err(format!("{action} failed: room is not invited: {target}"))
                }
                Err(RoomTargetError::Unknown) => {
                    Err(format!("{action} failed: invited room not found: {target}"))
                }
            }
        }
    }
}

/// Signs out another of the user's sessions (`/devices delete`, the
/// Sessions dialog's Sign Out...): a password account confirms in the Sign
/// Out Session dialog, owned by `owner`; an OAuth 2.0 account's page
/// confirms it on its own. `false` when the user cancelled: nothing sent.
pub(super) unsafe fn sign_out_session(
    owner: HWND,
    hwnd: HWND,
    device_id: String,
) -> Result<bool, String> {
    let auth = match &state_ref(hwnd).model.own_account {
        Some(account) => account.auth.clone(),
        None => return Err("not connected to server".to_owned()),
    };
    let password = match auth {
        SessionAuth::OAuth { .. } => None,
        SessionAuth::Matrix => match show_sign_out_session_dialog(owner, &device_id)? {
            Some(form) => form.password,
            None => return Ok(false),
        },
    };
    submit_device_sign_out(state_mut(hwnd), device_id, password)?;
    Ok(true)
}

/// The user's own ID, for commands that need a session.
pub(super) fn own_user_id(model: &UiModel) -> Result<&str, String> {
    if matches!(
        model.connection,
        ConnectionState::SignedOut
            | ConnectionState::SigningIn
            | ConnectionState::WaitingForBrowser
    ) {
        return Err("not connected to server".to_owned());
    }
    Ok(model
        .own_user_id()
        .expect("a signed-in session knows its user"))
}

/// The user's homeserver name, for commands that need a session.
pub(super) fn own_server(model: &UiModel) -> Result<&str, String> {
    let (_, server) = own_user_id(model)?
        .split_once(':')
        .expect("a Matrix user ID names its server");
    Ok(server)
}

/// `/create`: a public room's alias must be on the user's own server, the
/// only one that can publish it.
fn submit_create_room(state: &mut WindowState, args: CreateRoomArgs) -> Result<String, String> {
    let own_server = own_server(&state.model)?;
    let request = match args {
        CreateRoomArgs::Private { name } => CreateRoomRequest {
            name,
            visibility: CreateRoomVisibility::Private,
        },
        CreateRoomArgs::Public {
            alias_localpart,
            alias_server,
            name,
        } => {
            if alias_server.is_some_and(|server| server != own_server) {
                return Err(format!("the alias must be on your server ({own_server})"));
            }
            CreateRoomRequest {
                name: name.unwrap_or_else(|| alias_localpart.clone()),
                visibility: CreateRoomVisibility::Public { alias_localpart },
            }
        }
    };
    let status = format!("create submitted: {}", request.name);
    enqueue_command(state, Command::CreateRoom(request))?;
    Ok(status)
}

/// The room `target`, a room ID or alias as `/join` or a link names it: its
/// window comes to the front if the user has one, joined or invited, as
/// mIRC's `/join #channel` does for a channel it is on. Any other room is
/// joined through `via`, and its window opens when the join lands.
pub(super) unsafe fn open_room_link(
    hwnd: HWND,
    target: String,
    via: Vec<String>,
) -> Result<String, String> {
    let model = &state_ref(hwnd).model;
    let room_id = model
        .rooms
        .iter()
        .find(|room| {
            is_navigable_room(room) && room_answers_to(room, &model.joined_aliases, &target)
        })
        .map(|room| room.room_id.clone());
    match room_id {
        Some(room_id) => {
            activate_view(hwnd, &ViewId::Room(room_id), None)?;
            Ok(format!("window active: {target}"))
        }
        None => submit_join_room(state_mut(hwnd), target, via),
    }
}

/// Whether `target` is `room`'s ID or one of its aliases (published, or
/// joined by: `room_targets::aliases`), exactly: display names, which
/// `/window` also takes, name no room to join.
fn room_answers_to(
    room: &RoomSummary,
    joined_aliases: &BTreeMap<String, String>,
    target: &str,
) -> bool {
    room.room_id == target
        || room_targets::aliases(room, joined_aliases).any(|alias| alias == target)
}

pub(super) fn submit_join_room(
    state: &mut WindowState,
    target: String,
    via: Vec<String>,
) -> Result<String, String> {
    let command = Command::JoinRoom(JoinRoomRequest {
        room_or_alias: target.clone(),
        via,
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue join room command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("join submitted: {target}"))
}

pub(super) fn submit_accept_slash_command(
    state: &mut WindowState,
    window: &ViewId,
    target: Option<&str>,
) -> Result<String, String> {
    let room_id = resolve_invite_command_room_id(state, window, "accept", target)?;
    // The user's accept activates the window, even of an invite an
    // auto-join accepted already.
    state.quiet_joins.remove(&room_id);
    submit_accept_room_invite(state, room_id)
}

/// Asks the core to accept the invite to `room_id`; the status that says
/// so.
pub(super) fn submit_accept_room_invite(
    state: &mut WindowState,
    room_id: String,
) -> Result<String, String> {
    let command = Command::AcceptRoomInvite(AcceptRoomInviteRequest {
        room_id: room_id.clone(),
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue accept room invite command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("accept submitted: {room_id}"))
}

pub(super) fn submit_reject_slash_command(
    state: &mut WindowState,
    window: &ViewId,
    target: Option<&str>,
) -> Result<String, String> {
    let room_id = resolve_invite_command_room_id(state, window, "reject", target)?;
    let command = Command::RejectRoomInvite(RejectRoomInviteRequest {
        room_id: room_id.clone(),
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue reject room invite command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("reject submitted: {room_id}"))
}

pub(super) fn submit_part_slash_command(
    state: &mut WindowState,
    window: &ViewId,
    target: Option<&str>,
) -> Result<String, String> {
    let room_id = resolve_joined_room_target(state, window, target)?;
    let command = Command::LeaveRoom(LeaveRoomRequest {
        room_id: room_id.clone(),
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue leave room command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("part submitted: {room_id}"))
}

/// Room > Notifications: sets the room's mode on the server. The check
/// follows the next room snapshot, not the click.
pub(super) fn submit_room_notification_mode(
    state: &mut WindowState,
    window: &ViewId,
    target: Option<&str>,
    mode: RoomNotificationMode,
) -> Result<String, String> {
    let room_id = resolve_joined_room_target(state, window, target)?;
    let display_name = sendable_room_by_id(&state.model.rooms, &room_id)
        .expect("the joined room target is a joined room")
        .display_name
        .clone();
    enqueue_command(
        state,
        Command::SetRoomNotificationMode(SetRoomNotificationModeRequest { room_id, mode }),
    )?;
    Ok(format!(
        "notifications for {display_name}: {}",
        notification_mode_label(mode)
    ))
}

fn submit_react_slash_command(
    state: &mut WindowState,
    window: &ViewId,
    target_event_id: String,
    key: String,
) -> Result<String, String> {
    let room_id = resolve_joined_room_target(state, window, None)?;
    let command = Command::SendReaction(SendReactionRequest {
        room_id: room_id.clone(),
        target_event_id: target_event_id.clone(),
        key,
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue send reaction command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("Reaction queued for {target_event_id}"))
}

/// Replaces the user's message `target_event_id` in `window` with
/// `presentation` and `input`; sends nothing when that is what it says.
pub(super) fn send_edit(
    state: &mut WindowState,
    window: &ViewId,
    target_event_id: &str,
    presentation: RoomMessagePresentationKind,
    input: &str,
) -> Result<(), String> {
    let message = replies::editable_message(
        window_message(&state.model, window, target_event_id)
            .ok_or_else(|| "no such message in this window".to_owned())?,
        state.model.own_user_id(),
    )?;
    let (body, styles) = message_text_from_input(input)?;
    if message.presentation == presentation && message.body == body && message.styles == styles {
        return Ok(());
    }
    let room_id = resolve_joined_room_target(state, window, None)?;
    enqueue_command(
        state,
        Command::EditMessage(EditMessageRequest {
            room_id,
            target_event_id: target_event_id.to_owned(),
            presentation,
            body,
            styles,
        }),
    )
}

/// Sends a reply to the message `target_event_id` in `window`, in its
/// thread when the window is one.
pub(super) fn send_reply(
    state: &mut WindowState,
    window: &ViewId,
    target_event_id: &str,
    presentation: RoomMessagePresentationKind,
    input: &str,
) -> Result<(), String> {
    let (body, styles) = message_text_from_input(input)?;
    let destination = resolve_compose_destination(state, window, None)?;
    let sender_user_id = room_activity_sender_user_id(
        window_message(&state.model, window, target_event_id)
            .ok_or_else(|| "no such message in this window".to_owned())?,
    )
    .expect("a message that is not redacted has a sender")
    .to_owned();
    let clears_thread_marker = destination.thread.is_some();
    enqueue_send_message(
        state,
        SendMessageRequest {
            room_id: destination.room_id.clone(),
            presentation,
            body,
            styles,
            thread: destination.thread,
            reply: Some(SendMessageReplyTarget {
                event_id: target_event_id.to_owned(),
                sender_user_id,
            }),
        },
    )?;
    if clears_thread_marker {
        state.room_activity.remove(window);
    }
    let notice = away_notice(state, &destination.room_id);
    echo_lines_in_view(state, &echo_view(window), notice);
    Ok(())
}

fn submit_redact_event_command(
    state: &mut WindowState,
    window: &ViewId,
    event_id: String,
) -> Result<(), String> {
    let room_id = resolve_joined_room_target(state, window, None)?;
    let command = Command::RedactEvent(RedactEventRequest { room_id, event_id });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue redact event command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(())
}

pub(super) fn submit_redact_slash_command(
    state: &mut WindowState,
    window: &ViewId,
    event_id: String,
) -> Result<String, String> {
    submit_redact_event_command(state, window, event_id.clone())?;
    Ok(format!("Redaction queued for {event_id}"))
}

fn submit_unreact_slash_command(
    state: &mut WindowState,
    window: &ViewId,
    reaction_event_id: String,
) -> Result<String, String> {
    submit_redact_event_command(state, window, reaction_event_id.clone())?;
    Ok(format!("Reaction redaction queued for {reaction_event_id}"))
}

pub(super) fn submit_query_slash_command(
    state: &mut WindowState,
    user_id: String,
) -> Result<String, String> {
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    let command = Command::CreateDirectRoom(CreateDirectRoomRequest {
        user_id: user_id.clone(),
    });
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue create direct room command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("query submitted: {user_id}"))
}

/// `/topic [#room]`: the room's topic, as mIRC prints it.
fn room_topic_line(
    state: &WindowState,
    window: &ViewId,
    room: Option<&str>,
) -> Result<String, String> {
    let room_id = resolve_joined_room_target(state, window, room)?;
    Ok(commands::topic_line(
        state.model.room_views[&room_id].topic.as_deref(),
    ))
}

fn submit_topic_slash_command(
    state: &mut WindowState,
    window: &ViewId,
    room: Option<&str>,
    topic: String,
) -> Result<String, String> {
    let room_id = resolve_joined_room_target(state, window, room)?;
    let command = Command::SetRoomTopic(SetRoomTopicRequest {
        room_id: room_id.clone(),
        topic,
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue set room topic command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("topic change submitted for {room_id}"))
}

#[cfg(test)]
mod tests {
    use mirx_core::{
        DeviceSummary, IdentityChange, IdentityTrust, MemberPowerLevelChange, PresenceChange,
        PresenceKind, RoomIdentityChanges, RoomMemberRank, UserIdentityState,
    };

    use super::super::notify::NotifyList;
    use super::super::test_fixtures::*;
    use super::*;

    /// The `UpdateRoomSettings` of `/op` and the like: only member roles.
    fn member_roles_command(room_id: &str, roles: &[(&str, MemberPowerRole)]) -> Command {
        Command::UpdateRoomSettings(UpdateRoomSettingsRequest {
            room_id: room_id.to_owned(),
            topic_by_operators: None,
            moderated: None,
            member_roles: roles
                .iter()
                .map(|(user_id, role)| MemberPowerLevelChange {
                    user_id: (*user_id).to_owned(),
                    role: *role,
                })
                .collect(),
            invite_only: None,
            name: None,
            topic: None,
            listed_in_directory: None,
            unban: Vec::new(),
        })
    }

    /// A channel Ops with Alice and Bob, and a query with Alice; Ops active.
    fn irc_command_state() -> (WindowState, CoreHandle) {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![
            room("!ops:example", "Ops", false),
            query("!dm:example", "Alice", "@alice:example"),
        ];
        let mut bob = member_summary("@bob:example", "Bob", false);
        bob.rank = RoomMemberRank::Moderator;
        let ops = RoomViewModel {
            members: vec![member_summary("@alice:example", "Alice", false), bob],
            ..Default::default()
        };
        state
            .model
            .room_views
            .insert("!ops:example".to_owned(), ops);
        let dm = RoomViewModel {
            members: vec![member_summary("@alice:example", "Alice", false)],
            ..Default::default()
        };
        state.model.room_views.insert("!dm:example".to_owned(), dm);
        state.active_view = room_view_id("!ops:example");
        (state, handle)
    }

    fn sent_message(
        room_id: &str,
        presentation: RoomMessagePresentationKind,
        body: &str,
    ) -> Command {
        Command::SendMessage(SendMessageRequest {
            room_id: room_id.to_owned(),
            presentation,
            body: body.to_owned(),
            styles: Vec::new(),
            thread: None,
            reply: None,
        })
    }

    #[test]
    fn msg_and_notice_send_into_the_query_and_echo_in_the_active_window() {
        let (state, handle) = irc_command_state();
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/msg alice hi there");
                type_compose_line(hwnd, "/notice Alice heads up");
                type_compose_line(hwnd, "/msg Bob hi");
                type_compose_line(hwnd, "/msg Nobody hi");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![
                        sent_message("!dm:example", RoomMessagePresentationKind::Text, "hi there"),
                        sent_message(
                            "!dm:example",
                            RoomMessagePresentationKind::Notice,
                            "heads up"
                        ),
                    ]
                );
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    &lines[lines.len() - 4..],
                    [
                        "[22:12] -> *alice* hi there",
                        "[22:12] -> -Alice- heads up",
                        "[22:12] * /msg: no query with Bob; use /query Bob",
                        "[22:12] * /msg: no such nick: Nobody",
                    ]
                );
                assert_eq!(state_ref(hwnd).active_view, room_view_id("!ops:example"));
            });
        }
        stop_test_core(handle);
    }

    /// Rooms of every kind, out of window order.
    fn mixed_rooms_model() -> UiModel {
        UiModel {
            rooms: vec![
                query("!dm:example", "Alice", "@alice:example"),
                room("!ops:example", "Ops", false),
                invited_room("!invite:example", "Invite"),
                space("!space:example", "Space"),
                room("!dev:example", "Dev", true),
            ],
            ..UiModel::default()
        }
    }

    #[test]
    fn every_channel_is_the_joined_rooms_that_are_not_queries_in_window_order() {
        assert_eq!(
            channel_room_ids(&mixed_rooms_model()),
            ["!ops:example", "!dev:example"]
        );
    }

    #[test]
    fn clearall_clears_the_windows_its_switches_name() {
        let model = mixed_rooms_model();
        let switches = |line: &str| match commands::parse(line) {
            Ok(SlashCommand::ClearAll(switches)) => switches,
            other => panic!("{line}: {other:?}"),
        };
        let room = |room_id: &str| ViewId::Room(room_id.to_owned());
        assert_eq!(
            clearall_views(&model, switches("/clearall")),
            [
                ViewId::Status,
                room("!ops:example"),
                room("!dev:example"),
                room("!dm:example"),
                room("!invite:example"),
            ]
        );
        assert_eq!(
            clearall_views(&model, switches("/clearall -s")),
            [ViewId::Status]
        );
        assert_eq!(
            clearall_views(&model, switches("/clearall -n")),
            [room("!ops:example"), room("!dev:example")]
        );
        assert_eq!(
            clearall_views(&model, switches("/clearall -q")),
            [room("!dm:example")]
        );
        assert_eq!(
            clearall_views(&model, switches("/clearall -s -q")),
            [ViewId::Status, room("!dm:example")]
        );
        assert_eq!(
            clearall_views(&model, switches("/clearall -sn")),
            [ViewId::Status, room("!ops:example"), room("!dev:example")]
        );
    }

    #[test]
    fn amsg_and_ame_send_to_every_channel_and_describe_to_one() {
        let (mut state, handle) = irc_command_state();
        state.model.rooms.push(room("!dev:example", "Dev", false));
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/amsg hi all");
                type_compose_line(hwnd, "/ame waves");
                // Refused before the first send.
                type_compose_line(hwnd, "/amsg \x02\x02");
                type_compose_line(hwnd, "/describe alice nods");
                type_compose_line(hwnd, "/describe #Dev");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![
                        sent_message("!ops:example", RoomMessagePresentationKind::Text, "hi all"),
                        sent_message("!dev:example", RoomMessagePresentationKind::Text, "hi all"),
                        sent_message("!ops:example", RoomMessagePresentationKind::Emote, "waves"),
                        sent_message("!dev:example", RoomMessagePresentationKind::Emote, "waves"),
                        sent_message("!dm:example", RoomMessagePresentationKind::Emote, "nods"),
                    ]
                );
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    &lines[lines.len() - 3..],
                    [
                        "[22:12] * /amsg: message body must not be empty",
                        "[22:12] -> alice * nods",
                        "[22:12] * /describe: insufficient parameters",
                    ]
                );
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn amsg_without_a_channel_says_so() {
        let (mut state, handle) = irc_command_state();
        state.model.rooms.retain(|room| room.is_direct);
        state.active_view = room_view_id("!dm:example");
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/amsg hi");
                assert!(state_ref(hwnd).recorded_commands.is_empty());
                let lines = render_room_lines(state_ref(hwnd), "!dm:example");
                assert_eq!(lines.last().unwrap(), "[22:12] * /amsg: not in any channel");
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn msg_sends_formatting_codes_as_styles_and_echoes_the_text() {
        let (state, handle) = irc_command_state();
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/msg alice \x034red\x03 alert");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![Command::SendMessage(SendMessageRequest {
                        room_id: "!dm:example".to_owned(),
                        presentation: RoomMessagePresentationKind::Text,
                        body: "red alert".to_owned(),
                        styles: vec![TextStyleSpan {
                            start: 0,
                            end: 3,
                            style: mirx_core::TextStyle {
                                foreground: Some(mirx_core::TextColor {
                                    red: 0xff,
                                    green: 0,
                                    blue: 0
                                }),
                                ..mirx_core::TextStyle::default()
                            },
                        }],
                        thread: None,
                        reply: None,
                    })]
                );
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(lines.last().unwrap(), "[22:12] -> *alice* red alert");
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn whois_names_a_member_without_a_display_name_by_their_nick() {
        let mut model = UiModel::default();
        model.room_views.insert(
            "!ops:example".to_owned(),
            RoomViewModel {
                members: vec![member_summary("@ghost:example", "ghost", false)],
                ..Default::default()
            },
        );
        let profile = UserProfile {
            user_id: "@ghost:example".to_owned(),
            display_name: None,
            presence: None,
        };
        assert_eq!(
            whois_lines(&model, &profile)[0],
            "* ghost is @ghost:example"
        );
        assert_eq!(known_nick(&model, "@stranger:example"), None);
    }

    #[test]
    fn names_and_whois_print_member_data_in_the_active_window() {
        let (state, handle) = irc_command_state();
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/names");
                type_compose_line(hwnd, "/names Nowhere");
                // /whois asks the server, for members and strangers alike,
                // and prints its answer.
                type_compose_line(hwnd, "/whois Nobody");
                type_compose_line(hwnd, "/whois alice");
                type_compose_line(hwnd, "/whois @stranger:remote");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![
                        Command::FetchUserProfile("@alice:example".to_owned()),
                        Command::FetchUserProfile("@stranger:remote".to_owned()),
                    ]
                );
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "whois requested: @stranger:remote"
                );
                for profile in [
                    UserProfile {
                        user_id: "@alice:example".to_owned(),
                        display_name: Some("Alice Liddell".to_owned()),
                        presence: None,
                    },
                    UserProfile {
                        user_id: "@stranger:remote".to_owned(),
                        display_name: None,
                        presence: None,
                    },
                ] {
                    apply_core_event(state_mut(hwnd), Event::UserProfile(profile)).unwrap();
                }
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    &lines[lines.len() - 8..],
                    [
                        "[22:12] * Members of Ops: Alice @Bob",
                        "[22:12] * /names: unknown room: Nowhere",
                        "[22:12] * /whois: no such nick: Nobody",
                        "[22:12] * Alice is @alice:example (Alice Liddell)",
                        "[22:12] * Alice shares rooms with you: Ops Alice",
                        "[22:12] * Alice's homeserver is example",
                        "[22:12] * @stranger:remote is @stranger:remote",
                        "[22:12] * @stranger:remote's homeserver is remote",
                    ]
                );
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn devices_asks_the_server_and_prints_whois_style() {
        let (state, handle) = irc_command_state();
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                // Signed out, there are no own sessions to list.
                type_compose_line(hwnd, "/devices");
                state_mut(hwnd).model.connection = ConnectionState::Syncing;
                state_mut(hwnd).model.own_account =
                    Some(OwnAccount::matrix("@me:example".to_owned()));
                type_compose_line(hwnd, "/devices");
                type_compose_line(hwnd, "/devices Nobody");
                type_compose_line(hwnd, "/devices Bob");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![
                        Command::ListUserDevices("@me:example".to_owned()),
                        Command::ListUserDevices("@bob:example".to_owned()),
                    ]
                );
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "devices requested: @bob:example"
                );
                let device = |device_id: &str, verified, this_session| DeviceSummary {
                    device_id: device_id.to_owned(),
                    display_name: None,
                    verified,
                    this_session,
                    last_seen_ms: None,
                };
                for devices in [
                    UserDevices {
                        user_id: "@me:example".to_owned(),
                        identity: UserIdentityState::Verified,
                        devices: vec![device("MINE", true, true)],
                    },
                    UserDevices {
                        user_id: "@bob:example".to_owned(),
                        identity: UserIdentityState::Unverified,
                        devices: vec![device("B0B", false, false)],
                    },
                ] {
                    apply_core_event(state_mut(hwnd), Event::UserDevices(devices)).unwrap();
                }
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    &lines[lines.len() - 6..],
                    [
                        "[22:12] * /devices: not connected to server",
                        "[22:12] * /devices: no such nick: Nobody",
                        "[22:12] * @me:example sessions:",
                        "[22:12] * @me:example MINE verified (this session)",
                        "[22:12] * Bob is not verified; /verify @bob:example",
                        "[22:12] * Bob B0B unverified",
                    ]
                );
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn identity_accept_submits_only_for_a_changed_identity() {
        let (state, handle) = irc_command_state();
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                apply_core_event(
                    state_mut(hwnd),
                    Event::RoomIdentityChanges(RoomIdentityChanges {
                        room_id: "!ops:example".to_owned(),
                        changes: vec![IdentityChange {
                            user_id: "@bob:example".to_owned(),
                            trust: IdentityTrust::PinViolation,
                        }],
                    }),
                )
                .unwrap();
                for input in [
                    "/identity",
                    "/identity accept",
                    "/identity forget Bob",
                    "/identity accept Alice",
                    "/identity accept Nobody",
                    "/identity accept Bob",
                    "/devices Bob",
                ] {
                    type_compose_line(hwnd, input);
                }
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![
                        Command::AcceptUserIdentity("@bob:example".to_owned()),
                        Command::ListUserDevices("@bob:example".to_owned()),
                    ]
                );
                apply_core_event(
                    state_mut(hwnd),
                    Event::UserDevices(UserDevices {
                        user_id: "@bob:example".to_owned(),
                        identity: UserIdentityState::Unverified,
                        devices: Vec::new(),
                    }),
                )
                .unwrap();
                apply_core_event(
                    state_mut(hwnd),
                    Event::UserIdentityAccepted("@bob:example".to_owned()),
                )
                .unwrap();
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    &lines[lines.len() - 8..],
                    [
                        "[22:12] * Warning: Bob's identity was reset; /identity accept \
                         @bob:example to accept it, or /verify @bob:example",
                        "[22:12] * /identity: insufficient parameters",
                        "[22:12] * /identity: insufficient parameters",
                        "[22:12] * /identity: invalid parameters",
                        "[22:12] * /identity: Alice's identity has not changed",
                        "[22:12] * /identity: no such nick: Nobody",
                        "[22:12] * Bob changed identity; /identity accept @bob:example, or \
                         /verify @bob:example",
                        "[22:12] * Bob's new identity accepted",
                    ]
                );
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn ignore_and_unignore_check_locally_and_submit_core_commands() {
        let (mut state, handle) = irc_command_state();
        state.model.own_account = Some(OwnAccount::matrix("@me:example".to_owned()));
        state.model.ignored_users = Some(vec!["@spam:remote".to_owned()]);
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                for input in [
                    "/ignore alice",
                    "/ignore @me:example",
                    "/ignore @spam:remote",
                    "/unignore Bob",
                    "/unignore @spam:remote",
                    "/ignore",
                ] {
                    type_compose_line(hwnd, input);
                }
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![
                        Command::IgnoreUser("@alice:example".to_owned()),
                        Command::UnignoreUser("@spam:remote".to_owned()),
                    ]
                );
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "unignore submitted: @spam:remote"
                );
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    &lines[lines.len() - 5..],
                    [
                        "[22:12] * /ignore: you cannot ignore yourself",
                        "[22:12] * /ignore: @spam:remote is already ignored",
                        "[22:12] * /unignore: Bob is not ignored",
                        "[22:12] * Ignore list:",
                        "[22:12] *   @spam:remote",
                    ]
                );
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn notify_adds_updates_lists_and_removes_locally() {
        let (state, handle) = irc_command_state();
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                for input in [
                    "/notify",
                    "/notify dave",
                    "/notify -r @carol:remote",
                    "/notify bob lunch buddy",
                    "/notify @carol:remote",
                    "/notify +Bob",
                    "/notify -r @carol:remote",
                    "/notify -l",
                ] {
                    type_compose_line(hwnd, input);
                }
                assert_eq!(state_ref(hwnd).recorded_commands, vec![]);
                assert_eq!(state_ref(hwnd).model.status, "notify list saved");
                let mut list = NotifyList::default();
                list.add("@bob:example".to_owned(), None, true);
                assert_eq!(state_ref(hwnd).settings.settings().notify.list, list);
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    &lines[lines.len() - 9..],
                    [
                        "[22:12] * Notify list is empty",
                        "[22:12] * /notify: no such nick: dave",
                        "[22:12] * /notify: @carol:remote is not on the notify list",
                        "[22:12] * Added Bob (@bob:example) to notify list",
                        "[22:12] * Added @carol:remote to notify list",
                        "[22:12] * Updated +Bob (@bob:example) in notify list",
                        "[22:12] * Removed @carol:remote from notify list",
                        "[22:12] * Notify list:",
                        "[22:12] *   +Bob (@bob:example)",
                    ]
                );
            });
        }
        stop_test_core(handle);
    }

    /// `/cnick` edits the list, says what it did, and Alice's line in Ops
    /// takes each change at once; a notify entry follows `/notify`.
    #[test]
    fn cnick_colours_a_senders_nick_as_the_list_changes() {
        use super::super::colors::PaletteIndex;
        use super::super::nick_colors::{NickColorEntry, NickColorList, ResolvedNickColor};
        let (mut state, handle) = irc_command_state();
        let hello = sent_message_activity("!ops:example", "$hello", None, "hello", 0);
        apply_core_event(&mut state, Event::RoomActivity(Box::new(hello))).unwrap();
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                refresh_view_content(hwnd, &room_view_id("!ops:example")).unwrap();
                let alice_color = || {
                    let state = state_ref(hwnd);
                    let content = state.views[&state.active_view].content;
                    let rows = &timeline_state_ref(room_content_timeline(content)).source_rows;
                    let row = rows
                        .iter()
                        .find(|row| row.logical_text.ends_with("> hello"))
                        .expect("Alice's line is shown");
                    row.sender
                        .as_ref()
                        .expect("the line names its sender")
                        .color
                };
                let palette = |index| {
                    Some(ResolvedNickColor::Palette(
                        PaletteIndex::new(index).unwrap(),
                    ))
                };
                assert_eq!(alice_color(), None);
                for (input, color) in [
                    ("/cnick", None),
                    ("/cnick alice 4", palette(4)),
                    ("/cnick off", None),
                    ("/cnick on", palette(4)),
                    ("/cnick -y *:example 9", palette(4)),
                    ("/cnick -r 1", None),
                    ("/notify alice", palette(9)),
                    ("/cnick -r dave", palette(9)),
                    ("/cnick -r 3", palette(9)),
                    ("/cnick -a alice 12 ", palette(9)),
                    ("/cnick", palette(9)),
                    ("/notify -r alice", None),
                ] {
                    type_compose_line(hwnd, input);
                    assert_eq!(alice_color(), color, "after {input}");
                }
                assert_eq!(state_ref(hwnd).recorded_commands, vec![]);
                let entry = |line: &str| NickColorEntry::parse_ini_value(line).unwrap();
                let mut list = NickColorList::default();
                list.push(entry("*:example 9 notify"));
                list.push(entry("@alice:example 12 any"));
                assert_eq!(state_ref(hwnd).settings.settings().nick_colors.list, list);
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    &lines[lines.len() - 14..],
                    [
                        "[22:12] * Nick color list is empty (on)",
                        "[22:12] * Added Alice (@alice:example) to nick color list",
                        "[22:12] * Nick colors are off",
                        "[22:12] * Nick colors are on",
                        "[22:12] * Added *:example to nick color list",
                        "[22:12] * Removed Alice (@alice:example) from nick color list",
                        "[22:12] * Added Alice (@alice:example) to notify list",
                        "[22:12] * /cnick: no such nick: dave",
                        "[22:12] * /cnick: no nick color entry 3",
                        "[22:12] * Added Alice (@alice:example) to nick color list",
                        "[22:12] * Nick colors are on:",
                        "[22:12] *   1. *:example 9, notify",
                        "[22:12] *   2. Alice (@alice:example) 12, any rank",
                        "[22:12] * Removed Alice (@alice:example) from notify list",
                    ]
                );
            });
        }
        stop_test_core(handle);
    }

    /// Connected, a user added while the server says they are on is
    /// announced at once, and the listing says who is on.
    #[test]
    fn notify_announces_a_user_added_while_on() {
        let (mut state, handle) = irc_command_state();
        state.notify_watch.connected();
        state.model.presence.apply(vec![
            PresenceChange {
                user_id: "@bob:example".to_owned(),
                kind: PresenceKind::Unavailable,
                status_msg: None,
            },
            PresenceChange {
                user_id: "@alice:example".to_owned(),
                kind: PresenceKind::Offline,
                status_msg: None,
            },
        ]);
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                let status_lines = state_ref(hwnd).model.status_log.lines().len();
                for input in [
                    "/notify bob",
                    "/notify alice",
                    "/notify @carol:remote friend",
                    "/notify Bob lunch",
                    "/notify",
                ] {
                    type_compose_line(hwnd, input);
                }
                let status = &state_ref(hwnd).model.status_log.lines()[status_lines..];
                assert_eq!(
                    status
                        .iter()
                        .map(|line| (line.kind, line.text.as_str()))
                        .collect::<Vec<_>>(),
                    [(StatusLineKind::Notify, "* Bob is on Matrix")]
                );
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    &lines[lines.len() - 4..],
                    [
                        "[22:12] * Notify list:",
                        "[22:12] *   Bob (@bob:example) is on Matrix - lunch",
                        "[22:12] *   Alice (@alice:example) is not on Matrix",
                        "[22:12] *   @carol:remote - friend",
                    ]
                );
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn query_with_a_message_creates_the_query_and_says_the_message_waits() {
        let (state, handle) = irc_command_state();
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/query bob hello");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![Command::CreateDirectRoom(CreateDirectRoomRequest {
                        user_id: "@bob:example".to_owned(),
                    })]
                );
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "query submitted: @bob:example"
                );
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    lines.last().map(String::as_str),
                    Some("[22:12] * /query: creating a query with @bob:example; message not sent")
                );
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn moderation_commands_check_the_member_and_submit_core_commands() {
        let (mut state, handle) = irc_command_state();
        let ops = state.model.room_views.get_mut("!ops:example").unwrap();
        ops.members[0].can_kick = true;
        ops.members[0].can_ban = true;
        let mut me = member_summary("@me:example", "Me", false);
        me.rank = RoomMemberRank::Moderator;
        me.is_own_user = true;
        ops.members.push(me);
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                for input in [
                    "/kick alice spamming links",
                    "/kick Bob",
                    "/ban Alice",
                    "/kick",
                    "/kick Nobody",
                    "/unban @gone:example sorry",
                    "/invite Bob",
                    "/op Bob",
                    "/op alice",
                    "/voice Alice",
                    "/devoice Alice",
                    "/deop Bob",
                    "/deop Me",
                    "/nick New Name",
                ] {
                    type_compose_line(hwnd, input);
                }
                let room = || "!ops:example".to_owned();
                let power = |user_id: &str, role| member_roles_command(&room(), &[(user_id, role)]);
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![
                        Command::KickRoomMember(KickRoomMemberRequest {
                            room_id: room(),
                            user_id: "@alice:example".to_owned(),
                            reason: Some("spamming links".to_owned()),
                        }),
                        Command::BanRoomMember(BanRoomMemberRequest {
                            room_id: room(),
                            user_id: "@alice:example".to_owned(),
                            reason: None,
                        }),
                        Command::UnbanRoomMember(UnbanRoomMemberRequest {
                            room_id: room(),
                            user_id: "@gone:example".to_owned(),
                            reason: Some("sorry".to_owned()),
                        }),
                        power("@alice:example", MemberPowerRole::Moderator),
                        power("@alice:example", MemberPowerRole::Voiced),
                        power("@bob:example", MemberPowerRole::Default),
                        Command::SetDisplayName(SetDisplayNameRequest {
                            display_name: "New Name".to_owned(),
                        }),
                    ]
                );
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "nick change submitted: New Name"
                );
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    &lines[lines.len() - 7..],
                    [
                        "[22:12] * /kick: you cannot kick Bob",
                        "[22:12] * /kick: insufficient parameters",
                        "[22:12] * /kick: no such nick: Nobody",
                        "[22:12] * /invite: Bob is already in this room",
                        "[22:12] * /op: Bob already has moderator power",
                        "[22:12] * /devoice: Alice is not at the level to speak when the room is moderated",
                        "[22:12] * /deop: you cannot remove your own moderator power",
                    ]
                );
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn rank_commands_change_several_members_in_one_command_or_none() {
        let (mut state, handle) = irc_command_state();
        let ops = state.model.room_views.get_mut("!ops:example").unwrap();
        ops.members
            .push(member_summary("@carol:example", "Carol", false));
        let mut me = member_summary("@me:example", "Me", false);
        me.rank = RoomMemberRank::Moderator;
        me.is_own_user = true;
        ops.members.push(me);
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/op Alice  Carol");
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "power level change submitted: @alice:example, @carol:example"
                );
                for input in [
                    "/op Carol Bob",
                    "/op Carol @carol:example",
                    "/deop Bob Me",
                    "/voice Carol Nobody",
                ] {
                    type_compose_line(hwnd, input);
                }
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![member_roles_command(
                        "!ops:example",
                        &[
                            ("@alice:example", MemberPowerRole::Moderator),
                            ("@carol:example", MemberPowerRole::Moderator),
                        ]
                    )]
                );
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    &lines[lines.len() - 4..],
                    [
                        "[22:12] * /op: Bob already has moderator power",
                        "[22:12] * /op: Carol is named twice",
                        "[22:12] * /deop: you cannot remove your own moderator power",
                        "[22:12] * /voice: no such nick: Nobody",
                    ]
                );
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn close_in_status_is_refused_in_the_window() {
        let state = test_state();
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/close");
                assert_eq!(
                    project_timeline_rows(&build_status_timeline_rows(
                        &state_ref(hwnd).model.status_log,
                        state_ref(hwnd).timestamps()
                    )),
                    vec!["[22:12] * /close: the Status window cannot be closed".to_owned()]
                );
            });
        }
    }

    #[test]
    fn command_error_echoes_in_the_room_window_and_the_status_bar() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .model
            .room_views
            .insert("!room:example".to_owned(), RoomViewModel::default());
        state.active_view = room_view_id("!room:example");

        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/react");
                let lines = render_room_lines(state_ref(hwnd), "!room:example");
                assert_eq!(
                    lines.last().map(String::as_str),
                    Some("[22:12] * /react: insufficient parameters")
                );
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "* /react: insufficient parameters"
                );
                assert!(state_ref(hwnd).model.status_log.lines().is_empty());
                assert_eq!(state_ref(hwnd).room_activity, BTreeMap::new());
                assert_eq!(dump_compose_text(hwnd).unwrap(), "");
            });
        }
    }

    #[test]
    fn command_error_in_status_echoes_in_the_status_log() {
        let state = test_state();

        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/wat now");
                let rows = build_status_timeline_rows(
                    &state_ref(hwnd).model.status_log,
                    state_ref(hwnd).timestamps(),
                );
                assert_eq!(
                    project_timeline_rows(&rows),
                    vec!["[22:12] * /wat: unknown command".to_owned()]
                );
                assert_eq!(rows[0].kind, TimelineRowKind::Info);
                assert_eq!(state_ref(hwnd).model.status, "* /wat: unknown command");
            });
        }
    }

    #[test]
    fn help_prints_the_command_list_in_the_active_window() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .model
            .room_views
            .insert("!room:example".to_owned(), RoomViewModel::default());
        state.active_view = room_view_id("!room:example");

        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/help");
                type_compose_line(hwnd, "/help clear");
                type_compose_line(hwnd, "/help nope");
                let lines = render_room_lines(state_ref(hwnd), "!room:example");
                let echo = &lines[lines.len() - 4..];
                assert!(echo[0].starts_with("[22:12] * Commands: /accept /alias /ame /amsg /away /back /ban /channel /clear /clearall /close /cnick "));
                assert_eq!(echo[1], "[22:12] * /help <command> for usage");
                assert_eq!(echo[2], "[22:12] * /clear - clear this window");
                assert_eq!(echo[3], "[22:12] * /help: no such command: nope");
            });
        }
    }

    #[test]
    fn timestamp_command_restamps_every_window() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .model
            .room_views
            .insert("!room:example".to_owned(), RoomViewModel::default());
        state.active_view = room_view_id("!room:example");

        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/timestamp");
                type_compose_line(hwnd, "/timestamp off");
                assert_eq!(state_ref(hwnd).model.status, "timestamp: off");
                type_compose_line(hwnd, "/timestamp -f [HH:nn:ss]");
                assert_eq!(state_ref(hwnd).model.status, "timestamp format: [HH:nn:ss]");
                type_compose_line(hwnd, "/timestamp");
                type_compose_line(hwnd, "/timestamp on");
                let lines = render_room_lines(state_ref(hwnd), "!room:example");
                assert_eq!(
                    &lines[lines.len() - 2..],
                    [
                        "[22:12:00] * Timestamp is on ([HH:nn])",
                        "[22:12:00] * Timestamp is off ([HH:nn:ss])",
                    ]
                );
                let timeline = room_content_timeline(
                    state_ref(hwnd).views[&state_ref(hwnd).active_view].content,
                );
                assert_eq!(
                    timeline_state_ref(timeline)
                        .source_rows
                        .last()
                        .unwrap()
                        .logical_text,
                    "[22:12:00] * Timestamp is off ([HH:nn:ss])"
                );
            });
        }
    }

    #[test]
    fn resolve_direct_room_query_target_distinguishes_none_single_and_ambiguous() {
        let mut single_room = room("!single:example", "Single", false);
        single_room.is_direct = true;
        single_room.direct_user_ids = vec!["@alice:example.org".to_owned()];

        let mut ambiguous_one = room("!ambiguous-one:example", "Ambiguous One", false);
        ambiguous_one.is_direct = true;
        ambiguous_one.direct_user_ids = vec!["@carol:example.org".to_owned()];

        let mut ambiguous_two = room("!ambiguous-two:example", "Ambiguous Two", false);
        ambiguous_two.is_direct = true;
        ambiguous_two.direct_user_ids = vec![
            "@carol:example.org".to_owned(),
            "@other:example.org".to_owned(),
        ];

        let model = UiModel {
            rooms: vec![
                room("!plain:example", "Plain", false),
                single_room,
                ambiguous_one,
                ambiguous_two,
            ],
            ..Default::default()
        };

        assert_eq!(
            resolve_direct_room_query_target(&model, "@missing:example.org"),
            QueryTargetResolution::None
        );
        assert_eq!(
            resolve_direct_room_query_target(&model, "@alice:example.org"),
            QueryTargetResolution::Single("!single:example".to_owned())
        );
        assert_eq!(
            resolve_direct_room_query_target(&model, "@carol:example.org"),
            QueryTargetResolution::Ambiguous
        );
    }

    #[test]
    fn resolve_invite_command_room_id_rejects_ambiguous_joined_unknown_and_missing_active_invites()
    {
        let mut state = test_state();
        state.model.rooms = vec![
            invited_room("!invite-one:example", "Shared"),
            invited_room("!invite-two:example", "Shared"),
            invited_room("!invite-unique:example", "Unique"),
            room("!joined:example", "Joined", false),
        ];

        assert_eq!(
            resolve_invite_command_room_id(&state, &state.active_view, "accept", Some("Shared")),
            Err("accept failed: ambiguous invited room target: Shared".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(
                &state,
                &state.active_view,
                "accept",
                Some("!joined:example")
            ),
            Err("accept failed: room is not invited: !joined:example".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, &state.active_view, "accept", Some("Joined")),
            Err("accept failed: room is not invited: Joined".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, &state.active_view, "accept", Some("Missing")),
            Err("accept failed: invited room not found: Missing".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, &state.active_view, "accept", None),
            Err("accept failed: no invited room selected".to_owned())
        );

        state.active_view = room_view_id("!joined:example");
        assert_eq!(
            resolve_invite_command_room_id(&state, &state.active_view, "reject", None),
            Err("reject failed: no invited room selected".to_owned())
        );
    }

    /// A real frame with the joined rooms Ops (`#ops:example.org`, also
    /// `#operations:example.org`) and Dev, and an invite to Lobby; Dev is
    /// active, and the sender's commands wait.
    unsafe fn with_room_link_frame(test: impl FnOnce(HWND)) {
        with_test_frame(600, 400, |hwnd| {
            let (sender, commands) = inert_command_sender();
            std::mem::forget(commands);
            state_mut(hwnd).command_sender = Some(sender);
            let mut ops = room("!ops:example.org", "Ops", false);
            ops.canonical_alias = Some("#ops:example.org".to_owned());
            ops.alt_aliases = vec!["#operations:example.org".to_owned()];
            state_mut(hwnd).model.rooms = vec![
                ops,
                room("!dev:example.org", "Dev", false),
                invited_room("!lobby:example.org", "Lobby"),
            ];
            sync_views_and_tree(hwnd).unwrap();
            activate_view(hwnd, &room_view_id("!dev:example.org"), None).unwrap();
            test(hwnd);
        });
    }

    fn joins(state: &WindowState) -> Vec<&Command> {
        state
            .recorded_commands
            .iter()
            .filter(|command| matches!(command, Command::JoinRoom(_)))
            .collect()
    }

    #[test]
    fn a_room_link_activates_a_window_the_user_has_by_id_or_alias() {
        unsafe {
            with_room_link_frame(|hwnd| {
                for (target, room_id) in [
                    ("!ops:example.org", "!ops:example.org"),
                    ("#ops:example.org", "!ops:example.org"),
                    ("#operations:example.org", "!ops:example.org"),
                    ("!lobby:example.org", "!lobby:example.org"),
                    ("!dev:example.org", "!dev:example.org"),
                ] {
                    assert_eq!(
                        open_room_link(hwnd, target.to_owned(), vec!["x.org".to_owned()]),
                        Ok(format!("window active: {target}"))
                    );
                    assert_eq!(state_ref(hwnd).active_view, room_view_id(room_id));
                }
                assert!(joins(state_ref(hwnd)).is_empty());
            });
        }
    }

    #[test]
    fn a_room_link_joins_any_other_room_through_its_servers() {
        unsafe {
            with_room_link_frame(|hwnd| {
                // A display name is not an alias.
                assert_eq!(
                    open_room_link(hwnd, "Ops".to_owned(), Vec::new()),
                    Ok("join submitted: Ops".to_owned())
                );
                assert_eq!(
                    open_room_link(
                        hwnd,
                        "#rust:matrix.org".to_owned(),
                        vec!["matrix.org".to_owned()]
                    ),
                    Ok("join submitted: #rust:matrix.org".to_owned())
                );
                assert_eq!(
                    joins(state_ref(hwnd)),
                    vec![
                        &Command::JoinRoom(JoinRoomRequest {
                            room_or_alias: "Ops".to_owned(),
                            via: Vec::new(),
                        }),
                        &Command::JoinRoom(JoinRoomRequest {
                            room_or_alias: "#rust:matrix.org".to_owned(),
                            via: vec!["matrix.org".to_owned()],
                        }),
                    ]
                );
                assert_eq!(
                    state_ref(hwnd).active_view,
                    room_view_id("!dev:example.org")
                );
            });
        }
    }

    #[test]
    fn typed_join_of_a_joined_room_activates_its_window() {
        unsafe {
            with_room_link_frame(|hwnd| {
                run_local_command(
                    hwnd,
                    &state_ref(hwnd).active_view.clone(),
                    SlashCommand::Join {
                        target: "#ops:example.org".to_owned(),
                    },
                )
                .unwrap();
                assert_eq!(
                    state_ref(hwnd).active_view,
                    room_view_id("!ops:example.org")
                );
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "window active: #ops:example.org"
                );
                assert!(joins(state_ref(hwnd)).is_empty());
            });
        }
    }

    #[test]
    fn submit_join_slash_command_records_typed_join_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();

        let status =
            submit_join_room(&mut state, "#ops:example.org".to_owned(), Vec::new()).unwrap();

        assert_eq!(status, "join submitted: #ops:example.org");
        assert_eq!(
            state.recorded_commands,
            vec![Command::JoinRoom(JoinRoomRequest {
                room_or_alias: "#ops:example.org".to_owned(),
                via: Vec::new(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_create_room_records_a_private_or_public_room_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.connection = ConnectionState::Syncing;
        state.model.own_account = Some(OwnAccount::matrix("@me:example.org".to_owned()));

        let private = CreateRoomArgs::Private {
            name: "Team room".to_owned(),
        };
        let public = |server: Option<&str>, name: Option<&str>| CreateRoomArgs::Public {
            alias_localpart: "foo".to_owned(),
            alias_server: server.map(str::to_owned),
            name: name.map(str::to_owned),
        };
        assert_eq!(
            submit_create_room(&mut state, private),
            Ok("create submitted: Team room".to_owned())
        );
        assert_eq!(
            submit_create_room(&mut state, public(None, None)),
            Ok("create submitted: foo".to_owned())
        );
        assert_eq!(
            submit_create_room(&mut state, public(Some("example.org"), Some("Foo Bar"))),
            Ok("create submitted: Foo Bar".to_owned())
        );
        assert_eq!(
            submit_create_room(&mut state, public(Some("other.example"), None)),
            Err("the alias must be on your server (example.org)".to_owned())
        );

        let public_request = |name: &str| {
            Command::CreateRoom(CreateRoomRequest {
                name: name.to_owned(),
                visibility: CreateRoomVisibility::Public {
                    alias_localpart: "foo".to_owned(),
                },
            })
        };
        assert_eq!(
            state.recorded_commands,
            vec![
                Command::CreateRoom(CreateRoomRequest {
                    name: "Team room".to_owned(),
                    visibility: CreateRoomVisibility::Private,
                }),
                public_request("foo"),
                public_request("Foo Bar"),
            ]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_create_room_while_signed_out_records_nothing() {
        let mut state = test_state();

        assert_eq!(
            submit_create_room(
                &mut state,
                CreateRoomArgs::Private {
                    name: "Team room".to_owned()
                }
            ),
            Err("not connected to server".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_accept_slash_command_records_typed_accept_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![invited_room("!invite:example", "Invite")];
        state.active_view = room_view_id("!invite:example");
        let window = state.active_view.clone();

        let status = submit_accept_slash_command(&mut state, &window, None).unwrap();

        assert_eq!(status, "accept submitted: !invite:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::AcceptRoomInvite(AcceptRoomInviteRequest {
                room_id: "!invite:example".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_membership_commands,
            vec![DumpSubmittedMembershipCommand {
                action: DumpMembershipCommandAction::AcceptInvite,
                target: "!invite:example".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_reject_slash_command_records_typed_reject_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![invited_room("!invite:example", "Invite")];
        state.active_view = room_view_id("!invite:example");
        let window = state.active_view.clone();

        let status = submit_reject_slash_command(&mut state, &window, None).unwrap();

        assert_eq!(status, "reject submitted: !invite:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::RejectRoomInvite(RejectRoomInviteRequest {
                room_id: "!invite:example".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_membership_commands,
            vec![DumpSubmittedMembershipCommand {
                action: DumpMembershipCommandAction::RejectInvite,
                target: "!invite:example".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_query_slash_command_records_typed_create_direct_room_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();

        let status =
            submit_query_slash_command(&mut state, "@alice:example.org".to_owned()).unwrap();

        assert_eq!(status, "query submitted: @alice:example.org");
        assert_eq!(
            state.recorded_commands,
            vec![Command::CreateDirectRoom(CreateDirectRoomRequest {
                user_id: "@alice:example.org".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_part_slash_command_from_active_room_records_typed_leave_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");
        let window = state.active_view.clone();

        let status = submit_part_slash_command(&mut state, &window, None).unwrap();

        assert_eq!(status, "part submitted: !room:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::LeaveRoom(LeaveRoomRequest {
                room_id: "!room:example".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_part_slash_command_from_thread_targets_parent_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = thread_view_id("!room:example", "$thread-root");
        let window = state.active_view.clone();

        let status = submit_part_slash_command(&mut state, &window, None).unwrap();

        assert_eq!(status, "part submitted: !room:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::LeaveRoom(LeaveRoomRequest {
                room_id: "!room:example".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_room_notification_mode_records_the_command_and_names_the_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");
        let window = state.active_view.clone();

        assert_eq!(
            submit_room_notification_mode(&mut state, &window, None, RoomNotificationMode::Mute),
            Ok("notifications for Room: Mute".to_owned())
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::SetRoomNotificationMode(
                SetRoomNotificationModeRequest {
                    room_id: "!room:example".to_owned(),
                    mode: RoomNotificationMode::Mute,
                }
            )]
        );
        // The check follows the server, not the click.
        assert_eq!(
            state.model.rooms[0].notification_mode,
            Some(RoomNotificationMode::AllMessages)
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_part_slash_command_from_status_returns_no_room_selected() {
        let mut state = test_state();
        let window = state.active_view.clone();

        assert_eq!(
            submit_part_slash_command(&mut state, &window, None),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_part_slash_command_resolves_explicit_room_target_like_window() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![
            room("!alpha:example", "Alpha", false),
            room("!beta:example", "Beta", false),
        ];
        let window = state.active_view.clone();

        let status = submit_part_slash_command(&mut state, &window, Some("Beta")).unwrap();

        assert_eq!(status, "part submitted: !beta:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::LeaveRoom(LeaveRoomRequest {
                room_id: "!beta:example".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_part_slash_command_rejects_ambiguous_display_name_without_recording() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!shared-one:example", "Shared", false),
            room("!shared-two:example", "Shared", false),
        ];
        let window = state.active_view.clone();

        assert_eq!(
            submit_part_slash_command(&mut state, &window, Some("Shared")),
            Err("ambiguous room: Shared".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_react_slash_command_from_active_room_records_typed_request_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");
        let window = state.active_view.clone();

        let status = submit_react_slash_command(
            &mut state,
            &window,
            "$react-normal".to_owned(),
            "+1".to_owned(),
        )
        .unwrap();

        assert_eq!(status, "Reaction queued for $react-normal");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SendReaction(SendReactionRequest {
                room_id: "!room:example".to_owned(),
                target_event_id: "$react-normal".to_owned(),
                key: "+1".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_reaction_commands,
            vec![DumpSubmittedReactionCommand {
                room_id: "!room:example".to_owned(),
                target_event_id: "$react-normal".to_owned(),
                key: "+1".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_react_slash_command_from_thread_targets_parent_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = thread_view_id("!room:example", "$thread-root");
        let window = state.active_view.clone();

        let status = submit_react_slash_command(
            &mut state,
            &window,
            "$react-normal".to_owned(),
            "two words".to_owned(),
        )
        .unwrap();

        assert_eq!(status, "Reaction queued for $react-normal");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SendReaction(SendReactionRequest {
                room_id: "!room:example".to_owned(),
                target_event_id: "$react-normal".to_owned(),
                key: "two words".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_react_slash_command_from_status_returns_no_room_selected() {
        let mut state = test_state();
        let window = state.active_view.clone();

        assert_eq!(
            submit_react_slash_command(
                &mut state,
                &window,
                "$react-normal".to_owned(),
                "+1".to_owned(),
            ),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn send_reply_from_the_active_room_names_the_replied_to_sender() {
        let mut state = compose_target_state();
        let window = state.active_view.clone();

        send_reply(
            &mut state,
            &window,
            "$q",
            RoomMessagePresentationKind::Text,
            "hello there",
        )
        .unwrap();

        assert_eq!(
            state.recorded_commands,
            vec![reply_command(
                RoomMessagePresentationKind::Text,
                "hello there"
            )]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_reply_commands,
            vec![DumpSubmittedReplyCommand {
                room_id: "!ops:example".to_owned(),
                target_event_id: "$q".to_owned(),
                sender_user_id: "@alice:example".to_owned(),
                body: "hello there".to_owned(),
                styles: Vec::new(),
                thread: None,
            }]
        );
    }

    #[test]
    fn send_reply_from_a_thread_uses_the_compose_thread_destination() {
        let mut state = test_state();
        let (sender, _commands) = inert_command_sender();
        state.command_sender = Some(sender);
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root:example",
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
                "$thread-1:example",
                None,
                Some(thread_relation("$thread-root:example")),
                RoomMessagePresentationKind::Text,
                "durable reply",
                1_700_000_120_000,
            ),
        )
        .unwrap();
        state.active_view = thread_view_id("!ops:example", "$thread-root:example");
        state.room_activity.insert(
            thread_view_id("!ops:example", "$thread-root:example"),
            ViewActivityLevel::Message,
        );
        let window = state.active_view.clone();

        send_reply(
            &mut state,
            &window,
            "$thread-1:example",
            RoomMessagePresentationKind::Text,
            "threaded acknowledgement",
        )
        .unwrap();

        assert_eq!(
            state.recorded_commands,
            vec![Command::SendMessage(SendMessageRequest {
                room_id: "!ops:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "threaded acknowledgement".to_owned(),
                styles: Vec::new(),
                thread: Some(SendMessageThreadTarget {
                    thread_root_event_id: "$thread-root:example".to_owned(),
                    latest_event_id: "$thread-1:example".to_owned(),
                }),
                reply: Some(SendMessageReplyTarget {
                    event_id: "$thread-1:example".to_owned(),
                    sender_user_id: "@alice:example".to_owned(),
                }),
            })]
        );
        assert!(
            !state
                .room_activity
                .contains_key(&thread_view_id("!ops:example", "$thread-root:example"))
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_reply_commands,
            vec![DumpSubmittedReplyCommand {
                room_id: "!ops:example".to_owned(),
                target_event_id: "$thread-1:example".to_owned(),
                sender_user_id: "@alice:example".to_owned(),
                body: "threaded acknowledgement".to_owned(),
                styles: Vec::new(),
                thread: Some(DumpSubmittedReplyThreadTarget {
                    thread_root_event_id: "$thread-root:example".to_owned(),
                    latest_event_id: "$thread-1:example".to_owned(),
                }),
            }]
        );
    }

    #[test]
    fn send_reply_and_send_edit_need_the_message_in_the_active_window() {
        let mut state = compose_target_state();
        let window = state.active_view.clone();
        assert_eq!(
            send_reply(
                &mut state,
                &window,
                "$elsewhere",
                RoomMessagePresentationKind::Text,
                "hello"
            ),
            Err("no such message in this window".to_owned())
        );
        assert_eq!(
            send_edit(
                &mut state,
                &window,
                "$elsewhere",
                RoomMessagePresentationKind::Text,
                "hello"
            ),
            Err("no such message in this window".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn send_reply_and_send_edit_require_an_available_command_sender() {
        let mut state = compose_target_state();
        state.command_sender = None;
        let window = state.active_view.clone();
        for result in [
            send_reply(
                &mut state,
                &window,
                "$q",
                RoomMessagePresentationKind::Text,
                "hello",
            ),
            send_edit(
                &mut state,
                &window,
                "$mine",
                RoomMessagePresentationKind::Text,
                "hello",
            ),
        ] {
            assert_eq!(
                result,
                Err("core command sender is not available".to_owned())
            );
        }
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn send_edit_from_the_active_room_records_the_typed_request() {
        let mut state = compose_target_state();
        let window = state.active_view.clone();

        send_edit(
            &mut state,
            &window,
            "$mine",
            RoomMessagePresentationKind::Text,
            "replacement text",
        )
        .unwrap();

        assert_eq!(
            state.recorded_commands,
            vec![Command::EditMessage(EditMessageRequest {
                room_id: "!ops:example".to_owned(),
                target_event_id: "$mine".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "replacement text".to_owned(),
                styles: Vec::new(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_edit_commands,
            vec![DumpSubmittedEditCommand {
                room_id: "!ops:example".to_owned(),
                target_event_id: "$mine".to_owned(),
                body: "replacement text".to_owned(),
                styles: Vec::new(),
            }]
        );
    }

    #[test]
    fn send_edit_from_a_thread_targets_the_parent_room() {
        let mut state = compose_target_state();
        let mut in_thread = sent_message_activity_with_relation_and_presentation(
            "!ops:example",
            "$mine-in-thread",
            None,
            Some(thread_relation("$q")),
            RoomMessagePresentationKind::Text,
            "thread draft",
            1_700_000_180_000,
        );
        let RoomActivityKind::Message(message) = &mut in_thread.kind else {
            unreachable!()
        };
        message.sender = user_profile("@me:example");
        apply_core_event(&mut state, Event::RoomActivity(Box::new(in_thread))).unwrap();
        state.active_view = thread_view_id("!ops:example", "$q");
        let window = state.active_view.clone();

        send_edit(
            &mut state,
            &window,
            "$mine-in-thread",
            RoomMessagePresentationKind::Text,
            "thread final",
        )
        .unwrap();

        assert_eq!(
            state.recorded_commands,
            vec![Command::EditMessage(EditMessageRequest {
                room_id: "!ops:example".to_owned(),
                target_event_id: "$mine-in-thread".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "thread final".to_owned(),
                styles: Vec::new(),
            })]
        );
    }

    #[test]
    fn submit_redact_slash_command_from_active_room_records_typed_request_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");
        let window = state.active_view.clone();

        let status =
            submit_redact_slash_command(&mut state, &window, "$event-to-redact".to_owned())
                .unwrap();

        assert_eq!(status, "Redaction queued for $event-to-redact");
        assert_eq!(
            state.recorded_commands,
            vec![Command::RedactEvent(RedactEventRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$event-to-redact".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_redaction_commands,
            vec![DumpSubmittedRedactionCommand {
                room_id: "!room:example".to_owned(),
                event_id: "$event-to-redact".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_redact_slash_command_from_thread_targets_parent_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = thread_view_id("!room:example", "$thread-root");
        let window = state.active_view.clone();

        let status =
            submit_redact_slash_command(&mut state, &window, "$event-to-redact".to_owned())
                .unwrap();

        assert_eq!(status, "Redaction queued for $event-to-redact");
        assert_eq!(
            state.recorded_commands,
            vec![Command::RedactEvent(RedactEventRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$event-to-redact".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_unreact_slash_command_from_active_room_records_typed_request_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");
        let window = state.active_view.clone();

        let status = submit_unreact_slash_command(
            &mut state,
            &window,
            "$reaction-alice-plus-one".to_owned(),
        )
        .unwrap();

        assert_eq!(
            status,
            "Reaction redaction queued for $reaction-alice-plus-one"
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::RedactEvent(RedactEventRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$reaction-alice-plus-one".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_redaction_commands,
            vec![DumpSubmittedRedactionCommand {
                room_id: "!room:example".to_owned(),
                event_id: "$reaction-alice-plus-one".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_unreact_slash_command_from_thread_targets_parent_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = thread_view_id("!room:example", "$thread-root");
        let window = state.active_view.clone();

        let status = submit_unreact_slash_command(
            &mut state,
            &window,
            "$reaction-alice-plus-one".to_owned(),
        )
        .unwrap();

        assert_eq!(
            status,
            "Reaction redaction queued for $reaction-alice-plus-one"
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::RedactEvent(RedactEventRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$reaction-alice-plus-one".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_unreact_slash_command_from_status_returns_no_room_selected() {
        let mut state = test_state();
        let window = state.active_view.clone();

        assert_eq!(
            submit_unreact_slash_command(
                &mut state,
                &window,
                "$reaction-alice-plus-one".to_owned()
            ),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_topic_slash_command_from_active_room_records_typed_request_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");
        let window = state.active_view.clone();

        let status =
            submit_topic_slash_command(&mut state, &window, None, "incident review".to_owned())
                .unwrap();

        assert_eq!(status, "topic change submitted for !room:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SetRoomTopic(SetRoomTopicRequest {
                room_id: "!room:example".to_owned(),
                topic: "incident review".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_room_topic_commands,
            vec![DumpSubmittedRoomTopicCommand {
                room_id: "!room:example".to_owned(),
                topic: "incident review".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_topic_slash_command_from_thread_targets_parent_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = thread_view_id("!room:example", "$thread-root");
        let window = state.active_view.clone();

        let status =
            submit_topic_slash_command(&mut state, &window, None, "incident review".to_owned())
                .unwrap();

        assert_eq!(status, "topic change submitted for !room:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SetRoomTopic(SetRoomTopicRequest {
                room_id: "!room:example".to_owned(),
                topic: "incident review".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_topic_slash_command_from_status_returns_no_room_selected() {
        let mut state = test_state();
        let window = state.active_view.clone();

        assert_eq!(
            submit_topic_slash_command(&mut state, &window, None, "incident review".to_owned()),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    fn aliased_rooms() -> Vec<RoomSummary> {
        let mut ops = room("!ops:example", "Ops", false);
        ops.canonical_alias = Some("#ops:example".to_owned());
        ops.topic = Some("incident review".to_owned());
        let mut lobby = room("!lobby:example", "Lobby", false);
        lobby.canonical_alias = Some("#lobby:example".to_owned());
        lobby.alt_aliases = vec!["#hall:example".to_owned()];
        let mut invite = invited_room("!invite:example", "Invite");
        invite.canonical_alias = Some("#party:example".to_owned());
        vec![ops, lobby, invite]
    }

    #[test]
    fn submit_topic_slash_command_names_the_room_by_alias() {
        let (mut state, handle) = test_state_with_command_sender();
        apply_core_event(&mut state, Event::SyncedRooms(aliased_rooms())).unwrap();
        state.active_view = room_view_id("!ops:example");
        let window = state.active_view.clone();

        let status =
            submit_topic_slash_command(&mut state, &window, Some("#hall"), "new".to_owned())
                .unwrap();

        assert_eq!(status, "topic change submitted for !lobby:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SetRoomTopic(SetRoomTopicRequest {
                room_id: "!lobby:example".to_owned(),
                topic: "new".to_owned(),
            })]
        );
        assert_eq!(
            submit_topic_slash_command(&mut state, &window, Some("#nope"), "x".to_owned()),
            Err("unknown room: #nope".to_owned())
        );
        assert_eq!(
            submit_topic_slash_command(&mut state, &window, Some("#party"), "x".to_owned()),
            Err("room is not joined: #party".to_owned())
        );

        stop_test_core(handle);
    }

    #[test]
    fn room_topic_line_shows_the_active_or_named_rooms_topic() {
        let mut state = test_state();
        apply_core_event(&mut state, Event::SyncedRooms(aliased_rooms())).unwrap();

        assert_eq!(
            room_topic_line(&state, &state.active_view, None),
            Err("no room selected".to_owned())
        );
        state.active_view = room_view_id("!ops:example");
        assert_eq!(
            room_topic_line(&state, &state.active_view, None),
            Ok("* Topic is 'incident review'".to_owned())
        );
        assert_eq!(
            room_topic_line(&state, &state.active_view, Some("#lobby:example")),
            Ok("* No topic is set".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn invite_and_msg_targets_accept_aliases() {
        let mut state = test_state();
        apply_core_event(&mut state, Event::SyncedRooms(aliased_rooms())).unwrap();

        assert_eq!(
            resolve_invite_command_room_id(&state, &state.active_view, "accept", Some("#party")),
            Ok("!invite:example".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(
                &state,
                &state.active_view,
                "accept",
                Some("#ops:example")
            ),
            Err("accept failed: room is not invited: #ops:example".to_owned())
        );
        assert_eq!(
            msg_target_room_id(&state.model, "#lobby"),
            Ok("!lobby:example".to_owned())
        );
    }

    #[test]
    fn submit_topic_slash_command_does_not_mutate_local_topic_or_activity_before_inbound_event() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");
        apply_core_event(&mut state, topic_state("!room:example", "existing topic")).unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!room:example",
                "$event:example",
                None,
                "hello",
                1_700_000_000_000,
            ),
        )
        .unwrap();

        let before_activities = state
            .model
            .room_views
            .get("!room:example")
            .expect("room view must exist")
            .activities
            .clone();
        let window = state.active_view.clone();

        let status =
            submit_topic_slash_command(&mut state, &window, None, "incident review".to_owned())
                .unwrap();

        assert_eq!(status, "topic change submitted for !room:example");
        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .and_then(|view| view.topic.as_deref()),
            Some("existing topic")
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|view| view.activities.clone()),
            Some(before_activities)
        );
        assert_eq!(
            render_room_lines(&state, "!room:example"),
            vec![
                "* Now talking in Room (!room:example)".to_owned(),
                "* Topic is 'existing topic'".to_owned(),
                "[22:14] * @alice:example changes topic to 'existing topic'".to_owned(),
                "[22:13] <@alice:example> hello".to_owned(),
            ]
        );

        stop_test_core(handle);
    }

    #[test]
    fn bar_lines_name_the_bar() {
        assert_eq!(bar_is_line(Bar::Switchbar, true), "* Switchbar is on");
        assert_eq!(bar_is_line(Bar::Treebar, false), "* Treebar is off");
    }

    /// The channels Alpha and Beta, Beta active; Bob is in both (and can
    /// be kicked), Carol only in Beta.
    unsafe fn with_two_channels_frame(test: impl FnOnce(HWND)) {
        with_test_frame(600, 400, |hwnd| {
            let (sender, commands) = inert_command_sender();
            std::mem::forget(commands);
            let state = state_mut(hwnd);
            state.command_sender = Some(sender);
            state.model.own_account = Some(OwnAccount::matrix("@me:example".to_owned()));
            state.model.rooms = vec![
                room("!alpha:example", "Alpha", false),
                room("!beta:example", "Beta", false),
            ];
            let bob = RoomMemberSummary {
                can_kick: true,
                ..member_summary("@bob:example", "Bob", false)
            };
            for (room_id, topic, members) in [
                ("!alpha:example", "alpha topic", vec![bob.clone()]),
                (
                    "!beta:example",
                    "beta topic",
                    vec![bob, member_summary("@carol:example", "Carol", false)],
                ),
            ] {
                state.model.room_views.insert(
                    room_id.to_owned(),
                    RoomViewModel {
                        topic: Some(topic.to_owned()),
                        members,
                        ..Default::default()
                    },
                );
            }
            sync_views_and_tree(hwnd).unwrap();
            activate_view(hwnd, &room_view_id("!beta:example"), None).unwrap();
            test(hwnd);
        });
    }

    /// The rooms the recorded commands act on, in order.
    fn command_rooms(state: &WindowState) -> Vec<&str> {
        state
            .recorded_commands
            .iter()
            .map(|command| match command {
                Command::InviteRoomMember(request) => request.room_id.as_str(),
                Command::KickRoomMember(request) => request.room_id.as_str(),
                Command::UpdateRoomSettings(request) => request.room_id.as_str(),
                command => panic!("unexpected command {command:?}"),
            })
            .collect()
    }

    #[test]
    fn commands_act_in_the_window_they_were_given_in() {
        unsafe {
            with_two_channels_frame(|hwnd| {
                let alpha = room_view_id("!alpha:example");
                let beta_lines = render_room_lines(state_ref(hwnd), "!beta:example");
                let run = |window: &ViewId, line: &str| {
                    let command = commands::parse(line).unwrap();
                    run_local_command(hwnd, window, command).unwrap();
                };
                run(&alpha, "/invite Carol");
                run(&alpha, "/kick Bob");
                run(&alpha, "/op Bob");
                assert_eq!(
                    command_rooms(state_ref(hwnd)),
                    ["!alpha:example", "!alpha:example", "!alpha:example"]
                );
                let alpha_before = render_room_lines(state_ref(hwnd), "!alpha:example");
                run(&alpha, "/topic");
                run(&alpha, "/kick Nobody");
                let alpha_lines = render_room_lines(state_ref(hwnd), "!alpha:example");
                assert_eq!(alpha_lines.len(), alpha_before.len() + 2, "{alpha_lines:?}");
                assert!(alpha_lines[alpha_before.len()].ends_with("* Topic is 'alpha topic'"));
                assert!(
                    alpha_lines[alpha_before.len() + 1].ends_with("* /kick: no such nick: Nobody")
                );
                assert_eq!(
                    render_room_lines(state_ref(hwnd), "!beta:example"),
                    beta_lines
                );
                run(&alpha, "/clear");
                let alpha_lines = render_room_lines(state_ref(hwnd), "!alpha:example");
                assert!(
                    !alpha_lines.iter().any(|line| line.ends_with("Nobody")),
                    "{alpha_lines:?}"
                );

                let status_before = status_log_lines(state_ref(hwnd)).len();
                run(&ViewId::Status, "/topic");
                run(&ViewId::Status, "/names");
                let status = status_log_lines(state_ref(hwnd));
                assert_eq!(status.len(), status_before + 2, "{status:?}");
                assert!(status[status_before].ends_with("* /topic: no room selected"));
                assert!(status[status_before + 1].ends_with("* /names: no room selected"));
                assert_eq!(
                    render_room_lines(state_ref(hwnd), "!beta:example"),
                    beta_lines
                );
                assert_eq!(state_ref(hwnd).active_view, room_view_id("!beta:example"));
            });
        }
    }
}
