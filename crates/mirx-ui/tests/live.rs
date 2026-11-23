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

use std::collections::BTreeMap;
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
use matrix_sdk::ruma::api::client::directory::get_room_visibility;
use matrix_sdk::ruma::api::client::room::Visibility;
use matrix_sdk::ruma::api::client::room::create_room::v3::{
    Request as CreateRoomRequest, RoomPreset,
};
use matrix_sdk::ruma::api::client::state::get_state_event_for_key;
use matrix_sdk::ruma::api::client::uiaa::{
    AuthData, Dummy, MatrixUserIdentifier, Password, UserIdentifier,
};
use matrix_sdk::ruma::events::direct::DirectEventContent;
use matrix_sdk::ruma::events::ignored_user_list::IgnoredUserListEventContent;
use matrix_sdk::ruma::events::room::encryption::RoomEncryptionEventContent;
use matrix_sdk::ruma::events::room::member::MembershipState;
use matrix_sdk::ruma::events::room::message::{
    OriginalSyncRoomMessageEvent, RoomMessageEventContent,
};
use matrix_sdk::ruma::events::room::power_levels::RoomPowerLevelsEventContent;
use matrix_sdk::ruma::events::{InitialStateEvent, StateEventType, TimelineEventType};
use matrix_sdk::ruma::{DeviceId, OwnedDeviceId, OwnedRoomId, OwnedUserId, RoomId, UserId};
use mirx_core::{E2eeRecoveryState, E2eeStatus, RoomMemberRank, RoomMembership};
use mirx_testkit::{
    AutomationDialog, ComposeKey, ContextMenuCommand, MirxApp, UiChannelCentral, UiCheckState,
    UiSessions, ViewIdentity,
};
use support::*;
use windows_sys::Win32::Foundation::{HWND, WPARAM};
use windows_sys::Win32::UI::Controls::BST_CHECKED;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_SETCHECK, GetDlgItem, IDCANCEL, IDOK, PostMessageW, SendMessageW, SetWindowTextW, WM_COMMAND,
};

