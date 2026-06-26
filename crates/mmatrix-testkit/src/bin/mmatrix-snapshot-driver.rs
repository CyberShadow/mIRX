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

    use mmatrix_testkit::MmatrixApp;

    const DEFAULT_FIND_TIMEOUT: Duration = Duration::from_secs(10);
    const DEFAULT_WAIT_IDLE: Duration = Duration::from_secs(5);

    #[derive(Default)]
    struct Config {
        wait_idle: Duration,
        expect_selected: Option<String>,
        expect_rooms: Vec<String>,
        expect_timelines: Vec<TimelineExpectation>,
        expect_error: Option<String>,
        expect_verification: Option<String>,
    }

    struct TimelineExpectation {
        room_id: String,
        substring: String,
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

        let mut timeline_cache = BTreeMap::<String, Vec<String>>::new();
        for expectation in &config.expect_timelines {
            let lines = if let Some(lines) = timeline_cache.get(&expectation.room_id) {
                lines
            } else {
                let lines = app.timeline(Some(&expectation.room_id)).map_err(|error| {
                    format!(
                        "DUMP_TIMELINE failed for room {}: {error}",
                        expectation.room_id
                    )
                })?;
                timeline_cache.insert(expectation.room_id.clone(), lines);
                timeline_cache
                    .get(&expectation.room_id)
                    .expect("timeline cache entry must exist after insert")
            };

            let found = lines
                .iter()
                .any(|line| line.contains(&expectation.substring));
            if !found {
                return Err(format!(
                    "timeline assertion failed: room {} did not contain {:?}",
                    expectation.room_id, expectation.substring
                ));
            }
            println!(
                "PASS timeline {} contains {:?}",
                expectation.room_id, expectation.substring
            );
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
                "--expect-timeline" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --expect-timeline".to_owned())?;
                    let (room_id, substring) = value.split_once("::").ok_or_else(|| {
                        format!(
                            "invalid --expect-timeline value {value:?}; expected <room_id>::<substring>"
                        )
                    })?;
                    config.expect_timelines.push(TimelineExpectation {
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
            "  --expect-timeline <room_id>::<substring>",
            "  --expect-error <substring>",
            "  --expect-verification <flow_id>",
        ]
        .join("\n")
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
