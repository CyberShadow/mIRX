//! The user's presentation settings and `mirx.ini`, the file they persist
//! in, like mIRC's `mirc.ini`.
//!
//! mirx owns the file, so the parser rejects anything it does not write
//! rather than silently ignoring a typo. No Win32 calls.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::colors::{ColorItem, ColorScheme, PaletteIndex};
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
    pub(super) connect: ConnectSetting,
    pub(super) switchbar: SwitchbarSetting,
    pub(super) treebar: TreebarSetting,
    pub(super) logging: LoggingSetting,
    /// mIRC's Tools > Colors.
    pub(super) colors: ColorScheme,
}

/// mIRC's Options > Connect > Options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ConnectSetting {
    /// Whether a saved session syncs at startup. Off, mirx starts signed in
    /// but disconnected, until File > Connect or `/server`.
    pub(super) on_startup: bool,
}

impl Default for ConnectSetting {
    fn default() -> Self {
        Self { on_startup: true }
    }
}

impl Settings {
    pub(super) fn bar_shown(&self, bar: Bar) -> bool {
        match bar {
            Bar::Switchbar => self.switchbar.show,
            Bar::Treebar => self.treebar.show,
        }
    }

    pub(super) fn set_bar_shown(&mut self, bar: Bar, show: bool) {
        match bar {
            Bar::Switchbar => self.switchbar.show = show,
            Bar::Treebar => self.treebar.show = show,
        }
    }
}

/// The frame's bars the View menu shows and hides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Bar {
    Switchbar,
    Treebar,
}

impl Bar {
    /// Its command's name, without the `/`.
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Switchbar => "switchbar",
            Self::Treebar => "treebar",
        }
    }

    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Switchbar => "Switchbar",
            Self::Treebar => "Treebar",
        }
    }
}

/// The switchbar, mIRC's View > Switchbar and `/switchbar`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SwitchbarSetting {
    pub(super) show: bool,
    pub(super) buttons: ButtonWidth,
    /// mIRC's Options > Display Event, Message and Highlight: the colour
    /// an inactive window's switchbar button and treebar item take for
    /// what happened in it.
    pub(super) activity: ActivityColors,
}

impl Default for SwitchbarSetting {
    fn default() -> Self {
        Self {
            show: true,
            buttons: ButtonWidth::new(120).expect("the default button width must be valid"),
            activity: ActivityColors::default(),
        }
    }
}

/// The switchbar's activity colours, by level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ActivityColors {
    pub(super) event: PaletteIndex,
    pub(super) message: PaletteIndex,
    pub(super) highlight: PaletteIndex,
}

impl Default for ActivityColors {
    /// Maroon, red and blue. Message red is mIRC's well-known default;
    /// event maroon and highlight blue are best recollection of mIRC 7's.
    fn default() -> Self {
        let index = |index| PaletteIndex::new(index).expect("the defaults are palette indices");
        Self {
            event: index(5),
            message: index(4),
            highlight: index(12),
        }
    }
}

/// How wide every switchbar button is, mIRC's Options > Display "Buttons".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ButtonWidth(u16);

impl ButtonWidth {
    const PIXELS: std::ops::RangeInclusive<u16> = 40..=400;

    /// 40-400 pixels.
    pub(super) fn new(pixels: u16) -> Result<Self, String> {
        if Self::PIXELS.contains(&pixels) {
            Ok(Self(pixels))
        } else {
            Err(BUTTON_WIDTH_ERROR.to_owned())
        }
    }

    pub(super) fn pixels(self) -> i32 {
        i32::from(self.0)
    }
}

impl std::fmt::Display for ButtonWidth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

const BUTTON_WIDTH_ERROR: &str = "buttons must be 40-400";

/// Parses a button width: `buttons must be 40-400` for anything else.
pub(super) fn parse_button_width(text: &str) -> Result<ButtonWidth, String> {
    text.parse::<u16>()
        .map_err(|_| BUTTON_WIDTH_ERROR.to_owned())
        .and_then(ButtonWidth::new)
}

/// The treebar, mIRC's View > Treebar and `/treebar`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TreebarSetting {
    pub(super) show: bool,
}

impl Default for TreebarSetting {
    fn default() -> Self {
        Self { show: true }
    }
}

