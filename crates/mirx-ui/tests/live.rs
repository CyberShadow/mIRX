//! The live lane: the release mirx-ui.exe, driven through its GUI, against
//! a real homeserver (Conduit), with the other side played by matrix-sdk
//! clients in this process. Only `scripts/live-lane.sh` runs it, in a NixOS
//! VM (`nix/live-lane.nix`) that sets:
//!
//! - `MIRX_LIVE_HOMESERVER`: the homeserver's URL;
//! - `MIRX_UI_EXE`: mirx-ui.exe, as a Windows path.
//!
//! Every test registers its own users, so no test sees another's rooms.
#![cfg(windows)]

mod support;

use std::env;
use std::error::Error;
use std::fs;
use std::future::IntoFuture;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use matrix_sdk::Client;
use matrix_sdk::config::SyncSettings;
use matrix_sdk::room::Room as SdkRoom;
use matrix_sdk::ruma::api::client::account::register::v3::Request as RegisterRequest;
use matrix_sdk::ruma::api::client::room::Visibility;
use matrix_sdk::ruma::api::client::room::create_room::v3::{
    Request as CreateRoomRequest, RoomPreset,
};
use matrix_sdk::ruma::api::client::uiaa::{
    AuthData, Dummy, MatrixUserIdentifier, Password, UserIdentifier,
};
use matrix_sdk::ruma::events::InitialStateEvent;
use matrix_sdk::ruma::events::ignored_user_list::IgnoredUserListEventContent;
use matrix_sdk::ruma::events::room::encryption::RoomEncryptionEventContent;
use matrix_sdk::ruma::events::room::member::MembershipState;
use matrix_sdk::ruma::events::room::message::{
    OriginalSyncRoomMessageEvent, RoomMessageEventContent,
};
use matrix_sdk::ruma::{DeviceId, OwnedDeviceId, OwnedRoomId, OwnedUserId, RoomId, UserId};
use mirx_core::{E2eeRecoveryState, E2eeStatus, RoomMembership};
use mirx_testkit::{AutomationDialog, ComposeKey, ContextMenuCommand, MirxApp, ViewIdentity};
use support::*;
use windows_sys::Win32::Foundation::{HWND, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetDlgItem, PostMessageW, SetWindowTextW, WM_COMMAND,
};

/// Every live user's password.
const PASSWORD: &str = "live-lane-password";
/// How long mirx and the peers get to see a change on the server: Wine in
/// a VM is slow, and a change can wait out a whole 30 s `/sync` long poll.
const TIMEOUT: Duration = Duration::from_secs(90);
/// The Recover E2EE dialog's secret field and its OK button.
const RECOVER_E2EE_DIALOG_SECRET_ID: i32 = 5007;
const RECOVER_E2EE_DIALOG_OK_ID: usize = 5008;
/// The recovery key handoff dialog's "I Saved It".
const RECOVERY_KEY_HANDOFF_DIALOG_ACK_ID: usize = 5018;

/// The homeserver under test, and the runtime its peers run on.
struct Homeserver {
    url: String,
    runtime: tokio::runtime::Runtime,
}

fn homeserver() -> Homeserver {
    let url = env::var("MIRX_LIVE_HOMESERVER")
        .expect("MIRX_LIVE_HOMESERVER is not set: run scripts/live-lane.sh");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the peers' runtime builds");
    Homeserver { url, runtime }
}

/// `<stem>-<process id>-<counter>`: a name no other test uses.
fn unique_name(stem: &str) -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(1);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{stem}-{}-{counter}", std::process::id())
}

impl Homeserver {
    /// Registers a new user named after `stem`, with `display_name`, and
    /// returns its first device, synced once (so its keys are uploaded).
    fn register(&self, stem: &str, display_name: &str) -> Peer<'_> {
        let localpart = unique_name(stem);
        let client = self.runtime.block_on(async {
            let client = Client::builder()
                .homeserver_url(&self.url)
                .sqlite_store(unique_temp_path("mirx-live-peer", None), None)
                .build()
                .await
                .expect("a peer's client builds");
            let mut request = RegisterRequest::new();
            request.username = Some(localpart.clone());
            request.password = Some(PASSWORD.to_owned());
            request.auth = Some(AuthData::Dummy(Dummy::new()));
            client
                .matrix_auth()
                .register(request)
                .await
                .unwrap_or_else(|error| panic!("registering {localpart} failed: {error}"));
            // The server's default is not ours to pin.
            client
                .account()
                .set_display_name(Some(display_name))
                .await
                .expect("setting the display name succeeds");
            client
        });
        let texts = Arc::new(Mutex::new(Vec::new()));
        let handler_texts = Arc::clone(&texts);
        client.add_event_handler(move |event: OriginalSyncRoomMessageEvent, room: SdkRoom| {
            let texts = Arc::clone(&handler_texts);
            async move {
                texts.lock().unwrap().push(Text {
                    room_id: room.room_id().to_owned(),
                    sender: event.sender,
                    body: event.content.body().to_owned(),
                });
            }
        });
        let peer = Peer {
            homeserver: self,
            client: ManuallyDrop::new(client),
            localpart,
            texts,
        };
        peer.sync_until("its first sync", async || Ok(()));
        peer
    }
}

