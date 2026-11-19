//! Per-character font fallback, as mIRC does it: every character of a line
//! draws in the first font of a chain that has a glyph for it, the text font
//! first. GDI does no fallback for raster fonts such as Fixedsys, and
//! Uniscribe's falls back per script item to one font per script, so mirx
//! keeps its own chain with each font's coverage read from the font.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{RECT, SIZE};
#[cfg(test)]
use windows_sys::Win32::Graphics::Gdi::GGI_MARK_NONEXISTING_GLYPHS;
use windows_sys::Win32::Graphics::Gdi::{
    CreateFontIndirectW, DEFAULT_CHARSET, ETO_CLIPPED, ETO_GLYPH_INDEX, ExtTextOutW, FW_BOLD,
    FW_NORMAL, GDI_ERROR, GetDC, GetFontData, GetGlyphIndicesW, GetTextExtentPoint32W,
    GetTextExtentPointI, GetTextFaceW, GetTextMetricsW, HDC, HFONT, LOGFONTW, ReleaseDC,
    SelectObject, SetTextAlign, TA_BASELINE, TA_LEFT, TEXTMETRICW, TMPF_TRUETYPE,
};

use super::OwnedFont;
use super::cmap::{self, Cmap};
use super::glyph_runs::{GlyphRun, split_runs};

/// The fonts tried after the text font, in order, when installed. Text
/// faces come before symbol and emoji faces so that punctuation and letters
/// take a text face. Segoe UI covers Latin, Greek, Cyrillic, Hebrew, Arabic
/// and Armenian on Windows 10+; Tahoma stands in for it under Wine; the rest
/// cover symbols, emoji and the other scripts.
const FALLBACK_FACES: &[&str] = &[
    "Segoe UI",
    "Tahoma",
    "Segoe UI Symbol",
    "Segoe UI Emoji",
    "Noto Emoji",
    "Nirmala UI",
    "Leelawadee UI",
    "Microsoft YaHei",
    "Microsoft JhengHei",
    "Yu Gothic",
    "Malgun Gothic",
    "Ebrima",
    "Gadugi",
    "Myanmar Text",
    "Mongolian Baiti",
    "Microsoft Himalaya",
    "Sylfaen",
    "Segoe UI Historic",
    "Arial Unicode MS",
];

/// Which characters a font draws, read the way the font is drawn.
enum Coverage {
    /// A TrueType font: its own `cmap`, which also gives the glyph ids it is
    /// drawn with, so characters above the BMP work.
    Cmap(Cmap),
    /// Any other font (raster, vector): the sorted BMP characters GDI maps
    /// to a glyph other than the font's default one. Drawn as UTF-16.
    /// (`GetFontUnicodeRanges` cannot serve: Wine reports no ranges for
    /// raster fonts.)
    Chars(Vec<u16>),
}

impl Coverage {
    fn contains(&self, c: char) -> bool {
        match self {
            Coverage::Cmap(cmap) => cmap.glyph(c).is_some(),
            Coverage::Chars(chars) => {
                u16::try_from(u32::from(c)).is_ok_and(|unit| chars.binary_search(&unit).is_ok())
            }
        }
    }
}

struct GlyphFace {
    font: OwnedFont,
    /// The face name GDI reports for `font`.
    face: String,
    coverage: Coverage,
}

/// The text font and the fallback fonts installed on this machine, all of
/// the text font's cell height and weight and drawn on its baseline, so
/// lines keep the text font's height.
pub(super) struct GlyphFonts {
    /// The text font first, then the installed fallbacks in chain order.
    faces: Vec<GlyphFace>,
    metrics: TEXTMETRICW,
}

/// A run's characters as GDI takes them.
enum RunText {
    /// Glyph ids, for `ETO_GLYPH_INDEX`.
    Glyphs(Vec<u16>),
    Utf16(Vec<u16>),
}

impl GlyphFonts {
    /// Builds the chain behind `primary`, the text font, created `bold` or
    /// not. A fallback face that is not installed, is substituted by another
    /// face, is not TrueType or has an unreadable `cmap` is left out of the
    /// chain: these are facts about the installed fonts. The text font's
    /// coverage must be readable.
    pub(super) unsafe fn create(primary: OwnedFont, bold: bool) -> Result<Self, String> {
        let screen = GetDC(null_mut());
        assert!(!screen.is_null(), "GetDC must succeed for the screen");
        let result = Self::create_on(screen, primary, bold);
        ReleaseDC(null_mut(), screen);
        result
    }

