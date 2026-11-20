//! mIRC's "Hotlinks": the web addresses and Matrix links in a line of text.
//! Pure; the timeline points the hand at them and opens them.

use std::ops::Range;

use mirx_core::{MatrixLink, matrix_link};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LinkKind {
    /// A web address, as the shell opens it: `www.` gains `http://`.
    Web(String),
    Matrix(MatrixLink),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Link {
    /// Where the link is in the text it was found in.
    pub(super) range: Range<usize>,
    pub(super) kind: LinkKind,
}

const WEB_PREFIXES: [&str; 4] = ["https://", "http://", "ftp://", "www."];

/// Whether `text` starts with `prefix`, ignoring ASCII case as mIRC does
/// for `HTTP://`.
fn starts_with_ignoring_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

/// The length of a link candidate at the start of `text`: up to whitespace
/// or `<>"`, without trailing punctuation that ends a sentence or a
/// clause. A trailing `)` is the text's, as in `label (url)` or mirx's own
/// `(!room:server)`, unless the link opened it (`/wiki/Foo_(bar)`).
fn candidate_len(text: &str) -> usize {
    let end = text
        .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"'))
        .unwrap_or(text.len());
    let mut candidate = &text[..end];
    loop {
        let trimmed = candidate.trim_end_matches(['.', ',', ';', ':', '!', '?', '\'']);
        let trimmed = match trimmed.strip_suffix(')') {
            Some(inner) if inner.matches('(').count() < trimmed.matches(')').count() => inner,
            _ => trimmed,
        };
        if trimmed.len() == candidate.len() {
            return trimmed.len();
        }
        candidate = trimmed;
    }
}

/// The link starting at the start of `text`, if one does, and its length.
fn link_at_start(text: &str) -> Option<(usize, LinkKind)> {
    let len = candidate_len(text);
    let candidate = &text[..len];
    if let Some(prefix) = WEB_PREFIXES
        .iter()
        .find(|prefix| starts_with_ignoring_case(candidate, prefix))
    {
        if candidate.len() == prefix.len() {
            return None;
        }
        // A matrix.to permalink opens in mirx; any other address, a
        // malformed permalink included, in the browser.
        let kind = match matrix_link(candidate) {
            Some(link) => LinkKind::Matrix(link),
            None if *prefix == "www." => LinkKind::Web(format!("http://{candidate}")),
            None => LinkKind::Web(candidate.to_owned()),
        };
        return Some((len, kind));
    }
    // `matrix:` URIs and bare `#alias:server` / `!id:server` are links
    // only when they parse.
    let matrix_shaped =
        starts_with_ignoring_case(candidate, "matrix:") || candidate.starts_with(['#', '!']);
    matrix_shaped
        .then(|| matrix_link(candidate))
        .flatten()
        .map(|link| (len, LinkKind::Matrix(link)))
}

/// The links in `text`, in order. A link starts at a word: the start of the
/// text, or after a character that is neither alphanumeric nor `/`, so
/// `C#` and `issue#12` are not links.
fn links(text: &str) -> Vec<Link> {
    let mut links = Vec::new();
    let mut previous: Option<char> = None;
    let mut next_start = 0;
    for (index, ch) in text.char_indices() {
        let at_word_start = previous.is_none_or(|c| !c.is_alphanumeric() && c != '/');
        previous = Some(ch);
        if index < next_start || !at_word_start {
            continue;
        }
        if let Some((len, kind)) = link_at_start(&text[index..]) {
            links.push(Link {
                range: index..index + len,
                kind,
            });
            next_start = index + len;
        }
    }
    links
}

/// Where a row's text links to a user: its sender's nick, or a pill in its
/// message. `range` is in the row's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RowUserLink {
    pub(super) range: Range<usize>,
    pub(super) user_id: String,
}

/// The links of a row's `text`, in order: its `user_links` (sorted and
/// apart), and the links found in its text that overlap none of them, so
/// a pill or nick wins over the address-like text it shows.
pub(super) fn row_links(text: &str, user_links: &[RowUserLink]) -> Vec<Link> {
    let overlaps_a_user_link = |range: &Range<usize>| {
        user_links
            .iter()
            .any(|user| user.range.start < range.end && range.start < user.range.end)
    };
    let mut row_links: Vec<Link> = links(text)
        .into_iter()
        .filter(|link| !overlaps_a_user_link(&link.range))
        .chain(user_links.iter().map(|user| Link {
            range: user.range.clone(),
            kind: LinkKind::Matrix(MatrixLink::User {
                user_id: user.user_id.clone(),
            }),
        }))
        .collect();
    row_links.sort_by_key(|link| link.range.start);
    row_links
}