/// A message a peer received.
#[derive(Debug)]
struct Text {
    room_id: OwnedRoomId,
    sender: OwnedUserId,
    body: String,
}

/// A device of a user, played by matrix-sdk.
struct Peer<'a> {
    homeserver: &'a Homeserver,
    /// Dropped in the runtime: see `drop`.
    client: ManuallyDrop<Client>,
    localpart: String,
    /// Every message this device received, in order.
    texts: Arc<Mutex<Vec<Text>>>,
}

impl Drop for Peer<'_> {
    /// A client's stores close their connections on the runtime's blocking
    /// pool, so the client must drop inside the runtime.
    fn drop(&mut self) {
        let _runtime = self.homeserver.runtime.enter();
        // SAFETY: this is the peer's drop: `client` is never used again.
        unsafe { ManuallyDrop::drop(&mut self.client) }
    }
}

impl Peer<'_> {
    fn user_id(&self) -> &UserId {
        self.client.user_id().expect("a peer is signed in")
    }

    /// Syncs until `check` holds and returns what it found. On timeout,
    /// panics with what `check` saw last.
    fn sync_until<T>(&self, what: &str, mut check: impl AsyncFnMut() -> Result<T, String>) -> T {
        self.homeserver.runtime.block_on(async {
            let deadline = Instant::now() + TIMEOUT;
            loop {
                self.client
                    .sync_once(SyncSettings::default().timeout(Duration::from_secs(1)))
                    .await
                    .unwrap_or_else(|error| panic!("{} could not sync: {error}", self.localpart));
                match check().await {
                    Ok(found) => return found,
                    Err(seen) if Instant::now() >= deadline => {
                        panic!("{} never saw {what}: {seen}", self.localpart)
                    }
                    Err(_) => {}
                }
            }
        })
    }

    fn room(&self, room_id: &RoomId) -> SdkRoom {
        self.client
            .get_room(room_id)
            .unwrap_or_else(|| panic!("{} knows no room {room_id}", self.localpart))
    }

    /// Creates a public room, in the room directory under
    /// `#<alias_localpart>:<server>`.
    fn create_public_room(&self, name: &str, alias_localpart: &str) -> OwnedRoomId {
        let mut request = CreateRoomRequest::new();
        request.name = Some(name.to_owned());
        request.room_alias_name = Some(alias_localpart.to_owned());
        request.visibility = Visibility::Public;
        request.preset = Some(RoomPreset::PublicChat);
        self.homeserver
            .runtime
            .block_on(self.client.create_room(request))
            .expect("creating a public room succeeds")
            .room_id()
            .to_owned()
    }

    /// Creates an encrypted private room `invitee` is invited to.
    fn create_encrypted_room(&self, name: &str, invitee: &UserId) -> OwnedRoomId {
        let mut request = CreateRoomRequest::new();
        request.name = Some(name.to_owned());
        request.preset = Some(RoomPreset::PrivateChat);
        request.invite = vec![invitee.to_owned()];
        request.initial_state = vec![
            InitialStateEvent::with_empty_state_key(
                RoomEncryptionEventContent::with_recommended_defaults(),
            )
            .to_raw_any(),
        ];
        self.homeserver
            .runtime
            .block_on(self.client.create_room(request))
            .expect("creating an encrypted room succeeds")
            .room_id()
            .to_owned()
    }

    /// Waits until `user_id` has joined the room.
    fn wait_for_member(&self, room_id: &RoomId, user_id: &UserId) {
        let room = self.room(room_id);
        self.sync_until(&format!("{user_id} join {room_id}"), async || {
            let member = room
                .get_member_no_sync(user_id)
                .await
                .expect("the store reads members");
            match member {
                Some(member) if *member.membership() == MembershipState::Join => Ok(()),
                other => Err(format!(
                    "{:?}",
                    other.map(|member| member.membership().clone())
                )),
            }
        });
    }

    fn send_text(&self, room_id: &RoomId, body: &str) {
        let room = self.room(room_id);
        self.homeserver
            .runtime
            .block_on(
                room.send(RoomMessageEventContent::text_plain(body))
                    .into_future(),
            )
            .unwrap_or_else(|error| panic!("{} could not send {body:?}: {error}", self.localpart));
    }

    /// Waits for a message saying `body` in the room and returns its sender.
    fn wait_for_text(&self, room_id: &RoomId, body: &str) -> OwnedUserId {
        self.sync_until(&format!("{body:?} in {room_id}"), async || {
            let texts = self.texts.lock().unwrap();
            texts
                .iter()
                .find(|text| text.room_id == room_id && text.body == body)
                .map(|text| text.sender.clone())
                .ok_or_else(|| format!("{texts:?}"))
        })
    }

    /// The user's ignore list as the server has it, sorted.
    fn ignored_users(&self) -> Vec<String> {
        self.homeserver.runtime.block_on(async {
            let raw = self
                .client
                .account()
                .fetch_account_data_static::<IgnoredUserListEventContent>()
                .await
                .expect("reading the ignore list succeeds");
            raw.map(|raw| raw.deserialize().expect("the ignore list decodes"))
                .unwrap_or_default()
                .ignored_users
                .keys()
                .map(ToString::to_string)
                .collect()
        })
    }

    fn device_id(&self) -> &DeviceId {
        self.client.device_id().expect("a peer is signed in")
    }

    /// The user's devices, as the server lists them.
    fn devices(&self) -> Vec<OwnedDeviceId> {
        let mut devices = self
            .homeserver
            .runtime
            .block_on(self.client.devices())
            .expect("listing devices succeeds")
            .devices
            .into_iter()
            .map(|device| device.device_id)
            .collect::<Vec<_>>();
        devices.sort();
        devices
    }

    /// Signs one of the user's devices out, with the password the server
    /// asks for.
    fn delete_device(&self, device_id: &DeviceId) {
        let devices = [device_id.to_owned()];
        self.homeserver.runtime.block_on(async {
            let error = self
                .client
                .delete_devices(&devices, None)
                .await
                .expect_err("deleting a device asks for authentication");
            let session = error
                .as_uiaa_response()
                .unwrap_or_else(|| panic!("not an authentication request: {error}"))
                .session
                .clone();
            let mut password = Password::new(
                UserIdentifier::Matrix(MatrixUserIdentifier::new(self.localpart.clone())),
                PASSWORD.to_owned(),
            );
            password.session = session;
            self.client
                .delete_devices(&devices, Some(AuthData::Password(password)))
                .await
                .expect("deleting a device with the password succeeds");
        });
    }
}

