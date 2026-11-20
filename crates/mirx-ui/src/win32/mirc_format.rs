//! mIRC formatting codes, as typed in the editbox, and the styled text they
//! stand for. The codes and their rules are the ones documented at
//! <https://modern.ircdocs.horse/formatting>.

use mirx_core::{TextColor, TextStyle, TextStyleSpan, check_text_styles};

use crate::automation::MircFormatKey;

const BOLD: char = '\x02';
const COLOR: char = '\x03';
const HEX_COLOR: char = '\x04';
const RESET: char = '\x0f';
const MONOSPACE: char = '\x11';
const REVERSE: char = '\x16';
const ITALIC: char = '\x1d';
const STRIKETHROUGH: char = '\x1e';
const UNDERLINE: char = '\x1f';

const fn color(hex: u32) -> TextColor {
    TextColor {
        red: (hex >> 16) as u8,
        green: (hex >> 8) as u8,
        blue: hex as u8,
    }
}

/// The colours `^K` indices name: mIRC's default 16, then the extended 83
/// of the ircdocs table. Index 99 is the default colour, so not here.
#[rustfmt::skip]
pub(super) const MIRC_PALETTE: [TextColor; 99] = [
    // 0-15: mIRC's defaults.
    color(0xffffff), color(0x000000), color(0x00007f), color(0x009300),
    color(0xff0000), color(0x7f0000), color(0x9c009c), color(0xfc7f00),
    color(0xffff00), color(0x00fc00), color(0x009393), color(0x00ffff),
    color(0x0000fc), color(0xff00ff), color(0x7f7f7f), color(0xd2d2d2),
    // 16-27
    color(0x470000), color(0x472100), color(0x474700), color(0x324700),
    color(0x004700), color(0x00472c), color(0x004747), color(0x002747),
    color(0x000047), color(0x2e0047), color(0x470047), color(0x47002a),
    // 28-39
    color(0x740000), color(0x743a00), color(0x747400), color(0x517400),
    color(0x007400), color(0x007449), color(0x007474), color(0x004074),
    color(0x000074), color(0x4b0074), color(0x740074), color(0x740045),
    // 40-51
    color(0xb50000), color(0xb56300), color(0xb5b500), color(0x7db500),
    color(0x00b500), color(0x00b571), color(0x00b5b5), color(0x0063b5),
    color(0x0000b5), color(0x7500b5), color(0xb500b5), color(0xb5006b),
    // 52-63
    color(0xff0000), color(0xff8c00), color(0xffff00), color(0xb2ff00),
    color(0x00ff00), color(0x00ffa0), color(0x00ffff), color(0x008cff),
    color(0x0000ff), color(0xa500ff), color(0xff00ff), color(0xff0098),
    // 64-75
    color(0xff5959), color(0xffb459), color(0xffff71), color(0xcfff60),
    color(0x6fff6f), color(0x65ffc9), color(0x6dffff), color(0x59b4ff),
    color(0x5959ff), color(0xc459ff), color(0xff66ff), color(0xff59bc),
    // 76-87
    color(0xff9c9c), color(0xffd39c), color(0xffff9c), color(0xe2ff9c),
    color(0x9cff9c), color(0x9cffdb), color(0x9cffff), color(0x9cd3ff),
    color(0x9c9cff), color(0xdc9cff), color(0xff9cff), color(0xff94d3),
    // 88-98: greys.
    color(0x000000), color(0x131313), color(0x282828), color(0x363636),
    color(0x4d4d4d), color(0x656565), color(0x818181), color(0x9f9f9f),
    color(0xbcbcbc), color(0xe2e2e2), color(0xffffff),
];

/// The `^K` index of the default colour.
const DEFAULT_COLOR_INDEX: u8 = 99;

/// The code a Ctrl chord in the editbox types, Alt not held: B, K, U, I, O,
/// R and E, as in mIRC.
pub(super) fn mirc_format_key(ctrl: bool, alt: bool, virtual_key: usize) -> Option<MircFormatKey> {
    if !ctrl || alt {
        return None;
    }
    let key = u8::try_from(virtual_key).ok()?;
    match key {
        b'B' => Some(MircFormatKey::Bold),
        b'K' => Some(MircFormatKey::Color),
        b'U' => Some(MircFormatKey::Underline),
        b'I' => Some(MircFormatKey::Italic),
        b'O' => Some(MircFormatKey::Reset),
        b'R' => Some(MircFormatKey::Reverse),
        b'E' => Some(MircFormatKey::Strikethrough),
        _ => None,
    }
}

