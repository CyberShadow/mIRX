//! Line timestamps in the local time zone, formatted like mIRC's
//! `/timestamp -f` with a subset of the `$asctime` tokens.
//!
//! Pure code: the conversion from a Unix time to the local time is passed in,
//! so the Win32 conversion is used by the app and plain UTC arithmetic by the
//! tests.

use std::fmt;

/// A wall-clock time in the user's time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LocalTime {
    pub(super) year: u16,
    pub(super) month: u8,
    pub(super) day: u8,
    pub(super) hour: u8,
    pub(super) minute: u8,
    pub(super) second: u8,
}

impl LocalTime {
    /// The day of the week, 0 for Sunday (Sakamoto's method).
    pub(super) fn weekday(self) -> u8 {
        const MONTH_OFFSETS: [u16; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let year = self.year - u16::from(self.month < 3);
        let day = year + year / 4 - year / 100
            + year / 400
            + MONTH_OFFSETS[usize::from(self.month - 1)]
            + u16::from(self.day);
        (day % 7) as u8
    }

    /// `Fri Nov 20 14:03:12 2026`: mIRC's `$asctime` default
    /// (`ddd mmm dd HH:nn:ss yyyy`), as its log files' session lines use.
    pub(super) fn asctime(self) -> String {
        const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        format!(
            "{} {} {:02} {:02}:{:02}:{:02} {:04}",
            WEEKDAYS[usize::from(self.weekday())],
            MONTHS[usize::from(self.month - 1)],
            self.day,
            self.hour,
            self.minute,
            self.second,
            self.year
        )
    }
}

/// Converts Unix milliseconds to the local time.
pub(super) type LocalTimeFn = fn(u64) -> LocalTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    /// `HH` / `H`.
    Hour24,
    /// `hh` / `h`.
    Hour12,
    /// `nn` / `n`.
    Minute,
    /// `ss` / `s`.
    Second,
    /// `dd` / `d`.
    Day,
    /// `mm` / `m`.
    Month,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Literal(String),
    /// A number, zero-padded to two digits when `padded`.
    Number {
        field: Field,
        padded: bool,
    },
    /// `tt` (`am`/`pm`) and `TT` (`AM`/`PM`).
    AmPm {
        upper: bool,
    },
    /// `yy`.
    Year2,
    /// `yyyy`.
    Year4,
}

/// The tokens, longest first within each letter so that `HH` wins over `H`.
const TOKENS: &[(&str, Token)] = &[
    ("yyyy", Token::Year4),
    ("yy", Token::Year2),
    (
        "HH",
        Token::Number {
            field: Field::Hour24,
            padded: true,
        },
    ),
    (
        "H",
        Token::Number {
            field: Field::Hour24,
            padded: false,
        },
    ),
    (
        "hh",
        Token::Number {
            field: Field::Hour12,
            padded: true,
        },
    ),
    (
        "h",
        Token::Number {
            field: Field::Hour12,
            padded: false,
        },
    ),
    (
        "nn",
        Token::Number {
            field: Field::Minute,
            padded: true,
        },
    ),
    (
        "n",
        Token::Number {
            field: Field::Minute,
            padded: false,
        },
    ),
    (
        "ss",
        Token::Number {
            field: Field::Second,
            padded: true,
        },
    ),
    (
        "s",
        Token::Number {
            field: Field::Second,
            padded: false,
        },
    ),
    ("tt", Token::AmPm { upper: false }),
    ("TT", Token::AmPm { upper: true }),
    (
        "dd",
        Token::Number {
            field: Field::Day,
            padded: true,
        },
    ),
    (
        "d",
        Token::Number {
            field: Field::Day,
            padded: false,
        },
    ),
    (
        "mm",
        Token::Number {
            field: Field::Month,
            padded: true,
        },
    ),
    (
        "m",
        Token::Number {
            field: Field::Month,
            padded: false,
        },
    ),
];

/// The format's tokens, space-separated: what the Options dialog lists.
pub(super) fn format_tokens() -> String {
    TOKENS
        .iter()
        .map(|(text, _)| *text)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A parsed timestamp format such as `[HH:nn]`. Every character that does
/// not start a token is literal, as in mIRC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TimestampFormat {
    source: String,
    tokens: Vec<Token>,
}

impl TimestampFormat {
    pub(super) fn parse(source: &str) -> Result<Self, String> {
        if source.is_empty() {
            return Err("format cannot be empty".to_owned());
        }
        let mut tokens = Vec::new();
        let mut rest = source;
        while let Some(ch) = rest.chars().next() {
            match TOKENS.iter().find(|(text, _)| rest.starts_with(text)) {
                Some((text, token)) => {
                    tokens.push(token.clone());
                    rest = &rest[text.len()..];
                }
                None => {
                    match tokens.last_mut() {
                        Some(Token::Literal(literal)) => literal.push(ch),
                        _ => tokens.push(Token::Literal(ch.to_string())),
                    }
                    rest = &rest[ch.len_utf8()..];
                }
            }
        }
        Ok(Self {
            source: source.to_owned(),
            tokens,
        })
    }

