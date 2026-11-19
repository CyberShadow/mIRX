//! The user's presentation settings.
//!
//! Pure code, no Win32 calls.

/// The font every window's text area, nick list and editbox use; mIRC's
/// `/font`. mIRC's default is Fixedsys 9.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FontSetting {
    /// A face name: no comma, never empty.
    pub(super) face: String,
    /// 1-72.
    pub(super) points: u8,
    pub(super) bold: bool,
}

impl Default for FontSetting {
    fn default() -> Self {
        Self {
            face: "Fixedsys".to_owned(),
            points: 9,
            bold: false,
        }
    }
}
