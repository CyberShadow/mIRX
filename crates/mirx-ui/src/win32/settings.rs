//! The user's presentation settings and `mirx.ini`, the file they persist
//! in, like mIRC's `mirc.ini`.
//!
//! mirx owns the file, so the parser rejects anything it does not write
//! rather than silently ignoring a typo. No Win32 calls.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::timestamp::{TimestampFormat, TimestampSetting};

pub(super) const SETTINGS_FILE_NAME: &str = "mirx.ini";

/// The longest face name: a `LOGFONTW` holds 31 UTF-16 units and a NUL.
const MAX_FACE_UTF16_LEN: usize = 31;

/// The font every window's text area, nick list and editbox use; mIRC's
/// `/font`. mIRC's default is Fixedsys 9.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FontSetting {
    face: String,
    points: u8,
    bold: bool,
}

impl FontSetting {
    /// `face` is a font name without commas (`mirx.ini` separates fields
    /// with them), short enough for `LOGFONTW`; `points` is 1-72.
    pub(super) fn new(face: &str, points: u8, bold: bool) -> Result<Self, String> {
        if !(1..=72).contains(&points) {
            return Err("size must be 1-72".to_owned());
        }
        if face.is_empty() {
            return Err("font name cannot be empty".to_owned());
        }
        if face.contains(',') {
            return Err("font name cannot contain a comma".to_owned());
        }
        if face.encode_utf16().count() > MAX_FACE_UTF16_LEN {
            return Err(format!(
                "font name cannot be longer than {MAX_FACE_UTF16_LEN} characters"
            ));
        }
        Ok(Self {
            face: face.to_owned(),
            points,
            bold,
        })
    }

    pub(super) fn face(&self) -> &str {
        &self.face
    }

    pub(super) fn points(&self) -> u8 {
        self.points
    }

    pub(super) fn bold(&self) -> bool {
        self.bold
    }

    /// `Fixedsys 9`, `Courier 12 bold`.
    pub(super) fn label(&self) -> String {
        let bold = if self.bold { " bold" } else { "" };
        format!("{} {}{bold}", self.face, self.points)
    }
}

impl Default for FontSetting {
    fn default() -> Self {
        Self::new("Fixedsys", 9, false).expect("the default font must be valid")
    }
}

/// Parses a font size: `size must be 1-72` for anything else.
pub(super) fn parse_points(text: &str) -> Result<u8, String> {
    text.parse::<u8>()
        .ok()
        .filter(|points| (1..=72).contains(points))
        .ok_or_else(|| "size must be 1-72".to_owned())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Settings {
    pub(super) text_font: FontSetting,
    pub(super) timestamp: TimestampSetting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Fonts,
    Timestamp,
}

/// Parses `mirx.ini`. Missing keys keep their defaults.
pub(super) fn parse_ini(text: &str) -> Result<Settings, String> {
    let mut settings = Settings::default();
    let mut section = None;
    let mut seen = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let error = |reason: String| format!("line {}: {reason}", index + 1);
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') {
            continue;
        }
        if let Some(name) = trimmed
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            section = Some(match name.to_ascii_lowercase().as_str() {
                "fonts" => Section::Fonts,
                "timestamp" => Section::Timestamp,
                _ => return Err(error(format!("unknown section [{name}]"))),
            });
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(error("expected key=value".to_owned()));
        };
        let key = key.trim().to_ascii_lowercase();
        // Trailing whitespace is part of a timestamp format.
        let value = value.trim_start();
        let Some(section) = section else {
            return Err(error(format!("key {key} is outside a section")));
        };
        if seen.contains(&(section, key.clone())) {
            return Err(error(format!("duplicate key {key}")));
        }
        match (section, key.as_str()) {
            (Section::Fonts, "text") => {
                settings.text_font = parse_font_value(value.trim_end()).map_err(error)?;
            }
            (Section::Timestamp, "show") => {
                settings.timestamp.show = match value.trim_end().to_ascii_lowercase().as_str() {
                    "on" => true,
                    "off" => false,
                    _ => return Err(error("show must be on or off".to_owned())),
                };
            }
            (Section::Timestamp, "format") => {
                settings.timestamp.format = TimestampFormat::parse(value).map_err(error)?;
            }
            _ => return Err(error(format!("unknown key {key}"))),
        }
        seen.push((section, key));
    }
    Ok(settings)
}