    pub(super) fn format(&self, time: LocalTime) -> String {
        let mut text = String::new();
        for token in &self.tokens {
            match token {
                Token::Literal(literal) => text.push_str(literal),
                Token::Number { field, padded } => {
                    let value = match field {
                        Field::Hour24 => time.hour,
                        Field::Hour12 => match time.hour % 12 {
                            0 => 12,
                            hour => hour,
                        },
                        Field::Minute => time.minute,
                        Field::Second => time.second,
                        Field::Day => time.day,
                        Field::Month => time.month,
                    };
                    if *padded {
                        text.push_str(&format!("{value:02}"));
                    } else {
                        text.push_str(&value.to_string());
                    }
                }
                Token::AmPm { upper } => text.push_str(match (time.hour < 12, upper) {
                    (true, false) => "am",
                    (false, false) => "pm",
                    (true, true) => "AM",
                    (false, true) => "PM",
                }),
                Token::Year2 => text.push_str(&format!("{:02}", time.year % 100)),
                Token::Year4 => text.push_str(&format!("{:04}", time.year)),
            }
        }
        text
    }
}

impl Default for TimestampFormat {
    fn default() -> Self {
        Self::parse("[HH:nn]").expect("the default timestamp format must parse")
    }
}

/// The format as the user wrote it.
impl fmt::Display for TimestampFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.source)
    }
}

/// Whether lines are timestamped, and how; mIRC's `/timestamp` setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TimestampSetting {
    pub(super) show: bool,
    pub(super) format: TimestampFormat,
}

impl Default for TimestampSetting {
    fn default() -> Self {
        Self {
            show: true,
            format: TimestampFormat::default(),
        }
    }
}

/// The time every hanging indent is measured with: wide digits only.
const INDENT_SAMPLE_TIME: LocalTime = LocalTime {
    year: 2000,
    month: 1,
    day: 1,
    hour: 0,
    minute: 0,
    second: 0,
};

/// Stamps lines with a [`TimestampSetting`] in the local time zone.
#[derive(Clone, Copy)]
pub(super) struct TimestampStyle<'a> {
    setting: &'a TimestampSetting,
    local_time: LocalTimeFn,
}

impl<'a> TimestampStyle<'a> {
    pub(super) fn new(setting: &'a TimestampSetting, local_time: LocalTimeFn) -> Self {
        Self {
            setting,
            local_time,
        }
    }

    /// `"[22:12] "`, the start of a line at `timestamp_ms`, or `""` when
    /// timestamps are off.
    pub(super) fn prefix(self, timestamp_ms: u64) -> String {
        self.prefix_for((self.local_time)(timestamp_ms))
    }

    /// A prefix as wide as a line's: continuation lines are indented by its
    /// width so they align past the timestamp.
    pub(super) fn indent_sample(self) -> String {
        self.prefix_for(INDENT_SAMPLE_TIME)
    }

    fn prefix_for(self, time: LocalTime) -> String {
        if self.setting.show {
            format!("{} ", self.setting.format.format(time))
        } else {
            String::new()
        }
    }
}

