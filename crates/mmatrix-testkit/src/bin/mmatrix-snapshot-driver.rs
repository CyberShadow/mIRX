#[cfg(not(windows))]
fn main() {
    eprintln!("mmatrix-snapshot-driver requires the Windows target");
    std::process::exit(1);
}

#[cfg(windows)]
mod windows_main {
    use std::collections::BTreeMap;
    use std::env;
    use std::process::ExitCode;
    use std::time::Duration;

    use mmatrix_testkit::{
        MdiView, MmatrixApp, NavigationNode, RoomActivity, UiState, ViewIdentity,
    };

    const DEFAULT_FIND_TIMEOUT: Duration = Duration::from_secs(10);
    const DEFAULT_WAIT_IDLE: Duration = Duration::from_secs(5);

    #[derive(Default)]
    struct Config {
        wait_idle: Duration,
        expect_selected: Option<String>,
        expect_rooms: Vec<String>,
        expect_room_activities: Vec<RoomActivityExpectation>,
        expect_error: Option<String>,
        expect_verification: Option<String>,
        expect_active_view: Option<ViewIdentity>,
        expect_room_views: Vec<ViewTextExpectation>,
        expect_status_views: Vec<String>,
        expect_status_bars: Vec<String>,
        expect_switchbar_items: Vec<SwitchbarItemExpectation>,
        expect_no_switchbar_items: Vec<ViewIdentity>,
        expect_switchbar_active: Option<ViewIdentity>,
        expect_switchbar_activity: Vec<ViewIdentity>,
        expect_no_switchbar_activity: Vec<ViewIdentity>,
        expect_navigation_texts: Vec<String>,
        expect_no_navigation_texts: Vec<String>,
    }

    struct RoomActivityExpectation {
        room_id: String,
        substring: String,
    }

    struct ViewTextExpectation {
        room_id: String,
        substring: String,
    }

    struct SwitchbarItemExpectation {
        identity: ViewIdentity,
        title_substring: String,
    }

    impl Config {
        fn requires_ui_state(&self) -> bool {
            self.expect_active_view.is_some()
                || !self.expect_room_views.is_empty()
                || !self.expect_status_views.is_empty()
                || !self.expect_status_bars.is_empty()
                || !self.expect_switchbar_items.is_empty()
                || !self.expect_no_switchbar_items.is_empty()
                || self.expect_switchbar_active.is_some()
                || !self.expect_switchbar_activity.is_empty()
                || !self.expect_no_switchbar_activity.is_empty()
                || !self.expect_navigation_texts.is_empty()
                || !self.expect_no_navigation_texts.is_empty()
        }
    }