/// mirx-ui.exe, as the lane passes it.
fn mirx_ui_exe() -> PathBuf {
    PathBuf::from(
        env::var_os("MIRX_UI_EXE").expect("MIRX_UI_EXE is not set: run scripts/live-lane.sh"),
    )
}

fn launch_live_app(local_app_data: &Path) -> Result<GuiTestApp, Box<dyn Error>> {
    launch_automation_app(&mirx_ui_exe(), local_app_data)
}

/// Signs mirx in as `user` through Select Server and returns the device
/// it signed in as, from Status.
fn sign_in(app: &MirxApp, homeserver: &Homeserver, user: &Peer) -> Result<String, Box<dyn Error>> {
    sign_in_with_select_server(app, &homeserver.url, &user.localpart, PASSWORD).map_err(
        |error| {
            format!(
                "signing in failed: {error}; {:?}; Status: {:?}",
                app.dump_model()
                    .map(|model| (model.status, model.last_error)),
                status_view_lines(app)
            )
        },
    )?;
    let logged_in = format!("* Logged in as {} (device ", user.user_id());
    wait_for_status_line(app, &logged_in, TIMEOUT)?;
    let lines = status_view_lines(app)?;
    let line = lines
        .iter()
        .rev()
        .find_map(|line| line.split_once(&logged_in))
        .expect("Status shows the sign-in")
        .1;
    Ok(line
        .strip_suffix(')')
        .unwrap_or_else(|| panic!("a device ends the line: {line:?}"))
        .to_owned())
}

