//! The frame's text font: choosing it, its metrics, and applying it to
//! the views.

use super::*;

/// A GDI font the frame owns. Controls it is set on with `WM_SETFONT` only
/// borrow it, so it must outlive them.
pub(super) struct OwnedFont(pub(super) HFONT);

impl Drop for OwnedFont {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.0);
        }
    }
}

/// The text font of every window (`/font`) with its fallback chain, and the
/// heights laid out from it.
pub(super) struct TextFont {
    pub(super) glyphs: GlyphFonts,
    /// The height of an editbox showing one line in the font.
    pub(super) edit_height: i32,
}

/// Text height plus room around it inside an editbox's client area.
const EDIT_TEXT_MARGIN: i32 = 4;

/// The screen's vertical dots per inch, which points convert with.
unsafe fn screen_dpi() -> i32 {
    let screen = GetDC(null_mut());
    assert!(!screen.is_null(), "GetDC must succeed for the screen");
    let dpi = GetDeviceCaps(screen, LOGPIXELSY as i32);
    ReleaseDC(null_mut(), screen);
    dpi
}

unsafe fn font_setting_logfont(setting: &FontSetting) -> LOGFONTW {
    let face = setting.face().encode_utf16().collect::<Vec<_>>();
    let mut logfont = LOGFONTW {
        // Negative: the character height, as points are measured.
        lfHeight: -((i32::from(setting.points()) * screen_dpi() + 36) / 72),
        lfWeight: if setting.bold() { FW_BOLD } else { FW_NORMAL } as i32,
        lfCharSet: DEFAULT_CHARSET,
        ..LOGFONTW::default()
    };
    assert!(
        face.len() < logfont.lfFaceName.len(),
        "a font setting's face must fit LOGFONTW"
    );
    logfont.lfFaceName[..face.len()].copy_from_slice(&face);
    logfont
}

impl TextFont {
    pub(super) unsafe fn create(setting: &FontSetting) -> Result<Self, String> {
        let font = CreateFontIndirectW(&font_setting_logfont(setting));
        if font.is_null() {
            return Err(last_os_error("CreateFontIndirectW"));
        }
        let glyphs = GlyphFonts::create(OwnedFont(font), setting.bold())?;

        let mut edit_rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: glyphs.metrics().tmHeight + EDIT_TEXT_MARGIN,
        };
        if AdjustWindowRectEx(&mut edit_rect, COMPOSE_EDIT_STYLE, 0, COMPOSE_EDIT_EX_STYLE) == 0 {
            return Err(last_os_error("AdjustWindowRectEx editbox"));
        }
        Ok(Self {
            glyphs,
            edit_height: edit_rect.bottom - edit_rect.top,
        })
    }

    pub(super) fn hfont(&self) -> HFONT {
        self.glyphs.hfont()
    }
}

/// The metrics of `font`, or of the system font for null.
unsafe fn font_metrics(font: HFONT) -> TEXTMETRICW {
    let screen = GetDC(null_mut());
    assert!(!screen.is_null(), "GetDC must succeed for the screen");
    let previous = (!font.is_null()).then(|| SelectObject(screen, font));
    let mut metrics = TEXTMETRICW::default();
    let measured = GetTextMetricsW(screen, &mut metrics);
    if let Some(previous) = previous {
        SelectObject(screen, previous);
    }
    ReleaseDC(null_mut(), screen);
    assert!(measured != 0, "GetTextMetricsW must succeed on the screen");
    metrics
}

/// The distance between two lines of text in `font`.
pub(super) unsafe fn font_line_height(font: HFONT) -> i32 {
    let metrics = font_metrics(font);
    metrics.tmHeight + metrics.tmExternalLeading
}

/// `WM_SETFONT`: lines are as tall as the font's, so the rows rewrap.
pub(super) unsafe fn set_timeline_font(hwnd: HWND, font: HFONT) {
    {
        let state = timeline_state_mut(hwnd);
        state.font = font;
        state.line_height = font_line_height(font);
    }
    rebuild_timeline_paint_lines(hwnd);
    clamp_timeline_top_line(hwnd);
    update_timeline_scrollbar(hwnd);
}

