//! The automation harness's surface in a build without the `automation`
//! feature (the release a user gets): `harness.rs`'s methods, which keep
//! nothing, and its switches, all off. No launch asks for a harness there,
//! so a render-state run or a synthetic load never happens, no command or
//! secret-free record is kept, and the frame has no transport.

use mirx_core::Command;
use windows_sys::Win32::Foundation::HWND;

use super::commands::RoomKeyTransfer;
use super::{AppCommand, ViewId};

pub(super) struct Harness;

impl Harness {
    pub(super) fn for_launch(_command: &AppCommand) -> Result<Self, String> {
        Ok(Self)
    }

    pub(super) fn render_state(&self) -> bool {
        false
    }

    pub(super) fn synthetic_load(&self) -> bool {
        false
    }

    pub(super) fn record_command(&mut self, _command: Command) {}

    pub(super) fn record_opened_link(&mut self, _url: &str) {}

    pub(super) fn record_setup_e2ee_recovery(
        &mut self,
        _passphrase_redacted: bool,
        _wait_for_backups_to_upload: bool,
    ) {
    }

    pub(super) fn record_identity_reset(&mut self, _password_redacted: bool) {}

    pub(super) fn record_identity_reset_cancel(&mut self) {}

    pub(super) fn record_room_key_transfer(&mut self, _transfer: RoomKeyTransfer, _path: &str) {}

    pub(super) fn record_device_sign_out(&mut self, _device_id: &str, _password_redacted: bool) {}

    pub(super) fn record_device_rename(&mut self, _device_id: &str, _display_name: &str) {}

    pub(super) fn record_shown_tip(&mut self, _view: &ViewId, _title: &str, _text: &str) {}

    pub(super) fn count_frame_flash(&mut self) {}

    pub(super) fn count_row_build(&mut self, _view_id: &ViewId) {}

    pub(super) fn count_rows_wrapped(&mut self, _rows: usize) {}

    pub(super) fn count_treebar_rebuild(&mut self) {}

    pub(super) fn count_switchbar_rebuild(&mut self) {}

    pub(super) fn count_mdi_activation(&mut self) {}

    pub(super) fn options_dialog_open(&mut self, _dialog: Option<HWND>) {}

    pub(super) fn colors_dialog_open(&mut self, _dialog: Option<HWND>) {}

    pub(super) fn address_book_dialog_open(&mut self, _dialog: Option<HWND>) {}
}

pub(super) unsafe fn attach(_hwnd: HWND, _command: &AppCommand) -> Result<(), String> {
    Ok(())
}

pub(super) unsafe fn load(_hwnd: HWND) -> Result<(), String> {
    Ok(())
}
