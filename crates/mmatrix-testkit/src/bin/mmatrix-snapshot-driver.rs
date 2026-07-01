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
        MdiView, MmatrixApp, NavigationNode, RoomActivity, UiState, Verification,
        VerificationState, ViewIdentity,
    };

    const DEFAULT_FIND_TIMEOUT: Duration = Duration::from_secs(10);
    const DEFAULT_WAIT_IDLE: Duration = Duration::from_secs(5);

    #[derive(Default)]
    struct Config {
        find_timeout: Duration,
        wait_idle: Duration,
        select_view: Option<ViewIdentity>,
        expect_selected: Option<String>,
        expect_rooms: Vec<String>,
        expect_room_activities: Vec<RoomActivityExpectation>,
        expect_error: Option<String>,
        expect_verification: Option<String>,
        expect_verification_state: Option<String>,
        expect_verification_texts: Vec<String>,
        expect_active_view: Option<ViewIdentity>,
        expect_views: Vec<ViewExpectation>,
        expect_no_views: Vec<ViewExpectation>,
        expect_room_views: Vec<ViewTextExpectation>,
        expect_room_members: Vec<ViewTextExpectation>,
        expect_status_views: Vec<String>,
        expect_no_status_views: Vec<String>,
        expect_status_bars: Vec<String>,
        expect_no_status_bars: Vec<String>,
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

    struct ViewExpectation {
        identity: ViewIdentity,
        substring: String,
    }

    struct SwitchbarItemExpectation {
        identity: ViewIdentity,
        title_substring: String,
    }

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
                || !self.expect_switchbar_activity.is_empty()
                || !self.expect_no_switchbar_activity.is_empty()
                || !self.expect_navigation_texts.is_empty()
                || !self.expect_no_navigation_texts.is_empty()
        }
    }

    pub(crate) fn run() -> Result<(), String> {
        let config = parse_args(env::args().skip(1))?;
        let app = MmatrixApp::find(config.find_timeout).map_err(|error| {
            format!("failed to attach to mmatrix-ui automation window: {error}")
        })?;
        app.wait_idle(config.wait_idle)
            .map_err(|error| format!("WAIT_IDLE failed: {error}"))?;

        if let Some(view) = config.select_view.as_ref() {
            app.select_view(view.clone())
                .map_err(|error| format!("SELECT_VIEW failed: {error}"))?;
            app.wait_idle(config.wait_idle)
                .map_err(|error| format!("WAIT_IDLE after SELECT_VIEW failed: {error}"))?;
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
                    config.select_view = Some(parse_view_identity_arg("--select-view", &value)?);
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
            "  --find-timeout-ms <ms>",
            "  --wait-idle-ms <ms>",
            "  --select-view <identity>",
            "  --expect-selected <room_id>",
            "  --expect-room <room_id>",
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
            "  --expect-switchbar-activity <identity>",
            "  --expect-no-switchbar-activity <identity>",
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
                    "invalid {flag_name} value {value:?}; expected status, room:<room_id>, or thread:<room_id>|<thread_root_event_id>\n{}",
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
                    "invalid {flag_name} value {value:?}; expected status, room:<room_id>, or thread:<room_id>|<thread_root_event_id>\n{}",
                    usage()
                ));
            };
            if room_id.is_empty() || thread_root_event_id.is_empty() {
                return Err(format!(
                    "invalid {flag_name} value {value:?}; expected status, room:<room_id>, or thread:<room_id>|<thread_root_event_id>\n{}",
                    usage()
                ));
            }
            return Ok(ViewIdentity::Thread {
                room_id: room_id.to_owned(),
                thread_root_event_id: thread_root_event_id.to_owned(),
            });
        }
        Err(format!(
            "invalid {flag_name} value {value:?}; expected status, room:<room_id>, or thread:<room_id>|<thread_root_event_id>\n{}",
            usage()
        ))
    }

    fn parse_view_expectation(flag_name: &str, value: &str) -> Result<ViewExpectation, String> {
        let (identity, substring) = value.split_once("::").ok_or_else(|| {
            format!("invalid {flag_name} value {value:?}; expected <identity>::<substring>")
        })?;
        Ok(ViewExpectation {
            identity: parse_view_identity_arg(flag_name, identity)?,
            substring: substring.to_owned(),
        })
    }

    fn format_view_identity(identity: &ViewIdentity) -> String {
        match identity {
            ViewIdentity::Status => "status".to_owned(),
            ViewIdentity::Room { room_id } => format!("room:{room_id}"),
            ViewIdentity::Thread {
                room_id,
                thread_root_event_id,
            } => format!("thread:{room_id}|{thread_root_event_id}"),
        }
    }

    fn find_view<'a>(ui_state: &'a UiState, identity: &ViewIdentity) -> Option<&'a MdiView> {
        ui_state
            .mdi
            .views
            .iter()
            .find(|view| &view.identity == identity)
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

    #[cfg(test)]
    mod tests {
        use super::{
            ViewExpectation, format_view_identity, parse_args, parse_view_expectation,
            parse_view_identity_arg, usage,
        };
        use mmatrix_testkit::ViewIdentity;

        #[test]
        fn usage_documents_thread_identity_format() {
            assert!(usage().contains(
                "--expect-active-view <status|room:<room_id>|thread:<room_id>|<thread_root_event_id>>"
            ));
            assert!(usage().contains("--select-view <identity>"));
            assert!(usage().contains("--expect-view <identity>::<substring>"));
            assert!(usage().contains("--expect-no-view <identity>::<substring>"));
            assert!(usage().contains("thread:<room_id>|<thread_root_event_id>"));
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
                config.select_view,
                Some(ViewIdentity::Thread {
                    room_id: "!room:example".to_owned(),
                    thread_root_event_id: "$thread-root:example".to_owned(),
                })
            );
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
                    parse_args(args.into_iter().map(str::to_owned)).is_err(),
                    "expected {:?} to be rejected",
                    args
                );
            }
        }
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