pub(super) fn mirc_format_code(key: MircFormatKey) -> char {
    match key {
        MircFormatKey::Bold => BOLD,
        MircFormatKey::Color => COLOR,
        MircFormatKey::Underline => UNDERLINE,
        MircFormatKey::Italic => ITALIC,
        MircFormatKey::Reset => RESET,
        MircFormatKey::Reverse => REVERSE,
        MircFormatKey::Strikethrough => STRIKETHROUGH,
    }
}

/// Which number of a `^K` colour code is being typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ColorCodePart {
    Foreground,
    Background,
}

/// A `^K` colour code whose number is still being typed at the caret: the
/// part, and its digits so far (none or one), which a picked colour's two
/// digits replace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ColorCodeInProgress {
    pub(super) part: ColorCodePart,
    pub(super) typed_digits: usize,
    /// Where the `^K` is in the text before the caret, in UTF-16 units.
    pub(super) code_offset: usize,
}

/// The colour code `before_caret` ends in, while a digit can still extend
/// its number: `^K` and up to one digit, or `^K`, one or two digits, a
/// comma and up to one digit. A number of two digits is done, so the
/// colour index goes once it is typed.
pub(super) fn color_code_in_progress(before_caret: &str) -> Option<ColorCodeInProgress> {
    type Backwards<'a> = std::iter::Peekable<std::iter::Rev<std::str::Chars<'a>>>;
    // A number's digits, read backwards: at most two.
    fn take_digits(chars: &mut Backwards) -> usize {
        let mut count = 0;
        while count < 2 && chars.next_if(char::is_ascii_digit).is_some() {
            count += 1;
        }
        count
    }
    let mut chars = before_caret.chars().rev().peekable();
    let typed_digits = take_digits(&mut chars);
    // The code's length; it is all ASCII, so in UTF-16 units too.
    let (part, code_length) = match chars.next() {
        Some(COLOR) if typed_digits < 2 => (ColorCodePart::Foreground, 1 + typed_digits),
        Some(',') if typed_digits < 2 => match (take_digits(&mut chars), chars.next()) {
            (foreground @ (1 | 2), Some(COLOR)) => {
                (ColorCodePart::Background, 1 + foreground + 1 + typed_digits)
            }
            _ => return None,
        },
        _ => return None,
    };
    Some(ColorCodeInProgress {
        part,
        typed_digits,
        code_offset: before_caret.encode_utf16().count() - code_length,
    })
}

/// The formatting the codes so far turn on.
#[derive(Debug, Clone, Copy, Default)]
struct FormatState {
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
    foreground: Option<TextColor>,
    background: Option<TextColor>,
    reverse: bool,
}

impl FormatState {
    fn style(&self) -> TextStyle {
        // Reverse swaps the default colours, black on white becoming white
        // on black; mIRC shows no colour codes while it is on.
        let (foreground, background) = if self.reverse {
            (Some(MIRC_PALETTE[0]), Some(MIRC_PALETTE[1]))
        } else {
            (self.foreground, self.background)
        };
        TextStyle {
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            strikethrough: self.strikethrough,
            foreground,
            background,
        }
    }
}

/// Reads one or two ASCII digits at the start of `text`: a `^K` index.
fn color_index(text: &str) -> Option<(u8, usize)> {
    let digits = text.bytes().take(2).take_while(u8::is_ascii_digit).count();
    (digits > 0).then(|| (text[..digits].parse().expect("ASCII digits"), digits))
}

/// The colour `index` names, `None` for 99 (the default).
fn palette_color(index: u8) -> Option<TextColor> {
    (index != DEFAULT_COLOR_INDEX).then(|| MIRC_PALETTE[usize::from(index)])
}

/// Reads exactly six hex digits at the start of `text`: a `^D` colour.
fn hex_color(text: &str) -> Option<TextColor> {
    let hex = text.get(..6)?;
    hex.bytes()
        .all(|byte| byte.is_ascii_hexdigit())
        .then(|| color(u32::from_str_radix(hex, 16).expect("hex digits")))
}

