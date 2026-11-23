//! Marking text in a timeline, as mIRC's "Text Copy and Paste": where a
//! point falls in the rows' text, what a mark between two points copies,
//! and which part of each line shows it. Pure: measuring is passed in.

use std::ops::Range;

use super::timeline_layout::{TimelinePaintLine, TimelinePaintLineVisual};
use super::{TimelineRow, TimelineRowLayout};

/// A place in a timeline's text: a byte offset, at a char boundary, in a
/// row's `logical_text`. Ordered as the text reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct TextPoint {
    pub(super) row: usize,
    pub(super) offset: usize,
}

/// Where a timeline's lines are on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LineGeometry {
    pub(super) top_line: usize,
    pub(super) visible_rows: usize,
    pub(super) line_height: i32,
    pub(super) vertical_padding: i32,
    /// Between the client's left edge and a line's text, before its indent.
    pub(super) left_padding: i32,
}

/// A drag of the left button marking text, from `anchor` to `focus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TextDrag {
    pub(super) anchor: TextPoint,
    pub(super) focus: TextPoint,
    /// Where the button went down, in client pixels.
    pub(super) down: (i32, i32),
    /// Whether the pointer has left the system's drag box around `down`:
    /// until then the press is a click, which marks nothing.
    pub(super) moved: bool,
}

impl TextDrag {
    pub(super) fn new(point: TextPoint, down: (i32, i32)) -> Self {
        Self {
            anchor: point,
            focus: point,
            down,
            moved: false,
        }
    }

    /// The mark to show and copy: a moved drag's, if it covers any text.
    pub(super) fn mark(&self) -> Option<(TextPoint, TextPoint)> {
        (self.moved && self.anchor != self.focus).then_some((self.anchor, self.focus))
    }
}

/// Whether the pointer at `at` is outside the `drag_box` (`SM_CXDRAG`,
/// `SM_CYDRAG`) centred on `down`.
pub(super) fn left_drag_box(down: (i32, i32), at: (i32, i32), drag_box: (i32, i32)) -> bool {
    (at.0 - down.0).abs() > drag_box.0 / 2 || (at.1 - down.1).abs() > drag_box.1 / 2
}

/// The char boundary of `text` nearest `x`, measured from the text's left:
/// a point left of a character's middle is before it. 0 left of the text,
/// `text.len()` right of it.
pub(super) fn offset_at_x(text: &str, x: i32, measure: &dyn Fn(&str) -> i32) -> usize {
    let mut before = 0;
    for (index, ch) in text.char_indices() {
        let after = measure(&text[..index + ch.len_utf8()]);
        if x < (before + after) / 2 {
            return index;
        }
        before = after;
    }
    text.len()
}

/// The start of the character of `text` under `x`, if one is.
fn char_at_x(text: &str, x: i32, measure: &dyn Fn(&str) -> i32) -> Option<usize> {
    if x < 0 {
        return None;
    }
    text.char_indices()
        .find(|(index, ch)| x < measure(&text[..index + ch.len_utf8()]))
        .map(|(index, _)| index)
}

/// The visible line at client `y`, clamped to the visible lines: `None`
/// only when none is visible.
fn visible_line_at(lines_len: usize, geometry: LineGeometry, y: i32) -> Option<ClampedLine> {
    let visible = lines_len
        .saturating_sub(geometry.top_line)
        .min(geometry.visible_rows);
    if visible == 0 {
        return None;
    }
    let row = (y - geometry.vertical_padding).div_euclid(geometry.line_height);
    Some(if row < 0 {
        ClampedLine::Above(geometry.top_line)
    } else if row as usize >= visible {
        ClampedLine::Below(geometry.top_line + visible - 1)
    } else {
        ClampedLine::On(geometry.top_line + row as usize)
    })
}

enum ClampedLine {
    Above(usize),
    On(usize),
    Below(usize),
}

