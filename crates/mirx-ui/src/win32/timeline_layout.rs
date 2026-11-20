//! Timeline layout and paint: rows become wrapped paint lines, and paint
//! lines are drawn in the frame's text font.

use std::ops::Range;

use mirx_core::{TextColor, TextStyle, TextStyleSpan};
use windows_sys::Win32::Graphics::Gdi::HDC;

use super::mirc_format::MIRC_PALETTE;
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
    /// the row's styles clipped to it, at their offsets in `text`.
    Text {
        text: String,
        indent: i32,
        styles: Vec<TextStyleSpan>,
    },
    ImageSlot,
}

/// The width available to timeline text, never less than one pixel so
/// wrapping always makes progress in a collapsed view.
fn timeline_text_width(client_width: i32) -> i32 {
    (client_width - TIMELINE_LEFT_PADDING - TIMELINE_RIGHT_PADDING).max(1)
}

pub(super) fn plain_timeline_paint_lines(lines: &[String]) -> Vec<TimelinePaintLine> {
    lines
        .iter()
        .cloned()
        .map(|text| TimelinePaintLine {
            kind: TimelineRowKind::Plain,
            visual: TimelinePaintLineVisual::Text {
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
/// `first_width`, later ones `continuation_width`. Breaks prefer
/// whitespace, which is dropped at the break; unbroken tokens are split at
/// the last fitting character.
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

        let mut last_fit_end = 0;
        let mut last_whitespace_break = None;
        let mut first_char_end = None;
        for (index, ch) in remaining.char_indices() {
            let next = index + ch.len_utf8();
            if first_char_end.is_none() {
                first_char_end = Some(next);
            }
            // Whitespace is dropped at a break, so a space just past the
            // width still ends a line whose text fits.
            if ch.is_whitespace() && index > 0 {
                last_whitespace_break = Some(index);
            }
            if measure(&remaining[..next]) <= max_width {
                last_fit_end = next;
                continue;
            }
            break;
        }

        let break_index = last_whitespace_break
            .or_else(|| (last_fit_end > 0).then_some(last_fit_end))
            .or(first_char_end)
            .expect("a non-empty timeline body line must have at least one char boundary");
        wrapped.push(start..start + trimmed_end_len(&remaining[..break_index]));
        let rest = &remaining[break_index..];
        start += break_index + (rest.len() - rest.trim_start().len());
    }

    wrapped
}

/// Paint lines for a log row, as ranges of `text` with their indent: the
/// first line starts at the left padding and every continuation, whether
/// wrapped or from a newline in the text, is indented past the timestamp
/// like mIRC's hanging indent: by the width of `indent_sample`, a timestamp
/// prefix.
fn timeline_log_paint_ranges<Measure>(
    text: &str,
    width: i32,
    indent_sample: &str,
    measure: &Measure,
) -> Vec<(Range<usize>, i32)>
where
    Measure: Fn(&str) -> i32,
{
    let indent = measure(indent_sample).min(width - 1);
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

/// The parts of `styles` inside `segment`, at their offsets in it.
fn clip_styles(styles: &[TextStyleSpan], segment: &Range<usize>) -> Vec<TextStyleSpan> {
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

fn timeline_row_paint_lines_with_measure<Measure>(
    rows: &[TimelineRow],
    client_width: i32,
    line_height: i32,
    indent_sample: &str,
    measure: &Measure,
) -> Vec<TimelinePaintLine>
where
    Measure: Fn(&str) -> i32,
{
    let width = timeline_text_width(client_width);
    let mut paint_lines = Vec::new();
    for (source_row_index, row) in rows.iter().enumerate() {
        let segments = match row.layout {
            TimelineRowLayout::FullWidth => rendered_line_ranges(&row.logical_text)
                .into_iter()
                .map(|line| (line, 0))
                .collect(),
            TimelineRowLayout::Log => {
                timeline_log_paint_ranges(&row.logical_text, width, indent_sample, measure)
            }
        };
        for (segment, indent) in segments {
            paint_lines.push(TimelinePaintLine {
                kind: row.kind.clone(),
                visual: TimelinePaintLineVisual::Text {
                    text: row.logical_text[segment.clone()].to_owned(),
                    indent,
                    styles: clip_styles(&row.styles, &segment),
                },
                image_state: None,
                image_slot_index: None,
                source_row_index: Some(source_row_index),
            });
        }
        if let Some(image_state) = &row.image_state {
            for image_slot_index in 0..image_slot_lines(line_height) {
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
    paint_lines
}

pub(super) unsafe fn timeline_row_paint_lines(
    hwnd: HWND,
    rows: &[TimelineRow],
    client_width: i32,
) -> Vec<TimelinePaintLine> {
    let state = timeline_state_ref(hwnd);
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
    let paint_lines = timeline_row_paint_lines_with_measure(
        rows,
        client_width,
        state.line_height,
        &state.indent_sample,
        &|text| unsafe { glyphs.measure(hdc, text) },
    );
    ReleaseDC(hwnd, hdc);
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

pub(super) unsafe fn paint_timeline(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint);
    FillRect(hdc, &paint.rcPaint, (COLOR_WINDOW as isize + 1) as HBRUSH);
    SetBkMode(hdc, TRANSPARENT as i32);
    let mut client_rect = RECT::default();
    GetClientRect(hwnd, &mut client_rect);
    let frame_state = state_ref(frame_window_for(hwnd));
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
    let visible_rows = timeline_visible_rows(hwnd);
    for (row, line) in state
        .paint_lines
        .iter()
        .skip(state.top_line)
        .take(visible_rows)
        .enumerate()
    {
        let line_y = state.vertical_padding + row as i32 * state.line_height;
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
            } => {
                let left = TIMELINE_LEFT_PADDING + indent;
                let rect = RECT {
                    left,
                    top: line_y,
                    right: (client_rect.right - TIMELINE_RIGHT_PADDING).max(left + 1),
                    bottom: line_y + state.line_height,
                };
                paint_styled_text(
                    hdc,
                    glyphs,
                    left,
                    &rect,
                    text,
                    styles,
                    timeline_row_color(&line.kind),
                );
            }
            TimelinePaintLineVisual::ImageSlot => {}
        }
    }

    DeleteObject(find_highlight_brush);
    DeleteObject(selection_brush);
    SelectObject(hdc, previous);

    EndPaint(hwnd, &paint);
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

/// A style colour as GDI takes it.
const fn colorref(color: TextColor) -> u32 {
    rgb(color.red, color.green, color.blue)
}

pub(super) const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}

// The colours of mIRC's own text, by their colour code index.
pub(super) const MIRC_MAROON: u32 = colorref(MIRC_PALETTE[5]);
pub(super) const MIRC_RED: u32 = colorref(MIRC_PALETTE[4]);
pub(super) const MIRC_BLUE: u32 = colorref(MIRC_PALETTE[12]);
const MIRC_PURPLE: u32 = colorref(MIRC_PALETTE[6]);
const MIRC_GREEN: u32 = colorref(MIRC_PALETTE[3]);
const MIRC_NAVY: u32 = colorref(MIRC_PALETTE[2]);

// mIRC's switchbar/treebar activity colours (Options > Display > Event,
// Message, Highlight): an inactive window's button text takes the colour of
// what happened in it. Message red is mIRC's well-known default; event
// maroon and highlight blue are best recollection of mIRC 7's defaults and
// are pending a check against a real install. Highlighted timeline lines use
// the highlight colour too.
const ACTIVITY_EVENT_COLOR: u32 = MIRC_MAROON;
const ACTIVITY_MESSAGE_COLOR: u32 = MIRC_RED;
const ACTIVITY_HIGHLIGHT_COLOR: u32 = MIRC_BLUE;

pub(super) fn activity_level_color(level: ViewActivityLevel) -> u32 {
    match level {
        ViewActivityLevel::Event => ACTIVITY_EVENT_COLOR,
        ViewActivityLevel::Message => ACTIVITY_MESSAGE_COLOR,
        ViewActivityLevel::Highlight => ACTIVITY_HIGHLIGHT_COLOR,
    }
}

fn timeline_row_color(kind: &TimelineRowKind) -> u32 {
    match kind {
        // mIRC draws a highlighted line wholly in the highlight colour.
        TimelineRowKind::Message {
            highlight: true, ..
        }
        | TimelineRowKind::Image { highlight: true }
        | TimelineRowKind::File { highlight: true }
        | TimelineRowKind::Audio { highlight: true }
        | TimelineRowKind::Video { highlight: true }
        | TimelineRowKind::Unsupported { highlight: true }
        | TimelineRowKind::StateNotice { highlight: true } => ACTIVITY_HIGHLIGHT_COLOR,
        TimelineRowKind::Plain => rgb(0x00, 0x00, 0x00),
        TimelineRowKind::Event => MIRC_GREEN,
        TimelineRowKind::Info => MIRC_NAVY,
        TimelineRowKind::Error => MIRC_RED,
        TimelineRowKind::Typing => rgb(0x68, 0x68, 0x68),
        // Dim annotations around a message.
        TimelineRowKind::ReactionSummary | TimelineRowKind::ReplyQuote => rgb(0x6B, 0x6B, 0x6B),
        TimelineRowKind::Message {
            send_state: TimelineMessageSendState::Pending,
            ..
        } => rgb(0x4E, 0x5D, 0x69),
        TimelineRowKind::Message {
            send_state: TimelineMessageSendState::Failed,
            ..
        } => rgb(0x86, 0x22, 0x22),
        // mIRC's default palette: notices maroon, actions purple, and
        // joins/parts/kicks/nick and topic changes green.
        TimelineRowKind::Message {
            presentation: RoomMessagePresentationKind::Notice,
            send_state: TimelineMessageSendState::Sent,
            ..
        } => MIRC_MAROON,
        TimelineRowKind::Message {
            presentation: RoomMessagePresentationKind::Emote,
            send_state: TimelineMessageSendState::Sent,
            ..
        } => MIRC_PURPLE,
        TimelineRowKind::Message { .. } => rgb(0x00, 0x00, 0x00),
        TimelineRowKind::Image { .. }
        | TimelineRowKind::File { .. }
        | TimelineRowKind::Audio { .. }
        | TimelineRowKind::Video { .. } => MIRC_PURPLE,
        TimelineRowKind::Unsupported { .. } => rgb(0x7A, 0x46, 0x16),
        TimelineRowKind::UnableToDecrypt => rgb(0x8A, 0x5A, 0x18),
        TimelineRowKind::StateNotice { .. } => MIRC_GREEN,
    }
}

/// How many lines an image's slot takes: enough for the thumbnail and its
/// padding.
fn image_slot_lines(line_height: i32) -> usize {
    let slot_height = TIMELINE_IMAGE_THUMBNAIL_MAX_HEIGHT + TIMELINE_IMAGE_SLOT_PADDING;
    let line_height = u32::try_from(line_height).expect("a line height is positive");
    (slot_height as u32).div_ceil(line_height) as usize
}

/// The thumbnail's rect, centred in its slot, from the slot line at
/// `line_y` that is the slot's `image_slot_index`th.
fn timeline_image_slot_rect(line_y: i32, image_slot_index: usize, line_height: i32) -> RECT {
    let slot_height = image_slot_lines(line_height) as i32 * line_height;
    let top = line_y + (slot_height - TIMELINE_IMAGE_THUMBNAIL_MAX_HEIGHT) / 2
        - image_slot_index as i32 * line_height;
    let left = TIMELINE_LEFT_PADDING + TIMELINE_IMAGE_LEFT_INDENT;
    RECT {
        left,
        top,
        right: left + TIMELINE_IMAGE_THUMBNAIL_MAX_WIDTH,
        bottom: top + TIMELINE_IMAGE_THUMBNAIL_MAX_HEIGHT,
    }
}

unsafe fn paint_timeline_image_slot(
    hwnd: HWND,
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    line_y: i32,
    image_slot_index: usize,
    image_state: &TimelineImageState,
) {
    let rect = timeline_image_slot_rect(
        line_y,
        image_slot_index,
        timeline_state_ref(hwnd).line_height,
    );
    match image_state {
        TimelineImageState::Pending => {
            paint_timeline_image_placeholder(hdc, &rect, "loading image...");
        }
        TimelineImageState::Loaded { key, width, height } => {
            paint_timeline_loaded_image(hwnd, hdc, &rect, key, *width, *height);
        }
        TimelineImageState::Failed { .. } => {
            paint_timeline_image_placeholder(hdc, &rect, "image unavailable");
        }
    }
}

unsafe fn paint_timeline_image_placeholder(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: &RECT,
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
        rect.left + 8,
        rect.top + 8,
        text.as_ptr(),
        text.len() as i32 - 1,
    );
}

unsafe fn paint_timeline_loaded_image(
    hwnd: HWND,
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: &RECT,
    key: &str,
    width: u32,
    height: u32,
) {
    let window_state = timeline_window_state(hwnd);
    let Some(image) = window_state.decoded_images.get(key) else {
        paint_timeline_image_placeholder(hdc, rect, "image unavailable");
        return;
    };
    let (draw_width, draw_height) = scaled_thumbnail_dimensions(
        width,
        height,
        TIMELINE_IMAGE_THUMBNAIL_MAX_WIDTH,
        TIMELINE_IMAGE_THUMBNAIL_MAX_HEIGHT,
    );
    let draw_x = rect.left + (TIMELINE_IMAGE_THUMBNAIL_MAX_WIDTH - draw_width) / 2;
    let draw_y = rect.top + (TIMELINE_IMAGE_THUMBNAIL_MAX_HEIGHT - draw_height) / 2;

    let border = CreateSolidBrush(rgb(0x7A, 0x7A, 0x7A));
    if !border.is_null() {
        FrameRect(hdc, rect, border);
        DeleteObject(border as _);
    }

    let mut graphics = null_mut::<GpGraphics>();
    if GdipCreateFromHDC(hdc, &mut graphics) != 0 {
        paint_timeline_image_placeholder(hdc, rect, "image unavailable");
        return;
    }
    let status = GdipDrawImageRectI(graphics, image.raw, draw_x, draw_y, draw_width, draw_height);
    GdipDeleteGraphics(graphics);
    if status != 0 {
        paint_timeline_image_placeholder(hdc, rect, "image unavailable");
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
    use super::*;

    fn measure_ascii_width(text: &str) -> i32 {
        text.chars().count() as i32
    }

    #[test]
    fn the_hanging_indent_is_as_wide_as_the_timestamp_format() {
        let mut setting = TimestampSetting {
            show: true,
            format: timestamp::TimestampFormat::parse("[HH:nn:ss]").unwrap(),
        };
        let indent_of = |setting: &TimestampSetting| {
            let sample = TimestampStyle::new(setting, timestamp::utc_local_time).indent_sample();
            let rows = [TimelineRow::log(
                TimelineRowKind::Info,
                format!("{sample}alpha beta gamma"),
            )];
            timeline_row_paint_lines_with_measure(
                &rows,
                client_width_for_text(sample.len() as i32 + 10),
                20,
                &sample,
                &measure_ascii_width,
            )
            .into_iter()
            .map(|line| match line.visual {
                TimelinePaintLineVisual::Text { indent, .. } => indent,
                TimelinePaintLineVisual::ImageSlot => unreachable!("info rows have no image"),
            })
            .collect::<Vec<_>>()
        };

        assert_eq!(indent_of(&setting), vec![0, 11]);
        setting.show = false;
        assert_eq!(indent_of(&setting), vec![0, 0]);
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
        let paint_lines =
            timeline_row_paint_lines_with_measure(&rows, 360, 20, "[00:00] ", &measure_ascii_width);

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
            image_slot_lines(20)
        );
        assert_eq!(clamped_timeline_top_line(6, paint_lines.len(), 3), 6);
    }

    fn log_paint_texts(text: &str, client_width: i32) -> Vec<(String, i32)> {
        let rows = vec![TimelineRow::log(
            TimelineRowKind::Message {
                presentation: RoomMessagePresentationKind::Text,
                send_state: TimelineMessageSendState::Sent,
                highlight: false,
            },
            text.to_owned(),
        )];
        timeline_row_paint_lines_with_measure(
            &rows,
            client_width,
            20,
            "[00:00] ",
            &measure_ascii_width,
        )
        .into_iter()
        .map(|line| match line.visual {
            TimelinePaintLineVisual::Text { text, indent, .. } => (text, indent),
            TimelinePaintLineVisual::ImageSlot => unreachable!("message rows have no image"),
        })
        .collect()
    }

    #[test]
    fn image_slots_take_enough_lines_for_the_thumbnail() {
        assert_eq!(image_slot_lines(20), 7);
        assert_eq!(image_slot_lines(15), 10);
        assert_eq!(image_slot_lines(140), 1);
        assert_eq!(timeline_image_slot_rect(100, 0, 20).top, 110);
        assert_eq!(timeline_image_slot_rect(115, 1, 15).top, 115);
    }

    /// A client width whose text area is `text_width` pixels wide.
    fn client_width_for_text(text_width: i32) -> i32 {
        TIMELINE_LEFT_PADDING + text_width + TIMELINE_RIGHT_PADDING
    }

    #[test]
    fn timeline_row_paint_lines_wrap_log_rows_with_a_hanging_indent() {
        let text = "[22:13] <@alice:example> alpha beta gamma";
        let indent = "[00:00] ".len() as i32;

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
        let indent = "[00:00] ".len() as i32;

        assert_eq!(
            log_paint_texts(
                "[22:13] <@alice:example> first\nsecond line here and more",
                client_width_for_text(30)
            ),
            vec![
                ("[22:13] <@alice:example> first".to_owned(), 0),
                ("second line here and".to_owned(), indent),
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
            },
            text.to_owned(),
        )
        .with_styles(vec![
            style_span(25, 35, bold()),
            style_span(36, 41, italic()),
            style_span(47, 52, italic()),
        ]);
        let lines = timeline_row_paint_lines_with_measure(
            &[row],
            client_width_for_text(30),
            20,
            "[00:00] ",
            &measure_ascii_width,
        )
        .into_iter()
        .map(|line| match line.visual {
            TimelinePaintLineVisual::Text { text, styles, .. } => (text, styles),
            TimelinePaintLineVisual::ImageSlot => unreachable!("message rows have no image"),
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

    #[test]
    fn timeline_row_paint_lines_preserve_image_slots_after_wrapped_caption_lines() {
        let mut row = TimelineRow::log(
            TimelineRowKind::Image { highlight: false },
            "[16:00] * @alice:example.org sent image: alpha beta gamma".to_owned(),
        );
        row.image_state = Some(TimelineImageState::Pending);

        let paint_lines = timeline_row_paint_lines_with_measure(
            &[row],
            client_width_for_text(30),
            20,
            "[00:00] ",
            &measure_ascii_width,
        );
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
        assert_eq!(slot_lines.len(), image_slot_lines(20),);
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
            },
            // Three continuation-width words after a full first line.
            format!(
                "[22:13] <@alice:example> alpha{}",
                format!(" {}", "b".repeat(22)).repeat(3)
            ),
        )];
        let paint_lines = timeline_row_paint_lines_with_measure(
            &rows,
            client_width_for_text(30),
            20,
            "[00:00] ",
            &measure_ascii_width,
        );

        assert_eq!(paint_lines.len(), 4);
        assert_eq!(clamped_timeline_top_line(8, paint_lines.len(), 3), 1);
    }

    #[test]
    fn activity_levels_use_mirc_switchbar_colors() {
        assert_eq!(activity_level_color(ViewActivityLevel::Event), MIRC_MAROON);
        assert_eq!(activity_level_color(ViewActivityLevel::Message), MIRC_RED);
        assert_eq!(
            activity_level_color(ViewActivityLevel::Highlight),
            MIRC_BLUE
        );
    }

    #[test]
    fn highlighted_rows_use_the_highlight_color_whatever_their_presentation() {
        for presentation in [
            RoomMessagePresentationKind::Text,
            RoomMessagePresentationKind::Emote,
            RoomMessagePresentationKind::Notice,
        ] {
            let row = |highlight| TimelineRowKind::Message {
                presentation,
                send_state: TimelineMessageSendState::Sent,
                highlight,
            };
            assert_eq!(timeline_row_color(&row(true)), MIRC_BLUE);
            assert_ne!(timeline_row_color(&row(false)), MIRC_BLUE);
        }
        assert_eq!(
            timeline_row_color(&TimelineRowKind::Image { highlight: true }),
            MIRC_BLUE
        );
        assert_eq!(
            timeline_row_color(&TimelineRowKind::StateNotice { highlight: true }),
            MIRC_BLUE
        );
        assert_eq!(
            timeline_row_color(&TimelineRowKind::StateNotice { highlight: false }),
            MIRC_GREEN
        );
    }
}
