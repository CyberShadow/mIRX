mod api;
mod matrix;
mod runtime;

pub const APP_NAME: &str = "mirx";
pub const FOUNDATION_PHASE: &str = "bootstrap";

pub use api::{
    AcceptRoomInviteRequest, BackfillRoomHistoryRequest, BanRoomMemberRequest, Command, CoreConfig,
    CoreErrorEvent, CreateDirectRoomRequest, CreateRoomRequest, CreateRoomVisibility,
    E2eeBackupState, E2eeBackupStatus, E2eeCrossSigningStatus, E2eeRecoveryOperation,
    E2eeRecoverySetupSucceeded, E2eeRecoveryState, E2eeRecoverySucceeded, E2eeStatus,
    EditMessageRequest, Event, FetchRoomEventRequest, FetchRoomMediaRequest,
    InviteRoomMemberRequest, JoinRoomRequest, KickRoomMemberRequest, LeaveRoomRequest,
    ListPublicRoomsRequest, LoginRequest, MarkRoomReadRequest, MemberPowerRole, Operation,
    PublicRoom, PublicRoomsPage, PublicRoomsResult, ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
    ROOM_HISTORY_BACKFILL_LIMIT_MAX, RecoverE2eeRequest, RedactEventRequest,
    RejectRoomInviteRequest, RestoreSessionRequest, RoomActivity, RoomActivityKind,
    RoomActivityRelation, RoomActivityUpdate, RoomActivityUpdateKind, RoomActivityUpdateTarget,
    RoomAudioActivity, RoomAudioInfo, RoomEncryptedFileHashes, RoomEncryptedFileKey,
    RoomEventFetch, RoomEventFetchResult, RoomFileActivity, RoomFileInfo, RoomHistoryBackfill,
    RoomHistoryBackfillEvent, RoomImageActivity, RoomImageInfo, RoomJoined, RoomMedia,
    RoomMediaFormat, RoomMediaKind, RoomMediaSource, RoomMemberChange, RoomMemberRank,
    RoomMemberSummary, RoomMembers, RoomMembership, RoomMessageActivity,
    RoomMessagePresentationKind, RoomMessageSendState, RoomPowerLevelUserChange,
    RoomPowerLevelValueChange, RoomReadState, RoomRedactedActivity, RoomStateNoticeActivity,
    RoomStateNoticeContent, RoomSummary, RoomTagSummary, RoomTyping, RoomUnableToDecryptActivity,
    RoomUnableToDecryptReason, RoomUserProfile, RoomVideoActivity, RoomVideoInfo, SendFileRequest,
    SendMessageReplyTarget, SendMessageRequest, SendMessageThreadTarget, SendReactionRequest,
    SetDisplayNameRequest, SetRoomMemberPowerLevelRequest, SetRoomTopicRequest,
    SetupE2eeRecoveryRequest, SpaceChildSummary, StoredSession, SyncStopReason,
    UnbanRoomMemberRequest, UnsupportedRoomMessageActivity, VerificationDecimals,
    VerificationEmoji, VerificationFlowId, VerificationRequest, VerificationState,
    VerificationStateEvent, login_server,
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
    "mirx-ui self-test ok"
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
        assert_eq!(ui_self_test_line(), "mirx-ui self-test ok");
    }
}
