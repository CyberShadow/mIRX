//! Running command lines with mIRC's identifiers: each command of an alias
//! typed in the editbox (`aliases.rs`) is evaluated, then run as a typed
//! command would be. A `//line` typed in the editbox is run the same way,
//! as mIRC's is: evaluated, with no parameters, then run as a command. So
//! are the lines of Perform on connect (`perform.rs`), as typed in Status.
//!
//! What a call evaluates is fixed when it starts: `#` and `$me` are those
//! of the window the alias was typed in. Where a typed call's command acts
//! is not: a `say` goes to the window active when it runs, so `/q $$1 |
//! say hi` greets in the query (`ActsIn`). Perform's act in Status.
//!
//! A `$$` identifier left empty stops the whole call silently, nested
//! aliases too; a command's error stops it after printing the error with
//! the alias named, as mIRC's `(line 3, aliases.ini)` does.
//!
//! A call's commands run in order: each once the core has answered what
//! the one before it sent, as an IRC server reads a client's lines in
//! order, so `/join #x | msg #x hi` greets in the room just joined. A
//! call that waits is kept (`ScriptRuns`) and goes on at
//! `WM_SCRIPT_RESUME`; the UI stays usable meanwhile. A server's refusal
//! prints where it does and does not stop the call.

use std::collections::VecDeque;

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
    /// Perform: every command acts in Status, whatever is in front.
    Status,
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

    /// Status, where perform's lines run: `#` is empty, `$me` the user ID.
    fn status(state: &WindowState) -> Self {
        Self {
            chan: chan(&state.model, &ViewId::Status),
            me: me(&state.model, &ViewId::Status),
            acts_in: ActsIn::Status,
        }
    }

    /// The window the call's next command acts in, and prints in. The one
    /// place where the window in front decides where a command acts.
    fn window(&self, state: &WindowState) -> ViewId {
        match self.acts_in {
            ActsIn::FrontWindow => state.active_view.clone(),
            ActsIn::Status => ViewId::Status,
        }
    }
}

/// Who runs a command, as its error names it.
enum Caller {
    /// The alias of this name, as written: ` (in alias /x)`.
    Alias(String),
    /// A typed `//line`: nothing.
    Line,
    /// Perform on connect: ` (in perform)`.
    Perform,
}

impl Caller {
    /// An error of a command it ran, as printed.
    fn error_line(&self, error: impl std::fmt::Display) -> String {
        match self {
            Caller::Alias(name) => format!("{error} (in alias /{name})"),
            Caller::Line => error.to_string(),
            Caller::Perform => format!("{error} (in perform)"),
        }
    }

    /// The error of `command`, which it could not evaluate: an alias's
    /// names the alias, as mIRC's names its line; another's names the
    /// command.
    fn evaluation_error(&self, command: &str, reason: &str) -> String {
        match self {
            Caller::Alias(name) => format!("* /{name}: {reason}"),
            Caller::Line | Caller::Perform => {
                let name = command
                    .split_ascii_whitespace()
                    .next()
                    .expect("a command with a wrong identifier has a word")
                    .trim_start_matches('/');
                self.error_line(format!("* /{name}: {reason}"))
            }
        }
    }
}

/// The commands of one caller (an alias, a `//line`, a perform line) with
/// its parameters, and where it is.
struct Frame {
    caller: Caller,
    commands: Vec<String>,
    params: Vec<String>,
    /// The index of the command that runs next.
    next: usize,
}

impl Frame {
    fn new(caller: Caller, commands: Vec<String>, params: Vec<String>) -> Self {
        Self {
            caller,
            commands,
            params,
            next: 0,
        }
    }

    /// `alias`, called with `args`.
    fn alias(alias: &Alias, args: &str) -> Self {
        let params = args.split_ascii_whitespace().map(str::to_owned).collect();
        Self::new(
            Caller::Alias(alias.name.clone()),
            alias.commands.clone(),
            params,
        )
    }
}

