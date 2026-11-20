//! Following a hotlink: a web address in the default browser, a Matrix
//! room in its window or by joining it, a Matrix user in a query.

use mirx_core::MatrixLink;
use windows_sys::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::hotlinks::LinkKind;
use super::*;

/// Follows the link `text` the user double-clicked, as `kind` says.
pub(super) unsafe fn open_link(frame: HWND, text: &str, kind: LinkKind) -> Result<(), String> {
    match kind {
        LinkKind::Web(url) => open_web_link(frame, url),
        LinkKind::Matrix(
            MatrixLink::Room {
                room_id_or_alias,
                via,
            }
            | MatrixLink::Event {
                room_id_or_alias,
                via,
                ..
            },
        ) => match open_room_link(frame, room_id_or_alias, via) {
            Ok(status) => set_local_command_status(frame, status),
            Err(error) => report_error_line(frame, format!("* Unable to open {text}: {error}")),
        },
        // As `/query @user` does, errors included.
        LinkKind::Matrix(MatrixLink::User { user_id }) => run_local_command(
            frame,
            SlashCommand::Query {
                target: user_id,
                message: None,
            },
        ),
    }
}

/// Opens the web address `url` in the default browser: a hotlink, or the
/// homeserver's sign-in page. A failure prints where the user looks.
pub(super) unsafe fn open_web_link(frame: HWND, url: String) -> Result<(), String> {
    state_mut(frame).opened_links.push(url.clone());
    match open_in_browser(frame, &url) {
        Ok(()) => Ok(()),
        Err(error) => report_error_line(frame, format!("* Unable to open {url}: {error}")),
    }
}

/// Opens `url` in the default browser. The shell reports a failure here
/// rather than in a box of its own, and returns once it has started the
/// browser.
unsafe fn open_in_browser(owner: HWND, url: &str) -> Result<(), String> {
    let verb = wide_null("open");
    let file = wide_null(url);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
        hwnd: owner,
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..SHELLEXECUTEINFOW::default()
    };
    if ShellExecuteExW(&mut info) == 0 {
        return Err(last_os_error("ShellExecuteExW"));
    }
    Ok(())
}