/// mIRC's Options > IRC > Logging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LoggingSetting {
    /// Automatically log channels: every channel window logs from when it
    /// opens. Off by default, as in mIRC.
    pub(super) channels: bool,
    /// ... and queries.
    pub(super) queries: bool,
    /// Logs the plain text, without bold, colour and other control codes.
    pub(super) strip_codes: bool,
    /// Stamps each logged line with the window's timestamp format.
    pub(super) timestamp: bool,
}

impl Default for LoggingSetting {
    fn default() -> Self {
        Self {
            channels: false,
            queries: false,
            strip_codes: false,
            timestamp: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Fonts,
    Timestamp,
    Connect,
    Switchbar,
    Treebar,
    Logging,
    Colors,
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
                "connect" => Section::Connect,
                "switchbar" => Section::Switchbar,
                "treebar" => Section::Treebar,
                "logging" => Section::Logging,
                "colors" => Section::Colors,
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
                settings.timestamp.show = parse_on_off(value, "show").map_err(error)?;
            }
            (Section::Timestamp, "format") => {
                settings.timestamp.format = TimestampFormat::parse(value).map_err(error)?;
            }
            (Section::Connect, "startup") => {
                settings.connect.on_startup = parse_on_off(value, "startup").map_err(error)?;
            }
            (Section::Switchbar, "show") => {
                settings.switchbar.show = parse_on_off(value, "show").map_err(error)?;
            }
            (Section::Switchbar, "buttons") => {
                settings.switchbar.buttons = parse_button_width(value.trim_end()).map_err(error)?;
            }
            (Section::Switchbar, "event") => {
                settings.switchbar.activity.event = parse_palette_index(value).map_err(error)?;
            }
            (Section::Switchbar, "message") => {
                settings.switchbar.activity.message = parse_palette_index(value).map_err(error)?;
            }
            (Section::Switchbar, "highlight") => {
                settings.switchbar.activity.highlight =
                    parse_palette_index(value).map_err(error)?;
            }
            (Section::Treebar, "show") => {
                settings.treebar.show = parse_on_off(value, "show").map_err(error)?;
            }
            (Section::Logging, "channels") => {
                settings.logging.channels = parse_on_off(value, "channels").map_err(error)?;
            }
            (Section::Logging, "queries") => {
                settings.logging.queries = parse_on_off(value, "queries").map_err(error)?;
            }
            (Section::Logging, "strip") => {
                settings.logging.strip_codes = parse_on_off(value, "strip").map_err(error)?;
            }
            (Section::Logging, "timestamp") => {
                settings.logging.timestamp = parse_on_off(value, "timestamp").map_err(error)?;
            }
            (Section::Colors, key) if let Some(item) = ColorItem::from_ini_key(key) => {
                settings
                    .colors
                    .set(item, parse_palette_index(value).map_err(error)?);
            }
            _ => return Err(error(format!("unknown key {key}"))),
        }
        seen.push((section, key));
    }
    Ok(settings)
}

/// `on` or `off`, in any case.
fn parse_on_off(value: &str, key: &str) -> Result<bool, String> {
    match value.trim_end().to_ascii_lowercase().as_str() {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("{key} must be on or off")),
    }
}