/// Every live user's password.
const PASSWORD: &str = "live-lane-password";
/// How long mirx and the peers get to see a change on the server: Wine in
/// a VM is slow, and a change can wait out a whole 30 s `/sync` long poll.
const TIMEOUT: Duration = Duration::from_secs(90);
/// The Recover E2EE dialog's secret field and its OK button.
const RECOVER_E2EE_DIALOG_SECRET_ID: i32 = 5007;
const RECOVER_E2EE_DIALOG_OK_ID: usize = 5008;
/// The Address Book's tabs, its Control tab's list and buttons, and the
/// Add Ignore dialog's nickname and OK.
const ADDRESS_BOOK_TAB_ID: i32 = 7001;
const ADDRESS_BOOK_CONTROL_LIST_ID: i32 = 7202;
const ADDRESS_BOOK_CONTROL_ADD_ID: WPARAM = 7203;
const ADDRESS_BOOK_CONTROL_DELETE_ID: WPARAM = 7204;
const ADDRESS_ENTRY_NICKNAME_ID: i32 = 7301;
const ADDRESS_ENTRY_OK_ID: usize = 7304;
/// The recovery key handoff dialog's "I Saved It".
const RECOVERY_KEY_HANDOFF_DIALOG_ACK_ID: usize = 5018;
/// Channel Central's fields, Bans List, Remove and mode checks.
const CHANNEL_CENTRAL_NAME_ID: i32 = 7601;
const CHANNEL_CENTRAL_TOPIC_ID: i32 = 7602;
const CHANNEL_CENTRAL_BANS_ID: i32 = 7603;
const CHANNEL_CENTRAL_REMOVE_ID: WPARAM = 7604;
const CHANNEL_CENTRAL_MODE_IDS: [i32; 4] = [7605, 7606, 7607, 7608];
/// The Sessions dialog's list and buttons, the Rename Session prompt's
/// field and OK, and the Sign Out Session dialog's password and button.
const SESSIONS_LIST_ID: i32 = 7701;
const SESSIONS_RENAME_ID: WPARAM = 7703;
const SESSIONS_SIGN_OUT_ID: WPARAM = 7704;
const SINGLE_FIELD_DIALOG_EDIT_ID: i32 = 5010;
const SINGLE_FIELD_DIALOG_OK_ID: usize = 5011;
const SIGN_OUT_SESSION_DIALOG_PASSWORD_ID: i32 = 5019;
const SIGN_OUT_SESSION_DIALOG_SIGN_OUT_ID: usize = 5020;

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

    /// Creates a direct room with `user`, invited, as Element does.
    fn create_dm(&self, user: &UserId) -> OwnedRoomId {
        self.homeserver
            .runtime
            .block_on(self.client.create_dm(user))
            .expect("creating a direct room succeeds")
            .room_id()
            .to_owned()
    }

    /// The account's direct rooms as the server has them (`m.direct`):
    /// each user's rooms, as `user ID -> room IDs`.
    fn direct_rooms(&self) -> BTreeMap<String, Vec<String>> {
        self.homeserver.runtime.block_on(async {
            let raw = self
                .client
                .account()
                .fetch_account_data_static::<DirectEventContent>()
                .await
                .expect("reading m.direct succeeds");
            raw.map(|raw| raw.deserialize().expect("m.direct decodes"))
                .unwrap_or_default()
                .iter()
                .map(|(user, rooms)| {
                    (
                        user.to_string(),
                        rooms.iter().map(ToString::to_string).collect(),
                    )
                })
                .collect()
        })
    }

    /// Sets `user`'s power level in the room, editing the server's power
    /// levels.
    fn set_power_level(&self, room_id: &RoomId, user: &UserId, level: i32) {
        let mut content = self.server_power_levels_content(room_id);
        content.users.insert(user.to_owned(), level.into());
        self.homeserver
            .runtime
            .block_on(self.room(room_id).send_state_event(content).into_future())
            .expect("setting a power level succeeds");
    }

    fn join(&self, room_id: &RoomId) {
        self.homeserver
            .runtime
            .block_on(self.client.join_room_by_id(room_id))
            .unwrap_or_else(|error| panic!("{} could not join {room_id}: {error}", self.localpart));
    }

    fn server_power_levels_content(&self, room_id: &RoomId) -> RoomPowerLevelsEventContent {
        let request = get_state_event_for_key::v3::Request::new(
            room_id.to_owned(),
            StateEventType::RoomPowerLevels,
            String::new(),
        );
        self.homeserver
            .runtime
            .block_on(self.client.send(request).into_future())
            .expect("reading the power levels succeeds")
            .into_content()
            .deserialize_as_unchecked::<RoomPowerLevelsEventContent>()
            .expect("the power levels decode")
    }

    /// The room's `users` power levels as the server has them.
    fn server_power_levels(&self, room_id: &RoomId) -> BTreeMap<String, i64> {
        self.server_power_levels_content(room_id)
            .users
            .iter()
            .map(|(user, level)| (user.to_string(), i64::from(*level)))
            .collect()
    }

    /// The content of the room's state event of `event_type` and
    /// `state_key`, as the server has it.
    fn room_state(
        &self,
        room_id: &RoomId,
        event_type: StateEventType,
        state_key: &str,
    ) -> serde_json::Value {
        let request = get_state_event_for_key::v3::Request::new(
            room_id.to_owned(),
            event_type.clone(),
            state_key.to_owned(),
        );
        self.homeserver
            .runtime
            .block_on(self.client.send(request).into_future())
            .unwrap_or_else(|error| panic!("reading {event_type} {state_key:?} failed: {error}"))
            .into_content()
            .deserialize_as_unchecked::<serde_json::Value>()
            .expect("state content is JSON")
    }

    /// Whether the room is listed in the server's room directory.
    fn directory_visibility(&self, room_id: &RoomId) -> Visibility {
        self.homeserver
            .runtime
            .block_on(
                self.client
                    .send(get_room_visibility::v3::Request::new(room_id.to_owned()))
                    .into_future(),
            )
            .expect("reading the directory listing succeeds")
            .visibility
    }

    /// Sends a message the server may refuse; its error.
    fn try_send_text(&self, room_id: &RoomId, body: &str) -> Result<(), String> {
        let room = self.room(room_id);
        self.homeserver
            .runtime
            .block_on(
                room.send(RoomMessageEventContent::text_plain(body))
                    .into_future(),
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
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

    /// The name the server has for one of the user's devices.
    fn device_display_name(&self, device_id: &DeviceId) -> Option<String> {
        self.homeserver
            .runtime
            .block_on(self.client.devices())
            .expect("listing devices succeeds")
            .devices
            .into_iter()
            .find(|device| device.device_id == device_id)
            .unwrap_or_else(|| panic!("{device_id} is listed"))
            .display_name
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

/// Waits until the server's copy of `peer`'s ignore list holds
/// `expected`, as it settles after mirx's writes.
fn wait_for_server_ignore_list(
    app: &MirxApp,
    peer: &Peer,
    expected: &[String],
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + TIMEOUT;
    let mut on_the_server = peer.ignored_users();
    while on_the_server != expected {
        if Instant::now() >= deadline {
            return Err(format!(
                "the server's ignore list never became {expected:?}: {on_the_server:?}; \
                 Status: {:?}",
                status_texts(app)?
            )
            .into());
        }
        std::thread::sleep(POLL_INTERVAL);
        on_the_server = peer.ignored_users();
    }
    Ok(())
}

/// The Address Book's Control tab, opened on the server's list: the first
/// row selected and deleted.
fn delete_first_ignore_row(app: &MirxApp, dialog: HWND) -> Result<(), Box<dyn Error>> {
    let book = app
        .dump_ui_state()?
        .address_book
        .ok_or("the Address Book")?;
    let rows = book.control_rows.len();
    let list = unsafe { GetDlgItem(dialog, ADDRESS_BOOK_CONTROL_LIST_ID) };
    post_left_click(list, &book.control_row_click_points[0], false);
    app.wait_for_ui_state("a selected row", TIMEOUT, |ui_state| {
        ui_state
            .address_book
            .as_ref()
            .is_some_and(|book| book.control_delete_enabled)
    })?;
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, ADDRESS_BOOK_CONTROL_DELETE_ID, 0) },
        0
    );
    app.wait_for_ui_state("a deleted row", TIMEOUT, |ui_state| {
        ui_state
            .address_book
            .as_ref()
            .is_some_and(|book| book.control_rows.len() == rows - 1)
    })?;
    Ok(())
}

