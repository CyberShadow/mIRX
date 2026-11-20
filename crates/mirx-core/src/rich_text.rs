//! Matrix HTML (`org.matrix.custom.html`) to display text with flat style
//! spans.
//!
//! The UI draws a message as lines of text, the way mIRC does, so HTML's
//! blocks become lines (lists, quotes, code, paragraphs, `<br>`), inline
//! formatting becomes [`TextStyleSpan`]s, and links keep their URL in the
//! text. Everything the timeline cannot show (fonts, sizes, images) becomes
//! its text.

use std::ops::Range;

use matrix_sdk::ruma::UserId;
use ruma_html::{Html, NodeData, NodeRef, SanitizerConfig};

use crate::api::{
    TextColor, TextStyle, TextStyleSpan, UserLinkSpan, check_text_styles, check_user_links,
};

/// A message's display text and the styles that format it, which pass
/// [`check_text_styles`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StyledText {
    pub(crate) body: String,
    pub(crate) styles: Vec<TextStyleSpan>,
}

impl StyledText {
    pub(crate) fn plain(body: String) -> Self {
        Self {
            body,
            styles: Vec::new(),
        }
    }
}

/// The text of a Matrix HTML body: sanitized as the spec says (which also
/// drops `<mx-reply>` fallbacks), with blocks as lines, without blank
/// lines between blocks, and HTML's whitespace collapsing outside `<pre>`.
pub(crate) fn styled_text_from_html(html: &str) -> StyledText {
    let html = Html::parse(html);
    // The sanitizer keeps the text of the elements it drops; these hold
    // code or form data rather than text, so they go whole, as in Element.
    html.sanitize_with(
        &SanitizerConfig::compat()
            .remove_reply_fallback()
            .remove_elements(["script", "style", "textarea", "option", "noscript"]),
    );
    let mut writer = Writer::default();
    let context = Context {
        style: TextStyle::default(),
        pre: false,
    };
    for child in html.children() {
        writer.node(&child, context);
    }
    writer.finish()
}

#[derive(Debug, Clone, Copy)]
struct Context {
    style: TextStyle,
    /// Inside `<pre>`: text is kept verbatim.
    pre: bool,
}

struct List {
    /// The next item's number in an `<ol>`, `None` in a `<ul>`.
    next_number: Option<i64>,
}

/// Writes lines of styled text. Line prefixes (quote markers, list
/// indents and item markers) are written lazily with a line's first text,
/// so empty lines never carry them; collapsed whitespace is held back
/// until more text follows on the line, so lines end in a space only when
/// a no-break space put it there.
#[derive(Default)]
struct Writer {
    body: String,
    styles: Vec<TextStyleSpan>,
    /// Whether the current line has text, prefixes included.
    line_started: bool,
    /// A collapsed space waiting for more text on the line, in the style of
    /// the whitespace it stands for.
    pending_space: Option<TextStyle>,
    /// Prefixes of every line in the current blocks: `> ` per quote, two
    /// spaces per nested list.
    prefixes: Vec<&'static str>,
    /// The current list item's marker, for its first line.
    marker: Option<String>,
    lists: Vec<List>,
}

impl Writer {
    fn node(&mut self, node: &NodeRef, context: Context) {
        match node.data() {
            NodeData::Text(text) => self.text(&text.borrow(), context),
            NodeData::Element(element) => {
                let name = element.name.local.as_ref();
                let attrs = element.attrs.borrow();
                let attr = |attr_name: &str| {
                    attrs
                        .iter()
                        .find(|attr| attr.name.local.as_ref() == attr_name)
                        .map(|attr| attr.value.to_string())
                };
                self.element(node, name, &attr, context);
            }
            // The sanitizer removes everything else, comments included.
            NodeData::Document | NodeData::Other => {
                unreachable!("a sanitized HTML fragment has only elements and text")
            }
        }
    }

    fn children(&mut self, node: &NodeRef, context: Context) {
        for child in node.children() {
            self.node(&child, context);
        }
    }

