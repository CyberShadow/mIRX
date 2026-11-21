//! Running parsed slash commands; the parser is `commands.rs`.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
enum QueryTargetResolution {
    None,
    Single(String),
    Ambiguous,
}

/// Runs a command and reports how it went: its status, or its error in the
/// active window.
pub(super) unsafe fn run_local_command(hwnd: HWND, command: SlashCommand) -> Result<(), String> {
    match execute_slash_command(hwnd, command) {
        Ok(Some(status)) => set_local_command_status(hwnd, status),
        Ok(None) => Ok(()),
        Err(error) => report_command_error(hwnd, error),
    }
}

/// Runs a typed command. `Ok(Some(status))` is feedback for the status bar;
/// `Ok(None)` means the command reported (or needs) nothing.
unsafe fn execute_slash_command(
    hwnd: HWND,
    command: SlashCommand,
) -> Result<Option<String>, CommandError> {
    let name = command.name();
    run_slash_command(hwnd, command).map_err(|reason| CommandError::new(name, reason))
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

pub(super) unsafe fn run_slash_command(
    hwnd: HWND,
    command: SlashCommand,
) -> Result<Option<String>, String> {
    match command {
        SlashCommand::Status => {
            activate_view(hwnd, &ViewId::Status, None)?;
            Ok(Some("status window active".to_owned()))
        }
        SlashCommand::Find { query } => {
            execute_active_view_find(hwnd, &query)?;
            Ok(None)
        }
        SlashCommand::Window { target } => {
            let room_id =
                match resolve_room(&state_ref(hwnd).model.rooms, is_navigable_room, &target) {
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
                            room_id,
                            RoomMessagePresentationKind::Text,
                            &input,
                        )?;
                    }
                    Ok(Some(format!("query opened: {user_id}")))
                }
                QueryTargetResolution::Ambiguous => {
                    Err(format!("ambiguous query target: {user_id}"))
                }
                QueryTargetResolution::None => {
                    let status = submit_query_slash_command(state_mut(hwnd), user_id.clone())?;
                    if message.is_some() {
                        echo_in_active_view(
                            hwnd,
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
            let sent = send_room_message(state_mut(hwnd), room_id, presentation, &body)?;
            echo_in_active_view(hwnd, [commands::msg_echo(presentation, &target, &sent)])?;
            Ok(None)
        }
        SlashCommand::Names { target } => {
            let lines = {
                let state = state_ref(hwnd);
                let room_id = resolve_joined_room_target(state, target.as_deref())?;
                let room = room_by_id(&state.model.rooms, &room_id)
                    .expect("a joined room target must exist");
                commands::names_lines(&room.display_name, room_members(&state.model, &room_id))
            };
            echo_in_active_view(hwnd, lines)?;
            Ok(None)
        }
        SlashCommand::Whois { nick } => {
            let state = state_mut(hwnd);
            let user_id = commands::resolve_user(all_room_members(&state.model), &nick)?.user_id;
            enqueue_command(state, Command::FetchUserProfile(user_id.clone()))?;
            Ok(Some(format!("whois requested: {user_id}")))
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
            let auth = match &state_ref(hwnd).model.own_account {
                Some(account) => account.auth.clone(),
                None => return Err("not connected to server".to_owned()),
            };
            let password = match auth {
                // The account page confirms it, on its own.
                SessionAuth::OAuth { .. } => None,
                SessionAuth::Matrix => match show_sign_out_session_dialog(hwnd, &device_id)? {
                    Some(form) => form.password,
                    None => return Ok(None),
                },
            };
            let status = format!("session sign-out submitted: {device_id}");
            submit_device_sign_out(state_mut(hwnd), device_id, password)?;
            Ok(Some(status))
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
            echo_in_active_view(hwnd, lines)?;
            Ok(None)
        }
        SlashCommand::Ignore {
            target: Some(target),
        } => {
            let state = state_mut(hwnd);
            let user_id = ignore_target(&state.model, &target)?;
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
            let user_id = ignore_target(&state.model, &target)?;
            ignore::check_unignore(state.model.ignored_users.as_deref(), &user_id, |user_id| {
                known_nick(&state.model, user_id)
            })?;
            enqueue_command(state, Command::UnignoreUser(user_id.clone()))?;
            Ok(Some(format!("unignore submitted: {user_id}")))
        }
        SlashCommand::Close => {
            let state = state_ref(hwnd);
            if close_action(&state.model, &state.active_view) == CloseAction::Refuse {
                return Err("the Status window cannot be closed".to_owned());
            }
            // Posted: closing may ask first, and destroys this editbox.
            let child = state.views[&state.active_view].child;
            if PostMessageW(child, WM_CLOSE, 0, 0) == 0 {
                return Err(last_os_error("PostMessageW WM_CLOSE"));
            }
            Ok(None)
        }
        SlashCommand::Kick { nick, reason } => {
            let state = state_mut(hwnd);
            let (room_id, member) = room_member_target(state, &nick)?;
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
            let (room_id, member) = room_member_target(state, &nick)?;
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
            let room_id = resolve_joined_room_target(state, None)?;
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
            let room_id = resolve_joined_room_target(state, room.as_deref())?;
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
        SlashCommand::SetRank { nick, change } => {
            let state = state_mut(hwnd);
            let (room_id, member) = room_member_target(state, &nick)?;
            let role = commands::rank_change_role(change, member)?;
            let user_id = member.user_id.clone();
            enqueue_command(
                state,
                Command::SetRoomMemberPowerLevel(SetRoomMemberPowerLevelRequest {
                    room_id,
                    user_id: user_id.clone(),
                    role,
                }),
            )?;
            Ok(Some(format!("power level change submitted: {user_id}")))
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
            submit_accept_slash_command(state_mut(hwnd), target.as_deref()).map(Some)
        }
        SlashCommand::Reject { target } => {
            submit_reject_slash_command(state_mut(hwnd), target.as_deref()).map(Some)
        }
        SlashCommand::Part { target } => {
            submit_part_slash_command(state_mut(hwnd), target.as_deref()).map(Some)
        }
        SlashCommand::React {
            target_event_id,
            key,
        } => submit_react_slash_command(state_mut(hwnd), target_event_id, key).map(Some),
        SlashCommand::Reply {
            target_event_id,
            body: None,
        } => {
            arm_compose_target(
                hwnd,
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
                &target_event_id,
                RoomMessagePresentationKind::Text,
                &body,
            )?;
            Ok(Some(format!("Reply queued for {target_event_id}")))
        }
        SlashCommand::SendFile { path } => {
            let room_id = send_file_room_id(state_ref(hwnd))?;
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
                let activity = window_message(&state.model, &state.active_view, &target_event_id)
                    .ok_or_else(|| "no such message in this window".to_owned())?;
                replies::editable_message(activity, state.model.own_user_id())?.presentation
            };
            send_edit(state_mut(hwnd), &target_event_id, presentation, &body)?;
            Ok(Some(format!("Edit queued for {target_event_id}")))
        }
        SlashCommand::Redact { event_id } => {
            submit_redact_slash_command(state_mut(hwnd), event_id).map(Some)
        }
        SlashCommand::Unreact { reaction_event_id } => {
            submit_unreact_slash_command(state_mut(hwnd), reaction_event_id).map(Some)
        }
        SlashCommand::Topic {
            room,
            topic: Some(topic),
        } => submit_topic_slash_command(state_mut(hwnd), room.as_deref(), topic).map(Some),
        SlashCommand::Topic { room, topic: None } => {
            let line = room_topic_line(state_ref(hwnd), room.as_deref())?;
            echo_in_active_view(hwnd, [line])?;
            Ok(None)
        }
        SlashCommand::Clear => {
            let view_id = state_ref(hwnd).active_view.clone();
            clear_window(hwnd, &view_id).map(Some)
        }
        SlashCommand::SaveBuf { path } => {
            let view_id = state_ref(hwnd).active_view.clone();
            match path {
                Some(path) => save_window_buffer(hwnd, &view_id, Path::new(&path)).map(Some),
                None => run_save_buffer_dialog(hwnd, &view_id),
            }
        }
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
            echo_in_active_view(hwnd, commands::help_lines(topic.as_deref())?)?;
            Ok(None)
        }
        SlashCommand::Log(command) => {
            let (target, on) = match command {
                LogCommand::Show => (None, None),
                LogCommand::On { target } => (target, Some(true)),
                LogCommand::Off { target } => (target, Some(false)),
            };
            let view_id = log_command_view(state_ref(hwnd), target.as_deref())?;
            let line = match on {
                None => logging_is_line(state_ref(hwnd), &view_id),
                Some(on) => set_view_logging(state_mut(hwnd), &view_id, on)?,
            };
            echo_in_active_view(hwnd, [line])?;
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
                echo_in_active_view(hwnd, [line])?;
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
                    echo_in_active_view(hwnd, [bar_is_line(bar, shown)])?;
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

/// The window `/log` acts on: the active one (a thread window's room), or
/// the room `target` names, whose window must be open.
fn log_command_view(state: &WindowState, target: Option<&str>) -> Result<ViewId, String> {
    let Some(target) = target else {
        return log_view(&state.active_view);
    };
    let view_id = match resolve_room(&state.model.rooms, is_navigable_room, target) {
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

/// A typed command's error, printed in the window it was typed in; the
/// status bar says the same.
pub(super) unsafe fn report_command_error(hwnd: HWND, error: CommandError) -> Result<(), String> {
    report_error_line(hwnd, error.to_string())
}

/// An error line, `* ...`, printed in the active window as a typed
/// command's errors are; the status bar says the same.
pub(super) unsafe fn report_error_line(hwnd: HWND, text: String) -> Result<(), String> {
    echo_in_active_view(hwnd, [text.clone()])?;
    set_local_command_status(hwnd, text)
}

/// The window a command's lines print in: the active one. The Rooms List
/// has no text of its own: what a Commands menu item run there prints goes
/// to Status.
fn echo_view(active_view: &ViewId) -> ViewId {
    match active_view {
        ViewId::RoomList => ViewId::Status,
        view => view.clone(),
    }
}

/// Prints a command's lines in the active window. They are its newest lines,
/// in the window the user is looking at, so they raise no activity. The
/// caller refreshes the window.
pub(super) fn echo_lines_in_active_view(
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
    let now = state.clock.now_ms();
    for text in lines {
        match view {
            ViewId::Status => state.model.status_log.push(now, StatusLineKind::Info, text),
            ViewId::RoomList => unreachable!("Rooms List output goes to Status"),
            ViewId::Room(room_id) => state
                .model
                .room_views
                .entry_or_default(room_id.clone())
                .push_echo(now, None, StatusLineKind::Info, text),
            ViewId::Thread {
                room_id,
                thread_root_event_id,
            } => state
                .model
                .room_views
                .get_mut(room_id)
                .expect("a thread window's room must have a view")
                .push_echo(
                    now,
                    Some(thread_root_event_id.clone()),
                    StatusLineKind::Info,
                    text,
                ),
        }
    }
}

/// Prints a command's lines in the active window, and shows them.
pub(super) unsafe fn echo_in_active_view(
    hwnd: HWND,
    lines: impl IntoIterator<Item = String>,
) -> Result<(), String> {
    echo_lines_in_active_view(state_mut(hwnd), lines);
    let view = echo_view(&state_ref(hwnd).active_view);
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

/// The joined room a member command acts on, and the member the nick names
/// there.
fn room_member_target<'s>(
    state: &'s WindowState,
    nick: &str,
) -> Result<(String, &'s RoomMemberSummary), String> {
    let room_id = resolve_joined_room_target(state, None)?;
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

/// The nick a member of a known room goes by, if any room knows them with
/// a display name.
pub(super) fn known_nick(model: &UiModel, user_id: &str) -> Option<String> {
    all_room_members(model)
        .filter(|member| member.user_id == user_id)
        .find_map(|member| member.display_name.clone())
}

/// The user `/ignore` and `/unignore` name: a user ID, which need not be a
/// member of any known room, or a known member's nick. Ignored users stay
/// nameable by ID once they leave every room.
fn ignore_target(model: &UiModel, target: &str) -> Result<String, String> {
    Ok(commands::resolve_user(all_room_members(model), target)?.user_id)
}

/// Where `/msg` sends: a room by ID, alias or display name, else the query
/// with a user named by ID or nick. Like mIRC's `/msg`, it does not open a
/// query.
fn msg_target_room_id(model: &UiModel, target: &str) -> Result<String, String> {
    match resolve_room(&model.rooms, is_navigable_room, target) {
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

pub(super) fn resolve_joined_room_target(
    state: &WindowState,
    target: Option<&str>,
) -> Result<String, String> {
    let Some(target) = target else {
        return match &state.active_view {
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

    match resolve_room(&state.model.rooms, is_navigable_room, target) {
        Ok(room) if is_sendable_room(room) => Ok(room.room_id.clone()),
        Ok(_) => Err(format!("room is not joined: {target}")),
        Err(RoomTargetError::Unknown) => Err(format!("unknown room: {target}")),
        Err(RoomTargetError::Ambiguous) => Err(format!("ambiguous room: {target}")),
    }
}

pub(super) fn resolve_invite_command_room_id(
    state: &WindowState,
    action: &str,
    target: Option<&str>,
) -> Result<String, String> {
    let Some(target) = target else {
        return match &state.active_view {
            ViewId::Room(room_id)
                if room_by_id(&state.model.rooms, room_id)
                    .is_some_and(is_invite_command_target) =>
            {
                Ok(room_id.clone())
            }
            _ => Err(format!("{action} failed: no invited room selected")),
        };
    };

    let rooms = &state.model.rooms;
    match resolve_room(rooms, is_invite_command_target, target) {
        Ok(room) => Ok(room.room_id.clone()),
        Err(RoomTargetError::Ambiguous) => Err(format!(
            "{action} failed: ambiguous invited room target: {target}"
        )),
        Err(RoomTargetError::Unknown) => {
            match resolve_room(rooms, is_joined_message_room, target) {
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
    let room_id = state_ref(hwnd)
        .model
        .rooms
        .iter()
        .find(|room| is_navigable_room(room) && room_answers_to(room, &target))
        .map(|room| room.room_id.clone());
    match room_id {
        Some(room_id) => {
            activate_view(hwnd, &ViewId::Room(room_id), None)?;
            Ok(format!("window active: {target}"))
        }
        None => submit_join_room(state_mut(hwnd), target, via),
    }
}

/// Whether `target` is `room`'s ID or one of its published aliases, exactly:
/// display names, which `/window` also takes, name no room to join.
fn room_answers_to(room: &RoomSummary, target: &str) -> bool {
    room.room_id == target
        || room.canonical_alias.as_deref() == Some(target)
        || room.alt_aliases.iter().any(|alias| alias == target)
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
    target: Option<&str>,
) -> Result<String, String> {
    let room_id = resolve_invite_command_room_id(state, "accept", target)?;
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
    target: Option<&str>,
) -> Result<String, String> {
    let room_id = resolve_invite_command_room_id(state, "reject", target)?;
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
    target: Option<&str>,
) -> Result<String, String> {
    let room_id = resolve_joined_room_target(state, target)?;
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
    target: Option<&str>,
    mode: RoomNotificationMode,
) -> Result<String, String> {
    let room_id = resolve_joined_room_target(state, target)?;
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
    target_event_id: String,
    key: String,
) -> Result<String, String> {
    let room_id = resolve_joined_room_target(state, None)?;
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

/// Replaces the user's message `target_event_id` in the active window with
/// `presentation` and `input`; sends nothing when that is what it says.
pub(super) fn send_edit(
    state: &mut WindowState,
    target_event_id: &str,
    presentation: RoomMessagePresentationKind,
    input: &str,
) -> Result<(), String> {
    let message = replies::editable_message(
        window_message(&state.model, &state.active_view, target_event_id)
            .ok_or_else(|| "no such message in this window".to_owned())?,
        state.model.own_user_id(),
    )?;
    let (body, styles) = message_text_from_input(input)?;
    if message.presentation == presentation && message.body == body && message.styles == styles {
        return Ok(());
    }
    let room_id = resolve_joined_room_target(state, None)?;
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

/// Sends a reply to the message `target_event_id` in the active window, in
/// its thread when the window is one.
pub(super) fn send_reply(
    state: &mut WindowState,
    target_event_id: &str,
    presentation: RoomMessagePresentationKind,
    input: &str,
) -> Result<(), String> {
    let (body, styles) = message_text_from_input(input)?;
    let destination = resolve_compose_destination(state, None)?;
    let sender_user_id = room_activity_sender_user_id(
        window_message(&state.model, &state.active_view, target_event_id)
            .ok_or_else(|| "no such message in this window".to_owned())?,
    )
    .expect("a message that is not redacted has a sender")
    .to_owned();
    let clears_active_thread_marker = destination.thread.is_some();
    enqueue_send_message(
        state,
        SendMessageRequest {
            room_id: destination.room_id,
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
    if clears_active_thread_marker {
        state.room_activity.remove(&state.active_view);
    }
    Ok(())
}

fn submit_redact_event_command(state: &mut WindowState, event_id: String) -> Result<(), String> {
    let room_id = resolve_joined_room_target(state, None)?;
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
    event_id: String,
) -> Result<String, String> {
    submit_redact_event_command(state, event_id.clone())?;
    Ok(format!("Redaction queued for {event_id}"))
}

fn submit_unreact_slash_command(
    state: &mut WindowState,
    reaction_event_id: String,
) -> Result<String, String> {
    submit_redact_event_command(state, reaction_event_id.clone())?;
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
fn room_topic_line(state: &WindowState, room: Option<&str>) -> Result<String, String> {
    let room_id = resolve_joined_room_target(state, room)?;
    Ok(commands::topic_line(
        state.model.room_views[&room_id].topic.as_deref(),
    ))
}

fn submit_topic_slash_command(
    state: &mut WindowState,
    room: Option<&str>,
    topic: String,
) -> Result<String, String> {
    let room_id = resolve_joined_room_target(state, room)?;
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
        DeviceSummary, IdentityChange, IdentityTrust, RoomIdentityChanges, RoomMemberRank,
        UserIdentityState,
    };

    use super::super::test_fixtures::*;
    use super::*;

    /// A channel Ops with Alice and Bob, and a query with Alice; Ops active.
    fn irc_command_state() -> (WindowState, CoreHandle) {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![
            room("!ops:example", "Ops", false),
            query("!dm:example", "Alice", "@alice:example"),
        ];
        let mut bob = member_summary("@bob:example", Some("Bob"), false);
        bob.rank = RoomMemberRank::Moderator;
        let ops = RoomViewModel {
            members: vec![member_summary("@alice:example", Some("Alice"), false), bob],
            ..Default::default()
        };
        state
            .model
            .room_views
            .insert("!ops:example".to_owned(), ops);
        let dm = RoomViewModel {
            members: vec![member_summary("@alice:example", Some("Alice"), false)],
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
                    &lines[lines.len() - 11..],
                    [
                        "[22:12] * Names Ops: Alice @Bob",
                        "[22:12] * End of /NAMES list",
                        "[22:12] * /names: unknown room: Nowhere",
                        "[22:12] * /whois: no such nick: Nobody",
                        "[22:12] * Alice is @alice:example * Alice Liddell",
                        "[22:12] * Alice on Ops Alice",
                        "[22:12] * Alice using example",
                        "[22:12] * Alice End of /WHOIS list",
                        "[22:12] * @stranger:remote is @stranger:remote",
                        "[22:12] * @stranger:remote using remote",
                        "[22:12] * @stranger:remote End of /WHOIS list",
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
                    &lines[lines.len() - 8..],
                    [
                        "[22:12] * /devices: not connected to server",
                        "[22:12] * /devices: no such nick: Nobody",
                        "[22:12] * @me:example sessions:",
                        "[22:12] * @me:example MINE verified (this session)",
                        "[22:12] * @me:example End of /DEVICES list",
                        "[22:12] * Bob is not verified; /verify @bob:example",
                        "[22:12] * Bob B0B unverified",
                        "[22:12] * Bob End of /DEVICES list",
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
                    &lines[lines.len() - 9..],
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
                        "[22:12] * Bob End of /DEVICES list",
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
        let mut me = member_summary("@me:example", Some("Me"), false);
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
                let power = |user_id: &str, role| {
                    Command::SetRoomMemberPowerLevel(SetRoomMemberPowerLevelRequest {
                        room_id: room(),
                        user_id: user_id.to_owned(),
                        role,
                    })
                };
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
                        "[22:12] * /op: Bob is already an operator",
                        "[22:12] * /devoice: Alice is not voiced",
                        "[22:12] * /deop: you cannot deop yourself",
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
                assert!(echo[0].starts_with("[22:12] * Commands: /accept /ban /clear /close "));
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
            resolve_invite_command_room_id(&state, "accept", Some("Shared")),
            Err("accept failed: ambiguous invited room target: Shared".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", Some("!joined:example")),
            Err("accept failed: room is not invited: !joined:example".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", Some("Joined")),
            Err("accept failed: room is not invited: Joined".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", Some("Missing")),
            Err("accept failed: invited room not found: Missing".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", None),
            Err("accept failed: no invited room selected".to_owned())
        );

        state.active_view = room_view_id("!joined:example");
        assert_eq!(
            resolve_invite_command_room_id(&state, "reject", None),
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

        let status = submit_accept_slash_command(&mut state, None).unwrap();

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

        let status = submit_reject_slash_command(&mut state, None).unwrap();

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

        let status = submit_part_slash_command(&mut state, None).unwrap();

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

        let status = submit_part_slash_command(&mut state, None).unwrap();

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

        assert_eq!(
            submit_room_notification_mode(&mut state, None, RoomNotificationMode::Mute),
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

        assert_eq!(
            submit_part_slash_command(&mut state, None),
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

        let status = submit_part_slash_command(&mut state, Some("Beta")).unwrap();

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

        assert_eq!(
            submit_part_slash_command(&mut state, Some("Shared")),
            Err("ambiguous room: Shared".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_react_slash_command_from_active_room_records_typed_request_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");

        let status =
            submit_react_slash_command(&mut state, "$react-normal".to_owned(), "+1".to_owned())
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

        let status = submit_react_slash_command(
            &mut state,
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

        assert_eq!(
            submit_react_slash_command(&mut state, "$react-normal".to_owned(), "+1".to_owned(),),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn send_reply_from_the_active_room_names_the_replied_to_sender() {
        let mut state = compose_target_state();

        send_reply(
            &mut state,
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

        send_reply(
            &mut state,
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
        assert_eq!(
            send_reply(
                &mut state,
                "$elsewhere",
                RoomMessagePresentationKind::Text,
                "hello"
            ),
            Err("no such message in this window".to_owned())
        );
        assert_eq!(
            send_edit(
                &mut state,
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
        for result in [
            send_reply(&mut state, "$q", RoomMessagePresentationKind::Text, "hello"),
            send_edit(
                &mut state,
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

        send_edit(
            &mut state,
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

        send_edit(
            &mut state,
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

        let status =
            submit_redact_slash_command(&mut state, "$event-to-redact".to_owned()).unwrap();

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

        let status =
            submit_redact_slash_command(&mut state, "$event-to-redact".to_owned()).unwrap();

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

        let status =
            submit_unreact_slash_command(&mut state, "$reaction-alice-plus-one".to_owned())
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

        let status =
            submit_unreact_slash_command(&mut state, "$reaction-alice-plus-one".to_owned())
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

        assert_eq!(
            submit_unreact_slash_command(&mut state, "$reaction-alice-plus-one".to_owned()),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_topic_slash_command_from_active_room_records_typed_request_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");

        let status =
            submit_topic_slash_command(&mut state, None, "incident review".to_owned()).unwrap();

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

        let status =
            submit_topic_slash_command(&mut state, None, "incident review".to_owned()).unwrap();

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

        assert_eq!(
            submit_topic_slash_command(&mut state, None, "incident review".to_owned()),
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

        let status =
            submit_topic_slash_command(&mut state, Some("#hall"), "new".to_owned()).unwrap();

        assert_eq!(status, "topic change submitted for !lobby:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SetRoomTopic(SetRoomTopicRequest {
                room_id: "!lobby:example".to_owned(),
                topic: "new".to_owned(),
            })]
        );
        assert_eq!(
            submit_topic_slash_command(&mut state, Some("#nope"), "x".to_owned()),
            Err("unknown room: #nope".to_owned())
        );
        assert_eq!(
            submit_topic_slash_command(&mut state, Some("#party"), "x".to_owned()),
            Err("room is not joined: #party".to_owned())
        );

        stop_test_core(handle);
    }

    #[test]
    fn room_topic_line_shows_the_active_or_named_rooms_topic() {
        let mut state = test_state();
        apply_core_event(&mut state, Event::SyncedRooms(aliased_rooms())).unwrap();

        assert_eq!(
            room_topic_line(&state, None),
            Err("no room selected".to_owned())
        );
        state.active_view = room_view_id("!ops:example");
        assert_eq!(
            room_topic_line(&state, None),
            Ok("* Topic is 'incident review'".to_owned())
        );
        assert_eq!(
            room_topic_line(&state, Some("#lobby:example")),
            Ok("* No topic is set".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn invite_and_msg_targets_accept_aliases() {
        let mut state = test_state();
        apply_core_event(&mut state, Event::SyncedRooms(aliased_rooms())).unwrap();

        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", Some("#party")),
            Ok("!invite:example".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", Some("#ops:example")),
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

        let status =
            submit_topic_slash_command(&mut state, None, "incident review".to_owned()).unwrap();

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
}
