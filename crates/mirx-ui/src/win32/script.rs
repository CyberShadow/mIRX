//! Running aliases (`aliases.rs`): each command of an alias typed in the
//! editbox is evaluated, then run as a typed command would be. A `//line`
//! typed in the editbox is run the same way, as mIRC's is: evaluated, with
//! no parameters, then run as a command.
//!
//! What a call evaluates is fixed when it starts: `#` and `$me` are those
//! of the window the alias was typed in. Where a command acts is not: a
//! `say` goes to the window active when it runs, so `/q $$1 | say hi`
//! greets in the query.
//!
//! A `$$` identifier left empty stops the whole call silently, nested
//! aliases too; a command's error stops it after printing the error with
//! the alias named, as mIRC's `(line 3, aliases.ini)` does.

use super::aliases::{Alias, CallContext, Evaluated, evaluate};
use super::command_runner::execute_slash_command;
use super::commands::classify_command;
use super::compose::submit_compose;
use super::*;

/// Whether an alias call goes on after a command.
enum Flow {
    Continue,
    Stop,
}

/// Where the commands of an alias call or a `//line` act, and print.
#[derive(Clone, Copy)]
enum ActsIn {
    /// Typed: each command acts in the window active when it runs, so
    /// `/q $$1 | say hi` greets in the query.
    FrontWindow,
}

/// The window a call was given in, as `#` and `$me` see it, and where its
/// commands act.
struct CallWindow {
    chan: String,
    me: String,
    acts_in: ActsIn,
}

impl CallWindow {
    /// `window`, where a line was typed.
    fn typed_in(state: &WindowState, window: &ViewId) -> Self {
        Self {
            chan: chan(&state.model, window),
            me: me(&state.model, window),
            acts_in: ActsIn::FrontWindow,
        }
    }

    /// The window the call's next command acts in, and prints in. The one
    /// place where the window in front decides where a command acts.
    fn window(&self, state: &WindowState) -> ViewId {
        match self.acts_in {
            ActsIn::FrontWindow => state.active_view.clone(),
        }
    }
}

/// Who runs a command, as its error names it.
#[derive(Clone, Copy)]
enum Caller<'a> {
    /// The alias of this name: ` (in alias /x)`.
    Alias(&'a str),
    /// A typed `//line`: nothing.
    Line,
}

impl Caller<'_> {
    /// An error of a command it ran, as printed.
    fn error_line(self, error: impl std::fmt::Display) -> String {
        match self {
            Caller::Alias(name) => format!("{error} (in alias /{name})"),
            Caller::Line => error.to_string(),
        }
    }
}

/// Runs the alias `name`, typed with `args` in `window`.
pub(super) unsafe fn run_alias_call(
    hwnd: HWND,
    window: &ViewId,
    name: &str,
    args: &str,
) -> Result<(), String> {
    let state = state_ref(hwnd);
    let alias = state
        .aliases
        .aliases()
        .get(name)
        .expect("a typed alias call names an alias")
        .clone();
    let window = CallWindow::typed_in(state, window);
    run_alias(hwnd, &alias, args, &window, &mut Vec::new()).map(|_| ())
}

/// Runs `//line`, typed in `window`: `line` evaluated as an alias's command
/// is, with no parameters, then run as a command. An empty `$$` identifier
/// stops it silently; a wrong identifier is the command's error.
pub(super) unsafe fn run_evaluated_line(
    hwnd: HWND,
    window: &ViewId,
    line: &str,
) -> Result<(), String> {
    let window = CallWindow::typed_in(state_ref(hwnd), window);
    let context = CallContext {
        params: &[],
        chan: &window.chan,
        me: &window.me,
    };
    match evaluate(line, &context) {
        Ok(Evaluated::Command(text)) => {
            run_command(hwnd, &text, Caller::Line, &window, &mut Vec::new()).map(|_| ())
        }
        Ok(Evaluated::Halt) => Ok(()),
        Err(reason) => {
            let name = line
                .split_ascii_whitespace()
                .next()
                .expect("a line with a wrong identifier has a word")
                .trim_start_matches('/');
            let acts_in = window.window(state_ref(hwnd));
            report_error_line(hwnd, &acts_in, format!("* /{name}: {reason}"))
        }
    }
}

