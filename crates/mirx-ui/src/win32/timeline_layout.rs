//! Timeline layout and paint: rows become wrapped paint lines, and paint
//! lines are drawn in the frame's text font.

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use mirx_core::{TextStyle, TextStyleSpan};
use windows_sys::Win32::Graphics::Gdi::{HDC, InvertRect};

use super::colors::{ColorScheme, ERROR_COLOR, PaletteIndex, colorref, rgb};
use super::settings::ActivityColors;
use super::text_selection::line_selection;
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TimelinePaintLine {
    pub(super) kind: TimelineRowKind,
    pub(super) visual: TimelinePaintLineVisual,
    pub(super) image_state: Option<TimelineImageState>,
    pub(super) image_slot_index: Option<usize>,
    pub(super) source_row_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TimelinePaintLineVisual {
    /// Text drawn `indent` pixels right of the timeline's left padding, in
    /// the row's styles clipped to it, at their offsets in `text`. `text` is
    /// `range` of its row's `logical_text`: where marking and links find it.
    Text {
        text: String,
        indent: i32,
        styles: Vec<TextStyleSpan>,
        range: Range<usize>,
    },
    ImageSlot,
    /// The line marker's rule across the line's middle. No text: nothing
    /// hit-tests, marks or links on it.
    Rule,
}

/// The timeline's lengths at its window's DPI, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TimelineMetrics {
    /// Between the client's left edge and the text.
    pub(super) left_padding: i32,
    pub(super) right_padding: i32,
    /// Above the first line.
    pub(super) vertical_padding: i32,
    /// The box a thumbnail is drawn in.
    thumbnail_width: i32,
    thumbnail_height: i32,
    /// Space around a thumbnail inside its slot, over the slot's lines.
    image_slot_padding: i32,
    /// Between the text's left edge and the thumbnail.
    image_left_indent: i32,
    image_placeholder_inset: i32,
    /// How thick the line marker's rule is.
    rule_thickness: i32,
}

impl TimelineMetrics {
    pub(super) fn at(dpi: Dpi) -> Self {
        Self {
            left_padding: dpi.px(TIMELINE_LEFT_PADDING),
            right_padding: dpi.px(TIMELINE_RIGHT_PADDING),
            vertical_padding: dpi.px(TIMELINE_VERTICAL_PADDING),
            thumbnail_width: dpi.px(TIMELINE_IMAGE_THUMBNAIL_MAX_WIDTH),
            thumbnail_height: dpi.px(TIMELINE_IMAGE_THUMBNAIL_MAX_HEIGHT),
            image_slot_padding: dpi.px(TIMELINE_IMAGE_SLOT_PADDING),
            image_left_indent: dpi.px(TIMELINE_IMAGE_LEFT_INDENT),
            image_placeholder_inset: dpi.px(TIMELINE_IMAGE_PLACEHOLDER_INSET),
            rule_thickness: dpi.px(1),
        }
    }

    /// The width available to timeline text, never less than one pixel so
    /// wrapping always makes progress in a collapsed view.
    fn text_width(&self, client_width: i32) -> i32 {
        (client_width - self.left_padding - self.right_padding).max(1)
    }
}

pub(super) fn plain_timeline_paint_lines(lines: &[String]) -> Vec<TimelinePaintLine> {
    lines
        .iter()
        .cloned()
        .map(|text| TimelinePaintLine {
            kind: TimelineRowKind::Plain,
            visual: TimelinePaintLineVisual::Text {
                range: 0..text.len(),
                text,
                indent: 0,
                styles: Vec::new(),
            },
            image_state: None,
            image_slot_index: None,
            source_row_index: None,
        })
        .collect()
}

/// The ranges of `text`'s lines, split at `\n`, each without a trailing
/// `\r`. Empty text has no lines.
pub(super) fn rendered_line_ranges(text: &str) -> Vec<Range<usize>> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let mut start = 0;
    for line in text.split('\n') {
        let end = start + line.trim_end_matches('\r').len();
        ranges.push(start..end);
        start += line.len() + 1;
    }
    ranges
}

/// The length of `text` without its trailing whitespace.
fn trimmed_end_len(text: &str) -> usize {
    text.trim_end_matches(|c: char| c.is_whitespace()).len()
}

/// Wraps one text line into the ranges of its segments: the first fits
/// `first_width`, later ones `continuation_width`. A line breaks, as in
/// mIRC, at its last whitespace that fits, which is dropped at the break,
/// or right after its last comma that fits, whichever is later; a token
/// with neither is split at its last fitting character. The fit is a
/// binary search over the line's char boundaries, which relies on a
/// prefix never measuring narrower than a shorter one.
fn wrap_timeline_visual_line<Measure>(
    line: &str,
    first_width: i32,
    continuation_width: i32,
    measure: &Measure,
) -> Vec<Range<usize>>
where
    Measure: Fn(&str) -> i32,
{
    assert!(
        first_width > 0 && continuation_width > 0,
        "timeline wrap widths must be positive"
    );

    if line.is_empty() {
        // One empty visual line.
        return vec![Range { start: 0, end: 0 }];
    }

    let mut wrapped = Vec::new();
    let mut start = 0;
    while start < line.len() {
        let remaining = &line[start..];
        let max_width = if wrapped.is_empty() {
            first_width
        } else {
            continuation_width
        };
        if measure(remaining) <= max_width {
            wrapped.push(start..start + trimmed_end_len(remaining));
            break;
        }

        // The end of every char; the last one, all of `remaining`, does not
        // fit.
        let char_ends = remaining
            .char_indices()
            .map(|(index, ch)| index + ch.len_utf8())
            .collect::<Vec<_>>();
        let fitting = char_ends[..char_ends.len() - 1]
            .partition_point(|&end| measure(&remaining[..end]) <= max_width);
        let fit_end = fitting.checked_sub(1).map_or(0, |last| char_ends[last]);
        let break_index = line_break_within(remaining, fit_end)
            .or_else(|| (fit_end > 0).then_some(fit_end))
            .unwrap_or(char_ends[0]);
        wrapped.push(start..start + trimmed_end_len(&remaining[..break_index]));
        let rest = &remaining[break_index..];
        start += break_index + (rest.len() - rest.trim_start().len());
    }

    wrapped
}

/// Where `text`, whose first `fit_end` bytes fit a line, breaks: before its
/// last whitespace at or before `fit_end` (whitespace is dropped at a break,
/// so a space just past the width still ends a line whose text fits), or
/// after its last comma inside the fit, whichever is later. Never at 0.
fn line_break_within(text: &str, fit_end: usize) -> Option<usize> {
    text.char_indices()
        .take_while(|&(index, _)| index <= fit_end)
        .filter_map(|(index, ch)| {
            if ch.is_whitespace() {
                (index > 0).then_some(index)
            } else if ch == ',' {
                (index < fit_end).then_some(index + 1)
            } else {
                None
            }
        })
        .last()
}

/// What a continuation line is indented by, measured in the text font:
/// two spaces, mIRC's default wrap indent (`/echo -i`). mIRC has no option
/// for it, and it does not follow the timestamp.
const WRAP_INDENT_SAMPLE: &str = "  ";