/// Sets the text font on the controls of a view's window that draw text in
/// it: the text area, the nick list and the editbox, as in mIRC.
pub(super) unsafe fn set_view_text_font(view_id: &ViewId, child: HWND, font: HFONT) {
    let content = mdi_child_content(child);
    if *view_id == ViewId::RoomList {
        // The list is the whole window; there is no editbox.
        SendMessageW(content, WM_SETFONT, font as WPARAM, 1);
        return;
    }
    SendMessageW(
        view_timeline(view_id, content),
        WM_SETFONT,
        font as WPARAM,
        1,
    );
    match view_id {
        ViewId::Status => {}
        ViewId::RoomList => unreachable!("the Rooms List returned above"),
        ViewId::Room(_) | ViewId::Thread { .. } => {
            let members = room_content_member_list(content);
            SendMessageW(members, WM_SETFONT, font as WPARAM, 1);
            // Owner-drawn fixed items take their height from WM_MEASUREITEM
            // once, at creation.
            SendMessageW(
                members,
                LB_SETITEMHEIGHT,
                0,
                text_font_item_height(content) as LPARAM,
            );
        }
    }
    SendMessageW(mdi_child_edit(child), WM_SETFONT, font as WPARAM, 1);
}

/// Makes `setting` the text font of every window and saves it. The new
/// font is set on every control before the old one is deleted.
pub(super) unsafe fn apply_font_setting(
    hwnd: HWND,
    setting: FontSetting,
) -> Result<String, String> {
    let text_font = TextFont::create(&setting)?;
    let status = format!("font: {}", setting.label());
    state_mut(hwnd)
        .settings
        .update(|settings| settings.text_font = setting)?;
    let previous = std::mem::replace(&mut state_mut(hwnd).text_font, text_font);
    let views = state_ref(hwnd)
        .views
        .iter()
        .map(|(view_id, view)| (view_id.clone(), view.child))
        .collect::<Vec<_>>();
    let font = state_ref(hwnd).text_font.hfont();
    for (view_id, child) in &views {
        set_view_text_font(view_id, *child, font);
        layout_mdi_child(*child);
    }
    drop(previous);
    Ok(status)
}

/// The standard font dialog, starting at the current font; `None` when
/// cancelled.
unsafe fn choose_text_font(hwnd: HWND) -> Result<Option<FontSetting>, String> {
    let mut logfont = font_setting_logfont(&state_ref(hwnd).settings.settings().text_font);
    let mut choose = CHOOSEFONTW {
        lStructSize: size_of::<CHOOSEFONTW>() as u32,
        hwndOwner: hwnd,
        lpLogFont: &mut logfont,
        Flags: CF_SCREENFONTS | CF_INITTOLOGFONTSTRUCT | CF_NOVERTFONTS | CF_LIMITSIZE,
        nSizeMin: 1,
        nSizeMax: 72,
        ..CHOOSEFONTW::default()
    };
    if ChooseFontW(&mut choose) == 0 {
        return match CommDlgExtendedError() {
            0 => Ok(None),
            code => Err(format!("ChooseFontW failed: {code:#x}")),
        };
    }
    let face_len = logfont
        .lfFaceName
        .iter()
        .position(|&unit| unit == 0)
        .expect("LOGFONTW face names are NUL-terminated");
    let face = String::from_utf16_lossy(&logfont.lfFaceName[..face_len]);
    let points = u8::try_from(choose.iPointSize / 10).expect("CF_LIMITSIZE keeps sizes to 1-72");
    FontSetting::new(&face, points, logfont.lfWeight >= FW_BOLD as i32).map(Some)
}

/// `/font` with no parameters, and every window menu's Font...
pub(super) unsafe fn run_font_dialog(hwnd: HWND) -> Result<Option<String>, String> {
    // Modal: core events keep arriving, so nothing is held across it.
    match choose_text_font(hwnd)? {
        Some(setting) => apply_font_setting(hwnd, setting).map(Some),
        None => Ok(None),
    }
}