    fn element(
        &mut self,
        node: &NodeRef,
        name: &str,
        attr: &dyn Fn(&str) -> Option<String>,
        context: Context,
    ) {
        let mut inner = context;
        match name {
            "b" | "strong" => inner.style.bold = true,
            "i" | "em" => inner.style.italic = true,
            "u" => inner.style.underline = true,
            // The sanitizer turns `<strike>` into `<s>`.
            "del" | "s" => inner.style.strikethrough = true,
            // The sanitizer turns `<font>` into `<span>`, and its `color`
            // into `data-mx-color`.
            "span" => {
                if let Some(color) = attr("data-mx-color").as_deref().and_then(parse_html_color) {
                    inner.style.foreground = Some(color);
                }
                if let Some(color) = attr("data-mx-bg-color")
                    .as_deref()
                    .and_then(parse_html_color)
                {
                    inner.style.background = Some(color);
                }
                if attr("data-mx-spoiler").is_some() {
                    // mIRC's spoiler idiom: black on black (`^K1,1`).
                    inner.style.foreground = Some(BLACK);
                    inner.style.background = Some(BLACK);
                }
            }
            "br" => return self.end_line(),
            "hr" => {
                self.fresh_line();
                self.write("---", TextStyle::default());
                return self.fresh_line();
            }
            "a" => return self.link(node, attr("href"), context),
            "img" => {
                let text = attr("alt")
                    .filter(|alt| !alt.trim().is_empty())
                    .or_else(|| attr("title").filter(|title| !title.trim().is_empty()))
                    .unwrap_or_else(|| "image".to_owned());
                return self.text(
                    &text,
                    Context {
                        pre: false,
                        ..context
                    },
                );
            }
            "td" | "th" => self.space(TextStyle::default()),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                inner.style.bold = true;
                return self.block(node, inner);
            }
            "pre" => {
                inner.pre = true;
                return self.block(node, inner);
            }
            "p" | "div" | "table" | "tr" | "caption" | "details" | "summary" => {
                return self.block(node, inner);
            }
            "blockquote" => {
                self.prefixes.push("> ");
                self.block(node, inner);
                self.prefixes.pop();
                return;
            }
            "ul" | "ol" => {
                let next_number = (name == "ol").then(|| {
                    attr("start")
                        .and_then(|start| start.trim().parse::<i64>().ok())
                        .unwrap_or(1)
                });
                self.prefixes
                    .push(if self.lists.is_empty() { "" } else { "  " });
                self.lists.push(List { next_number });
                self.block(node, inner);
                self.lists.pop();
                self.prefixes.pop();
                return;
            }
            "li" => {
                let marker = match self.lists.last_mut() {
                    Some(List {
                        next_number: Some(number),
                    }) => {
                        let marker = format!("{number}. ");
                        *number += 1;
                        marker
                    }
                    Some(List { next_number: None }) | None => "* ".to_owned(),
                };
                self.fresh_line();
                self.marker = Some(marker);
                self.block(node, inner);
                self.marker = None;
                return;
            }
            // `code` keeps the timeline's font, which is already
            // fixed-pitch; the rest (`sup`, `sub`, table sections) only
            // contribute their text.
            _ => {}
        }
        self.children(node, inner);
    }

    /// A block's children on lines of their own.
    fn block(&mut self, node: &NodeRef, context: Context) {
        self.fresh_line();
        self.children(node, context);
        self.fresh_line();
    }

    /// A link's text, then its URL unless the text already says it or the
    /// link is a matrix.to permalink, whose text (a pill's display name)
    /// is what it shows.
    fn link(&mut self, node: &NodeRef, href: Option<String>, context: Context) {
        let start = self.body.len();
        self.children(node, context);
        let Some(href) = href else {
            return;
        };
        let shown_scheme = ["http:", "https:", "mailto:"]
            .iter()
            .any(|scheme| href.starts_with(scheme));
        if !shown_scheme || href.starts_with("https://matrix.to/#/") {
            return;
        }
        let text = self.body[start..].trim();
        let url_context = Context {
            pre: false,
            ..context
        };
        if text.is_empty() {
            self.text(&href, url_context);
        } else if text != href && Some(text) != href.strip_prefix("mailto:") {
            self.space(context.style);
            self.text(&format!("({href})"), url_context);
        }
    }

    fn text(&mut self, text: &str, context: Context) {
        if context.pre {
            for (index, line) in text.split('\n').enumerate() {
                if index > 0 {
                    self.end_line();
                }
                let line = line.replace('\r', "");
                if !line.is_empty() {
                    self.write(&line, context.style);
                }
            }
            return;
        }
        let mut rest = text;
        while !rest.is_empty() {
            let word_start = rest
                .find(|c: char| !c.is_ascii_whitespace())
                .unwrap_or(rest.len());
            if word_start > 0 {
                self.space(context.style);
            }
            rest = &rest[word_start..];
            let word_end = rest
                .find(|c: char| c.is_ascii_whitespace())
                .unwrap_or(rest.len());
            if word_end > 0 {
                self.write(&rest[..word_end], context.style);
            }
            rest = &rest[word_end..];
        }
    }

    /// Collapsed whitespace: one space, written only if text follows on
    /// this line.
    fn space(&mut self, style: TextStyle) {
        if self.line_started && self.pending_space.is_none() {
            self.pending_space = Some(style);
        }
    }

    fn write(&mut self, text: &str, style: TextStyle) {
        if !self.line_started {
            self.line_started = true;
            let prefix = self.prefixes.concat();
            self.push(&prefix, TextStyle::default());
            if let Some(marker) = self.marker.take() {
                self.push(&marker, TextStyle::default());
            }
        }
        if let Some(space_style) = self.pending_space.take() {
            self.push(" ", space_style);
        }
        // A no-break space is a space that HTML keeps: mirx writes runs of
        // spaces and spaces at a line's ends as `&nbsp;`.
        self.push(&text.replace('\u{a0}', " "), style);
    }

    fn push(&mut self, text: &str, style: TextStyle) {
        let start = self.body.len();
        self.body.push_str(text);
        if style == TextStyle::default() || text.is_empty() {
            return;
        }
        let end = self.body.len();
        match self.styles.last_mut() {
            Some(last) if last.end == start && last.style == style => last.end = end,
            _ => self.styles.push(TextStyleSpan { start, end, style }),
        }
    }

    /// Ends the current line, even an empty one, except before any text:
    /// a message never starts with an empty line.
    fn end_line(&mut self) {
        if self.body.is_empty() && !self.line_started {
            return;
        }
        self.body.push('\n');
        self.line_started = false;
        self.pending_space = None;
    }

    /// Starts a new line unless the current one is still empty.
    fn fresh_line(&mut self) {
        if self.line_started {
            self.end_line();
        }
    }

    fn finish(mut self) -> StyledText {
        // Spans never cover a `\n`, so trailing ones are unstyled.
        let trimmed = self.body.trim_end_matches('\n').len();
        self.body.truncate(trimmed);
        debug_assert_eq!(check_text_styles(&self.body, &self.styles), Ok(()));
        StyledText {
            body: self.body,
            styles: self.styles,
        }
    }
}

