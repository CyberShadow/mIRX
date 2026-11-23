//! mIRC's aliases: `aliases.ini`, the alias lines it holds, and the
//! evaluation of one alias command. Text macros, not scripting: no
//! variables, conditions or nested blocks (VISION).
//!
//! Pure code, no Win32 calls; `script.rs` runs the commands.

use std::fs;
use std::io;
use std::ops::Range;
use std::path::PathBuf;

use super::settings::write_atomically;

pub(super) const ALIASES_FILE_NAME: &str = "aliases.ini";

/// mIRC's default aliases where they map onto mirx's commands. mIRC's
/// `/op` is a built-in here, its `/dop` becomes `/deop`, `/kick` acts on
/// the active room without a `#`, and the DCC and CTCP ones have nothing to
/// call.
const DEFAULT_SOURCE: &[&str] = &[
    "/dop /deop $$1",
    "/j /join #$$1",
    "/p /part #",
    "/n /names #$$1",
    "/w /whois $$1",
    "/k /kick $$1 $2-",
    "/q /query $$1",
    "/s /server $$1",
];

pub(super) const NESTED_BLOCK: &str =
    "nested blocks need a scripting language, which mirx does not have";

/// The alias lines as the user wrote them (comments and blank lines too),
/// and the definitions they make. Only `Aliases::from_source` builds one,
/// so the two always agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Aliases {
    source: Vec<String>,
    /// In source order.
    definitions: Vec<Alias>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Alias {
    /// As written, without the `/`.
    pub(super) name: String,
    /// Split on ` | `, each with its words joined by single spaces;
    /// evaluated only when the alias runs.
    pub(super) commands: Vec<String>,
    /// Its source lines, which `/alias` replaces, removes and lists.
    lines: Range<usize>,
}

/// Why a source line makes no alias; `line` is 0-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceError {
    pub(super) line: usize,
    pub(super) reason: String,
}

impl Aliases {
    pub(super) fn from_source(source: Vec<String>) -> Result<Self, SourceError> {
        let mut definitions: Vec<Alias> = Vec::new();
        let mut index = 0;
        while index < source.len() {
            let at = |line: usize| move |reason: String| SourceError { line, reason };
            let line = source[index].trim();
            if is_ignored(line) {
                index += 1;
                continue;
            }
            let start = index;
            let (word, body) = split_word(line);
            if word == "}" {
                return Err(at(start)("} closes no block".to_owned()));
            }
            let name = word.strip_prefix('/').unwrap_or(word);
            check_name(name).map_err(at(start))?;
            if definitions
                .iter()
                .any(|alias| alias.name.eq_ignore_ascii_case(name))
            {
                return Err(at(start)(format!("/{name} is defined twice")));
            }
            let words = body.split_ascii_whitespace().collect::<Vec<_>>();
            let commands = match words.as_slice() {
                ["{"] => {
                    let mut commands = Vec::new();
                    loop {
                        index += 1;
                        let Some(line) = source.get(index).map(|line| line.trim()) else {
                            return Err(at(start)(format!("/{name}'s block has no }}")));
                        };
                        if line == "}" {
                            break;
                        }
                        if !is_ignored(line) {
                            commands.extend(
                                split_commands(line.split_ascii_whitespace()).map_err(at(index))?,
                            );
                        }
                    }
                    commands
                }
                ["{", inner @ .., "}"] => {
                    split_commands(inner.iter().copied()).map_err(at(start))?
                }
                _ => split_commands(words.into_iter()).map_err(at(start))?,
            };
            if commands.is_empty() {
                return Err(at(start)(format!("/{name} has no commands")));
            }
            index += 1;
            definitions.push(Alias {
                name: name.to_owned(),
                commands,
                lines: start..index,
            });
        }
        Ok(Self {
            source,
            definitions,
        })
    }

    pub(super) fn defaults() -> Self {
        Self::from_source(
            DEFAULT_SOURCE
                .iter()
                .map(|line| (*line).to_owned())
                .collect(),
        )
        .expect("the default aliases parse")
    }

    pub(super) fn source(&self) -> &[String] {
        &self.source
    }

    /// The alias `name` names, ignoring ASCII case as commands do.
    pub(super) fn get(&self, name: &str) -> Option<&Alias> {
        self.definitions
            .iter()
            .find(|alias| alias.name.eq_ignore_ascii_case(name))
    }