/// A run under way: an alias call, a `//line`, or perform's lines. Owned,
/// so that it can wait for the core between two commands
/// (`ScriptRuns`).
struct Run {
    window: CallWindow,
    /// The calls that run after the current one: perform's next lines.
    calls: VecDeque<Frame>,
    /// The current call: its own frame first, then the aliases it called,
    /// innermost last. Empty between calls.
    frames: Vec<Frame>,
}

impl Run {
    fn new(window: CallWindow, calls: impl IntoIterator<Item = Frame>) -> Self {
        Self {
            window,
            calls: calls.into_iter().collect(),
            frames: Vec::new(),
        }
    }

    /// The next command to run, which the innermost frame now holds;
    /// `None` when the run is done.
    fn next_command(&mut self) -> Option<String> {
        loop {
            match self.frames.last() {
                Some(frame) if frame.next < frame.commands.len() => break,
                Some(_) => {
                    self.frames.pop();
                }
                None => self.frames.push(self.calls.pop_front()?),
            }
        }
        let frame = self.innermost_mut();
        frame.next += 1;
        Some(frame.commands[frame.next - 1].clone())
    }

    /// The frame whose command runs.
    fn innermost(&self) -> &Frame {
        self.frames.last().expect("a command runs in a frame")
    }

    fn innermost_mut(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("a command runs in a frame")
    }

    /// Whether a command remains to run.
    fn has_more(&self) -> bool {
        !self.calls.is_empty()
            || self
                .frames
                .iter()
                .any(|frame| frame.next < frame.commands.len())
    }

    /// Whether `name` runs already: an alias never calls itself, nor one
    /// running it, so such a call names the built-in.
    fn is_running(&self, name: &str) -> bool {
        self.frames.iter().any(|frame| match &frame.caller {
            Caller::Alias(running) => running.eq_ignore_ascii_case(name),
            Caller::Line | Caller::Perform => false,
        })
    }
}

/// How far a step took a run.
enum Step {
    Done,
    /// Its next command waits for the core to answer these, which its last
    /// command sent.
    Waiting(Vec<CommandId>),
}

/// The runs waiting for the core, in the order they began to wait. Each
/// waits for commands its last command sent; it is ready once the core
/// answered them all. A run that is being stepped is not here.
#[derive(Default)]
pub(super) struct ScriptRuns(Vec<(Run, Vec<CommandId>)>);

impl ScriptRuns {
    fn suspend(&mut self, run: Run, ids: Vec<CommandId>) {
        assert!(!ids.is_empty(), "a run waits for commands");
        self.0.push((run, ids));
    }

    pub(super) fn any_ready(&self, answered: &AnsweredCommands) -> bool {
        self.0.iter().any(|(_, ids)| all_answered(ids, answered))
    }

    /// The first ready run, taken out.
    fn take_ready(&mut self, answered: &AnsweredCommands) -> Option<Run> {
        let index = self
            .0
            .iter()
            .position(|(_, ids)| all_answered(ids, answered))?;
        Some(self.0.remove(index).0)
    }
}

fn all_answered(ids: &[CommandId], answered: &AnsweredCommands) -> bool {
    ids.iter().all(|id| answered.is_answered(*id))
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
        .expect("a typed alias call names an alias");
    let call = Frame::alias(alias, args);
    start_run(hwnd, Run::new(CallWindow::typed_in(state, window), [call]))
}

/// Runs `//line`, typed in `window`: `line` evaluated as an alias's command
/// is, with no parameters, then run as a command. An empty `$$` identifier
/// stops it silently; a wrong identifier is the command's error.
pub(super) unsafe fn run_evaluated_line(
    hwnd: HWND,
    window: &ViewId,
    line: &str,
) -> Result<(), String> {
    let call = Frame::new(Caller::Line, vec![line.to_owned()], Vec::new());
    let window = CallWindow::typed_in(state_ref(hwnd), window);
    start_run(hwnd, Run::new(window, [call]))
}