/// Waits for a line containing `text` in the window and returns it.
fn wait_for_view_line(
    app: &MirxApp,
    view: &ViewIdentity,
    text: &str,
) -> Result<String, Box<dyn Error>> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let ui_state = app.dump_ui_state()?;
        let lines = ui_state
            .mdi
            .views
            .iter()
            .find(|candidate| &candidate.identity == view)
            .map(|view| &view.lines);
        if let Some(line) = lines.into_iter().flatten().find(|line| line.contains(text)) {
            return Ok(line.clone());
        }
        if Instant::now() >= deadline {
            return Err(format!("{view:?} never showed {text:?}: {lines:?}").into());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Joins a room by its address, typed in Status.
fn join_by_address(app: &MirxApp, alias: &str, room_id: &RoomId) -> Result<(), Box<dyn Error>> {
    app.select_status_view()?;
    type_commands(app, &[&format!("/join {alias}")])?;
    wait_for_view_line(app, &room_identity(room_id.as_str()), "* Now talking in ")?;
    Ok(())
}

/// Waits until mirx lists the room with `membership`.
fn wait_for_room_membership(
    app: &MirxApp,
    room_id: &RoomId,
    membership: RoomMembership,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let rooms = app.dump_model()?.rooms;
        if rooms
            .iter()
            .any(|room| room.room_id == room_id.as_str() && room.membership == membership)
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("{room_id} never had {membership:?}: {rooms:?}").into());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// What every test ends with: no error shown, mirx closed, its profile gone.
fn finish(app: GuiTestApp, local_app_data: &Path) -> Result<(), Box<dyn Error>> {
    assert_eq!(app.dump_model()?.last_error, None);
    drop(app);
    let _ = fs::remove_dir_all(local_app_data);
    Ok(())
}

#[test]
fn messages_go_both_ways_in_a_room_joined_by_address() -> Result<(), Box<dyn Error>> {
    let homeserver = homeserver();
    let bob = homeserver.register("bob", "Bob");
    let alice = homeserver.register("alice", "Alice");
    let alias_localpart = unique_name("lobby");
    let room_id = bob.create_public_room("Lobby", &alias_localpart);
    let local_app_data = unique_local_app_data_path();
    let app = launch_live_app(&local_app_data)?;
    sign_in(&app, &homeserver, &alice)?;

    join_by_address(&app, &format!("#{alias_localpart}:localhost"), &room_id)?;
    bob.wait_for_member(&room_id, alice.user_id());
    bob.send_text(&room_id, "hello from bob");
    let room = room_identity(room_id.as_str());
    wait_for_view_line(&app, &room, "<Bob> hello from bob")?;

    app.select_view(room)?;
    type_commands(&app, &["hi from mirx"])?;
    assert_eq!(bob.wait_for_text(&room_id, "hi from mirx"), alice.user_id());

    finish(app, &local_app_data)
}

#[test]
fn an_encrypted_room_talks_both_ways() -> Result<(), Box<dyn Error>> {
    let homeserver = homeserver();
    let bob = homeserver.register("bob", "Bob");
    let alice = homeserver.register("alice", "Alice");
    let local_app_data = unique_local_app_data_path();
    let app = launch_live_app(&local_app_data)?;
    sign_in(&app, &homeserver, &alice)?;
    let room_id = bob.create_encrypted_room("Secrets", alice.user_id());

    wait_for_room_membership(&app, &room_id, RoomMembership::Invite)?;
    app.select_room(room_id.as_str())?;
    app.open_selected_navigation_context_menu(ContextMenuCommand::AcceptInvite)?;
    wait_for_room_membership(&app, &room_id, RoomMembership::Join)?;
    let room = room_identity(room_id.as_str());
    wait_for_view_line(
        &app,
        &room,
        "* Messages in this room are end-to-end encrypted",
    )?;
    // Bob encrypts for the devices he knows of when he sends: mirx's, once
    // he has seen Alice join.
    bob.wait_for_member(&room_id, alice.user_id());
    bob.send_text(&room_id, "secret from bob");
    wait_for_view_line(&app, &room, "<Bob> secret from bob")?;
    let ui_state = app.dump_ui_state()?;
    let lines = &find_view(&ui_state, &room).lines;
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("could not be decrypted")),
        "{lines:?}"
    );

    app.select_view(room)?;
    type_commands(&app, &["secret from mirx"])?;
    assert_eq!(
        bob.wait_for_text(&room_id, "secret from mirx"),
        alice.user_id()
    );

    finish(app, &local_app_data)
}

/// Log Out ends the session on the server, and the next sign-in is a new
/// device that syncs the account's rooms again.
#[test]
fn logging_out_and_signing_in_again_works() -> Result<(), Box<dyn Error>> {
    let homeserver = homeserver();
    let bob = homeserver.register("bob", "Bob");
    let alice_phone = homeserver.register("alice", "Alice");
    let alias_localpart = unique_name("lobby");
    let room_id = bob.create_public_room("Lobby", &alias_localpart);
    let local_app_data = unique_local_app_data_path();
    let app = launch_live_app(&local_app_data)?;
    let first_device = sign_in(&app, &homeserver, &alice_phone)?;
    join_by_address(&app, &format!("#{alias_localpart}:localhost"), &room_id)?;
    assert!(
        alice_phone
            .devices()
            .iter()
            .any(|device| *device == first_device),
        "{:?}",
        alice_phone.devices()
    );

    // A new account has no recovery.
    log_out_anyway_with_the_menu(&app, &local_app_data)?;
    assert_eq!(alice_phone.devices(), [alice_phone.device_id().to_owned()]);

    let second_device = sign_in(&app, &homeserver, &alice_phone)?;
    assert_ne!(second_device, first_device);
    wait_for_room_membership(&app, &room_id, RoomMembership::Join)?;
    bob.send_text(&room_id, "after sign-in");
    wait_for_view_line(
        &app,
        &room_identity(room_id.as_str()),
        "<Bob> after sign-in",
    )?;

    finish(app, &local_app_data)
}