/// Runs `alias` with `stack` holding the aliases running it, by lowercase
/// name: none of them is called again, so a call names their built-in.
unsafe fn run_alias(
    hwnd: HWND,
    alias: &Alias,
    args: &str,
    window: &CallWindow,
    stack: &mut Vec<String>,
) -> Result<Flow, String> {
    stack.push(alias.name.to_ascii_lowercase());
    let flow = run_alias_commands(hwnd, alias, args, window, stack);
    stack.pop();
    flow
}

unsafe fn run_alias_commands(
    hwnd: HWND,
    alias: &Alias,
    args: &str,
    window: &CallWindow,
    stack: &mut Vec<String>,
) -> Result<Flow, String> {
    let params = args.split_ascii_whitespace().collect::<Vec<_>>();
    let context = CallContext {
        params: &params,
        chan: &window.chan,
        me: &window.me,
    };
    for command in &alias.commands {
        let flow = match evaluate(command, &context) {
            Ok(Evaluated::Command(text)) => {
                run_command(hwnd, &text, Caller::Alias(&alias.name), window, stack)?
            }
            Ok(Evaluated::Halt) => Flow::Stop,
            Err(reason) => {
                let acts_in = window.window(state_ref(hwnd));
                report_error_line(hwnd, &acts_in, format!("* /{}: {reason}", alias.name))?;
                Flow::Stop
            }
        };
        if let Flow::Stop = flow {
            return Ok(Flow::Stop);
        }
    }
    Ok(Flow::Continue)
}

/// Runs one evaluated command that `caller` runs, in the window `window`
/// says. Its leading `/` is optional, and `//` the same as `/`: the command
/// is evaluated already.
unsafe fn run_command(
    hwnd: HWND,
    text: &str,
    caller: Caller,
    window: &CallWindow,
    stack: &mut Vec<String>,
) -> Result<Flow, String> {
    // Every word was an empty parameter: there is nothing to run.
    if text.is_empty() {
        return Ok(Flow::Continue);
    }
    let line = format!("/{}", text.trim_start_matches('/'));
    let acts_in = window.window(state_ref(hwnd));
    let submission = {
        let aliases = state_ref(hwnd).aliases.aliases();
        classify_command(&line, |name| {
            aliases.get(name).is_some() && !stack.iter().any(|on| on.eq_ignore_ascii_case(name))
        })
    };
    let failed = |error: CommandError| {
        report_error_line(hwnd, &acts_in, caller.error_line(error)).map(|()| Flow::Stop)
    };
    match submission {
        // Never the editbox's armed reply or edit: the alias sends text.
        ComposeSubmission::Message { presentation, body } => {
            match submit_compose(state_mut(hwnd), &acts_in, presentation, body, None) {
                Ok(_) => Ok(Flow::Continue),
                Err(reason) => failed(CommandError::new(
                    message_command_name(presentation),
                    reason,
                )),
            }
        }
        ComposeSubmission::LocalCommand(Ok(command)) => {
            match execute_slash_command(hwnd, &acts_in, command) {
                Ok(Some(status)) => {
                    set_local_command_status(hwnd, status)?;
                    Ok(Flow::Continue)
                }
                Ok(None) => Ok(Flow::Continue),
                Err(error) => failed(error),
            }
        }
        ComposeSubmission::LocalCommand(Err(error)) => failed(error),
        ComposeSubmission::Alias { name, args } => {
            let alias = state_ref(hwnd)
                .aliases
                .aliases()
                .get(name)
                .expect("classify_command names an alias")
                .clone();
            run_alias(hwnd, &alias, args, window, stack)
        }
    }
}

/// The command a message line was: `/say` or `/me`.
fn message_command_name(presentation: RoomMessagePresentationKind) -> &'static str {
    match presentation {
        RoomMessagePresentationKind::Text => "say",
        RoomMessagePresentationKind::Emote => "me",
        RoomMessagePresentationKind::Notice => unreachable!("no command line is a notice"),
    }
}

