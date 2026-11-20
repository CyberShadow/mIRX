//! The saved session and the store passphrase on disk, and the sign-in,
//! restore, sign-out and E2EE recovery submissions.

use super::*;

pub(super) const PROFILE_SUBDIR: &str = "mirx\\ui-shell";
pub(super) const SESSION_FILE_NAME: &str = "session.json";
pub(super) const SAVED_SESSION_VERSION: u32 = 1;
const PROTECTED_SESSION_MAGIC: &[u8] = b"mirx-ui-session:v1\0";
const SESSION_DPAPI_ENTROPY: &[u8] = b"mirx-ui/session/v1";
const STORE_PASSPHRASE_FILE_NAME: &str = "store-passphrase.dpapi";
const PROTECTED_STORE_PASSPHRASE_MAGIC: &[u8] = b"mirx-ui-store-secret:v1\0";
const STORE_PASSPHRASE_DPAPI_ENTROPY: &[u8] = b"mirx-ui/store-passphrase/v1";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SavedSession {
    pub(super) version: u32,
    pub(super) homeserver_url: String,
    pub(super) session: StoredSession,
}

pub(super) fn profile_dir() -> Result<PathBuf, String> {
    let local_app_data =
        std::env::var_os("LOCALAPPDATA").ok_or_else(|| "LOCALAPPDATA is not set".to_owned())?;
    Ok(PathBuf::from(local_app_data).join(PROFILE_SUBDIR))
}

pub(super) fn session_path() -> Result<PathBuf, String> {
    Ok(profile_dir()?.join(SESSION_FILE_NAME))
}

pub(super) fn load_saved_session() -> Result<SavedSession, String> {
    let path = session_path()?;
    let bytes = fs::read(&path)
        .map_err(|err| format!("failed to read saved session {}: {err}", path.display()))?;
    let protected = bytes.strip_prefix(PROTECTED_SESSION_MAGIC).ok_or_else(|| {
        format!(
            "saved session {} is not in protected format",
            path.display()
        )
    })?;
    let payload = crate::dpapi::unprotect(protected, SESSION_DPAPI_ENTROPY).map_err(|err| {
        format!(
            "failed to unprotect saved session {}: {err}",
            path.display()
        )
    })?;
    let saved = serde_json::from_slice::<SavedSession>(&payload)
        .map_err(|err| format!("failed to parse saved session {}: {err}", path.display()))?;
    if saved.version != SAVED_SESSION_VERSION {
        return Err(format!(
            "unsupported saved session version {} in {}",
            saved.version,
            path.display()
        ));
    }
    Ok(saved)
}

pub(super) fn save_session(homeserver_url: &str, session: &StoredSession) -> Result<(), String> {
    let path = session_path()?;
    let Some(profile_dir) = path.parent() else {
        return Err(format!(
            "saved session path {} has no parent directory",
            path.display()
        ));
    };
    fs::create_dir_all(profile_dir).map_err(|err| {
        format!(
            "failed to create saved session directory {}: {err}",
            profile_dir.display()
        )
    })?;

    let temp_path = path.with_extension("json.tmp");
    let payload = SavedSession {
        version: SAVED_SESSION_VERSION,
        homeserver_url: homeserver_url.to_owned(),
        session: session.clone(),
    };
    let serialized = serde_json::to_vec(&payload).map_err(|err| {
        format!(
            "failed to serialize saved session {}: {err}",
            path.display()
        )
    })?;
    let protected = crate::dpapi::protect(&serialized, "mirx saved session", SESSION_DPAPI_ENTROPY)
        .map_err(|err| format!("failed to protect saved session {}: {err}", path.display()))?;
    let mut bytes = Vec::with_capacity(PROTECTED_SESSION_MAGIC.len() + protected.len());
    bytes.extend_from_slice(PROTECTED_SESSION_MAGIC);
    bytes.extend_from_slice(&protected);
    fs::write(&temp_path, bytes).map_err(|err| {
        format!(
            "failed to write saved session {}: {err}",
            temp_path.display()
        )
    })?;
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => {
            return Err(format!(
                "failed to replace saved session {}: {err}",
                path.display()
            ));
        }
    }
    fs::rename(&temp_path, &path)
        .map_err(|err| format!("failed to install saved session {}: {err}", path.display()))?;
    Ok(())
}

pub(super) fn delete_saved_session() -> Result<(), String> {
    let path = session_path()?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!(
            "failed to delete saved session {}: {err}",
            path.display()
        )),
    }
}

