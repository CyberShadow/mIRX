//! mIRC's nick colours (Address Book > Colors, `/cnick`): an ordered list
//! of entries, each a user pattern with a colour and the conditions it
//! applies under. The first entry that matches a user gives their nick's
//! colour, in the nick list and in the lines they send.
//!
//! mIRC matches `nick!user@host`; mirx matches the Matrix user ID, which a
//! user cannot change or copy as a display name can. An entry's modes are
//! the member's rank in the window's room (`~ & @ +`), and `notify` asks
//! for a user on the notify list. Auto colour picks a stable colour per
//! user among the palette colours that read on the window's background.
//!
//! Pure code: the list, matching, the auto colours, the `mirx.ini` values
//! and the lines `/cnick` prints.

use std::collections::{BTreeSet, HashMap};

use mirx_core::{RoomMemberRank, RoomMemberSummary, TextColor};

use super::colors::{ColorItem, ColorScheme, PaletteIndex};
use super::commands::is_user_id;
use super::ignore::user_label;
use super::settings::Settings;
use super::timeline_text::rank_prefix;

/// Whom an entry names: a user ID, or a pattern over user IDs where `*`
/// stands for any run of characters and `?` for one, as mIRC's masks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UserPattern(String);

impl UserPattern {
    pub(super) fn new(text: &str) -> Result<Self, String> {
        let pattern = Self(text.to_owned());
        if text.chars().any(char::is_whitespace) || !(pattern.is_wildcard() || is_user_id(text)) {
            return Err(format!("{text} is not a user ID or a pattern with * or ?"));
        }
        Ok(pattern)
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }

    pub(super) fn is_wildcard(&self) -> bool {
        self.0.contains(['*', '?'])
    }

    /// Whether the whole of `user_id` fits, ignoring ASCII case.
    pub(super) fn matches(&self, user_id: &str) -> bool {
        let pattern = self.0.chars().collect::<Vec<_>>();
        let text = user_id.chars().collect::<Vec<_>>();
        let (mut p, mut t) = (0, 0);
        // The last `*` and where the text stood when it was taken: on a
        // mismatch it takes one character more.
        let mut star = None;
        while t < text.len() {
            match pattern.get(p) {
                Some('*') => {
                    star = Some((p, t));
                    p += 1;
                }
                Some(&c) if c == '?' || c.eq_ignore_ascii_case(&text[t]) => {
                    p += 1;
                    t += 1;
                }
                _ => match star {
                    Some((star_p, star_t)) => {
                        p = star_p + 1;
                        t = star_t + 1;
                        star = Some((star_p, star_t + 1));
                    }
                    None => return false,
                },
            }
        }
        pattern[p..].iter().all(|&c| c == '*')
    }

    /// Whom it names: `Bob (@bob:example)` for a user ID, the pattern as
    /// typed otherwise.
    pub(super) fn label(&self, nick_of: &impl Fn(&str) -> Option<String>) -> String {
        if self.is_wildcard() {
            self.0.clone()
        } else {
            user_label(&self.0, nick_of)
        }
    }