/// A palette index, `0`-`15`.
pub(super) fn parse_palette_index(value: &str) -> Result<PaletteIndex, String> {
    value
        .trim_end()
        .parse::<u8>()
        .map_err(|_| "colors must be 0-15")
        .and_then(PaletteIndex::new)
        .map_err(str::to_owned)
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
    let activity = &settings.switchbar.activity;
    let logging = &settings.logging;
    let colors = ColorItem::ALL
        .into_iter()
        .map(|item| format!("{}={}\r\n", item.ini_key(), settings.colors.get(item).get()))
        .collect::<String>();
    format!(
        "[fonts]\r\ntext={},{}{bold}\r\n\
         [timestamp]\r\nshow={}\r\nformat={}\r\n\
         [connect]\r\nstartup={}\r\n\
         [switchbar]\r\nshow={}\r\nbuttons={}\r\n\
         event={}\r\nmessage={}\r\nhighlight={}\r\n\
         [treebar]\r\nshow={}\r\n\
         [logging]\r\nchannels={}\r\nqueries={}\r\nstrip={}\r\ntimestamp={}\r\n\
         [colors]\r\n{colors}",
        font.face,
        font.points,
        on_off(settings.timestamp.show),
        settings.timestamp.format,
        on_off(settings.connect.on_startup),
        on_off(settings.switchbar.show),
        settings.switchbar.buttons,
        activity.event.get(),
        activity.message.get(),
        activity.highlight.get(),
        on_off(settings.treebar.show),
        on_off(logging.channels),
        on_off(logging.queries),
        on_off(logging.strip_codes),
        on_off(logging.timestamp),
    )
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
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

    const DEFAULT_INI: &str = "[fonts]\r\ntext=Fixedsys,9\r\n\
        [timestamp]\r\nshow=on\r\nformat=[HH:nn]\r\n\
        [connect]\r\nstartup=on\r\n\
        [switchbar]\r\nshow=on\r\nbuttons=120\r\nevent=5\r\nmessage=4\r\nhighlight=12\r\n\
        [treebar]\r\nshow=on\r\n\
        [logging]\r\nchannels=off\r\nqueries=off\r\nstrip=off\r\ntimestamp=on\r\n\
        [colors]\r\nbackground=0\r\naction=6\r\nhighlight=5\r\ninfo=2\r\ninvite=3\r\n\
        join=3\r\nkick=3\r\nmode=3\r\nnick=3\r\nnormal=1\r\nnotice=5\r\nother=6\r\nown=1\r\n\
        part=3\r\ntopic=3\r\neditbox=0\r\neditbox_text=1\r\nlistbox=0\r\nlistbox_text=1\r\n\
        gray=14\r\ntreebar=0\r\ntreebar_text=1\r\n";

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
        // A file from before the bars had settings.
        assert_eq!(
            parse_ini("[fonts]\r\ntext=Fixedsys,9\r\n[timestamp]\r\nshow=on\r\nformat=[HH:nn]\r\n"),
            Ok(Settings::default())
        );
    }

    #[test]
    fn each_bar_has_its_own_setting() {
        let mut settings = Settings::default();
        settings.set_bar_shown(Bar::Treebar, false);
        assert!(settings.bar_shown(Bar::Switchbar));
        assert!(!settings.bar_shown(Bar::Treebar));
        assert!(!settings.treebar.show);
        settings.set_bar_shown(Bar::Switchbar, false);
        assert!(!settings.switchbar.show);
    }

    #[test]
    fn connect_and_bar_settings_round_trip() {
        let settings = Settings {
            connect: ConnectSetting { on_startup: false },
            switchbar: SwitchbarSetting {
                show: false,
                buttons: ButtonWidth::new(90).unwrap(),
                activity: ActivityColors {
                    event: PaletteIndex::new(7).unwrap(),
                    message: PaletteIndex::new(9).unwrap(),
                    highlight: PaletteIndex::new(0).unwrap(),
                },
            },
            treebar: TreebarSetting { show: false },
            ..Settings::default()
        };
        let text = to_ini(&settings);
        assert!(text.contains("[connect]\r\nstartup=off\r\n"));
        assert!(text.contains(
            "[switchbar]\r\nshow=off\r\nbuttons=90\r\nevent=7\r\nmessage=9\r\nhighlight=0\r\n"
        ));
        assert!(text.contains("[treebar]\r\nshow=off\r\n"));
        assert_eq!(parse_ini(&text), Ok(settings));
    }

    #[test]
    fn logging_settings_round_trip() {
        let settings = Settings {
            logging: LoggingSetting {
                channels: true,
                queries: false,
                strip_codes: true,
                timestamp: false,
            },
            ..Settings::default()
        };
        let text = to_ini(&settings);
        assert!(text.contains(
            "[treebar]\r\nshow=on\r\n\
             [logging]\r\nchannels=on\r\nqueries=off\r\nstrip=on\r\ntimestamp=off\r\n[colors]"
        ));
        assert_eq!(parse_ini(&text), Ok(settings));
        assert_eq!(
            parse_ini("[logging]\nQueries=ON\nchannels=off").map(|settings| settings.logging),
            Ok(LoggingSetting {
                queries: true,
                ..LoggingSetting::default()
            })
        );
        for (text, error) in [
            (
                "[logging]\nchannels=yes",
                "line 2: channels must be on or off",
            ),
            ("[logging]\nqueries=1", "line 2: queries must be on or off"),
            ("[logging]\nstrip=", "line 2: strip must be on or off"),
            (
                "[logging]\ntimestamp=no",
                "line 2: timestamp must be on or off",
            ),
            (
                "[logging]\nstrip=on\nstrip=off",
                "line 3: duplicate key strip",
            ),
            ("[logging]\nfolder=logs", "line 2: unknown key folder"),
        ] {
            assert_eq!(parse_ini(text), Err(error.to_owned()), "{text}");
        }
    }

    #[test]
    fn colors_round_trip_and_files_without_them_keep_the_defaults() {
        let mut colors = ColorScheme::default();
        colors.set(ColorItem::Background, PaletteIndex::new(1).unwrap());
        colors.set(ColorItem::TreebarText, PaletteIndex::new(15).unwrap());
        let settings = Settings {
            colors,
            ..Settings::default()
        };
        let text = to_ini(&settings);
        assert!(text.contains("[colors]\r\nbackground=1\r\naction=6\r\n"));
        assert!(text.ends_with("treebar=0\r\ntreebar_text=15\r\n"));
        assert_eq!(parse_ini(&text), Ok(settings));
        // Only some items: the rest keep mIRC's.
        let mut join = ColorScheme::default();
        join.set(ColorItem::Join, PaletteIndex::new(9).unwrap());
        assert_eq!(
            parse_ini("[colors]\njoin=9\n").map(|settings| settings.colors),
            Ok(join)
        );
    }

    #[test]
    fn bold_spaced_faces_and_trailing_format_spaces_round_trip() {
        let settings = Settings {
            text_font: FontSetting::new("Courier New", 12, true).unwrap(),
            timestamp: TimestampSetting {
                show: false,
                format: TimestampFormat::parse("[HH:nn] ").unwrap(),
            },
            ..Settings::default()
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
            ("[switchbar]\nshow=x", "line 2: show must be on or off"),
            ("[treebar]\n\nshow=1", "line 3: show must be on or off"),
            ("[treebar]\nshow=on\nshow=off", "line 3: duplicate key show"),
            ("[switchbar]\nwidth=9", "line 2: unknown key width"),
            (
                "[connect]\nstartup=maybe",
                "line 2: startup must be on or off",
            ),
            ("[switchbar]\nbuttons=39", "line 2: buttons must be 40-400"),
            ("[switchbar]\nbuttons=401", "line 2: buttons must be 40-400"),
            (
                "[switchbar]\nbuttons=wide",
                "line 2: buttons must be 40-400",
            ),
            ("[switchbar]\nbuttons=-5", "line 2: buttons must be 40-400"),
            (
                "[switchbar]\nbuttons=90\nbuttons=90",
                "line 3: duplicate key buttons",
            ),
            ("[timestamp]\nformat=", "line 2: format cannot be empty"),
            ("[colors]\njoin=16", "line 2: colors must be 0-15"),
            ("[colors]\njoin=-1", "line 2: colors must be 0-15"),
            ("[colors]\njoin=red", "line 2: colors must be 0-15"),
            ("[colors]\nctcp=4", "line 2: unknown key ctcp"),
            ("[colors]\njoin=3\njoin=4", "line 3: duplicate key join"),
            ("[switchbar]\nmessage=99", "line 2: colors must be 0-15"),
            ("[treebar]\nevent=5", "line 2: unknown key event"),
        ] {
            assert_eq!(parse_ini(text), Err(error.to_owned()), "{text:?}");
        }
    }

    #[test]
    fn button_widths_are_40_to_400_pixels() {
        assert_eq!(ButtonWidth::new(40).map(ButtonWidth::pixels), Ok(40));
        assert_eq!(ButtonWidth::new(400).map(ButtonWidth::pixels), Ok(400));
        assert!(ButtonWidth::new(39).is_err());
        assert!(ButtonWidth::new(401).is_err());
        assert_eq!(parse_button_width("120"), ButtonWidth::new(120));
        assert_eq!(
            parse_button_width("70000"),
            Err("buttons must be 40-400".to_owned())
        );
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