/// Ignore-list changes in a row all stay: each is sent before the server's
/// echo of the last reaches mirx, so none may edit the stored copy of the
/// list. Two typed `/ignore`s, then the Address Book's three at once.
#[test]
fn ignore_list_changes_in_a_row_all_stay() -> Result<(), Box<dyn Error>> {
    let homeserver = homeserver();
    let bob = homeserver.register("bob", "Bob");
    let carol = homeserver.register("carol", "Carol");
    let dave = homeserver.register("dave", "Dave");
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
    wait_for_server_ignore_list(&app, &alice, &both)?;
    wait_for_ignored_users(&app, &both)?;

    // Tools > Address Book, its Control tab: delete both, add Dave, OK.
    app.open_dialog(AutomationDialog::AddressBook)?;
    let dialog = wait_for_dialog(&app, "mirxWin32AddressBookDialog")?;
    let book = app
        .dump_ui_state()?
        .address_book
        .ok_or("the Address Book")?;
    let tab_control = unsafe { GetDlgItem(dialog, ADDRESS_BOOK_TAB_ID) };
    post_left_click(tab_control, &book.tab_click_points[1], false);
    app.wait_for_ui_state("the Control tab", TIMEOUT, |ui_state| {
        ui_state
            .address_book
            .as_ref()
            .is_some_and(|book| book.tab == "Control" && book.control_rows.len() == 2)
    })?;
    delete_first_ignore_row(&app, dialog)?;
    delete_first_ignore_row(&app, dialog)?;
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, ADDRESS_BOOK_CONTROL_ADD_ID, 0) },
        0
    );
    let entry = wait_for_window(&wide_null("mirxWin32AddressEntryDialog"), "Add Ignore")?;
    let nickname = wide_null(dave.user_id().as_str());
    assert_ne!(
        unsafe {
            SetWindowTextW(
                GetDlgItem(entry, ADDRESS_ENTRY_NICKNAME_ID),
                nickname.as_ptr(),
            )
        },
        0
    );
    press_dialog_button(&app, entry, ADDRESS_ENTRY_OK_ID, "Add Ignore")?;
    press_dialog_button(&app, dialog, IDOK as usize, "Address Book")?;
    let only_dave = vec![dave.user_id().to_string()];
    wait_for_server_ignore_list(&app, &alice, &only_dave)?;
    wait_for_ignored_users(&app, &only_dave)?;

    finish(app, &local_app_data)
}

/// Waits until `check` holds for what `read` returns, as the server
/// settles after mirx's writes; on timeout, says what it read last.
fn wait_for_server<T: std::fmt::Debug>(
    app: &MirxApp,
    what: &str,
    mut read: impl FnMut() -> T,
    check: impl Fn(&T) -> bool,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + TIMEOUT;
    let mut on_the_server = read();
    while !check(&on_the_server) {
        if Instant::now() >= deadline {
            return Err(format!(
                "the server never had {what}: {on_the_server:?}; Status: {:?}",
                status_texts(app)?
            )
            .into());
        }
        std::thread::sleep(POLL_INTERVAL);
        on_the_server = read();
    }
    Ok(())
}