/// Reads a colour code's arguments, `fg[,bg]`, after the code: how many
/// bytes they take, and the colours they set (`None` for a bare code, which
/// resets both).
fn color_arguments<Color>(
    text: &str,
    read: impl Fn(&str) -> Option<(Color, usize)>,
) -> (usize, Option<(Color, Option<Color>)>) {
    let Some((foreground, foreground_len)) = read(text) else {
        return (0, None);
    };
    let rest = &text[foreground_len..];
    // A comma not followed by a colour is text.
    match rest.strip_prefix(',').and_then(&read) {
        Some((background, background_len)) => (
            foreground_len + 1 + background_len,
            Some((foreground, Some(background))),
        ),
        None => (foreground_len, Some((foreground, None))),
    }
}

/// The text typed with mIRC codes, without them, and the styles they set.
pub(super) fn parse_mirc_input(input: &str) -> (String, Vec<TextStyleSpan>) {
    let mut body = String::new();
    let mut styles: Vec<TextStyleSpan> = Vec::new();
    let mut state = FormatState::default();
    let mut rest = input;
    while let Some(ch) = rest.chars().next() {
        rest = &rest[ch.len_utf8()..];
        match ch {
            BOLD => state.bold = !state.bold,
            ITALIC => state.italic = !state.italic,
            UNDERLINE => state.underline = !state.underline,
            STRIKETHROUGH => state.strikethrough = !state.strikethrough,
            REVERSE => state.reverse = !state.reverse,
            MONOSPACE => {}
            RESET => state = FormatState::default(),
            COLOR | HEX_COLOR => {
                let (len, colors) = if ch == COLOR {
                    let (len, indices) = color_arguments(rest, color_index);
                    let colors = indices.map(|(foreground, background)| {
                        (palette_color(foreground), background.map(palette_color))
                    });
                    (len, colors)
                } else {
                    color_arguments(rest, |text| hex_color(text).map(|color| (Some(color), 6)))
                };
                rest = &rest[len..];
                match colors {
                    Some((foreground, background)) => {
                        state.foreground = foreground;
                        if let Some(background) = background {
                            state.background = background;
                        }
                    }
                    None => {
                        state.foreground = None;
                        state.background = None;
                    }
                }
            }
            _ => {
                let start = body.len();
                body.push(ch);
                let style = state.style();
                // Line breaks end spans; a span never holds one.
                if style == TextStyle::default() || ch == '\n' {
                    continue;
                }
                match styles.last_mut() {
                    Some(last) if last.end == start && last.style == style => {
                        last.end = body.len();
                    }
                    _ => styles.push(TextStyleSpan {
                        start,
                        end: body.len(),
                        style,
                    }),
                }
            }
        }
    }
    check_text_styles(&body, &styles).expect("parsed mIRC codes make valid styles");
    (body, styles)
}

/// The `^K` index of `color`: the palette's own entry, else its nearest.
fn nearest_color_index(color: TextColor) -> usize {
    let distance = |entry: &TextColor| {
        let channel = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        channel(entry.red, color.red)
            + channel(entry.green, color.green)
            + channel(entry.blue, color.blue)
    };
    // The first of equally near entries: the base 16 before the rest.
    MIRC_PALETTE
        .iter()
        .enumerate()
        .min_by_key(|(index, entry)| (distance(entry), *index))
        .expect("the palette is not empty")
        .0
}

fn push_style_codes(input: &mut String, style: &TextStyle) {
    for (on, code) in [
        (style.bold, BOLD),
        (style.italic, ITALIC),
        (style.underline, UNDERLINE),
        (style.strikethrough, STRIKETHROUGH),
    ] {
        if on {
            input.push(code);
        }
    }
    if style.foreground.is_some() || style.background.is_some() {
        // Both colours, two digits each: text after the code can never be
        // read as part of it.
        let index = |color: Option<TextColor>| {
            color.map_or(usize::from(DEFAULT_COLOR_INDEX), nearest_color_index)
        };
        input.push(COLOR);
        input.push_str(&format!(
            "{:02},{:02}",
            index(style.foreground),
            index(style.background)
        ));
    }
}