/// Matrix HTML for `body` formatted by `styles` and linked by `pills`,
/// which must pass [`check_text_styles`] and [`check_user_links`].
///
/// Each style span is a `<span>` with its colours (new messages use `span`,
/// not the deprecated `font`; spec v1.10) around `<b><i><u><del>`, in that
/// order, and each pill an `<a>` to the user's matrix.to permalink, which
/// clients show as a pill. Spans are flat and a style span is cut at a
/// pill's edges, so the elements always nest properly. `\n` is `<br>`, and
/// spaces that HTML would collapse or drop are `&nbsp;`, so the text reads
/// as typed: see [`push_escaped_html`].
pub(crate) fn html_from_styled_text(
    body: &str,
    styles: &[TextStyleSpan],
    pills: &[UserLinkSpan],
) -> String {
    check_text_styles(body, styles).expect("styles sent must describe their body");
    check_user_links(body, pills).expect("pills sent must describe their body");
    let mut edges = vec![0, body.len()];
    edges.extend(styles.iter().flat_map(|span| [span.start, span.end]));
    edges.extend(pills.iter().flat_map(|pill| [pill.start, pill.end]));
    edges.sort_unstable();
    edges.dedup();
    let mut styles = styles.iter().peekable();
    let mut pills = pills.iter().peekable();
    let mut html = String::new();
    for segment in edges.windows(2) {
        let (start, end) = (segment[0], segment[1]);
        if let Some(pill) = pills.peek().filter(|pill| pill.start == start) {
            let user_id = <&UserId>::try_from(pill.user_id.as_str()).expect("checked user ID");
            html.push_str(&format!("<a href=\"{}\">", user_id.matrix_to_uri()));
        }
        // A segment is wholly inside one style span or outside them all.
        let style = styles
            .peek()
            .filter(|span| span.start <= start)
            .map(|span| span.style);
        push_styled_html(&mut html, body, start..end, style);
        if styles.peek().is_some_and(|span| span.end == end) {
            styles.next();
        }
        if pills.peek().is_some_and(|pill| pill.end == end) {
            html.push_str("</a>");
            pills.next();
        }
    }
    html
}

