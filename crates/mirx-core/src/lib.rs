mod api;
mod loopback;
mod matrix;
mod matrix_link;
mod mentions;
mod rich_text;
mod runtime;

pub const APP_NAME: &str = "mirx";
pub const FOUNDATION_PHASE: &str = "bootstrap";

pub use api::{
    AcceptRoomInviteRequest, BackfillRoomHistoryRequest, BanRoomMemberRequest, Command, CoreConfig,
    CoreErrorEvent, CreateDirectRoomRequest, CreateRoomRequest, CreateRoomVisibility,
    DeviceSummary, E2eeBackupState, E2eeBackupStatus, E2eeCrossSigningStatus,
    E2eeIdentityResetOutcome, E2eeRecoveryOperation, E2eeRecoverySetupSucceeded, E2eeRecoveryState,
    E2eeRecoverySucceeded, E2eeSessionState, E2eeStatus, EditMessageRequest, Event,
    FetchRoomEventRequest, FetchRoomMediaRequest, HighlightWord, IdentityChange, IdentityTrust,
    InviteRoomMemberRequest, JoinRoomRequest, KickRoomMemberRequest, LeaveRoomRequest,
    ListPublicRoomsRequest, LoginMethod, LoginRequest, MarkRoomReadRequest, MemberPowerRole,
    Operation, PublicRoom, PublicRoomsPage, PublicRoomsResult, ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
    ROOM_HISTORY_BACKFILL_LIMIT_MAX, RecoverE2eeRequest, RedactEventRequest,
    RejectRoomInviteRequest, RequestVerificationRequest, ResetE2eeIdentityRequest,
    RestoreSessionRequest, RoomActivity, RoomActivityKind, RoomActivityRelation,
    RoomActivityUpdate, RoomActivityUpdateKind, RoomActivityUpdateTarget, RoomAudioActivity,
    RoomAudioInfo, RoomEncryptedFileHashes, RoomEncryptedFileKey, RoomEventFetch,
    RoomEventFetchResult, RoomFileActivity, RoomFileInfo, RoomHistoryBackfill,
    RoomHistoryBackfillEvent, RoomIdentityChanges, RoomImageActivity, RoomImageInfo, RoomJoined,
    RoomMedia, RoomMediaFormat, RoomMediaKind, RoomMediaSource, RoomMemberChange, RoomMemberRank,
    RoomMemberSummary, RoomMembers, RoomMembership, RoomMessageActivity,
    RoomMessagePresentationKind, RoomMessageSendState, RoomNotificationMode,
    RoomPowerLevelUserChange, RoomPowerLevelValueChange, RoomReadState, RoomRedactedActivity,
    RoomStateNoticeActivity, RoomStateNoticeContent, RoomSummary, RoomTagSummary, RoomTyping,
    RoomUnableToDecryptActivity, RoomUnableToDecryptReason, RoomUserProfile, RoomVideoActivity,
    RoomVideoInfo, SendFileRequest, SendMessageReplyTarget, SendMessageRequest,
    SendMessageThreadTarget, SendReactionRequest, SessionAuth, SetDisplayNameRequest,
    SetRoomMemberPowerLevelRequest, SetRoomNotificationModeRequest, SetRoomTopicRequest,
    SetupE2eeRecoveryRequest, SpaceChildSummary, StoredSession, SyncStopReason, TextColor,
    TextStyle, TextStyleSpan, UnbanRoomMemberRequest, UnsupportedRoomMessageActivity, UserDevices,
    UserIdentityState, UserLinkSpan, UserProfile, VerificationDecimals, VerificationDirection,
    VerificationEmoji, VerificationFlowId, VerificationRequest, VerificationState,
    VerificationStateEvent, check_text_styles, check_user_links, login_method, login_server,
    validate_highlight_word,
};
pub use matrix_link::{MatrixLink, matrix_link};
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