/// Paint lines for a log row, as ranges of `text` with their indent: the
/// first line starts at the left padding and every continuation, whether
/// wrapped or from a newline in the text, is indented by the width of
/// [`WRAP_INDENT_SAMPLE`], as in mIRC.
fn timeline_log_paint_ranges<Measure>(
    text: &str,
    width: i32,
    measure: &Measure,
) -> Vec<(Range<usize>, i32)>
where
    Measure: Fn(&str) -> i32,
{
    let indent = measure(WRAP_INDENT_SAMPLE).min(width - 1);
    let continuation_width = width - indent;
    let mut lines = Vec::new();
    for line in rendered_line_ranges(text) {
        let first_width = if lines.is_empty() {
            width
        } else {
            continuation_width
        };
        for segment in wrap_timeline_visual_line(
            &text[line.clone()],
            first_width,
            continuation_width,
            measure,
        ) {
            let segment_indent = if lines.is_empty() { 0 } else { indent };
            lines.push((
                line.start + segment.start..line.start + segment.end,
                segment_indent,
            ));
        }
    }
    lines
}

/// A log row's paint lines, as ranges of its text with their indents.
type LogWrap = Rc<[(Range<usize>, i32)]>;

/// The wraps of a timeline's log rows at one text width, by row text, so
/// that new rows wrap only texts not wrapped already. It holds only the
/// texts of the rows last laid out. Wraps are in the timeline's font: a new
/// font must `clear` it (an `HFONT` is no key, as handles are reused).
#[derive(Default)]
pub(super) struct WrapCache {
    /// The text width the wraps are for.
    width: i32,
    by_text: HashMap<String, LogWrap>,
}

impl WrapCache {
    pub(super) fn clear(&mut self) {
        self.by_text.clear();
    }

    /// The wrap of each of `rows` at `width`, `None` for a row that is not
    /// a log row, and how many texts had to be wrapped (measured). Texts
    /// not in `rows` are dropped.
    fn rewrap<Measure>(
        &mut self,
        rows: &[TimelineRow],
        width: i32,
        measure: &Measure,
    ) -> (Vec<Option<LogWrap>>, usize)
    where
        Measure: Fn(&str) -> i32,
    {
        let mut previous = std::mem::take(&mut self.by_text);
        if width != self.width {
            previous.clear();
            self.width = width;
        }
        let mut wrapped = 0;
        let wraps = rows
            .iter()
            .map(|row| {
                (row.layout == TimelineRowLayout::Log).then(|| {
                    let text = &row.logical_text;
                    if let Some(wrap) = self.by_text.get(text) {
                        return Rc::clone(wrap);
                    }
                    let (text, wrap) = previous.remove_entry(text).unwrap_or_else(|| {
                        wrapped += 1;
                        (
                            text.clone(),
                            timeline_log_paint_ranges(text, width, measure).into(),
                        )
                    });
                    self.by_text.insert(text, Rc::clone(&wrap));
                    wrap
                })
            })
            .collect();
        (wraps, wrapped)
    }
}

/// The parts of `styles` inside `segment`, at their offsets in it.
pub(super) fn clip_styles(styles: &[TextStyleSpan], segment: &Range<usize>) -> Vec<TextStyleSpan> {
    styles
        .iter()
        .filter_map(|span| {
            let start = span.start.max(segment.start);
            let end = span.end.min(segment.end);
            (start < end).then(|| TextStyleSpan {
                start: start - segment.start,
                end: end - segment.start,
                style: span.style,
            })
        })
        .collect()
}

/// The paint lines of `rows` in a client `client_width` wide, with log rows
/// wrapped through `cache`, and how many log rows had to be wrapped.
fn timeline_row_paint_lines_with_measure<Measure>(
    rows: &[TimelineRow],
    client_width: i32,
    line_height: i32,
    metrics: &TimelineMetrics,
    cache: &mut WrapCache,
    measure: &Measure,
) -> (Vec<TimelinePaintLine>, usize)
where
    Measure: Fn(&str) -> i32,
{
    let (wraps, wrapped) = cache.rewrap(rows, metrics.text_width(client_width), measure);
    let mut paint_lines = Vec::new();
    for (source_row_index, (row, wrap)) in rows.iter().zip(wraps).enumerate() {
        if row.layout == TimelineRowLayout::Rule {
            paint_lines.push(TimelinePaintLine {
                kind: row.kind.clone(),
                visual: TimelinePaintLineVisual::Rule,
                image_state: None,
                image_slot_index: None,
                source_row_index: Some(source_row_index),
            });
            continue;
        }
        let segments = match wrap {
            None => rendered_line_ranges(&row.logical_text)
                .into_iter()
                .map(|line| (line, 0))
                .collect(),
            Some(wrap) => wrap.to_vec(),
        };
        for (segment, indent) in segments {
            paint_lines.push(TimelinePaintLine {
                kind: row.kind.clone(),
                visual: TimelinePaintLineVisual::Text {
                    text: row.logical_text[segment.clone()].to_owned(),
                    indent,
                    styles: clip_styles(&row.styles, &segment),
                    range: segment,
                },
                image_state: None,
                image_slot_index: None,
                source_row_index: Some(source_row_index),
            });
        }
        if let Some(image_state) = &row.image_state {
            for image_slot_index in 0..image_slot_lines(line_height, metrics) {
                paint_lines.push(TimelinePaintLine {
                    kind: row.kind.clone(),
                    visual: TimelinePaintLineVisual::ImageSlot,
                    image_state: Some(image_state.clone()),
                    image_slot_index: Some(image_slot_index),
                    source_row_index: Some(source_row_index),
                });
            }
        }
    }
    (paint_lines, wrapped)
}

/// Lays `rows` out in the timeline, wrapping only the log rows its wrap
/// cache has not wrapped at this width.
pub(super) unsafe fn timeline_row_paint_lines(
    hwnd: HWND,
    rows: &[TimelineRow],
    client_width: i32,
) -> Vec<TimelinePaintLine> {
    let state = timeline_state_mut(hwnd);
    let glyphs = &state_ref(frame_window_for(hwnd)).text_font.glyphs;
    assert_eq!(
        state.font,
        glyphs.hfont(),
        "a timeline lays out in the frame's text font"
    );
    let hdc = GetDC(hwnd);
    assert!(
        !hdc.is_null(),
        "GetDC must succeed for timeline measurement"
    );
    let (paint_lines, wrapped) = timeline_row_paint_lines_with_measure(
        rows,
        client_width,
        state.line_height,
        &state.metrics,
        &mut state.wrap_cache,
        &|text| unsafe { glyphs.measure(hdc, text) },
    );
    ReleaseDC(hwnd, hdc);
    state_mut(frame_window_for(hwnd))
        .refresh_counters
        .count_rows_wrapped(wrapped);
    paint_lines
}

/// `text` as consecutive pieces covering all of it: each styled span and
/// the plain text between them, with `None` for plain.
fn style_pieces<'a>(text: &'a str, styles: &[TextStyleSpan]) -> Vec<(&'a str, Option<TextStyle>)> {
    let mut pieces = Vec::new();
    let mut written = 0;
    for span in styles {
        if written < span.start {
            pieces.push((&text[written..span.start], None));
        }
        pieces.push((&text[span.start..span.end], Some(span.style)));
        written = span.end;
    }
    if written < text.len() {
        pieces.push((&text[written..], None));
    }
    pieces
}

/// Whether a rule (the line marker) is among the `visible_rows` lines from
/// `top_line`.
pub(super) fn rule_visible(
    paint_lines: &[TimelinePaintLine],
    top_line: usize,
    visible_rows: usize,
) -> bool {
    paint_lines
        .iter()
        .skip(top_line)
        .take(visible_rows)
        .any(|line| line.visual == TimelinePaintLineVisual::Rule)
}

