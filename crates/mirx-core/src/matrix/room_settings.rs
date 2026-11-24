//! A room's settings as Channel Central shows and changes them: name,
//! topic, join rule, who may set the topic, whether only voiced members
//! speak, the room directory listing, and the bans.

use std::collections::BTreeMap;

use matrix_sdk::ruma::api::client::room::Visibility;
use matrix_sdk::ruma::api::client::state::{get_state_event_for_key, get_state_events};
use matrix_sdk::ruma::events::room::join_rules::{JoinRule, OriginalSyncRoomJoinRulesEvent};
use matrix_sdk::ruma::events::room::member::{MembershipState, OriginalSyncRoomMemberEvent};
use matrix_sdk::ruma::events::room::power_levels::{
    RoomPowerLevels, RoomPowerLevelsEventContent, RoomPowerLevelsSource,
};
use matrix_sdk::ruma::events::{AnyStateEvent, StateEventType, TimelineEventType};
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{Int, RoomId, UserId};
use matrix_sdk::{Client, Room};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;

use super::{ErrorChain, MatrixCoreError, joined_room_by_id, room_join_rule};
use crate::api::{
    MemberPowerRole, RoomBan, RoomSettings, RoomSettingsPermissions, UpdateRoomSettingsRequest,
};

/// Moderator in the IRC sense: the conventional Matrix moderator level.
const MODERATOR_POWER_LEVEL: u8 = 50;

/// The room modes the power levels carry: only operators set the topic
/// (`+t`: its level above `users_default`), and moderated (`+m`: both
/// message types above it). The settings read, the write and the
/// power-levels line read them alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LevelModes {
    pub(super) topic_by_operators: bool,
    pub(super) moderated: bool,
}

/// The modes of power levels, from the fields they are made of (an
/// event's content, original or redacted, and ruma's `RoomPowerLevels`
/// all have them).
pub(super) fn level_modes(
    events: &BTreeMap<TimelineEventType, Int>,
    events_default: Int,
    state_default: Int,
    users_default: Int,
) -> LevelModes {
    let level = |event_type, default| events.get(&event_type).copied().unwrap_or(default);
    LevelModes {
        topic_by_operators: level(TimelineEventType::RoomTopic, state_default) > users_default,
        moderated: [
            TimelineEventType::RoomMessage,
            TimelineEventType::RoomEncrypted,
        ]
        .into_iter()
        .all(|message_type| level(message_type, events_default) > users_default),
    }
}

/// An `m.room.name` content, original or redacted: a redaction keeps no
/// key of it, and ruma's `RoomNameEventContent` requires the name.
#[derive(Deserialize)]
struct NameContent {
    name: Option<String>,
}

/// An `m.room.topic` content, original or redacted (see `NameContent`).
#[derive(Deserialize)]
struct TopicContent {
    topic: Option<String>,
}

/// A state event's `content`, which every event has (a redacted one too).
fn state_content<T: DeserializeOwned>(raw: &Raw<AnyStateEvent>) -> Result<T, String> {
    let content = raw
        .get_field::<&RawValue>("content")
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "no content".to_owned())?;
    serde_json::from_str(content.get()).map_err(|err| err.to_string())
}

/// The room's settings from the server's current state (`GET
/// /rooms/{id}/state`) and directory listing. The room's creators and
/// version (which never change) come from the store.
pub(crate) async fn fetch_room_settings(
    client: &Client,
    room_id: &str,
) -> Result<RoomSettings, MatrixCoreError> {
    let room_id = <&RoomId>::try_from(room_id).map_err(|err| {
        MatrixCoreError::new(format!("invalid Matrix room id: {}", ErrorChain(&err)))
    })?;
    let room = joined_room_by_id(client, room_id.as_str())?;
    let load_error =
        |err: String| MatrixCoreError::new(format!("failed to load Matrix room settings: {err}"));

    let state = client
        .send(get_state_events::v3::Request::new(room_id.to_owned()))
        .await
        .map_err(|err| load_error(ErrorChain(&err).to_string()))?
        .room_state;
    let mut name = None;
    let mut topic = None;
    // The spec's default when the room has no join rules.
    let mut join_rule = JoinRule::Invite;
    let mut power_levels = RoomPowerLevelsSource::None;
    let mut bans = Vec::new();
    for raw in state {
        let event_type = raw
            .get_field::<String>("type")
            .map_err(|err| load_error(err.to_string()))?
            .ok_or_else(|| load_error("a state event without a type".to_owned()))?;
        let decode_error = |err: String| load_error(format!("{event_type}: {err}"));
        // Each type is decoded on its own, never as `AnyStateEvent`: its
        // deserializer, for every state event type ruma knows, is 166 KiB
        // of the exe. The room's other state is not decoded.
        match event_type.as_str() {
            "m.room.name" => {
                let content: NameContent = state_content(&raw).map_err(decode_error)?;
                name = content.name.filter(|name| !name.is_empty());
            }
            "m.room.topic" => {
                let content: TopicContent = state_content(&raw).map_err(decode_error)?;
                topic = content.topic;
            }
            "m.room.join_rules" => {
                // A redaction keeps the join rule.
                join_rule = raw
                    .deserialize_as_unchecked::<OriginalSyncRoomJoinRulesEvent>()
                    .map_err(|err| decode_error(err.to_string()))?
                    .content
                    .join_rule;
            }
            "m.room.power_levels" => {
                // A redaction keeps every level the dialog reads (ban, events,
                // events_default, kick, redact, state_default, users,
                // users_default; invite from v11), and every field of the
                // content has a default: a redacted event's content reads
                // as the original's levels.
                power_levels =
                    RoomPowerLevelsSource::Original(state_content(&raw).map_err(decode_error)?);
            }
            "m.room.member" => {
                // A redaction keeps the membership; the other fields are
                // optional, so a redacted ban lists without name and reason.
                let event = raw
                    .deserialize_as_unchecked::<OriginalSyncRoomMemberEvent>()
                    .map_err(|err| decode_error(err.to_string()))?;
                if event.content.membership == MembershipState::Ban {
                    bans.push(RoomBan {
                        user_id: event.state_key.to_string(),
                        display_name: event.content.displayname,
                        banned_by: event.sender.to_string(),
                        reason: event.content.reason,
                        timestamp_ms: event.origin_server_ts.get().into(),
                    });
                }
            }
            _ => {}
        }
    }
    bans.sort_by_key(|ban| std::cmp::Reverse(ban.timestamp_ms));

    let listed_in_directory = room
        .privacy_settings()
        .get_room_visibility()
        .await
        .map_err(|err| load_error(ErrorChain(&err).to_string()))?
        == Visibility::Public;

    let creators = room
        .creators()
        .ok_or_else(|| load_error("the room's creation event is unknown".to_owned()))?;
    let rules = room.clone_info().room_version_rules_or_default();
    let levels = RoomPowerLevels::new(power_levels, &rules.authorization, creators);
    let own = room.own_user_id();
    let can_send = |state_type| levels.user_can_send_state(own, state_type);
    let allowed = RoomSettingsPermissions {
        name: can_send(StateEventType::RoomName),
        topic: can_send(StateEventType::RoomTopic),
        join_rule: can_send(StateEventType::RoomJoinRules),
        power_levels: can_send(StateEventType::RoomPowerLevels),
        // Synapse lets whoever may set the room's aliases publish it.
        directory: can_send(StateEventType::RoomCanonicalAlias),
        unban: levels.user_can_unban(own),
    };

    let modes = level_modes(
        &levels.events,
        levels.events_default,
        levels.state_default,
        levels.users_default,
    );

    Ok(RoomSettings {
        room_id: room_id.to_string(),
        name,
        topic,
        join_rule: room_join_rule(&join_rule),
        topic_by_operators: modes.topic_by_operators,
        moderated: modes.moderated,
        listed_in_directory,
        bans,
        allowed,
    })
}