/// A session signed out from another device: mirx says so and does not
/// retry; Log Out still works, and so does a new sign-in.
#[test]
fn a_session_signed_out_elsewhere_ends_and_signs_in_again() -> Result<(), Box<dyn Error>> {
    let homeserver = homeserver();
    let alice_phone = homeserver.register("alice", "Alice");
    let local_app_data = unique_local_app_data_path();
    write_connect_settings(&local_app_data, "retry_delay=1\r\n")?;
    let app = launch_live_app(&local_app_data)?;
    let device = sign_in(&app, &homeserver, &alice_phone)?;

    alice_phone.delete_device(device.as_str().into());
    wait_for_status_line(&app, "signed this session out (", TIMEOUT)?;
    let lines = status_view_lines(&app)?;
    let ended = lines
        .iter()
        .find(|line| line.contains("signed this session out ("))
        .expect("Status says the session ended");
    assert!(
        ended.ends_with(
            "). /e2ee export saves its room keys; File > Log Out ends it, then sign in again."
        ),
        "{ended:?}"
    );
    // Twice the retry delay: a retry, had one been armed, would have run.
    std::thread::sleep(Duration::from_secs(2));
    app.wait_idle(Duration::from_secs(5))?;
    let lines = status_view_lines(&app)?;
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("Reconnect") || line.contains("Connect retry")),
        "{lines:?}"
    );

    log_out_anyway_with_the_menu(&app, &local_app_data)?;
    assert!(
        !status_view_contains(&app, "Error during logout")?,
        "{:?}",
        status_view_lines(&app)?
    );
    sign_in(&app, &homeserver, &alice_phone)?;

    finish(app, &local_app_data)
}