pub(super) unsafe fn paint_timeline(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint);
    let frame_state = state_ref(frame_window_for(hwnd));
    let settings = frame_state.settings.settings();
    let scheme = &settings.colors;
    // The class has no background brush: this fills every pixel.
    FillRect(hdc, &paint.rcPaint, frame_state.scheme_brushes.background);
    SetBkMode(hdc, TRANSPARENT as i32);
    let mut client_rect = RECT::default();
    GetClientRect(hwnd, &mut client_rect);
    let glyphs = &frame_state.text_font.glyphs;
    let state = timeline_state_ref(hwnd);
    assert_eq!(
        state.font,
        glyphs.hfont(),
        "a timeline paints in the frame's text font"
    );
    let previous = SelectObject(hdc, state.font);
    let find_highlight_brush = CreateSolidBrush(TIMELINE_FIND_HIGHLIGHT_COLOR);
    let selection_brush = CreateSolidBrush(TIMELINE_SELECTION_COLOR);

    let selected_event_id = selected_timeline_event_id(frame_state);
    let mark = state.drag.and_then(|drag| drag.mark());
    let visible_rows = timeline_visible_rows(hwnd);
    for (row, line) in state
        .paint_lines
        .iter()
        .skip(state.top_line)
        .take(visible_rows)
        .enumerate()
    {
        let line_y = state.metrics.vertical_padding + row as i32 * state.line_height;
        if line.source_row_index.is_some_and(|index| {
            timeline_row_is_selected(
                state.source_rows[index].event_id.as_deref(),
                selected_event_id,
            )
        }) {
            let rect = RECT {
                left: 0,
                top: line_y,
                right: client_rect.right,
                bottom: line_y + state.line_height,
            };
            FillRect(hdc, &rect, selection_brush);
        }
        if state.selected_find_source_row_index.is_some()
            && line.source_row_index == state.selected_find_source_row_index
        {
            let rect = RECT {
                left: 0,
                top: line_y,
                right: client_rect.right,
                bottom: line_y + state.line_height,
            };
            FillRect(hdc, &rect, find_highlight_brush);
        }
        if let Some(image_slot_index) = line.image_slot_index {
            if let Some(image_state) = line.image_state.as_ref() {
                paint_timeline_image_slot(hwnd, hdc, line_y, image_slot_index, image_state);
            }
            continue;
        }

        match &line.visual {
            TimelinePaintLineVisual::Text {
                text,
                indent,
                styles,
                range,
            } => {
                let left = state.metrics.left_padding + indent;
                let rect = RECT {
                    left,
                    top: line_y,
                    right: (client_rect.right - state.metrics.right_padding).max(left + 1),
                    bottom: line_y + state.line_height,
                };
                paint_styled_text(
                    hdc,
                    glyphs,
                    left,
                    &rect,
                    text,
                    styles,
                    timeline_row_color(&line.kind, scheme, settings.marker.color),
                );
                // mIRC shows marked text in inverse video, which keeps
                // every style's colours readable.
                if let (Some((anchor, focus)), Some(row)) = (mark, line.source_row_index)
                    && let Some(marked) = line_selection(row, range, anchor, focus)
                {
                    let marked = RECT {
                        left: left + glyphs.measure(hdc, &text[..marked.start]),
                        top: line_y,
                        right: left + glyphs.measure(hdc, &text[..marked.end]),
                        bottom: line_y + state.line_height,
                    };
                    InvertRect(hdc, &marked);
                }
            }
            TimelinePaintLineVisual::ImageSlot => {}
            TimelinePaintLineVisual::Rule => {
                let left = state.metrics.left_padding;
                let thickness = state.metrics.rule_thickness;
                let top = line_y + (state.line_height - thickness) / 2;
                let right = client_rect.right - state.metrics.right_padding;
                fill_clipped(
                    hdc,
                    &client_rect,
                    left,
                    top,
                    right - left,
                    thickness,
                    timeline_row_color(&line.kind, scheme, settings.marker.color),
                );
            }
        }
    }

    DeleteObject(find_highlight_brush);
    DeleteObject(selection_brush);
    SelectObject(hdc, previous);

    EndPaint(hwnd, &paint);
    // The line marker was on screen in the window the user is in: leaving
    // it may move the marker. A window painted beside the active one (tiled)
    // is not the one being read.
    if state.line_marker_sight == line_marker::MarkerSight::Unseen
        && rule_visible(&state.paint_lines, state.top_line, visible_rows)
    {
        timeline_state_mut(hwnd).line_marker_sight = line_marker::MarkerSight::Seen;
    }
}

/// Draws one line of `text` from `left` within `rect`, a line's height,
/// in `default_color` except where `styles` say otherwise. Bold is drawn a
/// second time a pixel right, as GDI emboldens raster fonts, and italic in
/// the faces' italic fonts at the regular advances, so styles never change
/// where text goes and wrapping stays exact.
unsafe fn paint_styled_text(
    hdc: HDC,
    glyphs: &GlyphFonts,
    left: i32,
    rect: &RECT,
    text: &str,
    styles: &[TextStyleSpan],
    default_color: u32,
) {
    let baseline = rect.top + glyphs.metrics().tmAscent;
    if styles.is_empty() {
        SetTextColor(hdc, default_color);
        glyphs.draw(hdc, left, baseline, rect, text, false);
        return;
    }
    let mut x = left;
    for (piece, style) in style_pieces(text, styles) {
        let width = glyphs.measure(hdc, piece);
        let style = style.unwrap_or_default();
        let color = style.foreground.map_or(default_color, colorref);
        if let Some(background) = style.background {
            fill_clipped(
                hdc,
                rect,
                x,
                rect.top,
                width,
                rect.bottom - rect.top,
                colorref(background),
            );
        }
        SetTextColor(hdc, color);
        glyphs.draw(hdc, x, baseline, rect, piece, style.italic);
        if style.bold {
            glyphs.draw(hdc, x + 1, baseline, rect, piece, style.italic);
        }
        if style.underline {
            fill_clipped(hdc, rect, x, baseline + 1, width, 1, color);
        }
        if style.strikethrough {
            fill_clipped(
                hdc,
                rect,
                x,
                baseline - glyphs.metrics().tmAscent / 3,
                width,
                1,
                color,
            );
        }
        x += width;
    }
}

/// Fills the part of the `width` by `height` box at `x`, `y` inside `clip`.
unsafe fn fill_clipped(hdc: HDC, clip: &RECT, x: i32, y: i32, width: i32, height: i32, color: u32) {
    let rect = RECT {
        left: x.max(clip.left),
        top: y.max(clip.top),
        right: (x + width).min(clip.right),
        bottom: (y + height).min(clip.bottom),
    };
    if rect.left >= rect.right || rect.top >= rect.bottom {
        return;
    }
    let brush = CreateSolidBrush(color);
    assert!(!brush.is_null(), "CreateSolidBrush must succeed");
    FillRect(hdc, &rect, brush);
    DeleteObject(brush);
}