/// The level a member needs to set the topic in a room where only ops
/// may, the conventional moderator's (as `MODERATOR_POWER_LEVEL`) unless
/// members already have that.
fn operator_topic_level(users_default: Int) -> Int {
    Int::from(MODERATOR_POWER_LEVEL).max(users_default + Int::from(1))
}

/// Edits the levels for "only ops set topic" and "moderated". Turning on
/// what is already on (or off what is off) changes nothing.
///
/// The topic's level is its `events` entry, else `state_default`; on:
/// above `users_default` (the moderator level), off: `users_default`.
/// Moderated is `m.room.message` and `m.room.encrypted` above
/// `users_default`, through `events_default` and their entries; voice
/// (`member_power_role_level`) is then `users_default + 1`.
fn edit_power_levels(
    levels: &mut RoomPowerLevelsEventContent,
    topic_by_operators: Option<bool>,
    moderated: Option<bool>,
) {
    let users_default = levels.users_default;
    let above_default = users_default + Int::from(1);
    let modes = level_modes(
        &levels.events,
        levels.events_default,
        levels.state_default,
        users_default,
    );
    if let Some(topic_by_operators) = topic_by_operators
        && topic_by_operators != modes.topic_by_operators
    {
        let level = if topic_by_operators {
            operator_topic_level(users_default)
        } else {
            users_default
        };
        levels.events.insert(TimelineEventType::RoomTopic, level);
    }
    if let Some(moderated) = moderated {
        let message_types = [
            TimelineEventType::RoomMessage,
            TimelineEventType::RoomEncrypted,
        ];
        if moderated {
            levels.events_default = levels.events_default.max(above_default);
            for message_type in message_types {
                if let Some(level) = levels.events.get_mut(&message_type) {
                    *level = (*level).max(above_default);
                }
            }
        } else {
            levels.events_default = levels.events_default.min(users_default);
            for message_type in message_types {
                if let Some(level) = levels.events.get_mut(&message_type) {
                    *level = (*level).min(users_default);
                }
            }
        }
    }
}

/// The power level that gives `role` in a room with `levels`.
///
/// Voiced is the lowest level above `users_default` that may send
/// `m.room.message`; a room where that level reaches moderator has no
/// distinct voice level.
fn member_power_role_level(
    levels: &RoomPowerLevelsEventContent,
    role: MemberPowerRole,
) -> Result<Int, String> {
    let moderator = Int::from(MODERATOR_POWER_LEVEL);
    match role {
        MemberPowerRole::Moderator => Ok(moderator),
        MemberPowerRole::Default => Ok(levels.users_default),
        MemberPowerRole::Voiced => {
            let message_level = levels
                .events
                .get(&TimelineEventType::RoomMessage)
                .copied()
                .unwrap_or(levels.events_default);
            let level = message_level.max(levels.users_default + Int::from(1));
            if level >= moderator {
                return Err(format!(
                    "room has no voice level below moderator: voice needs power level {level}"
                ));
            }
            Ok(level)
        }
    }
}