    unsafe fn create_on(screen: HDC, primary: OwnedFont, bold: bool) -> Result<Self, String> {
        let previous = SelectObject(screen, primary.0);
        let metrics = text_metrics(screen);
        let face = text_face(screen);
        let coverage = if metrics.tmPitchAndFamily & TMPF_TRUETYPE != 0 {
            font_cmap(screen).map(Coverage::Cmap)
        } else {
            Ok(Coverage::Chars(gdi_covered_chars(screen, &metrics)))
        };
        SelectObject(screen, previous);
        let mut faces = vec![GlyphFace {
            font: primary,
            face,
            coverage: coverage.map_err(|error| format!("text font {error}"))?,
        }];

        for &candidate in FALLBACK_FACES {
            let font = OwnedFont(CreateFontIndirectW(&fallback_logfont(
                candidate,
                metrics.tmHeight,
                bold,
            )));
            assert!(!font.0.is_null(), "CreateFontIndirectW must succeed");
            let previous = SelectObject(screen, font.0);
            let face = text_face(screen);
            let truetype = text_metrics(screen).tmPitchAndFamily & TMPF_TRUETYPE != 0;
            let cmap = (face.eq_ignore_ascii_case(candidate) && truetype)
                .then(|| font_cmap(screen).ok())
                .flatten();
            SelectObject(screen, previous);
            if let Some(cmap) = cmap {
                faces.push(GlyphFace {
                    font,
                    face,
                    coverage: Coverage::Cmap(cmap),
                });
            }
        }
        Ok(Self { faces, metrics })
    }

    /// The text font.
    pub(super) fn hfont(&self) -> HFONT {
        self.faces[0].font.0
    }

    /// The text font's metrics.
    pub(super) fn metrics(&self) -> &TEXTMETRICW {
        &self.metrics
    }

    /// The face names of the chain, the text font first.
    pub(super) fn faces(&self) -> impl Iterator<Item = &str> {
        self.faces.iter().map(|face| face.face.as_str())
    }

    /// `text` split into runs by the font that draws them; a run's `slot`
    /// indexes `faces()`.
    pub(super) fn runs<'a>(&self, text: &'a str) -> Vec<GlyphRun<'a>> {
        split_runs(text, self.faces.len(), |slot, c| {
            self.faces[slot].coverage.contains(c)
        })
    }

    fn run_text(&self, run: &GlyphRun<'_>) -> RunText {
        match &self.faces[run.slot].coverage {
            // A character no font covers is in the text font's run and
            // draws as its missing glyph, 0.
            Coverage::Cmap(cmap) => RunText::Glyphs(
                run.text
                    .chars()
                    .map(|c| cmap.glyph(c).unwrap_or(0))
                    .collect(),
            ),
            Coverage::Chars(_) => RunText::Utf16(run.text.encode_utf16().collect()),
        }
    }

    /// The width of `text` drawn on `hdc`. The font selected in `hdc` is
    /// restored.
    pub(super) unsafe fn measure(&self, hdc: HDC, text: &str) -> i32 {
        self.runs(text)
            .iter()
            .map(|run| {
                let previous = SelectObject(hdc, self.faces[run.slot].font.0);
                let width = run_width(hdc, &self.run_text(run));
                SelectObject(hdc, previous);
                width
            })
            .sum()
    }

    /// Draws `text` from `x` on the baseline `baseline_y`, clipped to
    /// `clip`, in the text colour and background mode of `hdc`. The font and
    /// text alignment of `hdc` are restored.
    pub(super) unsafe fn draw(&self, hdc: HDC, x: i32, baseline_y: i32, clip: &RECT, text: &str) {
        let previous_align = SetTextAlign(hdc, TA_BASELINE | TA_LEFT);
        let mut x = x;
        for run in self.runs(text) {
            let previous = SelectObject(hdc, self.faces[run.slot].font.0);
            let run_text = self.run_text(&run);
            let (options, units) = match &run_text {
                RunText::Glyphs(glyphs) => (ETO_CLIPPED | ETO_GLYPH_INDEX, glyphs),
                RunText::Utf16(units) => (ETO_CLIPPED, units),
            };
            let drawn = ExtTextOutW(
                hdc,
                x,
                baseline_y,
                options,
                clip,
                units.as_ptr(),
                units.len() as u32,
                null(),
            );
            assert!(drawn != 0, "ExtTextOutW must succeed");
            x += run_width(hdc, &run_text);
            SelectObject(hdc, previous);
        }
        SetTextAlign(hdc, previous_align);
    }
}

unsafe fn run_width(hdc: HDC, run_text: &RunText) -> i32 {
    let mut size = SIZE::default();
    let measured = match run_text {
        RunText::Glyphs(glyphs) => {
            GetTextExtentPointI(hdc, glyphs.as_ptr(), glyphs.len() as i32, &mut size)
        }
        RunText::Utf16(units) => {
            GetTextExtentPoint32W(hdc, units.as_ptr(), units.len() as i32, &mut size)
        }
    };
    assert!(measured != 0, "measuring a text run must succeed");
    size.cx
}

