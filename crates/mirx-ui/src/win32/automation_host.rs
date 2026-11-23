//! The automation transport: the WM_COPYDATA request, its dispatch to the
//! UI, and the shared-memory response.

use super::*;

const AUTOMATION_RESPONSE_CAPACITY: usize = 4 * 1024 * 1024;

pub(super) struct AutomationState {
    response_name: String,
    response_mapping: HANDLE,
    response_view: *mut u8,
    response_capacity: usize,
}

pub(super) unsafe fn enable_automation(hwnd: HWND) -> Result<(), String> {
    let response_name = automation_response_name(hwnd);
    let response_name_wide = wide_null(&response_name);
    let mapping = CreateFileMappingW(
        INVALID_HANDLE_VALUE,
        null(),
        PAGE_READWRITE,
        (AUTOMATION_RESPONSE_CAPACITY >> 32) as u32,
        (AUTOMATION_RESPONSE_CAPACITY & u32::MAX as usize) as u32,
        response_name_wide.as_ptr(),
    );
    if mapping.is_null() {
        return Err(last_os_error("CreateFileMappingW"));
    }

    let view = MapViewOfFile(
        mapping,
        FILE_MAP_ALL_ACCESS,
        0,
        0,
        AUTOMATION_RESPONSE_CAPACITY,
    );
    if view.Value.is_null() {
        CloseHandle(mapping);
        return Err(last_os_error("MapViewOfFile"));
    }

    // No ChangeWindowMessageFilterEx: the driver and any user-automation client run
    // at the same integrity level as the app (same user/session), so UIPI never
    // blocks their WM_COPYDATA. Adding the filter would only let lower-integrity
    // processes reach the automation surface.
    (view.Value as *mut u32).write_unaligned(0);
    state_mut(hwnd).automation = Some(AutomationState {
        response_name,
        response_mapping: mapping,
        response_view: view.Value.cast(),
        response_capacity: AUTOMATION_RESPONSE_CAPACITY,
    });
    Ok(())
}

fn automation_response_name(hwnd: HWND) -> String {
    format!(
        "Local\\mirx-ui-automation-response-{}-{:x}",
        unsafe { GetCurrentProcessId() },
        hwnd as usize
    )
}

pub(super) fn destroy_automation_state(state: &mut WindowState) {
    let Some(automation) = state.automation.take() else {
        return;
    };

    unsafe {
        if !automation.response_view.is_null() {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: automation.response_view.cast(),
            });
        }
        if !automation.response_mapping.is_null() {
            CloseHandle(automation.response_mapping);
        }
    }
}

pub(super) unsafe fn handle_copydata(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    if state_ref(hwnd).automation.is_none() {
        return 0;
    }

    let (response_id, envelope) = if lparam == 0 {
        (
            None,
            ResponseEnvelope::<Value>::Failure(failure_response(
                None,
                "invalid_request",
                "COPYDATASTRUCT pointer was null",
            )),
        )
    } else {
        let request = &*(lparam as *const COPYDATASTRUCT);
        match copy_copydata_request(request) {
            Ok((command_id, request_bytes)) => {
                match parse_command_request(command_id, &request_bytes) {
                    Ok(command) => {
                        let response_id = Some(command_request_id(&command).to_owned());
                        (response_id, dispatch_command(hwnd, command))
                    }
                    Err(error) => {
                        let response_id = error.id.clone();
                        (response_id, ResponseEnvelope::Failure(error))
                    }
                }
            }
            Err(error) => {
                let response_id = error.id.clone();
                (response_id, ResponseEnvelope::Failure(error))
            }
        }
    };

    let response_bytes = match serialize_response_envelope(&envelope, response_id.clone()) {
        Ok(bytes) => bytes,
        Err(error) => match serialize_failure_envelope(ResponseEnvelope::<Value>::Failure(error)) {
            Some(bytes) => bytes,
            None => return 0,
        },
    };

    if write_response_bytes(hwnd, response_id.as_deref(), &response_bytes) {
        1
    } else {
        0
    }
}