/// mIRC's Perform on connect: each line of the setting, if it is enabled,
/// run as if typed in Status, whatever window is in front. A line stops at
/// its first error, which Status prints, and the next line still runs:
/// each is a call of its own, in one run, so a line runs once the core
/// answered what the line before it sent.
pub(super) unsafe fn run_perform(hwnd: HWND) -> Result<(), String> {
    let state = state_ref(hwnd);
    let perform = &state.settings.settings().perform;
    // Read when it runs: the setting may have changed since the connect
    // posted it.
    if !perform.enabled {
        return Ok(());
    }
    // Copied: a line may change the settings (`/notify`, `/cnick`).
    let calls = perform
        .lines()
        .iter()
        .map(|line| Frame::new(Caller::Perform, line.commands().to_vec(), Vec::new()))
        .collect::<Vec<_>>();
    start_run(hwnd, Run::new(CallWindow::status(state), calls))
}

/// Steps the first run whose commands the core has answered
/// (`WM_SCRIPT_RESUME`), and posts the next resume if another is ready.
pub(super) unsafe fn resume_ready_run(hwnd: HWND) -> Result<(), String> {
    let state = state_mut(hwnd);
    // A resume posted twice finds nothing the second time.
    let Some(run) = state.script_runs.take_ready(&state.core_answers) else {
        return Ok(());
    };
    let stepped = start_run(hwnd, run);
    post_resume_if_ready(hwnd);
    stepped
}

/// Posts `WM_SCRIPT_RESUME` if a waiting run can go on: after core events
/// are applied, acks among them. Posted, never run inside the batch: a
/// command may open a modal dialog, and the activation a `RoomJoined`
/// asks for happens in the batch's refresh.
pub(super) unsafe fn post_resume_if_ready(hwnd: HWND) {
    let state = state_ref(hwnd);
    if state.script_runs.any_ready(&state.core_answers) {
        PostMessageW(hwnd, WM_SCRIPT_RESUME, 0, 0);
    }
}

/// Steps `run`, and keeps it if it waits for the core.
unsafe fn start_run(hwnd: HWND, mut run: Run) -> Result<(), String> {
    match step(hwnd, &mut run)? {
        Step::Done => {}
        Step::Waiting(ids) => state_mut(hwnd).script_runs.suspend(run, ids),
    }
    Ok(())
}

/// Evaluates and runs `run`'s commands until it is done, or until a
/// command sent requests the core has not answered and another command
/// remains: that one waits, as an IRC server reads a client's lines in
/// order. A call stops at an empty `$$` identifier silently, and at a
/// wrong one or a failing command with its error; a server's refusal,
/// which comes as the core's answer, does not stop it.
///
/// What a command sent is every command ID handed out while it ran: the
/// UI thread is the only sender (every `CommandSender::send` runs on it),
/// so those are its requests, incidental ones too (a read receipt for the
/// window a `/join` activated), which it is right to wait for. Some may be
/// answered already: a command that opened a modal dialog handled core
/// events in its loop.
unsafe fn step(hwnd: HWND, run: &mut Run) -> Result<Step, String> {
    let sender = || {
        state_ref(hwnd)
            .command_sender
            .as_ref()
            .expect("scripts run with a core")
    };
    loop {
        let from = sender().next_id();
        let Some(flow) = run_next_command(hwnd, run)? else {
            return Ok(Step::Done);
        };
        if let Flow::Stop = flow {
            run.frames.clear();
        }
        let answers = &state_ref(hwnd).core_answers;
        let waiting = sender()
            .sent_since(from)
            .filter(|id| !answers.is_answered(*id))
            .collect::<Vec<_>>();
        if !waiting.is_empty() && run.has_more() {
            return Ok(Step::Waiting(waiting));
        }
    }
}