fn fallback_logfont(face: &str, cell_height: i32, bold: bool) -> LOGFONTW {
    let mut logfont = LOGFONTW {
        // Positive: the cell height, the text font's line height.
        lfHeight: cell_height,
        lfWeight: if bold { FW_BOLD } else { FW_NORMAL } as i32,
        lfCharSet: DEFAULT_CHARSET,
        ..LOGFONTW::default()
    };
    let face = face.encode_utf16().collect::<Vec<_>>();
    logfont.lfFaceName[..face.len()].copy_from_slice(&face);
    logfont
}

unsafe fn text_metrics(hdc: HDC) -> TEXTMETRICW {
    let mut metrics = TEXTMETRICW::default();
    assert!(
        GetTextMetricsW(hdc, &mut metrics) != 0,
        "GetTextMetricsW must succeed"
    );
    metrics
}

/// The face name of the font selected in `hdc`.
unsafe fn text_face(hdc: HDC) -> String {
    let mut face = [0u16; 32];
    let length = GetTextFaceW(hdc, face.len() as i32, face.as_mut_ptr());
    assert!(length > 0, "GetTextFaceW must succeed");
    String::from_utf16_lossy(&face[..length as usize - 1])
}

/// The parsed `cmap` of the TrueType font selected in `hdc`.
unsafe fn font_cmap(hdc: HDC) -> Result<Cmap, String> {
    let tag = u32::from_le_bytes(*b"cmap");
    let size = GetFontData(hdc, tag, 0, null_mut(), 0);
    if size == GDI_ERROR as u32 {
        return Err("has no cmap table".to_owned());
    }
    let mut table = vec![0u8; size as usize];
    let read = GetFontData(hdc, tag, 0, table.as_mut_ptr() as *mut c_void, size);
    assert_eq!(read, size, "GetFontData must read the cmap table it sized");
    cmap::parse(&table)
}