fn copy_copydata_request(request: &COPYDATASTRUCT) -> Result<(u32, Vec<u8>), FailureEnvelope> {
    let command_id = u32::try_from(request.dwData).map_err(|_| {
        failure_response(
            None,
            "invalid_request",
            format!("command id {} exceeds u32 range", request.dwData),
        )
    })?;

    if request.cbData == 0 {
        return Ok((command_id, Vec::new()));
    }

    if request.lpData.is_null() {
        return Err(failure_response(
            None,
            "invalid_request",
            "COPYDATASTRUCT data pointer was null",
        ));
    }

    let request_len = request.cbData as usize;
    let request_bytes =
        unsafe { std::slice::from_raw_parts(request.lpData.cast::<u8>(), request_len) }.to_vec();
    Ok((command_id, request_bytes))
}

unsafe fn dispatch_command(hwnd: HWND, command: ParsedCommand) -> ResponseEnvelope<Value> {
    let response_id = command_request_id(&command).to_owned();
    match execute_command(hwnd, command) {
        Ok(json) => success_response(response_id, json),
        Err(error) => ResponseEnvelope::Failure(error),
    }
}

/// What WAIT_IDLE looks at, recording the event generation for the next
/// probe.
unsafe fn probe_idle(hwnd: HWND) -> IdleProbe {
    // Before the queue's status is read: the inert drain may post a
    // script's resume.
    let pending_core_commands = pending_core_commands(hwnd);
    let state = state_mut(hwnd);
    let pending_core_events = state
        .event_queue
        .lock()
        .expect("core event queue poisoned")
        .len() as u64;
    let previous_generation = state
        .last_idle_probe_generation
        .replace(state.event_generation);
    IdleProbe {
        event_generation: state.event_generation,
        pending_core_events,
        pending_core_commands,
        // The probe runs inside the driver's sent WM_COPYDATA, which the
        // thread handles before any posted message: whatever the driver
        // (or a handler) posted before asking still waits in the queue.
        posted_messages_pending: GetQueueStatus(QS_ALLPOSTMESSAGE) >> 16 != 0,
        synthetic_load_in_progress: state.synthetic_load_in_progress,
        previous_generation,
    }
}

/// The commands sent to the core that it has not acked, or whose ack the
/// UI has not applied yet.
unsafe fn pending_core_commands(hwnd: HWND) -> u64 {
    let state = state_mut(hwnd);
    // The inert core of a render-state run answers nothing: its commands
    // are handled once dropped, and a script waiting for them goes on.
    if let Some(receiver) = &mut state.inert_command_receiver {
        while let Ok((id, _)) = receiver.try_recv() {
            state.core_answers.answer(id);
        }
        script::post_resume_if_ready(hwnd);
    }
    let state = state_ref(hwnd);
    let sent = state.command_sender.as_ref().map_or(0, CommandSender::sent);
    let answered = state.core_answers.count();
    sent.checked_sub(answered)
        .unwrap_or_else(|| panic!("the core acked {answered} commands, more than the {sent} sent"))
}