/// Two direct-room invitations accepted back to back both stay direct
/// rooms of the account: each join adds its room to `m.direct`, and the
/// second must not write over the first.
#[test]
fn accepting_two_direct_invites_keeps_both_direct() -> Result<(), Box<dyn Error>> {
    let homeserver = homeserver();
    let bob = homeserver.register("bob", "Bob");
    let carol = homeserver.register("carol", "Carol");
    let alice = homeserver.register("alice", "Alice");
    let local_app_data = unique_local_app_data_path();
    let app = launch_live_app(&local_app_data)?;
    sign_in(&app, &homeserver, &alice)?;
    let with_bob = bob.create_dm(alice.user_id());
    let with_carol = carol.create_dm(alice.user_id());
    wait_for_room_membership(&app, &with_bob, RoomMembership::Invite)?;
    wait_for_room_membership(&app, &with_carol, RoomMembership::Invite)?;

    app.select_status_view()?;
    type_commands(
        &app,
        &[
            &format!("/accept {with_bob}"),
            &format!("/accept {with_carol}"),
        ],
    )?;
    wait_for_room_membership(&app, &with_bob, RoomMembership::Join)?;
    wait_for_room_membership(&app, &with_carol, RoomMembership::Join)?;
    let both = BTreeMap::from([
        (bob.user_id().to_string(), vec![with_bob.to_string()]),
        (carol.user_id().to_string(), vec![with_carol.to_string()]),
    ]);
    wait_for_server(
        &app,
        &format!("m.direct {both:?}"),
        || alice.direct_rooms(),
        |direct| *direct == both,
    )?;
    // And mirx shows both as queries once the sync echoes it.
    app.wait_for_ui_state("both rooms direct", TIMEOUT, |ui_state| {
        [(&with_bob, &bob), (&with_carol, &carol)]
            .iter()
            .all(|(room_id, peer)| {
                ui_state.rooms.iter().any(|room| {
                    room.room_id == room_id.as_str()
                        && room.is_direct
                        && room.direct_user_ids == [peer.user_id().to_string()]
                })
            })
    })?;

    finish(app, &local_app_data)
}

/// The members of the room's window in mirx, as `user ID -> rank`.
fn nick_list_ranks(
    app: &MirxApp,
    room_id: &RoomId,
) -> Result<BTreeMap<String, RoomMemberRank>, Box<dyn Error>> {
    let ui_state = app.dump_ui_state()?;
    Ok(find_view(&ui_state, &room_identity(room_id.as_str()))
        .members
        .iter()
        .map(|member| (member.user_id.clone(), member.rank))
        .collect())
}

