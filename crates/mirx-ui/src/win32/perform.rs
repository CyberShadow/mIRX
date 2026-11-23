//! mIRC's Perform on connect (Options > Connect > Options > Perform...):
//! command lines run as if typed in Status each time mirx connects. A
//! line holds one or more commands separated by `|` words, with mIRC's
//! identifiers, as an alias's line does (`aliases.rs`); the `/` is
//! optional.
//!
//! Pure code: the setting and its lines. `mirx.ini` keeps them in
//! `[perform]`, `script::run_perform` runs them.

use super::aliases::split_commands;

/// mIRC's Perform on connect: whether it runs, and its lines as typed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PerformSetting {
    pub(super) enabled: bool,
    lines: Vec<PerformLine>,
}

/// One line as typed (what `mirx.ini` and the dialog show) and the
/// commands its `|` words split it into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PerformLine {
    text: String,
    commands: Vec<String>,
}

impl PerformLine {
    /// A line as typed, trimmed; refused as an alias's line would be.
    pub(super) fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let commands = split_commands(text.split_ascii_whitespace())?;
        if commands.is_empty() {
            return Err("has no commands".to_owned());
        }
        Ok(Self {
            text: text.to_owned(),
            commands,
        })
    }

    pub(super) fn text(&self) -> &str {
        &self.text
    }

    pub(super) fn commands(&self) -> &[String] {
        &self.commands
    }
}

impl PerformSetting {
    pub(super) fn new(enabled: bool, lines: Vec<PerformLine>) -> Self {
        Self { enabled, lines }
    }

    /// The Perform dialog's lines: blank ones dropped, each one trimmed.
    /// `Err((index, reason))` names the first wrong one by its index among
    /// all of `lines`, blank ones included, as the editbox counts them.
    pub(super) fn parse_lines<'a>(
        lines: impl Iterator<Item = &'a str>,
    ) -> Result<Vec<PerformLine>, (usize, String)> {
        lines
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(index, line)| PerformLine::parse(line).map_err(|reason| (index, reason)))
            .collect()
    }

    pub(super) fn lines(&self) -> &[PerformLine] {
        &self.lines
    }

    /// Adds a line after the others, as `mirx.ini`'s `n<i>=` keys come.
    pub(super) fn push(&mut self, line: PerformLine) {
        self.lines.push(line);
    }
}

#[cfg(test)]
mod tests {
    use super::super::aliases::NESTED_BLOCK;
    use super::*;

    fn texts(lines: &[PerformLine]) -> Vec<&str> {
        lines.iter().map(PerformLine::text).collect()
    }

    #[test]
    fn perform_lines_split_trim_and_refuse_like_aliases() {
        let lines = PerformSetting::parse_lines(
            [
                "",
                "  join #a:example | msg @b:x hi  ",
                "   ",
                "/notify +@c:x",
            ]
            .into_iter(),
        )
        .unwrap();
        assert_eq!(
            texts(&lines),
            ["join #a:example | msg @b:x hi", "/notify +@c:x"]
        );
        assert_eq!(lines[0].commands, ["join #a:example", "msg @b:x hi"]);
        assert_eq!(lines[1].commands, ["/notify +@c:x"]);

        assert_eq!(
            PerformSetting::parse_lines(["|"].into_iter()),
            Err((0, "has no commands".to_owned()))
        );
        // The index counts the blank lines: it is the editbox's line.
        assert_eq!(
            PerformSetting::parse_lines(["say a", "", "say {", "|"].into_iter()),
            Err((2, NESTED_BLOCK.to_owned()))
        );
        assert_eq!(
            PerformSetting::parse_lines(["", " "].into_iter()),
            Ok(vec![])
        );
    }
}