/// `body[range]` as HTML in `style`, if any.
fn push_styled_html(html: &mut String, body: &str, range: Range<usize>, style: Option<TextStyle>) {
    let Some(style) = style else {
        return push_escaped_html(html, body, range);
    };
    let mut closing = Vec::new();
    if style.foreground.is_some() || style.background.is_some() {
        html.push_str("<span");
        if let Some(color) = style.foreground {
            html.push_str(&format!(" data-mx-color=\"{color}\""));
        }
        if let Some(color) = style.background {
            html.push_str(&format!(" data-mx-bg-color=\"{color}\""));
        }
        html.push('>');
        closing.push("</span>");
    }
    for (on, open, close) in [
        (style.bold, "<b>", "</b>"),
        (style.italic, "<i>", "</i>"),
        (style.underline, "<u>", "</u>"),
        (style.strikethrough, "<del>", "</del>"),
    ] {
        if on {
            html.push_str(open);
            closing.push(close);
        }
    }
    push_escaped_html(html, body, range);
    for close in closing.into_iter().rev() {
        html.push_str(close);
    }
}

/// `body[range]` as HTML text, `\n` as `<br>`.
fn push_escaped_html(html: &mut String, body: &str, range: Range<usize>) {
    for (index, c) in body[range.clone()].char_indices() {
        match c {
            '&' => html.push_str("&amp;"),
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            '"' => html.push_str("&quot;"),
            '\n' => html.push_str("<br>"),
            ' ' if space_needs_nbsp(body, range.start + index) => html.push_str("&nbsp;"),
            c => html.push(c),
        }
    }
}

/// Whether the space at `body[index]` would be lost in HTML, which
/// collapses a run of spaces to one and drops spaces at a line's start and
/// end: one that starts or ends a line, or follows another space. A run of
/// n spaces after a word is then one space and n - 1 `&nbsp;`, which reads
/// back as n spaces.
fn space_needs_nbsp(body: &str, index: usize) -> bool {
    let is_line_edge = |c: Option<char>| c.is_none_or(|c| c == '\n');
    let previous = body[..index].chars().next_back();
    let next = body[index + 1..].chars().next();
    is_line_edge(previous) || previous == Some(' ') || is_line_edge(next)
}

const BLACK: TextColor = TextColor {
    red: 0,
    green: 0,
    blue: 0,
};