    /// The same pattern, as `/cnick` finds an entry to update or remove.
    pub(super) fn same(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

/// An entry's mode condition: the member's rank in the window's room.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ModeCondition {
    /// mIRC's Any mode: ranked above Member.
    AnyMode,
    /// mIRC's No mode: an ordinary member.
    NoMode,
    /// One of these ranks (a Matrix member has one), never Member, never
    /// none: `@&~` is the room's staff.
    Ranks(BTreeSet<RoomMemberRank>),
}

impl ModeCondition {
    /// Rank glyphs as typed: `@`, `~&@`, ...
    pub(super) fn parse_glyphs(text: &str) -> Result<Self, String> {
        let invalid = || format!("modes must be some of ~&@+, not {text}");
        let ranks = text
            .chars()
            .map(|glyph| {
                RANKED
                    .into_iter()
                    .find(|rank| rank_prefix(*rank).starts_with(glyph))
                    .ok_or_else(invalid)
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        if ranks.is_empty() {
            return Err(invalid());
        }
        Ok(Self::Ranks(ranks))
    }

    /// The condition `/cnick` and the Address Book's entry take: mode
    /// glyphs (empty for none), Any mode (`-a`) or No mode (`-n`), one at
    /// most; none of them is no condition.
    pub(super) fn from_parts(glyphs: &str, any: bool, none: bool) -> Result<Option<Self>, String> {
        match (glyphs.is_empty(), any, none) {
            (true, false, false) => Ok(None),
            (true, true, false) => Ok(Some(Self::AnyMode)),
            (true, false, true) => Ok(Some(Self::NoMode)),
            (_, true, true) => Err("any mode and no mode cannot be combined".to_owned()),
            (false, false, false) => Self::parse_glyphs(glyphs).map(Some),
            (false, _, _) => Err("modes cannot be given with any mode or no mode".to_owned()),
        }
    }

    fn matches(&self, rank: RoomMemberRank) -> bool {
        match self {
            Self::AnyMode => rank != RoomMemberRank::Member,
            Self::NoMode => rank == RoomMemberRank::Member,
            Self::Ranks(ranks) => ranks.contains(&rank),
        }
    }
}

/// The ranks that have a glyph, highest first.
const RANKED: [RoomMemberRank; 4] = [
    RoomMemberRank::Creator,
    RoomMemberRank::Administrator,
    RoomMemberRank::Moderator,
    RoomMemberRank::Voiced,
];

/// `~&@+` order, highest first.
pub(super) fn glyphs(ranks: &BTreeSet<RoomMemberRank>) -> String {
    ranks.iter().map(|rank| rank_prefix(*rank)).collect()
}

/// An entry's colour: a palette colour, or one picked per user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NickColor {
    Palette(PaletteIndex),
    Auto,
}

impl NickColor {
    /// `0`-`15`, or `*` (mIRC's) or `auto` for auto colour.
    pub(super) fn parse(text: &str) -> Result<Self, String> {
        if text == "*" || text.eq_ignore_ascii_case("auto") {
            return Ok(Self::Auto);
        }
        text.parse::<u8>()
            .map_err(|_| "colors must be 0-15")
            .and_then(PaletteIndex::new)
            .map(Self::Palette)
            .map_err(|_| format!("color must be 0-15 or *, not {text}"))
    }

    /// As `mirx.ini`, the listing and the Address Book show it.
    pub(super) fn text(self) -> String {
        match self {
            Self::Palette(index) => index.get().to_string(),
            Self::Auto => "auto".to_owned(),
        }
    }
}

/// One entry of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NickColorEntry {
    pub(super) pattern: UserPattern,
    pub(super) color: NickColor,
    pub(super) modes: Option<ModeCondition>,
    /// Only for users on the notify list.
    pub(super) notify: bool,
}

impl NickColorEntry {
    /// mIRC's Apply items are cumulative: the pattern, the modes and the
    /// notify list must all fit. A user who is not a member of the
    /// window's room (`rank` `None`) has no mode.
    fn matches(&self, user_id: &str, rank: Option<RoomMemberRank>, on_notify: bool) -> bool {
        self.pattern.matches(user_id)
            && self
                .modes
                .as_ref()
                .is_none_or(|modes| rank.is_some_and(|rank| modes.matches(rank)))
            && (!self.notify || on_notify)
    }

    pub(super) fn label(&self, nick_of: &impl Fn(&str) -> Option<String>) -> String {
        self.pattern.label(nick_of)
    }

    /// Its Apply items besides the pattern, as the Address Book's Apply
    /// column shows them: `@&~`, `any mode`, `no mode`, `notify`.
    pub(super) fn apply_text(&self) -> String {
        self.apply_items(false).join(", ")
    }

    fn apply_items(&self, modes_word: bool) -> Vec<String> {
        let modes = self.modes.as_ref().map(|modes| match modes {
            ModeCondition::AnyMode => "any mode".to_owned(),
            ModeCondition::NoMode => "no mode".to_owned(),
            ModeCondition::Ranks(ranks) if modes_word => format!("modes {}", glyphs(ranks)),
            ModeCondition::Ranks(ranks) => glyphs(ranks),
        });
        modes
            .into_iter()
            .chain(self.notify.then(|| "notify".to_owned()))
            .collect()
    }

    /// `mirx.ini`'s value: `<pattern> <color> [modes=<glyphs>|any|none]
    /// [notify]`.
    pub(super) fn ini_value(&self) -> String {
        let mut value = format!("{} {}", self.pattern.as_str(), self.color.text());
        match &self.modes {
            Some(ModeCondition::AnyMode) => value.push_str(" any"),
            Some(ModeCondition::NoMode) => value.push_str(" none"),
            Some(ModeCondition::Ranks(ranks)) => {
                value.push_str(&format!(" modes={}", glyphs(ranks)))
            }
            None => {}
        }
        if self.notify {
            value.push_str(" notify");
        }
        value
    }