/// The text line, its row and the range of the row's text it shows.
fn text_line(line: &TimelinePaintLine) -> Option<(usize, &str, i32, &Range<usize>)> {
    match &line.visual {
        TimelinePaintLineVisual::Text {
            text,
            indent,
            range,
            ..
        } => Some((line.source_row_index?, text.as_str(), *indent, range)),
        TimelinePaintLineVisual::ImageSlot | TimelinePaintLineVisual::Rule => None,
    }
}

/// Where client `x`, `y` falls in the rows' text, as a drag marks it: above
/// the visible lines is the start of the first, below them the end of the
/// last, and an image's lines are the end of its row's text. `None` where
/// no row is: a timeline set as plain text.
pub(super) fn point_at(
    lines: &[TimelinePaintLine],
    geometry: LineGeometry,
    x: i32,
    y: i32,
    measure: &dyn Fn(&str) -> i32,
) -> Option<TextPoint> {
    let (index, place) = match visible_line_at(lines.len(), geometry, y)? {
        ClampedLine::Above(index) => (index, Some(false)),
        ClampedLine::On(index) => (index, None),
        ClampedLine::Below(index) => (index, Some(true)),
    };
    let line = &lines[index];
    let row = line.source_row_index?;
    if line.visual == TimelinePaintLineVisual::Rule {
        // The line marker's row has no text: a point on it is its start.
        return Some(TextPoint { row, offset: 0 });
    }
    let Some((_, text, indent, range)) = text_line(line) else {
        let (_, _, _, range) = lines[..index]
            .iter()
            .rev()
            .filter(|line| line.source_row_index == Some(row))
            .find_map(text_line)
            .expect("an image's row has text above its image");
        return Some(TextPoint {
            row,
            offset: range.end,
        });
    };
    let offset = match place {
        Some(false) => range.start,
        Some(true) => range.end,
        None => range.start + offset_at_x(text, x - geometry.left_padding - indent, measure),
    };
    Some(TextPoint { row, offset })
}

/// The character under client `x`, `y`, if a row's text is there: what the
/// pointer is on, for hotlinks.
pub(super) fn exact_point_at(
    lines: &[TimelinePaintLine],
    geometry: LineGeometry,
    x: i32,
    y: i32,
    measure: &dyn Fn(&str) -> i32,
) -> Option<TextPoint> {
    let ClampedLine::On(index) = visible_line_at(lines.len(), geometry, y)? else {
        return None;
    };
    let (row, text, indent, range) = text_line(&lines[index])?;
    let offset = char_at_x(text, x - geometry.left_padding - indent, measure)?;
    Some(TextPoint {
        row,
        offset: range.start + offset,
    })
}