/// `<face>,<points>[,bold]`.
fn parse_font_value(value: &str) -> Result<FontSetting, String> {
    let fields = value.split(',').map(str::trim).collect::<Vec<_>>();
    let (face, points, bold) = match fields.as_slice() {
        [face, points] => (face, points, false),
        [face, points, bold] if bold.eq_ignore_ascii_case("bold") => (face, points, true),
        _ => return Err("text must be <font name>,<size>[,bold]".to_owned()),
    };
    FontSetting::new(face, parse_points(points)?, bold)
}

/// The whole file: every key, in a fixed order.
pub(super) fn to_ini(settings: &Settings) -> String {
    let font = &settings.text_font;
    let bold = if font.bold { ",bold" } else { "" };
    let show = if settings.timestamp.show { "on" } else { "off" };
    format!(
        "[fonts]\r\ntext={},{}{bold}\r\n[timestamp]\r\nshow={show}\r\nformat={}\r\n",
        font.face, font.points, settings.timestamp.format
    )
}

/// The settings and the file they are saved in. Every change is saved at
/// once, so the file always holds what the user sees.
pub(super) struct SettingsStore {
    /// `None` keeps the settings in memory only: render-state snapshots
    /// must never touch a profile.
    path: Option<PathBuf>,
    settings: Settings,
}

impl SettingsStore {
    /// Reads `path`; a missing file means the defaults, and is not created
    /// until something changes.
    pub(super) fn load(path: PathBuf) -> Result<Self, String> {
        let settings = match fs::read_to_string(&path) {
            Ok(text) => parse_ini(&text).map_err(|err| format!("{} {err}", path.display()))?,
            Err(err) if err.kind() == io::ErrorKind::NotFound => Settings::default(),
            Err(err) => return Err(format!("failed to read {}: {err}", path.display())),
        };
        Ok(Self {
            path: Some(path),
            settings,
        })
    }

    pub(super) fn in_memory() -> Self {
        Self {
            path: None,
            settings: Settings::default(),
        }
    }

    pub(super) fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Changes the settings and saves them; a failed save changes nothing.
    pub(super) fn update(&mut self, change: impl FnOnce(&mut Settings)) -> Result<(), String> {
        let mut settings = self.settings.clone();
        change(&mut settings);
        if let Some(path) = &self.path {
            save(path, &settings)?;
        }
        self.settings = settings;
        Ok(())
    }
}