pub(super) fn load_or_create_store_passphrase(profile_dir: &Path) -> Result<String, String> {
    let path = profile_dir.join(STORE_PASSPHRASE_FILE_NAME);
    if path.is_file() {
        let bytes = fs::read(&path).map_err(|err| {
            format!(
                "failed to read protected store passphrase {}: {err}",
                path.display()
            )
        })?;
        let protected = bytes
            .strip_prefix(PROTECTED_STORE_PASSPHRASE_MAGIC)
            .ok_or_else(|| {
                format!(
                    "protected store passphrase {} is missing the expected marker",
                    path.display()
                )
            })?;
        let payload =
            crate::dpapi::unprotect(protected, STORE_PASSPHRASE_DPAPI_ENTROPY).map_err(|err| {
                format!(
                    "failed to unprotect store passphrase {}: {err}",
                    path.display()
                )
            })?;
        let passphrase = String::from_utf8(payload).map_err(|err| {
            format!(
                "protected store passphrase {} is not valid UTF-8: {err}",
                path.display()
            )
        })?;
        if passphrase.len() != 64
            || !passphrase
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(format!(
                "protected store passphrase {} must be 64 lowercase hex characters",
                path.display()
            ));
        }
        return Ok(passphrase);
    }

    let matrix_store_path = profile_dir.join("matrix-store");
    if matrix_store_path.exists() {
        return Err(format!(
            "Matrix store {} exists but protected passphrase {} is missing",
            matrix_store_path.display(),
            path.display()
        ));
    }

    create_store_passphrase(profile_dir, &path)
}

fn create_store_passphrase(profile_dir: &Path, path: &Path) -> Result<String, String> {
    fs::create_dir_all(profile_dir).map_err(|err| {
        format!(
            "failed to create profile directory {}: {err}",
            profile_dir.display()
        )
    })?;

    let passphrase = hex_string(&crate::dpapi::random_bytes::<32>()?);
    let protected = crate::dpapi::protect(
        passphrase.as_bytes(),
        "mirx store passphrase",
        STORE_PASSPHRASE_DPAPI_ENTROPY,
    )
    .map_err(|err| {
        format!(
            "failed to protect store passphrase {}: {err}",
            path.display()
        )
    })?;
    let temp_path = path.with_extension("dpapi.tmp");
    let mut bytes = Vec::with_capacity(PROTECTED_STORE_PASSPHRASE_MAGIC.len() + protected.len());
    bytes.extend_from_slice(PROTECTED_STORE_PASSPHRASE_MAGIC);
    bytes.extend_from_slice(&protected);
    fs::write(&temp_path, bytes).map_err(|err| {
        format!(
            "failed to write protected store passphrase {}: {err}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, path).map_err(|err| {
        format!(
            "failed to install protected store passphrase {}: {err}",
            path.display()
        )
    })?;
    Ok(passphrase)
}

fn hex_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        out.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    out
}

pub(super) fn submit_login(
    state: &mut WindowState,
    server: String,
    username: String,
    password: String,
    device_display_name: String,
    start_sync: bool,
) -> Result<(), String> {
    connection::check_sign_in(state.model.connection)?;
    let server = login_server(&server, &username)?;
    if username.is_empty() {
        return Err("username must not be empty".to_owned());
    }
    if password.is_empty() {
        return Err("password must not be empty".to_owned());
    }

    let device_display_name = if device_display_name.is_empty() {
        DEFAULT_DEVICE_DISPLAY_NAME.to_owned()
    } else {
        device_display_name
    };

    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    let connecting = status_log::connecting(&server, &username);
    sender
        .send(Command::Login(LoginRequest {
            server,
            username,
            password,
            device_display_name,
            start_sync,
        }))
        .map_err(|err| format!("failed to enqueue login command: {err}"))?;

    log_status(state, StatusLineKind::Info, connecting);

    state.model.connection = ConnectionState::SigningIn;
    state.model.status = "logging in".to_owned();
    state.model.lifecycle_state = LifecycleState::LoggingIn;
    Ok(())
}

pub(super) fn submit_restore_session(
    state: &mut WindowState,
    homeserver_url: String,
    session: StoredSession,
    start_sync: bool,
) -> Result<(), String> {
    connection::check_sign_in(state.model.connection)?;
    if homeserver_url.is_empty() {
        return Err("homeserver URL must not be empty".to_owned());
    }
    if session.user_id.is_empty() {
        return Err("session user_id must not be empty".to_owned());
    }
    if session.device_id.is_empty() {
        return Err("session device_id must not be empty".to_owned());
    }
    if session.access_token.is_empty() {
        return Err("session access_token must not be empty".to_owned());
    }

    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    let user_id = session.user_id.clone();
    sender
        .send(Command::RestoreSession(RestoreSessionRequest {
            homeserver_url: homeserver_url.clone(),
            session,
            start_sync,
        }))
        .map_err(|err| format!("failed to enqueue restore session command: {err}"))?;

    log_status(
        state,
        StatusLineKind::Info,
        status_log::restoring(&homeserver_url, &user_id),
    );
    state.model.own_user_id = Some(user_id);
    state.current_session_homeserver_url = Some(homeserver_url);
    state.model.connection = ConnectionState::SigningIn;
    state.model.status = "restoring saved session".to_owned();
    state.model.lifecycle_state = LifecycleState::Restoring;
    Ok(())
}

