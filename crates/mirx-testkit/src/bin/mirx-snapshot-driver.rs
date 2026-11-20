#[cfg(not(windows))]
fn main() {
    eprintln!("mirx-snapshot-driver requires the Windows target");
    std::process::exit(1);
}

#[cfg(any(windows, test))]
mod cli {
    use std::time::Duration;

    use mirx_core::RoomMembership;
    use mirx_testkit::{
        AutomationDialog, MdiArrangement, MdiWindowState, MircFormatKey, NavigationNodeKey,
        ViewActivityLevel, ViewIdentity,
    };

    const DEFAULT_FIND_TIMEOUT: Duration = Duration::from_secs(10);
    const DEFAULT_WAIT_IDLE: Duration = Duration::from_secs(5);

    #[derive(Default)]
    pub(crate) struct Config {
        pub(crate) find_timeout: Duration,
        pub(crate) wait_idle: Duration,
        pub(crate) select_views: Vec<ViewIdentity>,
        pub(crate) trigger_history_backfill: bool,
        pub(crate) append_events: Option<String>,
        pub(crate) find_active_view: Option<String>,
        pub(crate) actions: Vec<Action>,
        pub(crate) expect_active_find_history: Option<String>,
        pub(crate) expect_active_find_selected_text: Option<String>,
        pub(crate) expect_selected: Option<String>,
        pub(crate) expect_rooms: Vec<String>,
        pub(crate) expect_room_memberships: Vec<RoomMembershipExpectation>,
        pub(crate) expect_no_rooms: Vec<String>,
        pub(crate) expect_room_activities: Vec<RoomActivityExpectation>,
        pub(crate) expect_error: Option<String>,
        pub(crate) expect_verification: Option<String>,
        pub(crate) expect_verification_state: Option<String>,
        pub(crate) expect_verification_texts: Vec<String>,
        pub(crate) expect_active_view: Option<ViewIdentity>,
        pub(crate) expect_views: Vec<ViewExpectation>,
        pub(crate) expect_no_views: Vec<ViewExpectation>,
        pub(crate) expect_room_views: Vec<ViewTextExpectation>,
        pub(crate) expect_room_members: Vec<ViewTextExpectation>,
        pub(crate) expect_status_views: Vec<String>,
        pub(crate) expect_no_status_views: Vec<String>,
        pub(crate) expect_status_bars: Vec<String>,
        pub(crate) expect_no_status_bars: Vec<String>,
        pub(crate) expect_switchbar_items: Vec<SwitchbarItemExpectation>,
        pub(crate) expect_no_switchbar_items: Vec<ViewIdentity>,
        pub(crate) expect_switchbar_active: Option<ViewIdentity>,
        pub(crate) expect_switchbar_rows: Option<u32>,
        pub(crate) expect_frame_bars_hidden: Vec<FrameBar>,
        pub(crate) expect_switchbar_activity: Vec<SwitchbarActivityExpectation>,
        pub(crate) expect_no_switchbar_activity: Vec<ViewIdentity>,
        pub(crate) expect_navigation_texts: Vec<String>,
        pub(crate) expect_no_navigation_texts: Vec<String>,
        pub(crate) expect_navigation_activity: Vec<NavigationActivityExpectation>,
        pub(crate) expect_no_navigation_activity: Vec<ViewIdentity>,
        pub(crate) expect_navigation_nodes: Vec<NavigationNodeExpectation>,
        pub(crate) expect_direct_rooms: Vec<String>,
        pub(crate) expect_room_tags: Vec<RoomTagExpectation>,
        pub(crate) expect_room_read_targets: Vec<RoomReadTargetExpectation>,
        pub(crate) expect_room_unreads: Vec<RoomUnreadExpectation>,
        pub(crate) expect_mdi_placements: Vec<MdiPlacementExpectation>,
        pub(crate) expect_mdi_titles: Vec<MdiTitleExpectation>,
        /// The title of the dialog the (last) `dialog:` action opened.
        pub(crate) expect_dialog_title: Option<String>,
    }

