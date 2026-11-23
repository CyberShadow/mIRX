//! mIRC's Channel Central (`/channel`, a double-click in a channel window,
//! Room > Channel Central...) over a Matrix room's settings: its name, the
//! topic and the topics seen while in it, the bans, and the modes Matrix
//! has: only ops set the topic (`+t`), invite only (`+i`), moderated
//! (`+m`) and private (`+p`, not in the room directory).
//!
//! Pure code: the form the dialog edits and the request its OK sends.
//! `channel_central_dialog` shows it.

use mirx_core::{RoomJoinRule, RoomSettings, RoomSettingsPermissions, UpdateRoomSettingsRequest};

use super::ignore::user_name;
use super::timestamp::{LocalTimeFn, list_date};

/// What the dialog says while the core reads the settings.
pub(super) const READING_TEXT: &str = "Reading the room's settings...";

/// The settings as read, and the bans the user removed since.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ChannelCentralForm {
    settings: RoomSettings,
    /// User IDs, in the order Remove took them.
    removed_bans: Vec<String>,
}

/// The dialog's controls as the user left them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ChannelCentralEdits {
    pub(super) name: String,
    pub(super) topic: String,
    pub(super) topic_by_operators: bool,
    /// `None`: indeterminate, the join rule left as it is.
    pub(super) invite_only: Option<bool>,
    pub(super) moderated: bool,
    /// Not listed in the room directory.
    pub(super) private: bool,
}

impl ChannelCentralForm {
    pub(super) fn new(settings: RoomSettings) -> Self {
        Self {
            settings,
            removed_bans: Vec::new(),
        }
    }

    pub(super) fn settings(&self) -> &RoomSettings {
        &self.settings
    }

    /// The controls as the settings fill them.
    pub(super) fn loaded_edits(&self) -> ChannelCentralEdits {
        let settings = &self.settings;
        ChannelCentralEdits {
            name: settings.name.clone().unwrap_or_default(),
            topic: settings.topic.clone().unwrap_or_default(),
            topic_by_operators: settings.topic_by_operators,
            invite_only: invite_only(&settings.join_rule),
            moderated: settings.moderated,
            private: !settings.listed_in_directory,
        }
    }

    /// The bans the list shows, newest first: User, Set by, Date, Reason.
    /// A user goes by the name their ban kept, the banner by their nick
    /// in the room.
    pub(super) fn ban_rows(
        &self,
        nick_of: &impl Fn(&str) -> Option<String>,
        local_time: LocalTimeFn,
    ) -> Vec<[String; 4]> {
        self.shown_bans()
            .map(|ban| {
                [
                    match &ban.display_name {
                        Some(name) => format!("{name} ({})", ban.user_id),
                        None => ban.user_id.clone(),
                    },
                    user_name(&ban.banned_by, nick_of),
                    list_date(local_time, ban.timestamp_ms),
                    ban.reason.clone().unwrap_or_default(),
                ]
            })
            .collect()
    }

    fn shown_bans(&self) -> impl Iterator<Item = &mirx_core::RoomBan> {
        self.settings
            .bans
            .iter()
            .filter(|ban| !self.removed_bans.contains(&ban.user_id))
    }

    /// Remove: the shown ban at `row` is lifted on OK.
    pub(super) fn remove_ban(&mut self, row: usize) {
        let user_id = self
            .shown_bans()
            .nth(row)
            .expect("Remove is greyed without a selected ban")
            .user_id
            .clone();
        self.removed_bans.push(user_id);
    }

    /// A later answer to the open dialog's read: `settings` replace the
    /// loaded ones, and every control the user left as the previous answer
    /// filled it shows the new settings, while what the user changed stays.
    /// A removed ban stays removed while `settings` still list it. The
    /// controls to show come back with the form.
    pub(super) fn rebase(
        self,
        settings: RoomSettings,
        edits: &ChannelCentralEdits,
    ) -> (ChannelCentralForm, ChannelCentralEdits) {
        let old = self.loaded_edits();
        let removed_bans = self
            .removed_bans
            .into_iter()
            .filter(|user_id| settings.bans.iter().any(|ban| &ban.user_id == user_id))
            .collect();
        let form = Self {
            settings,
            removed_bans,
        };
        let new = form.loaded_edits();
        fn kept<T: PartialEq + Clone>(old: T, new: T, edited: &T) -> T {
            if *edited == old { new } else { edited.clone() }
        }
        let edits = ChannelCentralEdits {
            name: kept(old.name, new.name, &edits.name),
            topic: kept(old.topic, new.topic, &edits.topic),
            topic_by_operators: kept(
                old.topic_by_operators,
                new.topic_by_operators,
                &edits.topic_by_operators,
            ),
            invite_only: kept(old.invite_only, new.invite_only, &edits.invite_only),
            moderated: kept(old.moderated, new.moderated, &edits.moderated),
            private: kept(old.private, new.private, &edits.private),
        };
        (form, edits)
    }