pub(super) fn submit_restore_saved_session(
    state: &mut WindowState,
    start_sync: bool,
) -> Result<(), String> {
    let saved = load_saved_session()?;
    submit_restore_session(state, saved.homeserver_url, saved.session, start_sync)
}

pub(super) fn submit_start_sync(state: &mut WindowState) -> Result<(), String> {
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(Command::StartSync)
        .map_err(|err| format!("failed to enqueue start sync command: {err}"))
}

pub(super) fn submit_logout(state: &mut WindowState) -> Result<(), String> {
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(Command::Logout)
        .map_err(|err| format!("failed to enqueue logout command: {err}"))
}

pub(super) fn submit_recover_e2ee(
    state: &mut WindowState,
    recovery_secret: String,
) -> Result<(), String> {
    if recovery_secret.is_empty() {
        return Err("recovery key or passphrase must not be empty".to_owned());
    }

    let command = Command::RecoverE2ee(RecoverE2eeRequest { recovery_secret });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue recover e2ee command: {err}"))?;
    state.recorded_commands.push(command);
    state.model.status = "e2ee recovery submitted".to_owned();
    Ok(())
}

pub(super) fn submit_setup_e2ee_recovery(
    state: &mut WindowState,
    passphrase: Option<String>,
    wait_for_backups_to_upload: bool,
) -> Result<(), String> {
    let passphrase_redacted = passphrase.is_some();
    let command = Command::SetupE2eeRecovery(SetupE2eeRecoveryRequest {
        passphrase,
        wait_for_backups_to_upload,
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command)
        .map_err(|err| format!("failed to enqueue setup e2ee recovery command: {err}"))?;
    state
        .submitted_setup_e2ee_recovery
        .push(SubmittedSetupE2eeRecovery {
            passphrase_redacted,
            wait_for_backups_to_upload,
        });
    state.model.status = "e2ee recovery setup submitted".to_owned();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::super::test_fixtures::*;
    use super::*;

    fn protected_saved_session_bytes(saved: &SavedSession) -> Vec<u8> {
        let serialized = serde_json::to_vec(saved).expect("saved session must serialize");
        protected_saved_session_payload_bytes(&serialized)
    }

    fn protected_saved_session_payload_bytes(payload: &[u8]) -> Vec<u8> {
        let protected =
            crate::dpapi::protect(payload, "mirx saved session test", SESSION_DPAPI_ENTROPY)
                .expect("saved session test payload must protect");
        let mut bytes = Vec::with_capacity(PROTECTED_SESSION_MAGIC.len() + protected.len());
        bytes.extend_from_slice(PROTECTED_SESSION_MAGIC);
        bytes.extend_from_slice(&protected);
        bytes
    }

    fn write_session_bytes(root: &Path, bytes: &[u8]) {
        let path = session_file_path(root);
        let parent = path.parent().expect("session file must have parent");
        fs::create_dir_all(parent).expect("session file parent must exist");
        fs::write(path, bytes).expect("session bytes must write");
    }

    fn store_passphrase_file_path(root: &Path) -> PathBuf {
        root.join(PROFILE_SUBDIR).join(STORE_PASSPHRASE_FILE_NAME)
    }

    #[test]
    fn submit_recover_e2ee_enqueues_typed_command_and_updates_status() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());

        submit_recover_e2ee(&mut state, "secret phrase".to_owned()).unwrap();

        assert_eq!(state.model.status, "e2ee recovery submitted");
        assert_eq!(
            state.recorded_commands,
            vec![Command::RecoverE2ee(RecoverE2eeRequest {
                recovery_secret: "secret phrase".to_owned(),
            })]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_recover_e2ee_rejects_empty_secret() {
        let mut state = test_state();

        assert_eq!(
            submit_recover_e2ee(&mut state, String::new()),
            Err("recovery key or passphrase must not be empty".to_owned())
        );
    }

    #[test]
    fn submit_setup_e2ee_recovery_enqueues_command_and_records_only_redacted_metadata() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());

        submit_setup_e2ee_recovery(&mut state, Some("secret phrase".to_owned()), true).unwrap();

        assert_eq!(state.model.status, "e2ee recovery setup submitted");
        assert_eq!(
            state.submitted_setup_e2ee_recovery,
            vec![SubmittedSetupE2eeRecovery {
                passphrase_redacted: true,
                wait_for_backups_to_upload: true,
            }]
        );
        assert!(!format!("{:?}", state.submitted_setup_e2ee_recovery).contains("secret phrase"));
        assert!(state.recorded_commands.is_empty());

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn saved_session_helper_round_trips_and_protects_sensitive_fields() {
        with_local_app_data(|root| {
            save_session("https://matrix.example", &stored_session()).unwrap();
            assert_eq!(load_saved_session().unwrap(), saved_session_payload());
            let saved_bytes = fs::read(session_file_path(&root)).unwrap();
            assert_bytes_do_not_contain(&saved_bytes, "secret-token");
            assert_bytes_do_not_contain(&saved_bytes, "refresh-token");
            assert_bytes_do_not_contain(&saved_bytes, "@alice:example");
            assert_bytes_do_not_contain(&saved_bytes, "DEVICE");
            assert_bytes_do_not_contain(&saved_bytes, "\"access_token\"");

            let path = session_file_path(&root);
            delete_saved_session().unwrap();
            delete_saved_session().unwrap();
            assert!(!path.exists());
        });
    }

    #[test]
    fn load_saved_session_rejects_legacy_cleartext_json_without_marker() {
        with_local_app_data(|root| {
            let cleartext = serde_json::to_vec(&saved_session_payload()).unwrap();
            write_session_bytes(&root, &cleartext);
            assert!(
                load_saved_session()
                    .unwrap_err()
                    .contains("is not in protected format")
            );
        });
    }

    #[test]
    fn load_saved_session_rejects_unsupported_version() {
        with_local_app_data(|root| {
            write_session_bytes(
                &root,
                &protected_saved_session_bytes(&SavedSession {
                    version: 2,
                    ..saved_session_payload()
                }),
            );
            assert!(
                load_saved_session()
                    .unwrap_err()
                    .contains("unsupported saved session version 2")
            );
        });
    }

    #[test]
    fn load_saved_session_rejects_invalid_protected_json() {
        with_local_app_data(|root| {
            write_session_bytes(
                &root,
                &protected_saved_session_payload_bytes(
                    br#"{"version":1,"homeserver_url":"https://matrix.example","session":{"user_id":"@alice:example""#,
                ),
            );
            assert!(
                load_saved_session()
                    .unwrap_err()
                    .contains("failed to parse saved session")
            );
        });
    }

    #[test]
    fn load_saved_session_rejects_corrupt_dpapi_payload() {
        with_local_app_data(|root| {
            write_session_bytes(
                &root,
                &[PROTECTED_SESSION_MAGIC, b"corrupt".as_slice()].concat(),
            );
            assert!(
                load_saved_session()
                    .unwrap_err()
                    .contains("failed to unprotect saved session")
            );
        });
    }

    #[test]
    fn store_passphrase_helper_round_trips_and_protects_secret() {
        with_local_app_data(|root| {
            let profile_dir = root.join(PROFILE_SUBDIR);
            let first = load_or_create_store_passphrase(&profile_dir).unwrap();
            let second = load_or_create_store_passphrase(&profile_dir).unwrap();
            let raw_bytes = fs::read(store_passphrase_file_path(&root)).unwrap();

            assert_eq!(first, second);
            assert_eq!(first.len(), 64);
            assert!(
                first
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            );
            assert_bytes_do_not_contain(&raw_bytes, &first);
        });
    }

    #[test]
    fn load_or_create_store_passphrase_rejects_missing_passphrase_when_store_exists() {
        with_local_app_data(|root| {
            let profile_dir = root.join(PROFILE_SUBDIR);
            let matrix_store_dir = profile_dir.join("matrix-store");
            fs::create_dir_all(&matrix_store_dir).unwrap();
            assert!(
                load_or_create_store_passphrase(&profile_dir)
                    .unwrap_err()
                    .contains("exists but protected passphrase")
            );
        });
    }

    #[test]
    fn load_or_create_store_passphrase_rejects_corrupt_passphrase_file() {
        with_local_app_data(|root| {
            let profile_dir = root.join(PROFILE_SUBDIR);
            let passphrase_path = store_passphrase_file_path(&root);
            let parent = passphrase_path.parent().unwrap();
            fs::create_dir_all(parent).unwrap();
            fs::write(
                &passphrase_path,
                [PROTECTED_STORE_PASSPHRASE_MAGIC, b"corrupt".as_slice()].concat(),
            )
            .unwrap();
            assert!(
                load_or_create_store_passphrase(&profile_dir)
                    .unwrap_err()
                    .contains("failed to unprotect store passphrase")
            );
        });
    }

    #[test]
    fn submit_login_and_restore_helpers_validate_inputs_and_update_lifecycle() {
        let mut state = test_state();
        let (sender, mut commands) = inert_command_sender();
        state.command_sender = Some(sender);

        submit_login(
            &mut state,
            "https://matrix.example".to_owned(),
            "@alice:example".to_owned(),
            "secret".to_owned(),
            String::new(),
            false,
        )
        .unwrap();
        assert_eq!(state.model.lifecycle_state, LifecycleState::LoggingIn);
        assert_eq!(state.model.connection, ConnectionState::SigningIn);
        assert_eq!(
            status_log_lines(&state).last().map(String::as_str),
            Some("[22:12] * Connecting to https://matrix.example as @alice:example")
        );
        assert!(matches!(
            commands.try_recv(),
            Ok(Command::Login(LoginRequest { server, username, start_sync: false, .. }))
                if server == "https://matrix.example" && username == "@alice:example"
        ));
        assert_eq!(
            submit_restore_session(
                &mut state,
                "https://matrix.example".to_owned(),
                stored_session(),
                false,
            ),
            Err("already connecting".to_owned())
        );

        // The login failed.
        state.model.connection = ConnectionState::SignedOut;
        submit_restore_session(
            &mut state,
            "https://matrix.example".to_owned(),
            stored_session(),
            true,
        )
        .unwrap();
        assert_eq!(
            state.current_session_homeserver_url,
            Some("https://matrix.example".to_owned())
        );
        assert_eq!(state.model.lifecycle_state, LifecycleState::Restoring);
        assert_eq!(state.model.connection, ConnectionState::SigningIn);
        assert_eq!(state.model.own_user_id.as_deref(), Some("@alice:example"));
        assert_eq!(
            commands.try_recv(),
            Ok(Command::RestoreSession(RestoreSessionRequest {
                homeserver_url: "https://matrix.example".to_owned(),
                session: stored_session(),
                start_sync: true,
            }))
        );

        state.model.connection = ConnectionState::SignedIn;
        assert_eq!(
            submit_login(
                &mut state,
                "https://matrix.example".to_owned(),
                "alice".to_owned(),
                "secret".to_owned(),
                String::new(),
                true,
            ),
            Err("already signed in; log out first".to_owned())
        );

        state.model.connection = ConnectionState::SignedOut;
        assert_eq!(
            submit_login(
                &mut state,
                String::new(),
                "alice".to_owned(),
                "secret".to_owned(),
                String::new(),
                true,
            ),
            Err("enter a server, or a full user ID like @alice:matrix.org".to_owned())
        );
        assert_eq!(
            submit_restore_session(&mut state, String::new(), stored_session(), true),
            Err("homeserver URL must not be empty".to_owned())
        );
        assert!(commands.try_recv().is_err(), "refused submits send nothing");
    }

    #[test]
    fn submit_restore_saved_session_uses_persisted_session_and_submitters_require_sender() {
        with_local_app_data(|_| {
            let mut state = test_state();

            let restore_error = submit_restore_saved_session(&mut state, false)
                .expect_err("missing session must fail");
            assert!(restore_error.contains("failed to read saved session"));
            assert!(restore_error.contains(SESSION_FILE_NAME));
            assert_eq!(
                submit_start_sync(&mut state),
                Err("core command sender is not available".to_owned())
            );
            assert_eq!(
                submit_logout(&mut state),
                Err("core command sender is not available".to_owned())
            );

            let (sender, mut commands) = inert_command_sender();
            state.command_sender = Some(sender);
            save_session("https://matrix.example", &stored_session()).unwrap();

            submit_restore_saved_session(&mut state, false).unwrap();
            assert_eq!(state.model.status, "restoring saved session");
            assert_eq!(state.model.lifecycle_state, LifecycleState::Restoring);

            submit_start_sync(&mut state).unwrap();
            submit_logout(&mut state).unwrap();
            assert_eq!(
                commands.try_recv(),
                Ok(Command::RestoreSession(RestoreSessionRequest {
                    homeserver_url: "https://matrix.example".to_owned(),
                    session: stored_session(),
                    start_sync: false,
                }))
            );
            assert_eq!(commands.try_recv(), Ok(Command::StartSync));
            assert_eq!(commands.try_recv(), Ok(Command::Logout));
        });
    }
}