/// Writes the request's parts in its field order, stopping at the first
/// the server refuses: the parts before it stay written. The modes and
/// the member roles are one write of the server's power levels (see
/// `write_power_levels`), and none when they change nothing.
pub(crate) async fn update_room_settings(
    client: &Client,
    request: &UpdateRoomSettingsRequest,
) -> Result<(), MatrixCoreError> {
    let room_id = <&RoomId>::try_from(request.room_id.as_str()).map_err(|err| {
        MatrixCoreError::new(format!("invalid Matrix room id: {}", ErrorChain(&err)))
    })?;
    fn user_id(user_id: &str) -> Result<&UserId, MatrixCoreError> {
        <&UserId>::try_from(user_id).map_err(|err| {
            MatrixCoreError::new(format!("invalid Matrix user id: {}", ErrorChain(&err)))
        })
    }
    let member_roles = request
        .member_roles
        .iter()
        .map(|change| Ok((user_id(&change.user_id)?, change.role)))
        .collect::<Result<Vec<_>, MatrixCoreError>>()?;
    let unbans = request
        .unban
        .iter()
        .map(|unbanned| user_id(unbanned))
        .collect::<Result<Vec<_>, _>>()?;
    let UpdateRoomSettingsRequest {
        room_id: _,
        topic_by_operators,
        moderated,
        member_roles: _,
        invite_only,
        name,
        topic,
        listed_in_directory,
        unban: _,
    } = request;
    if topic_by_operators.is_none()
        && moderated.is_none()
        && member_roles.is_empty()
        && invite_only.is_none()
        && name.is_none()
        && topic.is_none()
        && listed_in_directory.is_none()
        && unbans.is_empty()
    {
        return Err(MatrixCoreError::new("no room setting changes"));
    }
    let room = joined_room_by_id(client, room_id.as_str())?;
    let failed = |part: &str, err: String| MatrixCoreError::new(format!("failed to {part}: {err}"));

    if topic_by_operators.is_some() || moderated.is_some() || !member_roles.is_empty() {
        write_power_levels(
            client,
            &room,
            *topic_by_operators,
            *moderated,
            &member_roles,
        )
        .await
        .map_err(|err| failed("set the room's power levels", err))?;
    }
    if let Some(invite_only) = invite_only {
        let rule = if *invite_only {
            JoinRule::Invite
        } else {
            JoinRule::Public
        };
        room.privacy_settings()
            .update_join_rule(rule)
            .await
            .map_err(|err| failed("set the join rule", ErrorChain(&err).to_string()))?;
    }
    if let Some(name) = name {
        room.set_name(name.clone())
            .await
            .map_err(|err| failed("set the room name", ErrorChain(&err).to_string()))?;
    }
    if let Some(topic) = topic {
        room.set_room_topic(topic)
            .await
            .map_err(|err| failed("set the topic", ErrorChain(&err).to_string()))?;
    }
    if let Some(listed) = listed_in_directory {
        let visibility = if *listed {
            Visibility::Public
        } else {
            Visibility::Private
        };
        room.privacy_settings()
            .update_room_visibility(visibility)
            .await
            .map_err(|err| {
                failed(
                    "change the room directory listing",
                    ErrorChain(&err).to_string(),
                )
            })?;
    }
    for user_id in unbans {
        room.unban_user(user_id, None)
            .await
            .map_err(|err| failed(&format!("unban {user_id}"), ErrorChain(&err).to_string()))?;
    }
    Ok(())
}

/// The room's power levels as the server has them. The store's lag until
/// a sync echoes the last write.
async fn fetch_power_levels(
    client: &Client,
    room_id: &RoomId,
) -> Result<RoomPowerLevelsEventContent, String> {
    let request = get_state_event_for_key::v3::Request::new(
        room_id.to_owned(),
        StateEventType::RoomPowerLevels,
        String::new(),
    );
    client
        .send(request)
        .await
        .map_err(|err| ErrorChain(&err).to_string())?
        .into_content()
        .deserialize_as_unchecked::<RoomPowerLevelsEventContent>()
        .map_err(|err| err.to_string())
}