/// An HTML colour attribute, `#rgb` or `#rrggbb` in either case. Anything
/// else is not a colour: the attribute is the sender's input.
fn parse_html_color(value: &str) -> Option<TextColor> {
    let hex = value.trim().strip_prefix('#')?;
    if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |index: usize| u8::from_str_radix(&hex[index..=index], 16).expect("hex digit");
    match hex.len() {
        3 => Some(TextColor {
            red: digit(0) * 17,
            green: digit(1) * 17,
            blue: digit(2) * 17,
        }),
        6 => Some(TextColor {
            red: digit(0) * 16 + digit(1),
            green: digit(2) * 16 + digit(3),
            blue: digit(4) * 16 + digit(5),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color(red: u8, green: u8, blue: u8) -> TextColor {
        TextColor { red, green, blue }
    }

    fn bold() -> TextStyle {
        TextStyle {
            bold: true,
            ..TextStyle::default()
        }
    }

    fn italic() -> TextStyle {
        TextStyle {
            italic: true,
            ..TextStyle::default()
        }
    }

    fn foreground(color: TextColor) -> TextStyle {
        TextStyle {
            foreground: Some(color),
            ..TextStyle::default()
        }
    }

    /// The text with each span as `[text]` and its styles listed after.
    fn parse(html: &str) -> (String, Vec<(String, TextStyle)>) {
        let text = styled_text_from_html(html);
        assert_eq!(
            check_text_styles(&text.body, &text.styles),
            Ok(()),
            "{html}"
        );
        let spans = text
            .styles
            .iter()
            .map(|span| (text.body[span.start..span.end].to_owned(), span.style))
            .collect();
        (text.body, spans)
    }

    fn plain_text(html: &str) -> String {
        let (body, spans) = parse(html);
        assert_eq!(spans, Vec::new(), "{html}");
        body
    }

    #[test]
    fn inline_elements_map_to_styles() {
        let underline = TextStyle {
            underline: true,
            ..TextStyle::default()
        };
        let strike = TextStyle {
            strikethrough: true,
            ..TextStyle::default()
        };
        for (html, style) in [
            ("<b>x</b>", bold()),
            ("<strong>x</strong>", bold()),
            ("<i>x</i>", italic()),
            ("<em>x</em>", italic()),
            ("<u>x</u>", underline),
            ("<del>x</del>", strike),
            ("<s>x</s>", strike),
            ("<strike>x</strike>", strike),
        ] {
            assert_eq!(
                parse(&format!("a {html} b")),
                ("a x b".to_owned(), vec![("x".to_owned(), style)]),
                "{html}"
            );
        }
    }

    #[test]
    fn nested_styles_combine_and_equal_neighbours_merge() {
        assert_eq!(
            parse("<b><i>x</i></b>"),
            (
                "x".to_owned(),
                vec![(
                    "x".to_owned(),
                    TextStyle {
                        bold: true,
                        italic: true,
                        ..TextStyle::default()
                    }
                )]
            )
        );
        assert_eq!(
            parse("<b>al</b><strong>pha</strong> <b>beta</b>"),
            (
                "alpha beta".to_owned(),
                vec![("alpha".to_owned(), bold()), ("beta".to_owned(), bold())]
            )
        );
        assert_eq!(
            parse("<b>one <i>two</i> three</b>"),
            (
                "one two three".to_owned(),
                vec![
                    ("one ".to_owned(), bold()),
                    (
                        "two".to_owned(),
                        TextStyle {
                            bold: true,
                            italic: true,
                            ..TextStyle::default()
                        }
                    ),
                    (" three".to_owned(), bold()),
                ]
            )
        );
    }

    #[test]
    fn colours_come_from_font_and_span_attributes() {
        let red = color(0xff, 0, 0);
        assert_eq!(
            parse(r##"<font color="#FF0000">r</font>"##).1,
            vec![("r".to_owned(), foreground(red))]
        );
        assert_eq!(
            parse(r##"<font data-mx-color="#ff0000">r</font>"##).1,
            vec![("r".to_owned(), foreground(red))]
        );
        assert_eq!(
            parse(r##"<span data-mx-color="#f00" data-mx-bg-color="#00007F">r</span>"##).1,
            vec![(
                "r".to_owned(),
                TextStyle {
                    foreground: Some(red),
                    background: Some(color(0, 0, 0x7f)),
                    ..TextStyle::default()
                }
            )]
        );
        assert_eq!(
            parse(r##"<font data-mx-bg-color="#abc">r</font>"##).1,
            vec![(
                "r".to_owned(),
                TextStyle {
                    background: Some(color(0xaa, 0xbb, 0xcc)),
                    ..TextStyle::default()
                }
            )]
        );
        for malformed in ["red", "#ff00", "#gg0000", "ff0000", "#ff00000", ""] {
            assert_eq!(
                plain_text(&format!(r#"<span data-mx-color="{malformed}">r</span>"#)),
                "r",
                "{malformed}"
            );
        }
        // Colours on other elements are not Matrix's.
        assert_eq!(
            parse(r##"<b data-mx-color="#ff0000">r</b>"##).1,
            vec![("r".to_owned(), bold())]
        );
    }

    #[test]
    fn spoilers_are_black_on_black() {
        let black = color(0, 0, 0);
        assert_eq!(
            parse(r#"the end: <span data-mx-spoiler="plot">they win</span>"#),
            (
                "the end: they win".to_owned(),
                vec![(
                    "they win".to_owned(),
                    TextStyle {
                        foreground: Some(black),
                        background: Some(black),
                        ..TextStyle::default()
                    }
                )]
            )
        );
    }

    #[test]
    fn entities_and_whitespace_read_as_html_does() {
        assert_eq!(
            plain_text("a &amp; b &lt;c&gt; &quot;d&quot;"),
            r#"a & b <c> "d""#
        );
        assert_eq!(plain_text("  one \n\t two   three  "), "one two three");
        assert_eq!(plain_text("<p>  padded  </p>"), "padded");
        assert_eq!(plain_text("a&#13;b"), "a b");
    }

    #[test]
    fn line_breaks_and_blocks_become_lines() {
        assert_eq!(plain_text("one<br>two<br/>three"), "one\ntwo\nthree");
        assert_eq!(plain_text("one<br><br>two"), "one\n\ntwo");
        assert_eq!(plain_text("<br>one<br>"), "one");
        assert_eq!(plain_text("<p>a</p><p>b</p>"), "a\nb");
        assert_eq!(
            plain_text("before<div>inside</div>after"),
            "before\ninside\nafter"
        );
        assert_eq!(plain_text("<p>a</p>\n\n<p>b</p>\n"), "a\nb");
        assert_eq!(plain_text("a<hr>b"), "a\n---\nb");
        assert_eq!(
            plain_text("<table><tr><th>k</th><th>v</th></tr><tr><td>a</td><td>1</td></tr></table>"),
            "k v\na 1"
        );
    }

    #[test]
    fn headings_are_bold_lines() {
        assert_eq!(
            parse("<h2>Title</h2>text"),
            ("Title\ntext".to_owned(), vec![("Title".to_owned(), bold())])
        );
    }

    #[test]
    fn lists_are_marked_and_nested_lists_indented() {
        assert_eq!(
            plain_text("<ul><li>one</li><li>two</li></ul>"),
            "* one\n* two"
        );
        assert_eq!(
            plain_text("<ol start=\"3\"><li>three</li><li>four</li></ol>"),
            "3. three\n4. four"
        );
        assert_eq!(
            plain_text("intro<ol><li>a<ul><li>b</li><li>c</li></ul></li><li>d</li></ol>end"),
            "intro\n1. a\n  * b\n  * c\n2. d\nend"
        );
    }

    #[test]
    fn quotes_prefix_their_lines() {
        assert_eq!(
            plain_text("<blockquote>one<br>two</blockquote>reply"),
            "> one\n> two\nreply"
        );
        assert_eq!(
            plain_text("<blockquote>outer<blockquote>inner</blockquote></blockquote>"),
            "> outer\n> > inner"
        );
    }

    #[test]
    fn preformatted_text_keeps_its_whitespace_and_lines() {
        assert_eq!(
            plain_text("<pre><code>fn main() {\n    let  x = 1;\n\n}\n</code></pre>after"),
            "fn main() {\n    let  x = 1;\n\n}\nafter"
        );
    }

    #[test]
    fn links_show_their_url_unless_the_text_says_it() {
        assert_eq!(
            plain_text(r#"see <a href="https://example.org/docs">docs</a> now"#),
            "see docs (https://example.org/docs) now"
        );
        assert_eq!(
            plain_text(r#"<a href="https://example.org">https://example.org</a>"#),
            "https://example.org"
        );
        assert_eq!(
            plain_text(r#"<a href="mailto:ops@example.org">ops@example.org</a>"#),
            "ops@example.org"
        );
        assert_eq!(
            plain_text(r#"hi <a href="https://matrix.to/#/@alice:example.org">Alice</a>"#),
            "hi Alice"
        );
        assert_eq!(
            plain_text(r#"<a href="https://example.org/x"></a>"#),
            "https://example.org/x"
        );
        // Schemes the timeline does not show as URLs, and ones the
        // sanitizer drops with the link.
        assert_eq!(
            plain_text(r#"<a href="ftp://example.org">files</a>"#),
            "files"
        );
        assert_eq!(plain_text(r#"<a href="javascript:alert(1)">x</a>"#), "x");
        assert_eq!(
            parse(r#"<b><a href="https://example.org">site</a></b>"#),
            (
                "site (https://example.org)".to_owned(),
                vec![("site (https://example.org)".to_owned(), bold())]
            )
        );
    }

    #[test]
    fn images_show_their_alt_text() {
        assert_eq!(
            plain_text(r#"hi <img src="mxc://example.org/wave" alt=":wave:"> there"#),
            "hi :wave: there"
        );
        assert_eq!(
            plain_text(r#"<img src="mxc://example.org/x" title="logo">"#),
            "logo"
        );
        assert_eq!(plain_text(r#"<img src="mxc://example.org/x">"#), "image");
    }

    #[test]
    fn reply_fallbacks_and_disallowed_elements_are_removed() {
        assert_eq!(
            plain_text(
                "<mx-reply><blockquote><a href=\"https://matrix.to/#/!r/$e\">In reply to</a> \
                 <a href=\"https://matrix.to/#/@a:b\">@a:b</a><br>original</blockquote></mx-reply>\
                 the answer"
            ),
            "the answer"
        );
        assert_eq!(plain_text("<mx-reply>only a fallback</mx-reply>"), "");
        assert_eq!(plain_text("<!-- note -->kept"), "kept");
        assert_eq!(
            plain_text("<script>alert(1)</script><style>p {}</style>shown"),
            "shown"
        );
        assert_eq!(
            plain_text("<marquee>moving</marquee> <iframe src=\"https://x\"></iframe>text"),
            "moving text"
        );
    }

    #[test]
    fn styles_never_cross_lines() {
        assert_eq!(
            parse("<b>one<br>two</b>"),
            (
                "one\ntwo".to_owned(),
                vec![("one".to_owned(), bold()), ("two".to_owned(), bold())]
            )
        );
        assert_eq!(
            parse("<b>one </b><p><b>two</b></p>"),
            (
                "one\ntwo".to_owned(),
                vec![("one".to_owned(), bold()), ("two".to_owned(), bold())]
            )
        );
    }

    #[test]
    fn offsets_are_utf8_bytes_on_char_boundaries() {
        let text = styled_text_from_html("héllo <b>wörld</b> 👋<i>ü</i>");
        assert_eq!(text.body, "héllo wörld 👋ü");
        assert_eq!(
            text.styles,
            vec![
                TextStyleSpan {
                    start: 7,
                    end: 13,
                    style: bold()
                },
                TextStyleSpan {
                    start: 18,
                    end: 20,
                    style: italic()
                },
            ]
        );
    }

    #[test]
    fn markup_without_text_is_empty() {
        assert_eq!(plain_text(""), "");
        assert_eq!(plain_text("<p> </p><br>"), "");
    }

    fn span(start: usize, end: usize, style: TextStyle) -> TextStyleSpan {
        TextStyleSpan { start, end, style }
    }

    #[test]
    fn styled_text_becomes_flat_matrix_html() {
        assert_eq!(html_from_styled_text("plain", &[], &[]), "plain");
        assert_eq!(
            html_from_styled_text("a & <b> \"c\"\nnext", &[], &[]),
            "a &amp; &lt;b&gt; &quot;c&quot;<br>next"
        );
        let everything = TextStyle {
            bold: true,
            italic: true,
            underline: true,
            strikethrough: true,
            foreground: Some(color(0xff, 0, 0)),
            background: Some(color(0, 0, 0x7f)),
        };
        assert_eq!(
            html_from_styled_text("all of it", &[span(4, 6, everything)], &[]),
            "all <span data-mx-color=\"#ff0000\" data-mx-bg-color=\"#00007f\">\
             <b><i><u><del>of</del></u></i></b></span> it"
        );
        assert_eq!(
            html_from_styled_text(
                "bold, red bg <tag>",
                &[
                    span(0, 4, bold()),
                    span(6, 9, foreground(color(0xff, 0, 0))),
                    span(
                        10,
                        18,
                        TextStyle {
                            background: Some(color(0, 0, 0)),
                            ..TextStyle::default()
                        }
                    ),
                ],
                &[]
            ),
            "<b>bold</b>, <span data-mx-color=\"#ff0000\">red</span> \
             <span data-mx-bg-color=\"#000000\">bg &lt;tag&gt;</span>"
        );
        // Without a colour there is no span.
        assert_eq!(
            html_from_styled_text("x", &[span(0, 1, italic())], &[]),
            "<i>x</i>"
        );
    }

    fn pill(start: usize, end: usize, user_id: &str) -> UserLinkSpan {
        UserLinkSpan {
            start,
            end,
            user_id: user_id.to_owned(),
        }
    }

    #[test]
    fn user_ids_link_to_their_matrix_to_permalink() {
        let alice = <&UserId>::try_from("@alice:example.org").unwrap();
        assert_eq!(
            alice.matrix_to_uri().to_string(),
            "https://matrix.to/#/@alice:example.org"
        );
    }

    #[test]
    fn pills_link_their_text_to_the_user() {
        assert_eq!(
            html_from_styled_text(
                "Alice: ask Bob&Co",
                &[],
                &[
                    pill(0, 5, "@alice:example.org"),
                    pill(11, 17, "@bob:example.org")
                ]
            ),
            "<a href=\"https://matrix.to/#/@alice:example.org\">Alice</a>: ask \
             <a href=\"https://matrix.to/#/@bob:example.org\">Bob&amp;Co</a>"
        );
        assert_eq!(
            html_from_styled_text(
                "AB",
                &[],
                &[pill(0, 1, "@a:example.org"), pill(1, 2, "@b:example.org")]
            ),
            "<a href=\"https://matrix.to/#/@a:example.org\">A</a>\
             <a href=\"https://matrix.to/#/@b:example.org\">B</a>",
            "touching pills stay two links"
        );
    }

    #[test]
    fn styles_crossing_a_pill_edge_nest_inside_and_outside_it() {
        assert_eq!(
            html_from_styled_text(
                "hey Alice ok",
                &[span(0, 6, bold()), span(6, 12, italic())],
                &[pill(4, 9, "@alice:example.org")]
            ),
            "<b>hey </b><a href=\"https://matrix.to/#/@alice:example.org\"><b>Al</b>\
             <i>ice</i></a><i> ok</i>"
        );
        assert_eq!(
            html_from_styled_text(
                "x Alice y",
                &[span(0, 9, foreground(color(0xff, 0, 0)))],
                &[pill(2, 7, "@alice:example.org")]
            ),
            "<span data-mx-color=\"#ff0000\">x </span>\
             <a href=\"https://matrix.to/#/@alice:example.org\">\
             <span data-mx-color=\"#ff0000\">Alice</span></a>\
             <span data-mx-color=\"#ff0000\"> y</span>"
        );
    }

    #[test]
    fn spaces_html_would_lose_are_no_break_spaces() {
        let html = |body: &str| html_from_styled_text(body, &[], &[]);
        assert_eq!(html("a b"), "a b");
        assert_eq!(html("a  b"), "a &nbsp;b");
        assert_eq!(html("a    b"), "a &nbsp;&nbsp;&nbsp;b");
        assert_eq!(html(" a"), "&nbsp;a");
        assert_eq!(html("a "), "a&nbsp;");
        assert_eq!(html("a\n  b \nc"), "a<br>&nbsp;&nbsp;b&nbsp;<br>c");
        assert_eq!(html(" "), "&nbsp;");
        // Across element edges, the body decides.
        assert_eq!(
            html_from_styled_text("a  b", &[span(2, 4, bold())], &[]),
            "a <b>&nbsp;b</b>"
        );
    }

    #[test]
    fn no_break_spaces_read_as_spaces_html_keeps() {
        assert_eq!(plain_text("a &nbsp;&nbsp;b"), "a   b");
        assert_eq!(plain_text("&nbsp;a&nbsp;"), " a ");
        assert_eq!(plain_text("a\u{a0}b"), "a b");
        assert_eq!(plain_text("<p>&nbsp;</p>x"), " \nx");
    }

    #[test]
    #[should_panic(expected = "styles sent must describe their body")]
    fn styled_text_html_refuses_invalid_styles() {
        html_from_styled_text("x", &[span(0, 2, bold())], &[]);
    }

    #[test]
    fn styled_text_round_trips_through_html() {
        let styles = [
            TextStyle::default(),
            bold(),
            italic(),
            TextStyle {
                underline: true,
                strikethrough: true,
                ..TextStyle::default()
            },
            foreground(color(0x12, 0x34, 0x56)),
            TextStyle {
                bold: true,
                foreground: Some(color(0xff, 0xff, 0xff)),
                background: Some(color(0, 0, 0)),
                ..TextStyle::default()
            },
        ];
        let words = ["héllo", "<w&>", "x", "👋", "two words", "\"q\"", " pad  "];
        for (first, first_style) in styles.iter().enumerate() {
            for (second, second_style) in styles.iter().enumerate() {
                for separator in [" ", "", "\n", "   ", " \n ", "\n\n"] {
                    let mut body = String::new();
                    let mut spans = Vec::new();
                    for (word, style) in
                        [(words[first], first_style), (words[second], second_style)]
                    {
                        if !body.is_empty() {
                            body.push_str(separator);
                        }
                        let start = body.len();
                        body.push_str(word);
                        if *style != TextStyle::default() {
                            match spans.last_mut() {
                                Some(TextStyleSpan {
                                    end, style: last, ..
                                }) if *end == start && last == style => *end = body.len(),
                                _ => spans.push(span(start, body.len(), *style)),
                            }
                        }
                    }
                    let html = html_from_styled_text(&body, &spans, &[]);
                    assert_eq!(
                        styled_text_from_html(&html),
                        StyledText {
                            body: body.clone(),
                            styles: spans.clone()
                        },
                        "{html}"
                    );
                }
            }
        }
    }
}