/// Writes a temporary file and renames it over `path`, which replaces it
/// atomically: a crash leaves the old file or the new one.
fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    let failed = |err: io::Error| format!("failed to save {}: {err}", path.display());
    let parent = path
        .parent()
        .expect("the settings file must be inside the profile directory");
    // Settings may change before anything else created the profile.
    fs::create_dir_all(parent).map_err(failed)?;
    let temporary = path.with_extension("ini.tmp");
    fs::write(&temporary, to_ini(settings)).map_err(failed)?;
    fs::rename(&temporary, path).map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_INI: &str =
        "[fonts]\r\ntext=Fixedsys,9\r\n[timestamp]\r\nshow=on\r\nformat=[HH:nn]\r\n";

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mirx-settings-{name}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn defaults_round_trip_through_the_exact_file_text() {
        assert_eq!(to_ini(&Settings::default()), DEFAULT_INI);
        assert_eq!(parse_ini(DEFAULT_INI), Ok(Settings::default()));
        assert_eq!(parse_ini(""), Ok(Settings::default()));
    }

    #[test]
    fn bold_spaced_faces_and_trailing_format_spaces_round_trip() {
        let settings = Settings {
            text_font: FontSetting::new("Courier New", 12, true).unwrap(),
            timestamp: TimestampSetting {
                show: false,
                format: TimestampFormat::parse("[HH:nn] ").unwrap(),
            },
        };
        let text = to_ini(&settings);
        assert!(text.contains("text=Courier New,12,bold\r\n"));
        assert!(text.contains("format=[HH:nn] \r\n"));
        assert!(text.contains("show=off\r\n"));
        assert_eq!(parse_ini(&text), Ok(settings));
    }

    #[test]
    fn comments_blank_lines_and_name_case_are_ignored() {
        let settings = parse_ini(
            "; mirx settings\n\n[Fonts]\nTEXT = Courier , 10\n  ; indented\n[TIMESTAMP]\nShow=OFF\nformat= <H:nn>\n",
        )
        .unwrap();
        assert_eq!(
            settings.text_font,
            FontSetting::new("Courier", 10, false).unwrap()
        );
        assert!(!settings.timestamp.show);
        assert_eq!(settings.timestamp.format.to_string(), "<H:nn>");
    }

    #[test]
    fn malformed_files_are_rejected_with_their_line() {
        for (text, error) in [
            ("[bogus]", "line 1: unknown section [bogus]"),
            ("[fonts]\nsize=9", "line 2: unknown key size"),
            (
                "[fonts]\ntext=A,9\n\ntext=B,9",
                "line 4: duplicate key text",
            ),
            ("text=A,9", "line 1: key text is outside a section"),
            ("[fonts]\njunk", "line 2: expected key=value"),
            ("[fonts]\ntext=A,0", "line 2: size must be 1-72"),
            ("[fonts]\ntext=A,73", "line 2: size must be 1-72"),
            ("[fonts]\ntext=A,nine", "line 2: size must be 1-72"),
            ("[fonts]\ntext=,9", "line 2: font name cannot be empty"),
            (
                "[fonts]\ntext=A,B,9",
                "line 2: text must be <font name>,<size>[,bold]",
            ),
            (
                "[fonts]\ntext=A,9,italic",
                "line 2: text must be <font name>,<size>[,bold]",
            ),
            ("[timestamp]\nshow=maybe", "line 2: show must be on or off"),
            ("[timestamp]\nformat=", "line 2: format cannot be empty"),
        ] {
            assert_eq!(parse_ini(text), Err(error.to_owned()), "{text:?}");
        }
    }

    #[test]
    fn font_settings_check_their_fields() {
        assert_eq!(
            FontSetting::new("A,B", 9, false),
            Err("font name cannot contain a comma".to_owned())
        );
        assert!(FontSetting::new(&"x".repeat(31), 9, false).is_ok());
        assert!(FontSetting::new(&"x".repeat(32), 9, false).is_err());
        assert_eq!(FontSetting::default().label(), "Fixedsys 9");
        assert_eq!(
            FontSetting::new("Courier", 12, true).unwrap().label(),
            "Courier 12 bold"
        );
    }

    #[test]
    fn a_missing_file_gives_the_defaults_and_is_not_created() {
        let dir = temp_dir("missing");
        let path = dir.join(SETTINGS_FILE_NAME);
        let store = SettingsStore::load(path.clone()).unwrap();
        assert_eq!(store.settings(), &Settings::default());
        assert!(!path.exists());
    }

    #[test]
    fn updates_are_saved_and_load_back() {
        let dir = temp_dir("update");
        let path = dir.join(SETTINGS_FILE_NAME);
        let mut store = SettingsStore::load(path.clone()).unwrap();
        store
            .update(|settings| settings.timestamp.show = false)
            .unwrap();
        assert!(!store.settings().timestamp.show);
        assert!(fs::read_to_string(&path).unwrap().contains("show=off\r\n"));
        assert!(!path.with_extension("ini.tmp").exists());
        assert_eq!(
            SettingsStore::load(path).unwrap().settings(),
            store.settings()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn in_memory_settings_are_never_written() {
        let mut store = SettingsStore::in_memory();
        store
            .update(|settings| settings.timestamp.show = false)
            .unwrap();
        assert!(!store.settings().timestamp.show);
    }

    #[test]
    fn a_malformed_file_fails_to_load_and_is_left_alone() {
        let dir = temp_dir("malformed");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(SETTINGS_FILE_NAME);
        fs::write(&path, "[bogus]\n").unwrap();
        let error = SettingsStore::load(path.clone()).err().unwrap();
        assert_eq!(
            error,
            format!("{} line 1: unknown section [bogus]", path.display())
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "[bogus]\n");
        fs::remove_dir_all(dir).unwrap();
    }
}