    pub(super) fn parse_ini_value(value: &str) -> Result<Self, String> {
        let mut words = value.split_whitespace();
        let (Some(pattern), Some(color)) = (words.next(), words.next()) else {
            return Err("expected <pattern> <color> [modes=~&@+|any|none] [notify]".to_owned());
        };
        let mut entry = Self {
            pattern: UserPattern::new(pattern)?,
            color: NickColor::parse(color)?,
            modes: None,
            notify: false,
        };
        for word in words {
            let modes = match word {
                "notify" if !entry.notify => {
                    entry.notify = true;
                    continue;
                }
                "any" => ModeCondition::AnyMode,
                "none" => ModeCondition::NoMode,
                _ => match word.strip_prefix("modes=") {
                    Some(glyphs) => ModeCondition::parse_glyphs(glyphs)?,
                    None => return Err(format!("unexpected {word}")),
                },
            };
            if entry.modes.replace(modes).is_some() {
                return Err("one of modes=, any and none at most".to_owned());
            }
        }
        Ok(entry)
    }
}

/// The list, in the user's order: the first match wins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct NickColorList(Vec<NickColorEntry>);

/// What `add` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Added {
    New,
    /// The first entry with the same pattern took the new one's place.
    Updated,
}

impl NickColorList {
    pub(super) fn entries(&self) -> &[NickColorEntry] {
        &self.0
    }

    /// `/cnick`: replaces the first entry with the same pattern, unless
    /// `force` (mIRC's `-f`), which appends a new one.
    pub(super) fn add(&mut self, entry: NickColorEntry, force: bool) -> Added {
        match self.find(&entry.pattern).filter(|_| !force) {
            Some(index) => {
                self.0[index] = entry;
                Added::Updated
            }
            None => {
                self.0.push(entry);
                Added::New
            }
        }
    }

    /// The first entry with `pattern`.
    pub(super) fn find(&self, pattern: &UserPattern) -> Option<usize> {
        self.0.iter().position(|entry| entry.pattern.same(pattern))
    }

    pub(super) fn push(&mut self, entry: NickColorEntry) {
        self.0.push(entry);
    }

    pub(super) fn set(&mut self, index: usize, entry: NickColorEntry) {
        self.0[index] = entry;
    }

    pub(super) fn remove(&mut self, index: usize) -> NickColorEntry {
        self.0.remove(index)
    }

    /// Moves the entry at `index`, not the first, one place up.
    pub(super) fn move_up(&mut self, index: usize) {
        assert!(
            index > 0 && index < self.0.len(),
            "an entry moves up from below the first"
        );
        self.0.swap(index - 1, index);
    }
}

/// `mirx.ini`'s `[nickcolors]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NickColorSetting {
    /// Off (`/cnick off`, Enable unchecked), nothing is coloured; the list
    /// is kept.
    pub(super) active: bool,
    pub(super) list: NickColorList,
}

impl Default for NickColorSetting {
    fn default() -> Self {
        Self {
            active: true,
            list: NickColorList::default(),
        }
    }
}

impl NickColorSetting {
    /// Whether any nick can take a colour.
    pub(super) fn colors_nicks(&self) -> bool {
        self.active && !self.list.entries().is_empty()
    }

    /// The colour of the first entry that fits the user: `rank` is theirs
    /// in the window's room (`None`: not a member), `on_notify` whether
    /// the notify list has them.
    pub(super) fn resolve(
        &self,
        user_id: &str,
        rank: Option<RoomMemberRank>,
        on_notify: bool,
    ) -> Option<ResolvedNickColor> {
        if !self.active {
            return None;
        }
        let entry = self
            .list
            .entries()
            .iter()
            .find(|entry| entry.matches(user_id, rank, on_notify))?;
        Some(match entry.color {
            NickColor::Palette(index) => ResolvedNickColor::Palette(index),
            NickColor::Auto => ResolvedNickColor::Auto(fnv1a32(user_id)),
        })
    }
}

/// The nick colours of one window's lines: its room's members' ranks,
/// looked up once.
pub(super) struct NickColorer<'a> {
    settings: &'a Settings,
    /// Empty while no nick takes a colour.
    ranks: HashMap<&'a str, RoomMemberRank>,
}