/// The room a window shows, a thread window's room too.
fn view_room_id(view: &ViewId) -> Option<&str> {
    match view {
        ViewId::Room(room_id) | ViewId::Thread { room_id, .. } => Some(room_id),
        ViewId::Status | ViewId::RoomList => None,
    }
}

/// `#` and `$chan`: the channel of a channel window (or its thread
/// window), by its alias or else its room ID, which every room-taking
/// command resolves. Empty in a query, Status and the Rooms List, as mIRC's
/// `#` is outside a channel.
fn chan(model: &UiModel, view: &ViewId) -> String {
    let Some(room) = view_room_id(view)
        .map(|room_id| room_by_id(&model.rooms, room_id).expect("a window's room is in the model"))
    else {
        return String::new();
    };
    match room_window_kind(room) {
        RoomWindowKind::Channel => room
            .canonical_alias
            .clone()
            .unwrap_or_else(|| room.room_id.clone()),
        RoomWindowKind::Query | RoomWindowKind::Invite => String::new(),
    }
}

/// `$me`: the user's nick in the window's room (their localpart there when
/// they set no display name), otherwise the user ID; empty when signed out.
fn me(model: &UiModel, view: &ViewId) -> String {
    let Some(user_id) = model.own_user_id() else {
        return String::new();
    };
    view_room_id(view)
        .and_then(|room_id| model.room_views.get(room_id))
        .and_then(|room_view| {
            room_view
                .members
                .iter()
                .find(|member| member.user_id == user_id)
        })
        .map_or(user_id, |member| member.nick.as_str())
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;

    fn context_model() -> UiModel {
        let mut aliased = room("!ops:example", "Ops", false);
        aliased.canonical_alias = Some("#ops:example".to_owned());
        let mut model = UiModel {
            rooms: vec![
                aliased,
                room("!dev:example", "Dev", false),
                query("!dm:example", "Alice", "@alice:example"),
            ],
            ..UiModel::default()
        };
        model.own_account = Some(OwnAccount::matrix("@me:example".to_owned()));
        model.room_views.insert(
            "!ops:example".to_owned(),
            RoomViewModel {
                members: vec![member_summary("@me:example", "Mee", false)],
                ..Default::default()
            },
        );
        model
    }

    #[test]
    fn chan_is_a_channel_windows_alias_or_room_id() {
        let model = context_model();
        assert_eq!(chan(&model, &room_view_id("!ops:example")), "#ops:example");
        assert_eq!(chan(&model, &room_view_id("!dev:example")), "!dev:example");
        assert_eq!(
            chan(&model, &thread_view_id("!ops:example", "$root")),
            "#ops:example"
        );
        assert_eq!(chan(&model, &room_view_id("!dm:example")), "");
        assert_eq!(chan(&model, &ViewId::Status), "");
        assert_eq!(chan(&model, &ViewId::RoomList), "");
    }

    #[test]
    fn me_is_the_own_nick_in_the_room_or_the_user_id() {
        let mut model = context_model();
        assert_eq!(me(&model, &room_view_id("!ops:example")), "Mee");
        assert_eq!(me(&model, &room_view_id("!dev:example")), "@me:example");
        assert_eq!(me(&model, &ViewId::Status), "@me:example");
        // Without a display name there, the core names us by localpart.
        model.room_views.insert(
            "!dev:example".to_owned(),
            RoomViewModel {
                members: vec![member_summary("@me:example", "me", false)],
                ..Default::default()
            },
        );
        assert_eq!(me(&model, &room_view_id("!dev:example")), "me");
        model.own_account = None;
        assert_eq!(me(&model, &room_view_id("!ops:example")), "");
    }

    /// A frame with the channel Ops (`#ops:example`, active) and a query
    /// with Alice, signed in as Mee; `extra` adds alias lines to the
    /// defaults.
    unsafe fn with_alias_frame(extra: &[&str], test: impl FnOnce(HWND)) {
        with_test_frame(600, 400, |hwnd| {
            let (sender, commands) = inert_command_sender();
            std::mem::forget(commands);
            let state = state_mut(hwnd);
            state.command_sender = Some(sender);
            state.model = context_model();
            state.model.rooms.remove(1);
            state
                .model
                .room_views
                .get_mut("!ops:example")
                .unwrap()
                .members
                .push(member_summary("@alice:example", "Alice", false));
            let mut source = state.aliases.aliases().source().to_vec();
            source.extend(extra.iter().map(|line| (*line).to_owned()));
            let aliases = aliases::Aliases::from_source(source).unwrap();
            state.aliases.replace(aliases).unwrap();
            sync_views_and_tree(hwnd).unwrap();
            activate_view(hwnd, &room_view_id("!ops:example"), None).unwrap();
            test(hwnd);
        });
    }

    fn sent(state: &WindowState) -> Vec<(String, RoomMessagePresentationKind, String)> {
        state
            .recorded_commands
            .iter()
            .filter_map(|command| match command {
                Command::SendMessage(request) => Some((
                    request.room_id.clone(),
                    request.presentation,
                    request.body.clone(),
                )),
                _ => None,
            })
            .collect()
    }

    fn joins(state: &WindowState) -> Vec<String> {
        state
            .recorded_commands
            .iter()
            .filter_map(|command| match command {
                Command::JoinRoom(request) => Some(request.room_or_alias.clone()),
                _ => None,
            })
            .collect()
    }

    fn ops_lines(hwnd: HWND) -> Vec<String> {
        unsafe { render_room_lines(state_ref(hwnd), "!ops:example") }
    }

    fn text_to(room_id: &str, body: &str) -> (String, RoomMessagePresentationKind, String) {
        (
            room_id.to_owned(),
            RoomMessagePresentationKind::Text,
            body.to_owned(),
        )
    }

    #[test]
    fn default_aliases_run_the_commands_they_name() {
        unsafe {
            with_alias_frame(&[], |hwnd| {
                let lines_before = ops_lines(hwnd);
                type_compose_line(hwnd, "/j beta:example");
                type_compose_line(hwnd, "/J #gamma:example");
                assert_eq!(joins(state_ref(hwnd)), ["#beta:example", "#gamma:example"]);
                // `$$1` without a parameter: nothing at all.
                type_compose_line(hwnd, "/w");
                type_compose_line(hwnd, "/j");
                assert_eq!(state_ref(hwnd).recorded_commands.len(), 2);
                assert_eq!(ops_lines(hwnd), lines_before);
                type_compose_line(hwnd, "/!j x");
                assert_eq!(
                    ops_lines(hwnd).last().unwrap(),
                    "[22:12] * /j: unknown command"
                );
                // The line is history as typed.
                assert_eq!(
                    active_compose_history(state_mut(hwnd)).entries.first(),
                    Some(&"/j beta:example".to_owned())
                );
            });
        }
    }

    #[test]
    fn an_alias_runs_its_commands_until_one_fails() {
        unsafe {
            with_alias_frame(
                &["/x {", "  say one", "  me two | bogus", "  say three", "}"],
                |hwnd| {
                    type_compose_line(hwnd, "/x");
                    assert_eq!(
                        sent(state_ref(hwnd)),
                        [
                            text_to("!ops:example", "one"),
                            (
                                "!ops:example".to_owned(),
                                RoomMessagePresentationKind::Emote,
                                "two".to_owned()
                            ),
                        ]
                    );
                    assert_eq!(
                        ops_lines(hwnd).last().unwrap(),
                        "[22:12] * /bogus: unknown command (in alias /x)"
                    );
                    assert_eq!(
                        state_ref(hwnd).model.status,
                        "* /bogus: unknown command (in alias /x)"
                    );
                },
            );
        }
    }

    #[test]
    fn identifiers_take_the_call_and_its_window() {
        unsafe {
            with_alias_frame(
                &["/ctx say $2- in # as $me $0", "/bad say $active"],
                |hwnd| {
                    type_compose_line(hwnd, "/ctx a b  c");
                    assert_eq!(
                        sent(state_ref(hwnd)),
                        [text_to("!ops:example", "b c in #ops:example as Mee 3")]
                    );
                    type_compose_line(hwnd, "/bad");
                    assert_eq!(
                        ops_lines(hwnd).last().unwrap(),
                        "[22:12] * /bad: no such identifier: $active"
                    );
                    assert_eq!(sent(state_ref(hwnd)).len(), 1);
                },
            );
        }
    }

    #[test]
    fn an_alias_calling_itself_gets_the_built_in() {
        unsafe {
            with_alias_frame(
                &["/join say joining $1 | join $1", "/a b", "/b a"],
                |hwnd| {
                    type_compose_line(hwnd, "/join #beta:example");
                    assert_eq!(
                        sent(state_ref(hwnd)),
                        [text_to("!ops:example", "joining #beta:example")]
                    );
                    assert_eq!(joins(state_ref(hwnd)), ["#beta:example"]);
                    type_compose_line(hwnd, "/a");
                    assert_eq!(
                        ops_lines(hwnd).last().unwrap(),
                        "[22:12] * /a: unknown command (in alias /b)"
                    );
                },
            );
        }
    }

    #[test]
    fn a_halt_stops_the_whole_call_and_an_error_too() {
        unsafe {
            with_alias_frame(
                &[
                    "/outer inner $1 | say after",
                    "/inner say $$1",
                    "/fails nested | say after",
                    "/nested bogus",
                ],
                |hwnd| {
                    let lines_before = ops_lines(hwnd);
                    type_compose_line(hwnd, "/outer");
                    assert!(sent(state_ref(hwnd)).is_empty());
                    assert_eq!(ops_lines(hwnd), lines_before);
                    type_compose_line(hwnd, "/outer hi");
                    assert_eq!(
                        sent(state_ref(hwnd)),
                        [
                            text_to("!ops:example", "hi"),
                            text_to("!ops:example", "after")
                        ]
                    );
                    type_compose_line(hwnd, "/fails");
                    assert_eq!(sent(state_ref(hwnd)).len(), 2);
                    assert_eq!(
                        ops_lines(hwnd).last().unwrap(),
                        "[22:12] * /bogus: unknown command (in alias /nested)"
                    );
                },
            );
        }
    }

    #[test]
    fn a_double_slash_line_is_evaluated_then_run() {
        unsafe {
            with_alias_frame(&["/hi say hi $1"], |hwnd| {
                let lines_before = ops_lines(hwnd);
                type_compose_line(hwnd, "//echo in # as $me $+ ! $1");
                assert_eq!(ops_lines(hwnd).last().unwrap(), "in #ops:example as Mee!");
                // Aliases are called; `$$` stops silently.
                type_compose_line(hwnd, "//hi $me");
                type_compose_line(hwnd, "//say $$1 never");
                type_compose_line(hwnd, "///say slashes");
                assert_eq!(
                    sent(state_ref(hwnd)),
                    [
                        text_to("!ops:example", "hi Mee"),
                        text_to("!ops:example", "slashes")
                    ]
                );
                assert_eq!(ops_lines(hwnd).len(), lines_before.len() + 1);
                // Errors name the command, with no alias.
                type_compose_line(hwnd, "//hello");
                assert_eq!(
                    ops_lines(hwnd).last().unwrap(),
                    "[22:12] * /hello: unknown command"
                );
                type_compose_line(hwnd, "//say $active");
                assert_eq!(
                    ops_lines(hwnd).last().unwrap(),
                    "[22:12] * /say: no such identifier: $active"
                );
                // Typed text that starts with `/` is sent with /say.
                type_compose_line(hwnd, "/say //not a command");
                assert_eq!(
                    sent(state_ref(hwnd)).last().unwrap(),
                    &text_to("!ops:example", "//not a command")
                );
                assert_eq!(
                    active_compose_history(state_mut(hwnd)).entries.last(),
                    Some(&"/say //not a command".to_owned())
                );
            });
        }
    }

    #[test]
    fn say_goes_to_the_window_active_when_it_runs() {
        unsafe {
            with_alias_frame(&["/greet /q $$1 | say hi $1"], |hwnd| {
                type_compose_line(hwnd, "/greet Alice");
                assert_eq!(state_ref(hwnd).active_view, room_view_id("!dm:example"));
                assert_eq!(sent(state_ref(hwnd)), [text_to("!dm:example", "hi Alice")]);
            });
        }
    }

    #[test]
    fn echo_prints_where_it_is_told_and_raises_event_activity_elsewhere() {
        unsafe {
            with_alias_frame(&["/note echo -s noted $1"], |hwnd| {
                type_compose_line(hwnd, "/echo here");
                type_compose_line(hwnd, "/echo #ops:example here too");
                assert_eq!(
                    ops_lines(hwnd)[ops_lines(hwnd).len() - 2..],
                    ["here", "here too"]
                );
                assert_eq!(state_ref(hwnd).room_activity, BTreeMap::new());
                type_compose_line(hwnd, "/echo 4 -t !dm:example hi");
                type_compose_line(hwnd, "/note it");
                let state = state_ref(hwnd);
                assert_eq!(
                    render_room_lines(state, "!dm:example").last().unwrap(),
                    "[22:12] hi"
                );
                let status = project_timeline_rows(&build_status_timeline_rows(
                    &state.model.status_log,
                    state.timestamps(),
                ));
                assert_eq!(status.last().unwrap(), "noted it");
                assert_eq!(
                    state.room_activity,
                    BTreeMap::from([
                        (ViewId::Status, ViewActivityLevel::Event),
                        (room_view_id("!dm:example"), ViewActivityLevel::Event),
                    ])
                );
                type_compose_line(hwnd, "/echo #nope:example x");
                assert_eq!(
                    ops_lines(hwnd).last().unwrap(),
                    "[22:12] * /echo: no such window: #nope:example"
                );
            });
        }
    }

    #[test]
    fn alias_sets_lists_and_removes_aliases() {
        unsafe {
            with_alias_frame(&[], |hwnd| {
                type_compose_line(hwnd, "/alias hi say hello $1");
                assert_eq!(state_ref(hwnd).model.status, "alias set: /hi");
                type_compose_line(hwnd, "/hi there");
                assert_eq!(
                    sent(state_ref(hwnd)),
                    [text_to("!ops:example", "hello there")]
                );
                type_compose_line(hwnd, "/alias /hi { say a | say b }");
                type_compose_line(hwnd, "/alias");
                let lines = ops_lines(hwnd);
                assert_eq!(
                    lines[lines.len() - 10..],
                    [
                        "[22:12] * Aliases:",
                        "[22:12] *   /dop /deop $$1",
                        "[22:12] *   /j /join #$$1",
                        "[22:12] *   /p /part #",
                        "[22:12] *   /n /names #$$1",
                        "[22:12] *   /w /whois $$1",
                        "[22:12] *   /k /kick $$1 $2-",
                        "[22:12] *   /q /query $$1",
                        "[22:12] *   /s /server $$1",
                        "[22:12] *   /hi { say a | say b }",
                    ]
                );
                type_compose_line(hwnd, "/alias hi");
                assert_eq!(state_ref(hwnd).model.status, "alias removed: /hi");
                type_compose_line(hwnd, "/hi");
                assert_eq!(
                    ops_lines(hwnd).last().unwrap(),
                    "[22:12] * /hi: unknown command"
                );
                type_compose_line(hwnd, "/alias hi");
                assert_eq!(
                    ops_lines(hwnd).last().unwrap(),
                    "[22:12] * /alias: no such alias: /hi"
                );
                type_compose_line(hwnd, "/alias $x say");
                assert_eq!(
                    ops_lines(hwnd).last().unwrap(),
                    "[22:12] * /alias: bad alias name: $x"
                );
                for name in ["dop", "j", "p", "n", "w", "k", "q", "s"] {
                    type_compose_line(hwnd, &format!("/alias {name}"));
                }
                type_compose_line(hwnd, "/alias");
                assert_eq!(ops_lines(hwnd).last().unwrap(), "[22:12] * No aliases");
            });
        }
    }
}
