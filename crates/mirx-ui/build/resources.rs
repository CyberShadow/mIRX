//! The `.rc` script. Every value comes from Cargo's environment, so the
//! version shown in Explorer is the crate's.

use std::env;
use std::path::{Path, PathBuf};

/// The icon's resource id. `win32/app_icon.rs` has the same constant
/// (`APP_ICON_ID`); the bin's resource tests load group icon 1 from the exe.
const APP_ICON_ID: u16 = 1;

pub struct Version {
    major: u16,
    minor: u16,
    patch: u16,
    text: String,
}

impl Version {
    pub fn from_cargo_env() -> Self {
        let part = |name: &str| -> u16 {
            env::var(name)
                .unwrap_or_else(|_| panic!("cargo sets {name}"))
                .parse()
                .unwrap_or_else(|err| panic!("{name} must fit a VERSIONINFO word: {err}"))
        };
        Self {
            major: part("CARGO_PKG_VERSION_MAJOR"),
            minor: part("CARGO_PKG_VERSION_MINOR"),
            patch: part("CARGO_PKG_VERSION_PATCH"),
            text: env::var("CARGO_PKG_VERSION").expect("cargo sets CARGO_PKG_VERSION"),
        }
    }
}

pub struct RcInputs<'a> {
    pub icon_path: &'a Path,
    /// The toolbar's icon groups, by resource id.
    pub toolbar_icons: &'a [(u16, PathBuf)],
    pub manifest_path: &'a Path,
    pub version: Version,
}

pub fn rc_source(inputs: &RcInputs) -> String {
    let Version {
        major,
        minor,
        patch,
        text,
    } = &inputs.version;
    let icon = rc_path(inputs.icon_path);
    let manifest = rc_path(inputs.manifest_path);
    let toolbar_icons = inputs
        .toolbar_icons
        .iter()
        .map(|(id, path)| format!("{id} ICON \"{}\"\n", rc_path(path)))
        .collect::<String>();
    format!(
        r#"#pragma code_page(65001)
{APP_ICON_ID} ICON "{icon}"
{toolbar_icons}1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION     {major},{minor},{patch},0
PRODUCTVERSION  {major},{minor},{patch},0
FILEFLAGSMASK   0x3f
FILEFLAGS       0
FILEOS          0x40004
FILETYPE        1
FILESUBTYPE     0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "mirx"
      VALUE "FileVersion", "{text}"
      VALUE "InternalName", "mirx-ui"
      VALUE "OriginalFilename", "mirx-ui.exe"
      VALUE "ProductName", "mirx"
      VALUE "ProductVersion", "{text}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    )
}

/// A quoted path for windres's preprocessor: forward slashes, and nothing
/// that would need escaping.
fn rc_path(path: &Path) -> String {
    let text = path
        .to_str()
        .expect("resource paths are UTF-8")
        .replace('\\', "/");
    assert!(
        !text.contains('"'),
        "a resource path cannot contain a quote: {text}"
    );
    text
}