/// A row's colour where its styles set none: its scheme item's, mIRC's
/// line colours, and the line marker's own `marker` colour.
pub(super) fn timeline_row_color(
    kind: &TimelineRowKind,
    scheme: &ColorScheme,
    marker: PaletteIndex,
) -> u32 {
    let item = match kind {
        TimelineRowKind::LineMarker => return marker.colorref(),
        // mIRC draws a highlighted line wholly in the highlight colour.
        TimelineRowKind::Message {
            highlight: true, ..
        }
        | TimelineRowKind::Image { highlight: true }
        | TimelineRowKind::File { highlight: true }
        | TimelineRowKind::Audio { highlight: true }
        | TimelineRowKind::Video { highlight: true }
        | TimelineRowKind::Poll { highlight: true }
        | TimelineRowKind::Unsupported { highlight: true }
        | TimelineRowKind::StateNotice {
            highlight: true, ..
        } => ColorItem::Highlight,
        TimelineRowKind::Error
        | TimelineRowKind::Message {
            send_state: TimelineMessageSendState::Failed,
            ..
        } => return ERROR_COLOR.colorref(),
        TimelineRowKind::Plain => ColorItem::Normal,
        TimelineRowKind::Event { item } | TimelineRowKind::StateNotice { item, .. } => {
            item.color_item()
        }
        TimelineRowKind::Info => ColorItem::Info,
        // Dim lines: annotations around a message, sends on their way and
        // what is left of a redaction.
        TimelineRowKind::Typing
        | TimelineRowKind::ReactionSummary
        | TimelineRowKind::ReplyQuote
        | TimelineRowKind::Redacted
        | TimelineRowKind::Message {
            send_state: TimelineMessageSendState::Pending,
            ..
        } => ColorItem::Gray,
        TimelineRowKind::Message {
            presentation: RoomMessagePresentationKind::Notice,
            send_state: TimelineMessageSendState::Sent,
            ..
        } => ColorItem::Notice,
        TimelineRowKind::Message {
            presentation: RoomMessagePresentationKind::Emote,
            send_state: TimelineMessageSendState::Sent,
            ..
        } => ColorItem::Action,
        TimelineRowKind::Message {
            presentation: RoomMessagePresentationKind::Text,
            send_state: TimelineMessageSendState::Sent,
            own: true,
            ..
        } => ColorItem::Own,
        TimelineRowKind::Message {
            presentation: RoomMessagePresentationKind::Text,
            send_state: TimelineMessageSendState::Sent,
            own: false,
            ..
        } => ColorItem::Normal,
        // Everything that is not text a user wrote.
        TimelineRowKind::Image { highlight: false }
        | TimelineRowKind::File { highlight: false }
        | TimelineRowKind::Audio { highlight: false }
        | TimelineRowKind::Video { highlight: false }
        | TimelineRowKind::Poll { highlight: false }
        | TimelineRowKind::Unsupported { highlight: false }
        | TimelineRowKind::UnableToDecrypt => ColorItem::Other,
    };
    scheme.colorref(item)
}

/// mIRC's switchbar/treebar activity colours (Options > Display > Event,
/// Message, Highlight): an inactive window's button text takes the colour
/// of what happened in it.
pub(super) fn activity_level_color(level: ViewActivityLevel, colors: &ActivityColors) -> u32 {
    match level {
        ViewActivityLevel::Event => colors.event,
        ViewActivityLevel::Message => colors.message,
        ViewActivityLevel::Highlight => colors.highlight,
    }
    .colorref()
}

/// How many lines an image's slot takes: enough for the thumbnail and its
/// padding.
fn image_slot_lines(line_height: i32, metrics: &TimelineMetrics) -> usize {
    let slot_height = metrics.thumbnail_height + metrics.image_slot_padding;
    let line_height = u32::try_from(line_height).expect("a line height is positive");
    (slot_height as u32).div_ceil(line_height) as usize
}

/// The thumbnail's rect, centred in its slot, from the slot line at
/// `line_y` that is the slot's `image_slot_index`th.
fn timeline_image_slot_rect(
    line_y: i32,
    image_slot_index: usize,
    line_height: i32,
    metrics: &TimelineMetrics,
) -> RECT {
    let slot_height = image_slot_lines(line_height, metrics) as i32 * line_height;
    let top = line_y + (slot_height - metrics.thumbnail_height) / 2
        - image_slot_index as i32 * line_height;
    let left = metrics.left_padding + metrics.image_left_indent;
    RECT {
        left,
        top,
        right: left + metrics.thumbnail_width,
        bottom: top + metrics.thumbnail_height,
    }
}

unsafe fn paint_timeline_image_slot(
    hwnd: HWND,
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    line_y: i32,
    image_slot_index: usize,
    image_state: &TimelineImageState,
) {
    let state = timeline_state_ref(hwnd);
    let metrics = &state.metrics;
    let rect = timeline_image_slot_rect(line_y, image_slot_index, state.line_height, metrics);
    match image_state {
        TimelineImageState::Pending => {
            paint_timeline_image_placeholder(hdc, &rect, metrics, "loading image...");
        }
        TimelineImageState::Loaded { key, width, height } => {
            paint_timeline_loaded_image(hwnd, hdc, &rect, metrics, key, *width, *height);
        }
        TimelineImageState::Failed { .. } => {
            paint_timeline_image_placeholder(hdc, &rect, metrics, "image unavailable");
        }
    }
}

unsafe fn paint_timeline_image_placeholder(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: &RECT,
    metrics: &TimelineMetrics,
    message: &str,
) {
    let border = CreateSolidBrush(rgb(0x7A, 0x7A, 0x7A));
    if !border.is_null() {
        FrameRect(hdc, rect, border);
        DeleteObject(border as _);
    }
    let text = wide_null(message);
    SetTextColor(hdc, rgb(0x5E, 0x5E, 0x5E));
    TextOutW(
        hdc,
        rect.left + metrics.image_placeholder_inset,
        rect.top + metrics.image_placeholder_inset,
        text.as_ptr(),
        text.len() as i32 - 1,
    );
}

unsafe fn paint_timeline_loaded_image(
    hwnd: HWND,
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: &RECT,
    metrics: &TimelineMetrics,
    key: &str,
    width: u32,
    height: u32,
) {
    let window_state = state_ref(frame_window_for(hwnd));
    let Some(image) = window_state.decoded_images.get(key) else {
        paint_timeline_image_placeholder(hdc, rect, metrics, "image unavailable");
        return;
    };
    // An image's pixels are 96-dpi pixels, as a browser shows them: at a
    // higher DPI it is drawn larger, up to the box.
    let dpi = Dpi::of_window(hwnd);
    let natural = |length: u32| {
        let length = i32::try_from(length).expect("a decoded image's size fits i32");
        u32::try_from(dpi.px(length)).expect("a scaled length is not negative")
    };
    let (draw_width, draw_height) = scaled_thumbnail_dimensions(
        natural(width),
        natural(height),
        metrics.thumbnail_width,
        metrics.thumbnail_height,
    );
    let draw_x = rect.left + (metrics.thumbnail_width - draw_width) / 2;
    let draw_y = rect.top + (metrics.thumbnail_height - draw_height) / 2;

    let border = CreateSolidBrush(rgb(0x7A, 0x7A, 0x7A));
    if !border.is_null() {
        FrameRect(hdc, rect, border);
        DeleteObject(border as _);
    }

    let mut graphics = null_mut::<GpGraphics>();
    if GdipCreateFromHDC(hdc, &mut graphics) != 0 {
        paint_timeline_image_placeholder(hdc, rect, metrics, "image unavailable");
        return;
    }
    let status = GdipDrawImageRectI(graphics, image.raw, draw_x, draw_y, draw_width, draw_height);
    GdipDeleteGraphics(graphics);
    if status != 0 {
        paint_timeline_image_placeholder(hdc, rect, metrics, "image unavailable");
    }
}