    /// `/alias name command`: the aliases with `name` defined as `command`,
    /// in the place of its old definition, or after the last line.
    pub(super) fn set(&self, name: &str, command: &str) -> Result<Self, String> {
        check_name(name)?;
        let line = format!("/{name} {command}");
        let mut source = self.source.clone();
        match self.get(name) {
            Some(alias) => {
                source.splice(alias.lines.clone(), [line]);
            }
            None => source.push(line),
        }
        Self::from_source(source).map_err(|error| error.reason)
    }

    /// `/alias name`: the aliases without `name`'s lines; the comments and
    /// blank lines around it stay.
    pub(super) fn remove(&self, name: &str) -> Result<Self, String> {
        let alias = self
            .get(name)
            .ok_or_else(|| format!("no such alias: /{name}"))?;
        let mut source = self.source.clone();
        source.drain(alias.lines.clone());
        Ok(Self::from_source(source).expect("removing an alias leaves valid aliases"))
    }

    /// Every alias's source lines, in order, as `/alias` lists them.
    pub(super) fn list_lines(&self) -> Vec<String> {
        self.definitions
            .iter()
            .flat_map(|alias| self.source[alias.lines.clone()].iter().cloned())
            .collect()
    }
}

/// A blank line or a `;` comment, which defines nothing.
fn is_ignored(line: &str) -> bool {
    line.is_empty() || line.starts_with(';')
}

fn split_word(text: &str) -> (&str, &str) {
    match text.split_once(|ch: char| ch.is_ascii_whitespace()) {
        Some((word, rest)) => (word, rest.trim()),
        None => (text, ""),
    }
}

/// An alias name is one word, and none of mIRC's syntax: `/`, `$`, `#`, `!`
/// (the built-in prefix), braces or `|`.
fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.contains(['/', '$', '#', '!', '{', '}', '|']) {
        return Err(format!("bad alias name: {name}"));
    }
    Ok(())
}

/// A line's words split into commands at each `|` word; a brace word is a
/// block inside the block.
pub(super) fn split_commands<'a>(
    words: impl Iterator<Item = &'a str>,
) -> Result<Vec<String>, String> {
    let mut commands = Vec::new();
    let mut command: Vec<&str> = Vec::new();
    for word in words {
        match word {
            "{" | "}" => return Err(NESTED_BLOCK.to_owned()),
            "|" => commands.push(std::mem::take(&mut command)),
            _ => command.push(word),
        }
    }
    commands.push(command);
    Ok(commands
        .into_iter()
        .filter(|command| !command.is_empty())
        .map(|command| command.join(" "))
        .collect())
}

/// The aliases from `aliases.ini`'s text: `[aliases]`, then `n0=`, `n1=`,
/// ... in order, one alias line each, as mIRC writes it.
pub(super) fn parse_ini(text: &str) -> Result<Aliases, String> {
    let mut header = false;
    let mut source = Vec::new();
    let mut file_lines = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let error = |reason: String| format!("line {}: {reason}", index + 1);
        if !header {
            match line.trim() {
                "" => continue,
                section if section.eq_ignore_ascii_case("[aliases]") => {
                    header = true;
                    continue;
                }
                _ => return Err(error("expected [aliases]".to_owned())),
            }
        }
        if line.trim().is_empty() {
            continue;
        }
        let expected = format!("n{}", source.len());
        let value = line
            .split_once('=')
            .filter(|(key, _)| key.trim().eq_ignore_ascii_case(&expected))
            .map(|(_, value)| value)
            .ok_or_else(|| error(format!("expected {expected}=")))?;
        source.push(value.to_owned());
        file_lines.push(index + 1);
    }
    if !header {
        return Err("line 1: expected [aliases]".to_owned());
    }
    Aliases::from_source(source).map_err(|SourceError { line, reason }| {
        format!("line {} (n{line}): {reason}", file_lines[line])
    })
}

pub(super) fn to_ini(aliases: &Aliases) -> String {
    let mut text = "[aliases]\r\n".to_owned();
    for (index, line) in aliases.source.iter().enumerate() {
        text.push_str(&format!("n{index}={line}\r\n"));
    }
    text
}

/// The aliases and the file they are saved in. Every change is saved at
/// once, as `SettingsStore` saves the settings.
pub(super) struct AliasStore {
    /// `None` keeps them in memory only: render-state runs never touch a
    /// profile.
    path: Option<PathBuf>,
    aliases: Aliases,
}