/// Evaluates `run`'s next command and runs it; `None` when there is none.
unsafe fn run_next_command(hwnd: HWND, run: &mut Run) -> Result<Option<Flow>, String> {
    let Some(command) = run.next_command() else {
        return Ok(None);
    };
    let evaluated = {
        let params = run
            .innermost()
            .params
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let context = CallContext {
            params: &params,
            chan: &run.window.chan,
            me: &run.window.me,
        };
        evaluate(&command, &context)
    };
    let flow = match evaluated {
        Ok(Evaluated::Command(text)) => run_command(hwnd, run, &text)?,
        Ok(Evaluated::Halt) => Flow::Stop,
        Err(reason) => {
            let acts_in = run.window.window(state_ref(hwnd));
            let error = run.innermost().caller.evaluation_error(&command, &reason);
            report_error_line(hwnd, &acts_in, error)?;
            Flow::Stop
        }
    };
    Ok(Some(flow))
}

/// Runs one evaluated command of `run`'s innermost frame, in the window
/// `run`'s window says. Its leading `/` is optional, and `//` the same as
/// `/`: the command is evaluated already. An alias call pushes the
/// alias's frame, whose commands run next.
unsafe fn run_command(hwnd: HWND, run: &mut Run, text: &str) -> Result<Flow, String> {
    // Every word was an empty parameter: there is nothing to run.
    if text.is_empty() {
        return Ok(Flow::Continue);
    }
    let line = format!("/{}", text.trim_start_matches('/'));
    let acts_in = run.window.window(state_ref(hwnd));
    let submission = {
        let aliases = state_ref(hwnd).aliases.aliases();
        classify_command(&line, |name| {
            aliases.get(name).is_some() && !run.is_running(name)
        })
    };
    let caller = &run.innermost().caller;
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
                .expect("classify_command names an alias");
            run.frames.push(Frame::alias(alias, args));
            Ok(Flow::Continue)
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
    use super::super::perform::PerformSetting;
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
    /// defaults. What the UI sends waits in `inert_command_receiver`
    /// until `ack_sent` answers it.
    unsafe fn with_alias_frame(extra: &[&str], test: impl FnOnce(HWND)) {
        with_test_frame(600, 400, |hwnd| {
            let (sender, commands) = inert_command_sender();
            let state = state_mut(hwnd);
            state.command_sender = Some(sender);
            state.inert_command_receiver = Some(commands);
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

    /// The IDs of the commands sent since the last call.
    unsafe fn take_sent_ids(hwnd: HWND) -> Vec<CommandId> {
        let receiver = state_mut(hwnd)
            .inert_command_receiver
            .as_mut()
            .expect("an alias frame keeps its receiver");
        std::iter::from_fn(|| receiver.try_recv().ok())
            .map(|(id, _)| id)
            .collect()
    }

    /// Applies the core's ack of each of `ids`.
    unsafe fn ack(hwnd: HWND, ids: &[CommandId]) {
        let acks = ids.iter().map(|id| Event::CommandHandled(*id)).collect();
        apply_events_and_refresh(hwnd, acks).unwrap();
    }

    /// Handles the posted `WM_SCRIPT_RESUME`s; whether there was one.
    unsafe fn resume_posted(hwnd: HWND) -> bool {
        let mut message = std::mem::zeroed::<MSG>();
        let mut resumed = false;
        while PeekMessageW(
            &mut message,
            hwnd,
            WM_SCRIPT_RESUME,
            WM_SCRIPT_RESUME,
            PM_REMOVE,
        ) != 0
        {
            resume_ready_run(hwnd).unwrap();
            resumed = true;
        }
        resumed
    }

    /// Answers every command sent, as the core does with no event of its
    /// own, and runs what waited for them, until nothing more is sent.
    unsafe fn ack_sent(hwnd: HWND) {
        loop {
            let ids = take_sent_ids(hwnd);
            if ids.is_empty() {
                assert!(!resume_posted(hwnd), "a resume without a new ack");
                return;
            }
            ack(hwnd, &ids);
            resume_posted(hwnd);
        }
    }

    /// Types `line`, and answers what it sends until it is done.
    unsafe fn type_answered(hwnd: HWND, line: &str) {
        type_compose_line(hwnd, line);
        ack_sent(hwnd);
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
                    type_answered(hwnd, "/x");
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
                    type_answered(hwnd, "/join #beta:example");
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
                    type_answered(hwnd, "/outer hi");
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
                type_answered(hwnd, "/greet Alice");
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

    #[test]
    fn perform_runs_each_line_in_status_and_goes_on_after_an_error() {
        unsafe {
            with_alias_frame(&[], |hwnd| {
                let lines = PerformSetting::parse_lines(
                    [
                        "say hi",
                        "join #new:example | echo -s $me done",
                        "msg nobody x | echo -s not reached",
                        "notify +@bob:example",
                    ]
                    .into_iter(),
                )
                .unwrap();
                let ops_before = ops_lines(hwnd);
                let status_before = status_log_lines(state_ref(hwnd)).len();
                let set = |enabled: bool| {
                    state_mut(hwnd)
                        .settings
                        .update(|settings| {
                            settings.perform = PerformSetting::new(enabled, lines.clone())
                        })
                        .unwrap()
                };

                set(false);
                run_perform(hwnd).unwrap();
                assert!(state_ref(hwnd).recorded_commands.is_empty());
                assert_eq!(status_log_lines(state_ref(hwnd)).len(), status_before);

                set(true);
                run_perform(hwnd).unwrap();
                ack_sent(hwnd);
                assert!(sent(state_ref(hwnd)).is_empty());
                assert_eq!(joins(state_ref(hwnd)), ["#new:example"]);
                let status = status_log_lines(state_ref(hwnd));
                let status = &status[status_before..];
                assert_eq!(
                    status,
                    [
                        "[22:12] * /say: no room selected (in perform)",
                        "@me:example done",
                        "[22:12] * /msg: no such nick: nobody (in perform)",
                        "[22:12] * Added +@bob:example to notify list",
                    ]
                );
                assert!(
                    state_ref(hwnd)
                        .settings
                        .settings()
                        .notify
                        .list
                        .contains("@bob:example")
                );
                assert_eq!(ops_lines(hwnd), ops_before);
                assert_eq!(state_ref(hwnd).active_view, room_view_id("!ops:example"));
            });
        }
    }

    fn commands_sent(state: &WindowState) -> Vec<&Command> {
        state.recorded_commands.iter().collect()
    }

    fn status_tail(hwnd: HWND, from: usize) -> Vec<String> {
        unsafe { status_log_lines(state_ref(hwnd))[from..].to_vec() }
    }

    /// The rooms of the alias frame, and `room` (a room just joined, or a
    /// direct room just made).
    unsafe fn rooms_with(hwnd: HWND, room: RoomSummary) -> Event {
        let mut rooms = state_ref(hwnd).model.rooms.clone();
        rooms.push(room);
        Event::SyncedRooms(rooms)
    }

    #[test]
    fn commands_of_a_perform_line_wait_for_the_core_to_answer_the_one_before() {
        unsafe {
            with_alias_frame(&[], |hwnd| {
                let lines = PerformSetting::parse_lines(
                    ["join #new:example | echo -s joined", "msg #new:example hi"].into_iter(),
                )
                .unwrap();
                state_mut(hwnd)
                    .settings
                    .update(|settings| settings.perform = PerformSetting::new(true, lines))
                    .unwrap();
                let status_before = status_log_lines(state_ref(hwnd)).len();

                run_perform(hwnd).unwrap();
                assert_eq!(joins(state_ref(hwnd)), ["#new:example"]);
                assert_eq!(commands_sent(state_ref(hwnd)).len(), 1);
                assert_eq!(status_tail(hwnd, status_before), Vec::<String>::new());
                assert!(!resume_posted(hwnd));

                let join = take_sent_ids(hwnd);
                // Synced: the SDK knows no state of the room yet, so no alias.
                apply_events_and_refresh(
                    hwnd,
                    vec![
                        rooms_with(hwnd, room("!new:example", "!new:example", false)),
                        Event::RoomJoined(mirx_core::RoomJoined {
                            room_id: "!new:example".to_owned(),
                            alias: Some("#new:example".to_owned()),
                        }),
                        Event::CommandHandled(join[0]),
                    ],
                )
                .unwrap();
                assert!(resume_posted(hwnd));
                assert_eq!(sent(state_ref(hwnd)), [text_to("!new:example", "hi")]);
                assert_eq!(
                    status_tail(hwnd, status_before),
                    ["joined", "[22:12] -> *#new:example* hi"]
                );
            });
        }
    }

    #[test]
    fn an_alias_greets_in_the_query_it_creates() {
        unsafe {
            with_alias_frame(&["/greet /q $$1 | say hi $1"], |hwnd| {
                state_mut(hwnd)
                    .model
                    .room_views
                    .get_mut("!ops:example")
                    .unwrap()
                    .members
                    .push(member_summary("@bob:example", "Bob", false));

                type_compose_line(hwnd, "/greet Bob");
                assert!(
                    matches!(
                        commands_sent(state_ref(hwnd))[..],
                        [Command::CreateDirectRoom(request)] if request.user_id == "@bob:example"
                    ),
                    "{:?}",
                    state_ref(hwnd).recorded_commands
                );
                assert!(sent(state_ref(hwnd)).is_empty());

                let create = take_sent_ids(hwnd);
                apply_events_and_refresh(
                    hwnd,
                    vec![
                        rooms_with(hwnd, query("!dm-bob:example", "Bob", "@bob:example")),
                        Event::RoomJoined(mirx_core::RoomJoined {
                            room_id: "!dm-bob:example".to_owned(),
                            alias: None,
                        }),
                        Event::CommandHandled(create[0]),
                    ],
                )
                .unwrap();
                assert!(resume_posted(hwnd));
                assert_eq!(state_ref(hwnd).active_view, room_view_id("!dm-bob:example"));
                assert_eq!(
                    sent(state_ref(hwnd)),
                    [text_to("!dm-bob:example", "hi Bob")]
                );
            });
        }
    }

    #[test]
    fn a_refused_request_does_not_stop_the_alias() {
        unsafe {
            with_alias_frame(&["/x join #nowhere:example | echo -s after"], |hwnd| {
                let status_before = status_log_lines(state_ref(hwnd)).len();
                type_compose_line(hwnd, "/x");
                let join = take_sent_ids(hwnd);
                apply_events_and_refresh(
                    hwnd,
                    vec![
                        Event::Error(CoreErrorEvent {
                            operation: Operation::JoinRoom,
                            room_id: None,
                            message: "no such room".to_owned(),
                        }),
                        Event::CommandHandled(join[0]),
                    ],
                )
                .unwrap();
                assert!(resume_posted(hwnd));
                assert_eq!(
                    status_tail(hwnd, status_before),
                    ["[22:12] * Error during join room: no such room", "after"]
                );
            });
        }
    }

    #[test]
    fn two_waiting_calls_resume_on_their_own_answers() {
        unsafe {
            with_alias_frame(
                &[
                    "/a join #one:example | echo -s one done",
                    "/b join #two:example | echo -s two done",
                ],
                |hwnd| {
                    let status_before = status_log_lines(state_ref(hwnd)).len();
                    type_compose_line(hwnd, "/a");
                    type_compose_line(hwnd, "/b");
                    let [one, two] = take_sent_ids(hwnd)[..] else {
                        panic!("one join each");
                    };
                    ack(hwnd, &[two]);
                    assert!(resume_posted(hwnd));
                    assert_eq!(status_tail(hwnd, status_before), ["two done"]);
                    ack(hwnd, &[one]);
                    assert!(resume_posted(hwnd));
                    assert_eq!(status_tail(hwnd, status_before), ["two done", "one done"]);
                },
            );
        }
    }
}