fn scaled_thumbnail_dimensions(
    width: u32,
    height: u32,
    max_width: i32,
    max_height: i32,
) -> (i32, i32) {
    let width = width as f64;
    let height = height as f64;
    let scale = f64::min(
        1.0,
        f64::min(max_width as f64 / width, max_height as f64 / height),
    );
    let draw_width = (width * scale).round() as i32;
    let draw_height = (height * scale).round() as i32;
    (draw_width.max(1), draw_height.max(1))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::win32::colors::PaletteIndex;

    fn measure_ascii_width(text: &str) -> i32 {
        text.chars().count() as i32
    }

    const STANDARD: TimelineMetrics = TimelineMetrics {
        left_padding: 8,
        right_padding: 8,
        vertical_padding: 6,
        thumbnail_width: 160,
        thumbnail_height: 120,
        image_slot_padding: 20,
        image_left_indent: 24,
        image_placeholder_inset: 8,
        rule_thickness: 1,
    };

    #[test]
    fn metrics_at_96_dpi_are_the_layout_constants() {
        assert_eq!(TimelineMetrics::at(Dpi::STANDARD), STANDARD);
    }

    #[test]
    fn metrics_scale_with_the_dpi() {
        let metrics = TimelineMetrics::at(Dpi::new(144));
        assert_eq!(
            metrics,
            TimelineMetrics {
                left_padding: 12,
                right_padding: 12,
                vertical_padding: 9,
                thumbnail_width: 240,
                thumbnail_height: 180,
                image_slot_padding: 30,
                image_left_indent: 36,
                image_placeholder_inset: 12,
                rule_thickness: 2,
            }
        );
        // (180 + 30) / 15, rounded up.
        assert_eq!(image_slot_lines(15, &metrics), 14);
        let rect = timeline_image_slot_rect(0, 0, 15, &metrics);
        assert_eq!(
            (rect.left, rect.right - rect.left, rect.bottom - rect.top),
            (48, 240, 180)
        );
    }

    /// The paint lines of `rows` laid out with an empty wrap cache.
    fn fresh_paint_lines(
        rows: &[TimelineRow],
        client_width: i32,
        line_height: i32,
        measure: &dyn Fn(&str) -> i32,
    ) -> Vec<TimelinePaintLine> {
        timeline_row_paint_lines_with_measure(
            rows,
            client_width,
            line_height,
            &STANDARD,
            &mut WrapCache::default(),
            &measure,
        )
        .0
    }

    #[test]
    fn continuation_lines_are_indented_two_spaces_whatever_the_timestamp() {
        let indent_of = |setting: &TimestampSetting| {
            let prefix =
                TimestampStyle::new(setting, timestamp::utc_local_time).prefix(1_700_000_000_000);
            let rows = [TimelineRow::log(
                TimelineRowKind::Info,
                format!("{prefix}alpha beta gamma"),
            )];
            fresh_paint_lines(
                &rows,
                client_width_for_text(prefix.len() as i32 + 10),
                20,
                &measure_ascii_width,
            )
            .into_iter()
            .map(|line| match line.visual {
                TimelinePaintLineVisual::Text { indent, .. } => indent,
                TimelinePaintLineVisual::ImageSlot | TimelinePaintLineVisual::Rule => {
                    unreachable!("info rows are text")
                }
            })
            .collect::<Vec<_>>()
        };

        for format in ["[HH:nn]", "[HH:nn:ss]"] {
            let setting = TimestampSetting {
                show: true,
                format: timestamp::TimestampFormat::parse(format).unwrap(),
            };
            assert_eq!(indent_of(&setting), vec![0, 2], "{format}");
        }
        let off = TimestampSetting {
            show: false,
            ..TimestampSetting::default()
        };
        assert_eq!(indent_of(&off), vec![0, 2]);
    }

    /// The lines of a log row's text in a text area `width` wide.
    fn log_lines(text: &str, width: i32) -> Vec<(&str, i32)> {
        timeline_log_paint_ranges(text, width, &measure_ascii_width)
            .into_iter()
            .map(|(range, indent)| (&text[range], indent))
            .collect()
    }

    #[test]
    fn a_token_moves_to_the_next_line_before_it_is_split() {
        assert_eq!(
            log_lines("aaa bbbbbbbbbb", 10),
            vec![("aaa", 0), ("bbbbbbbb", 2), ("bb", 2)]
        );
        // A narrow tile: with a two-space indent the words fit whole.
        let lines = log_lines("* Messages in this room are end-to-end encrypted", 18);
        assert_eq!(
            lines,
            vec![
                ("* Messages in this", 0),
                ("room are", 2),
                ("end-to-end", 2),
                ("encrypted", 2),
            ]
        );
    }

    #[test]
    fn timeline_row_paint_lines_expand_image_slots_without_changing_logical_text_rows() {
        let mut image_row = TimelineRow::log(
            TimelineRowKind::Image { highlight: false },
            "[16:00] * @alice:example.org sent image: diagram.png".to_owned(),
        );
        image_row.image_state = Some(TimelineImageState::Pending);
        let rows = vec![
            image_row,
            TimelineRow::log(
                TimelineRowKind::Unsupported { highlight: false },
                "[16:01] * @alice:example.org sent unsupported com.example.custom: clip.mp4"
                    .to_owned(),
            ),
        ];

        let logical_lines = project_timeline_rows(&rows);
        let paint_lines = fresh_paint_lines(&rows, 360, 20, &measure_ascii_width);

        assert_eq!(
            logical_lines,
            vec![
                "[16:00] * @alice:example.org sent image: diagram.png".to_owned(),
                "[16:01] * @alice:example.org sent unsupported com.example.custom: clip.mp4"
                    .to_owned(),
            ]
        );
        assert_eq!(
            paint_lines
                .iter()
                .filter(|line| line.image_slot_index.is_none())
                .map(|line| line.visual.clone())
                .collect::<Vec<_>>(),
            logical_lines
                .into_iter()
                .map(|text| TimelinePaintLineVisual::Text {
                    range: 0..text.len(),
                    text,
                    indent: 0,
                    styles: Vec::new(),
                })
                .collect::<Vec<_>>()
        );
        assert_eq!(
            paint_lines
                .iter()
                .filter(|line| line.image_slot_index.is_some())
                .count(),
            image_slot_lines(20, &STANDARD)
        );
        assert_eq!(clamped_timeline_top_line(6, paint_lines.len(), 3), 6);
    }

    fn log_paint_texts(text: &str, client_width: i32) -> Vec<(String, i32)> {
        log_paint_lines(text, client_width)
            .into_iter()
            .map(|(text, indent, _)| (text, indent))
            .collect()
    }

    /// Each paint line of a message row: its text, indent, and range of the
    /// row's text.
    fn log_paint_lines(text: &str, client_width: i32) -> Vec<(String, i32, Range<usize>)> {
        let rows = vec![TimelineRow::log(
            TimelineRowKind::Message {
                presentation: RoomMessagePresentationKind::Text,
                send_state: TimelineMessageSendState::Sent,
                highlight: false,
                own: false,
            },
            text.to_owned(),
        )];
        fresh_paint_lines(&rows, client_width, 20, &measure_ascii_width)
            .into_iter()
            .map(|line| match line.visual {
                TimelinePaintLineVisual::Text {
                    text,
                    indent,
                    range,
                    ..
                } => (text, indent, range),
                TimelinePaintLineVisual::ImageSlot | TimelinePaintLineVisual::Rule => {
                    unreachable!("message rows are text")
                }
            })
            .collect()
    }

    #[test]
    fn paint_lines_carry_their_range_of_the_rows_text() {
        let text = "[22:13] <bob> alpha beta gamma\nnext";
        let lines = log_paint_lines(text, client_width_for_text(20));
        assert_eq!(
            lines,
            vec![
                ("[22:13] <bob> alpha".to_owned(), 0, 0..19),
                ("beta gamma".to_owned(), 2, 20..30),
                ("next".to_owned(), 2, 31..35),
            ]
        );
        for (line_text, _, range) in lines {
            assert_eq!(&text[range], line_text);
        }
        assert_eq!(
            plain_timeline_paint_lines(&["abc".to_owned()])[0].visual,
            TimelinePaintLineVisual::Text {
                text: "abc".to_owned(),
                indent: 0,
                styles: Vec::new(),
                range: 0..3,
            }
        );
    }

    #[test]
    fn image_slots_take_enough_lines_for_the_thumbnail() {
        assert_eq!(image_slot_lines(20, &STANDARD), 7);
        assert_eq!(image_slot_lines(15, &STANDARD), 10);
        assert_eq!(image_slot_lines(140, &STANDARD), 1);
        assert_eq!(timeline_image_slot_rect(100, 0, 20, &STANDARD).top, 110);
        assert_eq!(timeline_image_slot_rect(115, 1, 15, &STANDARD).top, 115);
    }

    /// A client width whose text area is `text_width` pixels wide.
    fn client_width_for_text(text_width: i32) -> i32 {
        STANDARD.left_padding + text_width + STANDARD.right_padding
    }

    #[test]
    fn timeline_row_paint_lines_wrap_log_rows_with_a_hanging_indent() {
        let text = "[22:13] <@alice:example> alpha beta gamma";
        let indent = 2;

        assert_eq!(
            log_paint_texts(text, client_width_for_text(30)),
            vec![
                ("[22:13] <@alice:example> alpha".to_owned(), 0),
                ("beta gamma".to_owned(), indent),
            ]
        );
        assert_eq!(
            log_paint_texts(text, client_width_for_text(100)),
            vec![(text.to_owned(), 0)]
        );
    }

    #[test]
    fn timeline_row_paint_lines_indent_lines_after_newlines_like_wrapped_lines() {
        let indent = 2;

        assert_eq!(
            log_paint_texts(
                "[22:13] <@alice:example> first\nsecond line here and some more",
                client_width_for_text(30)
            ),
            vec![
                ("[22:13] <@alice:example> first".to_owned(), 0),
                ("second line here and some".to_owned(), indent),
                ("more".to_owned(), indent),
            ]
        );
    }

    fn style_span(start: usize, end: usize, style: TextStyle) -> TextStyleSpan {
        TextStyleSpan { start, end, style }
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

    #[test]
    fn clip_styles_keeps_the_parts_inside_a_segment_at_its_offsets() {
        let styles = [
            style_span(0, 3, bold()),
            style_span(5, 9, italic()),
            style_span(12, 14, bold()),
        ];
        assert_eq!(
            clip_styles(&styles, &(2..10)),
            vec![style_span(0, 1, bold()), style_span(3, 7, italic())]
        );
        assert_eq!(clip_styles(&styles, &(3..5)), Vec::new());
        assert_eq!(clip_styles(&styles, &(0..20)), styles.to_vec());
        assert_eq!(clip_styles(&styles, &(9..9)), Vec::new());
    }

    #[test]
    fn style_pieces_cover_the_text_in_order() {
        assert_eq!(
            style_pieces(
                "plain bold mid it",
                &[style_span(6, 10, bold()), style_span(15, 17, italic())]
            ),
            vec![
                ("plain ", None),
                ("bold", Some(bold())),
                (" mid ", None),
                ("it", Some(italic())),
            ]
        );
        assert_eq!(
            style_pieces("ab", &[style_span(0, 2, bold())]),
            vec![("ab", Some(bold()))]
        );
        assert_eq!(style_pieces("ab", &[]), vec![("ab", None)]);
    }

    #[test]
    fn rendered_line_ranges_split_at_newlines_without_carriage_returns() {
        assert_eq!(rendered_line_ranges(""), Vec::<Range<usize>>::new());
        assert_eq!(
            rendered_line_ranges("a\r\nbc\n\nd"),
            vec![0..1, 3..5, 6..6, 7..8]
        );
    }

    #[test]
    fn styled_log_rows_split_their_spans_across_wrapped_lines() {
        // "[22:13] <@alice:example> alpha beta gamma" with "alpha beta"
        // bold: the break between alpha and beta splits the span, and the
        // dropped space at the break is in neither line.
        let text = "[22:13] <@alice:example> alpha beta gamma\nnext gamma";
        let row = TimelineRow::log(
            TimelineRowKind::Message {
                presentation: RoomMessagePresentationKind::Text,
                send_state: TimelineMessageSendState::Sent,
                highlight: false,
                own: false,
            },
            text.to_owned(),
        )
        .with_styles(vec![
            style_span(25, 35, bold()),
            style_span(36, 41, italic()),
            style_span(47, 52, italic()),
        ]);
        let lines = fresh_paint_lines(&[row], client_width_for_text(30), 20, &measure_ascii_width)
            .into_iter()
            .map(|line| match line.visual {
                TimelinePaintLineVisual::Text { text, styles, .. } => (text, styles),
                TimelinePaintLineVisual::ImageSlot | TimelinePaintLineVisual::Rule => {
                    unreachable!("message rows are text")
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            lines,
            vec![
                (
                    "[22:13] <@alice:example> alpha".to_owned(),
                    vec![style_span(25, 30, bold())]
                ),
                (
                    "beta gamma".to_owned(),
                    vec![style_span(0, 4, bold()), style_span(5, 10, italic())]
                ),
                ("next gamma".to_owned(), vec![style_span(5, 10, italic())]),
            ]
        );
    }

    #[test]
    fn wrap_timeline_visual_line_uses_the_first_width_once_then_the_continuation_width() {
        let wrap = |line: &'static str, first_width, continuation_width| {
            wrap_timeline_visual_line(line, first_width, continuation_width, &measure_ascii_width)
                .into_iter()
                .map(|segment| &line[segment])
                .collect::<Vec<_>>()
        };
        assert_eq!(
            wrap("abcdefghijklmno", 6, 4),
            vec!["abcdef", "ghij", "klmn", "o"]
        );
        assert_eq!(wrap("alpha   beta", 5, 5), vec!["alpha", "beta"]);
        assert_eq!(
            wrap_timeline_visual_line("alpha   beta  ", 5, 5, &measure_ascii_width),
            vec![0..5, 8..12]
        );
        assert_eq!(wrap("", 5, 5), vec![""]);
        assert_eq!(wrap("   ", 5, 5), vec![""]);
    }

    fn wrapped(line: &str, first_width: i32, continuation_width: i32) -> Vec<&str> {
        wrap_timeline_visual_line(line, first_width, continuation_width, &measure_ascii_width)
            .into_iter()
            .map(|segment| &line[segment])
            .collect()
    }

    #[test]
    fn wraps_after_a_comma_when_no_space_fits() {
        assert_eq!(
            wrapped("alpha,beta,gamma", 12, 12),
            vec!["alpha,beta,", "gamma"]
        );
        // A comma just past the width does not fit, so it is no break.
        assert_eq!(
            wrapped("alphabeta,gamma", 9, 9),
            vec!["alphabeta", ",gamma"]
        );
        // The later of a space and a comma wins.
        assert_eq!(wrapped("ab cd,efgh", 8, 8), vec!["ab cd,", "efgh"]);
        assert_eq!(wrapped("ab,cd efgh", 8, 8), vec!["ab,cd", "efgh"]);
    }

    #[test]
    fn a_space_after_a_comma_still_drops_at_the_break() {
        assert_eq!(wrapped("alpha, beta", 8, 8), vec!["alpha,", "beta"]);
        assert_eq!(wrapped("alpha, beta", 6, 6), vec!["alpha,", "beta"]);
    }

    /// The wrap before the binary search: one measure per char until the
    /// width overflows, with comma breaks. Kept to pin the search's result.
    fn linear_reference_wrap(
        line: &str,
        first_width: i32,
        continuation_width: i32,
        measure: &dyn Fn(&str) -> i32,
    ) -> Vec<Range<usize>> {
        if line.is_empty() {
            return vec![Range { start: 0, end: 0 }];
        }
        let mut wrapped = Vec::new();
        let mut start = 0;
        while start < line.len() {
            let remaining = &line[start..];
            let max_width = if wrapped.is_empty() {
                first_width
            } else {
                continuation_width
            };
            if measure(remaining) <= max_width {
                wrapped.push(start..start + trimmed_end_len(remaining));
                break;
            }
            let mut last_fit_end = 0;
            let mut last_break = None;
            let mut first_char_end = None;
            for (index, ch) in remaining.char_indices() {
                let next = index + ch.len_utf8();
                first_char_end.get_or_insert(next);
                if ch.is_whitespace() && index > 0 {
                    last_break = Some(index);
                }
                if measure(&remaining[..next]) <= max_width {
                    last_fit_end = next;
                    if ch == ',' {
                        last_break = Some(next);
                    }
                    continue;
                }
                break;
            }
            let break_index = last_break
                .or_else(|| (last_fit_end > 0).then_some(last_fit_end))
                .or(first_char_end)
                .unwrap();
            wrapped.push(start..start + trimmed_end_len(&remaining[..break_index]));
            let rest = &remaining[break_index..];
            start += break_index + (rest.len() - rest.trim_start().len());
        }
        wrapped
    }

    #[test]
    fn binary_search_bounds_measure_calls() {
        let calls = Cell::new(0usize);
        let counting = |text: &str| {
            calls.set(calls.get() + 1);
            measure_ascii_width(text)
        };
        let line = ["abcdefg"; 251].join(" ");
        let line = &line[..2000];
        let lines = wrap_timeline_visual_line(line, 60, 60, &counting).len();
        // One measure for the whole rest, and a search over at most 2000
        // boundaries, per line.
        assert!(
            calls.get() <= lines * (11 + 3),
            "{} measures for {lines} lines",
            calls.get()
        );

        // A wide char measures 2, so prefix widths are not char counts.
        let wide = |text: &str| {
            text.chars()
                .map(|ch| if ch.len_utf8() > 1 { 2 } else { 1 })
                .sum::<i32>()
        };
        let inputs = [
            "",
            "   ",
            "alpha beta gamma delta",
            "alpha,beta,gamma,delta",
            "a, b, c, d, e, f, g",
            "supercalifragilistic",
            "end-to-end encrypted (!lobby:example)",
            "café crème brûlée à la carte",
            "🙂🙂🙂 🙂🙂🙂🙂🙂 🙂,🙂",
            "  leading and trailing  ",
            ",,,,,,,,,,,,",
            "x",
        ];
        for input in inputs {
            for (first, continuation) in [(1, 1), (3, 2), (5, 5), (6, 4), (10, 8), (40, 40)] {
                for measure in [&measure_ascii_width as &dyn Fn(&str) -> i32, &wide] {
                    assert_eq!(
                        wrap_timeline_visual_line(input, first, continuation, &measure),
                        linear_reference_wrap(input, first, continuation, measure),
                        "{input:?} at {first}/{continuation}"
                    );
                }
            }
        }
    }

    fn numbered_rows(numbers: impl Iterator<Item = usize>) -> Vec<TimelineRow> {
        numbers
            .map(|number| TimelineRow::log(TimelineRowKind::Info, format!("* row {number}")))
            .collect()
    }

    /// How many rows `cache` wraps to lay `rows` out in a text area
    /// `text_width` wide.
    fn wraps(cache: &mut WrapCache, rows: &[TimelineRow], text_width: i32) -> usize {
        timeline_row_paint_lines_with_measure(
            rows,
            client_width_for_text(text_width),
            20,
            &STANDARD,
            cache,
            &measure_ascii_width,
        )
        .1
    }

    #[test]
    fn wrap_cache_wraps_only_new_texts() {
        let mut cache = WrapCache::default();
        assert_eq!(wraps(&mut cache, &numbered_rows(1..=50), 40), 50);
        assert_eq!(wraps(&mut cache, &numbered_rows(1..=51), 40), 1);
        assert_eq!(wraps(&mut cache, &numbered_rows(1..=51), 40), 0);
        // Rows that are not log rows are never wrapped.
        let typing = TimelineRow::full_width(TimelineRowKind::Typing, "* bob is typing".to_owned());
        let mut rows = numbered_rows(1..=51);
        rows.push(typing);
        assert_eq!(wraps(&mut cache, &rows, 40), 0);
    }

    #[test]
    fn wrap_cache_rewraps_everything_at_a_new_width() {
        let mut cache = WrapCache::default();
        let rows = numbered_rows(1..=10);
        assert_eq!(wraps(&mut cache, &rows, 40), 10);
        assert_eq!(wraps(&mut cache, &rows, 41), 10);
        assert_eq!(wraps(&mut cache, &rows, 41), 0);
        cache.clear();
        assert_eq!(wraps(&mut cache, &rows, 41), 10);
    }

    #[test]
    fn wrap_cache_keeps_only_the_current_rows() {
        let mut cache = WrapCache::default();
        assert_eq!(wraps(&mut cache, &numbered_rows(1..=51), 40), 51);
        assert_eq!(wraps(&mut cache, &numbered_rows(2..=51), 40), 0);
        assert_eq!(wraps(&mut cache, &numbered_rows(1..=1), 40), 1);
    }

    #[test]
    fn duplicate_texts_wrap_once() {
        let mut cache = WrapCache::default();
        let rows = numbered_rows([1, 2, 1, 1, 2].into_iter());
        assert_eq!(wraps(&mut cache, &rows, 40), 2);
        // The cached wraps lay out as fresh ones do.
        let lines = timeline_row_paint_lines_with_measure(
            &rows,
            client_width_for_text(4),
            20,
            &STANDARD,
            &mut cache,
            &measure_ascii_width,
        )
        .0;
        assert_eq!(
            lines,
            fresh_paint_lines(&rows, client_width_for_text(4), 20, &measure_ascii_width)
        );
    }

    #[test]
    fn timeline_row_paint_lines_preserve_image_slots_after_wrapped_caption_lines() {
        let mut row = TimelineRow::log(
            TimelineRowKind::Image { highlight: false },
            "[16:00] * @alice:example.org sent image: alpha beta gamma".to_owned(),
        );
        row.image_state = Some(TimelineImageState::Pending);

        let paint_lines =
            fresh_paint_lines(&[row], client_width_for_text(30), 20, &measure_ascii_width);
        let caption_lines = paint_lines
            .iter()
            .take_while(|line| line.image_slot_index.is_none())
            .collect::<Vec<_>>();
        let slot_lines = paint_lines
            .iter()
            .skip(caption_lines.len())
            .collect::<Vec<_>>();

        assert!(
            caption_lines.len() > 1,
            "caption should wrap before image slots"
        );
        assert_eq!(slot_lines.len(), image_slot_lines(20, &STANDARD),);
        assert!(
            slot_lines
                .iter()
                .all(|line| matches!(&line.visual, TimelinePaintLineVisual::ImageSlot))
        );
    }

    #[test]
    fn clamped_timeline_top_line_respects_wrapped_visual_line_growth() {
        let rows = vec![TimelineRow::log(
            TimelineRowKind::Message {
                presentation: RoomMessagePresentationKind::Text,
                send_state: TimelineMessageSendState::Sent,
                highlight: false,
                own: false,
            },
            // Three continuation-width words after a full first line.
            format!(
                "[22:13] <@alice:example> alpha{}",
                format!(" {}", "b".repeat(22)).repeat(3)
            ),
        )];
        let paint_lines =
            fresh_paint_lines(&rows, client_width_for_text(30), 20, &measure_ascii_width);

        assert_eq!(paint_lines.len(), 4);
        assert_eq!(clamped_timeline_top_line(8, paint_lines.len(), 3), 1);
    }

    #[test]
    fn a_line_marker_row_is_one_rule_line() {
        let rows = vec![
            TimelineRow::log(TimelineRowKind::Info, "* above".to_owned()),
            TimelineRow::line_marker(),
            TimelineRow::log(TimelineRowKind::Info, "* below".to_owned()),
        ];
        let lines = fresh_paint_lines(&rows, client_width_for_text(30), 20, &measure_ascii_width);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1].visual, TimelinePaintLineVisual::Rule);
        assert_eq!(lines[1].kind, TimelineRowKind::LineMarker);
        assert_eq!(lines[1].source_row_index, Some(1));
    }

    #[test]
    fn a_rule_is_visible_only_inside_the_shown_lines() {
        let rows = (0..3)
            .map(|line| TimelineRow::log(TimelineRowKind::Info, format!("* {line}")))
            .chain([TimelineRow::line_marker()])
            .chain((4..8).map(|line| TimelineRow::log(TimelineRowKind::Info, format!("* {line}"))))
            .collect::<Vec<_>>();
        let lines = fresh_paint_lines(&rows, client_width_for_text(30), 20, &measure_ascii_width);
        // The rule is line 3.
        assert!(rule_visible(&lines, 0, 4), "at the bottom");
        assert!(!rule_visible(&lines, 0, 3), "just below");
        assert!(rule_visible(&lines, 3, 2), "at the top");
        assert!(!rule_visible(&lines, 4, 4), "just above");
        assert!(!rule_visible(&lines[..3], 0, 10), "no rule");
    }

    fn index(index: u8) -> PaletteIndex {
        PaletteIndex::new(index).unwrap()
    }

    #[test]
    fn activity_levels_use_the_switchbar_colours() {
        let defaults = ActivityColors::default();
        let color = |level| activity_level_color(level, &defaults);
        assert_eq!(color(ViewActivityLevel::Event), index(5).colorref());
        assert_eq!(color(ViewActivityLevel::Message), index(4).colorref());
        assert_eq!(color(ViewActivityLevel::Highlight), index(12).colorref());
        let custom = ActivityColors {
            message: index(9),
            ..defaults
        };
        assert_eq!(
            activity_level_color(ViewActivityLevel::Message, &custom),
            index(9).colorref()
        );
        assert_eq!(
            activity_level_color(ViewActivityLevel::Event, &custom),
            index(5).colorref()
        );
    }

    fn message(
        presentation: RoomMessagePresentationKind,
        send_state: TimelineMessageSendState,
        highlight: bool,
        own: bool,
    ) -> TimelineRowKind {
        TimelineRowKind::Message {
            presentation,
            send_state,
            highlight,
            own,
        }
    }

    /// Every kind of row, with the item it takes under any scheme; `None`
    /// for the fixed error red.
    fn rows_and_items() -> Vec<(TimelineRowKind, Option<ColorItem>)> {
        use RoomMessagePresentationKind::{Emote, Notice, Text};
        use TimelineMessageSendState::{Failed, Pending, Sent};
        let event = |item| TimelineRowKind::Event { item };
        let notice = |item| TimelineRowKind::StateNotice {
            highlight: false,
            item,
        };
        vec![
            (event(EventItem::Join), Some(ColorItem::Join)),
            (event(EventItem::Topic), Some(ColorItem::Topic)),
            (event(EventItem::Invite), Some(ColorItem::Invite)),
            (notice(EventItem::Part), Some(ColorItem::Part)),
            (notice(EventItem::Kick), Some(ColorItem::Kick)),
            (notice(EventItem::Mode), Some(ColorItem::Mode)),
            (notice(EventItem::Nick), Some(ColorItem::Nick)),
            (TimelineRowKind::Info, Some(ColorItem::Info)),
            (TimelineRowKind::Error, None),
            (TimelineRowKind::Typing, Some(ColorItem::Gray)),
            (TimelineRowKind::ReactionSummary, Some(ColorItem::Gray)),
            (TimelineRowKind::ReplyQuote, Some(ColorItem::Gray)),
            (TimelineRowKind::Redacted, Some(ColorItem::Gray)),
            (TimelineRowKind::Plain, Some(ColorItem::Normal)),
            (message(Text, Sent, false, false), Some(ColorItem::Normal)),
            (message(Text, Sent, false, true), Some(ColorItem::Own)),
            (message(Notice, Sent, false, false), Some(ColorItem::Notice)),
            (message(Emote, Sent, false, true), Some(ColorItem::Action)),
            (message(Text, Pending, false, true), Some(ColorItem::Gray)),
            (message(Text, Failed, false, true), None),
            (
                message(Notice, Sent, true, false),
                Some(ColorItem::Highlight),
            ),
            (
                TimelineRowKind::Image { highlight: false },
                Some(ColorItem::Other),
            ),
            (
                TimelineRowKind::Audio { highlight: true },
                Some(ColorItem::Highlight),
            ),
            (
                TimelineRowKind::Unsupported { highlight: false },
                Some(ColorItem::Other),
            ),
            (TimelineRowKind::UnableToDecrypt, Some(ColorItem::Other)),
            (
                TimelineRowKind::StateNotice {
                    highlight: true,
                    item: EventItem::Join,
                },
                Some(ColorItem::Highlight),
            ),
        ]
    }

    #[test]
    fn rows_take_their_items_colour_and_errors_stay_red() {
        let classic = ColorScheme::default();
        for (kind, item) in rows_and_items() {
            let expected = item.map_or(rgb(0xff, 0, 0), |item| classic.colorref(item));
            assert_eq!(
                timeline_row_color(&kind, &classic, index(4)),
                expected,
                "{kind:?}"
            );
        }
        // mIRC Classic's line colours.
        let color = |kind| timeline_row_color(&kind, &classic, index(4));
        assert_eq!(
            color(TimelineRowKind::Event {
                item: EventItem::Join
            }),
            rgb(0, 0x93, 0)
        );
        assert_eq!(
            color(message(
                RoomMessagePresentationKind::Text,
                TimelineMessageSendState::Sent,
                true,
                false
            )),
            rgb(0x7f, 0, 0)
        );
        assert_eq!(color(TimelineRowKind::Typing), rgb(0x7f, 0x7f, 0x7f));
    }

    #[test]
    fn the_line_marker_takes_its_own_colour_in_any_scheme() {
        let classic = ColorScheme::default();
        for marker in [index(4), index(12)] {
            assert_eq!(
                timeline_row_color(&TimelineRowKind::LineMarker, &classic, marker),
                marker.colorref()
            );
        }
        let mut scheme = classic.clone();
        for item in ColorItem::ALL {
            scheme.set(item, index(13));
        }
        assert_eq!(
            timeline_row_color(&TimelineRowKind::LineMarker, &scheme, index(4)),
            index(4).colorref()
        );
    }

    #[test]
    fn changing_an_item_moves_only_its_rows() {
        let classic = ColorScheme::default();
        for changed in ColorItem::ALL {
            let mut scheme = classic.clone();
            // A colour no default uses.
            scheme.set(changed, index(13));
            for (kind, item) in rows_and_items() {
                let moved = timeline_row_color(&kind, &scheme, index(4))
                    != timeline_row_color(&kind, &classic, index(4));
                assert_eq!(moved, item == Some(changed), "{changed:?} {kind:?}");
            }
        }
    }
}