impl<'a> NickColorer<'a> {
    pub(super) fn new(settings: &'a Settings, members: &'a [RoomMemberSummary]) -> Self {
        let ranks = if settings.nick_colors.colors_nicks() {
            members
                .iter()
                .map(|member| (member.user_id.as_str(), member.rank))
                .collect()
        } else {
            HashMap::new()
        };
        Self { settings, ranks }
    }

    pub(super) fn color(&self, user_id: &str) -> Option<ResolvedNickColor> {
        self.settings.nick_colors.resolve(
            user_id,
            self.ranks.get(user_id).copied(),
            self.settings.notify.list.contains(user_id),
        )
    }
}

/// A user's nick colour. An auto colour keeps its user's hash, and picks
/// from the colours that read on the scheme's background when drawn, so a
/// scheme change needs nothing rebuilt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResolvedNickColor {
    Palette(PaletteIndex),
    Auto(u32),
}

impl ResolvedNickColor {
    pub(super) fn index(self, scheme: &ColorScheme) -> PaletteIndex {
        match self {
            Self::Palette(index) => index,
            Self::Auto(hash) => {
                let candidates = auto_candidates(scheme);
                candidates[hash as usize % candidates.len()]
            }
        }
    }
}

/// The colours auto colour picks from: the 16 but the scheme's Background
/// and Normal text and the two greys (grey is away in the nick list),
/// those with a WCAG contrast of at least 3 against the background.
///
/// Never empty: black or white always has a contrast above 4.5 with any
/// colour, and when one is the background and the other the normal text,
/// navy reads on white and yellow on black.
pub(super) fn auto_candidates(scheme: &ColorScheme) -> Vec<PaletteIndex> {
    let background = scheme.get(ColorItem::Background);
    let normal = scheme.get(ColorItem::Normal);
    let candidates = PaletteIndex::all()
        .filter(|index| {
            *index != background
                && *index != normal
                && index.get() < 14
                && contrast(index.color(), background.color()) >= 3.0
        })
        .collect::<Vec<_>>();
    assert!(
        !candidates.is_empty(),
        "every scheme has colours that read on its background"
    );
    candidates
}

/// WCAG 2's contrast ratio of two colours, 1 to 21.
fn contrast(a: TextColor, b: TextColor) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// WCAG 2's relative luminance of an sRGB colour.
fn luminance(color: TextColor) -> f64 {
    let channel = |value: u8| {
        let value = f64::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.red) + 0.7152 * channel(color.green) + 0.0722 * channel(color.blue)
}

/// 32-bit FNV-1a: an auto colour's hash of the user ID, the same in every
/// window and run, whatever the nick.
pub(super) fn fnv1a32(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// `* Nick colors are on`.
pub(super) fn state_line(active: bool) -> String {
    format!("* Nick colors are {}", if active { "on" } else { "off" })
}

/// `/cnick` alone: the list in order, numbered as `/cnick -r N` takes it.
pub(super) fn list_lines(
    setting: &NickColorSetting,
    nick_of: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let state = if setting.active { "on" } else { "off" };
    if setting.list.entries().is_empty() {
        return vec![format!("* Nick color list is empty ({state})")];
    }
    std::iter::once(format!("* Nick colors are {state}:"))
        .chain(
            setting
                .list
                .entries()
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    let apply = entry
                        .apply_items(true)
                        .into_iter()
                        .map(|item| format!(", {item}"))
                        .collect::<String>();
                    format!(
                        "*   {}. {} {}{apply}",
                        index + 1,
                        entry.label(&nick_of),
                        entry.color.text()
                    )
                }),
        )
        .collect()
}

/// What `/cnick <nick> <color>` says it did.
pub(super) fn added_line(
    added: Added,
    entry: &NickColorEntry,
    nick_of: impl Fn(&str) -> Option<String>,
) -> String {
    let label = entry.label(&nick_of);
    match added {
        Added::New => format!("* Added {label} to nick color list"),
        Added::Updated => format!("* Updated {label} in nick color list"),
    }
}