    /// What OK sends: the parts `edits` change, `None` when nothing does.
    /// Unbans go in the list's order.
    pub(super) fn request(&self, edits: &ChannelCentralEdits) -> Option<UpdateRoomSettingsRequest> {
        let loaded = self.loaded_edits();
        let changed = |loaded, edited| (loaded != edited).then_some(edited);
        let request = UpdateRoomSettingsRequest {
            room_id: self.settings.room_id.clone(),
            topic_by_operators: changed(loaded.topic_by_operators, edits.topic_by_operators),
            moderated: changed(loaded.moderated, edits.moderated),
            // Ranks change in the nick list, `/op` and `/mode`.
            member_roles: Vec::new(),
            // Indeterminate leaves the rule; a state it started in too.
            invite_only: edits
                .invite_only
                .filter(|invite_only| loaded.invite_only != Some(*invite_only)),
            name: (loaded.name != edits.name).then(|| edits.name.clone()),
            topic: (loaded.topic != edits.topic).then(|| edits.topic.clone()),
            listed_in_directory: changed(loaded.private, edits.private).map(|private| !private),
            unban: self
                .settings
                .bans
                .iter()
                .filter(|ban| self.removed_bans.contains(&ban.user_id))
                .map(|ban| ban.user_id.clone())
                .collect(),
        };
        let UpdateRoomSettingsRequest {
            room_id: _,
            topic_by_operators,
            moderated,
            member_roles: _,
            invite_only,
            name,
            topic,
            listed_in_directory,
            unban,
        } = &request;
        let unchanged = topic_by_operators.is_none()
            && moderated.is_none()
            && invite_only.is_none()
            && name.is_none()
            && topic.is_none()
            && listed_in_directory.is_none()
            && unban.is_empty();
        (!unchanged).then_some(request)
    }
}

/// Invite only's check: checked for `invite`, unchecked for `public`, and
/// indeterminate for every other rule, which neither state is.
pub(super) fn invite_only(rule: &RoomJoinRule) -> Option<bool> {
    match rule {
        RoomJoinRule::Invite => Some(true),
        RoomJoinRule::Public => Some(false),
        RoomJoinRule::Knock
        | RoomJoinRule::Restricted
        | RoomJoinRule::KnockRestricted
        | RoomJoinRule::Private
        | RoomJoinRule::Other(_) => None,
    }
}

/// The dialog's status line once the settings are in: why controls are
/// greyed, as mIRC greys them for a non-op.
pub(super) fn status_text(allowed: &RoomSettingsPermissions) -> &'static str {
    let RoomSettingsPermissions {
        name,
        topic,
        join_rule,
        power_levels,
        directory,
        unban,
    } = *allowed;
    let all = [name, topic, join_rule, power_levels, directory, unban];
    if all.iter().all(|allowed| *allowed) {
        ""
    } else if all.iter().any(|allowed| *allowed) {
        "Some settings need a higher power level."
    } else {
        "Only operators can change these settings."
    }
}