fn wait_for_nick_list(
    app: &MirxApp,
    room_id: &RoomId,
    expected: &BTreeMap<String, RoomMemberRank>,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let ranks = nick_list_ranks(app, room_id)?;
        if ranks == *expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("the nick list never became {expected:?}: {ranks:?}").into());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Rank changes typed in a row all stay (each is sent before the server's
/// echo of the last), and several nicks in one command change in one
/// write, shown as one line.
#[test]
fn rank_changes_in_a_row_and_together_all_stay() -> Result<(), Box<dyn Error>> {
    let homeserver = homeserver();
    let bob = homeserver.register("bob", "Bob");
    let carol = homeserver.register("carol", "Carol");
    let dave = homeserver.register("dave", "Dave");
    let alice = homeserver.register("alice", "Alice");
    let alias_localpart = unique_name("ranks");
    let room_id = bob.create_public_room("Ranks", &alias_localpart);
    bob.set_power_level(&room_id, alice.user_id(), 100);
    carol.join(&room_id);
    dave.join(&room_id);
    let local_app_data = unique_local_app_data_path();
    let app = launch_live_app(&local_app_data)?;
    sign_in(&app, &homeserver, &alice)?;
    join_by_address(&app, &format!("#{alias_localpart}:localhost"), &room_id)?;
    let (alice_id, bob_id, carol_id, dave_id) = (
        alice.user_id().to_string(),
        bob.user_id().to_string(),
        carol.user_id().to_string(),
        dave.user_id().to_string(),
    );
    let ranks = |carol_rank, dave_rank| {
        BTreeMap::from([
            (alice_id.clone(), RoomMemberRank::Administrator),
            // Conduit's rooms are version 12: their creator's level is
            // infinite, and not in the power levels.
            (bob_id.clone(), RoomMemberRank::Creator),
            (carol_id.clone(), carol_rank),
            (dave_id.clone(), dave_rank),
        ])
    };
    wait_for_nick_list(
        &app,
        &room_id,
        &ranks(RoomMemberRank::Member, RoomMemberRank::Member),
    )?;
    let levels = |carol_level: Option<i64>, dave_level: Option<i64>| {
        let mut levels = BTreeMap::from([(alice_id.clone(), 100)]);
        levels.extend(carol_level.map(|level| (carol_id.clone(), level)));
        levels.extend(dave_level.map(|level| (dave_id.clone(), level)));
        levels
    };

    let room = room_identity(room_id.as_str());
    app.select_view(room.clone())?;
    type_commands(&app, &["/op Carol", "/voice Dave"])?;
    let opped_and_voiced = levels(Some(50), Some(1));
    wait_for_server(
        &app,
        &format!("power levels {opped_and_voiced:?}"),
        || bob.server_power_levels(&room_id),
        |users| *users == opped_and_voiced,
    )?;
    wait_for_nick_list(
        &app,
        &room_id,
        &ranks(RoomMemberRank::Moderator, RoomMemberRank::Voiced),
    )?;

    type_commands(&app, &["/deop Carol"])?;
    wait_for_nick_list(
        &app,
        &room_id,
        &ranks(RoomMemberRank::Member, RoomMemberRank::Voiced),
    )?;
    type_commands(&app, &["/op Carol Dave"])?;
    let both_opped = levels(Some(50), Some(50));
    wait_for_server(
        &app,
        &format!("power levels {both_opped:?}"),
        || bob.server_power_levels(&room_id),
        |users| *users == both_opped,
    )?;
    // One write, one line, for both.
    wait_for_view_line(
        &app,
        &room,
        "* Alice sets power level: Carol 0 -> 50, Dave 1 -> 50",
    )?;

    finish(app, &local_app_data)
}

/// Opens Channel Central with `/channel` in the active room and waits for
/// the server's settings in it.
fn open_channel_central(app: &MirxApp) -> Result<(HWND, UiChannelCentral), Box<dyn Error>> {
    app.set_compose_text("/channel")?;
    app.open_dialog(AutomationDialog::ComposeReturn)?;
    let dialog = wait_for_dialog(app, "mirxWin32ChannelCentralDialog")?;
    let ui_state = app.wait_for_ui_state("Channel Central's settings", TIMEOUT, |ui_state| {
        ui_state
            .channel_central
            .as_ref()
            .is_some_and(|central| central.ok_enabled)
    })?;
    Ok((dialog, ui_state.channel_central.expect("waited for above")))
}

fn set_dialog_text(dialog: HWND, id: i32, text: &str) {
    let text = wide_null(text);
    assert_ne!(
        unsafe { SetWindowTextW(GetDlgItem(dialog, id), text.as_ptr()) },
        0
    );
}

/// Channel Central changes the room on the server: its creator (v12,
/// Conduit) renames it, sets the topic, makes it ops-only for the topic,
/// moderated, invite only and unlisted, and lifts a ban, all in one OK.
#[test]
fn channel_central_changes_the_room_on_the_server() -> Result<(), Box<dyn Error>> {
    let homeserver = homeserver();
    let bob = homeserver.register("bob", "Bob");
    let carol = homeserver.register("carol", "Carol");
    let alice = homeserver.register("alice", "Alice");
    let local_app_data = unique_local_app_data_path();
    let app = launch_live_app(&local_app_data)?;
    sign_in(&app, &homeserver, &alice)?;

    let alias_localpart = unique_name("central");
    let room_name = unique_name("Central");
    app.select_status_view()?;
    type_commands(
        &app,
        &[&format!("/create -public #{alias_localpart} {room_name}")],
    )?;
    let ui_state = app.wait_for_ui_state("the created room", TIMEOUT, |ui_state| {
        ui_state
            .rooms
            .iter()
            .any(|room| room.display_name == room_name)
    })?;
    let room_id = OwnedRoomId::try_from(
        ui_state
            .rooms
            .iter()
            .find(|room| room.display_name == room_name)
            .expect("waited for above")
            .room_id
            .as_str(),
    )?;
    bob.join(&room_id);
    carol.join(&room_id);
    let room = room_identity(room_id.as_str());
    // The room's creation prints its settings.
    wait_for_view_line(&app, &room, "* Alice makes the room public")?;
    wait_for_view_line(
        &app,
        &room,
        &format!("* Alice changes room name to '{room_name}'"),
    )?;
    wait_for_view_line(&app, &room, "Carol")?;
    app.select_view(room.clone())?;
    type_commands(&app, &["/ban Carol spam"])?;
    bob.sync_until("Carol banned", async || {
        let carol_member = bob
            .room(&room_id)
            .get_member_no_sync(carol.user_id())
            .await
            .expect("the store reads members");
        match carol_member {
            Some(member) if *member.membership() == MembershipState::Ban => Ok(()),
            other => Err(format!(
                "{:?}",
                other.map(|member| member.membership().clone())
            )),
        }
    });

    let (dialog, central) = open_channel_central(&app)?;
    // The creator may change everything.
    assert_eq!(central.status, "");
    assert_eq!(central.name, room_name);
    assert_eq!(central.ban_rows.len(), 1, "{:?}", central.ban_rows);
    let ban = &central.ban_rows[0];
    assert!(ban[0].contains(carol.user_id().as_str()), "{ban:?}");
    assert_eq!((ban[1].as_str(), ban[3].as_str()), ("Alice", "spam"));
    // Public, and listed in the Rooms List.
    assert_eq!(central.invite_only.state, UiCheckState::Unchecked);
    assert_eq!(central.private.state, UiCheckState::Unchecked);
    assert_eq!(central.moderated.state, UiCheckState::Unchecked);
    assert!(
        [
            central.topic_by_operators,
            central.invite_only,
            central.moderated,
            central.private
        ]
        .iter()
        .all(|check| check.enabled)
    );

    set_dialog_text(dialog, CHANNEL_CENTRAL_NAME_ID, "Renamed");
    set_dialog_text(dialog, CHANNEL_CENTRAL_TOPIC_ID, "only ops talk here");
    for id in CHANNEL_CENTRAL_MODE_IDS {
        unsafe {
            SendMessageW(
                GetDlgItem(dialog, id),
                BM_SETCHECK,
                BST_CHECKED as WPARAM,
                0,
            )
        };
    }
    let bans = unsafe { GetDlgItem(dialog, CHANNEL_CENTRAL_BANS_ID) };
    post_left_click(bans, &central.ban_row_click_points[0], false);
    app.wait_for_ui_state("the ban selected", TIMEOUT, |ui_state| {
        ui_state
            .channel_central
            .as_ref()
            .is_some_and(|central| central.remove_enabled)
    })?;
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, CHANNEL_CENTRAL_REMOVE_ID, 0) },
        0
    );
    app.wait_for_ui_state("the ban removed", TIMEOUT, |ui_state| {
        ui_state
            .channel_central
            .as_ref()
            .is_some_and(|central| central.ban_rows.is_empty())
    })?;
    press_dialog_button(&app, dialog, IDOK as usize, "Channel Central")?;

    // The server has every part, as Bob reads it.
    let state = |event_type: StateEventType, state_key: &str| {
        bob.room_state(&room_id, event_type, state_key)
    };
    wait_for_server(
        &app,
        "Carol unbanned",
        || state(StateEventType::RoomMember, carol.user_id().as_str()),
        |member| member["membership"] == "leave",
    )?;
    assert_eq!(state(StateEventType::RoomName, "")["name"], "Renamed");
    assert_eq!(
        state(StateEventType::RoomTopic, "")["topic"],
        "only ops talk here"
    );
    assert_eq!(
        state(StateEventType::RoomJoinRules, "")["join_rule"],
        "invite"
    );
    let levels = bob.server_power_levels_content(&room_id);
    assert!(i64::from(levels.events_default) >= 1, "{levels:?}");
    let topic_level = levels
        .events
        .get(&"m.room.topic".into())
        .map_or(levels.state_default, |level| *level);
    assert!(i64::from(topic_level) >= 50, "{levels:?}");
    assert_eq!(bob.directory_visibility(&room_id), Visibility::Private);
    // Moderated: Bob, at the default level, may not speak.
    let refused = bob.try_send_text(&room_id, "may I?");
    assert!(refused.is_err(), "{refused:?}");

    wait_for_view_line(&app, &room, "* Alice changes room name to 'Renamed'")?;
    wait_for_view_line(&app, &room, "* Alice makes the room invite only")?;
    // Read again from the server.
    let (dialog, central) = open_channel_central(&app)?;
    assert_eq!(central.name, "Renamed");
    assert_eq!(central.topic, "only ops talk here");
    assert!(central.ban_rows.is_empty(), "{:?}", central.ban_rows);
    for check in [
        central.topic_by_operators,
        central.invite_only,
        central.moderated,
        central.private,
    ] {
        assert_eq!(check.state, UiCheckState::Checked);
    }
    press_dialog_button(&app, dialog, IDCANCEL as usize, "Channel Central")?;

    finish(app, &local_app_data)
}