/// A restart restores the saved session (no new device) and catches up on
/// what was said while mirx was closed.
#[test]
fn a_restart_restores_the_session_and_catches_up() -> Result<(), Box<dyn Error>> {
    let homeserver = homeserver();
    let bob = homeserver.register("bob", "Bob");
    let alice_phone = homeserver.register("alice", "Alice");
    let alias_localpart = unique_name("lobby");
    let room_id = bob.create_public_room("Lobby", &alias_localpart);
    let room = room_identity(room_id.as_str());
    let local_app_data = unique_local_app_data_path();
    let app = launch_live_app(&local_app_data)?;
    sign_in(&app, &homeserver, &alice_phone)?;
    join_by_address(&app, &format!("#{alias_localpart}:localhost"), &room_id)?;
    bob.wait_for_member(&room_id, alice_phone.user_id());
    bob.send_text(&room_id, "before the restart");
    wait_for_view_line(&app, &room, "<Bob> before the restart")?;
    let devices = alice_phone.devices();
    let session = fs::read(session_file_path(&local_app_data))?;
    assert_bytes_do_not_contain(&session, "\"access_token\"");
    assert_bytes_do_not_contain(&session, PASSWORD);
    assert!(app.close(Duration::from_secs(30))?.success());

    bob.send_text(&room_id, "while mirx was closed");
    // Not launch_live_app: the restore starts before the core reports
    // Started, so the lifecycle never shows core_started.
    let app = launch_app_with_options(
        &mirx_ui_exe(),
        &["--automation".into()],
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_for_connection("syncing", TIMEOUT)?;
    wait_for_view_line(&app, &room, "<Bob> while mirx was closed")?;
    assert_eq!(alice_phone.devices(), devices);

    finish(app, &local_app_data)
}

/// With connect on startup off, a restart shows the room and Bob in its
/// nick list from the store a real server filled, before connecting; what
/// Bob said meanwhile comes with File > Connect.
#[test]
fn a_restart_with_connect_on_startup_off_shows_the_rooms_it_had() -> Result<(), Box<dyn Error>> {
    let homeserver = homeserver();
    let bob = homeserver.register("bob", "Bob");
    let alice = homeserver.register("alice", "Alice");
    let alias_localpart = unique_name("lobby");
    let room_id = bob.create_public_room("Lobby", &alias_localpart);
    let room = room_identity(room_id.as_str());
    let local_app_data = unique_local_app_data_path();
    let app = launch_live_app(&local_app_data)?;
    sign_in(&app, &homeserver, &alice)?;
    join_by_address(&app, &format!("#{alias_localpart}:localhost"), &room_id)?;
    bob.wait_for_member(&room_id, alice.user_id());
    bob.send_text(&room_id, "before the restart");
    wait_for_view_line(&app, &room, "<Bob> before the restart")?;
    assert!(app.close(Duration::from_secs(30))?.success());
    write_settings(&local_app_data, "[connect]\r\nstartup=off\r\n")?;

    bob.send_text(&room_id, "while mirx was closed");
    // Not launch_live_app: the restore starts before the core reports
    // Started, so the lifecycle never shows core_started.
    let app = launch_app_with_options(
        &mirx_ui_exe(),
        &["--automation".into()],
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_for_connection("signed_in", TIMEOUT)?;
    let ui_state = app.wait_for_ui_state("the room and Bob", TIMEOUT, |ui_state| {
        ui_state.mdi.views.iter().any(|view| {
            view.identity == room
                && view
                    .members
                    .iter()
                    .any(|member| member.user_id == bob.user_id().as_str())
        })
    })?;
    assert!(
        !view_contains(find_view(&ui_state, &room), "while mirx was closed"),
        "{:?}",
        find_view(&ui_state, &room).lines
    );
    assert_eq!(app.dump_model()?.connection, "signed_in");

    invoke_account_menu_command(&app, MENU_ID_FILE_CONNECT)?;
    wait_for_view_line(&app, &room, "<Bob> while mirx was closed")?;

    finish(app, &local_app_data)
}

/// Status's lines, without their timestamps.
fn status_texts(app: &MirxApp) -> Result<Vec<String>, Box<dyn Error>> {
    Ok(status_view_lines(app)?
        .into_iter()
        .map(|line| {
            line.split_once("] ")
                .map_or(line.as_str(), |(_, text)| text)
                .to_owned()
        })
        .collect())
}

/// Waits out what background tasks still have to say: the SDK's E2EE
/// state streams tick within moments of a sign-in, a restore and a sync.
fn let_background_tasks_settle(app: &MirxApp) -> Result<(), Box<dyn Error>> {
    app.wait_idle(Duration::from_secs(5))?;
    std::thread::sleep(Duration::from_secs(2));
    app.wait_idle(Duration::from_secs(5))?;
    Ok(())
}

/// How soon a connect on a quiet account must be through: a few
/// requests answered at once, against a 30 s long poll.
const CONNECT_AT_ONCE: Duration = Duration::from_secs(10);

/// Waits until the status bar says the session is connected, and checks
/// it took less than `CONNECT_AT_ONCE`, saying how long it took either way.
fn assert_connects_at_once(app: &MirxApp, what: &str) -> Result<(), Box<dyn Error>> {
    let start = Instant::now();
    loop {
        let status = app.dump_model()?.status;
        if status == "connected to localhost" {
            let took = start.elapsed();
            eprintln!("{what} connected after {took:?}");
            assert!(took < CONNECT_AT_ONCE, "{what} connected after {took:?}");
            return Ok(());
        }
        if start.elapsed() >= TIMEOUT {
            return Err(format!(
                "{what}: never connected; status {status:?}; Status: {:?}",
                status_texts(app)?
            )
            .into());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Conduit answers a `/sync` whose device and `since` equal a request it
/// is still working on with that request's answer: one mirx gave up on
/// (closed, or File > Disconnect) goes on for its 30 s long poll, and a
/// connect within it waits for its end, whatever its own `timeout`
/// (`sync_receivers` in Conduit's `sync_events_route`). A user who
/// reconnects later is not held: wait it out.
fn outlast_the_abandoned_long_poll() {
    std::thread::sleep(Duration::from_secs(31));
}

/// A connect on a quiet account is answered at once, not after the
/// server's 30 s long poll; and Status says each thing once, at a
/// sign-in, a restart and a Disconnect/Connect.
#[test]
fn a_restart_connects_at_once_and_status_says_each_thing_once() -> Result<(), Box<dyn Error>> {
    let homeserver = homeserver();
    let bob = homeserver.register("bob", "Bob");
    let alice = homeserver.register("alice", "Alice");
    let alias_localpart = unique_name("lobby");
    let room_id = bob.create_public_room("Lobby", &alias_localpart);
    let local_app_data = unique_local_app_data_path();
    let app = launch_live_app(&local_app_data)?;
    let device = sign_in(&app, &homeserver, &alice)?;
    wait_for_status_line(&app, "* Syncing with the homeserver", TIMEOUT)?;
    let_background_tasks_settle(&app)?;
    // A new account: the sign-in made its identity and key backup, and
    // recovery is not set up.
    let e2ee = "* E2EE: recovery disabled, local backup enabled, server backup exists, cross-signing complete, this session verified";
    let setup_hint = "* Use /e2ee setup to enable recovery";
    assert_eq!(
        status_texts(&app)?,
        [
            "* Not connected. Use File > Select Server... (Alt+E) or /server to connect to a Matrix homeserver".to_owned(),
            format!("* Connecting to {} as {}", homeserver.url, alice.localpart),
            format!("* Logged in as {} (device {device})", alice.user_id()),
            "* Synced rooms: 0 joined, 0 invited".to_owned(),
            e2ee.to_owned(),
            setup_hint.to_owned(),
            "* Syncing with the homeserver".to_owned(),
        ]
    );
    // A join lists the rooms again: no news for Status.
    join_by_address(&app, &format!("#{alias_localpart}:localhost"), &room_id)?;
    let_background_tasks_settle(&app)?;
    let lines = status_texts(&app)?;
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.starts_with("* Synced rooms: "))
            .count(),
        1,
        "{lines:?}"
    );
    assert!(app.close(Duration::from_secs(30))?.success());
    outlast_the_abandoned_long_poll();

    // Not launch_live_app: the restore starts before the core reports
    // Started, so the lifecycle never shows core_started.
    let app = launch_app_with_options(
        &mirx_ui_exe(),
        &["--automation".into()],
        launch_options(Some(local_app_data.clone())),
    )?;
    assert_connects_at_once(&app, "the restart")?;
    let_background_tasks_settle(&app)?;
    assert_eq!(
        status_texts(&app)?,
        [
            format!(
                "* Restoring session for {} on {}/",
                alice.user_id(),
                homeserver.url
            ),
            "* Session restored".to_owned(),
            "* Synced rooms: 1 joined, 0 invited".to_owned(),
            "* Syncing with the homeserver".to_owned(),
            e2ee.to_owned(),
            setup_hint.to_owned(),
        ]
    );

    invoke_account_menu_command(&app, MENU_ID_FILE_DISCONNECT)?;
    wait_for_status_line(&app, "* Disconnected", TIMEOUT)?;
    let before = status_texts(&app)?.len() - 1;
    outlast_the_abandoned_long_poll();
    invoke_account_menu_command(&app, MENU_ID_FILE_CONNECT)?;
    assert_connects_at_once(&app, "File > Connect")?;
    let_background_tasks_settle(&app)?;
    assert_eq!(
        status_texts(&app)?[before..],
        ["* Disconnected", "* Syncing with the homeserver"]
    );

    finish(app, &local_app_data)
}

/// The recovery passphrase L6 sets up and recovers with.
const RECOVERY_PASSPHRASE: &str = "live lane recovery passphrase";

/// Waits until mirx's E2EE status satisfies `check` and returns it.
fn wait_for_e2ee_status(
    app: &MirxApp,
    what: &str,
    check: impl Fn(&E2eeStatus) -> bool,
) -> Result<E2eeStatus, Box<dyn Error>> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let status = app.dump_model()?.e2ee_status;
        if let Some(status) = status.as_ref().filter(|status| check(status)) {
            return Ok(status.clone());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "the E2EE status never showed {what}: {status:?}; Status: {:?}",
                status_view_lines(app)?
            )
            .into());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Waits for the dialog of `class` the frame owns, opened by a gesture
/// already made, and returns it.
fn wait_for_dialog(app: &MirxApp, class: &str) -> Result<HWND, Box<dyn Error>> {
    let dialog = app.wait_for_modal_dialog(TIMEOUT)?;
    assert_eq!(dialog.class, class, "{dialog:?}");
    Ok(dialog.hwnd as HWND)
}

/// Presses the dialog's button `button_id`, and waits for it to close.
fn press_dialog_button(
    app: &MirxApp,
    dialog: HWND,
    button_id: usize,
    what: &str,
) -> Result<(), Box<dyn Error>> {
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, button_id as WPARAM, 0) },
        0
    );
    wait_for_window_gone(dialog, what)?;
    app.wait_idle(Duration::from_secs(5))?;
    Ok(())
}