impl AliasStore {
    /// Reads `path`; a missing file means mIRC's default aliases, and is
    /// not created until something changes.
    pub(super) fn load(path: PathBuf) -> Result<Self, String> {
        let aliases = match fs::read_to_string(&path) {
            Ok(text) => parse_ini(&text).map_err(|err| format!("{} {err}", path.display()))?,
            Err(err) if err.kind() == io::ErrorKind::NotFound => Aliases::defaults(),
            Err(err) => return Err(format!("failed to read {}: {err}", path.display())),
        };
        Ok(Self {
            path: Some(path),
            aliases,
        })
    }

    pub(super) fn in_memory() -> Self {
        Self {
            path: None,
            aliases: Aliases::defaults(),
        }
    }

    pub(super) fn aliases(&self) -> &Aliases {
        &self.aliases
    }

    /// Saves `aliases`, then makes them the aliases; a failed save changes
    /// nothing.
    pub(super) fn replace(&mut self, aliases: Aliases) -> Result<(), String> {
        if let Some(path) = &self.path {
            write_atomically(path, &to_ini(&aliases))?;
        }
        self.aliases = aliases;
        Ok(())
    }
}

/// What an alias call fixes when it starts: its parameters, and the
/// window's `#` and `$me`.
#[derive(Debug, Clone, Copy)]
pub(super) struct CallContext<'a> {
    pub(super) params: &'a [&'a str],
    pub(super) chan: &'a str,
    pub(super) me: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Evaluated {
    Command(String),
    /// A `$$` identifier was empty: the whole call stops, silently.
    Halt,
}

/// A word that is an identifier.
#[derive(Debug, PartialEq, Eq)]
enum Identifier {
    /// `$N`, `$N-`, `$N-M`, and with `$$` (`required`).
    Param {
        from: usize,
        /// `None`: to the last.
        to: Option<usize>,
        required: bool,
    },
    /// `$0`.
    Count,
    /// `#`, `$chan`.
    Chan,
    /// `#$N`, `#$$N`: the parameter as a room alias.
    ChanPrefixed {
        n: usize,
        required: bool,
    },
    Me,
    /// `$+`.
    Join,
    /// `$letters`, which mirx does not have.
    Unknown(String),
}

/// `N`, `N-` or `N-M`, N at least 1.
fn param_range(text: &str) -> Option<(usize, Option<usize>)> {
    let number = |digits: &str| {
        (!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| digits.parse::<usize>().ok())
            .flatten()
    };
    let (from, to) = match text.split_once('-') {
        None => {
            let n = number(text)?;
            (n, Some(n))
        }
        Some((from, "")) => (number(from)?, None),
        Some((from, to)) => (number(from)?, Some(number(to)?)),
    };
    (from >= 1).then_some((from, to))
}

fn identifier(word: &str) -> Option<Identifier> {
    if word == "#" {
        return Some(Identifier::Chan);
    }
    if let Some(rest) = word.strip_prefix("#$") {
        let (required, digits) = match rest.strip_prefix('$') {
            Some(digits) => (true, digits),
            None => (false, rest),
        };
        return match param_range(digits)? {
            (n, Some(to)) if to == n => Some(Identifier::ChanPrefixed { n, required }),
            _ => None,
        };
    }
    let rest = word.strip_prefix('$')?;
    if let Some(range) = rest.strip_prefix('$') {
        let (from, to) = param_range(range)?;
        return Some(Identifier::Param {
            from,
            to,
            required: true,
        });
    }
    if rest == "0" {
        return Some(Identifier::Count);
    }
    if rest == "+" {
        return Some(Identifier::Join);
    }
    if let Some((from, to)) = param_range(rest) {
        return Some(Identifier::Param {
            from,
            to,
            required: false,
        });
    }
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    Some(match rest.to_ascii_lowercase().as_str() {
        "chan" => Identifier::Chan,
        "me" => Identifier::Me,
        _ => Identifier::Unknown(rest.to_owned()),
    })
}