/// `/mode` changes the room's modes and a member's rank in one power-levels
/// write, and the echo prints mIRC's mode line; a join rule typed in Status
/// for a named room changes that room.
#[test]
fn mode_changes_the_room_and_ranks_in_one_write() -> Result<(), Box<dyn Error>> {
    let homeserver = homeserver();
    let bob = homeserver.register("bob", "Bob");
    let carol = homeserver.register("carol", "Carol");
    let dave = homeserver.register("dave", "Dave");
    let alice = homeserver.register("alice", "Alice");
    let alias_localpart = unique_name("modes");
    let room_id = bob.create_public_room("Modes", &alias_localpart);
    bob.set_power_level(&room_id, alice.user_id(), 100);
    carol.join(&room_id);
    dave.join(&room_id);
    // The server's new room: only ops set the topic, not moderated.
    let created = bob.server_power_levels_content(&room_id);
    let topic_level = created
        .events
        .get(&TimelineEventType::RoomTopic)
        .copied()
        .unwrap_or(created.state_default);
    assert!(topic_level > created.users_default, "{created:?}");
    assert!(
        created.events_default <= created.users_default,
        "{created:?}"
    );
    let local_app_data = unique_local_app_data_path();
    let app = launch_live_app(&local_app_data)?;
    sign_in(&app, &homeserver, &alice)?;
    let alias = format!("#{alias_localpart}:localhost");
    join_by_address(&app, &alias, &room_id)?;
    let alice_id = alice.user_id().to_string();
    let room = room_identity(room_id.as_str());
    // The nicks resolve in the nick list.
    wait_for_nick_list(
        &app,
        &room_id,
        &BTreeMap::from([
            (alice_id.clone(), RoomMemberRank::Administrator),
            (bob.user_id().to_string(), RoomMemberRank::Creator),
            (carol.user_id().to_string(), RoomMemberRank::Member),
            (dave.user_id().to_string(), RoomMemberRank::Member),
        ]),
    )?;

    app.select_view(room.clone())?;
    type_commands(&app, &["/mode +m-t+v Dave"])?;
    wait_for_server(
        &app,
        "a moderated room with Dave voiced",
        || bob.server_power_levels_content(&room_id),
        |levels| {
            levels.events_default > levels.users_default
                && levels.users.get(dave.user_id()) == Some(&1.into())
                && levels.users.get(alice.user_id()) == Some(&100.into())
        },
    )?;
    wait_for_view_line(
        &app,
        &room,
        "* Alice sets mode: +m-t; power level: Dave 0 -> 1",
    )?;
    assert!(carol.try_send_text(&room_id, "carol is muted").is_err());
    dave.try_send_text(&room_id, "dave has voice")
        .map_err(|error| format!("a voiced member speaks: {error}"))?;

    type_commands(&app, &["/mode -m+t-v Dave"])?;
    wait_for_server(
        &app,
        "an unmoderated room without Dave's voice",
        || bob.server_power_levels(&room_id),
        |users| *users == BTreeMap::from([(alice_id.clone(), 100)]),
    )?;
    wait_for_view_line(
        &app,
        &room,
        "* Alice sets mode: +t-m; power level: Dave 1 -> 0",
    )?;
    carol
        .try_send_text(&room_id, "carol speaks again")
        .map_err(|error| format!("an unmoderated room takes messages: {error}"))?;

    // Typed in Status, for the room it names.
    app.select_status_view()?;
    type_commands(&app, &[&format!("/mode {alias} +i")])?;
    wait_for_view_line(&app, &room, "* Alice makes the room invite only")?;

    finish(app, &local_app_data)
}