/// Topic History's items: the current topic, then the topics the window
/// saw set, newest first, each once.
pub(super) fn topic_history<'a>(
    current: Option<&'a str>,
    window_topics_newest_first: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    for topic in current.into_iter().chain(window_topics_newest_first) {
        if !items.iter().any(|item| item == topic) {
            items.push(topic.to_owned());
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use mirx_core::RoomBan;

    use super::*;
    use crate::win32::timestamp::utc_local_time;

    const ALL: RoomSettingsPermissions = RoomSettingsPermissions {
        name: true,
        topic: true,
        join_rule: true,
        power_levels: true,
        directory: true,
        unban: true,
    };

    fn ban(user_id: &str, display_name: Option<&str>, reason: Option<&str>) -> RoomBan {
        RoomBan {
            user_id: user_id.to_owned(),
            display_name: display_name.map(str::to_owned),
            banned_by: "@alice:example".to_owned(),
            reason: reason.map(str::to_owned),
            // 23/11/2026 22:14 UTC.
            timestamp_ms: 1_795_472_040_000,
        }
    }

    fn settings() -> RoomSettings {
        RoomSettings {
            room_id: "!ops:example".to_owned(),
            name: Some("Ops".to_owned()),
            topic: Some("release day".to_owned()),
            join_rule: RoomJoinRule::Invite,
            topic_by_operators: true,
            moderated: false,
            listed_in_directory: true,
            bans: vec![
                ban("@carol:example", Some("Carol"), Some("spam")),
                ban("@dave:example", None, None),
            ],
            allowed: ALL,
        }
    }

    fn untouched() -> UpdateRoomSettingsRequest {
        UpdateRoomSettingsRequest {
            room_id: "!ops:example".to_owned(),
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

    #[test]
    fn ok_sends_nothing_when_nothing_changed() {
        let form = ChannelCentralForm::new(settings());
        assert_eq!(form.request(&form.loaded_edits()), None);
    }

    #[test]
    fn ok_sends_each_changed_part_alone() {
        let form = ChannelCentralForm::new(settings());
        let loaded = form.loaded_edits();
        for (edits, expected) in [
            (
                ChannelCentralEdits {
                    name: String::new(),
                    ..loaded.clone()
                },
                UpdateRoomSettingsRequest {
                    name: Some(String::new()),
                    ..untouched()
                },
            ),
            (
                ChannelCentralEdits {
                    topic: "new".to_owned(),
                    ..loaded.clone()
                },
                UpdateRoomSettingsRequest {
                    topic: Some("new".to_owned()),
                    ..untouched()
                },
            ),
            (
                ChannelCentralEdits {
                    topic_by_operators: false,
                    ..loaded.clone()
                },
                UpdateRoomSettingsRequest {
                    topic_by_operators: Some(false),
                    ..untouched()
                },
            ),
            (
                ChannelCentralEdits {
                    moderated: true,
                    ..loaded.clone()
                },
                UpdateRoomSettingsRequest {
                    moderated: Some(true),
                    ..untouched()
                },
            ),
            (
                ChannelCentralEdits {
                    invite_only: Some(false),
                    ..loaded.clone()
                },
                UpdateRoomSettingsRequest {
                    invite_only: Some(false),
                    ..untouched()
                },
            ),
            (
                ChannelCentralEdits {
                    private: true,
                    ..loaded.clone()
                },
                UpdateRoomSettingsRequest {
                    listed_in_directory: Some(false),
                    ..untouched()
                },
            ),
        ] {
            assert_eq!(form.request(&edits), Some(expected), "{edits:?}");
        }
    }

    #[test]
    fn invite_only_is_indeterminate_for_rules_it_cannot_say() {
        for (rule, state) in [
            (RoomJoinRule::Invite, Some(true)),
            (RoomJoinRule::Public, Some(false)),
            (RoomJoinRule::Knock, None),
            (RoomJoinRule::Restricted, None),
            (RoomJoinRule::KnockRestricted, None),
            (RoomJoinRule::Private, None),
            (RoomJoinRule::Other("vip".to_owned()), None),
        ] {
            assert_eq!(invite_only(&rule), state, "{rule:?}");
        }
        let form = ChannelCentralForm::new(RoomSettings {
            join_rule: RoomJoinRule::Knock,
            ..settings()
        });
        let loaded = form.loaded_edits();
        assert_eq!(form.request(&loaded), None, "left indeterminate");
        for invite_only in [true, false] {
            assert_eq!(
                form.request(&ChannelCentralEdits {
                    invite_only: Some(invite_only),
                    ..loaded.clone()
                }),
                Some(UpdateRoomSettingsRequest {
                    invite_only: Some(invite_only),
                    ..untouched()
                })
            );
        }
    }

    #[test]
    fn an_unnamed_room_left_unnamed_and_untopiced_changes_nothing() {
        let form = ChannelCentralForm::new(RoomSettings {
            name: None,
            topic: None,
            ..settings()
        });
        assert_eq!(form.loaded_edits().name, "");
        assert_eq!(form.request(&form.loaded_edits()), None);
    }

    #[test]
    fn removed_bans_are_lifted_in_the_lists_order() {
        let mut form = ChannelCentralForm::new(settings());
        let nick_of = |user_id: &str| (user_id == "@alice:example").then(|| "Alice".to_owned());
        assert_eq!(
            form.ban_rows(&nick_of, utc_local_time),
            [
                [
                    "Carol (@carol:example)".to_owned(),
                    "Alice".to_owned(),
                    "23/11/2026 22:14".to_owned(),
                    "spam".to_owned(),
                ],
                [
                    "@dave:example".to_owned(),
                    "Alice".to_owned(),
                    "23/11/2026 22:14".to_owned(),
                    String::new(),
                ],
            ]
        );
        form.remove_ban(1);
        form.remove_ban(0);
        assert!(form.ban_rows(&nick_of, utc_local_time).is_empty());
        assert_eq!(
            form.request(&form.loaded_edits()),
            Some(UpdateRoomSettingsRequest {
                unban: vec!["@carol:example".to_owned(), "@dave:example".to_owned()],
                ..untouched()
            })
        );
        // The banner's ID without a nick.
        assert_eq!(
            ChannelCentralForm::new(settings()).ban_rows(&|_: &str| None, utc_local_time)[0][1],
            "@alice:example"
        );
    }

    #[test]
    fn the_status_says_why_controls_are_greyed() {
        assert_eq!(status_text(&ALL), "");
        assert_eq!(
            status_text(&RoomSettingsPermissions {
                topic: false,
                ..ALL
            }),
            "Some settings need a higher power level."
        );
        assert_eq!(
            status_text(&RoomSettingsPermissions {
                name: false,
                topic: false,
                join_rule: false,
                power_levels: false,
                directory: false,
                unban: false,
            }),
            "Only operators can change these settings."
        );
    }

    #[test]
    fn topic_history_starts_with_the_current_topic_each_once() {
        assert_eq!(
            topic_history(Some("c"), ["b", "c", "a", "b"]),
            ["c", "b", "a"]
        );
        assert_eq!(topic_history(None, ["b"]), ["b"]);
        assert!(topic_history(None, []).is_empty());
    }

    /// Answer A as `settings()` with topic `t1`; the user named the room
    /// `Mine`, made it moderated and removed Carol's ban.
    fn edited_after_answer_a() -> (ChannelCentralForm, ChannelCentralEdits) {
        let mut form = ChannelCentralForm::new(RoomSettings {
            topic: Some("t1".to_owned()),
            ..settings()
        });
        form.remove_ban(0);
        let edits = ChannelCentralEdits {
            name: "Mine".to_owned(),
            moderated: true,
            ..form.loaded_edits()
        };
        (form, edits)
    }

    fn answer_b() -> RoomSettings {
        RoomSettings {
            name: Some("Ops2".to_owned()),
            topic: Some("t2".to_owned()),
            join_rule: RoomJoinRule::Public,
            listed_in_directory: false,
            bans: vec![
                ban("@carol:example", Some("Carol"), Some("spam")),
                ban("@erin:example", None, None),
            ],
            ..settings()
        }
    }

    #[test]
    fn a_later_answer_refreshes_what_the_user_left() {
        let (form, edits) = edited_after_answer_a();
        let (form, edits) = form.rebase(answer_b(), &edits);
        assert_eq!(
            edits,
            ChannelCentralEdits {
                name: "Mine".to_owned(),
                topic: "t2".to_owned(),
                topic_by_operators: true,
                invite_only: Some(false),
                moderated: true,
                private: true,
            }
        );
        assert_eq!(form.settings(), &answer_b());
        // Carol's ban stays removed; Dave's, gone from the answer, is
        // forgotten.
        assert_eq!(form.removed_bans, ["@carol:example"]);
        assert_eq!(
            form.ban_rows(&|_: &str| None, utc_local_time)
                .iter()
                .map(|row| row[0].as_str())
                .collect::<Vec<_>>(),
            ["@erin:example"]
        );
        assert_eq!(
            form.request(&edits),
            Some(UpdateRoomSettingsRequest {
                name: Some("Mine".to_owned()),
                moderated: Some(true),
                unban: vec!["@carol:example".to_owned()],
                ..untouched()
            })
        );
        // A third answer rebases on the second.
        let (form, edits) = form.rebase(
            RoomSettings {
                topic: Some("t3".to_owned()),
                ..answer_b()
            },
            &ChannelCentralEdits {
                private: false,
                ..edits
            },
        );
        assert_eq!(edits.topic, "t3");
        assert!(!edits.private);
        assert_eq!(
            form.request(&edits)
                .map(|request| request.listed_in_directory),
            Some(Some(true))
        );
    }

    #[test]
    fn a_later_answer_equal_to_an_edit_makes_it_unchanged() {
        let form = ChannelCentralForm::new(settings());
        let edits = ChannelCentralEdits {
            topic: "t2".to_owned(),
            ..form.loaded_edits()
        };
        let (form, edits) = form.rebase(
            RoomSettings {
                topic: Some("t2".to_owned()),
                ..settings()
            },
            &edits,
        );
        assert_eq!(edits.topic, "t2");
        assert_eq!(form.request(&edits), None);
    }

    #[test]
    fn a_later_answer_rebases_an_indeterminate_invite_only() {
        let knock = ChannelCentralForm::new(RoomSettings {
            join_rule: RoomJoinRule::Knock,
            ..settings()
        });
        let invite = settings();

        // Left indeterminate: the new rule's state.
        let (form, edits) = knock.clone().rebase(invite.clone(), &knock.loaded_edits());
        assert_eq!(edits.invite_only, Some(true));
        assert_eq!(form.request(&edits), None);

        // Unchecked by the user: kept, and sent against the new rule.
        let unchecked = ChannelCentralEdits {
            invite_only: Some(false),
            ..knock.loaded_edits()
        };
        let (form, edits) = knock.rebase(invite, &unchecked);
        assert_eq!(edits.invite_only, Some(false));
        assert_eq!(
            form.request(&edits),
            Some(UpdateRoomSettingsRequest {
                invite_only: Some(false),
                ..untouched()
            })
        );
    }
}