    pub(crate) fn run() -> Result<(), String> {
        let config = parse_args(env::args().skip(1))?;
        let app = MmatrixApp::find(DEFAULT_FIND_TIMEOUT).map_err(|error| {
            format!("failed to attach to mmatrix-ui automation window: {error}")
        })?;
        app.wait_idle(config.wait_idle)
            .map_err(|error| format!("WAIT_IDLE failed: {error}"))?;

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

            if !config.expect_status_views.is_empty() {
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

            for identity in &config.expect_switchbar_activity {
                let item = ui_state.switchbar.item(identity).ok_or_else(|| {
                    format!(
                        "switchbar activity assertion failed: missing item {}",
                        format_view_identity(identity)
                    )
                })?;
                if !item.has_activity {
                    return Err(format!(
                        "switchbar activity assertion failed: expected {} to have activity, found false",
                        format_view_identity(identity)
                    ));
                }
                println!("PASS switchbar activity {}", format_view_identity(identity));
            }

            for identity in &config.expect_no_switchbar_activity {
                let item = ui_state.switchbar.item(identity).ok_or_else(|| {
                    format!(
                        "switchbar no-activity assertion failed: missing item {}",
                        format_view_identity(identity)
                    )
                })?;
                if item.has_activity {
                    return Err(format!(
                        "switchbar no-activity assertion failed: expected {} to have no activity, found true",
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
        }

        Ok(())
    }

    fn parse_args(args: impl Iterator<Item = String>) -> Result<Config, String> {
        let mut config = Config {
            wait_idle: DEFAULT_WAIT_IDLE,
            ..Config::default()
        };
        let mut args = args.peekable();

        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--wait-idle-ms" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --wait-idle-ms".to_owned())?;
                    let millis = value
                        .parse::<u64>()
                        .map_err(|_| format!("invalid --wait-idle-ms value: {value}"))?;
                    config.wait_idle = Duration::from_millis(millis);
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
                "--expect-active-view" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-active-view".to_owned())?;
                    config.expect_active_view =
                        Some(parse_view_identity_arg("--expect-active-view", &value)?);
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
                "--expect-status-view" => {
                    config.expect_status_views.push(
                        args.next()
                            .ok_or_else(|| "missing value for --expect-status-view".to_owned())?,
                    );
                }
                "--expect-status-bar" => {
                    config.expect_status_bars.push(
                        args.next()
                            .ok_or_else(|| "missing value for --expect-status-bar".to_owned())?,
                    );
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
                "--expect-switchbar-activity" => {
                    let value = args.next().ok_or_else(|| {
                        "missing value for --expect-switchbar-activity".to_owned()
                    })?;
                    config
                        .expect_switchbar_activity
                        .push(parse_view_identity_arg(
                            "--expect-switchbar-activity",
                            &value,
                        )?);
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
                "--help" => {
                    return Err(usage());
                }
                other => {
                    return Err(format!("unknown argument: {other}\n{}", usage()));
                }
            }
        }

        Ok(config)
    }

    fn usage() -> String {
        [
            "usage: mmatrix-snapshot-driver [options]",
            "  --wait-idle-ms <ms>",
            "  --expect-selected <room_id>",
            "  --expect-room <room_id>",
            "  --expect-room-activity <room_id>::<substring>",
            "  --expect-error <substring>",
            "  --expect-verification <flow_id>",
            "  --expect-active-view <status|room:<room_id>>",
            "  --expect-room-view <room_id>::<substring>",
            "  --expect-status-view <substring>",
            "  --expect-status-bar <substring>",
            "  --expect-switchbar-item <identity>::<title-substring>",
            "  --expect-no-switchbar-item <status|room:<room_id>>",
            "  --expect-switchbar-active <status|room:<room_id>>",
            "  --expect-switchbar-activity <status|room:<room_id>>",
            "  --expect-no-switchbar-activity <status|room:<room_id>>",
            "  --expect-navigation-text <substring>",
            "  --expect-no-navigation-text <substring>",
        ]
        .join("\n")
    }

    fn parse_view_identity_arg(flag_name: &str, value: &str) -> Result<ViewIdentity, String> {
        if value == "status" {
            return Ok(ViewIdentity::Status);
        }
        if let Some(room_id) = value.strip_prefix("room:") {
            if room_id.is_empty() {
                return Err(format!(
                    "invalid {flag_name} value {value:?}; expected status or room:<room_id>\n{}",
                    usage()
                ));
            }
            return Ok(ViewIdentity::Room {
                room_id: room_id.to_owned(),
            });
        }
        Err(format!(
            "invalid {flag_name} value {value:?}; expected status or room:<room_id>\n{}",
            usage()
        ))
    }

    fn format_view_identity(identity: &ViewIdentity) -> String {
        match identity {
            ViewIdentity::Status => "status".to_owned(),
            ViewIdentity::Room { room_id } => format!("room:{room_id}"),
        }
    }

    fn find_view<'a>(ui_state: &'a UiState, identity: &ViewIdentity) -> Option<&'a MdiView> {
        ui_state
            .mdi
            .views
            .iter()
            .find(|view| &view.identity == identity)
    }

    fn view_contains(view: &MdiView, substring: &str) -> bool {
        view.lines.iter().any(|line| line.contains(substring))
    }

    fn room_activity_contains(entry: &RoomActivity, substring: &str) -> bool {
        entry.contains(substring)
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