/// Waits until the open Sessions dialog's state passes `check`.
fn wait_for_sessions(
    app: &MirxApp,
    what: &str,
    check: impl Fn(&UiSessions) -> bool,
) -> Result<UiSessions, Box<dyn Error>> {
    let ui_state = app.wait_for_ui_state(what, TIMEOUT, |ui_state| {
        ui_state.sessions.as_ref().is_some_and(&check)
    })?;
    Ok(ui_state.sessions.expect("waited for above"))
}

/// The Sessions dialog renames another of the account's sessions and signs
/// it out, on the server: the peer is Alice's phone, mirx her second
/// device. The server's name for the phone is read by the phone itself;
/// once signed out, the phone's own token is refused.
#[test]
fn sessions_dialog_renames_and_signs_out_another_session() -> Result<(), Box<dyn Error>> {
    let homeserver = homeserver();
    let alice_phone = homeserver.register("alice", "Alice");
    let phone = alice_phone.device_id().to_string();
    let local_app_data = unique_local_app_data_path();
    let app = launch_live_app(&local_app_data)?;
    let device = sign_in(&app, &homeserver, &alice_phone)?;

    app.open_dialog(AutomationDialog::Sessions)?;
    let dialog = wait_for_dialog(&app, "mirxWin32SessionsDialog")?;
    let shown = wait_for_sessions(&app, "both sessions", |sessions| sessions.rows.len() == 2)?;
    assert_eq!(shown.rows[0][0], device, "{shown:?}");
    assert_eq!(shown.rows[0][3], "this session", "{shown:?}");
    assert_eq!(shown.rows[1][0], phone, "{shown:?}");
    let list = unsafe { GetDlgItem(dialog, SESSIONS_LIST_ID) };
    post_left_click(list, &shown.row_click_points[1], false);
    wait_for_sessions(&app, "the phone selected", |sessions| {
        sessions.selected == Some(1) && sessions.sign_out_enabled
    })?;

    // Rename... asks for the name over the dialog.
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, SESSIONS_RENAME_ID, 0) },
        0
    );
    let prompt = wait_for_window(&wide_null("mirxWin32SingleFieldDialog"), "Rename Session")?;
    set_dialog_text(prompt, SINGLE_FIELD_DIALOG_EDIT_ID, "Old phone");
    press_dialog_button(&app, prompt, SINGLE_FIELD_DIALOG_OK_ID, "Rename Session")?;
    wait_for_server(
        &app,
        "the phone's new name",
        || alice_phone.device_display_name(phone.as_str().into()),
        |name| name.as_deref() == Some("Old phone"),
    )?;
    wait_for_sessions(&app, "the new name listed", |sessions| {
        sessions
            .rows
            .get(1)
            .is_some_and(|row| row[1] == "Old phone")
            && sessions.selected == Some(1)
    })?;

    // Sign Out... confirms with the password.
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, SESSIONS_SIGN_OUT_ID, 0) },
        0
    );
    let confirm = wait_for_window(
        &wide_null("mirxWin32SignOutSessionDialog"),
        "Sign Out Session",
    )?;
    set_dialog_text(confirm, SIGN_OUT_SESSION_DIALOG_PASSWORD_ID, PASSWORD);
    press_dialog_button(
        &app,
        confirm,
        SIGN_OUT_SESSION_DIALOG_SIGN_OUT_ID,
        "Sign Out Session",
    )?;
    let shown = wait_for_sessions(&app, "the phone gone", |sessions| sessions.rows.len() == 1)?;
    assert_eq!(shown.rows[0][0], device, "{shown:?}");
    // "; this session is not verified" may follow: no cross-signing here.
    assert!(shown.status.starts_with("1 session"), "{shown:?}");
    wait_for_view_line(
        &app,
        &ViewIdentity::Status,
        &format!("* Session {phone} signed out"),
    )?;
    let refused = homeserver.runtime.block_on(alice_phone.client.devices());
    assert!(
        refused.is_err(),
        "the phone's token still works: {refused:?}"
    );

    press_dialog_button(&app, dialog, IDCANCEL as usize, "Sessions")?;
    finish(app, &local_app_data)
}