/// The editbox text that types `body` in `styles`: at each span, a reset
/// then the span's codes. Colours off the palette become their nearest.
pub(super) fn mirc_input_from_styled(body: &str, styles: &[TextStyleSpan]) -> String {
    check_text_styles(body, styles).expect("message styles are valid");
    let mut input = String::new();
    let mut cursor = 0;
    for span in styles {
        input.push_str(&body[cursor..span.start]);
        push_style_codes(&mut input, &span.style);
        input.push_str(&body[span.start..span.end]);
        // The end of the text needs no reset.
        if span.end != body.len() {
            input.push(RESET);
        }
        cursor = span.end;
    }
    input.push_str(&body[cursor..]);
    input
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: usize, end: usize, style: TextStyle) -> TextStyleSpan {
        TextStyleSpan { start, end, style }
    }

    fn bold() -> TextStyle {
        TextStyle {
            bold: true,
            ..TextStyle::default()
        }
    }

    fn colored(foreground: Option<usize>, background: Option<usize>) -> TextStyle {
        TextStyle {
            foreground: foreground.map(|index| MIRC_PALETTE[index]),
            background: background.map(|index| MIRC_PALETTE[index]),
            ..TextStyle::default()
        }
    }

    fn parsed(input: &str) -> (String, Vec<TextStyleSpan>) {
        parse_mirc_input(input)
    }

    #[test]
    fn the_palette_starts_with_mircs_16_and_ends_in_white() {
        assert_eq!(MIRC_PALETTE[4], color(0xff0000));
        assert_eq!(MIRC_PALETTE[12], color(0x0000fc));
        assert_eq!(MIRC_PALETTE[16], color(0x470000));
        assert_eq!(MIRC_PALETTE[61], color(0xa500ff));
        assert_eq!(MIRC_PALETTE[98], color(0xffffff));
    }

    #[test]
    fn toggles_style_text_between_them() {
        assert_eq!(
            parsed("\x02hi\x02 there"),
            ("hi there".to_owned(), vec![span(0, 2, bold())])
        );
        let all = TextStyle {
            bold: true,
            italic: true,
            underline: true,
            strikethrough: true,
            ..TextStyle::default()
        };
        assert_eq!(
            parsed("\x02\x1d\x1f\x1eall\x0f none"),
            ("all none".to_owned(), vec![span(0, 3, all)])
        );
        // Off and straight back on is one span.
        assert_eq!(
            parsed("\x02a\x02\x02b"),
            ("ab".to_owned(), vec![span(0, 2, bold())])
        );
    }

    #[test]
    fn codes_with_no_text_after_them_make_no_span() {
        assert_eq!(parsed("plain\x02\x034"), ("plain".to_owned(), vec![]));
        assert_eq!(parsed("\x02\x02"), (String::new(), vec![]));
    }

    #[test]
    fn monospace_is_dropped() {
        assert_eq!(parsed("\x11code\x11"), ("code".to_owned(), vec![]));
    }

    #[test]
    fn color_codes_take_up_to_two_digits_each() {
        assert_eq!(
            parsed("\x034red"),
            ("red".to_owned(), vec![span(0, 3, colored(Some(4), None))])
        );
        // Two digits are always read: 123 is colour 12, then "3".
        assert_eq!(
            parsed("\x03123"),
            ("3".to_owned(), vec![span(0, 1, colored(Some(12), None))])
        );
        assert_eq!(
            parsed("\x0304,01x"),
            ("x".to_owned(), vec![span(0, 1, colored(Some(4), Some(1)))])
        );
        assert_eq!(
            parsed("\x038,2x"),
            ("x".to_owned(), vec![span(0, 1, colored(Some(8), Some(2)))])
        );
        // A comma without a digit after it is text.
        assert_eq!(
            parsed("\x034,x"),
            (",x".to_owned(), vec![span(0, 2, colored(Some(4), None))])
        );
        // A foreground alone keeps the background.
        assert_eq!(
            parsed("\x034,1a\x039b"),
            (
                "ab".to_owned(),
                vec![
                    span(0, 1, colored(Some(4), Some(1))),
                    span(1, 2, colored(Some(9), Some(1)))
                ]
            )
        );
        assert_eq!(
            parsed("\x0361violet"),
            (
                "violet".to_owned(),
                vec![span(0, 6, colored(Some(61), None))]
            )
        );
    }

    #[test]
    fn a_bare_color_code_or_99_resets_colors() {
        assert_eq!(
            parsed("\x034,1a\x03b"),
            ("ab".to_owned(), vec![span(0, 1, colored(Some(4), Some(1)))])
        );
        assert_eq!(
            parsed("\x034,1a\x0399b"),
            (
                "ab".to_owned(),
                vec![
                    span(0, 1, colored(Some(4), Some(1))),
                    span(1, 2, colored(None, Some(1)))
                ]
            )
        );
        assert_eq!(
            parsed("\x034,1a\x0304,99b"),
            (
                "ab".to_owned(),
                vec![
                    span(0, 1, colored(Some(4), Some(1))),
                    span(1, 2, colored(Some(4), None))
                ]
            )
        );
    }

    #[test]
    fn hex_color_codes_take_six_digits() {
        let violet = TextStyle {
            foreground: Some(color(0xa5_00_ff)),
            background: Some(color(0x00_00_00)),
            ..TextStyle::default()
        };
        assert_eq!(
            parsed("\x04A500ff,000000v\x04 w"),
            ("v w".to_owned(), vec![span(0, 1, violet)])
        );
        // Fewer than six hex digits are text after a reset.
        assert_eq!(parsed("\x04abcx"), ("abcx".to_owned(), vec![]));
    }

    #[test]
    fn reverse_is_white_on_black_over_any_color() {
        let reversed = colored(Some(0), Some(1));
        assert_eq!(
            parsed("\x034a\x16b\x039c\x16d"),
            (
                "abcd".to_owned(),
                vec![
                    span(0, 1, colored(Some(4), None)),
                    span(1, 3, reversed),
                    span(3, 4, colored(Some(9), None))
                ]
            )
        );
    }

    #[test]
    fn reset_ends_every_style() {
        assert_eq!(
            parsed("\x02\x16\x034a\x0fb"),
            (
                "ab".to_owned(),
                vec![span(
                    0,
                    1,
                    TextStyle {
                        bold: true,
                        ..colored(Some(0), Some(1))
                    }
                )]
            )
        );
    }

    #[test]
    fn offsets_are_bytes_in_the_stripped_text() {
        assert_eq!(
            parsed("ж\x02курсив\x02!"),
            ("жкурсив!".to_owned(), vec![span(2, 14, bold())])
        );
    }

    #[test]
    fn line_breaks_split_spans() {
        assert_eq!(
            parsed("\x02a\nb"),
            (
                "a\nb".to_owned(),
                vec![span(0, 1, bold()), span(2, 3, bold())]
            )
        );
    }

    #[test]
    fn styled_text_writes_reset_then_set_codes() {
        assert_eq!(
            mirc_input_from_styled(
                "bold red and plain",
                &[span(0, 4, bold()), span(5, 8, colored(Some(4), None)),]
            ),
            "\x02bold\x0f \x0304,99red\x0f and plain"
        );
        // Adjacent spans: the reset ends the first.
        assert_eq!(
            mirc_input_from_styled(
                "ab",
                &[span(0, 1, bold()), span(1, 2, colored(None, Some(1)))]
            ),
            "\x02a\x0f\x0399,01b"
        );
        assert_eq!(mirc_input_from_styled("plain", &[]), "plain");
    }

    #[test]
    fn colors_off_the_palette_become_the_nearest() {
        assert_eq!(nearest_color_index(color(0xfe0101)), 4);
        // White is 0 and 98: the base colour wins.
        assert_eq!(nearest_color_index(color(0xffffff)), 0);
        assert_eq!(nearest_color_index(color(0xa400fe)), 61);
        assert_eq!(
            mirc_input_from_styled(
                "x",
                &[span(
                    0,
                    1,
                    TextStyle {
                        foreground: Some(color(0x123456)),
                        ..TextStyle::default()
                    }
                )]
            ),
            format!("\x03{:02},99x", nearest_color_index(color(0x123456)))
        );
    }

    #[test]
    fn styled_text_round_trips_through_codes() {
        let cases: Vec<(&str, Vec<TextStyleSpan>)> = vec![
            ("", vec![]),
            ("plain", vec![]),
            ("hi there", vec![span(0, 2, bold())]),
            ("1,2 digits", vec![span(0, 3, colored(Some(3), None))]),
            (
                "ab",
                vec![span(0, 1, bold()), span(1, 2, colored(Some(61), Some(98)))],
            ),
            ("жкурсив!", vec![span(2, 14, bold())]),
            ("a\nb", vec![span(0, 1, bold()), span(2, 3, bold())]),
        ];
        for (body, styles) in cases {
            let input = mirc_input_from_styled(body, &styles);
            assert_eq!(
                parse_mirc_input(&input),
                (body.to_owned(), styles),
                "{input:?}"
            );
        }

        // Every combination of the toggles and some colours, between plain
        // text and next to another style.
        let colors = [None, Some(0), Some(4), Some(15), Some(16), Some(97)];
        for flags in 0..16u8 {
            for foreground in colors {
                for background in colors {
                    let style = TextStyle {
                        bold: flags & 1 != 0,
                        italic: flags & 2 != 0,
                        underline: flags & 4 != 0,
                        strikethrough: flags & 8 != 0,
                        ..colored(foreground, background)
                    };
                    if style == TextStyle::default() {
                        continue;
                    }
                    for styles in [
                        vec![span(2, 5, style)],
                        vec![span(0, 2, bold()), span(2, 5, style)],
                        vec![span(2, 5, style), span(5, 7, colored(Some(2), None))],
                    ] {
                        let body = "1,2,3 4,5";
                        if styles.windows(2).any(|pair| pair[0].style == pair[1].style) {
                            continue;
                        }
                        let input = mirc_input_from_styled(body, &styles);
                        assert_eq!(
                            parse_mirc_input(&input),
                            (body.to_owned(), styles),
                            "{input:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn ctrl_chords_type_codes_without_alt() {
        for (letter, key) in [
            (b'B', MircFormatKey::Bold),
            (b'K', MircFormatKey::Color),
            (b'U', MircFormatKey::Underline),
            (b'I', MircFormatKey::Italic),
            (b'O', MircFormatKey::Reset),
            (b'R', MircFormatKey::Reverse),
            (b'E', MircFormatKey::Strikethrough),
        ] {
            let virtual_key = usize::from(letter);
            assert_eq!(mirc_format_key(true, false, virtual_key), Some(key));
            assert_eq!(mirc_format_key(false, false, virtual_key), None);
            // Ctrl+Alt is AltGr, which types characters.
            assert_eq!(mirc_format_key(true, true, virtual_key), None);
        }
        assert_eq!(mirc_format_key(true, false, usize::from(b'F')), None);
        assert_eq!(mirc_format_key(true, false, 0x109), None);
        assert_eq!(mirc_format_code(MircFormatKey::Color), '\x03');
        assert_eq!(mirc_format_code(MircFormatKey::Italic), '\x1d');
        assert_eq!(mirc_format_code(MircFormatKey::Strikethrough), '\x1e');
    }

    #[test]
    fn a_colour_code_is_in_progress_until_its_number_is_complete() {
        use ColorCodePart::{Background, Foreground};
        let in_progress = |part, typed_digits, code_offset| {
            Some(ColorCodeInProgress {
                part,
                typed_digits,
                code_offset,
            })
        };
        for (before_caret, expected) in [
            ("\x03", in_progress(Foreground, 0, 0)),
            ("\x034", in_progress(Foreground, 1, 0)),
            ("\x0304", None),
            ("\x034,", in_progress(Background, 0, 0)),
            ("\x034,1", in_progress(Background, 1, 0)),
            ("\x0304,", in_progress(Background, 0, 0)),
            ("\x034,12", None),
            ("a", None),
            ("\x03x", None),
            ("", None),
            (",", None),
            ("4,1", None),
            ("\x03,", None),
            ("\x03123,", None),
            ("hi \x03", in_progress(Foreground, 0, 3)),
            ("é \x0312,", in_progress(Background, 0, 2)),
            ("\u{1f600}\x034", in_progress(Foreground, 1, 2)),
        ] {
            assert_eq!(
                color_code_in_progress(before_caret),
                expected,
                "{before_caret:?}"
            );
        }
    }
}