/// The text between `a` and `b` in either order, rows joined by CRLF as
/// the clipboard takes lines. A wrapped row is one line, so what wrapping
/// dropped comes back. The line marker is no line of text and copies as
/// nothing. Styles are not text: they copy as nothing too.
pub(super) fn selected_text(rows: &[TimelineRow], a: TextPoint, b: TextPoint) -> String {
    let (start, end) = (a.min(b), a.max(b));
    (start.row..=end.row)
        .filter(|index| rows[*index].layout != TimelineRowLayout::Rule)
        .map(|index| {
            let row = &rows[index];
            let from = if index == start.row { start.offset } else { 0 };
            let to = if index == end.row {
                end.offset
            } else {
                row.logical_text.len()
            };
            row.logical_text[from..to].replace('\n', "\r\n")
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// The part of a line showing `range` of row `row`'s text that the mark
/// between `a` and `b` covers, at offsets in the line's text.
pub(super) fn line_selection(
    row: usize,
    range: &Range<usize>,
    a: TextPoint,
    b: TextPoint,
) -> Option<Range<usize>> {
    let (start, end) = (a.min(b), a.max(b));
    if row < start.row || row > end.row {
        return None;
    }
    let from = if row == start.row { start.offset } else { 0 };
    let to = if row == end.row {
        end.offset
    } else {
        usize::MAX
    };
    let from = from.max(range.start);
    let to = to.min(range.end);
    (from < to).then(|| from - range.start..to - range.start)
}

/// `point` in `new_rows`, the rows that replaced `old_rows`: the same row
/// found by identity. `None` if the row is gone or its text changed (an
/// edit), which ends the drag.
pub(super) fn remap(
    point: TextPoint,
    old_rows: &[TimelineRow],
    new_rows: &[TimelineRow],
) -> Option<TextPoint> {
    let old = &old_rows[point.row];
    let row = new_rows
        .iter()
        .position(|row| row.identity == old.identity)?;
    (new_rows[row].logical_text == old.logical_text).then_some(TextPoint {
        row,
        offset: point.offset,
    })
}

#[cfg(test)]
mod tests {
    use mirx_core::{TextColor, TextStyle, TextStyleSpan};

    use super::super::timeline_layout::TimelinePaintLineVisual;
    use super::super::{EventItem, TimelineRowIdentity, TimelineRowKind};
    use super::*;

    /// A monospace font: 10 pixels a character, whatever its bytes.
    fn mono(text: &str) -> i32 {
        text.chars().count() as i32 * 10
    }

    fn row(text: &str, activity_id: &str) -> TimelineRow {
        TimelineRow::log(
            TimelineRowKind::Event {
                item: EventItem::Join,
            },
            text.to_owned(),
        )
        .with_identity(TimelineRowIdentity::activity_primary(activity_id))
    }

    fn text_line(
        row: usize,
        rows: &[TimelineRow],
        range: Range<usize>,
        indent: i32,
    ) -> TimelinePaintLine {
        TimelinePaintLine {
            kind: TimelineRowKind::Event {
                item: EventItem::Join,
            },
            visual: TimelinePaintLineVisual::Text {
                text: rows[row].logical_text[range.clone()].to_owned(),
                indent,
                styles: Vec::new(),
                range,
            },
            image_state: None,
            image_slot_index: None,
            source_row_index: Some(row),
        }
    }

    fn point(row: usize, offset: usize) -> TextPoint {
        TextPoint { row, offset }
    }

    const GEOMETRY: LineGeometry = LineGeometry {
        top_line: 0,
        visible_rows: 10,
        line_height: 15,
        vertical_padding: 6,
        left_padding: 8,
    };

    /// The x of the left edge of character `n` of a line at `indent`.
    fn x_of(n: i32, indent: i32) -> i32 {
        GEOMETRY.left_padding + indent + n * 10
    }

    #[test]
    fn offset_at_x_picks_the_nearest_boundary() {
        assert_eq!(offset_at_x("abc", -5, &mono), 0);
        assert_eq!(offset_at_x("abc", 4, &mono), 0);
        assert_eq!(offset_at_x("abc", 5, &mono), 1);
        assert_eq!(offset_at_x("abc", 14, &mono), 1);
        assert_eq!(offset_at_x("abc", 15, &mono), 2);
        assert_eq!(offset_at_x("abc", 29, &mono), 3);
        assert_eq!(offset_at_x("abc", 400, &mono), 3);
        // Offsets are bytes at char boundaries.
        assert_eq!(offset_at_x("añb", 15, &mono), "añ".len());
        assert_eq!(offset_at_x("añb", 14, &mono), 1);
    }

    #[test]
    fn char_at_x_is_the_character_under_the_point() {
        assert_eq!(char_at_x("abc", -1, &mono), None);
        assert_eq!(char_at_x("abc", 0, &mono), Some(0));
        assert_eq!(char_at_x("abc", 9, &mono), Some(0));
        assert_eq!(char_at_x("abc", 10, &mono), Some(1));
        assert_eq!(char_at_x("abc", 29, &mono), Some(2));
        assert_eq!(char_at_x("abc", 30, &mono), None);
        assert_eq!(char_at_x("añb", 25, &mono), Some("añ".len()));
    }

    #[test]
    fn point_at_clamps_to_the_visible_lines() {
        let rows = vec![row("first row", "a"), row("second row wraps", "b")];
        let lines = vec![
            text_line(0, &rows, 0..9, 0),
            text_line(1, &rows, 0..10, 0),
            text_line(1, &rows, 11..16, 30),
        ];
        let at = |x, y| point_at(&lines, GEOMETRY, x, y, &mono);
        // On a line: the boundary nearest x, in its row's text.
        assert_eq!(at(x_of(3, 0), 6), Some(point(0, 3)));
        assert_eq!(at(x_of(2, 30) + 4, 6 + 2 * 15), Some(point(1, 13)));
        // Left of a wrapped line's indent is its start.
        assert_eq!(at(0, 6 + 2 * 15), Some(point(1, 11)));
        // Past a line's end is its end.
        assert_eq!(at(x_of(50, 0), 6 + 15), Some(point(1, 10)));
        // Above the lines, the start of the first; below, the end of the last.
        assert_eq!(at(x_of(5, 0), 0), Some(point(0, 0)));
        assert_eq!(at(x_of(0, 0), 6 + 3 * 15), Some(point(1, 16)));
        assert_eq!(at(x_of(0, 0), 400), Some(point(1, 16)));

        // Scrolled: the first visible line is the clamp.
        let scrolled = LineGeometry {
            top_line: 1,
            visible_rows: 1,
            ..GEOMETRY
        };
        assert_eq!(point_at(&lines, scrolled, 0, -20, &mono), Some(point(1, 0)));
        assert_eq!(
            point_at(&lines, scrolled, 0, 200, &mono),
            Some(point(1, 10))
        );
    }

    #[test]
    fn point_at_an_image_is_the_end_of_its_rows_text() {
        let rows = vec![row("[22:13] <bob> cat.png", "a")];
        let mut image = text_line(0, &rows, 0..21, 0);
        image.visual = TimelinePaintLineVisual::ImageSlot;
        let lines = vec![
            text_line(0, &rows, 0..13, 0),
            text_line(0, &rows, 14..21, 30),
            image.clone(),
            image,
        ];
        assert_eq!(
            point_at(&lines, GEOMETRY, 0, 6 + 3 * 15, &mono),
            Some(point(0, 21))
        );
        assert_eq!(exact_point_at(&lines, GEOMETRY, 0, 6 + 3 * 15, &mono), None);
    }

    #[test]
    fn the_line_marker_is_a_point_but_copies_as_nothing() {
        let rows = vec![
            row("above", "a"),
            TimelineRow::line_marker(),
            row("below", "b"),
        ];
        let mut rule = text_line(1, &rows, 0..0, 0);
        rule.kind = TimelineRowKind::LineMarker;
        rule.visual = TimelinePaintLineVisual::Rule;
        let lines = vec![
            text_line(0, &rows, 0..5, 0),
            rule,
            text_line(2, &rows, 0..5, 0),
        ];
        assert_eq!(
            point_at(&lines, GEOMETRY, x_of(3, 0), 6 + 15, &mono),
            Some(point(1, 0))
        );
        assert_eq!(
            exact_point_at(&lines, GEOMETRY, x_of(0, 0), 6 + 15, &mono),
            None
        );
        // Across it: the lines above and below, one CRLF apart.
        assert_eq!(selected_text(&rows, point(0, 2), point(2, 3)), "ove\r\nbel");
        // From or to it: only the other line's part.
        assert_eq!(selected_text(&rows, point(1, 0), point(2, 3)), "bel");
        assert_eq!(selected_text(&rows, point(0, 2), point(1, 0)), "ove");
    }

    #[test]
    fn lines_without_a_row_have_no_point() {
        let mut line = text_line(0, &[row("plain", "a")], 0..5, 0);
        line.source_row_index = None;
        assert_eq!(point_at(&[line.clone()], GEOMETRY, 10, 10, &mono), None);
        assert_eq!(exact_point_at(&[line], GEOMETRY, 10, 10, &mono), None);
        assert_eq!(point_at(&[], GEOMETRY, 10, 10, &mono), None);
    }

    #[test]
    fn exact_point_at_needs_a_character_under_the_point() {
        let rows = vec![row("abc", "a")];
        let lines = vec![text_line(0, &rows, 0..3, 0)];
        let at = |x, y| exact_point_at(&lines, GEOMETRY, x, y, &mono);
        assert_eq!(at(x_of(1, 0) + 9, 6), Some(point(0, 1)));
        assert_eq!(at(x_of(3, 0), 6), None);
        assert_eq!(at(x_of(0, 0) - 1, 6), None);
        assert_eq!(at(x_of(0, 0), 5), None);
        assert_eq!(at(x_of(0, 0), 6 + 15), None);
    }

    #[test]
    fn selected_text_joins_rows_with_crlf_and_restores_wraps() {
        let rows = vec![
            row("[22:13] <bob> one two three", "a"),
            row("[22:14] <amy> four\nfive", "b"),
            row("[22:15] <bob> six", "c"),
        ];
        // Within a line.
        assert_eq!(selected_text(&rows, point(0, 14), point(0, 17)), "one");
        // Across a wrap at the space after "two": one line again.
        assert_eq!(
            selected_text(&rows, point(0, 18), point(0, 27)),
            "two three"
        );
        // Across rows, reversed, with a newline inside a row.
        assert_eq!(
            selected_text(&rows, point(2, 3), point(0, 22)),
            "three\r\n[22:14] <amy> four\r\nfive\r\n[22"
        );
    }

    #[test]
    fn selected_text_is_plain_whatever_the_styles() {
        let bold = TextStyle {
            bold: true,
            ..TextStyle::default()
        };
        let red = TextStyle {
            foreground: Some(TextColor {
                red: 0xff,
                green: 0,
                blue: 0,
            }),
            ..TextStyle::default()
        };
        let mut styled = row("[22:13] <bob> a bold red word", "a");
        styled.styles = vec![
            TextStyleSpan {
                start: 16,
                end: 20,
                style: bold,
            },
            TextStyleSpan {
                start: 21,
                end: 24,
                style: red,
            },
        ];
        let rows = vec![styled];
        assert_eq!(
            selected_text(&rows, point(0, 14), point(0, 29)),
            "a bold red word"
        );
        assert_eq!(selected_text(&rows, point(0, 17), point(0, 19)), "ol");
    }

    #[test]
    fn line_selection_is_the_marked_part_of_a_line() {
        let a = point(1, 5);
        let b = point(3, 4);
        assert_eq!(line_selection(0, &(0..10), a, b), None);
        assert_eq!(line_selection(1, &(0..4), a, b), None);
        assert_eq!(line_selection(1, &(0..10), a, b), Some(5..10));
        assert_eq!(line_selection(1, &(6..12), b, a), Some(0..6));
        assert_eq!(line_selection(2, &(3..9), a, b), Some(0..6));
        assert_eq!(line_selection(3, &(0..10), a, b), Some(0..4));
        assert_eq!(line_selection(3, &(4..10), a, b), None);
        assert_eq!(line_selection(4, &(0..10), a, b), None);
    }

    #[test]
    fn remap_follows_the_row_by_identity() {
        let old = vec![row("one", "a"), row("two", "b")];
        // History arrived above: the row moved.
        let prepended = vec![row("zero", "z"), row("one", "a"), row("two", "b")];
        assert_eq!(remap(point(1, 2), &old, &prepended), Some(point(2, 2)));
        // The row is gone.
        assert_eq!(remap(point(1, 2), &old, &old[..1]), None);
        // The row was edited.
        let edited = vec![row("one", "a"), row("two!", "b")];
        assert_eq!(remap(point(1, 2), &old, &edited), None);
    }

    #[test]
    fn a_drag_marks_once_it_leaves_the_drag_box() {
        assert!(!left_drag_box((10, 10), (12, 8), (4, 4)));
        assert!(left_drag_box((10, 10), (13, 10), (4, 4)));
        assert!(left_drag_box((10, 10), (10, 7), (4, 4)));
        let mut drag = TextDrag::new(point(0, 3), (10, 10));
        drag.focus = point(0, 5);
        assert_eq!(drag.mark(), None);
        drag.moved = true;
        assert_eq!(drag.mark(), Some((point(0, 3), point(0, 5))));
        drag.focus = point(0, 3);
        assert_eq!(drag.mark(), None);
    }
}