/// Accepts the invitation to the room from the treebar menu and waits for
/// the room to say it is encrypted.
fn accept_encrypted_invite(app: &MirxApp, room_id: &RoomId) -> Result<(), Box<dyn Error>> {
    wait_for_room_membership(app, room_id, RoomMembership::Invite)?;
    app.select_room(room_id.as_str())?;
    app.open_selected_navigation_context_menu(ContextMenuCommand::AcceptInvite)?;
    wait_for_room_membership(app, room_id, RoomMembership::Join)?;
    wait_for_view_line(
        app,
        &room_identity(room_id.as_str()),
        "* Messages in this room are end-to-end encrypted",
    )?;
    Ok(())
}

/// Recovery, set up with a passphrase, carries an encrypted room's keys
/// across Log Out: the next sign-in, a new device, reads the room's history
/// once it recovers with the passphrase.
#[test]
fn recovery_carries_the_room_keys_across_log_out() -> Result<(), Box<dyn Error>> {
    let homeserver = homeserver();
    let bob = homeserver.register("bob", "Bob");
    // Alice's phone never verifies mirx, so only the backup holds the key.
    let alice_phone = homeserver.register("alice", "Alice");
    let local_app_data = unique_local_app_data_path();
    let app = launch_live_app(&local_app_data)?;
    let first_device = sign_in(&app, &homeserver, &alice_phone)?;
    // The sign-in hands its password to the SDK's cross-signing bootstrap,
    // which Conduit asks to authenticate.
    wait_for_e2ee_status(&app, "cross-signing complete, recovery off", |status| {
        status
            .cross_signing
            .as_ref()
            .is_some_and(|cross_signing| cross_signing.complete)
            && status.recovery == E2eeRecoveryState::Disabled
    })?;

    app.select_status_view()?;
    app.set_compose_text(&format!("/e2ee setup passphrase {RECOVERY_PASSPHRASE}"))?;
    app.send_compose_key(ComposeKey::Return)?;
    let handoff = wait_for_dialog(&app, "mirxWin32RecoveryKeyHandoffDialog")?;
    press_dialog_button(
        &app,
        handoff,
        RECOVERY_KEY_HANDOFF_DIALOG_ACK_ID,
        "recovery key handoff dialog",
    )?;
    wait_for_e2ee_status(&app, "recovery enabled", |status| {
        status.recovery == E2eeRecoveryState::Enabled
    })?;

    let room_id = bob.create_encrypted_room("Secrets", alice_phone.user_id());
    accept_encrypted_invite(&app, &room_id)?;
    let room = room_identity(room_id.as_str());
    bob.wait_for_member(&room_id, alice_phone.user_id());
    bob.send_text(&room_id, "kept across log out");
    wait_for_view_line(&app, &room, "<Bob> kept across log out")?;

    // At once: the key came in the sync that brought the line, and may not
    // be in the backup yet.
    log_out_with_the_menu(&app, &local_app_data)?;

    let second_device = sign_in(&app, &homeserver, &alice_phone)?;
    assert_ne!(second_device, first_device);
    let undecryptable = wait_for_view_line(&app, &room, "could not be decrypted")?;
    assert!(
        undecryptable.ends_with(
            "* Bob sent an encrypted message that could not be decrypted \
             (sent before this session existed: /verify this session or /e2ee recover to read it)"
        ),
        "{undecryptable:?}"
    );

    app.open_dialog(AutomationDialog::RecoverE2ee)?;
    let recover = wait_for_dialog(&app, "mirxWin32RecoverE2eeDialog")?;
    let secret = wide_null(RECOVERY_PASSPHRASE);
    assert_ne!(
        unsafe {
            SetWindowTextW(
                GetDlgItem(recover, RECOVER_E2EE_DIALOG_SECRET_ID),
                secret.as_ptr(),
            )
        },
        0
    );
    press_dialog_button(
        &app,
        recover,
        RECOVER_E2EE_DIALOG_OK_ID,
        "Recover E2EE dialog",
    )?;
    wait_for_e2ee_status(&app, "recovery enabled", |status| {
        status.recovery == E2eeRecoveryState::Enabled
    })?;
    wait_for_view_line(&app, &room, "<Bob> kept across log out")?;
    let ui_state = app.dump_ui_state()?;
    let lines = &find_view(&ui_state, &room).lines;
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("could not be decrypted")),
        "{lines:?}"
    );

    finish(app, &local_app_data)
}