/// UTC as the "local" time: deterministic tests.
#[cfg(test)]
pub(super) fn utc_local_time(timestamp_ms: u64) -> LocalTime {
    let seconds = timestamp_ms / 1000;
    let days = (seconds / 86_400) as i64;
    let second_of_day = seconds % 86_400;
    // Howard Hinnant's days_from_civil inverse.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    LocalTime {
        year: year as u16,
        month: month as u8,
        day: day as u8,
        hour: (second_of_day / 3600) as u8,
        minute: (second_of_day / 60 % 60) as u8,
        second: (second_of_day % 60) as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIME: LocalTime = LocalTime {
        year: 2023,
        month: 11,
        day: 4,
        hour: 22,
        minute: 7,
        second: 5,
    };

    fn format(source: &str, time: LocalTime) -> String {
        TimestampFormat::parse(source).unwrap().format(time)
    }

    fn at_hour(hour: u8) -> LocalTime {
        LocalTime { hour, ..TIME }
    }

    #[test]
    fn the_format_tokens_are_listed_longest_first() {
        assert_eq!(
            format_tokens(),
            "yyyy yy HH H hh h nn n ss s tt TT dd d mm m"
        );
    }

    #[test]
    fn every_token_formats_its_field() {
        assert_eq!(format("HH H", TIME), "22 22");
        assert_eq!(format("hh h", TIME), "10 10");
        assert_eq!(format("nn n", TIME), "07 7");
        assert_eq!(format("ss s", TIME), "05 5");
        assert_eq!(format("tt TT", TIME), "pm PM");
        assert_eq!(format("dd d", TIME), "04 4");
        assert_eq!(format("mm m", TIME), "11 11");
        assert_eq!(format("yy yyyy", TIME), "23 2023");
        assert_eq!(format("H", at_hour(7)), "7");
        assert_eq!(format("HH", at_hour(7)), "07");
    }

    #[test]
    fn the_longest_token_wins() {
        assert_eq!(format("HHH", at_hour(7)), "077");
        assert_eq!(format("yyy", TIME), "23y");
        assert_eq!(format("nnnn", TIME), "0707");
    }

    #[test]
    fn other_characters_are_literal() {
        assert_eq!(format("[HH:nn]", TIME), "[22:07]");
        assert_eq!(format("<H.n> at", TIME), "<22.7> at");
        assert_eq!(format("Ω", TIME), "Ω");
    }

    #[test]
    fn twelve_hour_clock_has_no_zero_hour() {
        assert_eq!(format("h tt", at_hour(0)), "12 am");
        assert_eq!(format("h tt", at_hour(11)), "11 am");
        assert_eq!(format("h TT", at_hour(12)), "12 PM");
        assert_eq!(format("hh tt", at_hour(13)), "01 pm");
    }

    #[test]
    fn an_empty_format_is_an_error() {
        assert_eq!(
            TimestampFormat::parse(""),
            Err("format cannot be empty".to_owned())
        );
    }

    #[test]
    fn display_gives_back_the_source() {
        assert_eq!(
            TimestampFormat::parse("[HH:nn:ss] ").unwrap().to_string(),
            "[HH:nn:ss] "
        );
        assert_eq!(TimestampFormat::default().to_string(), "[HH:nn]");
    }

    #[test]
    fn prefix_and_indent_follow_the_setting() {
        let mut setting = TimestampSetting::default();
        let style = TimestampStyle::new(&setting, utc_local_time);
        assert_eq!(style.prefix(1_700_000_000_000), "[22:13] ");
        assert_eq!(style.indent_sample(), "[00:00] ");

        setting.format = TimestampFormat::parse("[H:nn:ss tt]").unwrap();
        let style = TimestampStyle::new(&setting, utc_local_time);
        assert_eq!(style.prefix(1_700_000_000_000), "[22:13:20 pm] ");
        assert_eq!(style.indent_sample(), "[0:00:00 am] ");

        setting.show = false;
        let style = TimestampStyle::new(&setting, utc_local_time);
        assert_eq!(style.prefix(1_700_000_000_000), "");
        assert_eq!(style.indent_sample(), "");
    }

    fn date(year: u16, month: u8, day: u8) -> LocalTime {
        LocalTime {
            year,
            month,
            day,
            ..TIME
        }
    }

    #[test]
    fn weekday_counts_from_sunday() {
        assert_eq!(date(2026, 11, 20).weekday(), 5);
        assert_eq!(date(2024, 2, 29).weekday(), 4);
        assert_eq!(date(2000, 3, 1).weekday(), 3);
        assert_eq!(date(1970, 1, 4).weekday(), 0);
        assert_eq!(date(1970, 1, 1).weekday(), 4);
        assert_eq!(date(2023, 12, 31).weekday(), 0);
    }

    #[test]
    fn asctime_is_mircs_default_format() {
        let time = LocalTime {
            year: 2026,
            month: 11,
            day: 20,
            hour: 14,
            minute: 3,
            second: 12,
        };
        assert_eq!(time.asctime(), "Fri Nov 20 14:03:12 2026");
        let time = LocalTime {
            year: 2024,
            month: 2,
            day: 2,
            hour: 7,
            minute: 0,
            second: 9,
        };
        assert_eq!(time.asctime(), "Fri Feb 02 07:00:09 2024");
        assert_eq!(utc_local_time(0).asctime(), "Thu Jan 01 00:00:00 1970");
    }

    #[test]
    fn utc_local_time_converts_unix_milliseconds() {
        assert_eq!(
            utc_local_time(1_700_000_000_000),
            LocalTime {
                year: 2023,
                month: 11,
                day: 14,
                hour: 22,
                minute: 13,
                second: 20,
            }
        );
        assert_eq!(
            utc_local_time(0),
            LocalTime {
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
            }
        );
        assert_eq!(
            utc_local_time(951_782_400_000),
            LocalTime {
                year: 2000,
                month: 2,
                day: 29,
                hour: 0,
                minute: 0,
                second: 0,
            }
        );
    }
}