/// The BMP characters the font selected in `hdc` has a glyph of its own
/// for: those GDI does not map to the default character's glyph.
unsafe fn gdi_covered_chars(hdc: HDC, metrics: &TEXTMETRICW) -> Vec<u16> {
    let units = (0..=u16::MAX)
        .filter(|unit| !(0xD800..=0xDFFF).contains(unit))
        .collect::<Vec<_>>();
    let mut glyphs = vec![0u16; units.len()];
    let mapped = GetGlyphIndicesW(
        hdc,
        units.as_ptr(),
        units.len() as i32,
        glyphs.as_mut_ptr(),
        0,
    );
    assert_eq!(
        mapped as usize,
        units.len(),
        "GetGlyphIndicesW must map every character"
    );
    let mut default_glyph = 0u16;
    let mapped = GetGlyphIndicesW(hdc, &metrics.tmDefaultChar, 1, &mut default_glyph, 0);
    assert_eq!(mapped, 1, "GetGlyphIndicesW must map the default character");
    units
        .into_iter()
        .zip(glyphs)
        .filter(|&(_, glyph)| glyph != default_glyph)
        .map(|(unit, _)| unit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::TextFont;
    use super::super::settings::FontSetting;
    use super::*;

    fn text_font(face: &str, points: u8) -> TextFont {
        unsafe { TextFont::create(&FontSetting::new(face, points, false).unwrap()).unwrap() }
    }

    fn faces(glyphs: &GlyphFonts) -> Vec<&str> {
        glyphs.faces().collect()
    }

    /// `text`'s runs as `(text, face)`.
    fn runs<'a>(glyphs: &'a GlyphFonts, text: &str) -> Vec<(String, &'a str)> {
        glyphs
            .runs(text)
            .into_iter()
            .map(|run| (run.text.into_owned(), glyphs.faces[run.slot].face.as_str()))
            .collect()
    }

    /// Runs `measure` on a screen DC with `font` selected, checking the
    /// selection is restored.
    fn with_screen_dc<T>(font: HFONT, measure: impl FnOnce(HDC) -> T) -> T {
        unsafe {
            let screen = GetDC(null_mut());
            let previous = SelectObject(screen, font);
            let result = measure(screen);
            assert_eq!(SelectObject(screen, previous), font);
            ReleaseDC(null_mut(), screen);
            result
        }
    }

    #[test]
    fn default_glyph_fonts_resolve_tahoma_then_noto_emoji_under_the_harness() {
        // The harness prefix has Wine's Tahoma and the installed Noto Emoji;
        // Wine substitutes another face (DejaVu Sans) for the other
        // candidates, which the face check leaves out.
        let font = text_font("Fixedsys", 9);
        assert_eq!(faces(&font.glyphs), ["Fixedsys", "Tahoma", "Noto Emoji"]);
    }

    #[test]
    fn fixedsys_covers_its_code_page_only() {
        let font = text_font("Fixedsys", 9);
        let fixedsys = &font.glyphs.faces[0].coverage;
        assert!(matches!(fixedsys, Coverage::Chars(_)));
        // GDI maps `—` and `’` into cp1252, so Fixedsys draws them; Wine's
        // vgafix.fon has blocks in those slots (0x97, 0x92), real Windows
        // dashes and quotes.
        for c in ['A', 'é', '½', '—', '’'] {
            assert!(fixedsys.contains(c), "{c}");
        }
        // The euro sign's slot is Fixedsys's default glyph.
        for c in ['🐶', 'Ж', 'Ω', '€'] {
            assert!(!fixedsys.contains(c), "{c}");
        }
    }

    #[test]
    fn runs_send_cyrillic_to_tahoma_and_emoji_to_noto_emoji() {
        let font = text_font("Fixedsys", 9);
        assert_eq!(
            runs(&font.glyphs, "it’s — ok 👍\u{FE0F} 🐶"),
            [
                ("it’s — ok ".to_owned(), "Fixedsys"),
                ("👍".to_owned(), "Noto Emoji"),
                (" ".to_owned(), "Fixedsys"),
                ("🐶".to_owned(), "Noto Emoji"),
            ]
        );
        // Wine's Tahoma has Cyrillic but no Greek, and no harness font has
        // CJK: the text font draws its default glyph for them.
        assert_eq!(
            runs(&font.glyphs, "Привет, ελληνικά"),
            [
                ("Привет".to_owned(), "Tahoma"),
                (", ελληνικά".to_owned(), "Fixedsys"),
            ]
        );
        assert_eq!(
            runs(&font.glyphs, "日本"),
            [("日本".to_owned(), "Fixedsys")]
        );
    }

    #[test]
    fn the_chain_follows_the_text_font() {
        let font = text_font("Courier", 12);
        assert_eq!(faces(&font.glyphs), ["Courier", "Tahoma", "Noto Emoji"]);
        assert_eq!(font.glyphs.metrics().tmHeight, 13);
        assert_eq!(runs(&font.glyphs, "Ж"), [("Ж".to_owned(), "Tahoma")]);
    }

    #[test]
    fn measuring_text_font_text_matches_get_text_extent() {
        let font = text_font("Fixedsys", 9);
        with_screen_dc(font.hfont(), |hdc| unsafe {
            let text = "hello, naïve world";
            let units = text.encode_utf16().collect::<Vec<_>>();
            let mut size = SIZE::default();
            GetTextExtentPoint32W(hdc, units.as_ptr(), units.len() as i32, &mut size);
            assert_eq!(font.glyphs.measure(hdc, text), size.cx);
            assert_eq!(font.glyphs.measure(hdc, ""), 0);
        });
    }

    #[test]
    fn measuring_adds_up_fallback_runs() {
        let font = text_font("Fixedsys", 9);
        with_screen_dc(font.hfont(), |hdc| unsafe {
            let measure = |text| font.glyphs.measure(hdc, text);
            assert!(measure("Ж") > 0);
            assert!(measure("🐶") > 0);
            assert_eq!(
                measure("aЖb🐶"),
                measure("a") + measure("Ж") + measure("b") + measure("🐶")
            );
            assert_eq!(measure("👍\u{FE0F}"), measure("👍"));
        });
    }

    #[test]
    fn fallback_fonts_share_the_text_font_baseline_cell() {
        let font = text_font("Fixedsys", 9);
        for face in &font.glyphs.faces[1..] {
            let metrics = with_screen_dc(face.font.0, |hdc| unsafe { text_metrics(hdc) });
            assert!(
                (metrics.tmHeight - font.glyphs.metrics().tmHeight).abs() <= 1,
                "{} is {} px tall",
                face.face,
                metrics.tmHeight
            );
        }
    }

    #[test]
    fn cmap_parser_matches_gdi_for_tahoma() {
        let font = text_font("Fixedsys", 9);
        let tahoma = &font.glyphs.faces[1];
        assert_eq!(tahoma.face, "Tahoma");
        let Coverage::Cmap(cmap) = &tahoma.coverage else {
            panic!("Tahoma is TrueType");
        };
        with_screen_dc(tahoma.font.0, |hdc| unsafe {
            for c in ['A', 'Ж', '—', '’', 'ε', '\u{1}', '日'] {
                let unit = u16::try_from(u32::from(c)).unwrap();
                let mut glyph = 0u16;
                GetGlyphIndicesW(hdc, &unit, 1, &mut glyph, GGI_MARK_NONEXISTING_GLYPHS);
                assert_eq!(cmap.glyph(c), (glyph != 0xFFFF).then_some(glyph), "{c}");
            }
        });
    }
}