pub(super) fn removed_line(
    entry: &NickColorEntry,
    nick_of: impl Fn(&str) -> Option<String>,
) -> String {
    format!("* Removed {} from nick color list", entry.label(&nick_of))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(text: &str) -> UserPattern {
        UserPattern::new(text).expect("test patterns are valid")
    }

    fn palette(index: u8) -> NickColor {
        NickColor::Palette(PaletteIndex::new(index).expect("test colours are 0-15"))
    }

    fn entry(text: &str, color: NickColor) -> NickColorEntry {
        NickColorEntry {
            pattern: pattern(text),
            color,
            modes: None,
            notify: false,
        }
    }

    fn nick_of(user_id: &str) -> Option<String> {
        (user_id == "@bob:example").then(|| "Bob".to_owned())
    }

    #[test]
    fn patterns_match_whole_user_ids_with_wildcards_ignoring_case() {
        let fits = |pattern_text: &str, user_id: &str| pattern(pattern_text).matches(user_id);
        assert!(fits("@bob:example", "@bob:example"));
        assert!(fits("@Bob:Example", "@bob:EXAMPLE"));
        assert!(!fits("@bob:example", "@bob:example.org"));
        assert!(!fits("@bob:example", "@bobby:example"));
        assert!(fits("*", "@bob:example"));
        assert!(fits("@*:matrix.org", "@alice:matrix.org"));
        assert!(!fits("@*:matrix.org", "@alice:matrix.org.evil"));
        assert!(fits("*bot*", "@helpbot:example"));
        assert!(fits("*bot*", "@bot:example"));
        assert!(!fits("*bot*", "@bob:example"));
        assert!(fits("@b?b:example", "@bob:example"));
        assert!(!fits("@b?b:example", "@bb:example"));
        assert!(fits("@a*b*c:x", "@aXbYbZc:x"));
        assert!(!fits("@a*b*c:x", "@aXbYbZ:x"));
        assert!(fits("@**:x", "@:x"));
        // Multibyte IDs match by character, without panics.
        assert!(fits("@?ö*:x", "@äöü:x"));
        assert!(!fits("@ä:x", "@Ä:x"));
        assert!(fits("*ü*", "@äöü:x"));
        assert!(!fits("?", ""));
    }

    #[test]
    fn a_pattern_is_a_user_id_or_has_a_wildcard() {
        assert!(pattern("*").is_wildcard());
        assert!(!pattern("@bob:example").is_wildcard());
        for bad in ["bob", "", "@bob", "@bob: x", "* *"] {
            assert_eq!(
                UserPattern::new(bad),
                Err(format!("{bad} is not a user ID or a pattern with * or ?")),
                "{bad}"
            );
        }
        assert!(pattern("@Bob:example").same(&pattern("@bob:EXAMPLE")));
        assert!(!pattern("@bob:example").same(&pattern("@bob:example.org")));
    }

    #[test]
    fn modes_are_rank_glyphs() {
        let ranks = |glyphs: &str| match ModeCondition::parse_glyphs(glyphs) {
            Ok(ModeCondition::Ranks(ranks)) => ranks.into_iter().collect::<Vec<_>>(),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            ranks("@&~"),
            [
                RoomMemberRank::Creator,
                RoomMemberRank::Administrator,
                RoomMemberRank::Moderator
            ]
        );
        assert_eq!(ranks("++"), [RoomMemberRank::Voiced]);
        for bad in ["", "%", "@x"] {
            assert_eq!(
                ModeCondition::parse_glyphs(bad),
                Err(format!("modes must be some of ~&@+, not {bad}"))
            );
        }
    }

    #[test]
    fn the_first_entry_that_fits_gives_the_colour() {
        let mut setting = NickColorSetting::default();
        assert!(!setting.colors_nicks());
        assert_eq!(setting.resolve("@bob:example", None, false), None);
        let mut ops = entry("*", palette(4));
        ops.modes = Some(ModeCondition::parse_glyphs("@&~").unwrap());
        let mut voiced_bots = entry("*bot*", palette(12));
        voiced_bots.modes = Some(ModeCondition::parse_glyphs("+").unwrap());
        let mut buddies = entry("*", palette(3));
        buddies.notify = true;
        let mut members = entry("*", palette(6));
        members.modes = Some(ModeCondition::NoMode);
        let mut ranked = entry("*", palette(10));
        ranked.modes = Some(ModeCondition::AnyMode);
        for entry in [ops, voiced_bots, buddies, members, ranked] {
            setting.list.push(entry);
        }
        assert!(setting.colors_nicks());
        let color = |user_id: &str, rank, on_notify| {
            setting
                .resolve(user_id, rank, on_notify)
                .map(|color| color.index(&ColorScheme::default()).get())
        };
        use RoomMemberRank::*;
        assert_eq!(color("@bob:example", Some(Moderator), true), Some(4));
        assert_eq!(color("@bob:example", Some(Creator), false), Some(4));
        // Cumulative: a bot needs voice too.
        assert_eq!(color("@bot:example", Some(Voiced), false), Some(12));
        assert_eq!(color("@bob:example", Some(Voiced), false), Some(10));
        assert_eq!(color("@bot:example", Some(Member), true), Some(3));
        assert_eq!(color("@bot:example", Some(Member), false), Some(6));
        // A non-member has no mode: only entries without one fit.
        assert_eq!(color("@gone:example", None, true), Some(3));
        assert_eq!(color("@gone:example", None, false), None);
        setting.active = false;
        assert!(!setting.colors_nicks());
        assert_eq!(setting.resolve("@bob:example", Some(Moderator), true), None);
    }

    #[test]
    fn auto_colour_is_stable_per_user_and_reads_on_the_background() {
        let mut setting = NickColorSetting::default();
        setting.list.push(entry("*", NickColor::Auto));
        let scheme = ColorScheme::default();
        let bob = setting.resolve("@bob:example", None, false).unwrap();
        assert_eq!(bob, ResolvedNickColor::Auto(fnv1a32("@bob:example")));
        let candidates = auto_candidates(&scheme);
        assert_eq!(
            bob.index(&scheme),
            candidates[fnv1a32("@bob:example") as usize % candidates.len()]
        );
        assert_eq!(fnv1a32(""), 0x811c_9dc5);
        assert_eq!(fnv1a32("@bob:example"), 0xacae_00d2);
    }

    #[test]
    fn auto_candidates_follow_the_scheme() {
        let indices = |scheme: &ColorScheme| {
            auto_candidates(scheme)
                .into_iter()
                .map(PaletteIndex::get)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            indices(&ColorScheme::default()),
            [2, 3, 4, 5, 6, 10, 12, 13]
        );
        let mut dark = ColorScheme::default();
        dark.set(ColorItem::Background, PaletteIndex::new(1).unwrap());
        dark.set(ColorItem::Normal, PaletteIndex::new(0).unwrap());
        assert_eq!(indices(&dark), [3, 4, 7, 8, 9, 10, 11, 13]);
        // Every background with every normal text has some.
        for background in PaletteIndex::all() {
            for normal in PaletteIndex::all() {
                let mut scheme = ColorScheme::default();
                scheme.set(ColorItem::Background, background);
                scheme.set(ColorItem::Normal, normal);
                assert!(!auto_candidates(&scheme).is_empty());
            }
        }
    }

    #[test]
    fn add_updates_the_first_entry_with_the_pattern_unless_forced() {
        let mut list = NickColorList::default();
        assert_eq!(
            list.add(entry("@bob:example", palette(4)), false),
            Added::New
        );
        assert_eq!(list.add(entry("*", NickColor::Auto), false), Added::New);
        assert_eq!(
            list.add(entry("@BOB:example", palette(12)), false),
            Added::Updated
        );
        assert_eq!(
            list.add(entry("@bob:example", palette(3)), true),
            Added::New
        );
        assert_eq!(
            list.entries(),
            [
                entry("@BOB:example", palette(12)),
                entry("*", NickColor::Auto),
                entry("@bob:example", palette(3)),
            ]
        );
        assert_eq!(list.find(&pattern("@bob:example")), Some(0));
        assert_eq!(list.find(&pattern("@carol:example")), None);
        list.move_up(2);
        assert_eq!(list.entries()[1], entry("@bob:example", palette(3)));
        assert_eq!(list.remove(0), entry("@BOB:example", palette(12)));
        list.set(1, entry("@dave:example", palette(5)));
        assert_eq!(
            list.entries(),
            [
                entry("@bob:example", palette(3)),
                entry("@dave:example", palette(5)),
            ]
        );
    }

    #[test]
    #[should_panic(expected = "an entry moves up from below the first")]
    fn the_first_entry_cannot_move_up() {
        let mut list = NickColorList::default();
        list.push(entry("*", NickColor::Auto));
        list.move_up(0);
    }

    #[test]
    fn ini_values_round_trip() {
        let mut every = entry("@*:matrix.org", palette(12));
        every.modes = Some(ModeCondition::parse_glyphs("+@~").unwrap());
        every.notify = true;
        let mut any = entry("*", NickColor::Auto);
        any.modes = Some(ModeCondition::AnyMode);
        let mut none = entry("@bob:example", palette(0));
        none.modes = Some(ModeCondition::NoMode);
        for (entry, value) in [
            (every, "@*:matrix.org 12 modes=~@+ notify"),
            (any, "* auto any"),
            (none, "@bob:example 0 none"),
            (entry("*bot*", palette(15)), "*bot* 15"),
        ] {
            assert_eq!(entry.ini_value(), value);
            assert_eq!(NickColorEntry::parse_ini_value(value), Ok(entry));
        }
        assert_eq!(
            NickColorEntry::parse_ini_value("*  auto   notify  any"),
            Ok({
                let mut entry = entry("*", NickColor::Auto);
                entry.modes = Some(ModeCondition::AnyMode);
                entry.notify = true;
                entry
            })
        );
        for (value, error) in [
            (
                "",
                "expected <pattern> <color> [modes=~&@+|any|none] [notify]",
            ),
            (
                "*",
                "expected <pattern> <color> [modes=~&@+|any|none] [notify]",
            ),
            ("bob 4", "bob is not a user ID or a pattern with * or ?"),
            ("* 16", "color must be 0-15 or *, not 16"),
            ("* red", "color must be 0-15 or *, not red"),
            ("* 4 any none", "one of modes=, any and none at most"),
            ("* 4 modes=@ any", "one of modes=, any and none at most"),
            ("* 4 modes=%", "modes must be some of ~&@+, not %"),
            ("* 4 notify notify", "unexpected notify"),
            ("* 4 op", "unexpected op"),
        ] {
            assert_eq!(
                NickColorEntry::parse_ini_value(value),
                Err(error.to_owned()),
                "{value}"
            );
        }
    }

    #[test]
    fn the_lines_name_entries_as_the_notify_list_does() {
        let mut setting = NickColorSetting::default();
        assert_eq!(
            list_lines(&setting, nick_of),
            ["* Nick color list is empty (on)"]
        );
        setting.active = false;
        assert_eq!(
            list_lines(&setting, nick_of),
            ["* Nick color list is empty (off)"]
        );
        setting.active = true;
        setting.list.push(entry("@bob:example", palette(4)));
        let mut staff = entry("*", NickColor::Auto);
        staff.modes = Some(ModeCondition::parse_glyphs("@&~").unwrap());
        assert_eq!(staff.apply_text(), "~&@");
        setting.list.push(staff);
        let mut buddies = entry("@*:matrix.org", palette(12));
        buddies.notify = true;
        buddies.modes = Some(ModeCondition::AnyMode);
        assert_eq!(buddies.apply_text(), "any mode, notify");
        setting.list.push(buddies);
        let mut members = entry("@carol:example", palette(3));
        members.modes = Some(ModeCondition::NoMode);
        setting.list.push(members);
        assert_eq!(
            list_lines(&setting, nick_of),
            [
                "* Nick colors are on:",
                "*   1. Bob (@bob:example) 4",
                "*   2. * auto, modes ~&@",
                "*   3. @*:matrix.org 12, any mode, notify",
                "*   4. @carol:example 3, no mode",
            ]
        );
        assert_eq!(state_line(true), "* Nick colors are on");
        assert_eq!(state_line(false), "* Nick colors are off");
        let bob = entry("@bob:example", palette(4));
        assert_eq!(
            added_line(Added::New, &bob, nick_of),
            "* Added Bob (@bob:example) to nick color list"
        );
        assert_eq!(
            added_line(Added::Updated, &entry("*", NickColor::Auto), nick_of),
            "* Updated * in nick color list"
        );
        assert_eq!(
            removed_line(&bob, nick_of),
            "* Removed Bob (@bob:example) from nick color list"
        );
    }

    #[test]
    fn colours_parse_as_cnick_takes_them() {
        assert_eq!(NickColor::parse("*"), Ok(NickColor::Auto));
        assert_eq!(NickColor::parse("Auto"), Ok(NickColor::Auto));
        assert_eq!(NickColor::parse("04"), Ok(palette(4)));
        assert_eq!(
            NickColor::parse("-1"),
            Err("color must be 0-15 or *, not -1".to_owned())
        );
        assert_eq!(palette(4).text(), "4");
        assert_eq!(NickColor::Auto.text(), "auto");
    }
}