/// Waits until mirx's ignore list, the server's last echo, is `expected`.
fn wait_for_ignored_users(app: &MirxApp, expected: &[String]) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let ignored_users = app.dump_ui_state()?.ignored_users;
        if ignored_users.as_deref() == Some(expected) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "the ignore list never became {expected:?}: {ignored_users:?}; Status: {:?}",
                status_texts(app)?
            )
            .into());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Two ignore-list changes typed in a row both stay: the second is sent
/// before the server's echo of the first reaches mirx, so it must not edit
/// the stored copy of the list.
#[test]
fn ignoring_two_users_in_a_row_keeps_both() -> Result<(), Box<dyn Error>> {
    let homeserver = homeserver();
    let bob = homeserver.register("bob", "Bob");
    let carol = homeserver.register("carol", "Carol");
    let alice = homeserver.register("alice", "Alice");
    let local_app_data = unique_local_app_data_path();
    let app = launch_live_app(&local_app_data)?;
    sign_in(&app, &homeserver, &alice)?;
    wait_for_ignored_users(&app, &[])?;

    app.select_status_view()?;
    type_commands(
        &app,
        &[
            &format!("/ignore {}", bob.user_id()),
            &format!("/ignore {}", carol.user_id()),
        ],
    )?;
    let both = vec![bob.user_id().to_string(), carol.user_id().to_string()];
    let deadline = Instant::now() + TIMEOUT;
    let mut on_the_server = alice.ignored_users();
    while !on_the_server.contains(&carol.user_id().to_string()) {
        assert!(
            Instant::now() < deadline,
            "the server never listed Carol: {on_the_server:?}"
        );
        std::thread::sleep(POLL_INTERVAL);
        on_the_server = alice.ignored_users();
    }
    assert_eq!(on_the_server, both, "Status: {:?}", status_texts(&app)?);
    wait_for_ignored_users(&app, &both)?;

    finish(app, &local_app_data)
}