/// The link of a row's `text` (see [`row_links`]) that the byte at
/// `offset` is in.
pub(super) fn row_link_at(text: &str, user_links: &[RowUserLink], offset: usize) -> Option<Link> {
    row_links(text, user_links)
        .into_iter()
        .find(|link| link.range.contains(&offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each link's text and kind.
    fn found(text: &str) -> Vec<(&str, LinkKind)> {
        links(text)
            .into_iter()
            .map(|link| (&text[link.range], link.kind))
            .collect()
    }

    fn web(url: &str) -> LinkKind {
        LinkKind::Web(url.to_owned())
    }

    fn room(target: &str) -> LinkKind {
        LinkKind::Matrix(MatrixLink::Room {
            room_id_or_alias: target.to_owned(),
            via: Vec::new(),
        })
    }

    #[test]
    fn web_addresses_end_before_the_punctuation_around_them() {
        assert_eq!(
            found("see docs (https://example.org/docs)"),
            vec![("https://example.org/docs", web("https://example.org/docs"))]
        );
        assert_eq!(
            found("https://en.wikipedia.org/wiki/Foo_(bar) is it"),
            vec![(
                "https://en.wikipedia.org/wiki/Foo_(bar)",
                web("https://en.wikipedia.org/wiki/Foo_(bar)")
            )]
        );
        assert_eq!(
            found("(see https://en.wikipedia.org/wiki/Foo_(bar))."),
            vec![(
                "https://en.wikipedia.org/wiki/Foo_(bar)",
                web("https://en.wikipedia.org/wiki/Foo_(bar)")
            )]
        );
        assert_eq!(
            found("mirror on www.example.org."),
            vec![("www.example.org", web("http://www.example.org"))]
        );
        assert_eq!(
            found("ftp://files.example.org/a, then 'HTTP://EXAMPLE.ORG'!"),
            vec![
                (
                    "ftp://files.example.org/a",
                    web("ftp://files.example.org/a")
                ),
                ("HTTP://EXAMPLE.ORG", web("HTTP://EXAMPLE.ORG")),
            ]
        );
        assert_eq!(
            found("<https://example.org/q?a=\"b\"> ok"),
            vec![("https://example.org/q?a=", web("https://example.org/q?a="))]
        );
        assert_eq!(found("https:// and www. alone"), vec![]);
    }

    #[test]
    fn matrix_links_are_matrix_links() {
        assert_eq!(
            found("join https://matrix.to/#/#rust:matrix.org now"),
            vec![(
                "https://matrix.to/#/#rust:matrix.org",
                room("#rust:matrix.org")
            )]
        );
        assert_eq!(
            found("or https://matrix.to/#/%23rust%3Amatrix.org."),
            vec![(
                "https://matrix.to/#/%23rust%3Amatrix.org",
                room("#rust:matrix.org")
            )]
        );
        assert_eq!(
            found("ask https://matrix.to/#/@bob:example"),
            vec![(
                "https://matrix.to/#/@bob:example",
                LinkKind::Matrix(MatrixLink::User {
                    user_id: "@bob:example".to_owned()
                })
            )]
        );
        assert_eq!(
            found("matrix:r/rust:matrix.org and matrix:nonsense"),
            vec![("matrix:r/rust:matrix.org", room("#rust:matrix.org"))]
        );
        // A permalink to nothing mirx follows opens in the browser.
        assert_eq!(
            found("https://matrix.to/#/garbage"),
            vec![(
                "https://matrix.to/#/garbage",
                web("https://matrix.to/#/garbage")
            )]
        );
    }

    #[test]
    fn bare_rooms_need_a_server_and_a_word_start() {
        assert_eq!(
            found("on #rust:matrix.org, as always"),
            vec![("#rust:matrix.org", room("#rust:matrix.org"))]
        );
        assert_eq!(
            found("* Now talking in Ops (!ops:example)"),
            vec![("!ops:example", room("!ops:example"))]
        );
        assert_eq!(found("C# is not a link, nor is issue#12"), vec![]);
        assert_eq!(found("#nocolon and wow!! and !important"), vec![]);
    }

    #[test]
    fn a_link_is_found_once_in_its_row_whatever_its_lines() {
        let text = "[22:17] <Bob> logs: https://example.org/a/very/long/path then more";
        let links = links(text);
        assert_eq!(links.len(), 1);
        assert_eq!(
            &text[links[0].range.clone()],
            "https://example.org/a/very/long/path"
        );
        let start = links[0].range.start;
        assert_eq!(row_link_at(text, &[], start - 1), None);
        assert_eq!(row_link_at(text, &[], start), Some(links[0].clone()));
        assert_eq!(
            row_link_at(text, &[], links[0].range.end - 1),
            Some(links[0].clone())
        );
        assert_eq!(row_link_at(text, &[], links[0].range.end), None);
    }

    fn user_link(text: &str, part: &str, user_id: &str) -> RowUserLink {
        let start = text.find(part).expect("part of the text");
        RowUserLink {
            range: start..start + part.len(),
            user_id: user_id.to_owned(),
        }
    }

    fn user(user_id: &str) -> LinkKind {
        LinkKind::Matrix(MatrixLink::User {
            user_id: user_id.to_owned(),
        })
    }

    #[test]
    fn row_links_merge_user_links_with_the_text_links_they_do_not_overlap() {
        let text = "<www.bob.example> see www.example.org, Alice and #ops:example";
        let user_links = [
            user_link(text, "www.bob.example", "@bob:example"),
            user_link(text, "Alice", "@alice:example"),
        ];
        let found: Vec<(&str, LinkKind)> = row_links(text, &user_links)
            .into_iter()
            .map(|link| (&text[link.range], link.kind))
            .collect();
        assert_eq!(
            found,
            vec![
                ("www.bob.example", user("@bob:example")),
                ("www.example.org", web("http://www.example.org")),
                ("Alice", user("@alice:example")),
                ("#ops:example", room("#ops:example")),
            ]
        );
    }

    #[test]
    fn row_link_at_finds_user_links() {
        let text = "[12:00] <Alice> hi Bob";
        let user_links = [
            user_link(text, "Alice", "@alice:example"),
            user_link(text, "Bob", "@bob:example"),
        ];
        let at = |offset| row_link_at(text, &user_links, offset).map(|link| link.kind);
        assert_eq!(at(8), None, "the nick's < is not the nick");
        assert_eq!(at(9), Some(user("@alice:example")));
        assert_eq!(at(13), Some(user("@alice:example")));
        assert_eq!(at(14), None);
        assert_eq!(at(19), Some(user("@bob:example")));
        assert_eq!(at(21), Some(user("@bob:example")));
    }
}