/// Edits the room's power levels in one write: the modes, then the
/// members' roles against the edited levels (`+m+v Bob` voices Bob in the
/// moderated room). Every change or none: a refused voice sends nothing,
/// and levels that come out unchanged are not written.
///
/// Not the SDK's `Room::update_power_levels`: it edits the store's power
/// levels, which only learn a write from the sync's echo, so a second
/// change before the echo reverted the first. This reads the server's;
/// mirx's own edits of a room's power levels run one at a time (its
/// `Room` lane), but another client's change between the read and the
/// write is still lost (state events have no compare-and-swap).
async fn write_power_levels(
    client: &Client,
    room: &Room,
    topic_by_operators: Option<bool>,
    moderated: Option<bool>,
    member_roles: &[(&UserId, MemberPowerRole)],
) -> Result<(), String> {
    let mut levels = fetch_power_levels(client, room.room_id()).await?;
    let before = serde_json::to_value(&levels).map_err(|err| err.to_string())?;
    edit_power_levels(&mut levels, topic_by_operators, moderated);
    for &(user_id, role) in member_roles {
        let level = member_power_role_level(&levels, role)?;
        if level == levels.users_default {
            levels.users.remove(user_id);
        } else {
            levels.users.insert(user_id.to_owned(), level);
        }
    }
    if serde_json::to_value(&levels).map_err(|err| err.to_string())? == before {
        return Ok(());
    }
    room.send_state_event(levels)
        .await
        .map(|_| ())
        .map_err(|err| ErrorChain(&err).to_string())
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::events::AnySyncStateEvent;
    use matrix_sdk::ruma::serde::Raw;
    use matrix_sdk::test_utils::mocks::MatrixMockServer;
    use matrix_sdk_test::JoinedRoomBuilder;
    use wiremock::matchers::{method, path_regex};
    use wiremock::{Mock, ResponseTemplate};

    use super::*;
    use crate::api::{MemberPowerLevelChange, RoomJoinRule};

    const ROOM: &str = "!room:example.org";
    /// The room's ID in a request path, whether the SDK encodes it or not.
    const ROOM_PATH: &str = r"/rooms/(!|%21)room(:|%3A)example\.org";
    const OWN: &str = "@example:localhost";

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime")
    }

    fn state_event(
        event_type: &str,
        state_key: &str,
        sender: &str,
        ts: u64,
        content: serde_json::Value,
    ) -> serde_json::Value {
        serde_json::json!({
            "type": event_type,
            "state_key": state_key,
            "sender": sender,
            "event_id": format!("${event_type}-{state_key}-{ts}"),
            "origin_server_ts": ts,
            "room_id": ROOM,
            "content": content,
        })
    }

    /// The room's creation (`create`'s content) and the own join, as the
    /// store has them from a sync.
    async fn joined_client(server: &MatrixMockServer, create: serde_json::Value) -> Client {
        let client = server.client_builder().build().await;
        let state = [
            state_event("m.room.create", "", OWN, 1, create),
            state_event(
                "m.room.member",
                OWN,
                OWN,
                2,
                serde_json::json!({ "membership": "join" }),
            ),
        ]
        .map(|event| {
            Raw::<AnySyncStateEvent>::from_json_string(event.to_string()).expect("raw state")
        });
        server
            .sync_room(
                &client,
                JoinedRoomBuilder::new(<&RoomId>::try_from(ROOM).expect("room id"))
                    .add_state_bulk(state),
            )
            .await;
        client
    }

    async fn mount_state(server: &MatrixMockServer, state: Vec<serde_json::Value>) {
        Mock::given(method("GET"))
            .and(path_regex(format!("{ROOM_PATH}/state$")))
            .respond_with(ResponseTemplate::new(200).set_body_json(state))
            .mount(server.server())
            .await;
    }

    async fn mount_directory(server: &MatrixMockServer, visibility: &str) {
        Mock::given(method("GET"))
            .and(path_regex(
                r"/directory/list/room/(!|%21)room(:|%3A)example\.org$",
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "visibility": visibility })),
            )
            .mount(server.server())
            .await;
    }

    fn ban(user_id: &str, ts: u64, content: serde_json::Value) -> serde_json::Value {
        state_event("m.room.member", user_id, OWN, ts, content)
    }

    #[test]
    fn fetch_room_settings_reads_the_servers_state_and_listing() {
        runtime().block_on(async {
            let server = MatrixMockServer::new().await;
            let client = joined_client(&server, serde_json::json!({ "creator": OWN })).await;
            mount_state(
                &server,
                vec![
                    state_event(
                        "m.room.create",
                        "",
                        OWN,
                        1,
                        serde_json::json!({ "creator": OWN }),
                    ),
                    state_event(
                        "m.room.power_levels",
                        "",
                        OWN,
                        3,
                        serde_json::json!({ "users": { OWN: 100 } }),
                    ),
                    state_event(
                        "m.room.name",
                        "",
                        OWN,
                        4,
                        serde_json::json!({ "name": "Ops" }),
                    ),
                    state_event(
                        "m.room.topic",
                        "",
                        OWN,
                        5,
                        serde_json::json!({ "topic": "release day" }),
                    ),
                    state_event(
                        "m.room.join_rules",
                        "",
                        OWN,
                        6,
                        serde_json::json!({ "join_rule": "public" }),
                    ),
                    ban(
                        "@carol:example.org",
                        10,
                        serde_json::json!({
                            "membership": "ban", "displayname": "Carol", "reason": "spam"
                        }),
                    ),
                    ban(
                        "@dave:example.org",
                        20,
                        serde_json::json!({ "membership": "ban" }),
                    ),
                    state_event(
                        "m.room.member",
                        "@erin:example.org",
                        "@erin:example.org",
                        30,
                        serde_json::json!({ "membership": "join" }),
                    ),
                    // Other state is not decoded.
                    state_event("org.example.odd", "", OWN, 31, serde_json::json!(7)),
                ],
            )
            .await;
            mount_directory(&server, "public").await;

            let settings = fetch_room_settings(&client, ROOM).await.expect("settings");

            assert_eq!(
                settings,
                RoomSettings {
                    room_id: ROOM.to_owned(),
                    name: Some("Ops".to_owned()),
                    topic: Some("release day".to_owned()),
                    join_rule: RoomJoinRule::Public,
                    // state_default 50.
                    topic_by_operators: true,
                    moderated: false,
                    listed_in_directory: true,
                    bans: vec![
                        RoomBan {
                            user_id: "@dave:example.org".to_owned(),
                            display_name: None,
                            banned_by: OWN.to_owned(),
                            reason: None,
                            timestamp_ms: 20,
                        },
                        RoomBan {
                            user_id: "@carol:example.org".to_owned(),
                            display_name: Some("Carol".to_owned()),
                            banned_by: OWN.to_owned(),
                            reason: Some("spam".to_owned()),
                            timestamp_ms: 10,
                        },
                    ],
                    allowed: RoomSettingsPermissions {
                        name: true,
                        topic: true,
                        join_rule: true,
                        power_levels: true,
                        directory: true,
                        unban: true,
                    },
                }
            );
        });
    }

    #[test]
    fn fetch_room_settings_defaults_and_a_member_without_power() {
        runtime().block_on(async {
            let server = MatrixMockServer::new().await;
            let client = joined_client(&server, serde_json::json!({ "creator": OWN })).await;
            // No name, topic or join rules: the spec's invite.
            mount_state(
                &server,
                vec![state_event(
                    "m.room.power_levels",
                    "",
                    OWN,
                    3,
                    serde_json::json!({
                        "users": { "@bob:example.org": 100 },
                        "events": { "m.room.topic": 0, "m.room.message": 10 }
                    }),
                )],
            )
            .await;
            mount_directory(&server, "private").await;

            let settings = fetch_room_settings(&client, ROOM).await.expect("settings");

            assert_eq!(settings.name, None);
            assert_eq!(settings.topic, None);
            assert_eq!(settings.join_rule, RoomJoinRule::Invite);
            assert!(!settings.topic_by_operators);
            // m.room.encrypted still takes events_default 0.
            assert!(!settings.moderated);
            assert!(!settings.listed_in_directory);
            assert_eq!(
                settings.allowed,
                RoomSettingsPermissions {
                    name: false,
                    topic: true,
                    join_rule: false,
                    power_levels: false,
                    directory: false,
                    unban: false,
                }
            );
        });
    }

    #[test]
    fn fetch_room_settings_gives_a_room_v12_creator_everything() {
        runtime().block_on(async {
            let server = MatrixMockServer::new().await;
            let create = serde_json::json!({ "room_version": "12" });
            let client = joined_client(&server, create.clone()).await;
            // The creator has no `users` entry.
            mount_state(
                &server,
                vec![
                    state_event("m.room.create", "", OWN, 1, create),
                    state_event(
                        "m.room.power_levels",
                        "",
                        OWN,
                        3,
                        serde_json::json!({ "events_default": 1, "ban": 100, "state_default": 100 }),
                    ),
                ],
            )
            .await;
            mount_directory(&server, "private").await;

            let settings = fetch_room_settings(&client, ROOM).await.expect("settings");

            assert!(settings.moderated);
            assert!(settings.topic_by_operators);
            assert_eq!(
                settings.allowed,
                RoomSettingsPermissions {
                    name: true,
                    topic: true,
                    join_rule: true,
                    power_levels: true,
                    directory: true,
                    unban: true,
                }
            );
        });
    }

    /// `event` as redacted: what the redaction algorithm kept of its
    /// content, and the redaction in `unsigned`.
    fn redacted(mut event: serde_json::Value, kept: serde_json::Value) -> serde_json::Value {
        let event_id = event["event_id"].clone();
        event["content"] = kept;
        event["unsigned"] = serde_json::json!({
            "redacted_because": {
                "type": "m.room.redaction",
                "sender": OWN,
                "event_id": "$redaction",
                "origin_server_ts": 100,
                "room_id": ROOM,
                "redacts": event_id,
                "content": { "redacts": event_id },
            },
        });
        event
    }

    #[test]
    fn redacted_name_topic_and_power_levels_read_as_before() {
        runtime().block_on(async {
            let server = MatrixMockServer::new().await;
            let client = joined_client(&server, serde_json::json!({ "creator": OWN })).await;
            mount_state(
                &server,
                vec![
                    // The redaction keeps no keys of a name or a topic.
                    redacted(
                        state_event("m.room.name", "", OWN, 4, serde_json::json!({})),
                        serde_json::json!({}),
                    ),
                    redacted(
                        state_event("m.room.topic", "", OWN, 5, serde_json::json!({})),
                        serde_json::json!({}),
                    ),
                    // It keeps the levels.
                    redacted(
                        state_event("m.room.power_levels", "", OWN, 3, serde_json::json!({})),
                        serde_json::json!({ "users": { OWN: 100 } }),
                    ),
                    // And a member's membership.
                    redacted(
                        ban("@carol:example.org", 10, serde_json::json!({})),
                        serde_json::json!({ "membership": "ban" }),
                    ),
                ],
            )
            .await;
            mount_directory(&server, "private").await;

            let settings = fetch_room_settings(&client, ROOM).await.expect("settings");

            assert_eq!(
                settings,
                RoomSettings {
                    room_id: ROOM.to_owned(),
                    name: None,
                    topic: None,
                    join_rule: RoomJoinRule::Invite,
                    topic_by_operators: true,
                    moderated: false,
                    listed_in_directory: false,
                    bans: vec![RoomBan {
                        user_id: "@carol:example.org".to_owned(),
                        display_name: None,
                        banned_by: OWN.to_owned(),
                        reason: None,
                        timestamp_ms: 10,
                    }],
                    allowed: RoomSettingsPermissions {
                        name: true,
                        topic: true,
                        join_rule: true,
                        power_levels: true,
                        directory: true,
                        unban: true,
                    },
                }
            );
        });
    }

    async fn fetch_error(state: Vec<serde_json::Value>) -> String {
        let server = MatrixMockServer::new().await;
        let client = joined_client(&server, serde_json::json!({ "creator": OWN })).await;
        mount_state(&server, state).await;
        mount_directory(&server, "private").await;
        fetch_room_settings(&client, ROOM)
            .await
            .expect_err("malformed settings")
            .to_string()
    }

    #[test]
    fn a_malformed_settings_event_fails_the_read() {
        runtime().block_on(async {
            let err = fetch_error(vec![state_event(
                "m.room.join_rules",
                "",
                OWN,
                6,
                serde_json::json!({ "join_rule": 5 }),
            )])
            .await;
            assert!(
                err.starts_with("failed to load Matrix room settings: m.room.join_rules: "),
                "{err}"
            );

            let mut nameless = state_event("m.room.name", "", OWN, 4, serde_json::json!({}));
            nameless
                .as_object_mut()
                .expect("event object")
                .remove("content");
            let err = fetch_error(vec![nameless]).await;
            assert!(
                err.starts_with("failed to load Matrix room settings: m.room.name: "),
                "{err}"
            );
        });
    }

    fn request(room_id: &str) -> UpdateRoomSettingsRequest {
        UpdateRoomSettingsRequest {
            room_id: room_id.to_owned(),
            topic_by_operators: None,
            moderated: None,
            member_roles: Vec::new(),
            invite_only: None,
            name: None,
            topic: None,
            listed_in_directory: None,
            unban: Vec::new(),
        }
    }

    /// Mounts the server's power levels and accepting writes, then
    /// returns every request after the sync: `(method, path after the
    /// room, body)`.
    async fn update_against(
        server_levels: serde_json::Value,
        refuse_join_rule: bool,
        request: UpdateRoomSettingsRequest,
    ) -> (
        Result<(), MatrixCoreError>,
        Vec<(String, String, serde_json::Value)>,
    ) {
        let server = MatrixMockServer::new().await;
        let client = joined_client(&server, serde_json::json!({ "creator": OWN })).await;
        let before = server
            .received_requests()
            .await
            .expect("request recording enabled")
            .len();
        Mock::given(method("GET"))
            .and(path_regex(format!(
                r"{ROOM_PATH}/state/m\.room\.power_levels/?$"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(server_levels))
            .mount(server.server())
            .await;
        if refuse_join_rule {
            Mock::given(method("PUT"))
                .and(path_regex(r"/state/m\.room\.join_rules/?$"))
                .respond_with(ResponseTemplate::new(403).set_body_json(
                    serde_json::json!({ "errcode": "M_FORBIDDEN", "error": "not allowed" }),
                ))
                .with_priority(1)
                .mount(server.server())
                .await;
        }
        Mock::given(method("PUT"))
            .and(path_regex(format!("{ROOM_PATH}/state/")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "event_id": "$w" })),
            )
            .mount(server.server())
            .await;
        Mock::given(method("PUT"))
            .and(path_regex(r"/directory/list/room/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(server.server())
            .await;
        Mock::given(method("POST"))
            .and(path_regex(format!("{ROOM_PATH}/unban$")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(server.server())
            .await;

        let result = update_room_settings(&client, &request).await;
        let requests = server
            .received_requests()
            .await
            .expect("request recording enabled")
            .into_iter()
            .skip(before)
            .map(|request| {
                // The path after the API version, the room as `ROOM`.
                let path = request
                    .url
                    .path()
                    .split_once("/v3")
                    .expect("a v3 path")
                    .1
                    .replace("%21room%3Aexample.org", "ROOM")
                    .replace("!room:example.org", "ROOM");
                let body = if request.body.is_empty() {
                    serde_json::Value::Null
                } else {
                    request.body_json().expect("JSON body")
                };
                (request.method.to_string(), path, body)
            })
            .collect();
        (result, requests)
    }

    #[test]
    fn update_room_settings_writes_each_part_in_order_with_one_power_levels_write() {
        runtime().block_on(async {
            let (result, requests) = update_against(
                // The server's levels (the store has none): Bob's 50 stays.
                serde_json::json!({
                    "users": { OWN: 100, "@bob:example.org": 50 },
                    "events": { "m.room.topic": 0 }
                }),
                false,
                UpdateRoomSettingsRequest {
                    topic_by_operators: Some(true),
                    moderated: Some(true),
                    invite_only: Some(true),
                    name: Some("Ops".to_owned()),
                    topic: Some("release day".to_owned()),
                    listed_in_directory: Some(false),
                    unban: vec![
                        "@carol:example.org".to_owned(),
                        "@dave:example.org".to_owned(),
                    ],
                    ..request(ROOM)
                },
            )
            .await;
            result.expect("update");
            let shape = requests
                .iter()
                .map(|(method, path, _)| format!("{method} {path}"))
                .collect::<Vec<_>>();
            assert_eq!(
                shape,
                [
                    "GET /rooms/ROOM/state/m.room.power_levels/",
                    "PUT /rooms/ROOM/state/m.room.power_levels/",
                    "PUT /rooms/ROOM/state/m.room.join_rules/",
                    "PUT /rooms/ROOM/state/m.room.name/",
                    "PUT /rooms/ROOM/state/m.room.topic/",
                    "PUT /directory/list/room/ROOM",
                    "POST /rooms/ROOM/unban",
                    "POST /rooms/ROOM/unban",
                ]
            );
            let body = |index: usize| requests[index].2.clone();
            assert_eq!(
                body(1)["users"],
                serde_json::json!({ OWN: 100, "@bob:example.org": 50 })
            );
            assert_eq!(body(1)["events"], serde_json::json!({ "m.room.topic": 50 }));
            assert_eq!(body(1)["events_default"], serde_json::json!(1));
            assert_eq!(body(2), serde_json::json!({ "join_rule": "invite" }));
            assert_eq!(body(3), serde_json::json!({ "name": "Ops" }));
            assert_eq!(body(4)["topic"], serde_json::json!("release day"));
            assert_eq!(body(5), serde_json::json!({ "visibility": "private" }));
            assert_eq!(body(6)["user_id"], serde_json::json!("@carol:example.org"));
            assert_eq!(body(7)["user_id"], serde_json::json!("@dave:example.org"));
        });
    }

    #[test]
    fn update_room_settings_skips_a_power_levels_write_that_changes_nothing() {
        runtime().block_on(async {
            let (result, requests) = update_against(
                serde_json::json!({ "users": { OWN: 100 }, "events_default": 1 }),
                false,
                UpdateRoomSettingsRequest {
                    topic_by_operators: Some(true),
                    moderated: Some(true),
                    name: Some(String::new()),
                    ..request(ROOM)
                },
            )
            .await;
            result.expect("update");
            assert_eq!(
                requests
                    .iter()
                    .map(|(method, path, body)| (method.as_str(), path.as_str(), body.clone()))
                    .collect::<Vec<_>>(),
                [
                    (
                        "GET",
                        "/rooms/ROOM/state/m.room.power_levels/",
                        serde_json::Value::Null
                    ),
                    (
                        "PUT",
                        "/rooms/ROOM/state/m.room.name/",
                        serde_json::json!({ "name": "" })
                    ),
                ]
            );
        });
    }

    #[test]
    fn update_room_settings_stops_at_the_first_refused_part() {
        runtime().block_on(async {
            let (result, requests) = update_against(
                serde_json::json!({ "users": { OWN: 100 } }),
                true,
                UpdateRoomSettingsRequest {
                    moderated: Some(true),
                    invite_only: Some(false),
                    name: Some("Ops".to_owned()),
                    listed_in_directory: Some(true),
                    unban: vec!["@carol:example.org".to_owned()],
                    ..request(ROOM)
                },
            )
            .await;
            let err = result.expect_err("join rule refused");
            assert!(
                err.to_string().starts_with("failed to set the join rule: "),
                "{err}"
            );
            assert_eq!(
                requests
                    .iter()
                    .map(|(method, path, _)| format!("{method} {path}"))
                    .collect::<Vec<_>>(),
                [
                    "GET /rooms/ROOM/state/m.room.power_levels/",
                    "PUT /rooms/ROOM/state/m.room.power_levels/",
                    "PUT /rooms/ROOM/state/m.room.join_rules/",
                ]
            );
            assert_eq!(requests[2].2, serde_json::json!({ "join_rule": "public" }));
        });
    }

    #[test]
    fn update_room_settings_refuses_before_any_request() {
        runtime().block_on(async {
            for (request, expected) in [
                (request(ROOM), "no room setting changes"),
                (
                    request("invalid-room-id"),
                    "invalid Matrix room id: leading sigil is incorrect or missing",
                ),
                (
                    UpdateRoomSettingsRequest {
                        name: Some("Ops".to_owned()),
                        unban: vec!["@carol:example.org".to_owned(), "carol".to_owned()],
                        ..request(ROOM)
                    },
                    "invalid Matrix user id: leading sigil is incorrect or missing",
                ),
                (
                    UpdateRoomSettingsRequest {
                        member_roles: roles(&[
                            ("@carol:example.org", MemberPowerRole::Moderator),
                            ("carol", MemberPowerRole::Voiced),
                        ]),
                        ..request(ROOM)
                    },
                    "invalid Matrix user id: leading sigil is incorrect or missing",
                ),
                (
                    UpdateRoomSettingsRequest {
                        member_roles: roles(&[("@carol:example.org", MemberPowerRole::Voiced)]),
                        ..request("!other:example.org")
                    },
                    "joined room not found: !other:example.org",
                ),
            ] {
                let (result, requests) =
                    update_against(serde_json::json!({}), false, request).await;
                assert_eq!(result.expect_err("refused").to_string(), expected);
                assert!(requests.is_empty(), "{requests:?}");
            }
        });
    }

    fn roles(roles: &[(&str, MemberPowerRole)]) -> Vec<MemberPowerLevelChange> {
        roles
            .iter()
            .map(|(user_id, role)| MemberPowerLevelChange {
                user_id: (*user_id).to_owned(),
                role: *role,
            })
            .collect()
    }

    /// Every power-levels PUT body of `requests`.
    fn power_levels_puts(
        requests: &[(String, String, serde_json::Value)],
    ) -> Vec<serde_json::Value> {
        requests
            .iter()
            .filter(|(method, path, _)| {
                method == "PUT" && path == "/rooms/ROOM/state/m.room.power_levels/"
            })
            .map(|(_, _, body)| body.clone())
            .collect()
    }

    #[test]
    fn update_room_settings_writes_modes_and_member_roles_in_one_power_levels_write() {
        runtime().block_on(async {
            let (result, requests) = update_against(
                serde_json::json!({ "users": { OWN: 100 } }),
                false,
                UpdateRoomSettingsRequest {
                    moderated: Some(true),
                    member_roles: roles(&[
                        ("@bob:example.org", MemberPowerRole::Voiced),
                        ("@carol:example.org", MemberPowerRole::Moderator),
                    ]),
                    ..request(ROOM)
                },
            )
            .await;
            result.expect("update");
            assert_eq!(
                requests
                    .iter()
                    .map(|(method, path, _)| format!("{method} {path}"))
                    .collect::<Vec<_>>(),
                [
                    "GET /rooms/ROOM/state/m.room.power_levels/",
                    "PUT /rooms/ROOM/state/m.room.power_levels/",
                ]
            );
            let body = &requests[1].2;
            assert_eq!(body["events_default"], serde_json::json!(1));
            assert_eq!(
                body["users"],
                serde_json::json!({ OWN: 100, "@bob:example.org": 1, "@carol:example.org": 50 })
            );
        });
    }

    /// The roles take the levels the modes leave: a room where voice is
    /// impossible (messages need 50) gets one by unmoderating it.
    #[test]
    fn unmoderating_and_voicing_together_uses_the_unmoderated_voice_level() {
        runtime().block_on(async {
            let (result, requests) = update_against(
                serde_json::json!({ "users": { OWN: 100 }, "events_default": 50 }),
                false,
                UpdateRoomSettingsRequest {
                    moderated: Some(false),
                    member_roles: roles(&[("@bob:example.org", MemberPowerRole::Voiced)]),
                    ..request(ROOM)
                },
            )
            .await;
            result.expect("update");
            let puts = power_levels_puts(&requests);
            assert_eq!(puts.len(), 1, "{requests:?}");
            // Serializing drops the default 0.
            assert_eq!(puts[0].get("events_default"), None);
            assert_eq!(
                puts[0]["users"],
                serde_json::json!({ OWN: 100, "@bob:example.org": 1 })
            );
        });
    }

    /// The store's power levels lag behind the server's until a sync
    /// echoes the last write; an edit starts from the server's (the store
    /// has none here).
    #[test]
    fn member_roles_edit_the_servers_power_levels() {
        runtime().block_on(async {
            let (result, requests) = update_against(
                serde_json::json!({ "users": { OWN: 100, "@bob:example.org": 50 } }),
                false,
                UpdateRoomSettingsRequest {
                    member_roles: roles(&[("@target:example.org", MemberPowerRole::Moderator)]),
                    ..request(ROOM)
                },
            )
            .await;
            result.expect("update");
            assert_eq!(
                power_levels_puts(&requests)
                    .iter()
                    .map(|body| body["users"].clone())
                    .collect::<Vec<_>>(),
                [serde_json::json!({
                    OWN: 100,
                    "@bob:example.org": 50,
                    "@target:example.org": 50,
                })]
            );
        });
    }

    #[test]
    fn member_roles_map_irc_roles_to_room_levels() {
        runtime().block_on(async {
            let base = serde_json::json!({
                "users_default": 0,
                "events_default": 0,
                "users": { OWN: 100, "@target:example.org": 25 },
            });
            let with = |extra: serde_json::Value| {
                let mut content = base.clone();
                content
                    .as_object_mut()
                    .expect("power levels object")
                    .extend(extra.as_object().expect("extra object").clone());
                content
            };

            for (content, role, target_level) in [
                (base.clone(), MemberPowerRole::Moderator, Some(50)),
                (base.clone(), MemberPowerRole::Voiced, Some(1)),
                (
                    with(serde_json::json!({ "events": { "m.room.message": 10 } })),
                    MemberPowerRole::Voiced,
                    Some(10),
                ),
                (
                    with(serde_json::json!({ "events_default": 20 })),
                    MemberPowerRole::Voiced,
                    Some(20),
                ),
                (
                    with(serde_json::json!({ "users_default": 5 })),
                    MemberPowerRole::Voiced,
                    Some(6),
                ),
                (base.clone(), MemberPowerRole::Default, None),
            ] {
                let (result, requests) = update_against(
                    content,
                    false,
                    UpdateRoomSettingsRequest {
                        member_roles: roles(&[("@target:example.org", role)]),
                        ..request(ROOM)
                    },
                )
                .await;
                result.expect("update");
                let mut expected = serde_json::json!({ OWN: 100 });
                if let Some(level) = target_level {
                    expected["@target:example.org"] = serde_json::json!(level);
                }
                assert_eq!(
                    power_levels_puts(&requests)
                        .iter()
                        .map(|body| body["users"].clone())
                        .collect::<Vec<_>>(),
                    [expected],
                    "{role:?}"
                );
            }
        });
    }

    #[test]
    fn a_role_the_server_already_has_writes_nothing() {
        runtime().block_on(async {
            let (result, requests) = update_against(
                serde_json::json!({ "users": { OWN: 100, "@bob:example.org": 1 } }),
                false,
                UpdateRoomSettingsRequest {
                    member_roles: roles(&[("@bob:example.org", MemberPowerRole::Voiced)]),
                    ..request(ROOM)
                },
            )
            .await;
            result.expect("update");
            assert!(power_levels_puts(&requests).is_empty(), "{requests:?}");
        });
    }

    #[test]
    fn member_roles_refuse_voice_when_messages_need_moderator() {
        runtime().block_on(async {
            for content in [
                serde_json::json!({
                    "events": { "m.room.message": 50 },
                    "users": { OWN: 100 },
                }),
                serde_json::json!({ "users_default": 49, "users": { OWN: 100 } }),
            ] {
                // With a change that could be made, and a later part, the
                // refusal still sends nothing.
                let (result, requests) = update_against(
                    content,
                    false,
                    UpdateRoomSettingsRequest {
                        topic_by_operators: Some(false),
                        member_roles: roles(&[
                            ("@other:example.org", MemberPowerRole::Moderator),
                            ("@target:example.org", MemberPowerRole::Voiced),
                        ]),
                        name: Some("Ops".to_owned()),
                        ..request(ROOM)
                    },
                )
                .await;
                let err = result.expect_err("no voice level below moderator");
                assert!(
                    err.to_string().starts_with(
                        "failed to set the room's power levels: \
                         room has no voice level below moderator"
                    ),
                    "{err}"
                );
                assert_eq!(
                    requests
                        .iter()
                        .map(|(method, path, _)| format!("{method} {path}"))
                        .collect::<Vec<_>>(),
                    ["GET /rooms/ROOM/state/m.room.power_levels/"]
                );
            }
        });
    }

    fn levels(json: serde_json::Value) -> RoomPowerLevelsEventContent {
        serde_json::from_value(json).expect("power levels")
    }

    fn edited(
        json: serde_json::Value,
        topic_by_operators: Option<bool>,
        moderated: Option<bool>,
    ) -> serde_json::Value {
        let mut levels = levels(json);
        edit_power_levels(&mut levels, topic_by_operators, moderated);
        let mut json = serde_json::to_value(&levels).expect("serialize");
        // Only what the tests look at.
        json.as_object_mut().expect("object").retain(|key, _| {
            ["events", "events_default", "users_default", "state_default"].contains(&key.as_str())
        });
        json
    }

    #[test]
    fn only_ops_set_topic_raises_the_topic_level_or_lowers_it_to_the_default() {
        assert_eq!(
            edited(serde_json::json!({}), Some(true), None)["events"],
            serde_json::Value::Null,
            "state_default 50 already above the default: no events entry"
        );
        assert_eq!(
            edited(
                serde_json::json!({ "events": { "m.room.topic": 0 } }),
                Some(true),
                None
            )["events"],
            serde_json::json!({ "m.room.topic": 50 })
        );
        assert_eq!(
            edited(
                serde_json::json!({ "users_default": 60, "events": { "m.room.topic": 0 } }),
                Some(true),
                None
            )["events"],
            serde_json::json!({ "m.room.topic": 61 })
        );
        assert_eq!(
            edited(serde_json::json!({}), Some(false), None)["events"],
            serde_json::json!({ "m.room.topic": 0 })
        );
        assert_eq!(
            edited(
                serde_json::json!({ "events": { "m.room.topic": 0 } }),
                Some(false),
                None
            )["events"],
            serde_json::json!({ "m.room.topic": 0 })
        );
    }

    #[test]
    fn moderated_raises_message_levels_above_the_default_and_back() {
        assert_eq!(
            edited(
                serde_json::json!({ "events": { "m.room.message": 0, "m.room.name": 50 } }),
                None,
                Some(true)
            ),
            edited(
                serde_json::json!({
                    "events_default": 1,
                    "events": { "m.room.message": 1, "m.room.name": 50 }
                }),
                None,
                None
            )
        );
        // Already moderated: unchanged, a higher level kept.
        let moderated = serde_json::json!({
            "events_default": 5,
            "events": { "m.room.encrypted": 10 }
        });
        assert_eq!(
            edited(moderated.clone(), None, Some(true)),
            edited(moderated.clone(), None, None)
        );
        assert_eq!(
            edited(moderated, None, Some(false)),
            edited(
                serde_json::json!({
                    "events_default": 0,
                    "events": { "m.room.encrypted": 0 }
                }),
                None,
                None
            )
        );
    }
}