/// mIRC's Perform on connect on a real server: the first connect joins
/// the room (its message fails there, before the join lands, and says so
/// in Status), and a reconnect, with the room known, sends the message.
#[test]
fn perform_joins_and_messages_on_connect() -> Result<(), Box<dyn Error>> {
    let homeserver = homeserver();
    let bob = homeserver.register("bob", "Bob");
    let alice = homeserver.register("alice", "Alice");
    let alias_localpart = unique_name("lobby");
    let room_id = bob.create_public_room("Lobby", &alias_localpart);
    let alias = format!("#{alias_localpart}:localhost");
    let local_app_data = unique_local_app_data_path();
    write_settings(
        &local_app_data,
        &format!(
            "[perform]\r\nenabled=on\r\nn0=join {alias}\r\n\
             n1=msg {alias} hello from perform\r\n"
        ),
    )?;
    let app = launch_live_app(&local_app_data)?;
    sign_in(&app, &homeserver, &alice)?;
    bob.wait_for_member(&room_id, alice.user_id());
    let room = room_identity(room_id.as_str());
    wait_for_view_line(&app, &room, "* Now talking in ")?;
    wait_for_status_line(&app, "* /msg: ", TIMEOUT)?;
    let refused = status_texts(&app)?
        .into_iter()
        .filter(|line| line.starts_with("* /msg: "))
        .collect::<Vec<_>>();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(refused[0].ends_with(" (in perform)"), "{refused:?}");

    invoke_account_menu_command(&app, MENU_ID_FILE_DISCONNECT)?;
    wait_for_status_line(&app, "* Disconnected", TIMEOUT)?;
    outlast_the_abandoned_long_poll();
    invoke_account_menu_command(&app, MENU_ID_FILE_CONNECT)?;
    assert_eq!(
        bob.wait_for_text(&room_id, "hello from perform"),
        alice.user_id()
    );
    let_background_tasks_settle(&app)?;
    let ui_state = app.dump_ui_state()?;
    let lines = &find_view(&ui_state, &room).lines;
    assert!(
        !lines.iter().any(|line| line.contains("(in perform)")),
        "{lines:?}"
    );
    let refused = status_texts(&app)?
        .into_iter()
        .filter(|line| line.contains("(in perform)"))
        .count();
    assert_eq!(refused, 1);

    finish(app, &local_app_data)
}