/// Evaluates one alias command's identifiers. Each word that is wholly an
/// identifier is replaced, any other word kept as written; empty values
/// drop out (after `$+` joined them) and the rest are joined by single
/// spaces. A value is never
/// evaluated again.
pub(super) fn evaluate(command: &str, context: &CallContext) -> Result<Evaluated, String> {
    let params = context.params;
    let range = |from: usize, to: Option<usize>| {
        let end = to.unwrap_or(params.len()).min(params.len());
        params.get(from - 1..end).unwrap_or_default().join(" ")
    };
    let mut words: Vec<String> = Vec::new();
    // After `$+`: the next value goes on the last word.
    let mut join = false;
    for word in command.split_ascii_whitespace() {
        let (value, required) = match identifier(word) {
            None => (word.to_owned(), false),
            Some(Identifier::Param { from, to, required }) => (range(from, to), required),
            Some(Identifier::Count) => (params.len().to_string(), false),
            Some(Identifier::Chan) => (context.chan.to_owned(), false),
            Some(Identifier::ChanPrefixed { n, required }) => {
                let value = range(n, Some(n));
                let value = if value.is_empty() || value.starts_with(['#', '!']) {
                    value
                } else {
                    format!("#{value}")
                };
                (value, required)
            }
            Some(Identifier::Me) => (context.me.to_owned(), false),
            Some(Identifier::Join) => {
                join = true;
                continue;
            }
            Some(Identifier::Unknown(name)) => return Err(format!("no such identifier: ${name}")),
        };
        if value.is_empty() && required {
            return Ok(Evaluated::Halt);
        }
        match words.last_mut() {
            Some(last) if std::mem::take(&mut join) => last.push_str(&value),
            _ => {
                join = false;
                words.push(value);
            }
        }
    }
    words.retain(|word| !word.is_empty());
    Ok(Evaluated::Command(words.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_INI: &str = "[aliases]\r\nn0=/dop /deop $$1\r\nn1=/j /join #$$1\r\n\
        n2=/p /part #\r\nn3=/n /names #$$1\r\nn4=/w /whois $$1\r\nn5=/k /kick $$1 $2-\r\n\
        n6=/q /query $$1\r\nn7=/s /server $$1\r\n";

    fn source(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| (*line).to_owned()).collect()
    }

    fn aliases(lines: &[&str]) -> Aliases {
        Aliases::from_source(source(lines)).unwrap()
    }

    fn refusal(lines: &[&str]) -> (usize, String) {
        let error = Aliases::from_source(source(lines)).unwrap_err();
        (error.line, error.reason)
    }

    fn commands<'a>(aliases: &'a Aliases, name: &str) -> &'a [String] {
        &aliases.get(name).unwrap().commands
    }

    #[test]
    fn the_defaults_round_trip_through_the_exact_file_text() {
        assert_eq!(to_ini(&Aliases::defaults()), DEFAULT_INI);
        assert_eq!(parse_ini(DEFAULT_INI), Ok(Aliases::defaults()));
        assert_eq!(
            parse_ini(&DEFAULT_INI.replace("\r\n", "\n")),
            Ok(Aliases::defaults())
        );
        let defaults = Aliases::defaults();
        assert_eq!(defaults.get("J").unwrap().name, "j");
        assert_eq!(commands(&defaults, "j"), ["/join #$$1"]);
    }

    #[test]
    fn definitions_one_line_blocks_and_multi_line_blocks() {
        let parsed = aliases(&[
            "; mine",
            "/brb /away be right back | /amsg   brb",
            "",
            "hi say hello",
            "/two { say a | say b }",
            "/many {",
            "  say one",
            "  ; a comment",
            "",
            "  say two | say three",
            "}",
        ]);
        assert_eq!(
            commands(&parsed, "brb"),
            ["/away be right back", "/amsg brb"]
        );
        assert_eq!(commands(&parsed, "hi"), ["say hello"]);
        assert_eq!(commands(&parsed, "two"), ["say a", "say b"]);
        assert_eq!(
            commands(&parsed, "many"),
            ["say one", "say two", "say three"]
        );
        assert_eq!(parsed.get("mine"), None);
        assert_eq!(parsed.source().len(), 11);
    }

    #[test]
    fn malformed_lines_are_refused_with_their_line() {
        let nested = NESTED_BLOCK.to_owned();
        assert_eq!(
            refusal(&["/j /join #$$1", "", "/J /part"]),
            (2, "/J is defined twice".to_owned())
        );
        assert_eq!(refusal(&["/x"]), (0, "/x has no commands".to_owned()));
        assert_eq!(refusal(&["/x |"]), (0, "/x has no commands".to_owned()));
        assert_eq!(
            refusal(&["/x {", "}"]),
            (0, "/x has no commands".to_owned())
        );
        assert_eq!(
            refusal(&["; c", "/x {", "say a"]),
            (1, "/x's block has no }".to_owned())
        );
        assert_eq!(refusal(&["}"]), (0, "} closes no block".to_owned()));
        assert_eq!(
            refusal(&["/x {", "if (a) {", "}", "}"]),
            (1, nested.clone())
        );
        assert_eq!(refusal(&["/x { say { }"]), (0, nested.clone()));
        assert_eq!(refusal(&["/x say }"]), (0, nested));
        assert_eq!(refusal(&["$x say"]), (0, "bad alias name: $x".to_owned()));
        assert_eq!(refusal(&["/#x say"]), (0, "bad alias name: #x".to_owned()));
        assert_eq!(refusal(&["//x say"]), (0, "bad alias name: /x".to_owned()));
    }

    #[test]
    fn the_file_is_refused_with_its_line_and_key() {
        assert_eq!(
            parse_ini("n0=/j /join #$$1\r\n"),
            Err("line 1: expected [aliases]".to_owned())
        );
        assert_eq!(parse_ini(""), Err("line 1: expected [aliases]".to_owned()));
        assert_eq!(
            parse_ini("[ALIASES]\r\n").unwrap().source(),
            [] as [String; 0]
        );
        assert_eq!(
            parse_ini("[aliases]\r\nn0=/a say\r\nn2=/b say\r\n"),
            Err("line 3: expected n1=".to_owned())
        );
        assert_eq!(
            parse_ini("[aliases]\r\nalias=/a say\r\n"),
            Err("line 2: expected n0=".to_owned())
        );
        assert_eq!(
            parse_ini("[aliases]\r\n\r\nn0=/a say\r\nn1=/x\r\n"),
            Err("line 4 (n1): /x has no commands".to_owned())
        );
    }

    #[test]
    fn set_replaces_in_place_or_appends() {
        let parsed = aliases(&["/a {", "say a", "}", "; b next", "/b say b"]);
        let set = parsed.set("A", "say new").unwrap();
        assert_eq!(set.source(), ["/A say new", "; b next", "/b say b"]);
        let set = set.set("c", "{ say c | say d }").unwrap();
        assert_eq!(
            set.source(),
            ["/A say new", "; b next", "/b say b", "/c { say c | say d }"]
        );
        assert_eq!(commands(&set, "c"), ["say c", "say d"]);
        assert_eq!(
            parsed.set("$x", "say"),
            Err("bad alias name: $x".to_owned())
        );
        assert_eq!(parsed.set("x", "{ say"), Err(NESTED_BLOCK.to_owned()));
    }

    #[test]
    fn remove_keeps_the_lines_around_the_alias() {
        let parsed = aliases(&["; a", "/a {", "say a", "}", "", "/b say b"]);
        let removed = parsed.remove("A").unwrap();
        assert_eq!(removed.source(), ["; a", "", "/b say b"]);
        assert_eq!(removed.get("a"), None);
        assert_eq!(parsed.remove("x"), Err("no such alias: /x".to_owned()));
    }

    #[test]
    fn list_lines_are_every_aliases_source_lines() {
        let parsed = aliases(&["; a", "/a {", "  say a", "}", "", "/b say b"]);
        assert_eq!(parsed.list_lines(), ["/a {", "  say a", "}", "/b say b"]);
        assert_eq!(aliases(&["; none"]).list_lines(), [] as [String; 0]);
    }

    #[test]
    fn identifiers_are_whole_words() {
        use Identifier::*;
        let param = |from, to, required| Param { from, to, required };
        assert_eq!(identifier("$1"), Some(param(1, Some(1), false)));
        assert_eq!(identifier("$2-"), Some(param(2, None, false)));
        assert_eq!(identifier("$2-3"), Some(param(2, Some(3), false)));
        assert_eq!(identifier("$$1"), Some(param(1, Some(1), true)));
        assert_eq!(identifier("$$2-"), Some(param(2, None, true)));
        assert_eq!(identifier("$0"), Some(Count));
        assert_eq!(identifier("#"), Some(Chan));
        assert_eq!(identifier("$chan"), Some(Chan));
        assert_eq!(identifier("$ME"), Some(Me));
        assert_eq!(identifier("$+"), Some(Join));
        assert_eq!(
            identifier("#$1"),
            Some(ChanPrefixed {
                n: 1,
                required: false
            })
        );
        assert_eq!(
            identifier("#$$1"),
            Some(ChanPrefixed {
                n: 1,
                required: true
            })
        );
        assert_eq!(identifier("$foo"), Some(Unknown("foo".to_owned())));
        for word in [
            "$", "$$", "$5.00", "a$1", "##", "#x", "$$0", "$-1", "$1-x", "$foo1", "#$1-", "hi",
        ] {
            assert_eq!(identifier(word), None, "{word}");
        }
    }

    fn eval(command: &str, params: &[&str], chan: &str) -> Evaluated {
        evaluate(
            command,
            &CallContext {
                params,
                chan,
                me: "Me",
            },
        )
        .unwrap()
    }

    fn command(text: &str) -> Evaluated {
        Evaluated::Command(text.to_owned())
    }

    #[test]
    fn evaluate_replaces_parameters_and_the_window() {
        let abc = ["a", "b", "c"];
        assert_eq!(
            eval("say $1 $2- $2-3 $0", &abc, ""),
            command("say a b c b c 3")
        );
        assert_eq!(eval("say $3-9 $4 $4- x", &abc, ""), command("say c x"));
        assert_eq!(eval("say $2-1 x", &abc, ""), command("say x"));
        assert_eq!(eval("say $$1", &abc, ""), command("say a"));
        assert_eq!(eval("say $$1", &[], ""), Evaluated::Halt);
        assert_eq!(eval("say $$2-", &["a"], ""), Evaluated::Halt);
        assert_eq!(eval("part #", &[], "#ops:x"), command("part #ops:x"));
        assert_eq!(eval("part # $chan", &[], ""), command("part"));
        assert_eq!(eval("join #$$1", &["ops:x"], ""), command("join #ops:x"));
        assert_eq!(eval("join #$$1", &["#ops:x"], ""), command("join #ops:x"));
        assert_eq!(eval("join #$$1", &["!id:x"], ""), command("join !id:x"));
        assert_eq!(eval("join #$$1", &[], ""), Evaluated::Halt);
        assert_eq!(eval("join #$1 x", &[], ""), command("join x"));
        assert_eq!(eval("say I am $me", &[], ""), command("say I am Me"));
        assert_eq!(eval("say $1 $+ !", &["bob"], ""), command("say bob!"));
        assert_eq!(eval("$+ say x $+", &[], ""), command("say x"));
        assert_eq!(eval("say $2 $+ !", &["a"], ""), command("say !"));
        assert_eq!(
            eval("say $5.00 costs$1", &["a"], ""),
            command("say $5.00 costs$1")
        );
        assert_eq!(eval("say   a    b", &[], ""), command("say a b"));
        // A value is not split or evaluated again.
        assert_eq!(
            eval("say $1-", &["x", "|", "$me"], ""),
            command("say x | $me")
        );
        assert_eq!(
            evaluate(
                "say $active",
                &CallContext {
                    params: &[],
                    chan: "",
                    me: ""
                }
            ),
            Err("no such identifier: $active".to_owned())
        );
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mirx-aliases-{name}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_missing_file_means_the_defaults_and_is_not_created() {
        let dir = temp_dir("missing");
        let path = dir.join(ALIASES_FILE_NAME);
        let store = AliasStore::load(path.clone()).unwrap();
        assert_eq!(store.aliases(), &Aliases::defaults());
        assert!(!path.exists());
    }

    #[test]
    fn replace_saves_and_loads_back() {
        let dir = temp_dir("replace");
        let path = dir.join(ALIASES_FILE_NAME);
        let mut store = AliasStore::load(path.clone()).unwrap();
        let changed = store.aliases().set("hi", "say hello $1").unwrap();
        store.replace(changed.clone()).unwrap();
        assert_eq!(store.aliases(), &changed);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{DEFAULT_INI}n8=/hi say hello $1\r\n")
        );
        assert_eq!(AliasStore::load(path).unwrap().aliases(), &changed);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn in_memory_aliases_are_never_written() {
        let mut store = AliasStore::in_memory();
        let changed = store.aliases().remove("j").unwrap();
        store.replace(changed.clone()).unwrap();
        assert_eq!(store.aliases(), &changed);
    }

    #[test]
    fn a_malformed_file_fails_to_load_and_is_left_alone() {
        let dir = temp_dir("malformed");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(ALIASES_FILE_NAME);
        fs::write(&path, "[aliases]\r\nn0=/x\r\n").unwrap();
        let error = AliasStore::load(path.clone()).err().unwrap();
        assert!(
            error.ends_with("aliases.ini line 2 (n0): /x has no commands"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "[aliases]\r\nn0=/x\r\n");
        let _ = fs::remove_dir_all(dir);
    }
}
