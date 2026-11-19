//! Splitting text into runs that each draw in one font of a fallback chain.

use std::borrow::Cow;

/// Text drawn in one font: `slot` 0 is the primary font and `k` the `k`th
/// fallback. `text` holds only the characters to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GlyphRun<'a> {
    pub(super) slot: usize,
    pub(super) text: Cow<'a, str>,
}

/// Format characters that are not drawn: zero-width spaces, joiners and
/// direction marks, word joiner and invisible operators, variation
/// selectors, the BOM and tags. Drawn, a font without them would show a box
/// after `👍️` (thumbs up plus VS16), the common form Element sends.
pub(super) fn is_default_ignorable(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}'
            | '\u{2060}'..='\u{2064}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{E0000}'..='\u{E0FFF}'
    )
}

/// Splits `text` into runs: each character goes to the first of the
/// `slot_count` slots that `covers` it, or to slot 0 when none does (the
/// primary font then draws its default glyph); adjacent characters of a slot
/// share a run. Default-ignorable characters are dropped and split no run.
pub(super) fn split_runs(
    text: &str,
    slot_count: usize,
    covers: impl Fn(usize, char) -> bool,
) -> Vec<GlyphRun<'_>> {
    let mut runs = Vec::<GlyphRun<'_>>::new();
    for (index, c) in text.char_indices() {
        if is_default_ignorable(c) {
            continue;
        }
        let end = index + c.len_utf8();
        let slot = (0..slot_count).find(|&slot| covers(slot, c)).unwrap_or(0);
        match runs.last_mut() {
            Some(run) if run.slot == slot => match run.text {
                // Still one slice of `text` and contiguous: widen it.
                Cow::Borrowed(slice) if slice_end(text, slice) == index => {
                    run.text = Cow::Borrowed(&text[slice_end(text, slice) - slice.len()..end]);
                }
                // An ignorable character was skipped inside the run.
                _ => run.text.to_mut().push(c),
            },
            _ => runs.push(GlyphRun {
                slot,
                text: Cow::Borrowed(&text[index..end]),
            }),
        }
    }
    runs
}

/// The byte offset in `text` just past `slice`, a slice of it.
fn slice_end(text: &str, slice: &str) -> usize {
    slice.as_ptr() as usize - text.as_ptr() as usize + slice.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Slot 0 covers ASCII, slot 1 punctuation and Cyrillic, slot 2 emoji.
    fn covers(slot: usize, c: char) -> bool {
        match slot {
            0 => c.is_ascii(),
            1 => matches!(c, '—' | '’' | 'А'..='я'),
            2 => matches!(c, '\u{1F300}'..='\u{1FAFF}'),
            _ => unreachable!("three slots"),
        }
    }

    fn runs(text: &str) -> Vec<(usize, String)> {
        split_runs(text, 3, covers)
            .into_iter()
            .map(|run| (run.slot, run.text.into_owned()))
            .collect()
    }

    #[test]
    fn ascii_is_one_primary_run() {
        let runs = split_runs("hello world", 3, covers);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].slot, 0);
        assert!(matches!(runs[0].text, Cow::Borrowed("hello world")));
    }

    #[test]
    fn a_fallback_character_splits_the_run() {
        assert_eq!(
            runs("a—b"),
            [(0, "a".into()), (1, "—".into()), (0, "b".into())]
        );
    }

    #[test]
    fn supplementary_characters_go_to_the_slot_covering_them() {
        assert_eq!(
            runs("hi 🐶!"),
            [(0, "hi ".into()), (2, "🐶".into()), (0, "!".into())]
        );
    }

    #[test]
    fn adjacent_characters_of_a_slot_merge() {
        assert_eq!(runs("Привет"), [(1, "Привет".into())]);
        assert_eq!(runs("🐶🐱 ok"), [(2, "🐶🐱".into()), (0, " ok".into())]);
    }

    #[test]
    fn variation_selectors_are_dropped() {
        assert_eq!(runs("👍\u{FE0F}"), [(2, "👍".into())]);
        assert_eq!(runs("👍\u{FE0F} ok"), [(2, "👍".into()), (0, " ok".into())]);
    }

    #[test]
    fn joiners_are_dropped_without_splitting_a_run() {
        assert_eq!(runs("a\u{200D}b"), [(0, "ab".into())]);
        assert_eq!(runs("\u{200B}a"), [(0, "a".into())]);
    }

    #[test]
    fn uncovered_characters_stay_in_the_primary() {
        assert_eq!(runs("a日本b"), [(0, "a日本b".into())]);
        assert_eq!(runs("—日"), [(1, "—".into()), (0, "日".into())]);
    }

    #[test]
    fn empty_and_ignorable_only_text_has_no_runs() {
        assert_eq!(runs(""), []);
        assert_eq!(runs("\u{FE0F}\u{200D}"), []);
    }

    #[test]
    fn default_ignorable_ranges_are_exact() {
        for (c, ignorable) in [
            ('\u{200A}', false),
            ('\u{200B}', true),
            ('\u{200F}', true),
            ('\u{2010}', false),
            ('\u{205F}', false),
            ('\u{2060}', true),
            ('\u{2064}', true),
            ('\u{2065}', false),
            ('\u{FDFF}', false),
            ('\u{FE00}', true),
            ('\u{FE0F}', true),
            ('\u{FE10}', false),
            ('\u{FEFE}', false),
            ('\u{FEFF}', true),
            ('\u{FF00}', false),
            ('\u{DFFFF}', false),
            ('\u{E0000}', true),
            ('\u{E0FFF}', true),
            ('\u{E1000}', false),
        ] {
            assert_eq!(is_default_ignorable(c), ignorable, "{:#x}", u32::from(c));
        }
    }
}
