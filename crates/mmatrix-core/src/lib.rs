mod api;
mod matrix;
mod runtime;

pub const APP_NAME: &str = "mmatrix";
pub const FOUNDATION_PHASE: &str = "bootstrap";

pub use api::{
    Command, CoreConfig, CoreErrorEvent, E2eeBackupState, E2eeBackupStatus, E2eeCrossSigningStatus,
    E2eeRecoveryOperation, E2eeRecoveryState, E2eeRecoverySucceeded, E2eeStatus, Event,
    LoginRequest, MarkRoomReadRequest, Operation, RecoverE2eeRequest, RestoreSessionRequest,
    RoomActivity, RoomActivityKind, RoomActivityUpdate, RoomActivityUpdateKind,
    RoomMessageActivity, RoomMessagePresentationKind, RoomMessageSendState, RoomReadState,
    RoomStateNoticeActivity, RoomStateNoticeContent, RoomSummary, RoomTyping, SendMessageRequest,
    SpaceChildSummary, StoredSession, UnsupportedRoomMessageActivity, VerificationDecimals,
    VerificationEmoji, VerificationFlowId, VerificationRequest, VerificationState,
    VerificationStateEvent,
};
pub use runtime::{CommandSender, CoreHandle, SendCommandError, StartError, start};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceInfo {
    pub project: &'static str,
    pub phase: &'static str,
    pub windows_target: &'static str,
}

pub fn workspace_info() -> WorkspaceInfo {
    WorkspaceInfo {
        project: APP_NAME,
        phase: FOUNDATION_PHASE,
        windows_target: "x86_64-pc-windows-gnu",
    }
}

pub fn ui_self_test_line() -> &'static str {
    "mmatrix-ui self-test ok"
}

#[cfg(test)]
mod tests {
    use super::{APP_NAME, FOUNDATION_PHASE, ui_self_test_line, workspace_info};

    #[test]
    fn workspace_info_matches_bootstrap_target() {
        let info = workspace_info();
        assert_eq!(info.project, APP_NAME);
        assert_eq!(info.phase, FOUNDATION_PHASE);
        assert_eq!(info.windows_target, "x86_64-pc-windows-gnu");
    }

    #[test]
    fn ui_self_test_line_is_stable() {
        assert_eq!(ui_self_test_line(), "mmatrix-ui self-test ok");
    }
}