    /// A frame bar that View shows and hides.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum FrameBar {
        Switchbar,
        Treebar,
    }

    impl FrameBar {
        pub(crate) fn name(self) -> &'static str {
            match self {
                Self::Switchbar => "switchbar",
                Self::Treebar => "treebar",
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct MdiTitleExpectation {
        pub(crate) identity: ViewIdentity,
        pub(crate) title: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct MdiPlacementExpectation {
        pub(crate) identity: ViewIdentity,
        pub(crate) state: MdiWindowState,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Action {
        Find(String),
        /// Types a line into the active window's editbox and presses Return.
        Input(String),
        /// Puts text in the active window's editbox, caret at its end,
        /// without sending it.
        ComposeText(String),
        /// Presses a formatting chord in the active window's editbox.
        ComposeKey(MircFormatKey),
        Append(String),
        Arrange(MdiArrangement),
        WindowState(ViewIdentity, MdiWindowState),
        /// Collapses a treebar item, as its button does.
        Collapse(NavigationNodeKey),
        /// Gives Colors dialog items palette indices, by `mirx.ini` key, as
        /// the user does: the item drop-down, a click on the palette box
        /// and OK.
        Colors(Vec<(String, u8)>),
        /// Opens a dialog, and for Options shows the page at the tree path.
        /// The dialog disables the frame, so this is the last action.
        Dialog {
            dialog: AutomationDialog,
            options_page: Option<String>,
        },
        /// Presses the left button on the active timeline at one point and
        /// moves to another, marking the text between them. The button is
        /// never released (a release copies the mark and clears it), so this
        /// is the last action.
        Drag {
            from: TextPoint,
            to: TextPoint,
        },
    }

    impl Action {
        /// Whether nothing may follow this action: a dialog disables the
        /// frame, and a drag holds the mouse.
        fn is_terminal(&self) -> bool {
            matches!(self, Action::Dialog { .. } | Action::Drag { .. })
        }
    }

    /// A character boundary on screen: `offset` bytes into the text of the
    /// active timeline's visible line `line`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct TextPoint {
        pub(crate) line: usize,
        pub(crate) offset: usize,
    }

    /// Parses a protocol enum from its wire name, so the CLI names cannot
    /// drift from the protocol's.
    fn parse_wire_name<T: serde::de::DeserializeOwned>(name: &str) -> Option<T> {
        serde_json::from_value(serde_json::Value::String(name.to_owned())).ok()
    }

    pub(crate) struct RoomActivityExpectation {
        pub(crate) room_id: String,
        pub(crate) substring: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct RoomMembershipExpectation {
        pub(crate) room_id: String,
        pub(crate) membership: RoomMembership,
    }

    pub(crate) struct ViewTextExpectation {
        pub(crate) room_id: String,
        pub(crate) substring: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct ViewExpectation {
        pub(crate) identity: ViewIdentity,
        pub(crate) substring: String,
    }

    pub(crate) struct SwitchbarItemExpectation {
        pub(crate) identity: ViewIdentity,
        pub(crate) title_substring: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct SwitchbarActivityExpectation {
        pub(crate) identity: ViewIdentity,
        pub(crate) level: ViewActivityLevel,
    }

    /// A treebar item's expanded state and the colour it is drawn with.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct NavigationNodeExpectation {
        pub(crate) node: NavigationNodeKey,
        pub(crate) expanded: bool,
        pub(crate) activity: Option<ViewActivityLevel>,
    }

    /// `space:<room_id>` or a view identity.
    pub(crate) fn parse_navigation_node_arg(
        flag_name: &str,
        value: &str,
    ) -> Result<NavigationNodeKey, String> {
        match value.strip_prefix("space:") {
            Some("") => Err(format!(
                "invalid {flag_name} value {value:?}; expected space:<room_id> or a view identity\n{}",
                usage()
            )),
            Some(room_id) => Ok(NavigationNodeKey::Space(room_id.to_owned())),
            None => parse_view_identity_arg(flag_name, value).map(NavigationNodeKey::View),
        }
    }

    /// `<node>::<expanded|collapsed>::<event|message|highlight|none>`.
    pub(crate) fn parse_navigation_node_expectation(
        value: &str,
    ) -> Result<NavigationNodeExpectation, String> {
        let invalid = || {
            format!(
                "invalid --expect-navigation-node value {value:?}; expected <node>::<expanded|collapsed>::<event|message|highlight|none>\n{}",
                usage()
            )
        };
        let (rest, activity) = value.rsplit_once("::").ok_or_else(invalid)?;
        let (node, expanded) = rest.rsplit_once("::").ok_or_else(invalid)?;
        Ok(NavigationNodeExpectation {
            node: parse_navigation_node_arg("--expect-navigation-node", node)?,
            expanded: match expanded {
                "expanded" => true,
                "collapsed" => false,
                _ => return Err(invalid()),
            },
            activity: match activity {
                "none" => None,
                level => Some(parse_wire_name(level).ok_or_else(invalid)?),
            },
        })
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct NavigationActivityExpectation {
        pub(crate) identity: ViewIdentity,
        pub(crate) level: ViewActivityLevel,
    }

    /// Splits `<target>::<event|message|highlight>`. The level is last, so
    /// targets may contain `::`-free Matrix IDs with single colons.
    pub(crate) fn parse_activity_expectation<'a>(
        flag_name: &str,
        value: &'a str,
    ) -> Result<(&'a str, ViewActivityLevel), String> {
        let invalid = || {
            format!(
                "invalid {flag_name} value {value:?}; expected <target>::<event|message|highlight>\n{}",
                usage()
            )
        };
        let (target, level) = value.rsplit_once("::").ok_or_else(invalid)?;
        let level = match level {
            "event" => ViewActivityLevel::Event,
            "message" => ViewActivityLevel::Message,
            "highlight" => ViewActivityLevel::Highlight,
            _ => return Err(invalid()),
        };
        Ok((target, level))
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct RoomTagExpectation {
        pub(crate) room_id: String,
        pub(crate) tag: String,
        pub(crate) order: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct RoomReadTargetExpectation {
        pub(crate) room_id: String,
        pub(crate) read_target_event_id: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct RoomUnreadExpectation {
        pub(crate) room_id: String,
        pub(crate) num_unread: u64,
        pub(crate) num_notifications: u64,
        pub(crate) num_mentions: u64,
        pub(crate) marked_unread: bool,
    }

    pub(crate) fn parse_args(args: impl Iterator<Item = String>) -> Result<Config, String> {
        let mut config = Config {
            find_timeout: DEFAULT_FIND_TIMEOUT,
            wait_idle: DEFAULT_WAIT_IDLE,
            ..Config::default()
        };
        let mut args = args.peekable();

        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--find-timeout-ms" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --find-timeout-ms".to_owned())?;
                    let millis = value
                        .parse::<u64>()
                        .map_err(|_| format!("invalid --find-timeout-ms value: {value}"))?;
                    config.find_timeout = Duration::from_millis(millis);
                }
                "--wait-idle-ms" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --wait-idle-ms".to_owned())?;
                    let millis = value
                        .parse::<u64>()
                        .map_err(|_| format!("invalid --wait-idle-ms value: {value}"))?;
                    config.wait_idle = Duration::from_millis(millis);
                }
                "--select-view" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --select-view".to_owned())?;
                    config
                        .select_views
                        .push(parse_view_identity_arg("--select-view", &value)?);
                }
                "--trigger-history-backfill" => {
                    config.trigger_history_backfill = true;
                }
                "--append-events" => {
                    config.append_events = Some(
                        args.next()
                            .ok_or_else(|| "missing value for --append-events".to_owned())?,
                    );
                }
                "--find-active-view" => {
                    config.find_active_view = Some(
                        args.next()
                            .ok_or_else(|| "missing value for --find-active-view".to_owned())?,
                    );
                }
                "--action" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --action".to_owned())?;
                    config.actions.push(parse_action(&value)?);
                }
                "--expect-active-find-history" => {
                    let value = args.next().ok_or_else(|| {
                        "missing value for --expect-active-find-history".to_owned()
                    })?;
                    match value.as_str() {
                        "local_only" | "loading" | "exhausted" | "failed" => {
                            config.expect_active_find_history = Some(value);
                        }
                        _ => {
                            return Err(format!(
                                "invalid --expect-active-find-history value {value:?}; expected local_only|loading|exhausted|failed\n{}",
                                usage()
                            ));
                        }
                    }
                }
                "--expect-active-find-selected-text" => {
                    config.expect_active_find_selected_text =
                        Some(args.next().ok_or_else(|| {
                            "missing value for --expect-active-find-selected-text".to_owned()
                        })?);
                }
                "--expect-selected" => {
                    config.expect_selected = Some(
                        args.next()
                            .ok_or_else(|| "missing value for --expect-selected".to_owned())?,
                    );
                }
                "--expect-room" => {
                    config.expect_rooms.push(
                        args.next()
                            .ok_or_else(|| "missing value for --expect-room".to_owned())?,
                    );
                }
                "--expect-room-membership" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-room-membership".to_owned())?;
                    config
                        .expect_room_memberships
                        .push(parse_room_membership_expectation(&value)?);
                }
                "--expect-no-room" => {
                    config.expect_no_rooms.push(
                        args.next()
                            .ok_or_else(|| "missing value for --expect-no-room".to_owned())?,
                    );
                }
                "--expect-direct-room" => {
                    config.expect_direct_rooms.push(
                        args.next()
                            .ok_or_else(|| "missing value for --expect-direct-room".to_owned())?,
                    );
                }
                "--expect-room-tag" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-room-tag".to_owned())?;
                    config
                        .expect_room_tags
                        .push(parse_room_tag_expectation(&value)?);
                }
                "--expect-room-read-target" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-room-read-target".to_owned())?;
                    config
                        .expect_room_read_targets
                        .push(parse_room_read_target_expectation(&value)?);
                }
                "--expect-room-unread" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-room-unread".to_owned())?;
                    config
                        .expect_room_unreads
                        .push(parse_room_unread_expectation(&value)?);
                }
                "--expect-room-activity" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for room activity expectation".to_owned())?;
                    let (room_id, substring) = value.split_once("::").ok_or_else(|| {
                        format!(
                            "invalid room activity expectation {value:?}; expected <room_id>::<substring>"
                        )
                    })?;
                    config.expect_room_activities.push(RoomActivityExpectation {
                        room_id: room_id.to_owned(),
                        substring: substring.to_owned(),
                    });
                }
                "--expect-error" => {
                    config.expect_error = Some(
                        args.next()
                            .ok_or_else(|| "missing value for --expect-error".to_owned())?,
                    );
                }
                "--expect-verification" => {
                    config.expect_verification = Some(
                        args.next()
                            .ok_or_else(|| "missing value for --expect-verification".to_owned())?,
                    );
                }
                "--expect-verification-state" => {
                    let value = args.next().ok_or_else(|| {
                        "missing value for --expect-verification-state".to_owned()
                    })?;
                    match value.as_str() {
                        "requested" | "accepted" | "ready" | "sas_ready" | "confirmed" | "done"
                        | "cancelled" => {
                            config.expect_verification_state = Some(value);
                        }
                        _ => {
                            return Err(format!(
                                "invalid --expect-verification-state value {value:?}; expected requested|accepted|ready|sas_ready|confirmed|done|cancelled\n{}",
                                usage()
                            ));
                        }
                    }
                }
                "--expect-verification-text" => {
                    config
                        .expect_verification_texts
                        .push(args.next().ok_or_else(|| {
                            "missing value for --expect-verification-text".to_owned()
                        })?);
                }
                "--expect-active-view" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-active-view".to_owned())?;
                    config.expect_active_view =
                        Some(parse_view_identity_arg("--expect-active-view", &value)?);
                }
                "--expect-view" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-view".to_owned())?;
                    config
                        .expect_views
                        .push(parse_view_expectation("--expect-view", &value)?);
                }
                "--expect-no-view" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-no-view".to_owned())?;
                    config
                        .expect_no_views
                        .push(parse_view_expectation("--expect-no-view", &value)?);
                }
                "--expect-room-view" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-room-view".to_owned())?;
                    let (room_id, substring) = value.split_once("::").ok_or_else(|| {
                        format!(
                            "invalid --expect-room-view value {value:?}; expected <room_id>::<substring>"
                        )
                    })?;
                    config.expect_room_views.push(ViewTextExpectation {
                        room_id: room_id.to_owned(),
                        substring: substring.to_owned(),
                    });
                }
                "--expect-room-member" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-room-member".to_owned())?;
                    let (room_id, substring) = value.split_once("::").ok_or_else(|| {
                        format!(
                            "invalid --expect-room-member value {value:?}; expected <room_id>::<substring>"
                        )
                    })?;
                    config.expect_room_members.push(ViewTextExpectation {
                        room_id: room_id.to_owned(),
                        substring: substring.to_owned(),
                    });
                }
                "--expect-status-view" => {
                    config.expect_status_views.push(
                        args.next()
                            .ok_or_else(|| "missing value for --expect-status-view".to_owned())?,
                    );
                }
                "--expect-no-status-view" => {
                    config.expect_no_status_views.push(
                        args.next().ok_or_else(|| {
                            "missing value for --expect-no-status-view".to_owned()
                        })?,
                    );
                }
                "--expect-status-bar" => {
                    config.expect_status_bars.push(
                        args.next()
                            .ok_or_else(|| "missing value for --expect-status-bar".to_owned())?,
                    );
                }
                "--expect-no-status-bar" => {
                    config
                        .expect_no_status_bars
                        .push(args.next().ok_or_else(|| {
                            "missing value for --expect-no-status-bar".to_owned()
                        })?);
                }
                "--expect-switchbar-item" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-switchbar-item".to_owned())?;
                    let (identity, title_substring) = value.split_once("::").ok_or_else(|| {
                        format!(
                            "invalid --expect-switchbar-item value {value:?}; expected <identity>::<title-substring>"
                        )
                    })?;
                    config
                        .expect_switchbar_items
                        .push(SwitchbarItemExpectation {
                            identity: parse_view_identity_arg("--expect-switchbar-item", identity)?,
                            title_substring: title_substring.to_owned(),
                        });
                }
                "--expect-no-switchbar-item" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-no-switchbar-item".to_owned())?;
                    config
                        .expect_no_switchbar_items
                        .push(parse_view_identity_arg(
                            "--expect-no-switchbar-item",
                            &value,
                        )?);
                }
                "--expect-switchbar-active" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-switchbar-active".to_owned())?;
                    config.expect_switchbar_active = Some(parse_view_identity_arg(
                        "--expect-switchbar-active",
                        &value,
                    )?);
                }
                "--expect-switchbar-rows" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-switchbar-rows".to_owned())?;
                    config.expect_switchbar_rows =
                        Some(value.parse().ok().filter(|rows| *rows >= 1).ok_or_else(|| {
                            format!(
                                "invalid --expect-switchbar-rows value {value:?}; expected a row count of at least 1"
                            )
                        })?);
                }
                "--expect-frame-bar-hidden" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-frame-bar-hidden".to_owned())?;
                    let bar = [FrameBar::Switchbar, FrameBar::Treebar]
                        .into_iter()
                        .find(|bar| bar.name() == value)
                        .ok_or_else(|| {
                            format!(
                                "invalid --expect-frame-bar-hidden value {value:?}; expected switchbar or treebar"
                            )
                        })?;
                    config.expect_frame_bars_hidden.push(bar);
                }
                "--expect-switchbar-activity" => {
                    let value = args.next().ok_or_else(|| {
                        "missing value for --expect-switchbar-activity".to_owned()
                    })?;
                    let (identity, level) =
                        parse_activity_expectation("--expect-switchbar-activity", &value)?;
                    config
                        .expect_switchbar_activity
                        .push(SwitchbarActivityExpectation {
                            identity: parse_view_identity_arg(
                                "--expect-switchbar-activity",
                                identity,
                            )?,
                            level,
                        });
                }
                "--expect-no-switchbar-activity" => {
                    let value = args.next().ok_or_else(|| {
                        "missing value for --expect-no-switchbar-activity".to_owned()
                    })?;
                    config
                        .expect_no_switchbar_activity
                        .push(parse_view_identity_arg(
                            "--expect-no-switchbar-activity",
                            &value,
                        )?);
                }
                "--expect-no-navigation-text" => {
                    config
                        .expect_no_navigation_texts
                        .push(args.next().ok_or_else(|| {
                            "missing value for --expect-no-navigation-text".to_owned()
                        })?);
                }
                "--expect-navigation-text" => {
                    config.expect_navigation_texts.push(
                        args.next().ok_or_else(|| {
                            "missing value for --expect-navigation-text".to_owned()
                        })?,
                    );
                }
                "--expect-navigation-activity" => {
                    let value = args.next().ok_or_else(|| {
                        "missing value for --expect-navigation-activity".to_owned()
                    })?;
                    let (identity, level) =
                        parse_activity_expectation("--expect-navigation-activity", &value)?;
                    config
                        .expect_navigation_activity
                        .push(NavigationActivityExpectation {
                            identity: parse_view_identity_arg(
                                "--expect-navigation-activity",
                                identity,
                            )?,
                            level,
                        });
                }
                "--expect-no-navigation-activity" => {
                    let value = args.next().ok_or_else(|| {
                        "missing value for --expect-no-navigation-activity".to_owned()
                    })?;
                    config
                        .expect_no_navigation_activity
                        .push(parse_view_identity_arg(
                            "--expect-no-navigation-activity",
                            &value,
                        )?);
                }
                "--expect-navigation-node" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-navigation-node".to_owned())?;
                    config
                        .expect_navigation_nodes
                        .push(parse_navigation_node_expectation(&value)?);
                }
                "--expect-mdi-placement" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-mdi-placement".to_owned())?;
                    config
                        .expect_mdi_placements
                        .push(parse_mdi_placement_expectation(&value)?);
                }
                "--expect-mdi-title" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-mdi-title".to_owned())?;
                    config
                        .expect_mdi_titles
                        .push(parse_mdi_title_expectation(&value)?);
                }
                "--expect-dialog-title" => {
                    config.expect_dialog_title = Some(
                        args.next()
                            .ok_or_else(|| "missing value for --expect-dialog-title".to_owned())?,
                    );
                }
                "--help" => {
                    return Err(usage());
                }
                other => {
                    return Err(format!("unknown argument: {other}\n{}", usage()));
                }
            }
        }

        let terminal_actions = config
            .actions
            .iter()
            .filter(|action| action.is_terminal())
            .count();
        if terminal_actions > 1
            || (terminal_actions == 1 && !config.actions.last().is_some_and(Action::is_terminal))
        {
            return Err(format!(
                "a dialog: or drag: action must be the only one and the last: a dialog disables the frame, and a drag holds the mouse\n{}",
                usage()
            ));
        }
        let dialog_actions = config
            .actions
            .iter()
            .filter(|action| matches!(action, Action::Dialog { .. }))
            .count();
        if config.expect_dialog_title.is_some() && dialog_actions == 0 {
            return Err(format!(
                "--expect-dialog-title needs a dialog: action\n{}",
                usage()
            ));
        }

        Ok(config)
    }

    pub(crate) fn usage() -> String {
        [
            "usage: mirx-snapshot-driver [options]",
            "  --find-timeout-ms <ms>",
            "  --wait-idle-ms <ms>",
            "  --select-view <identity> (repeatable)",
            "  --trigger-history-backfill",
            "  --append-events <path>",
            "  --find-active-view <query>",
            "  --action <find:<query>|input:<text>|append:<path>|arrange:<arrangement>|window-state:<identity>=<state>|collapse:<node>|dialog:<dialog>[:<options page>]|drag:<point>:<point>|colors:<item>=<index>[,...]|compose-text:<text>|compose-key:<format key>> (repeatable)",
            "  <item> is a Colors dialog item's mirx.ini key such as join or editbox_text, and <index> a palette index 0-15",
            "  <format key> is bold, color, underline, italic, reset, reverse, or strikethrough",
            "  <node> is a treebar item: space:<room_id> for a space folder, or a view identity",
            "  <arrangement> is cascade, tile_horizontal, tile_vertical, or arrange_icons",
            "  <state> is maximized, normal, or minimized",
            "  <dialog> is options, colors, select_server, about, commands_join, recover_e2ee, setup_e2ee_recovery, or close_active_view; it must be the last action",
            "  <options page> is an Options tree path such as IRC/Messages",
            "  <point> is <line>,<offset>: a byte offset into the text of a visible line of the active timeline, from 0; the drag is not released, and it must be the last action",
            "  --expect-active-find-history <local_only|loading|exhausted|failed>",
            "  --expect-active-find-selected-text <substring>",
            "  --expect-selected <room_id>",
            "  --expect-room <room_id>",
            "  --expect-room-membership <room_id>::<join|invite>",
            "  --expect-no-room <room_id>",
            "  --expect-direct-room <room_id>",
            "  --expect-room-tag <room_id>::<tag>::<order-or-null>",
            "  --expect-room-read-target <room_id>::<event-id-or-null>",
            "  --expect-room-unread <room_id>::<num_unread>::<num_notifications>::<num_mentions>::<marked_unread>",
            "  --expect-room-activity <room_id>::<substring>",
            "  --expect-error <substring>",
            "  --expect-verification <flow_id>",
            "  --expect-verification-state <requested|accepted|ready|sas_ready|confirmed|done|cancelled>",
            "  --expect-verification-text <substring>",
            "  --expect-active-view <status|room:<room_id>|thread:<room_id>|<thread_root_event_id>>",
            "  --expect-view <identity>::<substring>",
            "  --expect-no-view <identity>::<substring>",
            "  --expect-room-view <room_id>::<substring>",
            "  --expect-room-member <room_id>::<substring>",
            "  --expect-status-view <substring>",
            "  --expect-no-status-view <substring>",
            "  --expect-status-bar <substring>",
            "  --expect-no-status-bar <substring>",
            "  --expect-switchbar-item <identity>::<title-substring>",
            "  <identity> is status, room:<room_id>, or thread:<room_id>|<thread_root_event_id>",
            "  --expect-no-switchbar-item <identity>",
            "  --expect-switchbar-active <identity>",
            "  --expect-switchbar-rows <count>",
            "  --expect-frame-bar-hidden <switchbar|treebar>",
            "  --expect-switchbar-activity <identity>::<event|message|highlight>",
            "  --expect-no-switchbar-activity <identity>",
            "  --expect-navigation-text <substring>",
            "  --expect-no-navigation-text <substring>",
            "  --expect-navigation-activity <identity>::<event|message|highlight>",
            "  --expect-no-navigation-activity <identity>",
            "  --expect-navigation-node <node>::<expanded|collapsed>::<event|message|highlight|none>",
            "  --expect-mdi-placement <identity>::<state>",
            "  --expect-mdi-title <identity>::<caption>",
            "  --expect-dialog-title <title> (needs a dialog: action)",
        ]
        .join("\n")
    }

    fn parse_action(value: &str) -> Result<Action, String> {
        if let Some(query) = value.strip_prefix("find:") {
            return Ok(Action::Find(query.to_owned()));
        }
        if let Some(text) = value.strip_prefix("input:") {
            return Ok(Action::Input(text.to_owned()));
        }
        if let Some(path) = value.strip_prefix("append:") {
            return Ok(Action::Append(path.to_owned()));
        }
        let invalid = || format!("invalid --action value {value:?}\n{}", usage());
        if let Some(arrangement) = value.strip_prefix("arrange:") {
            return parse_wire_name(arrangement)
                .map(Action::Arrange)
                .ok_or_else(invalid);
        }
        if let Some(target) = value.strip_prefix("dialog:") {
            let (name, options_page) = match target.split_once(':') {
                Some((name, page)) => (name, Some(page)),
                None => (target, None),
            };
            let dialog = parse_wire_name(name).ok_or_else(invalid)?;
            if options_page
                .is_some_and(|page| page.is_empty() || dialog != AutomationDialog::Options)
            {
                return Err(invalid());
            }
            return Ok(Action::Dialog {
                dialog,
                options_page: options_page.map(str::to_owned),
            });
        }
        if let Some(pairs) = value.strip_prefix("colors:") {
            return pairs
                .split(',')
                .map(|pair| {
                    let (key, index) = pair.split_once('=')?;
                    let index = index.parse::<u8>().ok().filter(|index| *index < 16)?;
                    (!key.is_empty()).then(|| (key.to_owned(), index))
                })
                .collect::<Option<Vec<_>>>()
                .map(Action::Colors)
                .ok_or_else(invalid);
        }
        if let Some(text) = value.strip_prefix("compose-text:") {
            return Ok(Action::ComposeText(text.to_owned()));
        }
        if let Some(key) = value.strip_prefix("compose-key:") {
            return parse_wire_name(key)
                .map(Action::ComposeKey)
                .ok_or_else(invalid);
        }
        if let Some(points) = value.strip_prefix("drag:") {
            let (from, to) = points.split_once(':').ok_or_else(invalid)?;
            let point = |text: &str| {
                let (line, offset) = text.split_once(',')?;
                Some(TextPoint {
                    line: line.parse().ok()?,
                    offset: offset.parse().ok()?,
                })
            };
            return Ok(Action::Drag {
                from: point(from).ok_or_else(invalid)?,
                to: point(to).ok_or_else(invalid)?,
            });
        }
        if let Some(node) = value.strip_prefix("collapse:") {
            return parse_navigation_node_arg("--action", node).map(Action::Collapse);
        }
        if let Some(target) = value.strip_prefix("window-state:") {
            let (identity, state) = target.rsplit_once('=').ok_or_else(invalid)?;
            let state = parse_wire_name(state).ok_or_else(invalid)?;
            return Ok(Action::WindowState(
                parse_view_identity_arg("--action", identity)?,
                state,
            ));
        }
        Err(invalid())
    }

    /// Identities contain no `::`, so the caption is everything after the
    /// first one.
    pub(crate) fn parse_mdi_title_expectation(value: &str) -> Result<MdiTitleExpectation, String> {
        let (identity, title) = value.split_once("::").ok_or_else(|| {
            format!(
                "invalid --expect-mdi-title value {value:?}; expected <identity>::<caption>\n{}",
                usage()
            )
        })?;
        Ok(MdiTitleExpectation {
            identity: parse_view_identity_arg("--expect-mdi-title", identity)?,
            title: title.to_owned(),
        })
    }

    pub(crate) fn parse_mdi_placement_expectation(
        value: &str,
    ) -> Result<MdiPlacementExpectation, String> {
        let invalid = || {
            format!(
                "invalid --expect-mdi-placement value {value:?}; expected <identity>::<state>\n{}",
                usage()
            )
        };
        let (identity, state) = value.rsplit_once("::").ok_or_else(invalid)?;
        Ok(MdiPlacementExpectation {
            identity: parse_view_identity_arg("--expect-mdi-placement", identity)?,
            state: parse_wire_name(state).ok_or_else(invalid)?,
        })
    }

    pub(crate) fn parse_view_identity_arg(
        flag_name: &str,
        value: &str,
    ) -> Result<ViewIdentity, String> {
        if value == "status" {
            return Ok(ViewIdentity::Status);
        }
        if value == "room_list" {
            return Ok(ViewIdentity::RoomList);
        }
        if let Some(room_id) = value.strip_prefix("room:") {
            if room_id.is_empty() {
                return Err(format!(
                    "invalid {flag_name} value {value:?}; expected status, room_list, room:<room_id>, or thread:<room_id>|<thread_root_event_id>\n{}",
                    usage()
                ));
            }
            return Ok(ViewIdentity::Room {
                room_id: room_id.to_owned(),
            });
        }
        if let Some(thread_identity) = value.strip_prefix("thread:") {
            let Some((room_id, thread_root_event_id)) = thread_identity.split_once('|') else {
                return Err(format!(
                    "invalid {flag_name} value {value:?}; expected status, room_list, room:<room_id>, or thread:<room_id>|<thread_root_event_id>\n{}",
                    usage()
                ));
            };
            if room_id.is_empty() || thread_root_event_id.is_empty() {
                return Err(format!(
                    "invalid {flag_name} value {value:?}; expected status, room_list, room:<room_id>, or thread:<room_id>|<thread_root_event_id>\n{}",
                    usage()
                ));
            }
            return Ok(ViewIdentity::Thread {
                room_id: room_id.to_owned(),
                thread_root_event_id: thread_root_event_id.to_owned(),
            });
        }
        Err(format!(
            "invalid {flag_name} value {value:?}; expected status, room_list, room:<room_id>, or thread:<room_id>|<thread_root_event_id>\n{}",
            usage()
        ))
    }

    pub(crate) fn parse_view_expectation(
        flag_name: &str,
        value: &str,
    ) -> Result<ViewExpectation, String> {
        let (identity, substring) = value.split_once("::").ok_or_else(|| {
            format!("invalid {flag_name} value {value:?}; expected <identity>::<substring>")
        })?;
        Ok(ViewExpectation {
            identity: parse_view_identity_arg(flag_name, identity)?,
            substring: substring.to_owned(),
        })
    }

    pub(crate) fn parse_room_membership_expectation(
        value: &str,
    ) -> Result<RoomMembershipExpectation, String> {
        let (room_id, membership) = value.split_once("::").ok_or_else(|| {
            format!(
                "invalid --expect-room-membership value {value:?}; expected <room_id>::<join|invite>"
            )
        })?;
        if room_id.is_empty() {
            return Err(format!(
                "invalid --expect-room-membership value {value:?}; expected <room_id>::<join|invite>"
            ));
        }
        Ok(RoomMembershipExpectation {
            room_id: room_id.to_owned(),
            membership: parse_room_membership_arg(membership, value)?,
        })
    }

    pub(crate) fn parse_room_tag_expectation(value: &str) -> Result<RoomTagExpectation, String> {
        let parts: Vec<_> = value.split("::").collect();
        if parts.len() != 3 || parts.iter().any(|part| part.is_empty()) {
            return Err(format!(
                "invalid --expect-room-tag value {value:?}; expected <room_id>::<tag>::<order-or-null>"
            ));
        }
        Ok(RoomTagExpectation {
            room_id: parts[0].to_owned(),
            tag: parts[1].to_owned(),
            order: parse_optional_null(parts[2]),
        })
    }

    pub(crate) fn parse_room_read_target_expectation(
        value: &str,
    ) -> Result<RoomReadTargetExpectation, String> {
        let parts: Vec<_> = value.split("::").collect();
        if parts.len() != 2 || parts.iter().any(|part| part.is_empty()) {
            return Err(format!(
                "invalid --expect-room-read-target value {value:?}; expected <room_id>::<event-id-or-null>"
            ));
        }
        Ok(RoomReadTargetExpectation {
            room_id: parts[0].to_owned(),
            read_target_event_id: parse_optional_null(parts[1]),
        })
    }

    pub(crate) fn parse_room_unread_expectation(
        value: &str,
    ) -> Result<RoomUnreadExpectation, String> {
        let parts: Vec<_> = value.split("::").collect();
        if parts.len() != 5 || parts.iter().any(|part| part.is_empty()) {
            return Err(format!(
                "invalid --expect-room-unread value {value:?}; expected <room_id>::<num_unread>::<num_notifications>::<num_mentions>::<marked_unread>"
            ));
        }

        Ok(RoomUnreadExpectation {
            room_id: parts[0].to_owned(),
            num_unread: parts[1].parse::<u64>().map_err(|_| {
                format!("invalid --expect-room-unread num_unread {value:?}; expected u64")
            })?,
            num_notifications: parts[2].parse::<u64>().map_err(|_| {
                format!("invalid --expect-room-unread num_notifications {value:?}; expected u64")
            })?,
            num_mentions: parts[3].parse::<u64>().map_err(|_| {
                format!("invalid --expect-room-unread num_mentions {value:?}; expected u64")
            })?,
            marked_unread: parse_strict_bool_arg("--expect-room-unread", value, parts[4])?,
        })
    }

    fn parse_optional_null(value: &str) -> Option<String> {
        if value == "null" {
            None
        } else {
            Some(value.to_owned())
        }
    }

    fn parse_room_membership_arg(value: &str, full_value: &str) -> Result<RoomMembership, String> {
        match value {
            "join" => Ok(RoomMembership::Join),
            "invite" => Ok(RoomMembership::Invite),
            _ => Err(format!(
                "invalid --expect-room-membership value {full_value:?}; expected <room_id>::<join|invite>"
            )),
        }
    }

    fn parse_strict_bool_arg(
        flag_name: &str,
        full_value: &str,
        value: &str,
    ) -> Result<bool, String> {
        match value {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(format!(
                "invalid {flag_name} marked_unread {full_value:?}; expected true or false"
            )),
        }
    }

    pub(crate) fn format_view_identity(identity: &ViewIdentity) -> String {
        match identity {
            ViewIdentity::Status => "status".to_owned(),
            ViewIdentity::RoomList => "room_list".to_owned(),
            ViewIdentity::Room { room_id } => format!("room:{room_id}"),
            ViewIdentity::Thread {
                room_id,
                thread_root_event_id,
            } => format!("thread:{room_id}|{thread_root_event_id}"),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{
            Action, FrameBar, MdiPlacementExpectation, MdiTitleExpectation,
            NavigationActivityExpectation, NavigationNodeExpectation, RoomMembershipExpectation,
            RoomReadTargetExpectation, RoomTagExpectation, RoomUnreadExpectation,
            SwitchbarActivityExpectation, TextPoint, ViewExpectation, format_view_identity,
            parse_args, parse_view_expectation, parse_view_identity_arg, usage,
        };
        use mirx_core::RoomMembership;
        use mirx_testkit::{
            AutomationDialog, MdiArrangement, MdiWindowState, MircFormatKey, NavigationNodeKey,
            ViewActivityLevel, ViewIdentity,
        };
        use std::time::Duration;

        #[test]
        fn usage_documents_thread_identity_format() {
            assert!(usage().contains(
                "--expect-active-view <status|room:<room_id>|thread:<room_id>|<thread_root_event_id>>"
            ));
            assert!(usage().contains("--select-view <identity> (repeatable)"));
            assert!(usage().contains("--append-events <path>"));
            assert!(usage().contains(
                "--action <find:<query>|input:<text>|append:<path>|arrange:<arrangement>|window-state:<identity>=<state>|collapse:<node>|dialog:<dialog>[:<options page>]|drag:<point>:<point>|colors:<item>=<index>[,...]|compose-text:<text>|compose-key:<format key>> (repeatable)"
            ));
            assert!(usage().contains(
                "--expect-navigation-node <node>::<expanded|collapsed>::<event|message|highlight|none>"
            ));
            assert!(usage().contains("--expect-dialog-title <title> (needs a dialog: action)"));
            assert!(usage().contains("--expect-mdi-placement <identity>::<state>"));
            assert!(usage().contains("--expect-mdi-title <identity>::<caption>"));
            assert!(
                usage()
                    .contains("--expect-active-find-history <local_only|loading|exhausted|failed>")
            );
            assert!(usage().contains("--expect-active-find-selected-text <substring>"));
            assert!(usage().contains("--expect-view <identity>::<substring>"));
            assert!(usage().contains("--expect-no-view <identity>::<substring>"));
            assert!(usage().contains("--expect-room-membership <room_id>::<join|invite>"));
            assert!(usage().contains("--expect-no-room <room_id>"));
            assert!(usage().contains("--expect-direct-room <room_id>"));
            assert!(usage().contains("--expect-room-tag <room_id>::<tag>::<order-or-null>"));
            assert!(usage().contains("--expect-room-read-target <room_id>::<event-id-or-null>"));
            assert!(usage().contains(
                "--expect-room-unread <room_id>::<num_unread>::<num_notifications>::<num_mentions>::<marked_unread>"
            ));
            assert!(usage().contains("thread:<room_id>|<thread_root_event_id>"));
            assert!(
                usage()
                    .contains("--expect-navigation-activity <identity>::<event|message|highlight>")
            );
            assert!(usage().contains("--expect-no-navigation-activity <identity>"));
        }

        #[test]
        fn usage_documents_trigger_history_backfill_flag() {
            assert!(usage().contains("--trigger-history-backfill"));
        }

        #[test]
        fn usage_documents_find_active_view_flag() {
            assert!(usage().contains("--find-active-view <query>"));
        }

        #[test]
        fn parse_view_identity_arg_accepts_thread_identity() {
            assert_eq!(
                parse_view_identity_arg(
                    "--expect-active-view",
                    "thread:!room:example|$thread-root:example"
                )
                .unwrap(),
                ViewIdentity::Thread {
                    room_id: "!room:example".to_owned(),
                    thread_root_event_id: "$thread-root:example".to_owned(),
                }
            );
        }

        #[test]
        fn parse_view_identity_arg_rejects_missing_thread_components() {
            for value in [
                "thread:",
                "thread:!room:example",
                "thread:!room:example|",
                "thread:|$thread-root:example",
            ] {
                assert!(
                    parse_view_identity_arg("--expect-active-view", value).is_err(),
                    "expected {value:?} to be rejected"
                );
            }
        }

        #[test]
        fn format_view_identity_formats_thread_identity() {
            assert_eq!(
                format_view_identity(&ViewIdentity::Thread {
                    room_id: "!room:example".to_owned(),
                    thread_root_event_id: "$thread-root:example".to_owned(),
                }),
                "thread:!room:example|$thread-root:example"
            );
        }

        #[test]
        fn parse_args_accepts_select_view_thread_identity() {
            let config = parse_args(
                ["--select-view", "thread:!room:example|$thread-root:example"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(
                config.select_views,
                vec![ViewIdentity::Thread {
                    room_id: "!room:example".to_owned(),
                    thread_root_event_id: "$thread-root:example".to_owned(),
                }]
            );
        }

        #[test]
        fn parse_args_preserves_repeated_select_view_order() {
            let config = parse_args(
                [
                    "--select-view",
                    "room:!room:example",
                    "--select-view",
                    "thread:!room:example|$thread-root:example",
                    "--select-view",
                    "status",
                ]
                .into_iter()
                .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(
                config.select_views,
                vec![
                    ViewIdentity::Room {
                        room_id: "!room:example".to_owned(),
                    },
                    ViewIdentity::Thread {
                        room_id: "!room:example".to_owned(),
                        thread_root_event_id: "$thread-root:example".to_owned(),
                    },
                    ViewIdentity::Status,
                ]
            );
        }

        #[test]
        fn parse_args_accepts_append_events_path() {
            let config = parse_args(
                ["--append-events", "C:\\snapshots\\thread-update.json"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(
                config.append_events,
                Some("C:\\snapshots\\thread-update.json".to_owned())
            );
            assert_eq!(config.find_timeout, Duration::from_secs(10));
            assert_eq!(config.wait_idle, Duration::from_secs(5));
        }

        #[test]
        fn parse_args_accepts_find_active_view_query() {
            let config = parse_args(
                ["--find-active-view", "alpha needle"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(config.find_active_view, Some("alpha needle".to_owned()));
        }

        #[test]
        fn parse_args_preserves_ordered_actions() {
            let config = parse_args(
                [
                    "--action",
                    "find:older needle",
                    "--action",
                    "input:/kick Bob  spaced reason",
                    "--action",
                    "append:C:\\snapshots\\backfill.json",
                ]
                .into_iter()
                .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(
                config.actions,
                vec![
                    Action::Find("older needle".to_owned()),
                    Action::Input("/kick Bob  spaced reason".to_owned()),
                    Action::Append("C:\\snapshots\\backfill.json".to_owned()),
                ]
            );
        }

        #[test]
        fn parse_args_accepts_window_actions_and_placement_expectations() {
            let config = parse_args(
                [
                    "--action",
                    "arrange:tile_vertical",
                    "--action",
                    "arrange:arrange_icons",
                    "--action",
                    "window-state:room:!lobby:example=minimized",
                    "--expect-mdi-placement",
                    "thread:!room:example|$root::normal",
                    "--expect-mdi-title",
                    "room:!ops:example::Ops [e2ee]: a::b",
                ]
                .into_iter()
                .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(
                config.actions,
                vec![
                    Action::Arrange(MdiArrangement::TileVertical),
                    Action::Arrange(MdiArrangement::ArrangeIcons),
                    Action::WindowState(
                        ViewIdentity::Room {
                            room_id: "!lobby:example".to_owned()
                        },
                        MdiWindowState::Minimized
                    ),
                ]
            );
            assert_eq!(
                config.expect_mdi_placements,
                vec![MdiPlacementExpectation {
                    identity: ViewIdentity::Thread {
                        room_id: "!room:example".to_owned(),
                        thread_root_event_id: "$root".to_owned(),
                    },
                    state: MdiWindowState::Normal,
                }]
            );
            assert_eq!(
                config.expect_mdi_titles,
                vec![MdiTitleExpectation {
                    identity: ViewIdentity::Room {
                        room_id: "!ops:example".to_owned()
                    },
                    title: "Ops [e2ee]: a::b".to_owned(),
                }]
            );
        }

        #[test]
        fn parse_args_rejects_bad_switchbar_rows_and_bars() {
            for value in ["0", "-1", "two"] {
                assert!(
                    parse_args(
                        ["--expect-switchbar-rows", value]
                            .into_iter()
                            .map(str::to_owned)
                    )
                    .is_err(),
                    "{value:?} must be rejected"
                );
            }
            assert!(
                parse_args(
                    ["--expect-frame-bar-hidden", "toolbar"]
                        .into_iter()
                        .map(str::to_owned)
                )
                .is_err()
            );
        }

        #[test]
        fn parse_args_accepts_a_last_dialog_action_and_its_title() {
            for (action, expected) in [
                (
                    "dialog:options:IRC/Messages",
                    Action::Dialog {
                        dialog: AutomationDialog::Options,
                        options_page: Some("IRC/Messages".to_owned()),
                    },
                ),
                (
                    "dialog:options",
                    Action::Dialog {
                        dialog: AutomationDialog::Options,
                        options_page: None,
                    },
                ),
                (
                    "dialog:select_server",
                    Action::Dialog {
                        dialog: AutomationDialog::SelectServer,
                        options_page: None,
                    },
                ),
                (
                    "dialog:close_active_view",
                    Action::Dialog {
                        dialog: AutomationDialog::CloseActiveView,
                        options_page: None,
                    },
                ),
            ] {
                let config = parse_args(
                    [
                        "--action",
                        "arrange:cascade",
                        "--action",
                        action,
                        "--expect-dialog-title",
                        "mirx Options",
                    ]
                    .into_iter()
                    .map(str::to_owned),
                )
                .unwrap();
                assert_eq!(
                    config.actions,
                    [Action::Arrange(MdiArrangement::Cascade), expected]
                );
                assert_eq!(config.expect_dialog_title.as_deref(), Some("mirx Options"));
            }
        }

        #[test]
        fn parse_args_reads_compose_text_and_key_actions() {
            let config = parse_args(
                [
                    "--action",
                    "compose-text:hello ",
                    "--action",
                    "compose-key:color",
                    "--action",
                    "compose-key:strikethrough",
                ]
                .into_iter()
                .map(str::to_owned),
            )
            .unwrap();
            assert_eq!(
                config.actions,
                [
                    Action::ComposeText("hello ".to_owned()),
                    Action::ComposeKey(MircFormatKey::Color),
                    Action::ComposeKey(MircFormatKey::Strikethrough)
                ]
            );
            for bad in ["compose-key:", "compose-key:Color", "compose-key:return"] {
                assert!(
                    parse_args(["--action", bad].into_iter().map(str::to_owned)).is_err(),
                    "{bad}"
                );
            }
        }

        #[test]
        fn parse_args_reads_colors_actions() {
            let config = parse_args(
                ["--action", "colors:join=4,background=1"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();
            assert_eq!(
                config.actions,
                [Action::Colors(vec![
                    ("join".to_owned(), 4),
                    ("background".to_owned(), 1)
                ])]
            );
            for bad in [
                "colors:",
                "colors:join",
                "colors:join=16",
                "colors:=4",
                "colors:join=4,",
                "colors:join=x",
            ] {
                assert!(
                    parse_args(["--action", bad].into_iter().map(str::to_owned)).is_err(),
                    "{bad}"
                );
            }
        }

        #[test]
        fn parse_args_rejects_bad_dialog_actions() {
            for args in [
                &["--action", "dialog:no_such"][..],
                &["--action", "dialog:select-server"],
                &["--action", "dialog:about:Display"],
                &["--action", "dialog:options:"],
                &["--action", "dialog:"],
                // The dialog disables the frame: nothing may follow it.
                &["--action", "dialog:about", "--action", "arrange:cascade"],
                &["--action", "dialog:about", "--action", "dialog:options"],
                &["--expect-dialog-title", "About mirx"],
                &["--action", "dialog:about", "--expect-dialog-title"],
            ] {
                assert!(
                    parse_args(args.iter().map(|arg| (*arg).to_owned())).is_err(),
                    "{args:?} must be rejected"
                );
            }
        }

        #[test]
        fn parse_args_reads_collapse_actions_and_navigation_node_expectations() {
            let config = parse_args(
                [
                    "--action",
                    "collapse:space:!work:example",
                    "--action",
                    "collapse:status",
                    "--expect-navigation-node",
                    "space:!work:example::collapsed::highlight",
                    "--expect-navigation-node",
                    "room:!ops:example::expanded::none",
                ]
                .into_iter()
                .map(str::to_owned),
            )
            .unwrap();
            assert_eq!(
                config.actions,
                [
                    Action::Collapse(NavigationNodeKey::Space("!work:example".to_owned())),
                    Action::Collapse(NavigationNodeKey::View(ViewIdentity::Status)),
                ]
            );
            assert_eq!(
                config.expect_navigation_nodes,
                [
                    NavigationNodeExpectation {
                        node: NavigationNodeKey::Space("!work:example".to_owned()),
                        expanded: false,
                        activity: Some(ViewActivityLevel::Highlight),
                    },
                    NavigationNodeExpectation {
                        node: NavigationNodeKey::View(ViewIdentity::Room {
                            room_id: "!ops:example".to_owned(),
                        }),
                        expanded: true,
                        activity: None,
                    },
                ]
            );
            for args in [
                &["--action", "collapse:space:"][..],
                &["--action", "collapse:!work:example"],
                &["--expect-navigation-node", "space:!work:example::collapsed"],
                &[
                    "--expect-navigation-node",
                    "space:!work:example::folded::none",
                ],
                &[
                    "--expect-navigation-node",
                    "space:!work:example::collapsed::red",
                ],
                &["--expect-navigation-node"],
            ] {
                assert!(
                    parse_args(args.iter().map(|arg| (*arg).to_owned())).is_err(),
                    "{args:?} must be rejected"
                );
            }
        }

        #[test]
        fn parse_args_accepts_a_last_drag_action() {
            let config = parse_args(
                ["--action", "find:needle", "--action", "drag:3,12:5,0"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();
            assert_eq!(
                config.actions,
                [
                    Action::Find("needle".to_owned()),
                    Action::Drag {
                        from: TextPoint {
                            line: 3,
                            offset: 12
                        },
                        to: TextPoint { line: 5, offset: 0 },
                    }
                ]
            );
        }

        #[test]
        fn parse_args_rejects_bad_drag_actions() {
            for args in [
                &["--action", "drag:"][..],
                &["--action", "drag:3,12"],
                &["--action", "drag:3:5"],
                &["--action", "drag:3,12:5,"],
                &["--action", "drag:-1,0:5,0"],
                &["--action", "drag:3,12:5,0:7,0"],
                // The drag holds the mouse: nothing may follow it.
                &["--action", "drag:3,12:5,0", "--action", "find:needle"],
                &["--action", "drag:3,12:5,0", "--action", "drag:1,0:2,0"],
                &["--action", "drag:3,12:5,0", "--action", "dialog:about"],
                &["--action", "dialog:about", "--action", "drag:3,12:5,0"],
            ] {
                assert!(
                    parse_args(args.iter().map(|arg| (*arg).to_owned())).is_err(),
                    "{args:?} must be rejected"
                );
            }
        }

        #[test]
        fn parse_args_rejects_unknown_window_actions_and_states() {
            for args in [
                ["--action", "arrange:tile"],
                ["--action", "window-state:status=hidden"],
                ["--action", "window-state:status"],
                ["--expect-mdi-placement", "status::hidden"],
                ["--expect-mdi-placement", "status"],
                ["--expect-mdi-title", "status"],
                ["--expect-mdi-title", "nowhere::Status"],
            ] {
                assert!(
                    parse_args(args.into_iter().map(str::to_owned)).is_err(),
                    "{args:?} must be rejected"
                );
            }
        }

        #[test]
        fn parse_args_accepts_active_find_expectations() {
            let config = parse_args(
                [
                    "--expect-active-find-history",
                    "loading",
                    "--expect-active-find-selected-text",
                    "older needle",
                ]
                .into_iter()
                .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(
                config.expect_active_find_history.as_deref(),
                Some("loading")
            );
            assert_eq!(
                config.expect_active_find_selected_text.as_deref(),
                Some("older needle")
            );
        }

        #[test]
        fn parse_args_accepts_trigger_history_backfill_flag() {
            let config = parse_args(
                ["--trigger-history-backfill"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();

            assert!(config.trigger_history_backfill);
        }

        #[test]
        fn parse_args_rejects_unknown_argument() {
            let error = parse_args(
                ["--trigger-history-backfill", "--unknown-flag"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .err()
            .unwrap();

            assert!(error.contains("unknown argument: --unknown-flag"));
            assert!(error.contains("--trigger-history-backfill"));
        }

        #[test]
        fn parse_args_reads_runtime_expectation_fields() {
            let config = parse_args(
                [
                    "--expect-selected",
                    "!ops:example",
                    "--expect-room",
                    "!work:example",
                    "--expect-room-membership",
                    "!invite:example::invite",
                    "--expect-no-room",
                    "!missing:example",
                    "--expect-room-activity",
                    "!ops:example::incident review",
                    "--expect-error",
                    "boom",
                    "--expect-verification",
                    "flow-1",
                    "--expect-verification-state",
                    "requested",
                    "--expect-verification-text",
                    "emoji",
                    "--expect-active-view",
                    "room:!ops:example",
                    "--expect-room-view",
                    "!ops:example::timeline",
                    "--expect-room-member",
                    "!ops:example::alice",
                    "--expect-status-view",
                    "status line",
                    "--expect-no-status-view",
                    "forbidden status",
                    "--expect-status-bar",
                    "connected",
                    "--expect-no-status-bar",
                    "disconnected",
                    "--expect-switchbar-item",
                    "thread:!ops:example|$thread-root::root msg",
                    "--expect-no-switchbar-item",
                    "status",
                    "--expect-switchbar-active",
                    "room:!ops:example",
                    "--expect-switchbar-rows",
                    "2",
                    "--expect-frame-bar-hidden",
                    "treebar",
                    "--expect-frame-bar-hidden",
                    "switchbar",
                    "--expect-switchbar-activity",
                    "thread:!ops:example|$thread-root::highlight",
                    "--expect-no-switchbar-activity",
                    "room:!ops:example",
                    "--expect-navigation-text",
                    "Ops",
                    "--expect-no-navigation-text",
                    "plain",
                    "--expect-navigation-activity",
                    "thread:!alpha:example|$root::event",
                    "--expect-no-navigation-activity",
                    "room:!beta:example",
                ]
                .into_iter()
                .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(config.expect_selected, Some("!ops:example".to_owned()));
            assert_eq!(config.expect_rooms, vec!["!work:example".to_owned()]);
            assert_eq!(
                config.expect_room_memberships,
                vec![RoomMembershipExpectation {
                    room_id: "!invite:example".to_owned(),
                    membership: RoomMembership::Invite,
                }]
            );
            assert_eq!(config.expect_no_rooms, vec!["!missing:example".to_owned()]);
            assert_eq!(config.expect_room_activities.len(), 1);
            assert_eq!(config.expect_room_activities[0].room_id, "!ops:example");
            assert_eq!(
                config.expect_room_activities[0].substring,
                "incident review"
            );
            assert_eq!(config.expect_error, Some("boom".to_owned()));
            assert_eq!(config.expect_verification, Some("flow-1".to_owned()));
            assert_eq!(
                config.expect_verification_state,
                Some("requested".to_owned())
            );
            assert_eq!(config.expect_verification_texts, vec!["emoji".to_owned()]);
            assert_eq!(
                config.expect_active_view,
                Some(ViewIdentity::Room {
                    room_id: "!ops:example".to_owned(),
                })
            );
            assert_eq!(config.expect_room_views.len(), 1);
            assert_eq!(config.expect_room_views[0].room_id, "!ops:example");
            assert_eq!(config.expect_room_views[0].substring, "timeline");
            assert_eq!(config.expect_room_members.len(), 1);
            assert_eq!(config.expect_room_members[0].room_id, "!ops:example");
            assert_eq!(config.expect_room_members[0].substring, "alice");
            assert_eq!(config.expect_status_views, vec!["status line".to_owned()]);
            assert_eq!(
                config.expect_no_status_views,
                vec!["forbidden status".to_owned()]
            );
            assert_eq!(config.expect_status_bars, vec!["connected".to_owned()]);
            assert_eq!(
                config.expect_no_status_bars,
                vec!["disconnected".to_owned()]
            );
            assert_eq!(config.expect_switchbar_items.len(), 1);
            assert_eq!(
                config.expect_switchbar_items[0].identity,
                ViewIdentity::Thread {
                    room_id: "!ops:example".to_owned(),
                    thread_root_event_id: "$thread-root".to_owned(),
                }
            );
            assert_eq!(config.expect_switchbar_items[0].title_substring, "root msg");
            assert_eq!(config.expect_no_switchbar_items, vec![ViewIdentity::Status]);
            assert_eq!(
                config.expect_switchbar_active,
                Some(ViewIdentity::Room {
                    room_id: "!ops:example".to_owned(),
                })
            );
            assert_eq!(config.expect_switchbar_rows, Some(2));
            assert_eq!(
                config.expect_frame_bars_hidden,
                vec![FrameBar::Treebar, FrameBar::Switchbar]
            );
            assert_eq!(
                config.expect_switchbar_activity,
                vec![SwitchbarActivityExpectation {
                    identity: ViewIdentity::Thread {
                        room_id: "!ops:example".to_owned(),
                        thread_root_event_id: "$thread-root".to_owned(),
                    },
                    level: ViewActivityLevel::Highlight,
                }]
            );
            assert_eq!(
                config.expect_no_switchbar_activity,
                vec![ViewIdentity::Room {
                    room_id: "!ops:example".to_owned(),
                }]
            );
            assert_eq!(config.expect_navigation_texts, vec!["Ops".to_owned()]);
            assert_eq!(config.expect_no_navigation_texts, vec!["plain".to_owned()]);
            assert_eq!(
                config.expect_navigation_activity,
                vec![NavigationActivityExpectation {
                    identity: ViewIdentity::Thread {
                        room_id: "!alpha:example".to_owned(),
                        thread_root_event_id: "$root".to_owned(),
                    },
                    level: ViewActivityLevel::Event,
                }]
            );
            assert_eq!(
                config.expect_no_navigation_activity,
                vec![ViewIdentity::Room {
                    room_id: "!beta:example".to_owned(),
                }]
            );
        }

        #[test]
        fn parse_args_rejects_missing_append_events_value() {
            assert_eq!(
                parse_args(["--append-events"].into_iter().map(str::to_owned))
                    .err()
                    .unwrap(),
                "missing value for --append-events"
            );
        }

        #[test]
        fn parse_args_rejects_missing_find_active_view_value() {
            assert_eq!(
                parse_args(["--find-active-view"].into_iter().map(str::to_owned))
                    .err()
                    .unwrap(),
                "missing value for --find-active-view"
            );
        }

        #[test]
        fn parse_args_rejects_missing_navigation_activity_values() {
            assert_eq!(
                parse_args(
                    ["--expect-navigation-activity"]
                        .into_iter()
                        .map(str::to_owned),
                )
                .err()
                .unwrap(),
                "missing value for --expect-navigation-activity"
            );
            assert_eq!(
                parse_args(
                    ["--expect-no-navigation-activity"]
                        .into_iter()
                        .map(str::to_owned),
                )
                .err()
                .unwrap(),
                "missing value for --expect-no-navigation-activity"
            );
        }

        #[test]
        fn parse_args_rejects_activity_expectations_without_a_level() {
            for (flag, value) in [
                ("--expect-switchbar-activity", "room:!ops:example"),
                ("--expect-switchbar-activity", "room:!ops:example::loud"),
                ("--expect-navigation-activity", "room:!beta:example"),
            ] {
                let error = parse_args([flag, value].into_iter().map(str::to_owned))
                    .err()
                    .unwrap();
                assert!(
                    error.starts_with(&format!(
                        "invalid {flag} value {value:?}; expected <target>::<event|message|highlight>"
                    )),
                    "{error}"
                );
            }
        }

        #[test]
        fn parse_args_accepts_generic_thread_view_expectation() {
            let config = parse_args(
                [
                    "--expect-view",
                    "thread:!room:example|$thread-root:example::needle",
                ]
                .into_iter()
                .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(
                config.expect_views,
                vec![ViewExpectation {
                    identity: ViewIdentity::Thread {
                        room_id: "!room:example".to_owned(),
                        thread_root_event_id: "$thread-root:example".to_owned(),
                    },
                    substring: "needle".to_owned(),
                }]
            );
        }

        #[test]
        fn parse_args_accepts_generic_thread_negative_view_expectation() {
            let config = parse_args(
                [
                    "--expect-no-view",
                    "thread:!room:example|$thread-root:example::needle",
                ]
                .into_iter()
                .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(
                config.expect_no_views,
                vec![ViewExpectation {
                    identity: ViewIdentity::Thread {
                        room_id: "!room:example".to_owned(),
                        thread_root_event_id: "$thread-root:example".to_owned(),
                    },
                    substring: "needle".to_owned(),
                }]
            );
        }

        #[test]
        fn parse_view_expectation_rejects_malformed_thread_identity() {
            for value in [
                "thread:::needle",
                "thread:!room:example::needle",
                "thread:!room:example|::needle",
                "thread:|$thread-root:example::needle",
            ] {
                assert!(
                    parse_view_expectation("--expect-view", value).is_err(),
                    "expected {value:?} to be rejected"
                );
            }
        }

        #[test]
        fn parse_args_rejects_malformed_thread_identity_for_view_flags() {
            for args in [
                vec!["--select-view", "thread:!room:example"],
                vec!["--expect-view", "thread:!room:example::needle"],
                vec!["--expect-no-view", "thread:!room:example::needle"],
            ] {
                assert!(
                    parse_args(args.iter().copied().map(str::to_owned)).is_err(),
                    "expected {:?} to be rejected",
                    args
                );
            }
        }

        #[test]
        fn parse_args_accepts_direct_room_expectation() {
            let config = parse_args(
                ["--expect-direct-room", "!direct:example"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(
                config.expect_direct_rooms,
                vec!["!direct:example".to_owned()]
            );
        }

        #[test]
        fn parse_args_accepts_room_membership_expectation() {
            let config = parse_args(
                ["--expect-room-membership", "!invite:example::join"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(
                config.expect_room_memberships,
                vec![RoomMembershipExpectation {
                    room_id: "!invite:example".to_owned(),
                    membership: RoomMembership::Join,
                }]
            );
        }

        #[test]
        fn parse_args_rejects_malformed_room_membership_expectation() {
            let error = parse_args(
                ["--expect-room-membership", "!invite:example"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .err()
            .unwrap();

            assert_eq!(
                error,
                "invalid --expect-room-membership value \"!invite:example\"; expected <room_id>::<join|invite>"
            );
        }

        #[test]
        fn parse_args_rejects_unknown_room_membership_value() {
            let error = parse_args(
                ["--expect-room-membership", "!invite:example::leave"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .err()
            .unwrap();

            assert_eq!(
                error,
                "invalid --expect-room-membership value \"!invite:example::leave\"; expected <room_id>::<join|invite>"
            );
        }

        #[test]
        fn parse_args_accepts_room_tag_with_string_order() {
            let config = parse_args(
                ["--expect-room-tag", "!direct:example::m.favourite::0.25"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(
                config.expect_room_tags,
                vec![RoomTagExpectation {
                    room_id: "!direct:example".to_owned(),
                    tag: "m.favourite".to_owned(),
                    order: Some("0.25".to_owned()),
                }]
            );
        }

        #[test]
        fn parse_args_accepts_room_tag_with_null_order() {
            let config = parse_args(
                ["--expect-room-tag", "!direct:example::u.work::null"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(
                config.expect_room_tags,
                vec![RoomTagExpectation {
                    room_id: "!direct:example".to_owned(),
                    tag: "u.work".to_owned(),
                    order: None,
                }]
            );
        }

        #[test]
        fn parse_args_rejects_malformed_room_tag_expectation() {
            let error = parse_args(
                ["--expect-room-tag", "!direct:example::m.favourite"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .err()
            .unwrap();

            assert_eq!(
                error,
                "invalid --expect-room-tag value \"!direct:example::m.favourite\"; expected <room_id>::<tag>::<order-or-null>"
            );
        }

        #[test]
        fn parse_args_accepts_room_read_target_string() {
            let config = parse_args(
                ["--expect-room-read-target", "!alpha:example::$alpha-latest"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(
                config.expect_room_read_targets,
                vec![RoomReadTargetExpectation {
                    room_id: "!alpha:example".to_owned(),
                    read_target_event_id: Some("$alpha-latest".to_owned()),
                }]
            );
        }

        #[test]
        fn parse_args_accepts_room_read_target_null() {
            let config = parse_args(
                ["--expect-room-read-target", "!beta:example::null"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(
                config.expect_room_read_targets,
                vec![RoomReadTargetExpectation {
                    room_id: "!beta:example".to_owned(),
                    read_target_event_id: None,
                }]
            );
        }

        #[test]
        fn parse_args_rejects_malformed_room_read_target_expectation() {
            let error = parse_args(
                ["--expect-room-read-target", "!beta:example"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .err()
            .unwrap();

            assert_eq!(
                error,
                "invalid --expect-room-read-target value \"!beta:example\"; expected <room_id>::<event-id-or-null>"
            );
        }

        #[test]
        fn parse_args_accepts_room_unread_expectation() {
            let config = parse_args(
                ["--expect-room-unread", "!alpha:example::1::2::1::true"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(
                config.expect_room_unreads,
                vec![RoomUnreadExpectation {
                    room_id: "!alpha:example".to_owned(),
                    num_unread: 1,
                    num_notifications: 2,
                    num_mentions: 1,
                    marked_unread: true,
                }]
            );
        }

        #[test]
        fn parse_args_accepts_room_unread_expectation_with_marked_unread_false() {
            let config = parse_args(
                ["--expect-room-unread", "!beta:example::4::0::0::false"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();

            assert_eq!(
                config.expect_room_unreads,
                vec![RoomUnreadExpectation {
                    room_id: "!beta:example".to_owned(),
                    num_unread: 4,
                    num_notifications: 0,
                    num_mentions: 0,
                    marked_unread: false,
                }]
            );
        }

        #[test]
        fn parse_args_rejects_malformed_room_unread_counter() {
            let error = parse_args(
                ["--expect-room-unread", "!alpha:example::x::2::1::true"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .err()
            .unwrap();

            assert_eq!(
                error,
                "invalid --expect-room-unread num_unread \"!alpha:example::x::2::1::true\"; expected u64"
            );
        }

        #[test]
        fn parse_args_rejects_invalid_room_unread_boolean() {
            let error = parse_args(
                ["--expect-room-unread", "!alpha:example::1::2::1::yes"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .err()
            .unwrap();

            assert_eq!(
                error,
                "invalid --expect-room-unread marked_unread \"!alpha:example::1::2::1::yes\"; expected true or false"
            );
        }
    }
}

#[cfg(any(windows, test))]
fn assert_room_membership(
    rooms: &[mirx_testkit::Room],
    room_id: &str,
    expected_membership: mirx_core::RoomMembership,
) -> Result<(), String> {
    let room = rooms
        .iter()
        .find(|room| room.room_id == room_id)
        .ok_or_else(|| format!("room membership assertion failed: missing room {room_id}"))?;
    if room.membership != expected_membership {
        return Err(format!(
            "room membership assertion failed: room {room_id} expected {}, found {}",
            format_room_membership(&expected_membership),
            format_room_membership(&room.membership)
        ));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn assert_no_room(rooms: &[mirx_testkit::Room], room_id: &str) -> Result<(), String> {
    if rooms.iter().any(|room| room.room_id == room_id) {
        return Err(format!(
            "room absence assertion failed: unexpected room {room_id}"
        ));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn format_room_membership(membership: &mirx_core::RoomMembership) -> &'static str {
    match membership {
        mirx_core::RoomMembership::Join => "join",
        mirx_core::RoomMembership::Invite => "invite",
    }
}

#[cfg(test)]
mod room_assertion_tests {
    use super::{assert_no_room, assert_room_membership};
    use mirx_core::RoomMembership;
    use mirx_testkit::Room;

    fn room(room_id: &str, membership: RoomMembership) -> Room {
        Room {
            room_id: room_id.to_owned(),
            display_name: room_id.to_owned(),
            topic: None,
            membership,
            encrypted: false,
            is_space: false,
            is_direct: false,
            direct_user_ids: Vec::new(),
            tags: Vec::new(),
            space_children: Vec::new(),
        }
    }

    #[test]
    fn room_membership_assertion_accepts_matching_membership() {
        let rooms = vec![room("!invite:example", RoomMembership::Invite)];

        assert!(assert_room_membership(&rooms, "!invite:example", RoomMembership::Invite).is_ok());
    }

    #[test]
    fn room_membership_assertion_rejects_missing_room() {
        let rooms = vec![room("!invite:example", RoomMembership::Invite)];

        assert_eq!(
            assert_room_membership(&rooms, "!missing:example", RoomMembership::Invite).unwrap_err(),
            "room membership assertion failed: missing room !missing:example"
        );
    }

    #[test]
    fn room_membership_assertion_rejects_wrong_membership() {
        let rooms = vec![room("!invite:example", RoomMembership::Invite)];

        assert_eq!(
            assert_room_membership(&rooms, "!invite:example", RoomMembership::Join).unwrap_err(),
            "room membership assertion failed: room !invite:example expected join, found invite"
        );
    }

    #[test]
    fn room_absence_assertion_accepts_missing_room() {
        let rooms = vec![room("!invite:example", RoomMembership::Invite)];

        assert!(assert_no_room(&rooms, "!missing:example").is_ok());
    }

    #[test]
    fn room_absence_assertion_rejects_present_room() {
        let rooms = vec![room("!invite:example", RoomMembership::Invite)];

        assert_eq!(
            assert_no_room(&rooms, "!invite:example").unwrap_err(),
            "room absence assertion failed: unexpected room !invite:example"
        );
    }
}

#[cfg(windows)]
mod windows_main {
    use std::collections::BTreeMap;
    use std::env;
    use std::fs;
    use std::process::ExitCode;

    use mirx_testkit::{
        AutomationDialog, ComposeKey, MdiView, MirxApp, ModalDialog, NavigationNode, RoomActivity,
        UiFindHistorySearchState, UiState, Verification, VerificationState, ViewIdentity,
    };

    use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows_sys::Win32::System::SystemServices::MK_LBUTTON;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CB_SETCURSEL, CBN_SELCHANGE, GetDlgItem, IDOK, IsWindow, PostMessageW, SMTO_NORMAL,
        SendMessageTimeoutW, WM_COMMAND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    };

    use crate::cli::{
        Action, Config, FrameBar, TextPoint, ViewExpectation, format_view_identity, parse_args,
    };

    impl Config {
        fn requires_ui_state(&self) -> bool {
            self.expect_active_view.is_some()
                || !self.expect_views.is_empty()
                || !self.expect_no_views.is_empty()
                || !self.expect_room_views.is_empty()
                || !self.expect_room_members.is_empty()
                || !self.expect_status_views.is_empty()
                || !self.expect_no_status_views.is_empty()
                || !self.expect_status_bars.is_empty()
                || !self.expect_no_status_bars.is_empty()
                || !self.expect_switchbar_items.is_empty()
                || !self.expect_no_switchbar_items.is_empty()
                || self.expect_switchbar_active.is_some()
                || self.expect_switchbar_rows.is_some()
                || !self.expect_frame_bars_hidden.is_empty()
                || !self.expect_switchbar_activity.is_empty()
                || !self.expect_no_switchbar_activity.is_empty()
                || !self.expect_navigation_texts.is_empty()
                || !self.expect_no_navigation_texts.is_empty()
                || !self.expect_navigation_activity.is_empty()
                || !self.expect_no_navigation_activity.is_empty()
                || !self.expect_navigation_nodes.is_empty()
                || !self.expect_direct_rooms.is_empty()
                || !self.expect_room_tags.is_empty()
                || !self.expect_room_read_targets.is_empty()
                || !self.expect_room_unreads.is_empty()
                || !self.expect_mdi_placements.is_empty()
                || !self.expect_mdi_titles.is_empty()
                || self.expect_active_find_history.is_some()
                || self.expect_active_find_selected_text.is_some()
        }
    }

    fn expected_history_backfill_room_id(config: &Config) -> Option<&str> {
        config
            .expect_selected
            .as_deref()
            .or(match config.expect_active_view.as_ref() {
                Some(ViewIdentity::Room { room_id }) => Some(room_id.as_str()),
                _ => None,
            })
    }

    pub(crate) fn run() -> Result<(), String> {
        let config = parse_args(env::args().skip(1))?;
        let app = MirxApp::find(config.find_timeout)
            .map_err(|error| format!("failed to attach to mirx-ui automation window: {error}"))?;
        app.wait_idle(config.wait_idle)
            .map_err(|error| format!("WAIT_IDLE failed: {error}"))?;

        for view in &config.select_views {
            app.select_view(view.clone())
                .map_err(|error| format!("SELECT_VIEW failed: {error}"))?;
            app.wait_idle(config.wait_idle)
                .map_err(|error| format!("WAIT_IDLE after SELECT_VIEW failed: {error}"))?;
        }

        if config.trigger_history_backfill {
            app.trigger_active_timeline_history_backfill()
                .map_err(|error| format!("TRIGGER_HISTORY_BACKFILL failed: {error}"))?;
            app.wait_idle(config.wait_idle).map_err(|error| {
                format!("WAIT_IDLE after TRIGGER_HISTORY_BACKFILL failed: {error}")
            })?;
            if let Some(expected_room_id) = expected_history_backfill_room_id(&config) {
                let model = app.dump_model().map_err(|error| {
                    format!("DUMP_MODEL after TRIGGER_HISTORY_BACKFILL failed: {error}")
                })?;
                let submitted = model
                    .submitted_history_commands
                    .iter()
                    .any(|command| command.room_id == expected_room_id);
                if !submitted {
                    return Err(format!(
                        "history backfill assertion failed: no submitted history command recorded for {expected_room_id}"
                    ));
                }
                println!("PASS submitted history command {expected_room_id}");
            }
        }

        if let Some(path) = config.append_events.as_deref() {
            let json_text = fs::read_to_string(path)
                .map_err(|error| format!("failed to read append-events file {path:?}: {error}"))?;
            app.append_synthetic_events(&json_text)
                .map_err(|error| format!("APPEND_SYNTHETIC_EVENTS failed: {error}"))?;
            app.wait_idle(config.wait_idle).map_err(|error| {
                format!("WAIT_IDLE after APPEND_SYNTHETIC_EVENTS failed: {error}")
            })?;
        }

        if let Some(query) = config.find_active_view.as_deref() {
            app.find_active_view(query)
                .map_err(|error| format!("FIND_ACTIVE_VIEW failed: {error}"))?;
            app.wait_idle(config.wait_idle)
                .map_err(|error| format!("WAIT_IDLE after FIND_ACTIVE_VIEW failed: {error}"))?;
        }

        let mut opened_dialog = None;
        for action in &config.actions {
            match action {
                Action::Find(query) => {
                    app.find_active_view(query)
                        .map_err(|error| format!("FIND_ACTIVE_VIEW failed: {error}"))?;
                    app.wait_idle(config.wait_idle).map_err(|error| {
                        format!("WAIT_IDLE after FIND_ACTIVE_VIEW failed: {error}")
                    })?;
                }
                Action::Input(text) => {
                    app.set_compose_text(text)
                        .map_err(|error| format!("SET_COMPOSE_TEXT failed: {error}"))?;
                    app.send_compose_key(ComposeKey::Return)
                        .map_err(|error| format!("SEND_COMPOSE_KEY failed: {error}"))?;
                    app.wait_idle(config.wait_idle).map_err(|error| {
                        format!("WAIT_IDLE after SEND_COMPOSE_KEY failed: {error}")
                    })?;
                }
                Action::Append(path) => {
                    let json_text = fs::read_to_string(path).map_err(|error| {
                        format!("failed to read action append file {path:?}: {error}")
                    })?;
                    app.append_synthetic_events(&json_text)
                        .map_err(|error| format!("APPEND_SYNTHETIC_EVENTS failed: {error}"))?;
                    app.wait_idle(config.wait_idle).map_err(|error| {
                        format!("WAIT_IDLE after APPEND_SYNTHETIC_EVENTS failed: {error}")
                    })?;
                }
                Action::Arrange(arrangement) => {
                    app.arrange_windows(*arrangement).map_err(|error| {
                        format!("ARRANGE_WINDOWS_VIA_WINDOW_MENU failed: {error}")
                    })?;
                    app.wait_idle(config.wait_idle).map_err(|error| {
                        format!("WAIT_IDLE after ARRANGE_WINDOWS_VIA_WINDOW_MENU failed: {error}")
                    })?;
                }
                Action::WindowState(identity, state) => {
                    app.set_view_window_state(identity.clone(), *state)
                        .map_err(|error| format!("SET_VIEW_WINDOW_STATE failed: {error}"))?;
                    app.wait_idle(config.wait_idle).map_err(|error| {
                        format!("WAIT_IDLE after SET_VIEW_WINDOW_STATE failed: {error}")
                    })?;
                }
                Action::Collapse(node) => {
                    app.set_navigation_expanded(node, false)
                        .map_err(|error| format!("SET_NAVIGATION_EXPANDED failed: {error}"))?;
                    app.wait_idle(config.wait_idle).map_err(|error| {
                        format!("WAIT_IDLE after SET_NAVIGATION_EXPANDED failed: {error}")
                    })?;
                }
                Action::Dialog {
                    dialog,
                    options_page,
                } => {
                    opened_dialog = Some(open_dialog(&app, &config, *dialog, options_page)?);
                }
                Action::Drag { from, to } => {
                    drag_text(&app, *from, *to)?;
                }
                Action::Colors(pairs) => {
                    set_colors(&app, &config, pairs)?;
                }
                Action::ComposeText(text) => {
                    app.set_compose_text(text)
                        .map_err(|error| format!("SET_COMPOSE_TEXT failed: {error}"))?;
                }
                Action::ComposeKey(key) => {
                    app.send_compose_key(ComposeKey::Format { key: *key })
                        .map_err(|error| format!("SEND_COMPOSE_KEY failed: {error}"))?;
                    app.wait_idle(config.wait_idle).map_err(|error| {
                        format!("WAIT_IDLE after SEND_COMPOSE_KEY failed: {error}")
                    })?;
                }
            }
        }

        if let Some(expected_title) = config.expect_dialog_title.as_deref() {
            let opened = opened_dialog
                .as_ref()
                .expect("parse_args requires a dialog action with --expect-dialog-title");
            if opened.title != expected_title {
                return Err(format!(
                    "dialog title assertion failed: expected {expected_title:?}, found {:?}",
                    opened.title
                ));
            }
            println!("PASS dialog {expected_title:?}");
        }

        let model = app
            .dump_model()
            .map_err(|error| format!("DUMP_MODEL failed: {error}"))?;

        if let Some(expected_room_id) = config.expect_selected.as_deref() {
            let actual_room_id = model.selected_room.as_deref().ok_or_else(|| {
                format!("selected room assertion failed: expected {expected_room_id}, found none")
            })?;
            if actual_room_id != expected_room_id {
                return Err(format!(
                    "selected room assertion failed: expected {expected_room_id}, found {actual_room_id}"
                ));
            }
            println!("PASS selected room {expected_room_id}");
        }

        for expected_room_id in &config.expect_rooms {
            let exists = model
                .rooms
                .iter()
                .any(|room| room.room_id == *expected_room_id);
            if !exists {
                return Err(format!(
                    "room assertion failed: missing room {expected_room_id}"
                ));
            }
            println!("PASS room {expected_room_id}");
        }

        for expectation in &config.expect_room_memberships {
            crate::assert_room_membership(
                &model.rooms,
                &expectation.room_id,
                expectation.membership,
            )?;
            println!(
                "PASS room membership {} {}",
                expectation.room_id,
                crate::format_room_membership(&expectation.membership)
            );
        }

        for room_id in &config.expect_no_rooms {
            crate::assert_no_room(&model.rooms, room_id)?;
            println!("PASS no room {room_id}");
        }

        if let Some(expected_flow_id) = config.expect_verification.as_deref() {
            let verification = model.last_verification.as_ref().ok_or_else(|| {
                format!(
                    "verification assertion failed: expected flow {expected_flow_id}, found none"
                )
            })?;
            if verification.flow_id != expected_flow_id {
                return Err(format!(
                    "verification assertion failed: expected flow {expected_flow_id}, found {}",
                    verification.flow_id
                ));
            }
            println!("PASS verification {expected_flow_id}");
        }

        if let Some(expected_state) = config.expect_verification_state.as_deref() {
            let verification = model.last_verification.as_ref().ok_or_else(|| {
                format!(
                    "verification state assertion failed: expected state {expected_state}, found none"
                )
            })?;
            let actual_state = verification_state_name(&verification.state);
            if actual_state != expected_state {
                return Err(format!(
                    "verification state assertion failed: expected {expected_state}, found {actual_state}"
                ));
            }
            println!("PASS verification state {expected_state}");
        }

        if !config.expect_verification_texts.is_empty() {
            let verification = model.last_verification.as_ref().ok_or_else(|| {
                "verification text assertion failed: expected verification, found none".to_owned()
            })?;
            let rendered = render_verification_text(verification);
            for substring in &config.expect_verification_texts {
                if !rendered.contains(substring) {
                    return Err(format!(
                        "verification text assertion failed: expected substring {:?} in {:?}",
                        substring, rendered
                    ));
                }
                println!("PASS verification text contains {:?}", substring);
            }
        }

        if let Some(expected_substring) = config.expect_error.as_deref() {
            let error = model.last_error.as_ref().ok_or_else(|| {
                format!(
                    "error assertion failed: expected substring {:?}, found no error",
                    expected_substring
                )
            })?;
            let rendered_error = format!("{}: {}", error.operation, error.message);
            if !rendered_error.contains(expected_substring) {
                return Err(format!(
                    "error assertion failed: expected substring {:?} in {:?}",
                    expected_substring, rendered_error
                ));
            }
            println!("PASS error contains {:?}", expected_substring);
        }

        let mut room_activity_cache = BTreeMap::<String, Vec<RoomActivity>>::new();
        for expectation in &config.expect_room_activities {
            let entries = if let Some(entries) = room_activity_cache.get(&expectation.room_id) {
                entries
            } else {
                let entries = app
                    .room_activity(Some(&expectation.room_id))
                    .map_err(|error| {
                        format!(
                            "DUMP_ROOM_ACTIVITY failed for room {}: {error}",
                            expectation.room_id
                        )
                    })?;
                room_activity_cache.insert(expectation.room_id.clone(), entries);
                room_activity_cache
                    .get(&expectation.room_id)
                    .expect("room activity cache entry must exist after insert")
            };

            let found = entries
                .iter()
                .any(|entry| room_activity_contains(entry, &expectation.substring));
            if !found {
                return Err(format!(
                    "room activity assertion failed: room {} did not contain {:?}",
                    expectation.room_id, expectation.substring
                ));
            }
            println!(
                "PASS room activity {} contains {:?}",
                expectation.room_id, expectation.substring
            );
        }

        if config.requires_ui_state() {
            let ui_state = app
                .dump_ui_state()
                .map_err(|error| format!("DUMP_UI_STATE failed: {error}"))?;

            if let Some(expected_history) = config.expect_active_find_history.as_deref() {
                let active_find = ui_state.active_find.as_ref().ok_or_else(|| {
                    "active Find history assertion failed: no active Find state".to_owned()
                })?;
                let actual_history = active_find_history_name(&active_find.history_search);
                if actual_history != expected_history {
                    return Err(format!(
                        "active Find history assertion failed: expected {expected_history}, found {actual_history}"
                    ));
                }
                println!("PASS active Find history {expected_history}");
            }

            if let Some(expected_text) = config.expect_active_find_selected_text.as_deref() {
                let active_find = ui_state.active_find.as_ref().ok_or_else(|| {
                    "active Find selected text assertion failed: no active Find state".to_owned()
                })?;
                let selected_text = active_find.selected_text.as_deref().ok_or_else(|| {
                    "active Find selected text assertion failed: no selected text".to_owned()
                })?;
                if !selected_text.contains(expected_text) {
                    return Err(format!(
                        "active Find selected text assertion failed: expected substring {expected_text:?} in {selected_text:?}"
                    ));
                }
                println!("PASS active Find selected text contains {expected_text:?}");
            }

            if let Some(expected_view) = config.expect_active_view.as_ref() {
                if &ui_state.mdi.active != expected_view {
                    return Err(format!(
                        "active view assertion failed: expected {}, found {}",
                        format_view_identity(expected_view),
                        format_view_identity(&ui_state.mdi.active)
                    ));
                }
                println!("PASS active view {}", format_view_identity(expected_view));
            }

            for expectation in &config.expect_views {
                assert_view_contains(&ui_state, expectation, "view assertion failed")?;
                println!(
                    "PASS view {} contains {:?}",
                    format_view_identity(&expectation.identity),
                    expectation.substring
                );
            }

            for expectation in &config.expect_no_views {
                let view = find_view(&ui_state, &expectation.identity).ok_or_else(|| {
                    format!(
                        "view assertion failed: missing view {}",
                        format_view_identity(&expectation.identity)
                    )
                })?;
                if view_contains(view, &expectation.substring) {
                    return Err(format!(
                        "view assertion failed: {} unexpectedly contained {:?}",
                        format_view_identity(&expectation.identity),
                        expectation.substring
                    ));
                }
                println!(
                    "PASS view {} excludes {:?}",
                    format_view_identity(&expectation.identity),
                    expectation.substring
                );
            }

            for expectation in &config.expect_room_views {
                let identity = ViewIdentity::Room {
                    room_id: expectation.room_id.clone(),
                };
                let view = find_view(&ui_state, &identity).ok_or_else(|| {
                    format!(
                        "room view assertion failed: missing room view {}",
                        expectation.room_id
                    )
                })?;
                if !view_contains(view, &expectation.substring) {
                    return Err(format!(
                        "room view assertion failed: room {} did not contain {:?}",
                        expectation.room_id, expectation.substring
                    ));
                }
                println!(
                    "PASS room view {} contains {:?}",
                    expectation.room_id, expectation.substring
                );
            }

            for expectation in &config.expect_room_members {
                let identity = ViewIdentity::Room {
                    room_id: expectation.room_id.clone(),
                };
                let view = find_view(&ui_state, &identity).ok_or_else(|| {
                    format!(
                        "room member assertion failed: missing room view {}",
                        expectation.room_id
                    )
                })?;
                if !view_members_contain(view, &expectation.substring) {
                    return Err(format!(
                        "room member assertion failed: room {} did not contain {:?}",
                        expectation.room_id, expectation.substring
                    ));
                }
                println!(
                    "PASS room member {} contains {:?}",
                    expectation.room_id, expectation.substring
                );
            }

            if !config.expect_status_views.is_empty() || !config.expect_no_status_views.is_empty() {
                let status_view = find_view(&ui_state, &ViewIdentity::Status).ok_or_else(|| {
                    "status view assertion failed: missing status view".to_owned()
                })?;
                for substring in &config.expect_status_views {
                    if !view_contains(status_view, substring) {
                        return Err(format!(
                            "status view assertion failed: status did not contain {:?}",
                            substring
                        ));
                    }
                    println!("PASS status view contains {:?}", substring);
                }
                for substring in &config.expect_no_status_views {
                    if view_contains(status_view, substring) {
                        return Err(format!(
                            "status view assertion failed: status unexpectedly contained {:?}",
                            substring
                        ));
                    }
                    println!("PASS status view excludes {:?}", substring);
                }
            }

            for substring in &config.expect_status_bars {
                if !ui_state.status_bar.text.contains(substring) {
                    return Err(format!(
                        "status bar assertion failed: expected substring {:?} in {:?}",
                        substring, ui_state.status_bar.text
                    ));
                }
                println!("PASS status bar contains {:?}", substring);
            }

            for substring in &config.expect_no_status_bars {
                if ui_state.status_bar.text.contains(substring) {
                    return Err(format!(
                        "status bar assertion failed: unexpected substring {:?} in {:?}",
                        substring, ui_state.status_bar.text
                    ));
                }
                println!("PASS status bar excludes {:?}", substring);
            }

            for expectation in &config.expect_switchbar_items {
                let item = ui_state
                    .switchbar
                    .item(&expectation.identity)
                    .ok_or_else(|| {
                        format!(
                            "switchbar item assertion failed: missing item {}",
                            format_view_identity(&expectation.identity)
                        )
                    })?;
                if !item.title.contains(&expectation.title_substring) {
                    return Err(format!(
                        "switchbar item assertion failed: expected {} title to contain {:?}, found {:?}",
                        format_view_identity(&expectation.identity),
                        expectation.title_substring,
                        item.title
                    ));
                }
                println!(
                    "PASS switchbar item {} contains {:?}",
                    format_view_identity(&expectation.identity),
                    expectation.title_substring
                );
            }

            for identity in &config.expect_no_switchbar_items {
                if ui_state.switchbar.item(identity).is_some() {
                    return Err(format!(
                        "switchbar item assertion failed: unexpected item {}",
                        format_view_identity(identity)
                    ));
                }
                println!("PASS switchbar excludes {}", format_view_identity(identity));
            }

            for expected in &config.expect_mdi_titles {
                let view = ui_state
                    .mdi
                    .views
                    .iter()
                    .find(|view| view.identity == expected.identity)
                    .ok_or_else(|| {
                        format!(
                            "MDI title assertion failed: missing view {}",
                            format_view_identity(&expected.identity)
                        )
                    })?;
                if view.title != expected.title {
                    return Err(format!(
                        "MDI title assertion failed: {} is {:?}, expected {:?}",
                        format_view_identity(&expected.identity),
                        view.title,
                        expected.title
                    ));
                }
                println!(
                    "PASS MDI {} title {:?}",
                    format_view_identity(&expected.identity),
                    expected.title
                );
            }

            for expected in &config.expect_mdi_placements {
                let view = ui_state
                    .mdi
                    .views
                    .iter()
                    .find(|view| view.identity == expected.identity)
                    .ok_or_else(|| {
                        format!(
                            "MDI placement assertion failed: missing view {}",
                            format_view_identity(&expected.identity)
                        )
                    })?;
                if view.placement.state != expected.state {
                    return Err(format!(
                        "MDI placement assertion failed: {} is {:?}, expected {:?}",
                        format_view_identity(&expected.identity),
                        view.placement.state,
                        expected.state
                    ));
                }
                println!(
                    "PASS MDI {} is {:?}",
                    format_view_identity(&expected.identity),
                    expected.state
                );
            }

            if let Some(expected_rows) = config.expect_switchbar_rows {
                if ui_state.switchbar.rows != expected_rows {
                    return Err(format!(
                        "switchbar rows assertion failed: {} row(s), expected {expected_rows}",
                        ui_state.switchbar.rows
                    ));
                }
                println!("PASS switchbar rows {expected_rows}");
            }

            for bar in &config.expect_frame_bars_hidden {
                let layout = &ui_state.frame_layout;
                let rect = match bar {
                    FrameBar::Switchbar => layout.switchbar,
                    FrameBar::Treebar => layout.treebar,
                };
                if let Some(rect) = rect {
                    return Err(format!(
                        "frame bar assertion failed: the {} is shown at {rect:?}",
                        bar.name()
                    ));
                }
                println!("PASS {} hidden", bar.name());
            }

            if let Some(expected_identity) = config.expect_switchbar_active.as_ref() {
                let active_items: Vec<_> = ui_state
                    .switchbar
                    .items
                    .iter()
                    .filter(|item| item.active)
                    .collect();
                if active_items.len() != 1 {
                    let actual = active_items
                        .iter()
                        .map(|item| format_view_identity(&item.identity))
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(format!(
                        "switchbar active assertion failed: expected exactly one active item {}, found {} active item(s): [{}]",
                        format_view_identity(expected_identity),
                        active_items.len(),
                        actual
                    ));
                }
                let actual_identity = &active_items[0].identity;
                if actual_identity != expected_identity {
                    return Err(format!(
                        "switchbar active assertion failed: expected {}, found {}",
                        format_view_identity(expected_identity),
                        format_view_identity(actual_identity)
                    ));
                }
                println!(
                    "PASS switchbar active {}",
                    format_view_identity(expected_identity)
                );
            }

            for expectation in &config.expect_switchbar_activity {
                let identity = &expectation.identity;
                let item = ui_state.switchbar.item(identity).ok_or_else(|| {
                    format!(
                        "switchbar activity assertion failed: missing item {}",
                        format_view_identity(identity)
                    )
                })?;
                if item.activity != Some(expectation.level) {
                    return Err(format!(
                        "switchbar activity assertion failed: expected {} to have {:?} activity, found {:?}",
                        format_view_identity(identity),
                        expectation.level,
                        item.activity
                    ));
                }
                println!(
                    "PASS switchbar activity {} {:?}",
                    format_view_identity(identity),
                    expectation.level
                );
            }

            for identity in &config.expect_no_switchbar_activity {
                let item = ui_state.switchbar.item(identity).ok_or_else(|| {
                    format!(
                        "switchbar no-activity assertion failed: missing item {}",
                        format_view_identity(identity)
                    )
                })?;
                if let Some(level) = item.activity {
                    return Err(format!(
                        "switchbar no-activity assertion failed: expected {} to have no activity, found {level:?}",
                        format_view_identity(identity)
                    ));
                }
                println!(
                    "PASS switchbar no activity {}",
                    format_view_identity(identity)
                );
            }

            for substring in &config.expect_navigation_texts {
                if !navigation_contains(&ui_state.navigation, substring) {
                    return Err(format!(
                        "navigation assertion failed: navigation did not contain {:?}",
                        substring
                    ));
                }
                println!("PASS navigation contains {:?}", substring);
            }

            for substring in &config.expect_no_navigation_texts {
                if navigation_contains(&ui_state.navigation, substring) {
                    return Err(format!(
                        "navigation assertion failed: navigation unexpectedly contained {:?}",
                        substring
                    ));
                }
                println!("PASS navigation excludes {:?}", substring);
            }

            for expectation in &config.expect_navigation_activity {
                let identity = format_view_identity(&expectation.identity);
                let nodes = ui_state.navigation.find_view(&expectation.identity);
                if nodes.is_empty() {
                    return Err(format!(
                        "navigation activity assertion failed: no node for {identity}"
                    ));
                }
                for node in nodes {
                    if node.activity != Some(expectation.level) {
                        return Err(format!(
                            "navigation activity assertion failed: expected {identity} ({:?}) to have {:?} activity, found {:?}",
                            node.text, expectation.level, node.activity
                        ));
                    }
                }
                println!(
                    "PASS navigation activity {identity} {:?}",
                    expectation.level
                );
            }

            for view in &config.expect_no_navigation_activity {
                let identity = format_view_identity(view);
                let nodes = ui_state.navigation.find_view(view);
                if nodes.is_empty() {
                    return Err(format!(
                        "navigation no-activity assertion failed: no node for {identity}"
                    ));
                }
                for node in nodes {
                    if let Some(level) = node.activity {
                        return Err(format!(
                            "navigation no-activity assertion failed: expected {identity} ({:?}) to have no activity, found {level:?}",
                            node.text
                        ));
                    }
                }
                println!("PASS navigation no activity {identity}");
            }

            for expectation in &config.expect_navigation_nodes {
                let node = ui_state.navigation.find(&expectation.node).ok_or_else(|| {
                    format!(
                        "navigation node assertion failed: no node for {:?}",
                        expectation.node
                    )
                })?;
                if node.expanded != Some(expectation.expanded)
                    || node.activity != expectation.activity
                {
                    return Err(format!(
                        "navigation node assertion failed: expected {:?} ({:?}) expanded {} with {:?} activity, found expanded {:?} with {:?}",
                        expectation.node,
                        node.text,
                        expectation.expanded,
                        expectation.activity,
                        node.expanded,
                        node.activity
                    ));
                }
                println!(
                    "PASS navigation node {:?} expanded {} activity {:?}",
                    expectation.node, expectation.expanded, expectation.activity
                );
            }

            for expected_room_id in &config.expect_direct_rooms {
                let room = find_room_state(&ui_state, expected_room_id).ok_or_else(|| {
                    format!("direct room assertion failed: missing room {expected_room_id}")
                })?;
                if !room.is_direct {
                    return Err(format!(
                        "direct room assertion failed: room {expected_room_id} is not direct"
                    ));
                }
                println!("PASS direct room {expected_room_id}");
            }

            for expectation in &config.expect_room_tags {
                let room = find_room_state(&ui_state, &expectation.room_id).ok_or_else(|| {
                    format!(
                        "room tag assertion failed: missing room {}",
                        expectation.room_id
                    )
                })?;
                let found = room
                    .tags
                    .iter()
                    .any(|tag| tag.tag == expectation.tag && tag.order == expectation.order);
                if !found {
                    return Err(format!(
                        "room tag assertion failed: room {} missing tag {} with order {:?}",
                        expectation.room_id, expectation.tag, expectation.order
                    ));
                }
                let order = expectation.order.as_deref().unwrap_or("null");
                println!(
                    "PASS room tag {} {} {}",
                    expectation.room_id, expectation.tag, order
                );
            }

            for expectation in &config.expect_room_read_targets {
                let room = find_room_state(&ui_state, &expectation.room_id).ok_or_else(|| {
                    format!(
                        "room read target assertion failed: missing room {}",
                        expectation.room_id
                    )
                })?;
                if room.read_target_event_id != expectation.read_target_event_id {
                    return Err(format!(
                        "room read target assertion failed: room {} expected {:?}, found {:?}",
                        expectation.room_id,
                        expectation.read_target_event_id,
                        room.read_target_event_id
                    ));
                }
                let read_target = expectation
                    .read_target_event_id
                    .as_deref()
                    .unwrap_or("null");
                println!(
                    "PASS room read target {} {}",
                    expectation.room_id, read_target
                );
            }

            for expectation in &config.expect_room_unreads {
                let room = find_room_state(&ui_state, &expectation.room_id).ok_or_else(|| {
                    format!(
                        "room unread assertion failed: missing room {}",
                        expectation.room_id
                    )
                })?;
                let read_state = room.read_state.as_ref().ok_or_else(|| {
                    format!(
                        "room unread assertion failed: room {} has no read state",
                        expectation.room_id
                    )
                })?;
                if read_state.num_unread != expectation.num_unread
                    || read_state.num_notifications != expectation.num_notifications
                    || read_state.num_mentions != expectation.num_mentions
                    || read_state.marked_unread != expectation.marked_unread
                {
                    return Err(format!(
                        "room unread assertion failed: room {} expected unread={}, notifications={}, mentions={}, marked_unread={}, found unread={}, notifications={}, mentions={}, marked_unread={}",
                        expectation.room_id,
                        expectation.num_unread,
                        expectation.num_notifications,
                        expectation.num_mentions,
                        expectation.marked_unread,
                        read_state.num_unread,
                        read_state.num_notifications,
                        read_state.num_mentions,
                        read_state.marked_unread
                    ));
                }
                println!(
                    "PASS room unread {} {} {} {} {}",
                    expectation.room_id,
                    expectation.num_unread,
                    expectation.num_notifications,
                    expectation.num_mentions,
                    expectation.marked_unread
                );
            }
        }

        Ok(())
    }

    /// Opens `dialog`, shows its Options page, and has it paint before the
    /// screen is captured.
    /// Presses at `from` and moves to `to` on the active timeline, through
    /// the mouse messages the timeline handles, and leaves the button down.
    fn drag_text(app: &MirxApp, from: TextPoint, to: TextPoint) -> Result<(), String> {
        let ui_state = app
            .dump_ui_state()
            .map_err(|error| format!("DUMP_UI_STATE before drag failed: {error}"))?;
        let font = &ui_state.text_font;
        if font.face != "Fixedsys" {
            return Err(format!(
                "drag: points are computed for the fixed-pitch Fixedsys, not {:?}",
                font.face
            ));
        }
        let timeline = ui_state
            .active_timeline
            .ok_or("drag: needs an active timeline")?;
        let client_point = |point: TextPoint| -> Result<(i32, i32), String> {
            let line = timeline.visible_lines.get(point.line).ok_or_else(|| {
                format!(
                    "drag: line {} is not among the {} visible lines",
                    point.line,
                    timeline.visible_lines.len()
                )
            })?;
            let before = line.text.get(..point.offset).ok_or_else(|| {
                format!(
                    "drag: offset {} is not a boundary of line {} {:?}",
                    point.offset, point.line, line.text
                )
            })?;
            if !before.is_ascii() {
                return Err(format!(
                    "drag: the text before offset {} of line {} {:?} must be ASCII",
                    point.offset, point.line, line.text
                ));
            }
            let width = font.average_char_width;
            // A quarter character in: on the boundary's side of the
            // character's middle, whatever rounding the hit test does.
            Ok((
                line.text_left + point.offset as i32 * width + width / 4,
                line.click_point.y,
            ))
        };
        let from = client_point(from)?;
        let to = client_point(to)?;
        let hwnd = app
            .active_timeline_hwnd()
            .map_err(|error| format!("ACTIVE_TIMELINE_HWND failed: {error}"))?
            as HWND;
        send_mouse(hwnd, WM_LBUTTONDOWN, MK_LBUTTON, from)?;
        send_mouse(hwnd, WM_MOUSEMOVE, MK_LBUTTON, to)
    }

    /// Sends a mouse message to `hwnd` and waits for it to be handled.
    fn send_mouse(
        hwnd: HWND,
        message: u32,
        buttons: u32,
        (x, y): (i32, i32),
    ) -> Result<(), String> {
        let lparam = ((y as u16 as isize) << 16) | x as u16 as isize;
        let mut result = 0;
        let sent = unsafe {
            SendMessageTimeoutW(
                hwnd,
                message,
                buttons as WPARAM,
                lparam,
                SMTO_NORMAL,
                5_000,
                &mut result,
            )
        };
        if sent == 0 {
            return Err(format!(
                "drag: mouse message {message:#x} was not handled: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    /// The Colors dialog's item drop-down and palette.
    const COLORS_ITEM_ID: i32 = 6502;
    const COLORS_PALETTE_ID: i32 = 6503;

    /// Sends `message` to `hwnd` and waits for it to be handled.
    fn send(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Result<(), String> {
        let mut result = 0;
        let sent = unsafe {
            SendMessageTimeoutW(
                hwnd,
                message,
                wparam,
                lparam,
                SMTO_NORMAL,
                5_000,
                &mut result,
            )
        };
        if sent == 0 {
            return Err(format!(
                "message {message:#x} was not handled: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    /// Opens Colors, gives each item its index through the drop-down and
    /// a click on the palette, and presses OK.
    fn set_colors(app: &MirxApp, config: &Config, pairs: &[(String, u8)]) -> Result<(), String> {
        let opened = open_dialog(app, config, AutomationDialog::Colors, &None)?;
        let dialog = opened.hwnd as HWND;
        let (combo, palette) = unsafe {
            (
                GetDlgItem(dialog, COLORS_ITEM_ID),
                GetDlgItem(dialog, COLORS_PALETTE_ID),
            )
        };
        let colors_dialog = || -> Result<_, String> {
            app.dump_ui_state()
                .map_err(|error| format!("DUMP_UI_STATE failed: {error}"))?
                .colors_dialog
                .ok_or_else(|| "the Colors dialog is not open".to_owned())
        };
        for (key, index) in pairs {
            let shown = colors_dialog()?;
            let position = shown
                .items
                .iter()
                .position(|item| item.item == *key)
                .ok_or_else(|| format!("colors: the Colors dialog has no item {key:?}"))?;
            send(combo, CB_SETCURSEL, position, 0)?;
            send(
                dialog,
                WM_COMMAND,
                ((CBN_SELCHANGE as usize) << 16) | COLORS_ITEM_ID as usize,
                combo as LPARAM,
            )?;
            let cell = shown.palette_cells[usize::from(*index)];
            let centre = (cell.x + cell.width / 2, cell.y + cell.height / 2);
            send_mouse(palette, WM_LBUTTONDOWN, MK_LBUTTON, centre)?;
            send_mouse(palette, WM_LBUTTONUP, 0, centre)?;
            let edited = colors_dialog()?;
            if edited.selected != *key || edited.items[position].index != *index {
                return Err(format!(
                    "colors: {key}={index} did not take: picked {:?}, {:?}",
                    edited.selected, edited.items[position]
                ));
            }
        }
        if unsafe { PostMessageW(dialog, WM_COMMAND, IDOK as WPARAM, 0) } == 0 {
            return Err(format!(
                "posting OK to the Colors dialog failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let deadline = std::time::Instant::now() + config.find_timeout;
        while unsafe { IsWindow(dialog) } != 0 {
            if std::time::Instant::now() > deadline {
                return Err("the Colors dialog did not close on OK".to_owned());
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        app.wait_idle(config.wait_idle)
            .map_err(|error| format!("WAIT_IDLE after the Colors dialog failed: {error}"))
    }

    fn open_dialog(
        app: &MirxApp,
        config: &Config,
        dialog: AutomationDialog,
        options_page: &Option<String>,
    ) -> Result<ModalDialog, String> {
        app.open_dialog(dialog)
            .map_err(|error| format!("OPEN_DIALOG failed: {error}"))?;
        let opened = app
            .wait_for_modal_dialog(config.find_timeout)
            .map_err(|error| format!("no dialog opened for {dialog:?}: {error}"))?;
        if let Some(path) = options_page {
            app.select_options_page(path)
                .map_err(|error| format!("SELECT_OPTIONS_PAGE failed: {error}"))?;
            let selected = app
                .dump_ui_state()
                .map_err(|error| format!("DUMP_UI_STATE failed: {error}"))?
                .options_dialog
                .ok_or_else(|| "the Options dialog is not open".to_owned())?
                .selected;
            if &selected != path {
                return Err(format!(
                    "Options page assertion failed: expected {path:?}, found {selected:?}"
                ));
            }
        }
        app.redraw_window(opened.hwnd)
            .map_err(|error| format!("redrawing the dialog failed: {error}"))?;
        app.wait_idle(config.wait_idle)
            .map_err(|error| format!("WAIT_IDLE after OPEN_DIALOG failed: {error}"))?;
        Ok(opened)
    }

    fn find_view<'a>(ui_state: &'a UiState, identity: &ViewIdentity) -> Option<&'a MdiView> {
        ui_state
            .mdi
            .views
            .iter()
            .find(|view| &view.identity == identity)
    }

    fn find_room_state<'a>(
        ui_state: &'a UiState,
        room_id: &str,
    ) -> Option<&'a mirx_testkit::UiRoomState> {
        ui_state.rooms.iter().find(|room| room.room_id == room_id)
    }

    fn assert_view_contains(
        ui_state: &UiState,
        expectation: &ViewExpectation,
        error_prefix: &str,
    ) -> Result<(), String> {
        let view = find_view(ui_state, &expectation.identity).ok_or_else(|| {
            format!(
                "{error_prefix}: missing view {}",
                format_view_identity(&expectation.identity)
            )
        })?;
        if !view_contains(view, &expectation.substring) {
            return Err(format!(
                "{error_prefix}: {} did not contain {:?}",
                format_view_identity(&expectation.identity),
                expectation.substring
            ));
        }
        Ok(())
    }

    fn view_contains(view: &MdiView, substring: &str) -> bool {
        view.lines.iter().any(|line| line.contains(substring))
    }

    fn view_members_contain(view: &MdiView, substring: &str) -> bool {
        view.members
            .iter()
            .any(|member| member.label.contains(substring) || member.user_id.contains(substring))
    }

    fn room_activity_contains(entry: &RoomActivity, substring: &str) -> bool {
        entry.contains(substring)
    }

    fn verification_state_name(state: &VerificationState) -> &'static str {
        match state {
            VerificationState::Requested => "requested",
            VerificationState::Accepted => "accepted",
            VerificationState::Ready => "ready",
            VerificationState::SasReady { .. } => "sas_ready",
            VerificationState::Confirmed => "confirmed",
            VerificationState::Done => "done",
            VerificationState::Cancelled { .. } => "cancelled",
        }
    }

    fn active_find_history_name(state: &UiFindHistorySearchState) -> &'static str {
        match state {
            UiFindHistorySearchState::LocalOnly => "local_only",
            UiFindHistorySearchState::Loading => "loading",
            UiFindHistorySearchState::Exhausted => "exhausted",
            UiFindHistorySearchState::Failed { .. } => "failed",
        }
    }

    fn render_verification_text(verification: &Verification) -> String {
        let mut parts = vec![
            format!("flow_id={}", verification.flow_id),
            format!("user_id={}", verification.user_id),
            format!("state={}", verification_state_name(&verification.state)),
        ];

        match &verification.state {
            VerificationState::SasReady { emojis, decimals } => {
                let emoji_text = emojis
                    .iter()
                    .map(|emoji| format!("{} {}", emoji.symbol, emoji.description))
                    .collect::<Vec<_>>()
                    .join(" | ");
                parts.push(format!("emoji={emoji_text}"));
                parts.push(format!(
                    "decimals={}-{}-{}",
                    decimals.first, decimals.second, decimals.third
                ));
            }
            VerificationState::Cancelled {
                reason,
                code,
                cancelled_by_us,
            } => {
                parts.push(format!("reason={reason}"));
                parts.push(format!("code={code}"));
                parts.push(format!("cancelled_by_us={cancelled_by_us}"));
            }
            VerificationState::Requested
            | VerificationState::Accepted
            | VerificationState::Ready
            | VerificationState::Confirmed
            | VerificationState::Done => {}
        }

        parts.join(" ")
    }

    fn navigation_contains(node: &NavigationNode, substring: &str) -> bool {
        node.text.contains(substring)
            || node
                .children
                .iter()
                .any(|child| navigation_contains(child, substring))
    }

    pub(crate) fn main() -> ExitCode {
        match run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("{message}");
                ExitCode::from(1)
            }
        }
    }
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    windows_main::main()
}
