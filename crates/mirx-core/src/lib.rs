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
    AcceptRoomInviteRequest, AwayChanged, BackfillRoomHistoryRequest, BanRoomMemberRequest,
    Command, CommandId, ComposeFormat, CoreConfig, CoreErrorEvent, CreateDirectRoomRequest,
    CreateRoomRequest, CreateRoomVisibility, DeviceSummary, E2eeBackupState, E2eeBackupStatus,
    E2eeCrossSigningStatus, E2eeIdentityResetOutcome, E2eeRecoveryOperation,
    E2eeRecoverySetupSucceeded, E2eeRecoveryState, E2eeRecoverySucceeded, E2eeSessionState,
    E2eeStatus, EditMessageRequest, Event, ExportRoomKeysRequest, FetchRoomEventRequest,
    FetchRoomMediaRequest, HighlightWord, HistoryFrom, IdentityChange, IdentityTrust,
    ImportRoomKeysRequest, InviteRoomMemberRequest, JoinRoomRequest, KickRoomMemberRequest,
    LeaveRoomRequest, ListPublicRoomsRequest, LoginMethod, LoginRequest, LogoutRefusal,
    LogoutRequest, LogoutScope, MarkRoomReadRequest, MemberPowerLevelChange, MemberPowerRole,
    Operation, PresenceChange, PresenceKind, PublicRoom, PublicRoomsPage, PublicRoomsResult,
    ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT, ROOM_HISTORY_BACKFILL_LIMIT_MAX, RecoverE2eeRequest,
    RedactEventRequest, RejectRoomInviteRequest, RenameDeviceRequest, RequestVerificationRequest,
    ResetE2eeIdentityRequest, RestoreSessionRequest, RetryDecryptionRequest, RoomActivity,
    RoomActivityKind, RoomActivityRelation, RoomActivityUpdate, RoomActivityUpdateKind,
    RoomActivityUpdateTarget, RoomAudioActivity, RoomAudioInfo, RoomBan, RoomEncryptedFileHashes,
    RoomEncryptedFileKey, RoomEventFetch, RoomEventFetchResult, RoomEventsDecrypted,
    RoomFileActivity, RoomFileInfo, RoomHistoryBackfill, RoomHistoryBackfillEvent,
    RoomIdentityChanges, RoomImageActivity, RoomImageInfo, RoomInviteSummary, RoomJoinRule,
    RoomJoined, RoomKeysAtRisk, RoomMedia, RoomMediaFormat, RoomMediaKind, RoomMediaSource,
    RoomMemberChange, RoomMemberRank, RoomMemberSummary, RoomMembers, RoomMembership,
    RoomMessageActivity, RoomMessagePresentationKind, RoomMessageSendState, RoomNotificationMode,
    RoomPollActivity, RoomPollContent, RoomPowerLevelUserChange, RoomPowerLevelValueChange,
    RoomReadState, RoomRedactedActivity, RoomSettings, RoomSettingsPermissions,
    RoomStateNoticeActivity, RoomStateNoticeContent, RoomSummary, RoomTagSummary, RoomTyping,
    RoomUnableToDecryptActivity, RoomUnableToDecryptCause, RoomUserProfile,
    RoomVerificationRequestActivity, RoomVideoActivity, RoomVideoInfo, SendFileRequest,
    SendMessageReplyTarget, SendMessageRequest, SendMessageThreadTarget, SendReactionRequest,
    SessionAuth, SetDisplayNameRequest, SetReadMarkerRequest, SetRoomNotificationModeRequest,
    SetRoomTopicRequest, SetTypingRequest, SetupE2eeRecoveryRequest, SignOutDeviceRequest,
    SpaceChildSummary, StoredSession, SyncStopReason, TextColor, TextStyle, TextStyleSpan,
    UnbanRoomMemberRequest, UnsupportedRoomMessageActivity, UpdateRoomSettingsRequest, UserDevices,
    UserIdentityState, UserLinkSpan, UserPresence, UserProfile, VerificationDecimals,
    VerificationDirection, VerificationEmoji, VerificationFlowId, VerificationRequest,
    VerificationState, VerificationStateEvent, check_text_styles, check_user_links, login_method,
    login_server, validate_highlight_word,
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