unsafe fn execute_command(hwnd: HWND, command: ParsedCommand) -> Result<Value, FailureEnvelope> {
    match command {
        ParsedCommand::Ping { id } => serialize_success_json(&id, PongResponse { pong: true }),
        ParsedCommand::GetVersion { id } => {
            serialize_success_json(&id, GetVersionResponse::current())
        }
        ParsedCommand::WaitIdle { id, args } => {
            let _ = args.timeout_ms;
            drain_core_events_and_refresh(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "state_unavailable", message)
            })?;
            serialize_success_json(&id, WaitIdleResponse::from_probe(probe_idle(hwnd)))
        }
        ParsedCommand::DumpModel { id } => {
            let dump = dump_model_from_state(state_ref(hwnd));
            serialize_success_json(&id, dump)
        }
        ParsedCommand::DumpUiState { id } => {
            drain_core_events_and_refresh(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "ui_state_unavailable", message)
            })?;
            sync_views_and_tree(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "ui_state_unavailable", message)
            })?;
            let dump = dump_ui_state(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "ui_state_unavailable", message)
            })?;
            serialize_success_json(&id, dump)
        }
        ParsedCommand::DumpVerificationMenuState { id } => {
            let state = dump_verification_menu_state(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "verification_menu_unavailable", message)
            })?;
            serialize_success_json(&id, state)
        }
        ParsedCommand::DumpRoomMenuState { id } => {
            let state = dump_room_menu_state(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "room_menu_unavailable", message)
            })?;
            serialize_success_json(&id, state)
        }
        ParsedCommand::DumpMessageMenuState { id } => {
            let state = dump_message_menu_state(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "message_menu_unavailable", message)
            })?;
            serialize_success_json(&id, state)
        }
        ParsedCommand::DumpMemberMenuState { id } => {
            let state = dump_member_menu_state(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "member_menu_unavailable", message)
            })?;
            serialize_success_json(&id, state)
        }
        ParsedCommand::SelectMember { id, args } => {
            select_member_for_automation(hwnd, &args.user_id).map_err(|message| {
                failure_response(Some(id.clone()), "selected_member_unavailable", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::QuerySelectedMemberViaMenu { id } => {
            invoke_menu_command_for_automation(hwnd, MENU_ID_MEMBER_QUERY).map_err(|message| {
                failure_response(Some(id.clone()), "selected_member_unavailable", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::MentionSelectedMemberViaMenu { id } => {
            invoke_menu_command_for_automation(hwnd, MENU_ID_MEMBER_MENTION).map_err(
                |message| {
                    failure_response(Some(id.clone()), "selected_member_unavailable", message)
                },
            )?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::OpenMemberContextMenu { id, args } => {
            invoke_member_context_menu_for_automation(hwnd, &args.user_id, &args.command).map_err(
                |message| failure_response(Some(id.clone()), "member_menu_action_failed", message),
            )?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::OpenSelectedMemberContextMenu { id, args } => {
            invoke_selected_member_context_menu_for_automation(hwnd, &args.command).map_err(
                |message| failure_response(Some(id.clone()), "member_menu_action_failed", message),
            )?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::SelectTimelineEvent { id, args } => {
            select_timeline_event_for_automation(hwnd, &args.event_id).map_err(|message| {
                let code = if message.starts_with("unknown_event:") {
                    "unknown_event"
                } else {
                    "selected_message_unavailable"
                };
                failure_response(Some(id.clone()), code, message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::AcceptInviteViaRoomMenu { id } => {
            invoke_menu_command_for_automation(hwnd, MENU_ID_ROOM_ACCEPT_INVITE).map_err(
                |message| failure_response(Some(id.clone()), "room_menu_action_failed", message),
            )?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::RejectInviteViaRoomMenu { id } => {
            invoke_menu_command_for_automation(hwnd, MENU_ID_ROOM_REJECT_INVITE).map_err(
                |message| failure_response(Some(id.clone()), "room_menu_action_failed", message),
            )?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::PartRoomViaRoomMenu { id } => {
            invoke_menu_command_for_automation(hwnd, MENU_ID_ROOM_PART).map_err(|message| {
                failure_response(Some(id.clone()), "room_menu_action_failed", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::ReplyToSelectedMessageViaMenu { id } => {
            invoke_menu_command_for_automation(hwnd, MENU_ID_MESSAGE_REPLY).map_err(|message| {
                failure_response(Some(id.clone()), "selected_message_unavailable", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::EditSelectedMessageViaMenu { id } => {
            invoke_menu_command_for_automation(hwnd, MENU_ID_MESSAGE_EDIT).map_err(|message| {
                failure_response(Some(id.clone()), "selected_message_unavailable", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::ReactToSelectedMessageViaMenu { id } => {
            invoke_menu_command_for_automation(hwnd, MENU_ID_MESSAGE_REACT).map_err(|message| {
                failure_response(Some(id.clone()), "selected_message_unavailable", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::RedactSelectedMessageViaMenu { id } => {
            invoke_menu_command_for_automation(hwnd, MENU_ID_MESSAGE_REDACT).map_err(
                |message| {
                    failure_response(Some(id.clone()), "selected_message_unavailable", message)
                },
            )?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::SaveAttachmentViaMessageMenu { id } => {
            invoke_menu_command_for_automation(hwnd, MENU_ID_MESSAGE_SAVE_ATTACHMENT).map_err(
                |message| {
                    failure_response(Some(id.clone()), "selected_message_unavailable", message)
                },
            )?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::OpenNavigationContextMenu { id, args } => {
            select_navigation_path(hwnd, &args.path).map_err(|message| {
                failure_response(Some(id.clone()), "room_context_menu_unavailable", message)
            })?;
            invoke_navigation_context_menu_for_automation(hwnd, &args.command).map_err(
                |message| failure_response(Some(id.clone()), "room_menu_action_failed", message),
            )?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::OpenSelectedNavigationContextMenu { id, args } => {
            let tree = state_ref(hwnd).controls.tree;
            let item = selected_tree_item(tree).ok_or_else(|| {
                failure_response(
                    Some(id.clone()),
                    "room_context_menu_unavailable",
                    "room context menu target is unavailable".to_owned(),
                )
            })?;
            let is_room = matches!(
                state_ref(hwnd).tree_items[&item].node,
                NavNode::View(ViewId::Room(_))
            );
            if !is_room {
                return Err(failure_response(
                    Some(id.clone()),
                    "room_context_menu_unavailable",
                    "room context menu target is not a room".to_owned(),
                ));
            }
            invoke_navigation_context_menu_for_automation(hwnd, &args.command).map_err(
                |message| failure_response(Some(id.clone()), "room_menu_action_failed", message),
            )?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::OpenTimelineContextMenu { id, args } => {
            invoke_timeline_context_menu_for_automation(hwnd, &args.command, Some(&args.event_id))
                .map_err(|message| {
                    failure_response(Some(id.clone()), "selected_message_unavailable", message)
                })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::OpenSelectedTimelineContextMenu { id, args } => {
            if selected_timeline_event_id_for_automation(hwnd).is_none() {
                return Err(failure_response(
                    Some(id.clone()),
                    "selected_message_unavailable",
                    "selected message is unavailable".to_owned(),
                ));
            }
            invoke_timeline_context_menu_for_automation(hwnd, &args.command, None).map_err(
                |message| {
                    failure_response(Some(id.clone()), "selected_message_unavailable", message)
                },
            )?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::DumpSelectedNavigationPath { id } => {
            let response = dump_selected_navigation_path(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "ui_state_unavailable", message)
            })?;
            serialize_success_json(&id, response)
        }
        ParsedCommand::DumpRoomActivity { id, args } => {
            let room_id = resolve_timeline_room_id(state_ref(hwnd), args.room_id.as_deref())
                .map_err(|message| failure_response(Some(id.clone()), "unknown_room", message))?;
            let entries = state_ref(hwnd)
                .model
                .room_views
                .get(&room_id)
                .map(|room_view| {
                    room_view
                        .activities
                        .iter()
                        .map(|activity| {
                            dump_room_activity_entry_from_room_entry(room_view, activity)
                        })
                        .collect()
                })
                .unwrap_or_default();
            serialize_success_json(&id, DumpRoomActivityResponse { room_id, entries })
        }
        ParsedCommand::ListRooms { id } => {
            let rooms = state_ref(hwnd)
                .model
                .rooms
                .iter()
                .map(dump_room_from_summary)
                .collect();
            serialize_success_json(&id, ListRoomsResponse { rooms })
        }
        ParsedCommand::SelectRoom { id, args } => {
            activate_view(hwnd, &ViewId::Room(args.room_id.clone()), None)
                .map_err(|message| failure_response(Some(id.clone()), "unknown_room", message))?;
            serialize_success_json(
                &id,
                SelectRoomResponse {
                    selected_room: args.room_id,
                },
            )
        }
        ParsedCommand::ArrangeWindowsViaWindowMenu { id, args } => {
            let command_id = window_menu_command_id(args.arrangement);
            invoke_menu_command_for_automation(hwnd, command_id).map_err(|message| {
                failure_response(Some(id.clone()), "window_menu_failed", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::SetViewWindowState { id, args } => {
            set_view_window_state(hwnd, &view_id_from_identity(&args.view), args.state)
                .map_err(|message| failure_response(Some(id.clone()), "unknown_view", message))?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::SelectView { id, args } => {
            let requested_view = view_id_from_identity(&args.view);
            activate_view(hwnd, &requested_view, None)
                .map_err(|message| failure_response(Some(id.clone()), "unknown_view", message))?;
            let active = active_view_identity(hwnd)
                .map_err(|message| failure_response(Some(id.clone()), "unknown_view", message))?;
            serialize_success_json(&id, SelectViewResponse { active })
        }
        ParsedCommand::FindActiveView { id, args } => {
            let window = state_ref(hwnd).active_view.clone();
            let response = execute_view_find(hwnd, &window, &args.query)
                .map(find_active_view_response)
                .map_err(|message| failure_response(Some(id.clone()), "find_failed", message))?;
            serialize_success_json(&id, response)
        }
        ParsedCommand::SelectNavigationPath { id, args } => {
            select_navigation_path(hwnd, &args.path).map_err(|message| {
                failure_response(Some(id.clone()), "unknown_navigation", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::OpenDialog { id, args } => {
            open_dialog_for_automation(hwnd, args.dialog)
                .map_err(|(code, message)| failure_response(Some(id.clone()), code, message))?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::Crash { args, .. } => {
            panic!("{}", args.message);
        }
        ParsedCommand::ChooseTrayMenuItem { id, args } => {
            unsafe { choose_tray_menu_item(hwnd, &args.label) }.map_err(|message| {
                failure_response(Some(id.clone()), "unknown_tray_menu_item", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::SelectOptionsPage { id, args } => {
            let dialog = state_ref(hwnd).options_dialog.ok_or_else(|| {
                failure_response(
                    Some(id.clone()),
                    "no_options_dialog",
                    "the Options dialog is not open",
                )
            })?;
            options_dialog::select_path(dialog, &args.path).map_err(|message| {
                failure_response(Some(id.clone()), "unknown_options_page", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::SetNavigationExpanded { id, args } => {
            let node = match &args.node {
                NavigationNodeKey::View(identity) => NavNode::View(view_id_from_identity(identity)),
                NavigationNodeKey::Space(room_id) => NavNode::Space {
                    room_id: room_id.clone(),
                },
            };
            set_navigation_expanded(hwnd, &node, args.expanded).map_err(|message| {
                failure_response(Some(id.clone()), "unknown_navigation_node", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::SelectNavigationView { id, args } => {
            select_navigation_view(hwnd, &args.view).map_err(|message| {
                failure_response(Some(id.clone()), "unknown_navigation_view", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::SendText { id, args } => {
            let body = args.body;
            let edit = compose_edit(hwnd);
            set_edit_text(edit, &body);
            let room_id =
                submit_compose_from_control(hwnd, args.room_id.as_deref()).map_err(|message| {
                    let code = if message.starts_with("unknown room id: ") {
                        "unknown_room"
                    } else {
                        "send_failed"
                    };
                    failure_response(Some(id.clone()), code, message)
                })?;
            serialize_success_json(
                &id,
                SendTextResponse {
                    accepted: true,
                    room_id,
                },
            )
        }
        ParsedCommand::SendFile { id, args } => {
            let room_id = match args.room_id {
                Some(room_id) => room_id,
                None => match &state_ref(hwnd).active_view {
                    ViewId::Room(room_id) => room_id.clone(),
                    ViewId::Thread { .. } => {
                        return Err(failure_response(
                            Some(id.clone()),
                            "send_failed",
                            "file attachments cannot target threads yet".to_owned(),
                        ));
                    }
                    ViewId::Status | ViewId::RoomList => {
                        return Err(failure_response(
                            Some(id.clone()),
                            "send_failed",
                            "no room selected".to_owned(),
                        ));
                    }
                },
            };
            let command = Command::SendFile(SendFileRequest {
                room_id: room_id.clone(),
                path: PathBuf::from(args.path),
                filename: args.filename,
                content_type: args.content_type,
            });
            let state = state_mut(hwnd);
            if sendable_room_by_id(&state.model.rooms, &room_id).is_none() {
                return Err(failure_response(
                    Some(id.clone()),
                    "send_failed",
                    format!("room is not joined: {room_id}"),
                ));
            }
            state
                .command_sender
                .as_ref()
                .ok_or_else(|| {
                    failure_response(
                        Some(id.clone()),
                        "send_failed",
                        "core command sender is not available".to_owned(),
                    )
                })?
                .send(command.clone())
                .map_err(|err| {
                    failure_response(
                        Some(id.clone()),
                        "send_failed",
                        format!("failed to enqueue send file command: {err}"),
                    )
                })?;
            #[cfg(test)]
            state.recorded_commands.push(command);
            serialize_success_json(
                &id,
                SendTextResponse {
                    accepted: true,
                    room_id,
                },
            )
        }
        ParsedCommand::DumpComposeText { id } => {
            let text = dump_compose_text(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "compose_unavailable", message)
            })?;
            serialize_success_json(&id, ComposeTextResponse { text })
        }
        ParsedCommand::SetComposeText { id, args } => {
            set_compose_text(hwnd, &args.text).map_err(|message| {
                failure_response(Some(id.clone()), "compose_unavailable", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::SendComposeKey { id, args } => {
            send_compose_key(hwnd, args.key).map_err(|message| {
                failure_response(Some(id.clone()), "compose_key_failed", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::LoadSyntheticState { id, args } => {
            validate_synthetic_room_activity_events(&RoomViews::default(), &args.events).map_err(
                |message| failure_response(Some(id.clone()), "invalid_synthetic_state", message),
            )?;
            let (rooms, timeline_events) =
                unsafe { apply_event_load(hwnd, args.events) }.map_err(|message| {
                    failure_response(Some(id.clone()), "invalid_synthetic_state", message)
                })?;
            serialize_success_json(
                &id,
                LoadEventsResponse {
                    loaded: true,
                    rooms,
                    timeline_events,
                },
            )
        }
        ParsedCommand::AppendSyntheticEvents { id, args } => {
            let room_views = state_ref(hwnd).model.room_views.clone();
            validate_synthetic_room_activity_events(&room_views, &args.events).map_err(
                |message| failure_response(Some(id.clone()), "invalid_synthetic_state", message),
            )?;
            unsafe { apply_events_and_refresh(hwnd, args.events) }.map_err(|message| {
                failure_response(Some(id.clone()), "invalid_synthetic_state", message)
            })?;
            let rooms = state_ref(hwnd).model.rooms.len();
            let timeline_events = state_ref(hwnd)
                .model
                .room_views
                .values()
                .map(|room_view| room_view.activities.len())
                .sum();
            serialize_success_json(
                &id,
                LoadEventsResponse {
                    loaded: true,
                    rooms,
                    timeline_events,
                },
            )
        }
        ParsedCommand::Login { id, args } => {
            submit_login(
                state_mut(hwnd),
                args.homeserver_url,
                args.username,
                args.password,
                args.device_display_name,
                false,
            )
            .map_err(|message| failure_response(Some(id.clone()), "login_failed", message))?;
            refresh_status_bar(hwnd)
                .map_err(|message| failure_response(Some(id.clone()), "login_failed", message))?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::RestoreSession { id, args } => {
            submit_restore_session(state_mut(hwnd), args.homeserver_url, args.session, false)
                .map_err(|message| failure_response(Some(id.clone()), "restore_failed", message))?;
            refresh_status_bar(hwnd)
                .map_err(|message| failure_response(Some(id.clone()), "restore_failed", message))?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::RestoreSavedSession { id } => {
            submit_restore_saved_session(state_mut(hwnd), false).map_err(|message| {
                failure_response(Some(id.clone()), "restore_saved_failed", message)
            })?;
            refresh_status_bar(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "restore_saved_failed", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::StartSync { id } => {
            submit_start_sync(state_mut(hwnd)).map_err(|message| {
                failure_response(Some(id.clone()), "start_sync_failed", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::Logout { id } => {
            log_out(hwnd)
                .map_err(|message| failure_response(Some(id.clone()), "logout_failed", message))?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::RecoverE2ee { id, args } => {
            submit_recover_e2ee(state_mut(hwnd), args.recovery_secret).map_err(|message| {
                failure_response(Some(id.clone()), "recover_e2ee_failed", message)
            })?;
            refresh_status_bar(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "recover_e2ee_failed", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::SetupE2eeRecovery { id, args } => {
            submit_setup_e2ee_recovery(
                state_mut(hwnd),
                args.passphrase,
                args.wait_for_backups_to_upload,
            )
            .map_err(|message| {
                failure_response(Some(id.clone()), "setup_e2ee_recovery_failed", message)
            })?;
            refresh_status_bar(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "setup_e2ee_recovery_failed", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::AcceptVerification { id, args } => {
            submit_automation_verification_action(hwnd, VerificationAction::Accept, args).map_err(
                |message| failure_response(Some(id.clone()), "verification_failed", message),
            )?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::CancelVerification { id, args } => {
            submit_automation_verification_action(hwnd, VerificationAction::Cancel, args).map_err(
                |message| failure_response(Some(id.clone()), "verification_failed", message),
            )?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::ConfirmVerificationSas { id, args } => {
            submit_automation_verification_action(hwnd, VerificationAction::Confirm, args)
                .map_err(|message| {
                    failure_response(Some(id.clone()), "verification_failed", message)
                })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::RejectVerificationSas { id, args } => {
            submit_automation_verification_action(hwnd, VerificationAction::Reject, args).map_err(
                |message| failure_response(Some(id.clone()), "verification_failed", message),
            )?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
    }
}

fn command_request_id(command: &ParsedCommand) -> &str {
    match command {
        ParsedCommand::Ping { id }
        | ParsedCommand::GetVersion { id }
        | ParsedCommand::WaitIdle { id, .. }
        | ParsedCommand::DumpModel { id }
        | ParsedCommand::DumpUiState { id }
        | ParsedCommand::DumpVerificationMenuState { id }
        | ParsedCommand::DumpRoomMenuState { id }
        | ParsedCommand::DumpMessageMenuState { id }
        | ParsedCommand::DumpMemberMenuState { id }
        | ParsedCommand::SelectTimelineEvent { id, .. }
        | ParsedCommand::SelectMember { id, .. }
        | ParsedCommand::QuerySelectedMemberViaMenu { id }
        | ParsedCommand::MentionSelectedMemberViaMenu { id }
        | ParsedCommand::OpenMemberContextMenu { id, .. }
        | ParsedCommand::OpenSelectedMemberContextMenu { id, .. }
        | ParsedCommand::ArrangeWindowsViaWindowMenu { id, .. }
        | ParsedCommand::SetViewWindowState { id, .. }
        | ParsedCommand::AcceptInviteViaRoomMenu { id }
        | ParsedCommand::RejectInviteViaRoomMenu { id }
        | ParsedCommand::PartRoomViaRoomMenu { id }
        | ParsedCommand::ReplyToSelectedMessageViaMenu { id }
        | ParsedCommand::EditSelectedMessageViaMenu { id }
        | ParsedCommand::ReactToSelectedMessageViaMenu { id }
        | ParsedCommand::RedactSelectedMessageViaMenu { id }
        | ParsedCommand::SaveAttachmentViaMessageMenu { id }
        | ParsedCommand::OpenNavigationContextMenu { id, .. }
        | ParsedCommand::OpenSelectedNavigationContextMenu { id, .. }
        | ParsedCommand::OpenTimelineContextMenu { id, .. }
        | ParsedCommand::OpenSelectedTimelineContextMenu { id, .. }
        | ParsedCommand::DumpSelectedNavigationPath { id }
        | ParsedCommand::DumpRoomActivity { id, .. }
        | ParsedCommand::ListRooms { id }
        | ParsedCommand::SelectRoom { id, .. }
        | ParsedCommand::SelectView { id, .. }
        | ParsedCommand::SendText { id, .. }
        | ParsedCommand::SendFile { id, .. }
        | ParsedCommand::LoadSyntheticState { id, .. }
        | ParsedCommand::Login { id, .. }
        | ParsedCommand::RestoreSession { id, .. }
        | ParsedCommand::RestoreSavedSession { id }
        | ParsedCommand::StartSync { id }
        | ParsedCommand::Logout { id }
        | ParsedCommand::DumpComposeText { id }
        | ParsedCommand::SetComposeText { id, .. }
        | ParsedCommand::SendComposeKey { id, .. }
        | ParsedCommand::SelectNavigationPath { id, .. }
        | ParsedCommand::SelectNavigationView { id, .. }
        | ParsedCommand::SetNavigationExpanded { id, .. }
        | ParsedCommand::OpenDialog { id, .. }
        | ParsedCommand::SelectOptionsPage { id, .. }
        | ParsedCommand::ChooseTrayMenuItem { id, .. }
        | ParsedCommand::Crash { id, .. }
        | ParsedCommand::FindActiveView { id, .. }
        | ParsedCommand::AppendSyntheticEvents { id, .. }
        | ParsedCommand::RecoverE2ee { id, .. }
        | ParsedCommand::SetupE2eeRecovery { id, .. }
        | ParsedCommand::AcceptVerification { id, .. }
        | ParsedCommand::CancelVerification { id, .. }
        | ParsedCommand::ConfirmVerificationSas { id, .. }
        | ParsedCommand::RejectVerificationSas { id, .. } => id,
    }
}

fn serialize_success_json<T: Serialize>(id: &str, payload: T) -> Result<Value, FailureEnvelope> {
    serde_json::to_value(payload).map_err(|err| {
        failure_response(
            Some(id.to_owned()),
            "serialization_error",
            format!("failed to serialize response payload: {err}"),
        )
    })
}

fn serialize_response_envelope(
    envelope: &ResponseEnvelope<Value>,
    response_id: Option<String>,
) -> Result<Vec<u8>, FailureEnvelope> {
    serde_json::to_vec(envelope).map_err(|err| {
        failure_response(
            response_id,
            "serialization_error",
            format!("failed to serialize response envelope: {err}"),
        )
    })
}

fn serialize_failure_envelope(envelope: ResponseEnvelope<Value>) -> Option<Vec<u8>> {
    serde_json::to_vec(&envelope).ok()
}

unsafe fn write_response_bytes(
    hwnd: HWND,
    response_id: Option<&str>,
    response_bytes: &[u8],
) -> bool {
    let Some(automation) = state_ref(hwnd).automation.as_ref() else {
        return false;
    };

    if write_response_bytes_if_fits(automation, response_bytes) {
        return true;
    }

    let fallback = ResponseEnvelope::<Value>::Failure(failure_response(
        response_id.map(str::to_owned),
        "response_too_large",
        format!(
            "response exceeded shared-memory capacity {}",
            automation.response_capacity
        ),
    ));
    let Some(fallback_bytes) = serialize_failure_envelope(fallback) else {
        return false;
    };
    write_response_bytes_if_fits(automation, &fallback_bytes)
}

fn write_response_bytes_if_fits(automation: &AutomationState, response_bytes: &[u8]) -> bool {
    if automation.response_view.is_null() || automation.response_capacity < size_of::<u32>() {
        return false;
    }

    let Ok(response_len) = u32::try_from(response_bytes.len()) else {
        return false;
    };
    let Some(required_capacity) = size_of::<u32>().checked_add(response_bytes.len()) else {
        return false;
    };
    if required_capacity > automation.response_capacity {
        return false;
    }

    unsafe {
        let buffer =
            std::slice::from_raw_parts_mut(automation.response_view, automation.response_capacity);
        buffer[..size_of::<u32>()].copy_from_slice(&response_len.to_le_bytes());
        buffer[size_of::<u32>()..required_capacity].copy_from_slice(response_bytes);
    }
    true
}

pub(super) unsafe fn send_automation_request(
    hwnd: HWND,
    command_id: u32,
    request_json: Value,
) -> Result<Value, String> {
    let request_bytes = serde_json::to_vec(&request_json)
        .map_err(|err| format!("failed to serialize self-test request: {err}"))?;
    let request_len = u32::try_from(request_bytes.len())
        .map_err(|_| "self-test request exceeded WM_COPYDATA size range".to_owned())?;
    let mut request = COPYDATASTRUCT {
        dwData: command_id as usize,
        cbData: request_len,
        lpData: request_bytes.as_ptr().cast_mut().cast(),
    };

    let send_result = SendMessageW(
        hwnd,
        WM_COPYDATA,
        hwnd as WPARAM,
        (&mut request as *mut COPYDATASTRUCT) as LPARAM,
    );
    if send_result == 0 {
        return Err(format!(
            "automation transport returned no response for command id {command_id}"
        ));
    }

    read_automation_response(hwnd)
}

unsafe fn read_automation_response(hwnd: HWND) -> Result<Value, String> {
    let automation = state_ref(hwnd)
        .automation
        .as_ref()
        .ok_or_else(|| "automation shared memory is not enabled".to_owned())?;
    if automation.response_view.is_null() || automation.response_capacity < size_of::<u32>() {
        return Err(format!(
            "automation shared memory {} is not mapped",
            automation.response_name
        ));
    }

    let buffer = std::slice::from_raw_parts(automation.response_view, automation.response_capacity);
    let response_len = u32::from_le_bytes(buffer[..size_of::<u32>()].try_into().unwrap()) as usize;
    let required_capacity = size_of::<u32>()
        .checked_add(response_len)
        .ok_or_else(|| "automation response length overflowed".to_owned())?;
    if required_capacity > automation.response_capacity {
        return Err(format!(
            "automation response from {} exceeded mapped capacity {}",
            automation.response_name, automation.response_capacity
        ));
    }

    serde_json::from_slice(&buffer[size_of::<u32>()..required_capacity])
        .map_err(|err| format!("failed to parse automation response JSON: {err}"))
}
