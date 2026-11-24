//! What the automation harness keeps on the frame: its transport, the
//! switches its launches set, and its record of what the UI did, which the
//! state dump and the unit tests read. Production code tells it what
//! happened and asks it the switches through the methods below, and only
//! through them.

use std::collections::BTreeMap;

use mirx_core::{Command, CommandId};
use tokio::sync::mpsc::UnboundedReceiver;
use windows_sys::Win32::Foundation::HWND;

use super::ViewId;
use super::automation_host::AutomationState;
use super::commands::RoomKeyTransfer;
use super::navigation::view_identity_from_view_id;
use crate::automation::{
    DumpRoomKeyTransferDirection, DumpShownTip, DumpSubmittedIdentityReset,
    DumpSubmittedRoomKeyTransfer, DumpSubmittedSessionCommand, DumpSubmittedSetupRecoveryCommand,
};

#[derive(Default)]
pub(super) struct Harness {
    /// The `WM_COPYDATA` transport's response mapping, while automation
    /// is on.
    pub(super) transport: Option<AutomationState>,
    /// The inert core of a `--render-state` run: commands go here and are
    /// never answered. WAIT_IDLE drops them and counts them handled, as no
    /// core will ack them.
    pub(super) inert_commands: Option<UnboundedReceiver<(CommandId, Command)>>,
    /// A `--render-state` run: no profile, a fixed clock, no caret, no
    /// logs.
    pub(super) render_state: bool,
    /// A synthetic load replays its events: nothing they say goes to the
    /// server, the shell or the saved session.
    pub(super) synthetic_load: bool,
    /// The event generation WAIT_IDLE saw last.
    pub(super) last_idle_probe_generation: Option<u64>,
    /// Every command sent to the core, except those that carry a secret.
    pub(super) commands: Vec<Command>,
    /// The web links handed to the shell, in order.
    pub(super) opened_links: Vec<String>,
    /// What went to the core with a secret, without the secret.
    pub(super) setup_e2ee_recoveries: Vec<DumpSubmittedSetupRecoveryCommand>,
    pub(super) identity_resets: Vec<DumpSubmittedIdentityReset>,
    pub(super) room_key_transfers: Vec<DumpSubmittedRoomKeyTransfer>,
    pub(super) session_commands: Vec<DumpSubmittedSessionCommand>,
    /// Every tip the shell took, in order.
    pub(super) shown_tips: Vec<DumpShownTip>,
    pub(super) refresh_counters: RefreshCounters,
    /// The Options dialog while it is open.
    pub(super) options_dialog: Option<HWND>,
    /// The Colors dialog while it is open.
    pub(super) colors_dialog: Option<HWND>,
    /// The Address Book while it is open.
    pub(super) address_book_dialog: Option<HWND>,
}

impl Harness {
    pub(super) fn render_state(&self) -> bool {
        self.render_state
    }

    pub(super) fn synthetic_load(&self) -> bool {
        self.synthetic_load
    }

    pub(super) fn record_command(&mut self, command: Command) {
        self.commands.push(command);
    }

    pub(super) fn record_opened_link(&mut self, url: &str) {
        self.opened_links.push(url.to_owned());
    }

    pub(super) fn record_setup_e2ee_recovery(
        &mut self,
        passphrase_redacted: bool,
        wait_for_backups_to_upload: bool,
    ) {
        self.setup_e2ee_recoveries
            .push(DumpSubmittedSetupRecoveryCommand {
                passphrase_redacted,
                wait_for_backups_to_upload,
            });
    }

    pub(super) fn record_identity_reset(&mut self, password_redacted: bool) {
        self.identity_resets
            .push(DumpSubmittedIdentityReset::Reset { password_redacted });
    }

    pub(super) fn record_identity_reset_cancel(&mut self) {
        self.identity_resets
            .push(DumpSubmittedIdentityReset::Cancel);
    }

    pub(super) fn record_room_key_transfer(&mut self, transfer: RoomKeyTransfer, path: &str) {
        self.room_key_transfers.push(DumpSubmittedRoomKeyTransfer {
            direction: match transfer {
                RoomKeyTransfer::Export => DumpRoomKeyTransferDirection::Export,
                RoomKeyTransfer::Import => DumpRoomKeyTransferDirection::Import,
            },
            path: path.to_owned(),
        });
    }

    pub(super) fn record_device_sign_out(&mut self, device_id: &str, password_redacted: bool) {
        self.session_commands
            .push(DumpSubmittedSessionCommand::SignOut {
                device_id: device_id.to_owned(),
                password_redacted,
            });
    }

    pub(super) fn record_device_rename(&mut self, device_id: &str, display_name: &str) {
        self.session_commands
            .push(DumpSubmittedSessionCommand::Rename {
                device_id: device_id.to_owned(),
                display_name: display_name.to_owned(),
            });
    }

    pub(super) fn record_shown_tip(&mut self, view: &ViewId, title: &str, text: &str) {
        self.shown_tips.push(DumpShownTip {
            view: view_identity_from_view_id(view),
            title: title.to_owned(),
            text: text.to_owned(),
        });
    }

    pub(super) fn count_row_build(&mut self, view_id: &ViewId) {
        let counters = &mut self.refresh_counters;
        counters.timeline_row_builds += 1;
        if let ViewId::Room(room_id) = view_id {
            *counters
                .row_builds_by_room
                .entry(room_id.clone())
                .or_default() += 1;
        }
    }

    pub(super) fn count_rows_wrapped(&mut self, rows: usize) {
        self.refresh_counters.rows_wrapped += rows as u64;
    }

    pub(super) fn count_treebar_rebuild(&mut self) {
        self.refresh_counters.treebar_rebuilds += 1;
    }

    pub(super) fn count_switchbar_rebuild(&mut self) {
        self.refresh_counters.switchbar_rebuilds += 1;
    }

    pub(super) fn options_dialog_open(&mut self, dialog: Option<HWND>) {
        self.options_dialog = dialog;
    }

    pub(super) fn colors_dialog_open(&mut self, dialog: Option<HWND>) {
        self.colors_dialog = dialog;
    }

    pub(super) fn address_book_dialog_open(&mut self, dialog: Option<HWND>) {
        self.address_book_dialog = dialog;
    }
}

/// How often the refresh path did its costly work since the frame started:
/// rows built for a window, log rows wrapped (measured), and treebar and
/// switchbar rebuilds. They only count up, so tests read deltas from the
/// automation dump; they assert work, never time.
#[derive(Debug, Default)]
pub(super) struct RefreshCounters {
    timeline_row_builds: u64,
    rows_wrapped: u64,
    treebar_rebuilds: u64,
    switchbar_rebuilds: u64,
    /// `timeline_row_builds` of room windows, by room.
    row_builds_by_room: BTreeMap<String, u64>,
}

impl RefreshCounters {
    pub(super) fn timeline_row_builds(&self) -> u64 {
        self.timeline_row_builds
    }

    pub(super) fn rows_wrapped(&self) -> u64 {
        self.rows_wrapped
    }

    pub(super) fn treebar_rebuilds(&self) -> u64 {
        self.treebar_rebuilds
    }

    pub(super) fn switchbar_rebuilds(&self) -> u64 {
        self.switchbar_rebuilds
    }

    pub(super) fn room_row_builds(&self, room_id: &str) -> u64 {
        self.row_builds_by_room.get(room_id).copied().unwrap_or(0)
    }
}
