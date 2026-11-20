#![allow(unsafe_op_in_unsafe_fn)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::{OsStr, c_void};
use std::fs;
use std::io::ErrorKind;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::ptr::{copy_nonoverlapping, null, null_mut};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, mpsc::RecvTimeoutError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mirx_core::{
    AcceptRoomInviteRequest, BackfillRoomHistoryRequest, BanRoomMemberRequest, Command,
    CommandSender, CoreConfig, CoreErrorEvent, CoreHandle, CreateDirectRoomRequest,
    CreateRoomRequest, CreateRoomVisibility, E2eeBackupState, E2eeBackupStatus,
    E2eeCrossSigningStatus, E2eeRecoveryState, E2eeStatus, EditMessageRequest, Event,
    FetchRoomEventRequest, FetchRoomMediaRequest, HighlightWord, InviteRoomMemberRequest,
    JoinRoomRequest, KickRoomMemberRequest, LeaveRoomRequest, LoginRequest, MarkRoomReadRequest,
    MemberPowerRole, Operation, PublicRoomsPage, ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
    RecoverE2eeRequest, RedactEventRequest, RejectRoomInviteRequest, RestoreSessionRequest,
    RoomActivity, RoomActivityKind, RoomActivityRelation, RoomActivityUpdate,
    RoomActivityUpdateKind, RoomActivityUpdateTarget, RoomAudioInfo, RoomEventFetch,
    RoomEventFetchResult, RoomFileInfo, RoomHistoryBackfill, RoomHistoryBackfillEvent,
    RoomImageInfo, RoomMedia, RoomMediaFormat, RoomMediaKind, RoomMediaSource, RoomMemberSummary,
    RoomMembers, RoomMembership, RoomMessageActivity, RoomMessagePresentationKind,
    RoomMessageSendState, RoomNotificationMode, RoomReadState, RoomRedactedActivity,
    RoomStateNoticeContent, RoomSummary, RoomTyping, RoomUserProfile, RoomVideoInfo,
    SendFileRequest, SendMessageReplyTarget, SendMessageRequest, SendMessageThreadTarget,
    SendReactionRequest, SetDisplayNameRequest, SetRoomMemberPowerLevelRequest,
    SetRoomNotificationModeRequest, SetRoomTopicRequest, SetupE2eeRecoveryRequest, StoredSession,
    TextStyleSpan, UnbanRoomMemberRequest, UserProfile, VerificationEmoji, VerificationFlowId,
    VerificationRequest, VerificationState, VerificationStateEvent, login_server, start,
};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use windows_sys::Win32::Foundation::{
    CloseHandle, FILETIME, GetLastError, GlobalFree, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM,
    LRESULT, POINT, RECT, SYSTEMTIME, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT,
    ClientToScreen, CreateFontIndirectW, CreateSolidBrush, DEFAULT_CHARSET, DT_CENTER,
    DT_END_ELLIPSIS, DT_SINGLELINE, DeleteObject, DrawFocusRect, EndPaint, FW_BOLD, FW_NORMAL,
    FillRect, FrameRect, GetDC, GetDeviceCaps, GetSysColor, GetSysColorBrush, GetTextFaceW,
    GetTextMetricsW, HBRUSH, HFONT, InvalidateRect, LOGFONTW, LOGPIXELSY, MapWindowPoints,
    PAINTSTRUCT, ReleaseDC, ScreenToClient, SelectObject, SetBkMode, SetTextColor, TEXTMETRICW,
    TRANSPARENT, TextOutW,
};
use windows_sys::Win32::Graphics::GdiPlus::{
    GdipCreateFromHDC, GdipDeleteGraphics, GdipDisposeImage, GdipDrawImageRectI,
    GdipGetImageHeight, GdipGetImageWidth, GdipLoadImageFromStream, GdiplusShutdown,
    GdiplusStartup, GdiplusStartupInput, GpGraphics, GpImage,
};
use windows_sys::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, PAGE_READWRITE, UnmapViewOfFile,
};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use windows_sys::Win32::UI::Controls::Dialogs::{
    CF_INITTOLOGFONTSTRUCT, CF_LIMITSIZE, CF_NOVERTFONTS, CF_SCREENFONTS, CHOOSEFONTW, ChooseFontW,
    CommDlgExtendedError,
};
use windows_sys::Win32::UI::Controls::{
    BTNS_CHECK, BTNS_SHOWTEXT, CCS_NOPARENTALIGN, CCS_NORESIZE, CDDS_ITEMPREPAINT, CDDS_PREPAINT,
    CDRF_NOTIFYITEMDRAW, DRAWITEMSTRUCT, EM_GETSEL, EM_REPLACESEL, EM_SETSEL, HDM_GETITEMCOUNT,
    HDM_GETITEMRECT, I_IMAGENONE, ICC_BAR_CLASSES, ICC_LISTVIEW_CLASSES, ICC_TREEVIEW_CLASSES,
    INITCOMMONCONTROLSEX, InitCommonControlsEx, LVCF_FMT, LVCF_TEXT, LVCF_WIDTH, LVCFMT_LEFT,
    LVCFMT_RIGHT, LVCOLUMNW, LVIF_TEXT, LVIR_LABEL, LVIS_FOCUSED, LVIS_SELECTED, LVITEMW,
    LVM_GETCOLUMNW, LVM_GETCOUNTPERPAGE, LVM_GETHEADER, LVM_GETITEMCOUNT, LVM_GETITEMRECT,
    LVM_GETITEMTEXTW, LVM_GETNEXTITEM, LVM_GETTOPINDEX, LVM_INSERTCOLUMNW,
    LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMCOUNT, LVM_SETITEMSTATE, LVN_COLUMNCLICK,
    LVN_GETDISPINFOW, LVNI_SELECTED, LVS_EX_FULLROWSELECT, LVS_OWNERDATA, LVS_REPORT,
    LVS_SHOWSELALWAYS, LVS_SINGLESEL, LVSICF_NOSCROLL, MEASUREITEMSTRUCT, NM_CUSTOMDRAW, NM_DBLCLK,
    NM_RETURN, NMHDR, NMITEMACTIVATE, NMLISTVIEW, NMLVDISPINFOW, NMTBCUSTOMDRAW, NMTVCUSTOMDRAW,
    ODS_FOCUS, ODS_SELECTED, SB_SETTEXTW, SBARS_SIZEGRIP, STATUSCLASSNAMEW, SetScrollInfo,
    TB_ADDBUTTONSW, TB_BUTTONCOUNT, TB_BUTTONSTRUCTSIZE, TB_CHECKBUTTON, TB_DELETEBUTTON,
    TB_GETBUTTONSIZE, TB_GETITEMRECT, TB_GETROWS, TB_SETBITMAPSIZE, TB_SETBUTTONWIDTH,
    TB_SETDRAWTEXTFLAGS, TB_SETEXTENDEDSTYLE, TB_SETMAXTEXTROWS, TBBUTTON, TBSTATE_CHECKED,
    TBSTATE_ENABLED, TBSTYLE_EX_MIXEDBUTTONS, TBSTYLE_FLAT, TBSTYLE_WRAPABLE, TOOLBARCLASSNAMEW,
    TVE_EXPAND, TVGN_CARET, TVGN_CHILD, TVGN_NEXT, TVGN_PARENT, TVGN_ROOT, TVHITTESTINFO, TVI_LAST,
    TVI_ROOT, TVIF_TEXT, TVINSERTSTRUCTW, TVINSERTSTRUCTW_0, TVITEMEXW, TVM_DELETEITEM, TVM_EXPAND,
    TVM_GETITEMRECT, TVM_GETITEMW, TVM_GETNEXTITEM, TVM_HITTEST, TVM_INSERTITEMW, TVM_SELECTITEM,
    TVN_SELCHANGEDW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetKeyState, SetFocus, VK_CONTROL, VK_MENU, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ACCEL, CreateAcceleratorTableW, DestroyAcceleratorTable, FALT, FCONTROL, FVIRTKEY, GetMenu,
    GetMenuItemInfoW, HACCEL, MENUITEMINFOW, MFS_CHECKED, MFS_DISABLED, MFT_BITMAP, MFT_SEPARATOR,
    MIIM_FTYPE, MIIM_STATE, MIIM_STRING, MIIM_SUBMENU, TranslateAcceleratorW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, AppendMenuW, BM_GETCHECK, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_PUSHBUTTON,
    CLIENTCREATESTRUCT, CREATESTRUCTW, CS_DBLCLKS, CW_USEDEFAULT, CallWindowProcW, CheckMenuItem,
    CheckMenuRadioItem, CreateMenu, CreatePopupMenu, CreateWindowExW, DefFrameProcW,
    DefMDIChildProcW, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, DrawMenuBar,
    ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE, ES_PASSWORD, ES_READONLY, EnableMenuItem,
    FLASHW_TIMERNOFG, FLASHW_TRAY, FLASHWINFO, FlashWindowEx, GA_ROOT, GWL_STYLE, GWLP_WNDPROC,
    GetAncestor, GetClassNameW, GetClientRect, GetDlgItem, GetForegroundWindow, GetMenuItemCount,
    GetMenuState, GetMenuStringW, GetMessageW, GetParent, GetScrollInfo, GetSystemMenu,
    GetWindowLongPtrW, GetWindowRect, HMENU, HideCaret, IDC_ARROW, IDYES, IsChild, IsIconic,
    IsWindow, IsZoomed, KillTimer, LB_ADDSTRING, LB_ERR, LB_GETCURSEL, LB_GETITEMRECT, LB_GETTEXT,
    LB_GETTEXTLEN, LB_ITEMFROMPOINT, LB_RESETCONTENT, LB_SETCURSEL, LB_SETITEMHEIGHT,
    LBS_HASSTRINGS, LBS_NOINTEGRALHEIGHT, LBS_OWNERDRAWFIXED, LoadCursorW, MB_ICONERROR,
    MB_ICONQUESTION, MB_OK, MB_YESNO, MDICREATESTRUCTW, MDITILE_HORIZONTAL, MDITILE_VERTICAL,
    MF_BYCOMMAND, MF_BYPOSITION, MF_CHECKED, MF_ENABLED, MF_GRAYED, MF_POPUP, MF_SEPARATOR,
    MF_STRING, MF_UNCHECKED, MSG, MessageBoxW, MoveWindow, PostMessageW, PostQuitMessage,
    RegisterClassExW, SB_BOTTOM, SB_LINEDOWN, SB_LINEUP, SB_PAGEDOWN, SB_PAGEUP, SB_THUMBPOSITION,
    SB_THUMBTRACK, SB_TOP, SB_VERT, SC_CLOSE, SCROLLINFO, SHOW_WINDOW_CMD, SIF_ALL, SIF_PAGE,
    SIF_POS, SIF_RANGE, SIF_TRACKPOS, SW_HIDE, SW_MINIMIZE, SW_SHOW, SendMessageW,
    SetForegroundWindow, SetMenu, SetTimer, SetWindowLongPtrW, ShowWindow, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMDISysAccel, TranslateMessage, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU, WM_COPYDATA, WM_CREATE,
    WM_DESTROY, WM_DRAWITEM, WM_GETFONT, WM_GETTEXT, WM_GETTEXTLENGTH, WM_INITMENUPOPUP,
    WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_MDIACTIVATE, WM_MDICASCADE, WM_MDICREATE,
    WM_MDIDESTROY, WM_MDIGETACTIVE, WM_MDIICONARRANGE, WM_MDIMAXIMIZE, WM_MDIRESTORE, WM_MDITILE,
    WM_MEASUREITEM, WM_NCCREATE, WM_NCDESTROY, WM_NOTIFY, WM_PAINT, WM_PASTE, WM_SETFOCUS,
    WM_SETFONT, WM_SETTEXT, WM_SIZE, WM_SYSCOMMAND, WM_TIMER, WM_VSCROLL, WNDCLASSEXW, WNDPROC,
    WS_BORDER, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN, WS_EX_CLIENTEDGE, WS_MAXIMIZE, WS_OVERLAPPED,
    WS_OVERLAPPEDWINDOW, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use self::glyph_fonts::GlyphFonts;
use crate::{
    AppCommand, FrameSize, HELP, RunOptions,
    automation::{
        AcceptedResponse, COMMAND_ID_DUMP_COMPOSE_TEXT, COMMAND_ID_DUMP_ROOM_ACTIVITY,
        COMMAND_ID_LOAD_SYNTHETIC_STATE, COMMAND_ID_PING, COMMAND_ID_WAIT_IDLE, ComposeKey,
        ComposeTextResponse, ConnectionState, ContextMenuCommand, DumpE2eeRecoverySetupResult,
        DumpMemberMenuState, DumpMemberModerationAction, DumpMemberPowerRole,
        DumpMembershipCommandAction, DumpMessageMenuState, DumpModel, DumpRoomActivity,
        DumpRoomActivityKind, DumpRoomActivityResponse, DumpRoomMenuState,
        DumpRoomMessageActivityLine, DumpRoomPreviewMediaState, DumpRoomReactionSummary,
        DumpSubmittedDirectRoomCommand, DumpSubmittedEditCommand, DumpSubmittedFileCommand,
        DumpSubmittedHistoryCommand, DumpSubmittedMemberModerationCommand,
        DumpSubmittedMembershipCommand, DumpSubmittedMessageCommand,
        DumpSubmittedNotificationCommand, DumpSubmittedPowerLevelCommand,
        DumpSubmittedProfileCommand, DumpSubmittedReactionCommand, DumpSubmittedRecoveryCommand,
        DumpSubmittedRedactionCommand, DumpSubmittedReplyCommand, DumpSubmittedReplyThreadTarget,
        DumpSubmittedRoomListRequest, DumpSubmittedRoomTopicCommand,
        DumpSubmittedSetupRecoveryCommand, DumpSubmittedUserCommand,
        DumpSubmittedVerificationAction, DumpUserCommandAction, DumpVerificationActionKind,
        DumpVerificationMenuState, FailureEnvelope, FindActiveViewResponse, GetVersionResponse,
        LifecycleState, ListRoomsResponse, LoadEventsResponse, MdiArrangement, MdiState, MdiView,
        MdiWindowPlacement, MdiWindowState, NavigationNode, NavigationPathSegment, ParsedCommand,
        PongResponse, ResponseEnvelope, RoomMemberView, SelectRoomResponse, SelectViewResponse,
        SelectedNavigationPathResponse, SendTextResponse, StatusBarState, SwitchbarItem,
        SwitchbarState, UiActiveTimeline, UiComposeTarget, UiComposeTargetKind,
        UiFindHistorySearchState, UiFindState, UiFrameLayout, UiMenu, UiMenuItem, UiPoint, UiRect,
        UiRoomHistoryBackfillState, UiRoomList, UiRoomState, UiState, UiTextFont, UiTimelineLine,
        UiTimelineLineKind, UiTimestamp, VerificationActionArgs, ViewActivityLevel, ViewIdentity,
        WaitIdleResponse, dump_error_from_core, dump_room_activity_from_activity,
        dump_room_from_summary, dump_room_message_send_state_from_core,
        dump_ui_room_read_state_from_core, dump_verification_from_state_event, failure_response,
        parse_command_request, success_response,
    },
};

mod account;
mod attachments;
mod automation_dump;
mod automation_host;
mod cmap;
mod commands;
mod connection;
mod dialogs;
mod echo;
mod errors;
mod find;
mod fonts;
mod glyph_fonts;
mod glyph_runs;
mod ignore;
mod images;
mod mdi;
mod menus;
mod mirc_format;
mod nick_completion;
mod notifications;
mod options;
mod options_dialog;
mod reactions;
mod replies;
mod room_list;
mod room_targets;
mod self_test;
mod send_file;
mod settings;
mod status_log;
mod switchbar;
#[cfg(test)]
mod test_fixtures;
mod timeline_layout;
mod timeline_text;
mod timestamp;
mod verification;
mod window_data;

#[cfg(test)]
use account::{
    PROFILE_SUBDIR, SAVED_SESSION_VERSION, SESSION_FILE_NAME, SavedSession, load_saved_session,
};
use account::{
    delete_saved_session, load_or_create_store_passphrase, profile_dir, save_session, session_path,
    submit_login, submit_logout, submit_recover_e2ee, submit_restore_saved_session,
    submit_restore_session, submit_setup_e2ee_recovery, submit_start_sync,
};
use attachments::{
    AttachmentSaveResult, PendingAttachmentSave, apply_attachment_save_media_event,
    save_selected_message_attachment,
};
use automation_dump::{
    active_view_identity, dump_member_menu_state, dump_message_menu_state, dump_model_from_state,
    dump_room_activity_entry_from_room_entry, dump_room_menu_state, dump_ui_state,
    dump_verification_menu_state,
};
#[cfg(test)]
use automation_dump::{dump_frame_layout, frame_child_rect};
use automation_host::{
    AutomationState, destroy_automation_state, enable_automation, handle_copydata,
    send_automation_request,
};
use commands::{
    BarCommand, CommandError, ComposeSubmission, CreateRoomArgs, E2eeSlashCommand, FontCommand,
    SlashCommand, TimestampCommand, VerificationAction, local_command_history_entry,
    parse_compose_submission,
};
use connection::ConnectAction;
#[cfg(test)]
use dialogs::{
    ABOUT_DIALOG_CLASS_NAME, ABOUT_DIALOG_OK_ID, FIND_DIALOG_CLASS_NAME, FindDialogState,
};
use dialogs::{
    SubmittedSetupE2eeRecovery, create_child_window, pack_size_lparam, register_about_dialog_class,
    register_find_dialog_class, register_login_dialog_class, register_recover_e2ee_dialog_class,
    register_recovery_key_handoff_dialog_class, register_setup_e2ee_dialog_class, run_modal_dialog,
    run_options_dialog, show_about_dialog, show_e2ee_recovery_key, show_login_dialog,
    show_recover_e2ee_dialog, show_setup_e2ee_dialog, show_single_field_dialog,
};
use echo::{EchoLine, EchoLog};
use errors::{ErrorDestination, error_destination};
#[cfg(test)]
use find::build_view_find_state;
use find::{
    ViewFindState, active_view_find_feedback, apply_find_to_active_view, begin_find_dialog,
    clear_find_projection_for_timeline, continue_timeline_find_history_search_after_refresh,
    dump_active_find_state, execute_active_view_find, find_active_view_response,
    pending_timeline_find_history_views, recompute_find_for_view,
    scroll_timeline_to_source_row_index,
};
use fonts::{
    OwnedFont, TextFont, apply_font_setting, font_line_height, run_font_dialog, set_timeline_font,
    set_view_text_font,
};
#[cfg(test)]
use images::room_preview_media_storage_key;
use images::{
    GdiplusImage, GdiplusSession, apply_room_media_event, collect_lazy_room_preview_media_requests,
    dispatch_lazy_room_preview_media_requests, initialize_gdiplus_state, prune_decoded_images,
    timeline_thumbnail_format, timeline_video_preview_source,
};
use menus::{
    BarMenu, COMMANDS_MENU, FILE_MENU, HELP_MENU, MEMBER_CONTROL_MENU, MEMBER_MENU, MENU_BAR,
    MENU_ID_COMMANDS_FIRST, MENU_ID_COMMANDS_LIST, MENU_ID_FILE_CONNECT, MENU_ID_FILE_DISCONNECT,
    MENU_ID_FILE_EXIT, MENU_ID_FILE_SELECT_SERVER, MENU_ID_FIND, MENU_ID_HELP_ABOUT,
    MENU_ID_HELP_TOPICS, MENU_ID_LOGOUT, MENU_ID_MEMBER_BAN, MENU_ID_MEMBER_DEOP,
    MENU_ID_MEMBER_DEVOICE, MENU_ID_MEMBER_IGNORE, MENU_ID_MEMBER_KICK, MENU_ID_MEMBER_MENTION,
    MENU_ID_MEMBER_OP, MENU_ID_MEMBER_QUERY, MENU_ID_MEMBER_UNIGNORE, MENU_ID_MEMBER_VOICE,
    MENU_ID_MEMBER_WHOIS, MENU_ID_MESSAGE_EDIT, MENU_ID_MESSAGE_GO_TO_ORIGINAL,
    MENU_ID_MESSAGE_REACT, MENU_ID_MESSAGE_REDACT, MENU_ID_MESSAGE_REPLY,
    MENU_ID_MESSAGE_SAVE_ATTACHMENT, MENU_ID_RECOVER_E2EE, MENU_ID_ROOM_ACCEPT_INVITE,
    MENU_ID_ROOM_NOTIFY_ALL, MENU_ID_ROOM_NOTIFY_MENTIONS, MENU_ID_ROOM_NOTIFY_MUTE,
    MENU_ID_ROOM_PART, MENU_ID_ROOM_REJECT_INVITE, MENU_ID_SETUP_E2EE_RECOVERY,
    MENU_ID_TOOLS_OPTIONS, MENU_ID_VERIFY_ACCEPT, MENU_ID_VERIFY_CANCEL,
    MENU_ID_VERIFY_CONFIRM_SAS, MENU_ID_VERIFY_REJECT_SAS, MENU_ID_VIEW_SWITCHBAR,
    MENU_ID_VIEW_TREEBAR, MENU_ID_WINDOW_ARRANGE_ICONS, MENU_ID_WINDOW_CASCADE,
    MENU_ID_WINDOW_TILE_HORIZONTAL, MENU_ID_WINDOW_TILE_VERTICAL, MenuEntry,
    ROOM_NOTIFICATIONS_MENU, TOOLS_MENU, VIEW_MENU, WINDOW_MENU, commands_menu_item,
    commands_menu_line, commands_menu_title, file_menu_state, member_menu_command,
    member_menu_item_enabled, member_menu_item_ids, notification_menu_id, notification_menu_mode,
    notification_mode_label, verification_menu_action_enabled, view_menu_bar, view_menu_checks,
};
use nick_completion::{NickCompletion, member_mention_text};
use replies::ComposeTarget;
use room_list::{PageApplied, RoomListColumn, RoomListQuery, RoomListState};
use room_targets::{RoomTargetError, resolve_room};
use self_test::run_self_test;
use status_log::{StatusLineKind, StatusLog};
use verification::{
    apply_verification_event, e2ee_backup_health_summary, e2ee_cross_signing_label,
    e2ee_local_backup_label, e2ee_recovery_state_label, e2ee_server_backup_label,
    format_verification_emojis, submit_accept_verification, submit_automation_verification_action,
    submit_cancel_verification, submit_confirm_verification_sas, submit_menu_verification_action,
    submit_reject_verification_sas, verification_action_label,
    verification_state_event_from_request, verification_state_label,
};

use settings::{Bar, FontSetting, SETTINGS_FILE_NAME, SettingsStore};
use timeline_layout::{
    TimelinePaintLine, TimelinePaintLineVisual, activity_level_color, paint_timeline,
    plain_timeline_paint_lines, rendered_line_ranges, timeline_row_paint_lines,
};
use timeline_text::{
    ENCRYPTED_ROOM_TEXT, INVITE_RESPONSE_TEXT, action_line, context_line, event_line, invited_text,
    member_change_text, message_line, nick_label, now_talking_text, power_levels_change_text,
    rank_prefix, topic_change_text, topic_is_text,
};
#[cfg(test)]
use timestamp::TimestampSetting;
use timestamp::{LocalTime, LocalTimeFn, TimestampStyle};

const FRAME_CLASS_NAME: &str = "mirxWin32Frame";
const MDI_CHILD_CLASS_NAME: &str = "mirxWin32MdiChild";
const ROOM_CONTENT_CLASS_NAME: &str = "mirxWin32RoomContent";
/// `WC_LISTVIEWW`, the common controls' list view.
const ROOM_LIST_CLASS_NAME: &str = "SysListView32";
const TIMELINE_CLASS_NAME: &str = "mirxWin32Timeline";
const FRAME_TITLE: &str = "mirx";
const DEFAULT_FRAME_SIZE: FrameSize = FrameSize {
    width: 1024,
    height: 768,
};
const TREE_CLASS_NAME: &str = "SysTreeView32";
const MDI_CLASS_NAME: &str = "MDICLIENT";
const EDIT_CLASS_NAME: &str = "EDIT";
const STATIC_CLASS_NAME: &str = "STATIC";
const BUTTON_CLASS_NAME: &str = "BUTTON";
const TREE_ID: u16 = 1001;
const MDI_ID: u16 = 1002;
const STATUS_BAR_ID: u16 = 1004;
const SWITCHBAR_ID: u16 = 1005;
const VIEW_CONTENT_ID: u16 = 2001;
/// Every MDI child's editbox; a sibling of its `VIEW_CONTENT_ID` window.
const VIEW_EDIT_ID: u16 = 2002;
/// The window menu's Font... item. `WM_SYSCOMMAND` ids are below 0xF000
/// with the low four bits clear: Windows uses those bits itself.
const SYSMENU_ID_FONT: usize = 0x0110;
const SYSMENU_FONT_LABEL: &str = "&Font...";
/// The window menu's Find Text item, mIRC's window-menu Find.
const SYSMENU_ID_FIND: usize = 0x0120;
const SYSMENU_FIND_LABEL: &str = "Find &Text...\tCtrl+F";
/// A room window's window-menu Send File..., mIRC's DCC Send dialog.
const SYSMENU_ID_SEND_FILE: usize = 0x0130;
const SYSMENU_SEND_FILE_LABEL: &str = "Send &File...";
const COMPOSE_EDIT_STYLE: WINDOW_STYLE = WS_CHILD | WS_BORDER | ES_AUTOHSCROLL as u32;
const COMPOSE_EDIT_EX_STYLE: WINDOW_EX_STYLE = WS_EX_CLIENTEDGE;
const SWITCHBAR_COMMAND_FIRST: usize = 41001;
const SWITCHBAR_COMMAND_LAST: usize = 41999;
const WM_CORE_EVENT: u32 = WM_APP + 1;
const WM_SELF_TEST: u32 = WM_APP + 2;
/// Posted at startup without a saved session, so Select Server opens once
/// the frame is shown, as mIRC opens its connect options on a first run.
const WM_CONNECT_AT_STARTUP: u32 = WM_APP + 3;
const WM_TIMELINE_GET_TOP_LINE: u32 = WM_APP + 100;
const AUTO_CLOSE_TIMER_ID: usize = 1;
const AUTO_CLOSE_DELAY_MS: u32 = 250;
const TREE_WIDTH: i32 = 240;
const TIMELINE_LEFT_PADDING: i32 = 8;
const TIMELINE_RIGHT_PADDING: i32 = 8;
const TIMELINE_VERTICAL_PADDING: i32 = 6;
const TIMELINE_IMAGE_THUMBNAIL_MAX_WIDTH: i32 = 160;
const TIMELINE_IMAGE_THUMBNAIL_MAX_HEIGHT: i32 = 120;
/// Space around a thumbnail inside its slot, over the slot's lines.
const TIMELINE_IMAGE_SLOT_PADDING: i32 = 20;
const TIMELINE_IMAGE_LEFT_INDENT: i32 = 24;
const TIMELINE_FIND_HIGHLIGHT_COLOR: u32 = 0x00f0f8d8;
const TIMELINE_SELECTION_COLOR: u32 = 0x00f5e6cc;
const DEFAULT_DEVICE_DISPLAY_NAME: &str = "mirx";
const VK_RETURN_KEY: WPARAM = 13;
const VK_UP_KEY: WPARAM = 38;
const VK_DOWN_KEY: WPARAM = 40;
const VK_DELETE_KEY: WPARAM = 46;
const VK_TAB_KEY: WPARAM = 9;
const VK_ESCAPE_KEY: WPARAM = 27;

#[derive(Default)]
struct Controls {
    tree: HWND,
    mdi_client: HWND,
    status_bar: HWND,
    switchbar: HWND,
    /// The menubar's File, View and Tools menus, which
    /// `refresh_menu_popup` updates before they open.
    file_menu: HMENU,
    view_menu: HMENU,
    tools_menu: HMENU,
    /// Right-click menus. They hang in no menubar, so `WM_DESTROY`
    /// destroys them.
    room_menu: HMENU,
    /// The room menu's Notifications submenu, which gets its own
    /// `WM_INITMENUPOPUP`; destroyed with `room_menu`.
    room_notifications_menu: HMENU,
    message_menu: HMENU,
    member_menu: HMENU,
    accelerators: HACCEL,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ViewId {
    Status,
    Room(String),
    Thread {
        room_id: String,
        thread_root_event_id: String,
    },
    /// The Rooms List window, mIRC's Channels List; there is one at most.
    RoomList,
}

impl ViewId {
    fn room_id(&self) -> Option<&str> {
        match self {
            Self::Status | Self::RoomList => None,
            Self::Room(room_id) => Some(room_id.as_str()),
            Self::Thread { room_id, .. } => Some(room_id.as_str()),
        }
    }

    fn thread_key(&self) -> Option<(&str, &str)> {
        match self {
            Self::Thread {
                room_id,
                thread_root_event_id,
            } => Some((room_id.as_str(), thread_root_event_id.as_str())),
            _ => None,
        }
    }
}

struct MdiViewState {
    child: HWND,
    content: HWND,
    /// `None` only for the Rooms List, which has no editbox, as mIRC's
    /// Channels List has none.
    compose: Option<ViewCompose>,
}

/// A window's editbox and what it remembers.
struct ViewCompose {
    edit: HWND,
    /// What Up and Down recall in this window's editbox, as in mIRC, where
    /// each window remembers what was typed in it.
    history: ComposeHistory,
    /// The message the editbox replies to or edits, until sent or Esc.
    target: Option<ComposeTarget>,
}

impl MdiViewState {
    /// The editbox of a window that has one: every window the user can
    /// type in, which the Rooms List is not.
    fn compose(&self) -> &ViewCompose {
        self.compose
            .as_ref()
            .expect("only the Rooms List has no editbox")
    }

    fn compose_mut(&mut self) -> &mut ViewCompose {
        self.compose
            .as_mut()
            .expect("only the Rooms List has no editbox")
    }
}

struct MdiChildCreateState {
    view_id: ViewId,
}

struct RoomContentState {
    timeline: HWND,
    members: HWND,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum TimelineRowIdentity {
    ActivityPrimary {
        activity_id: String,
    },
    ActivityReactionSummary {
        activity_id: String,
    },
    /// The quote line above a reply.
    ActivityReplyQuote {
        activity_id: String,
    },
    Synthetic {
        label: TimelineSyntheticRowLabel,
        logical_text: String,
    },
    /// A Status log line; identical lines stay distinct.
    StatusLine {
        sequence: u64,
    },
    /// A command's line in a room or thread window.
    EchoLine {
        sequence: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TimelineSyntheticRowLabel {
    NowTalking,
    Topic,
    Encryption,
    Invite,
    Typing,
    Backfill,
    /// What the editbox replies to or edits.
    ComposeTarget,
    Context,
    Plain,
}

impl TimelineRowIdentity {
    fn activity_primary(activity_id: &str) -> Self {
        Self::ActivityPrimary {
            activity_id: activity_id.to_owned(),
        }
    }

    fn activity_reaction_summary(activity_id: &str) -> Self {
        Self::ActivityReactionSummary {
            activity_id: activity_id.to_owned(),
        }
    }

    fn activity_reply_quote(activity_id: &str) -> Self {
        Self::ActivityReplyQuote {
            activity_id: activity_id.to_owned(),
        }
    }

    fn status_line(sequence: u64) -> Self {
        Self::StatusLine { sequence }
    }

    fn synthetic(label: TimelineSyntheticRowLabel, logical_text: &str) -> Self {
        Self::Synthetic {
            label,
            logical_text: logical_text.to_owned(),
        }
    }

    fn default_for_row(kind: &TimelineRowKind, logical_text: &str) -> Self {
        let label = match kind {
            TimelineRowKind::Typing => TimelineSyntheticRowLabel::Typing,
            TimelineRowKind::Plain => TimelineSyntheticRowLabel::Plain,
            _ => TimelineSyntheticRowLabel::Context,
        };
        Self::synthetic(label, logical_text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimelineRow {
    identity: TimelineRowIdentity,
    event_id: Option<String>,
    kind: TimelineRowKind,
    /// The row's text, painted exactly as find and selection see it.
    logical_text: String,
    /// How `logical_text` is formatted: a message's own styles, at their
    /// offsets in the row's text. Every other row is plain.
    styles: Vec<TextStyleSpan>,
    layout: TimelineRowLayout,
    image_state: Option<TimelineImageState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimelineRowLayout {
    /// Context rows (typing, history backfill): one painted line per text line.
    FullWidth,
    /// mIRC log lines: wrapped to the view, continuation lines indented past
    /// the timestamp.
    Log,
}

impl TimelineRow {
    fn full_width(kind: TimelineRowKind, text: String) -> Self {
        Self {
            identity: TimelineRowIdentity::default_for_row(&kind, &text),
            event_id: None,
            kind,
            logical_text: text,
            styles: Vec::new(),
            layout: TimelineRowLayout::FullWidth,
            image_state: None,
        }
    }

    fn log(kind: TimelineRowKind, text: String) -> Self {
        Self {
            identity: TimelineRowIdentity::default_for_row(&kind, &text),
            event_id: None,
            kind,
            logical_text: text,
            styles: Vec::new(),
            layout: TimelineRowLayout::Log,
            image_state: None,
        }
    }

    fn with_identity(mut self, identity: TimelineRowIdentity) -> Self {
        self.identity = identity;
        self
    }

    fn with_event_id(mut self, event_id: Option<&str>) -> Self {
        self.event_id = event_id.map(str::to_owned);
        self
    }

    fn with_styles(mut self, styles: Vec<TextStyleSpan>) -> Self {
        assert_eq!(
            mirx_core::check_text_styles(&self.logical_text, &styles),
            Ok(()),
            "a row's styles format its text"
        );
        self.styles = styles;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Rows for lines other users wrote carry `highlight`: the user's push
/// rules highlighted the line, so it is drawn in the highlight colour.
enum TimelineRowKind {
    /// mIRC's green event lines, such as `* Now talking in` and `* Topic is`.
    Event,
    /// mIRC's navy info lines.
    Info,
    Error,
    Typing,
    ReactionSummary,
    /// What a reply answers, above it.
    ReplyQuote,
    Message {
        presentation: RoomMessagePresentationKind,
        send_state: TimelineMessageSendState,
        highlight: bool,
    },
    Image {
        highlight: bool,
    },
    File {
        highlight: bool,
    },
    Audio {
        highlight: bool,
    },
    Video {
        highlight: bool,
    },
    Unsupported {
        highlight: bool,
    },
    UnableToDecrypt,
    StateNotice {
        highlight: bool,
    },
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimelineMessageSendState {
    Pending,
    Sent,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TimelineImageState {
    Pending,
    Loaded {
        key: String,
        width: u32,
        height: u32,
    },
    Failed {
        message: String,
    },
}

struct TimelineState {
    source_rows: Vec<TimelineRow>,
    rendered_text: String,
    lines: Vec<String>,
    paint_lines: Vec<TimelinePaintLine>,
    selected_find_source_row_index: Option<usize>,
    top_line: usize,
    /// The `WM_SETFONT` font, borrowed from the frame's `TextFont`; null
    /// draws in the system font.
    font: HFONT,
    /// The height of a line in `font`.
    line_height: i32,
    vertical_padding: i32,
    /// The timestamp prefix sample that log rows' continuation lines are
    /// indented past; set with the rows, as the rows were stamped with it.
    indent_sample: String,
}

impl TimelineState {
    fn new(line_height: i32) -> Self {
        Self {
            source_rows: Vec::new(),
            rendered_text: String::new(),
            lines: Vec::new(),
            paint_lines: Vec::new(),
            selected_find_source_row_index: None,
            top_line: 0,
            font: null_mut(),
            line_height,
            vertical_padding: TIMELINE_VERTICAL_PADDING,
            indent_sample: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RoomHistoryBackfillStatus {
    Idle,
    Loading {
        from: Option<String>,
    },
    Exhausted,
    Failed {
        from: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RoomHistoryBackfillState {
    next_token: Option<String>,
    status: RoomHistoryBackfillStatus,
}

impl Default for RoomHistoryBackfillState {
    fn default() -> Self {
        Self {
            next_token: None,
            status: RoomHistoryBackfillStatus::Idle,
        }
    }
}

#[derive(Default)]
struct UiModel {
    status: String,
    /// The signed-in account, known from the login result or the session
    /// being restored.
    own_user_id: Option<String>,
    /// Kept across logouts, as mIRC keeps Status text across disconnects.
    status_log: StatusLog,
    lifecycle_state: LifecycleState,
    connection: ConnectionState,
    rooms: Vec<RoomSummary>,
    room_read_states: BTreeMap<String, RoomReadState>,
    selected_room: Option<String>,
    room_views: BTreeMap<String, RoomViewModel>,
    last_activity: Option<LastMessageActivity>,
    last_e2ee_status: Option<E2eeStatus>,
    last_e2ee_recovery_setup_result: bool,
    last_verification: Option<VerificationStateEvent>,
    last_error: Option<CoreErrorEvent>,
    /// The `/list` listing; the Rooms List window exists exactly while this
    /// is `Some`.
    room_list: Option<RoomListState>,
    /// The user's highlight words as the server last listed them; `None`
    /// until the first list after signing in, and after logout.
    highlight_words: Option<Vec<HighlightWord>>,
    /// The account's ignore list as the server last listed it, sorted;
    /// `None` until the first list after signing in, and after logout.
    ignored_users: Option<Vec<String>>,
}

#[derive(Default, Clone)]
struct RoomViewModel {
    topic: Option<String>,
    activities: Vec<RoomActivity>,
    reactions_by_target: BTreeMap<String, BTreeMap<String, RoomReactionEntry>>,
    threads: BTreeMap<String, ThreadViewModel>,
    preview_media: BTreeMap<String, RoomPreviewMediaState>,
    /// Messages replies answer that are not among `activities`, by event ID.
    reply_targets: BTreeMap<String, ReplyTargetState>,
    /// The messages among `activities` that were edited, shown `(edited)`.
    edited_event_ids: BTreeSet<String>,
    typing_user_ids: Vec<String>,
    members: Vec<RoomMemberSummary>,
    history: RoomHistoryBackfillState,
    echo: EchoLog,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimelineBackfillAnchor {
    room_id: String,
    source_row: TimelineRow,
    source_row_index: usize,
    source_row_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThreadViewModel {
    label: String,
}

/// A replied-to message fetched from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ReplyTargetState {
    Pending,
    Loaded(Box<RoomActivity>),
    Failed,
}

/// What a reply answers, as far as the window knows.
enum ReplyTarget<'a> {
    Known(&'a RoomActivity),
    Loading,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RoomPreviewMediaState {
    Pending {
        event_id: String,
        format: RoomMediaFormat,
    },
    Loaded {
        event_id: String,
        format: RoomMediaFormat,
        image_key: String,
        width: u32,
        height: u32,
    },
    Failed {
        event_id: String,
        format: RoomMediaFormat,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LastMessageActivity {
    room_id: String,
    event_id: Option<String>,
    transaction_id: Option<String>,
    sender: String,
    timestamp_ms: u64,
    presentation: RoomMessagePresentationKind,
    body: String,
    send_state: RoomMessageSendState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MessageReplacementFields {
    presentation: RoomMessagePresentationKind,
    body: String,
    styles: Vec<TextStyleSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RoomReactionEntry {
    reaction_event_id: String,
    sender: String,
    timestamp_ms: u64,
    key: String,
}

impl RoomViewModel {
    fn clear_visible_entries(&mut self) {
        self.activities.retain(|activity| {
            matches!(
                &activity.kind,
                RoomActivityKind::Message(RoomMessageActivity {
                    send_state: RoomMessageSendState::Pending,
                    ..
                })
            )
        });
        self.reactions_by_target.clear();
        // Only pending messages stay, and they have no event to be edited.
        self.edited_event_ids.clear();
        self.echo.clear_room_window();
        self.retain_preview_media_for_visible_activities();
        self.rebuild_threads();
    }

    fn apply_activity(&mut self, activity: RoomActivity) {
        match &activity.kind {
            RoomActivityKind::Message(message) => {
                let transaction_id = message.transaction_id.clone();
                let event_id = message.event_id.clone();
                match &message.send_state {
                    RoomMessageSendState::Pending => {
                        let transaction_id = transaction_id
                            .as_deref()
                            .expect("pending room activity requires a transaction id");
                        self.upsert_message_activity(activity, Some(transaction_id), None);
                    }
                    RoomMessageSendState::Sent => {
                        let event_id = event_id
                            .as_deref()
                            .expect("sent room activity requires an event id");
                        self.upsert_message_activity(
                            activity,
                            transaction_id.as_deref(),
                            Some(event_id),
                        );
                    }
                    RoomMessageSendState::Failed { .. } => {
                        self.upsert_message_activity(
                            activity,
                            transaction_id.as_deref(),
                            event_id.as_deref(),
                        );
                    }
                }
            }
            RoomActivityKind::Image(_)
            | RoomActivityKind::File(_)
            | RoomActivityKind::Audio(_)
            | RoomActivityKind::Video(_)
            | RoomActivityKind::UnsupportedMessage(_)
            | RoomActivityKind::UnableToDecrypt(_) => self.activities.push(activity),
            RoomActivityKind::StateNotice(notice) => {
                if let RoomStateNoticeContent::Topic { topic } = &notice.content {
                    self.topic = Some(topic.clone());
                }
                self.activities.push(activity);
            }
            RoomActivityKind::Redacted(redacted) => {
                self.remove_reactions_for_target(&redacted.event_id);
                self.activities.push(activity);
            }
        }
        self.rebuild_threads();
    }

    fn apply_history_backfill(&mut self, room_id: &str, backfill: RoomHistoryBackfill) {
        assert_eq!(
            backfill.room_id, room_id,
            "room history backfill must target the mutated room view"
        );

        let mut prepended_count = 0usize;
        let mut changed = false;
        for event in backfill.events {
            match event {
                RoomHistoryBackfillEvent::Activity(activity) => {
                    assert_eq!(
                        activity.room_id, room_id,
                        "room history activity must target the mutated room view"
                    );
                    if room_activity_event_id(&activity).is_some_and(|event_id| {
                        self.find_activity_index_by_event_id(event_id).is_some()
                    }) {
                        continue;
                    }

                    self.activities.insert(prepended_count, *activity);
                    prepended_count += 1;
                    changed = true;
                }
                RoomHistoryBackfillEvent::ActivityUpdate(update) => {
                    assert_eq!(
                        update.room_id, room_id,
                        "room history activity update must target the mutated room view"
                    );
                    if self.validate_update(&update).is_err() {
                        continue;
                    }

                    let applied = match (&update.target, &update.kind) {
                        (
                            RoomActivityUpdateTarget::Event { event_id },
                            RoomActivityUpdateKind::MessageReplaced {
                                presentation,
                                body,
                                styles,
                                ..
                            },
                        ) => self.replace_message(
                            event_id,
                            MessageReplacementFields {
                                presentation: *presentation,
                                body: body.clone(),
                                styles: styles.clone(),
                            },
                        ),
                        (
                            RoomActivityUpdateTarget::Event { event_id },
                            RoomActivityUpdateKind::ActivityRedacted { .. },
                        ) => self.redact_activity(event_id),
                        (
                            RoomActivityUpdateTarget::Event { event_id },
                            RoomActivityUpdateKind::ReactionAdded {
                                reaction_event_id,
                                sender,
                                timestamp_ms,
                                key,
                            },
                        ) => self.add_reaction(
                            event_id,
                            reaction_event_id,
                            sender,
                            *timestamp_ms,
                            key,
                        ),
                        (
                            RoomActivityUpdateTarget::Event { event_id },
                            RoomActivityUpdateKind::ReactionRemoved {
                                reaction_event_id,
                                key,
                                ..
                            },
                        ) => self.remove_reaction(event_id, reaction_event_id, key),
                        _ => false,
                    };
                    changed |= applied;
                }
            }
        }

        if changed {
            self.rebuild_threads();
            self.retain_preview_media_for_visible_activities();
        }
    }

    fn replace_typing(&mut self, typing: RoomTyping) {
        self.typing_user_ids = typing.user_ids;
    }

    fn replace_members(&mut self, members: RoomMembers) {
        self.members = members.members;
    }

    /// A user's nick from the joined-member snapshot; users outside it, such
    /// as typing notifications racing a membership change, show by user ID.
    fn member_nick(&self, user_id: &str) -> String {
        self.members
            .iter()
            .find(|member| member.user_id == user_id)
            .map_or_else(|| user_id.to_owned(), room_member_nick)
    }

    fn projected_preview_state(
        &self,
        activity_id: &str,
        event_id: &str,
        format: &RoomMediaFormat,
    ) -> TimelineImageState {
        match self.preview_media.get(activity_id) {
            Some(RoomPreviewMediaState::Pending {
                event_id: stored_event_id,
                format: stored_format,
            }) if stored_event_id == event_id && stored_format == format => {
                TimelineImageState::Pending
            }
            Some(RoomPreviewMediaState::Loaded {
                event_id: stored_event_id,
                format: stored_format,
                image_key,
                width,
                height,
            }) if stored_event_id == event_id && stored_format == format => {
                TimelineImageState::Loaded {
                    key: image_key.clone(),
                    width: *width,
                    height: *height,
                }
            }
            Some(RoomPreviewMediaState::Failed {
                event_id: stored_event_id,
                format: stored_format,
                message,
            }) if stored_event_id == event_id && stored_format == format => {
                TimelineImageState::Failed {
                    message: message.clone(),
                }
            }
            _ => TimelineImageState::Pending,
        }
    }

    fn should_request_preview_media(
        &self,
        activity_id: &str,
        event_id: &str,
        format: &RoomMediaFormat,
    ) -> bool {
        match self.preview_media.get(activity_id) {
            Some(RoomPreviewMediaState::Pending {
                event_id: stored_event_id,
                format: stored_format,
            })
            | Some(RoomPreviewMediaState::Loaded {
                event_id: stored_event_id,
                format: stored_format,
                ..
            })
            | Some(RoomPreviewMediaState::Failed {
                event_id: stored_event_id,
                format: stored_format,
                ..
            }) => stored_event_id != event_id || stored_format != format,
            None => true,
        }
    }

    fn mark_preview_media_pending(
        &mut self,
        activity_id: String,
        event_id: String,
        format: RoomMediaFormat,
    ) -> Option<RoomPreviewMediaState> {
        self.preview_media.insert(
            activity_id,
            RoomPreviewMediaState::Pending { event_id, format },
        )
    }

    fn remove_preview_media(&mut self, activity_id: &str) -> Option<RoomPreviewMediaState> {
        self.preview_media.remove(activity_id)
    }

    fn insert_preview_media(
        &mut self,
        activity_id: String,
        preview_media: RoomPreviewMediaState,
    ) -> Option<RoomPreviewMediaState> {
        self.preview_media.insert(activity_id, preview_media)
    }

    fn upsert_message_activity(
        &mut self,
        activity: RoomActivity,
        transaction_id: Option<&str>,
        event_id: Option<&str>,
    ) {
        if let Some(index) = self.find_message_activity_index(transaction_id, event_id) {
            self.activities[index] = activity;
        } else {
            self.activities.push(activity);
        }
    }

    fn mark_message_sent(&mut self, update: RoomActivityUpdate) {
        let RoomActivityUpdate { target, kind, .. } = update;
        let RoomActivityUpdateTarget::Transaction { transaction_id } = target else {
            panic!("message sent update requires a transaction target");
        };
        let RoomActivityUpdateKind::MessageSent { event_id } = kind else {
            panic!("message sent update requires a sent activity update");
        };
        let transaction_index = self
            .find_pending_message_activity_index(transaction_id.as_str())
            .expect("outgoing sent requires an existing queued timeline row");
        let duplicate_event_index = self
            .find_message_activity_index(None, Some(event_id.as_str()))
            .filter(|event_index| *event_index != transaction_index);
        let duplicate_event =
            duplicate_event_index.map(|event_index| self.activities[event_index].clone());
        let duplicate_event = duplicate_event.and_then(|activity| {
            let RoomActivityKind::Message(message) = activity.kind else {
                return None;
            };
            Some((
                activity.activity_id,
                activity.timestamp_ms,
                activity.relation,
                message.sender,
                message.presentation,
                message.body,
            ))
        });
        let new_activity_id = duplicate_event
            .as_ref()
            .map(|(activity_id, _, _, _, _, _)| activity_id.clone())
            .unwrap_or_else(|| format!("event:{event_id}"));
        let synced_timestamp_ms = duplicate_event
            .as_ref()
            .map(|(_, timestamp_ms, _, _, _, _)| *timestamp_ms);
        let synced_relation = duplicate_event
            .as_ref()
            .and_then(|(_, _, relation, _, _, _)| relation.clone());
        let synced_sender = duplicate_event
            .as_ref()
            .map(|(_, _, _, sender, _, _)| sender.clone());
        let synced_presentation = duplicate_event
            .as_ref()
            .map(|(_, _, _, _, presentation, _)| *presentation);
        let synced_body = duplicate_event
            .as_ref()
            .map(|(_, _, _, _, _, body)| body.clone());

        let activity = self
            .activities
            .get_mut(transaction_index)
            .expect("transaction row index must resolve");
        activity.activity_id = new_activity_id;
        if let Some(synced_timestamp_ms) = synced_timestamp_ms {
            activity.timestamp_ms = synced_timestamp_ms;
        }
        if let Some(synced_relation) = synced_relation {
            activity.relation = Some(synced_relation);
        }
        let RoomActivityKind::Message(message) = &mut activity.kind else {
            panic!("outgoing sent requires a message room activity");
        };

        message.event_id = Some(event_id.clone());
        if let Some(synced_sender) = synced_sender {
            message.sender = synced_sender;
        }
        if let Some(synced_presentation) = synced_presentation {
            message.presentation = synced_presentation;
        }
        if let Some(synced_body) = synced_body {
            message.body = synced_body;
        }
        message.send_state = RoomMessageSendState::Sent;

        if let Some(duplicate_event_index) = duplicate_event_index {
            self.activities.remove(duplicate_event_index);
        }
        self.rebuild_threads();
    }

    fn mark_message_failed(&mut self, update: RoomActivityUpdate) {
        let RoomActivityUpdate { target, kind, .. } = update;
        let RoomActivityUpdateTarget::Transaction { transaction_id } = target else {
            panic!("message failed update requires a transaction target");
        };
        let RoomActivityUpdateKind::MessageFailed {
            message,
            recoverable,
        } = kind
        else {
            panic!("message failed update requires a failed activity update");
        };
        let activity = self
            .find_pending_message_activity_mut(transaction_id.as_str())
            .expect("outgoing failed requires an existing queued timeline row");
        let RoomActivityKind::Message(room_message) = &mut activity.kind else {
            panic!("outgoing failed requires a message room activity");
        };

        room_message.send_state = RoomMessageSendState::Failed {
            message,
            recoverable,
        };
        self.rebuild_threads();
    }

    fn replace_message(
        &mut self,
        target_event_id: &str,
        replacement: MessageReplacementFields,
    ) -> bool {
        let Some(activity) = self.activities.iter_mut().find(|activity| {
            matches!(
                &activity.kind,
                RoomActivityKind::Message(RoomMessageActivity {
                    event_id: Some(event_id),
                    ..
                }) if event_id == target_event_id
            )
        }) else {
            return false;
        };
        let RoomActivityKind::Message(message) = &mut activity.kind else {
            panic!("message replacement requires a message room activity");
        };
        message.presentation = replacement.presentation;
        message.body = replacement.body;
        message.styles = replacement.styles;
        self.edited_event_ids.insert(target_event_id.to_owned());
        self.rebuild_threads();
        true
    }

    fn redact_activity(&mut self, target_event_id: &str) -> bool {
        let Some(index) = self.find_activity_index_by_event_id(target_event_id) else {
            return false;
        };
        let activity_id = self.activities[index].activity_id.clone();
        self.remove_reactions_for_target(target_event_id);
        // A redacted line says only that; what it was, edited or not, is gone.
        self.edited_event_ids.remove(target_event_id);
        self.activities[index].kind = RoomActivityKind::Redacted(RoomRedactedActivity {
            event_id: target_event_id.to_owned(),
        });
        self.remove_preview_media(&activity_id);
        self.rebuild_threads();
        true
    }

    fn add_reaction(
        &mut self,
        target_event_id: &str,
        reaction_event_id: &str,
        sender: &str,
        timestamp_ms: u64,
        key: &str,
    ) -> bool {
        let Some(index) = self.find_activity_index_by_event_id(target_event_id) else {
            return false;
        };
        if matches!(self.activities[index].kind, RoomActivityKind::Redacted(_)) {
            return false;
        }
        self.reactions_by_target
            .entry(target_event_id.to_owned())
            .or_default()
            .insert(
                reaction_event_id.to_owned(),
                RoomReactionEntry {
                    reaction_event_id: reaction_event_id.to_owned(),
                    sender: sender.to_owned(),
                    timestamp_ms,
                    key: key.to_owned(),
                },
            );
        true
    }

    fn remove_reaction(
        &mut self,
        target_event_id: &str,
        reaction_event_id: &str,
        key: &str,
    ) -> bool {
        let Some(reactions) = self.reactions_by_target.get_mut(target_event_id) else {
            return false;
        };
        let matches_key = reactions
            .get(reaction_event_id)
            .is_some_and(|entry| entry.key == key);
        if !matches_key {
            return false;
        }
        reactions.remove(reaction_event_id);
        if reactions.is_empty() {
            self.reactions_by_target.remove(target_event_id);
        }
        true
    }

    fn remove_reactions_for_target(&mut self, target_event_id: &str) {
        self.reactions_by_target.remove(target_event_id);
    }

    fn reaction_summaries_for_event(&self, target_event_id: &str) -> Vec<DumpRoomReactionSummary> {
        let Some(reactions) = self.reactions_by_target.get(target_event_id) else {
            return Vec::new();
        };

        // In reaction time, so the first reaction stays first as more
        // arrive: reactors within a group, and groups by their first.
        let mut entries = reactions.values().collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            (left.timestamp_ms, &left.reaction_event_id)
                .cmp(&(right.timestamp_ms, &right.reaction_event_id))
        });
        let mut grouped = Vec::<(String, Vec<&RoomReactionEntry>)>::new();
        for entry in entries {
            match grouped.iter_mut().find(|(key, _)| *key == entry.key) {
                Some((_, group)) => group.push(entry),
                None => grouped.push((entry.key.clone(), vec![entry])),
            }
        }

        grouped
            .into_iter()
            .map(|(key, entries)| DumpRoomReactionSummary {
                key,
                count: entries.len() as u64,
                senders: entries.iter().map(|entry| entry.sender.clone()).collect(),
                reaction_event_ids: entries
                    .iter()
                    .map(|entry| entry.reaction_event_id.clone())
                    .collect(),
            })
            .collect()
    }

    fn find_message_activity_index(
        &self,
        transaction_id: Option<&str>,
        event_id: Option<&str>,
    ) -> Option<usize> {
        if let Some(transaction_id) = transaction_id
            && let Some(index) = self.activities.iter().position(|activity| {
                matches!(
                    &activity.kind,
                    RoomActivityKind::Message(RoomMessageActivity {
                        transaction_id: Some(existing_transaction_id),
                        ..
                    }) if existing_transaction_id == transaction_id
                )
            })
        {
            return Some(index);
        }

        if let Some(event_id) = event_id
            && let Some(index) = self.activities.iter().position(|activity| {
                matches!(
                    &activity.kind,
                    RoomActivityKind::Message(RoomMessageActivity {
                        event_id: Some(existing_event_id),
                        ..
                    }) if existing_event_id == event_id
                )
            })
        {
            return Some(index);
        }

        None
    }

    /// A loaded message wins over a fetched one: it is live, with its
    /// edits and redaction.
    fn reply_target(&self, event_id: &str) -> ReplyTarget<'_> {
        if let Some(index) = self.find_activity_index_by_event_id(event_id) {
            return ReplyTarget::Known(&self.activities[index]);
        }
        match self.reply_targets.get(event_id) {
            Some(ReplyTargetState::Loaded(activity)) => ReplyTarget::Known(activity),
            Some(ReplyTargetState::Failed) => ReplyTarget::Unavailable,
            Some(ReplyTargetState::Pending) | None => ReplyTarget::Loading,
        }
    }

    fn find_activity_index_by_event_id(&self, event_id: &str) -> Option<usize> {
        self.activities
            .iter()
            .position(|activity| room_activity_event_id(activity) == Some(event_id))
    }

    fn find_pending_message_activity_index(&self, transaction_id: &str) -> Option<usize> {
        self.activities.iter().position(|activity| {
            matches!(
                &activity.kind,
                RoomActivityKind::Message(RoomMessageActivity {
                    transaction_id: Some(existing_transaction_id),
                    send_state: RoomMessageSendState::Pending,
                    ..
                }) if existing_transaction_id == transaction_id
            )
        })
    }

    fn find_pending_message_activity_mut(
        &mut self,
        transaction_id: &str,
    ) -> Option<&mut RoomActivity> {
        let index = self.find_pending_message_activity_index(transaction_id)?;
        self.activities.get_mut(index)
    }

    fn rebuild_threads(&mut self) {
        let root_labels = self
            .activities
            .iter()
            .filter_map(|activity| {
                Some((
                    room_activity_event_id(activity)?.to_owned(),
                    room_activity_thread_root_label(activity)?,
                ))
            })
            .collect::<BTreeMap<_, _>>();

        self.threads = self
            .activities
            .iter()
            .filter_map(|activity| room_activity_thread_root_event_id(activity))
            .map(|thread_root_event_id| {
                let label = root_labels
                    .get(thread_root_event_id)
                    .cloned()
                    .unwrap_or_else(|| thread_root_event_id.to_owned());
                (thread_root_event_id.to_owned(), ThreadViewModel { label })
            })
            .collect();
    }

    fn validate_update(&self, update: &RoomActivityUpdate) -> Result<(), String> {
        match (&update.target, &update.kind) {
            (
                RoomActivityUpdateTarget::Transaction { transaction_id },
                RoomActivityUpdateKind::MessageSent { .. },
            ) => self
                .find_pending_message_activity_index(transaction_id)
                .map(|_| ())
                .ok_or_else(|| "outgoing sent requires an existing queued timeline row".to_owned()),
            (
                RoomActivityUpdateTarget::Transaction { transaction_id },
                RoomActivityUpdateKind::MessageFailed { .. },
            ) => self
                .find_pending_message_activity_index(transaction_id)
                .map(|_| ())
                .ok_or_else(|| {
                    "outgoing failed requires an existing queued timeline row".to_owned()
                }),
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::MessageReplaced { .. },
            ) => self
                .find_message_activity_index(None, Some(event_id))
                .map(|_| ())
                .ok_or_else(|| "message edit requires an existing visible target event".to_owned()),
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ActivityRedacted { .. },
            ) => self
                .find_activity_index_by_event_id(event_id)
                .map(|_| ())
                .ok_or_else(|| {
                    "activity redaction requires an existing visible target event".to_owned()
                }),
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ReactionAdded { .. },
            ) => {
                let Some(index) = self.find_activity_index_by_event_id(event_id) else {
                    return Err("reaction add requires an existing visible target event".to_owned());
                };
                if matches!(self.activities[index].kind, RoomActivityKind::Redacted(_)) {
                    return Err("reaction add requires an existing visible target event".to_owned());
                }
                Ok(())
            }
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ReactionRemoved {
                    reaction_event_id,
                    key,
                    ..
                },
            ) => self
                .reactions_by_target
                .get(event_id)
                .and_then(|reactions| reactions.get(reaction_event_id))
                .filter(|entry| entry.key == *key)
                .map(|_| ())
                .ok_or_else(|| {
                    "reaction removal requires a matching stored reaction event".to_owned()
                }),
            (
                RoomActivityUpdateTarget::Event { .. },
                RoomActivityUpdateKind::MessageSent { .. },
            ) => Err("message sent update requires a transaction target".to_owned()),
            (
                RoomActivityUpdateTarget::Event { .. },
                RoomActivityUpdateKind::MessageFailed { .. },
            ) => Err("message failed update requires a transaction target".to_owned()),
            (
                RoomActivityUpdateTarget::Transaction { .. },
                RoomActivityUpdateKind::MessageReplaced { .. },
            ) => Err("message edit requires an event target".to_owned()),
            (
                RoomActivityUpdateTarget::Transaction { .. },
                RoomActivityUpdateKind::ActivityRedacted { .. },
            ) => Err("activity redacted update requires an event target".to_owned()),
            (
                RoomActivityUpdateTarget::Transaction { .. },
                RoomActivityUpdateKind::ReactionAdded { .. },
            ) => Err("reaction add requires an event target".to_owned()),
            (
                RoomActivityUpdateTarget::Transaction { .. },
                RoomActivityUpdateKind::ReactionRemoved { .. },
            ) => Err("reaction removal requires an event target".to_owned()),
        }
    }

    fn retain_preview_media_for_visible_activities(&mut self) {
        self.preview_media.retain(|activity_id, _| {
            self.activities
                .iter()
                .any(|activity| activity.activity_id == *activity_id)
        });
    }
}

fn validate_synthetic_room_activity_events(
    room_views: &BTreeMap<String, RoomViewModel>,
    events: &[Event],
) -> Result<(), String> {
    let mut room_views = room_views.clone();

    for event in events {
        match event {
            Event::RoomActivity(activity) => room_views
                .entry(activity.room_id.clone())
                .or_default()
                .apply_activity(activity.as_ref().clone()),
            Event::RoomActivityUpdate(update) => {
                let room_view = room_views.entry(update.room_id.clone()).or_default();
                room_view.validate_update(update)?;
                match (&update.target, &update.kind) {
                    (
                        RoomActivityUpdateTarget::Transaction { .. },
                        RoomActivityUpdateKind::MessageSent { .. },
                    ) => room_view.mark_message_sent(update.clone()),
                    (
                        RoomActivityUpdateTarget::Transaction { .. },
                        RoomActivityUpdateKind::MessageFailed { .. },
                    ) => room_view.mark_message_failed(update.clone()),
                    (
                        RoomActivityUpdateTarget::Event { event_id },
                        RoomActivityUpdateKind::MessageReplaced {
                            presentation,
                            body,
                            styles,
                            ..
                        },
                    ) => {
                        let replaced = room_view.replace_message(
                            event_id,
                            MessageReplacementFields {
                                presentation: *presentation,
                                body: body.clone(),
                                styles: styles.clone(),
                            },
                        );
                        assert!(replaced, "validated synthetic message edit must resolve");
                    }
                    (
                        RoomActivityUpdateTarget::Transaction { .. },
                        RoomActivityUpdateKind::MessageReplaced { .. },
                    ) => panic!("message edit requires an event target"),
                    (
                        RoomActivityUpdateTarget::Event { event_id },
                        RoomActivityUpdateKind::ActivityRedacted { .. },
                    ) => {
                        let redacted = room_view.redact_activity(event_id);
                        assert!(
                            redacted,
                            "validated synthetic activity redaction must resolve"
                        );
                    }
                    (
                        RoomActivityUpdateTarget::Event { event_id },
                        RoomActivityUpdateKind::ReactionAdded {
                            reaction_event_id,
                            sender,
                            timestamp_ms,
                            key,
                        },
                    ) => {
                        let added = room_view.add_reaction(
                            event_id,
                            reaction_event_id,
                            sender,
                            *timestamp_ms,
                            key,
                        );
                        assert!(added, "validated synthetic reaction add must resolve");
                    }
                    (
                        RoomActivityUpdateTarget::Event { event_id },
                        RoomActivityUpdateKind::ReactionRemoved {
                            reaction_event_id,
                            key,
                            ..
                        },
                    ) => {
                        let removed = room_view.remove_reaction(event_id, reaction_event_id, key);
                        assert!(removed, "validated synthetic reaction removal must resolve");
                    }
                    (
                        RoomActivityUpdateTarget::Event { .. },
                        RoomActivityUpdateKind::MessageSent { .. },
                    ) => panic!("message sent update requires a transaction target"),
                    (
                        RoomActivityUpdateTarget::Event { .. },
                        RoomActivityUpdateKind::MessageFailed { .. },
                    ) => panic!("message failed update requires a transaction target"),
                    (
                        RoomActivityUpdateTarget::Transaction { .. },
                        RoomActivityUpdateKind::ActivityRedacted { .. },
                    ) => panic!("activity redacted update requires an event target"),
                    (
                        RoomActivityUpdateTarget::Transaction { .. },
                        RoomActivityUpdateKind::ReactionAdded { .. },
                    ) => panic!("reaction add requires an event target"),
                    (
                        RoomActivityUpdateTarget::Transaction { .. },
                        RoomActivityUpdateKind::ReactionRemoved { .. },
                    ) => panic!("reaction removal requires an event target"),
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn last_activity_from_room_activity(activity: &RoomActivity) -> Option<LastMessageActivity> {
    match &activity.kind {
        RoomActivityKind::Message(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: message.event_id.clone(),
            transaction_id: message.transaction_id.clone(),
            sender: message.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: message.presentation,
            body: message.body.clone(),
            send_state: message.send_state.clone(),
        }),
        RoomActivityKind::UnsupportedMessage(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: format!("[unsupported {}] {}", message.msgtype, message.body),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::UnableToDecrypt(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: "[encrypted] Unable to decrypt message".to_owned(),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::Image(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: format!(
                "[image] {}",
                message.filename.as_deref().unwrap_or(message.body.as_str())
            ),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::File(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: format!(
                "[file] {}",
                message.filename.as_deref().unwrap_or(message.body.as_str())
            ),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::Audio(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: format!(
                "[audio] {}",
                message.filename.as_deref().unwrap_or(message.body.as_str())
            ),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::Video(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.user_id.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: format!(
                "[video] {}",
                message.filename.as_deref().unwrap_or(message.body.as_str())
            ),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::StateNotice(_) => None,
        RoomActivityKind::Redacted(_) => None,
    }
}

/// Who a line comes from. A redaction placeholder has no sender: the core
/// does not report who removed the event.
fn room_activity_sender_user_id(activity: &RoomActivity) -> Option<&str> {
    match &activity.kind {
        RoomActivityKind::Message(message) => Some(&message.sender.user_id),
        RoomActivityKind::Image(message) => Some(&message.sender.user_id),
        RoomActivityKind::File(message) => Some(&message.sender.user_id),
        RoomActivityKind::Audio(message) => Some(&message.sender.user_id),
        RoomActivityKind::Video(message) => Some(&message.sender.user_id),
        RoomActivityKind::UnsupportedMessage(message) => Some(&message.sender.user_id),
        RoomActivityKind::UnableToDecrypt(message) => Some(&message.sender.user_id),
        RoomActivityKind::StateNotice(notice) => Some(&notice.sender.user_id),
        RoomActivityKind::Redacted(RoomRedactedActivity { event_id: _ }) => None,
    }
}

/// The activity a line marks its view with while the user is elsewhere.
/// `None` for our own lines, as in mIRC: our pending echo, its sync echo
/// and what we sent from another device. Without a signed-in user
/// (`own_user_id` is `None`) no line is ours. The login flow delivers the
/// initial sync's lines before `LoggedIn`, but their rooms are not in the
/// room list yet, so they colour nothing either.
fn activity_level_for(
    activity: &RoomActivity,
    own_user_id: Option<&str>,
) -> Option<ViewActivityLevel> {
    if own_user_id.is_some_and(|me| room_activity_sender_user_id(activity) == Some(me)) {
        return None;
    }
    let level = match &activity.kind {
        RoomActivityKind::Message(_)
        | RoomActivityKind::Image(_)
        | RoomActivityKind::File(_)
        | RoomActivityKind::Audio(_)
        | RoomActivityKind::Video(_)
        | RoomActivityKind::UnsupportedMessage(_)
        | RoomActivityKind::UnableToDecrypt(_) => ViewActivityLevel::Message,
        RoomActivityKind::StateNotice(_) | RoomActivityKind::Redacted(_) => {
            ViewActivityLevel::Event
        }
    };
    Some(if activity.highlight {
        ViewActivityLevel::Highlight
    } else {
        level
    })
}

/// Records activity in a view, keeping the higher level: mIRC never lowers
/// a window's colour until the user looks at it.
fn raise_view_activity(state: &mut WindowState, view_id: ViewId, level: ViewActivityLevel) {
    let recorded = state.room_activity.entry(view_id).or_insert(level);
    *recorded = (*recorded).max(level);
}

fn room_activity_event_id(activity: &RoomActivity) -> Option<&str> {
    match &activity.kind {
        RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some(event_id),
            ..
        }) => Some(event_id.as_str()),
        RoomActivityKind::Image(message) => Some(message.event_id.as_str()),
        RoomActivityKind::File(message) => Some(message.event_id.as_str()),
        RoomActivityKind::Audio(message) => Some(message.event_id.as_str()),
        RoomActivityKind::Video(message) => Some(message.event_id.as_str()),
        RoomActivityKind::UnsupportedMessage(message) => Some(message.event_id.as_str()),
        RoomActivityKind::UnableToDecrypt(message) => Some(message.event_id.as_str()),
        RoomActivityKind::StateNotice(notice) => Some(notice.event_id.as_str()),
        RoomActivityKind::Redacted(activity) => Some(activity.event_id.as_str()),
        _ => None,
    }
}

fn room_activity_thread_root_event_id(activity: &RoomActivity) -> Option<&str> {
    match &activity.relation {
        Some(RoomActivityRelation::Thread {
            thread_root_event_id,
            ..
        }) => Some(thread_root_event_id.as_str()),
        _ => None,
    }
}

fn room_activity_thread_root_label(activity: &RoomActivity) -> Option<String> {
    match &activity.kind {
        RoomActivityKind::Message(message) => Some(message.body.clone()),
        RoomActivityKind::Image(message) => Some(
            message
                .filename
                .as_deref()
                .unwrap_or(message.body.as_str())
                .to_owned(),
        ),
        RoomActivityKind::File(message) => Some(
            message
                .filename
                .as_deref()
                .unwrap_or(message.body.as_str())
                .to_owned(),
        ),
        RoomActivityKind::Audio(message) => Some(
            message
                .filename
                .as_deref()
                .unwrap_or(message.body.as_str())
                .to_owned(),
        ),
        RoomActivityKind::Video(message) => Some(
            message
                .filename
                .as_deref()
                .unwrap_or(message.body.as_str())
                .to_owned(),
        ),
        RoomActivityKind::UnsupportedMessage(message) => Some(message.body.clone()),
        RoomActivityKind::UnableToDecrypt(_) => Some("Unable to decrypt message".to_owned()),
        RoomActivityKind::StateNotice(_) => None,
        RoomActivityKind::Redacted(_) => Some("Message redacted".to_owned()),
    }
}

fn room_activity_belongs_to_thread(activity: &RoomActivity, thread_root_event_id: &str) -> bool {
    room_activity_event_id(activity) == Some(thread_root_event_id)
        || room_activity_thread_root_event_id(activity) == Some(thread_root_event_id)
}

fn room_activity_view_id(room_id: &str, relation: Option<&RoomActivityRelation>) -> ViewId {
    match relation {
        Some(RoomActivityRelation::Thread {
            thread_root_event_id,
            ..
        }) => ViewId::Thread {
            room_id: room_id.to_owned(),
            thread_root_event_id: thread_root_event_id.clone(),
        },
        _ => ViewId::Room(room_id.to_owned()),
    }
}

fn room_activity_target_view_id(activity: &RoomActivity) -> ViewId {
    room_activity_view_id(&activity.room_id, activity.relation.as_ref())
}

fn room_activity_update_marks_inactive_room(update: &RoomActivityUpdate) -> bool {
    matches!(
        (&update.target, &update.kind),
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::MessageFailed { .. }
        )
    )
}

fn failed_room_activity_update_view_id(state: &WindowState, update: &RoomActivityUpdate) -> ViewId {
    let (
        RoomActivityUpdateTarget::Transaction { transaction_id },
        RoomActivityUpdateKind::MessageFailed { .. },
    ) = (&update.target, &update.kind)
    else {
        panic!("failed room activity marker target requires a failed activity update");
    };
    let room_view = state
        .model
        .room_views
        .get(&update.room_id)
        .expect("outgoing failed requires an existing queued timeline row");
    let pending_index = room_view
        .find_pending_message_activity_index(transaction_id)
        .expect("outgoing failed requires an existing queued timeline row");
    let pending_activity = room_view
        .activities
        .get(pending_index)
        .expect("pending room activity index must resolve");
    room_activity_target_view_id(pending_activity)
}

fn read_target_for_room_activity(activity: &RoomActivity) -> Option<&str> {
    match &activity.kind {
        RoomActivityKind::Message(RoomMessageActivity { send_state, .. })
            if !matches!(send_state, RoomMessageSendState::Pending) =>
        {
            room_activity_event_id(activity)
        }
        RoomActivityKind::Image(_)
        | RoomActivityKind::File(_)
        | RoomActivityKind::Audio(_)
        | RoomActivityKind::Video(_)
        | RoomActivityKind::UnsupportedMessage(_)
        | RoomActivityKind::UnableToDecrypt(_)
        | RoomActivityKind::StateNotice(_)
        | RoomActivityKind::Redacted(_) => room_activity_event_id(activity),
        _ => None,
    }
}

fn apply_room_event_fetch(state: &mut WindowState, fetch: RoomEventFetch) {
    // The room was left while the fetch ran: no window shows its replies.
    let Some(room_view) = state.model.room_views.get_mut(&fetch.room_id) else {
        return;
    };
    let target = match fetch.result {
        RoomEventFetchResult::Loaded { activity } => ReplyTargetState::Loaded(activity),
        RoomEventFetchResult::Failed { .. } => ReplyTargetState::Failed,
    };
    room_view.reply_targets.insert(fetch.event_id, target);
}

/// Returns whether the activity should flash the frame. The active view gets
/// no activity colour, but it can still want attention while mIRC is in the
/// background.
fn apply_room_activity_event(state: &mut WindowState, activity: RoomActivity) -> bool {
    let clears_last_activity = matches!(activity.kind, RoomActivityKind::Redacted(_));
    let target_view_id = room_activity_target_view_id(&activity);
    let mut wants_attention = false;
    if let Some(level) = activity_level_for(&activity, state.model.own_user_id.as_deref())
        && let Some((room_is_direct, mode)) =
            sendable_room_by_id(&state.model.rooms, &activity.room_id)
                .map(|room| (room.is_direct, joined_room_notification_mode(room)))
    {
        wants_attention = !state.synthetic_load_in_progress
            && notifications::activity_wants_attention(level, room_is_direct, mode);
        // A closed query pops up again when the other side talks, as in
        // mIRC, muted or not; the next view sync recreates its window.
        if level >= ViewActivityLevel::Message
            && let ViewId::Room(room_id) = &target_view_id
        {
            state.closed_queries.remove(room_id);
        }
        if state.active_view != target_view_id {
            // The room's mode caps its threads' activity too.
            let level = notifications::room_activity_level(level, mode);
            raise_view_activity(state, target_view_id.clone(), level);
            if !state.synthetic_load_in_progress
                && let Some(event_id) = read_target_for_room_activity(&activity)
            {
                state
                    .room_activity_read_targets
                    .insert(target_view_id, event_id.to_owned());
            }
        }
    }
    let room_id = activity.room_id.clone();
    let last_activity = last_activity_from_room_activity(&activity);
    state
        .model
        .room_views
        .entry(room_id)
        .or_default()
        .apply_activity(activity);
    if let Some(last_activity) = last_activity {
        state.model.last_activity = Some(last_activity);
    } else if clears_last_activity {
        state.model.last_activity = None;
    }
    wants_attention
}

fn apply_room_activity_update_event(state: &mut WindowState, update: RoomActivityUpdate) {
    let target_view_id = room_activity_update_marks_inactive_room(&update)
        .then(|| failed_room_activity_update_view_id(state, &update));
    if let Some(target_view_id) = target_view_id
        && state.active_view != target_view_id
        && sendable_room_by_id(&state.model.rooms, &update.room_id).is_some()
    {
        raise_view_activity(state, target_view_id, ViewActivityLevel::Message);
    }
    let room_id = update.room_id.clone();
    let refresh_last_activity = state
        .model
        .last_activity
        .as_ref()
        .is_some_and(|last_activity| match (&update.target, &update.kind) {
            (
                RoomActivityUpdateTarget::Transaction { transaction_id },
                RoomActivityUpdateKind::MessageSent { event_id },
            ) => {
                last_activity.room_id == room_id
                    && (last_activity.transaction_id.as_deref() == Some(transaction_id.as_str())
                        || last_activity.event_id.as_deref() == Some(event_id.as_str()))
            }
            (
                RoomActivityUpdateTarget::Transaction { transaction_id },
                RoomActivityUpdateKind::MessageFailed { .. },
            ) => {
                last_activity.room_id == room_id
                    && last_activity.transaction_id.as_deref() == Some(transaction_id.as_str())
            }
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::MessageReplaced { .. },
            ) => {
                last_activity.room_id == room_id
                    && last_activity.event_id.as_deref() == Some(event_id.as_str())
            }
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ActivityRedacted { .. },
            ) => {
                last_activity.room_id == room_id
                    && last_activity.event_id.as_deref() == Some(event_id.as_str())
            }
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ReactionAdded { .. },
            )
            | (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ReactionRemoved { .. },
            ) => {
                last_activity.room_id == room_id
                    && last_activity.event_id.as_deref() == Some(event_id.as_str())
            }
            _ => false,
        });
    match (&update.target, &update.kind) {
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::MessageSent { .. },
        ) => state
            .model
            .room_views
            .entry(room_id.clone())
            .or_default()
            .mark_message_sent(update.clone()),
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::MessageFailed { .. },
        ) => state
            .model
            .room_views
            .entry(room_id.clone())
            .or_default()
            .mark_message_failed(update.clone()),
        (
            RoomActivityUpdateTarget::Event { event_id },
            RoomActivityUpdateKind::MessageReplaced {
                presentation,
                body,
                styles,
                ..
            },
        ) => {
            let _ = state
                .model
                .room_views
                .entry(room_id.clone())
                .or_default()
                .replace_message(
                    event_id,
                    MessageReplacementFields {
                        presentation: *presentation,
                        body: body.clone(),
                        styles: styles.clone(),
                    },
                );
        }
        (
            RoomActivityUpdateTarget::Event { event_id },
            RoomActivityUpdateKind::ActivityRedacted { .. },
        ) => {
            let redacted = state
                .model
                .room_views
                .entry(room_id.clone())
                .or_default()
                .redact_activity(event_id);
            if redacted {
                prune_decoded_images(state);
            }
        }
        (
            RoomActivityUpdateTarget::Event { event_id },
            RoomActivityUpdateKind::ReactionAdded {
                reaction_event_id,
                sender,
                timestamp_ms,
                key,
            },
        ) => {
            let _ = state
                .model
                .room_views
                .entry(room_id.clone())
                .or_default()
                .add_reaction(event_id, reaction_event_id, sender, *timestamp_ms, key);
        }
        (
            RoomActivityUpdateTarget::Event { event_id },
            RoomActivityUpdateKind::ReactionRemoved {
                reaction_event_id,
                key,
                ..
            },
        ) => {
            let _ = state
                .model
                .room_views
                .entry(room_id.clone())
                .or_default()
                .remove_reaction(event_id, reaction_event_id, key);
        }
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::MessageReplaced { .. },
        ) => panic!("message edit requires an event target"),
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::ActivityRedacted { .. },
        ) => panic!("activity redacted update requires an event target"),
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::ReactionAdded { .. },
        ) => panic!("reaction add requires an event target"),
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::ReactionRemoved { .. },
        ) => panic!("reaction removal requires an event target"),
        (RoomActivityUpdateTarget::Event { .. }, RoomActivityUpdateKind::MessageSent { .. }) => {
            panic!("message sent update requires a transaction target")
        }
        (RoomActivityUpdateTarget::Event { .. }, RoomActivityUpdateKind::MessageFailed { .. }) => {
            panic!("message failed update requires a transaction target")
        }
    }
    if refresh_last_activity
        && let Some(updated_activity) =
            state
                .model
                .room_views
                .get(&room_id)
                .and_then(|room_view| match &update.target {
                    RoomActivityUpdateTarget::Transaction { transaction_id } => room_view
                        .find_message_activity_index(Some(transaction_id.as_str()), None)
                        .and_then(|index| room_view.activities.get(index)),
                    RoomActivityUpdateTarget::Event { event_id } => room_view
                        .find_activity_index_by_event_id(event_id.as_str())
                        .and_then(|index| room_view.activities.get(index)),
                })
    {
        state.model.last_activity = last_activity_from_room_activity(updated_activity);
    }
}

fn apply_room_history_backfill_event(state: &mut WindowState, backfill: RoomHistoryBackfill) {
    let room_id = backfill.room_id.clone();
    let end = backfill.end.clone();
    let room_view = state.model.room_views.entry(room_id.clone()).or_default();
    room_view.apply_history_backfill(&room_id, backfill);
    room_view.history.next_token = end.clone();
    room_view.history.status = match end {
        Some(_) => RoomHistoryBackfillStatus::Idle,
        None => RoomHistoryBackfillStatus::Exhausted,
    };
    prune_decoded_images(state);
}

/// The room's history load failed. An error for a room that is not loading
/// (it was left, or the user cleared it meanwhile) changes nothing.
fn apply_backfill_room_history_error(state: &mut WindowState, room_id: &str, message: String) {
    let Some(room_view) = state.model.room_views.get_mut(room_id) else {
        return;
    };
    let RoomHistoryBackfillStatus::Loading { from } = &room_view.history.status else {
        return;
    };
    let from = from.clone();
    room_view.history.status = RoomHistoryBackfillStatus::Failed {
        from: from.clone(),
        message,
    };
}

fn apply_room_read_state_event(state: &mut WindowState, read_state: RoomReadState) {
    let room_id = read_state.room_id.clone();
    let Some(mode) =
        sendable_room_by_id(&state.model.rooms, &room_id).map(joined_room_notification_mode)
    else {
        return;
    };
    state
        .model
        .room_read_states
        .insert(room_id.clone(), read_state.clone());

    let latest_active_event_id = read_state.latest_active_event_id;
    let fully_read_event_id = read_state.fully_read_event_id;
    let marked_unread = read_state.marked_unread;
    let num_unread = read_state.num_unread;
    let num_notifications = read_state.num_notifications;
    let num_mentions = read_state.num_mentions;

    let has_unread_state =
        marked_unread || num_unread > 0 || num_notifications > 0 || num_mentions > 0;
    let clear_read_target = !has_unread_state
        || state
            .room_activity_read_targets
            .get(&ViewId::Room(room_id.clone()))
            .is_some_and(|target| {
                Some(target.as_str()) == latest_active_event_id.as_deref()
                    || Some(target.as_str()) == fully_read_event_id.as_deref()
            });
    if clear_read_target {
        state
            .room_activity_read_targets
            .remove(&ViewId::Room(room_id.clone()));
    }

    // The server's counts decide: no unread state clears even event-only
    // activity, mentions are highlights.
    let room_view_id = ViewId::Room(room_id.clone());
    let room_is_active = state.active_view == room_view_id;
    if has_unread_state && !room_is_active {
        let level = if num_mentions > 0 {
            ViewActivityLevel::Highlight
        } else {
            ViewActivityLevel::Message
        };
        raise_view_activity(
            state,
            room_view_id,
            notifications::room_activity_level(level, mode),
        );
    } else {
        state.room_activity.remove(&room_view_id);
    }
}

fn apply_room_typing_event(state: &mut WindowState, typing: RoomTyping) {
    state
        .model
        .room_views
        .entry(typing.room_id.clone())
        .or_default()
        .replace_typing(typing);
}

fn apply_room_members_event(state: &mut WindowState, members: RoomMembers) {
    state
        .model
        .room_views
        .entry(members.room_id.clone())
        .or_default()
        .replace_members(members);
}

fn room_member_nick(member: &RoomMemberSummary) -> String {
    nick_label(
        &member.user_id,
        member.display_name.as_deref(),
        member.display_name_ambiguous,
    )
}

fn room_member_view_rows(members: &[RoomMemberSummary]) -> Vec<RoomMemberView> {
    members
        .iter()
        .map(|member| RoomMemberView {
            user_id: member.user_id.clone(),
            display_name: member.display_name.clone(),
            label: room_member_nick(member),
            rank: member.rank,
            can_kick: member.can_kick,
            can_ban: member.can_ban,
        })
        .collect()
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
struct ComposeHistory {
    entries: Vec<String>,
    navigation: Option<ComposeHistoryNavigation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComposeHistoryNavigation {
    index: usize,
    draft: String,
}

impl ComposeHistory {
    fn record_submission(&mut self, body: String) {
        self.entries.push(body);
        self.navigation = None;
    }

    fn reset_navigation(&mut self) {
        self.navigation = None;
    }

    fn previous(&mut self, current_text: String) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }

        match self.navigation.as_mut() {
            Some(navigation) => {
                if navigation.index > 0 {
                    navigation.index -= 1;
                }
            }
            None => {
                self.navigation = Some(ComposeHistoryNavigation {
                    index: self.entries.len() - 1,
                    draft: current_text,
                });
            }
        }

        let index = self
            .navigation
            .as_ref()
            .expect("compose history navigation must exist after previous")
            .index;
        Some(&self.entries[index])
    }

    fn next(&mut self) -> Option<String> {
        let navigation = self.navigation.as_mut()?;
        if navigation.index + 1 < self.entries.len() {
            navigation.index += 1;
            return Some(self.entries[navigation.index].clone());
        }

        let draft = navigation.draft.clone();
        self.navigation = None;
        Some(draft)
    }
}

enum ComposeHistoryDirection {
    Previous,
    Next,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum QueryTargetResolution {
    None,
    Single(String),
    Ambiguous,
}

struct WindowState {
    controls: Controls,
    event_queue: Arc<Mutex<VecDeque<Event>>>,
    command_sender: Option<CommandSender>,
    inert_command_receiver: Option<UnboundedReceiver<Command>>,
    automation: Option<AutomationState>,
    render_state_mode: bool,
    clock: UiClock,
    settings: SettingsStore,
    /// The time zone timestamps are shown in.
    local_time: LocalTimeFn,
    text_font: TextFont,
    exit_code: Arc<AtomicI32>,
    event_generation: u64,
    last_idle_probe_generation: Option<u64>,
    synthetic_load_in_progress: bool,
    current_session_homeserver_url: Option<String>,
    tree_items: BTreeMap<isize, ViewId>,
    primary_tree_items: BTreeMap<ViewId, isize>,
    views: BTreeMap<ViewId, MdiViewState>,
    hwnd_views: BTreeMap<isize, ViewId>,
    active_view: ViewId,
    selected_timeline_event: Option<SelectedTimelineEvent>,
    find_states: BTreeMap<ViewId, ViewFindState>,
    /// The Go to Original waiting for older history; one at a time.
    pending_original_jump: Option<replies::OriginalJump>,
    switchbar_views: Vec<ViewId>,
    room_activity: BTreeMap<ViewId, ViewActivityLevel>,
    /// Joined queries whose window the user closed. Like mIRC's closed query
    /// windows, they stay closed until a new message from the other side or
    /// an explicit activation. Holds only joined DM rooms in `model.rooms`.
    closed_queries: BTreeSet<String>,
    room_activity_read_targets: BTreeMap<ViewId, String>,
    /// The last Tab completion, for further Tabs to cycle.
    nick_completion: Option<ComposeNickCompletion>,
    edit_window_proc: WNDPROC,
    model: UiModel,
    decoded_images: BTreeMap<String, GdiplusImage>,
    gdiplus_session: Option<GdiplusSession>,
    gdiplus_startup_error: Option<String>,
    recorded_commands: Vec<Command>,
    submitted_setup_e2ee_recovery: Vec<SubmittedSetupE2eeRecovery>,
    pending_attachment_saves: BTreeMap<String, PendingAttachmentSave>,
    last_attachment_save: Option<AttachmentSaveResult>,
    /// The Options dialog while it is open, for the automation dump.
    options_dialog: Option<HWND>,
}

/// Where Status lines get their arrival time. Render-state snapshots must not
/// depend on when they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiClock {
    System,
    Fixed(u64),
}

/// 2023-11-14 22:12 UTC, a minute before the fixtures' first event.
const RENDER_STATE_CLOCK_MS: u64 = 1_699_999_920_000;

impl WindowState {
    fn timestamps(&self) -> TimestampStyle<'_> {
        TimestampStyle::new(&self.settings.settings().timestamp, self.local_time)
    }
}

/// `timestamp_ms` in the user's time zone, with the DST rules of that date.
fn system_local_time(timestamp_ms: u64) -> LocalTime {
    /// 100 ns intervals from 1601-01-01 to 1970-01-01.
    const UNIX_EPOCH_FILETIME: u64 = 116_444_736_000_000_000;
    let ticks = UNIX_EPOCH_FILETIME + timestamp_ms * 10_000;
    let file_time = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    unsafe {
        assert!(
            FileTimeToSystemTime(&file_time, &mut utc) != 0,
            "FileTimeToSystemTime must convert a post-1970 time"
        );
        assert!(
            SystemTimeToTzSpecificLocalTime(null(), &utc, &mut local) != 0,
            "SystemTimeToTzSpecificLocalTime must convert to the current time zone"
        );
    }
    LocalTime {
        year: local.wYear,
        month: local.wMonth as u8,
        day: local.wDay as u8,
        hour: local.wHour as u8,
        minute: local.wMinute as u8,
        second: local.wSecond as u8,
    }
}

impl UiClock {
    fn now_ms(self) -> u64 {
        match self {
            Self::System => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock must be after the unix epoch")
                .as_millis() as u64,
            Self::Fixed(timestamp_ms) => timestamp_ms,
        }
    }
}

/// A Tab completion and the view whose members it completed; switching
/// views ends it, as the compose box is shared by every view.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ComposeNickCompletion {
    view_id: ViewId,
    completion: NickCompletion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectedTimelineEvent {
    view_id: ViewId,
    event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectedMediaAttachment {
    room_id: String,
    activity_id: String,
    event_id: String,
    filename: Option<String>,
    source: RoomMediaSource,
}

impl Drop for WindowState {
    fn drop(&mut self) {
        self.decoded_images.clear();
    }
}

struct CoreBridge {
    command_sender: CommandSender,
    join_handle: thread::JoinHandle<()>,
}

pub(crate) fn run(command: AppCommand) -> ExitCode {
    match run_inner(command) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

fn run_inner(command: AppCommand) -> Result<ExitCode, String> {
    if matches!(command, AppCommand::Help) {
        println!("{HELP}");
        return Ok(ExitCode::SUCCESS);
    }

    let render_state_events = match &command {
        AppCommand::Run(options) => load_render_state(options)?,
        AppCommand::SelfTest | AppCommand::AutoClose | AppCommand::Help => None,
    };
    let render_state_mode = match &command {
        AppCommand::Run(options) => options.render_state.is_some(),
        AppCommand::SelfTest | AppCommand::AutoClose | AppCommand::Help => false,
    };
    let automation_enabled = automation_enabled_for_command(&command);

    let instance = unsafe { GetModuleHandleW(null()) };
    if instance.is_null() {
        return Err(last_os_error("GetModuleHandleW"));
    }

    init_common_controls()?;
    register_frame_class(instance)?;
    register_mdi_child_class(instance)?;
    register_room_content_class(instance)?;
    register_timeline_class(instance)?;
    register_login_dialog_class(instance)?;
    register_recover_e2ee_dialog_class(instance)?;
    register_setup_e2ee_dialog_class(instance)?;
    register_recovery_key_handoff_dialog_class(instance)?;
    register_find_dialog_class(instance)?;
    register_about_dialog_class(instance)?;
    options_dialog::register_class(instance)?;

    let exit_code = Arc::new(AtomicI32::new(0));
    let event_queue = Arc::new(Mutex::new(VecDeque::new()));
    let (gdiplus_session, gdiplus_startup_error) = initialize_gdiplus_state();
    let settings = load_settings(render_state_mode)
        .inspect_err(|message| show_error_message(null_mut(), FRAME_TITLE, message))?;
    let text_font = unsafe { TextFont::create(&settings.settings().text_font)? };

    let state = Box::new(WindowState {
        controls: Controls::default(),
        event_queue: Arc::clone(&event_queue),
        command_sender: None,
        inert_command_receiver: None,
        automation: None,
        render_state_mode,
        clock: if render_state_mode {
            UiClock::Fixed(RENDER_STATE_CLOCK_MS)
        } else {
            UiClock::System
        },
        settings,
        local_time: system_local_time,
        text_font,
        exit_code: Arc::clone(&exit_code),
        event_generation: 0,
        last_idle_probe_generation: None,
        synthetic_load_in_progress: false,
        current_session_homeserver_url: None,
        tree_items: BTreeMap::new(),
        primary_tree_items: BTreeMap::new(),
        views: BTreeMap::new(),
        hwnd_views: BTreeMap::new(),
        active_view: ViewId::Status,
        selected_timeline_event: None,
        find_states: BTreeMap::new(),
        pending_original_jump: None,
        switchbar_views: Vec::new(),
        room_activity: BTreeMap::new(),
        closed_queries: BTreeSet::new(),
        room_activity_read_targets: BTreeMap::new(),
        nick_completion: None,
        edit_window_proc: None,
        model: initial_model(),
        decoded_images: BTreeMap::new(),
        gdiplus_session,
        gdiplus_startup_error,
        recorded_commands: Vec::new(),
        submitted_setup_e2ee_recovery: Vec::new(),
        pending_attachment_saves: BTreeMap::new(),
        last_attachment_save: None,
        options_dialog: None,
    });

    let hwnd = create_main_window(instance, state, frame_size_for_command(&command))?;

    let startup = (|| -> Result<Option<CoreBridge>, String> {
        if automation_enabled {
            unsafe { enable_automation(hwnd)? };
        }

        let bridge = if should_start_core_bridge(&command) {
            let bridge = start_core_bridge(hwnd, Arc::clone(&event_queue))?;
            unsafe {
                state_mut(hwnd).command_sender = Some(bridge.command_sender.clone());
                let session_saved = session_path()?.is_file();
                if session_saved {
                    let on_startup = state_ref(hwnd).settings.settings().connect.on_startup;
                    submit_restore_saved_session(state_mut(hwnd), on_startup)?;
                    if !on_startup {
                        log_status(
                            state_mut(hwnd),
                            StatusLineKind::Info,
                            status_log::STARTUP_CONNECT_OFF.to_owned(),
                        );
                    }
                    refresh_status_bar(hwnd)?;
                } else {
                    log_status(
                        state_mut(hwnd),
                        StatusLineKind::Info,
                        status_log::CONNECT_HINT.to_owned(),
                    );
                }
                if opens_connect_dialog_at_startup(&command, session_saved)
                    && PostMessageW(hwnd, WM_CONNECT_AT_STARTUP, 0, 0) == 0
                {
                    return Err(last_os_error("PostMessageW WM_CONNECT_AT_STARTUP"));
                }
            }
            Some(bridge)
        } else {
            unsafe {
                if should_install_inert_command_sender(&command) {
                    install_inert_command_sender(state_mut(hwnd));
                }
            }
            None
        };

        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            if automation_enabled {
                SetForegroundWindow(hwnd);
            }
            if let Some(events) = render_state_events {
                apply_event_load(hwnd, events)?;
            }
        }

        Ok(bridge)
    })();

    let bridge = match startup {
        Ok(bridge) => bridge,
        Err(message) => {
            unsafe {
                DestroyWindow(hwnd);
            }
            return Err(message);
        }
    };

    unsafe {
        match command {
            AppCommand::SelfTest => {
                PostMessageW(hwnd, WM_SELF_TEST, 0, 0);
            }
            AppCommand::AutoClose => {
                SetTimer(hwnd, AUTO_CLOSE_TIMER_ID, AUTO_CLOSE_DELAY_MS, None);
            }
            AppCommand::Run(_) | AppCommand::Help => {}
        }
    }

    message_loop();

    if let Some(bridge) = bridge {
        let _ = bridge.join_handle.join();
    }

    Ok(ExitCode::from(
        exit_code.load(Ordering::Relaxed).clamp(0, 255) as u8,
    ))
}

/// The user's `mirx.ini`; render-state snapshots use the defaults and never
/// touch a profile. A file mirx cannot read stops startup: running on would
/// overwrite it with the next change.
fn load_settings(render_state_mode: bool) -> Result<SettingsStore, String> {
    if render_state_mode {
        return Ok(SettingsStore::in_memory());
    }
    SettingsStore::load(profile_dir()?.join(SETTINGS_FILE_NAME))
}

fn start_core_bridge(hwnd: HWND, queue: Arc<Mutex<VecDeque<Event>>>) -> Result<CoreBridge, String> {
    let profile_dir = profile_dir()?;
    let store_passphrase = load_or_create_store_passphrase(&profile_dir)?;
    let handle = start(CoreConfig {
        profile_dir,
        store_passphrase,
    })
    .map_err(|err| format!("failed to start mirx-core: {err}"))?;

    let command_sender = handle.command_sender().clone();
    let hwnd_value = hwnd as isize;
    let join_handle = thread::Builder::new()
        .name("mirx-ui-core-bridge".to_owned())
        .spawn(move || bridge_core_events(hwnd_value as HWND, queue, handle))
        .map_err(|err| format!("failed to spawn core bridge thread: {err}"))?;

    Ok(CoreBridge {
        command_sender,
        join_handle,
    })
}

fn automation_enabled_for_command(command: &AppCommand) -> bool {
    match command {
        AppCommand::Run(options) => options.automation || options.render_state.is_some(),
        AppCommand::SelfTest => true,
        AppCommand::AutoClose | AppCommand::Help => false,
    }
}

fn should_start_core_bridge(command: &AppCommand) -> bool {
    match command {
        AppCommand::Run(options) => options.render_state.is_none(),
        AppCommand::AutoClose => true,
        AppCommand::SelfTest | AppCommand::Help => false,
    }
}

/// Only an interactive run without a saved session opens Select Server by
/// itself; automation, `--auto-close` and `--self-test` never open a modal.
fn opens_connect_dialog_at_startup(command: &AppCommand, session_saved: bool) -> bool {
    match command {
        AppCommand::Run(options) => {
            !options.automation && options.render_state.is_none() && !session_saved
        }
        AppCommand::SelfTest | AppCommand::AutoClose | AppCommand::Help => false,
    }
}

fn should_install_inert_command_sender(command: &AppCommand) -> bool {
    match command {
        AppCommand::Run(options) => options.automation && options.render_state.is_some(),
        AppCommand::SelfTest | AppCommand::AutoClose | AppCommand::Help => false,
    }
}

fn install_inert_command_sender(state: &mut WindowState) {
    let (sender, receiver) = unbounded_channel();
    state.command_sender = Some(CommandSender::new(sender));
    state.inert_command_receiver = Some(receiver);
}

fn frame_size_for_command(command: &AppCommand) -> FrameSize {
    match command {
        AppCommand::Run(options) => options.frame_size.unwrap_or(DEFAULT_FRAME_SIZE),
        AppCommand::SelfTest | AppCommand::AutoClose | AppCommand::Help => DEFAULT_FRAME_SIZE,
    }
}

fn initial_model() -> UiModel {
    UiModel {
        status: "offline".to_owned(),
        lifecycle_state: LifecycleState::Offline,
        ..UiModel::default()
    }
}

fn load_render_state(options: &RunOptions) -> Result<Option<Vec<Event>>, String> {
    let Some(path) = &options.render_state else {
        return Ok(None);
    };

    let file_bytes = fs::read(path)
        .map_err(|err| format!("failed to read render-state file {}: {err}", path.display()))?;
    serde_json::from_slice(&file_bytes)
        .map(Some)
        .map_err(|err| {
            format!(
                "failed to parse render-state file {} as JSON event list: {err}",
                path.display()
            )
        })
}

fn bridge_core_events(hwnd: HWND, queue: Arc<Mutex<VecDeque<Event>>>, handle: CoreHandle) {
    loop {
        match handle.events().recv_timeout(Duration::from_millis(100)) {
            Ok(event) => {
                let should_stop = matches!(event, Event::Stopped);
                queue
                    .lock()
                    .expect("core event queue poisoned")
                    .push_back(event);
                unsafe {
                    PostMessageW(hwnd, WM_CORE_EVENT, 0, 0);
                }
                if should_stop {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = handle.join();
}

fn init_common_controls() -> Result<(), String> {
    let controls = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_TREEVIEW_CLASSES | ICC_BAR_CLASSES | ICC_LISTVIEW_CLASSES,
    };
    let success = unsafe { InitCommonControlsEx(&controls) };
    if success == 0 {
        return Err(last_os_error("InitCommonControlsEx"));
    }
    Ok(())
}

fn register_frame_class(instance: *mut c_void) -> Result<(), String> {
    let class_name = wide_null(FRAME_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_WINDOW as isize + 1) as HBRUSH,
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: null_mut(),
    };

    let atom = unsafe { RegisterClassExW(&window_class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        if error != 1410 {
            return Err(last_os_error("RegisterClassExW"));
        }
    }

    Ok(())
}

fn register_mdi_child_class(instance: *mut c_void) -> Result<(), String> {
    let class_name = wide_null(MDI_CHILD_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(mdi_child_window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_WINDOW as isize + 1) as HBRUSH,
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: null_mut(),
    };

    let atom = unsafe { RegisterClassExW(&window_class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        if error != 1410 {
            return Err(last_os_error("RegisterClassExW MDI child"));
        }
    }

    Ok(())
}

fn register_room_content_class(instance: *mut c_void) -> Result<(), String> {
    let class_name = wide_null(ROOM_CONTENT_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(room_content_window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_WINDOW as isize + 1) as HBRUSH,
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: null_mut(),
    };

    let atom = unsafe { RegisterClassExW(&window_class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        if error != 1410 {
            return Err(last_os_error("RegisterClassExW room content"));
        }
    }

    Ok(())
}

fn register_timeline_class(instance: *mut c_void) -> Result<(), String> {
    let class_name = wide_null(TIMELINE_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        // A double-click on a quote line goes to the message it quotes.
        style: CS_DBLCLKS,
        lpfnWndProc: Some(timeline_window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_WINDOW as isize + 1) as HBRUSH,
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: null_mut(),
    };

    let atom = unsafe { RegisterClassExW(&window_class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        if error != 1410 {
            return Err(last_os_error("RegisterClassExW timeline"));
        }
    }

    Ok(())
}

fn create_main_window(
    instance: *mut c_void,
    state: Box<WindowState>,
    frame_size: FrameSize,
) -> Result<HWND, String> {
    let class_name = wide_null(FRAME_CLASS_NAME);
    let title = wide_null(FRAME_TITLE);
    let mut state = window_data::CreateParam::new(*state);

    let hwnd = unsafe {
        CreateWindowExW(
            0 as WINDOW_EX_STYLE,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            frame_size.width,
            frame_size.height,
            null_mut(),
            null_mut(),
            instance,
            state.as_param(),
        )
    };

    if hwnd.is_null() {
        return Err(last_os_error("CreateWindowExW"));
    }

    Ok(hwnd)
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            window_data::adopt_create_param::<WindowState>(hwnd, lparam);
            // Sets the window text from the CreateWindowExW title.
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_CREATE => match create_controls(hwnd) {
            Ok(()) => {
                if let Err(message) = sync_views_and_tree(hwnd) {
                    state_mut(hwnd).exit_code.store(1, Ordering::Relaxed);
                    eprintln!("{message}");
                    -1
                } else {
                    0
                }
            }
            Err(message) => {
                state_mut(hwnd).exit_code.store(1, Ordering::Relaxed);
                eprintln!("{message}");
                -1
            }
        },
        WM_SIZE => {
            layout_controls(hwnd);
            0
        }
        WM_COMMAND => {
            if handle_switchbar_command(hwnd, wparam) || handle_menu_command(hwnd, wparam).is_some()
            {
                0
            } else {
                default_window_proc(hwnd, message, wparam, lparam)
            }
        }
        WM_INITMENUPOPUP => {
            if let Err(message) = refresh_menu_popup(hwnd, wparam as HMENU) {
                show_error_message(hwnd, "Menu", &message);
            }
            0
        }
        WM_CONTEXTMENU if wparam as HWND == state_ref(hwnd).controls.tree => {
            if let Err(message) = show_navigation_room_context_menu(hwnd, lparam, None) {
                show_error_message(hwnd, "Room", &message);
            }
            0
        }
        WM_CONTEXTMENU => {
            let member_list = state_ref(hwnd)
                .views
                .get(&state_ref(hwnd).active_view)
                .map(|view| room_content_member_list(view.content));
            if member_list == Some(wparam as HWND) {
                if let Err(message) = show_member_context_menu(hwnd, wparam as HWND, lparam, None) {
                    show_error_message(hwnd, "Member", &message);
                }
                0
            } else {
                default_window_proc(hwnd, message, wparam, lparam)
            }
        }
        WM_CORE_EVENT => {
            handle_core_events(hwnd);
            0
        }
        WM_CONNECT_AT_STARTUP => {
            if let Err(message) = connect(hwnd, None) {
                show_error_message(hwnd, "Connect", &message);
            }
            0
        }
        WM_COPYDATA => handle_copydata(hwnd, lparam),
        WM_NOTIFY => {
            let header = &*(lparam as *const NMHDR);
            let control_id =
                u16::try_from(header.idFrom).expect("frame child control ids are 16-bit");
            match (control_id, header.code) {
                (TREE_ID, TVN_SELCHANGEDW) => {
                    handle_tree_selection(hwnd);
                    0
                }
                (TREE_ID, NM_CUSTOMDRAW) => {
                    tree_custom_draw(state_ref(hwnd), &mut *(lparam as *mut NMTVCUSTOMDRAW))
                }
                (SWITCHBAR_ID, NM_CUSTOMDRAW) => {
                    switchbar_custom_draw(state_ref(hwnd), &mut *(lparam as *mut NMTBCUSTOMDRAW))
                }
                _ => default_window_proc(hwnd, message, wparam, lparam),
            }
        }
        WM_SELF_TEST => {
            if let Err(message) = run_self_test(hwnd) {
                state_mut(hwnd).exit_code.store(1, Ordering::Relaxed);
                eprintln!("{message}");
            }
            DestroyWindow(hwnd);
            0
        }
        WM_TIMER if wparam == AUTO_CLOSE_TIMER_ID => {
            KillTimer(hwnd, AUTO_CLOSE_TIMER_ID);
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            let controls = &mut state_mut(hwnd).controls;
            // The menubar goes with the window; these hang in no menubar.
            for menu in [
                &mut controls.room_menu,
                &mut controls.message_menu,
                &mut controls.member_menu,
            ] {
                DestroyMenu(*menu);
                *menu = null_mut();
            }
            controls.room_notifications_menu = null_mut();
            DestroyAcceleratorTable(controls.accelerators);
            controls.accelerators = null_mut();
            if let Some(sender) = state_mut(hwnd).command_sender.as_ref() {
                let _ = sender.send(Command::Shutdown);
            }
            PostQuitMessage(0);
            0
        }
        WM_NCDESTROY => {
            if let Some(state) = window_data::get::<WindowState>(hwnd) {
                destroy_automation_state(state);
            }
            window_data::drop_owned::<WindowState>(hwnd);
            0
        }
        _ => default_window_proc(hwnd, message, wparam, lparam),
    }
}

unsafe extern "system" fn mdi_child_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            match create_mdi_child_controls(hwnd, &mdi_child_create_state(lparam).view_id) {
                Ok(()) => 0,
                Err(message) => {
                    eprintln!("{message}");
                    -1
                }
            }
        }
        WM_SIZE => {
            layout_mdi_child(hwnd);
            // MDI tracks maximization and the frame title here.
            DefMDIChildProcW(hwnd, message, wparam, lparam)
        }
        WM_SETFOCUS => {
            // Activating a window puts the caret in its editbox, as in mIRC.
            let result = DefMDIChildProcW(hwnd, message, wparam, lparam);
            SetFocus(mdi_child_focus(hwnd));
            result
        }
        WM_MDIACTIVATE => {
            if lparam as HWND == hwnd
                && let Err(message) = handle_native_mdi_child_activation(hwnd)
            {
                eprintln!("{message}");
            }
            DefMDIChildProcW(hwnd, message, wparam, lparam)
        }
        WM_SYSCOMMAND if wparam & 0xFFF0 == SYSMENU_ID_FIND => {
            let frame = frame_window_for(hwnd);
            if let Err(message) = begin_find_dialog(frame) {
                show_error_message(frame, "Find", &message);
            }
            0
        }
        WM_SYSCOMMAND if wparam & 0xFFF0 == SYSMENU_ID_SEND_FILE => {
            let frame = frame_window_for(hwnd);
            let ViewId::Room(room_id) = state_ref(frame)
                .hwnd_views
                .get(&(hwnd as isize))
                .cloned()
                .expect("every MDI child is a tracked view")
            else {
                unreachable!("only room windows have Send File...");
            };
            let sent = run_send_file_dialog(frame, room_id).and_then(|status| match status {
                Some(status) => set_local_command_status(frame, status),
                None => Ok(()),
            });
            if let Err(message) = sent {
                show_error_message(frame, "Send File", &message);
            }
            0
        }
        WM_SYSCOMMAND if wparam & 0xFFF0 == SYSMENU_ID_FONT => {
            let frame = frame_window_for(hwnd);
            if let Err(message) = run_local_command(frame, SlashCommand::Font(FontCommand::Dialog))
            {
                show_error_message(frame, "Font", &message);
            }
            0
        }
        WM_CLOSE => {
            let frame = frame_window_for(hwnd);
            if close_view_requested(frame, hwnd) {
                let result = DefMDIChildProcW(hwnd, message, wparam, lparam);
                // The treebar and switchbar follow the views left.
                if let Err(message) = sync_views_and_tree(frame) {
                    show_error_message(frame, "Window", &message);
                }
                result
            } else {
                0
            }
        }
        WM_NCDESTROY => {
            unregister_destroyed_view(hwnd);
            DefMDIChildProcW(hwnd, message, wparam, lparam)
        }
        // Only the Rooms List's list notifies its MDI child.
        WM_NOTIFY => match handle_room_list_notify(frame_window_for(hwnd), lparam) {
            Some(result) => result,
            None => DefMDIChildProcW(hwnd, message, wparam, lparam),
        },
        _ => DefMDIChildProcW(hwnd, message, wparam, lparam),
    }
}

/// Which membership change closing a room window asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloseLeave {
    Part,
    RejectInvite,
}

/// What closing a view's window does, after mIRC.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CloseAction {
    /// The Status window cannot be closed.
    Refuse,
    /// Thread windows just close; no sync reopens them.
    Destroy,
    /// A query window closes, and stays closed until the other side talks
    /// or the user opens it again.
    HideQuery(String),
    /// Closing a channel parts it, and closing an invite rejects it. Unlike
    /// mIRC this asks first, as leaving an invite-only Matrix room cannot be
    /// undone with a rejoin. The window stays until the room leaves the room
    /// list, as mIRC's closes when the PART lands.
    Confirm { room_id: String, leave: CloseLeave },
    /// Closing the Rooms List drops its listing; pages still on the way
    /// are ignored.
    DropRoomList,
}

fn close_action(model: &UiModel, view_id: &ViewId) -> CloseAction {
    match view_id {
        ViewId::Status => CloseAction::Refuse,
        ViewId::RoomList => CloseAction::DropRoomList,
        ViewId::Thread { .. } => CloseAction::Destroy,
        ViewId::Room(room_id) => {
            let room = navigable_room_by_id(&model.rooms, room_id)
                .expect("a room window's room must be navigable");
            match room_window_kind(room) {
                RoomWindowKind::Channel => CloseAction::Confirm {
                    room_id: room_id.clone(),
                    leave: CloseLeave::Part,
                },
                RoomWindowKind::Query => CloseAction::HideQuery(room_id.clone()),
                RoomWindowKind::Invite => CloseAction::Confirm {
                    room_id: room_id.clone(),
                    leave: CloseLeave::RejectInvite,
                },
            }
        }
    }
}

fn close_confirmation_text(leave: CloseLeave, room: &RoomSummary) -> String {
    match leave {
        CloseLeave::Part => format!(
            "Leave {}?\n\nYou may need a new invite to rejoin.",
            room.display_name
        ),
        CloseLeave::RejectInvite => format!("Reject the invite to {}?", room.display_name),
    }
}

/// Handles `WM_CLOSE` on an MDI child (caption X, Ctrl+F4, the system
/// menu); returns whether to destroy the window.
unsafe fn close_view_requested(frame: HWND, child: HWND) -> bool {
    let view_id = state_ref(frame)
        .hwnd_views
        .get(&(child as isize))
        .cloned()
        .expect("every MDI child is a tracked view");
    let action = close_action(&state_ref(frame).model, &view_id);
    match &action {
        CloseAction::Refuse => false,
        CloseAction::Destroy => true,
        CloseAction::HideQuery(room_id) => {
            state_mut(frame).closed_queries.insert(room_id.clone());
            true
        }
        // Before the window goes, so no view sync recreates it.
        CloseAction::DropRoomList => {
            state_mut(frame).model.room_list = None;
            true
        }
        CloseAction::Confirm { room_id, leave } => {
            let text = close_confirmation_text(
                *leave,
                room_by_id(&state_ref(frame).model.rooms, room_id)
                    .expect("a room window's room must exist"),
            );
            let text = wide_null(&text);
            let caption = wide_null(FRAME_TITLE);
            let answer = MessageBoxW(
                frame,
                text.as_ptr(),
                caption.as_ptr(),
                MB_YESNO | MB_ICONQUESTION,
            );
            // Core events keep arriving during the box's modal loop: the
            // room may have left, taking its window with it, or changed.
            if answer != IDYES
                || state_ref(frame).hwnd_views.get(&(child as isize)) != Some(&view_id)
                || close_action(&state_ref(frame).model, &view_id) != action
            {
                return false;
            }
            let result = {
                let state = state_mut(frame);
                match leave {
                    CloseLeave::Part => submit_part_slash_command(state, Some(room_id)),
                    CloseLeave::RejectInvite => submit_reject_slash_command(state, Some(room_id)),
                }
            };
            match result {
                Ok(message) | Err(message) => {
                    if let Err(error) = set_local_command_status(frame, message) {
                        show_error_message(frame, "Window", &error);
                    }
                }
            }
            false
        }
    }
}

unsafe extern "system" fn timeline_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            let line_height = font_line_height(null_mut());
            window_data::adopt(hwnd, TimelineState::new(line_height));
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_SETFONT => {
            set_timeline_font(hwnd, wparam as HFONT);
            if lparam != 0 {
                InvalidateRect(hwnd, null(), 1);
            }
            0
        }
        WM_GETFONT => timeline_state_ref(hwnd).font as LRESULT,
        WM_SETTEXT => set_timeline_text(hwnd, lparam as *const u16),
        WM_GETTEXTLENGTH => timeline_text_len(hwnd),
        WM_GETTEXT => timeline_get_text(hwnd, wparam, lparam),
        WM_TIMELINE_GET_TOP_LINE => timeline_state_ref(hwnd).top_line as LRESULT,
        WM_SIZE => {
            rebuild_timeline_paint_lines(hwnd);
            clamp_timeline_top_line(hwnd);
            update_timeline_scrollbar(hwnd);
            InvalidateRect(hwnd, null(), 1);
            0
        }
        WM_VSCROLL => scroll_timeline(hwnd, wparam),
        WM_LBUTTONDOWN => {
            select_timeline_row(hwnd, lparam);
            0
        }
        // The second click of a double-click: it selects as a click does,
        // and on a quote line goes to the quoted message.
        WM_LBUTTONDBLCLK => {
            select_timeline_row(hwnd, lparam);
            let y = ((lparam >> 16) as i16) as i32;
            let on_quote = timeline_source_row_index_at_client_y(hwnd, y).is_some_and(|index| {
                timeline_state_ref(hwnd).source_rows[index].kind == TimelineRowKind::ReplyQuote
            });
            let frame = frame_window_for(hwnd);
            if on_quote && let Err(message) = go_to_original_of_selected_message(frame) {
                show_error_message(frame, "Message", &message);
            }
            0
        }
        WM_CONTEXTMENU => {
            let frame = frame_window_for(hwnd);
            if let Err(message) = show_timeline_message_context_menu(hwnd, lparam, None) {
                show_error_message(frame, "Message", &message);
            }
            0
        }
        WM_PAINT => {
            paint_timeline(hwnd);
            0
        }
        WM_NCDESTROY => {
            window_data::drop_owned::<TimelineState>(hwnd);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe extern "system" fn room_content_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => match create_room_content_state(hwnd) {
            Ok(state) => {
                window_data::adopt(hwnd, *state);
                layout_room_content(hwnd);
                0
            }
            Err(message) => {
                eprintln!("{message}");
                -1
            }
        },
        WM_SIZE => {
            layout_room_content(hwnd);
            0
        }
        // From the owner-drawn nick list. The first arrives while the list
        // is created, before this window's state exists: the height comes
        // from the frame.
        WM_MEASUREITEM => {
            let measure = &mut *(lparam as *mut MEASUREITEMSTRUCT);
            measure.itemHeight = text_font_item_height(hwnd);
            1
        }
        WM_DRAWITEM => {
            draw_nick_list_item(hwnd, &*(lparam as *const DRAWITEMSTRUCT));
            1
        }
        WM_NCDESTROY => {
            window_data::drop_owned::<RoomContentState>(hwnd);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn mdi_child_create_state(lparam: LPARAM) -> &'static MdiChildCreateState {
    let create = &*(lparam as *const CREATESTRUCTW);
    let mdi_create = &*(create.lpCreateParams as *const MDICREATESTRUCTW);
    let state_ptr = mdi_create.lParam as *const MdiChildCreateState;
    assert!(
        !state_ptr.is_null(),
        "MDI child creation state must be present during WM_CREATE"
    );
    &*state_ptr
}

unsafe fn room_content_state_ref(hwnd: HWND) -> &'static RoomContentState {
    window_data::get(hwnd).expect("room content state must exist while the control is alive")
}

unsafe fn create_room_content_state(hwnd: HWND) -> Result<Box<RoomContentState>, String> {
    let instance = GetModuleHandleW(null());
    let timeline = CreateWindowExW(
        WS_EX_CLIENTEDGE,
        wide_null(TIMELINE_CLASS_NAME).as_ptr(),
        wide_null("").as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_VSCROLL,
        0,
        0,
        0,
        0,
        hwnd,
        null_mut(),
        instance,
        null_mut(),
    );
    if timeline.is_null() {
        return Err(last_os_error("CreateWindowExW room timeline"));
    }

    let members = CreateWindowExW(
        WS_EX_CLIENTEDGE,
        wide_null("LISTBOX").as_ptr(),
        wide_null("").as_ptr(),
        WS_CHILD
            | WS_VISIBLE
            | WS_VSCROLL
            | (LBS_NOINTEGRALHEIGHT | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS) as u32,
        0,
        0,
        0,
        0,
        hwnd,
        null_mut(),
        instance,
        null_mut(),
    );
    if members.is_null() {
        return Err(last_os_error("CreateWindowExW room member list"));
    }

    Ok(Box::new(RoomContentState { timeline, members }))
}

/// A nick list item's height: a line of the frame's text font, as the
/// stock listbox sizes its items for its font.
unsafe fn text_font_item_height(hwnd: HWND) -> u32 {
    let height = state_ref(frame_window_for(hwnd))
        .text_font
        .glyphs
        .metrics()
        .tmHeight;
    u32::try_from(height).expect("a font height is positive")
}

/// Draws a nick list item like the stock listbox does, with the text in the
/// frame's text font and its fallbacks.
unsafe fn draw_nick_list_item(hwnd: HWND, item: &DRAWITEMSTRUCT) {
    let hdc = item.hDC;
    // An empty list with the focus: only the focus rectangle.
    if item.itemID == u32::MAX {
        if item.itemState & ODS_FOCUS != 0 {
            DrawFocusRect(hdc, &item.rcItem);
        }
        return;
    }
    let (background, text_color) = if item.itemState & ODS_SELECTED != 0 {
        (COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT)
    } else {
        (COLOR_WINDOW, COLOR_WINDOWTEXT)
    };
    FillRect(hdc, &item.rcItem, GetSysColorBrush(background));
    let previous_color = SetTextColor(hdc, GetSysColor(text_color));
    let previous_mode = SetBkMode(hdc, TRANSPARENT as i32);

    let length = SendMessageW(item.hwndItem, LB_GETTEXTLEN, item.itemID as WPARAM, 0);
    let length = usize::try_from(length).expect("a nick list item has text");
    let mut text = vec![0u16; length + 1];
    SendMessageW(
        item.hwndItem,
        LB_GETTEXT,
        item.itemID as WPARAM,
        text.as_mut_ptr() as LPARAM,
    );
    let text = String::from_utf16(&text[..length]).expect("nick list items are UTF-16 text");
    let glyphs = &state_ref(frame_window_for(hwnd)).text_font.glyphs;
    glyphs.draw(
        hdc,
        item.rcItem.left + 1,
        item.rcItem.top + glyphs.metrics().tmAscent,
        &item.rcItem,
        &text,
        false,
    );

    SetBkMode(hdc, previous_mode);
    SetTextColor(hdc, previous_color);
    if item.itemState & ODS_FOCUS != 0 {
        DrawFocusRect(hdc, &item.rcItem);
    }
}

unsafe fn layout_room_content(hwnd: HWND) {
    let Some(state) = window_data::get::<RoomContentState>(hwnd) else {
        return;
    };
    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);
    let height = rect.bottom - rect.top;
    let layout = mdi::room_content_layout(rect.right - rect.left);
    MoveWindow(state.timeline, 0, 0, layout.timeline_width, height, 1);
    MoveWindow(
        state.members,
        layout.members_x,
        0,
        layout.members_width,
        height,
        1,
    );
}

unsafe fn timeline_state_ref(hwnd: HWND) -> &'static TimelineState {
    timeline_state_mut(hwnd)
}

unsafe fn timeline_state_mut(hwnd: HWND) -> &'static mut TimelineState {
    window_data::get(hwnd).expect("timeline state must exist while the control is alive")
}

unsafe fn timeline_client_width(hwnd: HWND) -> i32 {
    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);
    rect.right - rect.left
}

unsafe fn rebuild_timeline_paint_lines(hwnd: HWND) {
    let client_width = timeline_client_width(hwnd);
    let (source_rows, lines) = {
        let state = timeline_state_ref(hwnd);
        (state.source_rows.clone(), state.lines.clone())
    };
    let paint_lines = if source_rows.is_empty() {
        plain_timeline_paint_lines(&lines)
    } else {
        timeline_row_paint_lines(hwnd, &source_rows, client_width)
    };
    timeline_state_mut(hwnd).paint_lines = paint_lines;
}

unsafe fn apply_timeline_content(
    hwnd: HWND,
    source_rows: Vec<TimelineRow>,
    rendered_text: String,
    lines: Vec<String>,
    paint_lines: Vec<TimelinePaintLine>,
    retain_tail: bool,
) {
    let visible_rows = timeline_visible_rows(hwnd);
    let (top_line, tail_pinned) = {
        let state = timeline_state_ref(hwnd);
        (
            state.top_line,
            timeline_is_tail_pinned(
                state.source_rows.len(),
                state.top_line,
                state.paint_lines.len(),
                visible_rows,
            ),
        )
    };
    let state = timeline_state_mut(hwnd);
    state.source_rows = source_rows;
    state.rendered_text = rendered_text;
    state.lines = lines;
    state.paint_lines = paint_lines;
    state.top_line = timeline_top_line_after_content_replaced(
        top_line,
        tail_pinned && retain_tail,
        state.paint_lines.len(),
        visible_rows,
    );
    update_timeline_scrollbar(hwnd);
    InvalidateRect(hwnd, null(), 1);
}

unsafe fn set_timeline_text(hwnd: HWND, text_ptr: *const u16) -> LRESULT {
    let (rendered_text, lines) = normalize_timeline_text(&wide_ptr_to_string(text_ptr));
    let paint_lines = plain_timeline_paint_lines(&lines);
    apply_timeline_content(hwnd, Vec::new(), rendered_text, lines, paint_lines, false);
    1
}

/// Shows `rows`, whose timestamps are as wide as `indent_sample`.
unsafe fn set_timeline_rows(hwnd: HWND, rows: &[TimelineRow], indent_sample: String) {
    timeline_state_mut(hwnd).indent_sample = indent_sample;
    let (rendered_text, lines) =
        normalize_timeline_text(&render_view_text(&project_timeline_rows(rows)));
    let paint_lines = timeline_row_paint_lines(hwnd, rows, timeline_client_width(hwnd));
    debug_assert_eq!(
        project_timeline_rows(rows),
        rows.iter()
            .map(|row| row.logical_text.clone())
            .collect::<Vec<_>>()
    );
    apply_timeline_content(hwnd, rows.to_vec(), rendered_text, lines, paint_lines, true);
}

unsafe fn select_timeline_row(hwnd: HWND, lparam: LPARAM) {
    let y = ((lparam >> 16) as i16) as i32;
    select_timeline_event_at_client_y(hwnd, y);
}

/// The source row of the line at client `y`, if a line is there.
unsafe fn timeline_source_row_index_at_client_y(hwnd: HWND, y: i32) -> Option<usize> {
    let state = timeline_state_ref(hwnd);
    let line_index = y
        .checked_sub(state.vertical_padding)
        .filter(|offset| *offset >= 0)
        .map(|offset| state.top_line + (offset / state.line_height) as usize);
    line_index
        .and_then(|index| state.paint_lines.get(index))
        .and_then(|line| line.source_row_index)
}

unsafe fn timeline_event_id_at_client_y(hwnd: HWND, y: i32) -> Option<String> {
    timeline_source_row_index_at_client_y(hwnd, y)
        .and_then(|index| timeline_state_ref(hwnd).source_rows[index].event_id.clone())
}

/// The first row of the event's lines: its quote line if it is a reply.
unsafe fn timeline_source_row_index_of_event(hwnd: HWND, event_id: &str) -> Option<usize> {
    timeline_state_ref(hwnd)
        .source_rows
        .iter()
        .position(|row| row.event_id.as_deref() == Some(event_id))
}

unsafe fn select_timeline_event_at_client_y(hwnd: HWND, y: i32) {
    let event_id = timeline_event_id_at_client_y(hwnd, y);
    let frame = frame_window_for(hwnd);
    let window_state = state_mut(frame);
    window_state.selected_timeline_event = event_id.map(|event_id| SelectedTimelineEvent {
        view_id: window_state.active_view.clone(),
        event_id,
    });
    InvalidateRect(hwnd, null(), 1);
}

unsafe fn context_menu_screen_point_for_lparam(lparam: LPARAM) -> Option<(i32, i32)> {
    (lparam != -1).then_some((((lparam as i16) as i32), (((lparam >> 16) as i16) as i32)))
}

unsafe fn default_context_point_for_window(hwnd: HWND) -> Result<(i32, i32), String> {
    let mut rect = RECT::default();
    if GetClientRect(hwnd, &mut rect) == 0 {
        return Err(last_os_error("GetClientRect"));
    }
    let mut point = POINT {
        x: (rect.right - rect.left) / 2,
        y: (rect.bottom - rect.top) / 2,
    };
    if ClientToScreen(hwnd, &mut point) == 0 {
        return Err(last_os_error("ClientToScreen"));
    }
    Ok((point.x, point.y))
}

unsafe fn show_context_menu(
    hwnd: HWND,
    menu: HMENU,
    x: i32,
    y: i32,
    requested_command: Option<usize>,
) -> Result<Option<usize>, String> {
    if let Some(command) = requested_command {
        return Ok(Some(command));
    }
    let command = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, x, y, 0, hwnd, null());
    Ok((command != 0).then_some(command as usize))
}

unsafe fn show_timeline_message_context_menu(
    timeline: HWND,
    lparam: LPARAM,
    requested_command: Option<usize>,
) -> Result<(), String> {
    let frame = frame_window_for(timeline);
    let (x, y) = match context_menu_screen_point_for_lparam(lparam) {
        Some((x, y)) => {
            let mut point = POINT { x, y };
            if ScreenToClient(timeline, &mut point) == 0 {
                return Err(last_os_error("ScreenToClient"));
            }
            select_timeline_event_at_client_y(timeline, point.y);
            (x, y)
        }
        None => default_context_point_for_window(timeline)?,
    };
    let menu = state_ref(frame).controls.message_menu;
    refresh_message_menu_state(frame, menu)?;
    let action_state = active_message_action_state_for_automation(frame);
    if requested_command.is_some_and(|command| {
        action_state.selected_event_id.is_none()
            || (command == MENU_ID_MESSAGE_EDIT && !action_state.edit_enabled)
            || (command == MENU_ID_MESSAGE_SAVE_ATTACHMENT && !action_state.save_attachment_enabled)
            || (command == MENU_ID_MESSAGE_GO_TO_ORIGINAL && !action_state.go_to_original_enabled)
    }) {
        return Err("message context menu action is disabled".to_owned());
    }
    if let Some(command) = show_context_menu(frame, menu, x, y, requested_command)?
        && let Some(result) = handle_menu_command(frame, command)
    {
        result?;
    }
    Ok(())
}

unsafe fn show_member_context_menu(
    frame: HWND,
    member_list: HWND,
    lparam: LPARAM,
    requested_command: Option<usize>,
) -> Result<(), String> {
    let (x, y) = match context_menu_screen_point_for_lparam(lparam) {
        Some((x, y)) => {
            let mut point = POINT { x, y };
            if ScreenToClient(member_list, &mut point) == 0 {
                return Err(last_os_error("ScreenToClient"));
            }
            let hit = SendMessageW(
                member_list,
                LB_ITEMFROMPOINT,
                0,
                ((point.y as u32 as usize) << 16 | point.x as u16 as usize) as LPARAM,
            );
            if (hit as usize & 0xffff_0000) != 0 {
                return Ok(());
            }
            if SendMessageW(member_list, LB_SETCURSEL, hit as usize, 0) == LB_ERR as isize {
                return Ok(());
            }
            (x, y)
        }
        None => default_context_point_for_window(member_list)?,
    };
    let menu = state_ref(frame).controls.member_menu;
    refresh_member_menu_state(frame, menu)?;
    let action_state = active_member_action_state(frame);
    if requested_command.is_some_and(|command| !action_state.enabled(command)) {
        return Err("member context menu action is disabled".to_owned());
    }
    if let Some(command) = show_context_menu(frame, menu, x, y, requested_command)?
        && let Some(result) = handle_menu_command(frame, command)
    {
        result?;
    }
    Ok(())
}

unsafe fn query_selected_member(hwnd: HWND) -> Result<(), String> {
    let member = selected_member(hwnd).ok_or_else(|| "no member is selected".to_owned())?;
    let status = submit_query_slash_command(state_mut(hwnd), member.user_id)?;
    state_mut(hwnd).model.status = status;
    refresh_status_bar(hwnd)
}

/// Inserts the selected member's name at the compose caret, replacing any
/// selection, followed by a space to type on.
unsafe fn mention_selected_member(hwnd: HWND) -> Result<(), String> {
    let member = selected_member(hwnd).ok_or_else(|| "no member is selected".to_owned())?;
    let mention = member_mention_text(&member.user_id, member.display_name.as_deref());
    let edit = compose_edit(hwnd);
    active_compose_history(state_mut(hwnd)).reset_navigation();
    replace_edit_selection(edit, &format!("{mention} "));
    SetFocus(edit);
    Ok(())
}

/// Replaces the edit control's selection, or inserts at its caret, and
/// leaves the caret after `text`.
unsafe fn replace_edit_selection(edit: HWND, text: &str) {
    let text = wide_null(text);
    // FALSE: no separate undo step for the replacement.
    SendMessageW(edit, EM_REPLACESEL, 0, text.as_ptr() as LPARAM);
}

/// The compose text as UTF-16 and the caret position in it.
unsafe fn compose_text_and_caret(edit: HWND) -> (Vec<u16>, usize) {
    let text_len = SendMessageW(edit, WM_GETTEXTLENGTH, 0, 0) as usize;
    let mut text = vec![0u16; text_len + 1];
    let copied = SendMessageW(edit, WM_GETTEXT, text.len(), text.as_mut_ptr() as LPARAM) as usize;
    text.truncate(copied);
    let mut start = 0u32;
    let mut end = 0u32;
    SendMessageW(
        edit,
        EM_GETSEL,
        (&mut start as *mut u32) as WPARAM,
        (&mut end as *mut u32) as LPARAM,
    );
    // The caret is at the selection's end; completing replaces the word
    // before it, which drops any selection.
    (text, end as usize)
}

/// Tab (or Shift+Tab when `reverse`) in the compose box: continues the last
/// completion while the box still holds what it inserted, else completes
/// the word before the caret against the active view's members.
unsafe fn complete_compose_nick(hwnd: HWND, reverse: bool) -> Result<(), String> {
    let edit = compose_edit(hwnd);
    let (text, caret) = compose_text_and_caret(edit);
    let state = state_mut(hwnd);
    let view_id = state.active_view.clone();
    let edit_op = match state.nick_completion.as_mut() {
        Some(last) if last.view_id == view_id && last.completion.continues(&text, caret) => {
            last.completion.advance(reverse)
        }
        _ => {
            let started = NickCompletion::start(&text, caret, active_view_members(state), reverse);
            let Some((completion, edit_op)) = started else {
                state.nick_completion = None;
                return Ok(());
            };
            state.nick_completion = Some(ComposeNickCompletion {
                view_id,
                completion,
            });
            edit_op
        }
    };
    active_compose_history(state).reset_navigation();
    SendMessageW(
        edit,
        EM_SETSEL,
        edit_op.replace.start,
        edit_op.replace.end as LPARAM,
    );
    replace_edit_selection(edit, &edit_op.text);
    Ok(())
}

/// Runs a member menu item's command on the selected member, as if typed:
/// its refusals and errors print where typed ones do.
unsafe fn run_selected_member_command(hwnd: HWND, id: usize) -> Result<(), String> {
    let member = selected_member(hwnd).ok_or_else(|| "no member is selected".to_owned())?;
    let command = member_menu_command(id, &member.user_id)
        .unwrap_or_else(|| panic!("member menu item {id} runs no command"));
    run_local_command(hwnd, command)
}

unsafe fn invoke_member_context_menu_for_automation(
    hwnd: HWND,
    user_id: &str,
    command: &ContextMenuCommand,
) -> Result<(), String> {
    let list = active_member_list(hwnd).ok_or_else(|| "member list is unavailable".to_owned())?;
    let lparam = member_context_lparam_for_automation(hwnd, list, user_id)?;
    let command = member_context_menu_command_id(command)?;
    show_member_context_menu(hwnd, list, lparam, Some(command))
}

unsafe fn invoke_selected_member_context_menu_for_automation(
    hwnd: HWND,
    command: &ContextMenuCommand,
) -> Result<(), String> {
    let command = member_context_menu_command_id(command)?;
    let list = active_member_list(hwnd).ok_or_else(|| "member list is unavailable".to_owned())?;
    show_member_context_menu(hwnd, list, -1, Some(command))
}

fn member_context_menu_command_id(command: &ContextMenuCommand) -> Result<usize, String> {
    context_menu_command_id(command)
        .filter(|id| member_menu_item_ids().any(|member_id| member_id == *id))
        .ok_or_else(|| "member context menu command is unavailable".to_owned())
}

unsafe fn member_context_lparam_for_automation(
    hwnd: HWND,
    list: HWND,
    user_id: &str,
) -> Result<LPARAM, String> {
    let index = active_member_rows(hwnd)
        .iter()
        .position(|member| member.user_id == user_id)
        .ok_or_else(|| format!("unknown_member: {user_id}"))?;
    let mut rect = RECT::default();
    if SendMessageW(
        list,
        LB_GETITEMRECT,
        index,
        &mut rect as *mut RECT as LPARAM,
    ) == LB_ERR as isize
    {
        return Err("member context menu target is unavailable".to_owned());
    }
    let mut point = POINT {
        x: (rect.left + rect.right) / 2,
        y: (rect.top + rect.bottom) / 2,
    };
    if ClientToScreen(list, &mut point) == 0 {
        return Err(last_os_error("ClientToScreen"));
    }
    Ok(((point.y as u32 as usize) << 16 | (point.x as u16 as usize)) as LPARAM)
}

fn validate_selected_timeline_event(
    state: &mut WindowState,
    view_id: &ViewId,
    rows: &[TimelineRow],
) {
    let Some(selected) = state.selected_timeline_event.as_ref() else {
        return;
    };
    if selected.view_id == *view_id
        && !rows
            .iter()
            .any(|row| row.event_id.as_deref() == Some(&selected.event_id))
    {
        state.selected_timeline_event = None;
    }
}

unsafe fn timeline_source_row_paint_line_count(hwnd: HWND, row: &TimelineRow) -> usize {
    timeline_row_paint_lines(hwnd, std::slice::from_ref(row), timeline_client_width(hwnd)).len()
}

unsafe fn timeline_source_row_index_for_top_line(
    hwnd: HWND,
    rows: &[TimelineRow],
    top_line: usize,
) -> Option<usize> {
    let mut next_top_line = 0usize;
    for (index, row) in rows.iter().enumerate() {
        let line_count = timeline_source_row_paint_line_count(hwnd, row);
        if top_line < next_top_line + line_count {
            return Some(index);
        }
        next_top_line += line_count;
    }

    (!rows.is_empty()).then_some(rows.len() - 1)
}

unsafe fn timeline_top_line_for_source_row_index(
    hwnd: HWND,
    rows: &[TimelineRow],
    source_row_index: usize,
) -> usize {
    rows.iter()
        .take(source_row_index)
        .map(|row| timeline_source_row_paint_line_count(hwnd, row))
        .sum()
}

/// Fills the nick list in `rows` order: rows are looked up by list index, so
/// the list must not sort itself.
unsafe fn set_room_member_list(hwnd: HWND, rows: &[RoomMemberView]) {
    SendMessageW(hwnd, LB_RESETCONTENT, 0, 0);
    for row in rows {
        let label = wide_null(&format!("{}{}", rank_prefix(row.rank), row.label));
        SendMessageW(hwnd, LB_ADDSTRING, 0, label.as_ptr() as LPARAM);
    }
}

unsafe fn timeline_text_len(hwnd: HWND) -> LRESULT {
    timeline_state_ref(hwnd)
        .rendered_text
        .encode_utf16()
        .count() as LRESULT
}

unsafe fn timeline_get_text(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if wparam == 0 || lparam == 0 {
        return 0;
    }

    let state = timeline_state_ref(hwnd);
    let capacity = wparam;
    let buffer = lparam as *mut u16;
    let encoded = state.rendered_text.encode_utf16().collect::<Vec<_>>();
    let copied = encoded.len().min(capacity.saturating_sub(1));
    copy_nonoverlapping(encoded.as_ptr(), buffer, copied);
    *buffer.add(copied) = 0;
    copied as LRESULT
}

/// Copies `text` into a control's text buffer of `capacity` UTF-16 units,
/// NUL-terminated and cut short when it does not fit, never inside a
/// surrogate pair.
unsafe fn copy_text_to_buffer(text: &str, buffer: *mut u16, capacity: i32) {
    let capacity = usize::try_from(capacity).expect("a text buffer has a capacity");
    assert!(capacity > 0, "a text buffer holds at least the NUL");
    let encoded = text.encode_utf16().collect::<Vec<_>>();
    let mut copied = encoded.len().min(capacity - 1);
    if copied < encoded.len() && (0xD800..0xDC00).contains(&encoded[copied]) {
        // `copied` is the low half of a pair cut off: drop the high half.
        copied -= 1;
    }
    copy_nonoverlapping(encoded.as_ptr(), buffer, copied);
    *buffer.add(copied) = 0;
}

unsafe fn timeline_visible_rows(hwnd: HWND) -> usize {
    let state = timeline_state_ref(hwnd);
    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);
    let client_height = rect.bottom - rect.top;
    let usable_height = (client_height - state.vertical_padding * 2).max(state.line_height);
    (usable_height / state.line_height).max(1) as usize
}

fn timeline_max_top_line(line_count: usize, visible_rows: usize) -> usize {
    line_count.saturating_sub(visible_rows)
}

fn clamped_timeline_top_line(top_line: usize, line_count: usize, visible_rows: usize) -> usize {
    top_line.min(timeline_max_top_line(line_count, visible_rows))
}

fn timeline_is_tail_pinned(
    source_row_count: usize,
    top_line: usize,
    line_count: usize,
    visible_rows: usize,
) -> bool {
    source_row_count == 0 || top_line >= timeline_max_top_line(line_count, visible_rows)
}

fn timeline_top_line_after_content_replaced(
    previous_top_line: usize,
    tail_pinned: bool,
    line_count: usize,
    visible_rows: usize,
) -> usize {
    if tail_pinned {
        timeline_max_top_line(line_count, visible_rows)
    } else {
        clamped_timeline_top_line(previous_top_line, line_count, visible_rows)
    }
}

unsafe fn clamp_timeline_top_line(hwnd: HWND) {
    let visible_rows = timeline_visible_rows(hwnd);
    let state = timeline_state_mut(hwnd);
    state.top_line =
        clamped_timeline_top_line(state.top_line, state.paint_lines.len(), visible_rows);
}

unsafe fn update_timeline_scrollbar(hwnd: HWND) {
    let visible_rows = timeline_visible_rows(hwnd);
    let state = timeline_state_ref(hwnd);
    let expected_max = state.paint_lines.len().saturating_sub(1) as i32;
    let expected_page = visible_rows.max(1).min(state.paint_lines.len().max(1)) as u32;
    let scroll_info = SCROLLINFO {
        cbSize: size_of::<SCROLLINFO>() as u32,
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
        nMin: 0,
        nMax: expected_max,
        nPage: expected_page,
        nPos: state.top_line as i32,
        nTrackPos: 0,
    };
    let set_result = SetScrollInfo(hwnd, SB_VERT, &scroll_info, 1);

    let mut actual = SCROLLINFO {
        cbSize: size_of::<SCROLLINFO>() as u32,
        fMask: SIF_ALL,
        nMin: 0,
        nMax: 0,
        nPage: 0,
        nPos: 0,
        nTrackPos: 0,
    };
    let get_success = GetScrollInfo(hwnd, SB_VERT, &mut actual);
    if get_success == 0
        || actual.nMax != expected_max
        || actual.nPage != expected_page
        || actual.nPos != state.top_line as i32
    {
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect);
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let error = if get_success == 0 { GetLastError() } else { 0 };
        eprintln!(
            "timeline scrollbar mismatch: hwnd={:#x} style=0x{style:x} rect={}x{} lines={} visible_rows={} expected_max={} expected_page={} expected_pos={} set_result={} get_success={} actual_max={} actual_page={} actual_pos={} last_error={}",
            hwnd as usize,
            rect.right - rect.left,
            rect.bottom - rect.top,
            state.paint_lines.len(),
            visible_rows,
            expected_max,
            expected_page,
            state.top_line,
            set_result,
            get_success,
            actual.nMax,
            actual.nPage,
            actual.nPos,
            error
        );
    }
}

unsafe fn capture_room_history_backfill_anchor(
    state: &WindowState,
    room_id: &str,
) -> Option<TimelineBackfillAnchor> {
    let ViewId::Room(active_room_id) = &state.active_view else {
        return None;
    };
    if active_room_id != room_id {
        return None;
    }

    let view = state.views.get(&state.active_view)?;
    let timeline = room_content_timeline(view.content);
    let timeline_state = timeline_state_ref(timeline);
    let visible_source_row_index = timeline_source_row_index_for_top_line(
        timeline,
        &timeline_state.source_rows,
        timeline_state.top_line,
    )?;
    let source_row_index = timeline_history_backfill_anchor_row_index(
        &timeline_state.source_rows,
        visible_source_row_index,
    )?;
    Some(TimelineBackfillAnchor {
        room_id: room_id.to_owned(),
        source_row: timeline_state.source_rows.get(source_row_index)?.clone(),
        source_row_index,
        source_row_count: timeline_state.source_rows.len(),
    })
}

fn timeline_history_backfill_anchor_row_index(
    rows: &[TimelineRow],
    visible_source_row_index: usize,
) -> Option<usize> {
    rows.iter()
        .enumerate()
        .skip(visible_source_row_index)
        .find_map(|(index, row)| (!timeline_row_is_fixed_room_context(row)).then_some(index))
}

fn timeline_row_is_fixed_room_context(row: &TimelineRow) -> bool {
    matches!(
        &row.kind,
        TimelineRowKind::Event | TimelineRowKind::Info | TimelineRowKind::Typing
    )
}

unsafe fn restore_room_history_backfill_anchor(hwnd: HWND, anchor: TimelineBackfillAnchor) {
    let state = state_ref(hwnd);
    let ViewId::Room(active_room_id) = &state.active_view else {
        return;
    };
    if *active_room_id != anchor.room_id {
        return;
    }

    let Some(view) = state.views.get(&state.active_view) else {
        return;
    };
    let timeline = room_content_timeline(view.content);
    let timeline_state = timeline_state_ref(timeline);
    if timeline_state.source_rows.is_empty() {
        return;
    }

    let source_row_index = timeline_state
        .source_rows
        .iter()
        .position(|row| row == &anchor.source_row)
        .unwrap_or_else(|| {
            anchor
                .source_row_index
                .saturating_add(
                    timeline_state
                        .source_rows
                        .len()
                        .saturating_sub(anchor.source_row_count),
                )
                .min(timeline_state.source_rows.len().saturating_sub(1))
        });
    let top_line = timeline_top_line_for_source_row_index(
        timeline,
        &timeline_state.source_rows,
        source_row_index,
    );
    let visible_rows = timeline_visible_rows(timeline);
    let max_top_line = timeline_max_top_line(timeline_state.paint_lines.len(), visible_rows);
    timeline_state_mut(timeline).top_line = top_line.min(max_top_line);
    update_timeline_scrollbar(timeline);
    InvalidateRect(timeline, null(), 1);
}

unsafe fn scroll_timeline(hwnd: HWND, wparam: WPARAM) -> LRESULT {
    // The user scrolling a window drops its pending Go to Original; the
    // jump's own scroll sets the top line without WM_VSCROLL.
    cancel_original_jump_for_timeline(hwnd);
    let command = (wparam & 0xffff) as i32;
    let visible_rows = timeline_visible_rows(hwnd);
    let max_top_line = {
        let state = timeline_state_ref(hwnd);
        timeline_max_top_line(state.paint_lines.len(), visible_rows)
    };
    let current = timeline_state_ref(hwnd).top_line;

    let next = match command {
        SB_TOP => 0,
        SB_BOTTOM => max_top_line,
        SB_LINEUP => current.saturating_sub(1),
        SB_LINEDOWN => current.saturating_add(1),
        SB_PAGEUP => current.saturating_sub(visible_rows),
        SB_PAGEDOWN => current.saturating_add(visible_rows),
        SB_THUMBPOSITION | SB_THUMBTRACK => {
            let mut scroll_info = SCROLLINFO {
                cbSize: size_of::<SCROLLINFO>() as u32,
                fMask: SIF_TRACKPOS,
                nMin: 0,
                nMax: 0,
                nPage: 0,
                nPos: 0,
                nTrackPos: 0,
            };
            GetScrollInfo(hwnd, SB_VERT, &mut scroll_info);
            scroll_info.nTrackPos.max(0) as usize
        }
        _ => current,
    }
    .min(max_top_line);

    if next != current {
        timeline_state_mut(hwnd).top_line = next;
        update_timeline_scrollbar(hwnd);
        InvalidateRect(hwnd, null(), 1);
    }

    if matches!(
        command,
        SB_TOP | SB_PAGEUP | SB_THUMBPOSITION | SB_THUMBTRACK
    ) && next == 0
        && let Err(message) = submit_backfill_room_history(hwnd)
    {
        eprintln!("{message}");
    }

    0
}

unsafe fn timeline_window_state(hwnd: HWND) -> &'static WindowState {
    let room_content = GetParent(hwnd);
    assert!(
        !room_content.is_null(),
        "timeline control must have a room content parent"
    );
    let mdi_child = GetParent(room_content);
    assert!(
        !mdi_child.is_null(),
        "room content must have an mdi child parent"
    );
    let mdi_client = GetParent(mdi_child);
    assert!(
        !mdi_client.is_null(),
        "mdi child must have an mdi client parent"
    );
    let frame = GetParent(mdi_client);
    assert!(!frame.is_null(), "mdi client must have a frame parent");
    state_ref(frame)
}

unsafe fn timeline_window_state_mut(hwnd: HWND) -> &'static mut WindowState {
    let room_content = GetParent(hwnd);
    assert!(
        !room_content.is_null(),
        "timeline control must have a room content parent"
    );
    let mdi_child = GetParent(room_content);
    assert!(
        !mdi_child.is_null(),
        "room content must have an mdi child parent"
    );
    let mdi_client = GetParent(mdi_child);
    assert!(
        !mdi_client.is_null(),
        "mdi child must have an mdi client parent"
    );
    let frame = GetParent(mdi_client);
    assert!(!frame.is_null(), "mdi client must have a frame parent");
    state_mut(frame)
}

unsafe fn create_controls(hwnd: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let state = state_mut(hwnd);

    create_frame_menu(hwnd)?;

    state.controls.switchbar = CreateWindowExW(
        0,
        TOOLBARCLASSNAMEW,
        null(),
        // The frame lays the switchbar out. Without CCS_NORESIZE the toolbar
        // sizes itself to its ideal height and overlaps the MDI client's top
        // edge, painting over it whenever the switchbar repaints. Its
        // buttons wrap onto more rows on its own WM_SIZE (TBSTYLE_WRAPABLE);
        // `layout_controls` shows it as the settings say and sizes it to
        // its rows.
        WS_CHILD | TBSTYLE_FLAT | TBSTYLE_WRAPABLE | CCS_NORESIZE as u32 | CCS_NOPARENTALIGN as u32,
        0,
        0,
        0,
        0,
        hwnd,
        SWITCHBAR_ID as usize as HMENU,
        instance,
        null_mut(),
    );
    if state.controls.switchbar.is_null() {
        return Err(last_os_error("CreateWindowExW switchbar"));
    }
    SendMessageW(
        state.controls.switchbar,
        TB_BUTTONSTRUCTSIZE,
        size_of::<TBBUTTON>(),
        0,
    );
    SendMessageW(
        state.controls.switchbar,
        TB_SETEXTENDEDSTYLE,
        0,
        TBSTYLE_EX_MIXEDBUTTONS as LPARAM,
    );
    set_switchbar_button_width(
        state.controls.switchbar,
        state.settings.settings().switchbar.buttons.pixels(),
    );
    // One line per label, cut with "..." when it is wider than its button.
    SendMessageW(state.controls.switchbar, TB_SETMAXTEXTROWS, 1, 0);
    let text_flags = DT_CENTER | DT_SINGLELINE | DT_END_ELLIPSIS;
    SendMessageW(
        state.controls.switchbar,
        TB_SETDRAWTEXTFLAGS,
        text_flags as WPARAM,
        text_flags as LPARAM,
    );

    // `layout_controls` shows it, as the settings say.
    state.controls.tree = create_child_window(
        hwnd,
        TREE_CLASS_NAME,
        "",
        WS_CHILD | WS_BORDER,
        0,
        TREE_ID,
        instance,
        null_mut(),
    )?;

    let mut mdi_client: CLIENTCREATESTRUCT = zeroed();
    // No MDI window list in the Window menu: the switchbar is the window
    // list, as in mIRC.
    mdi_client.hWindowMenu = null_mut();
    mdi_client.idFirstChild = 50000;
    state.controls.mdi_client = create_child_window(
        hwnd,
        MDI_CLASS_NAME,
        "",
        WS_CHILD | WS_VISIBLE | WS_BORDER,
        0,
        MDI_ID,
        instance,
        (&mut mdi_client as *mut CLIENTCREATESTRUCT).cast(),
    )?;

    state.controls.status_bar = CreateWindowExW(
        0,
        STATUSCLASSNAMEW,
        null(),
        WS_CHILD | WS_VISIBLE | SBARS_SIZEGRIP,
        0,
        0,
        0,
        0,
        hwnd,
        STATUS_BAR_ID as usize as HMENU,
        instance,
        null_mut(),
    );
    if state.controls.status_bar.is_null() {
        return Err(last_os_error("CreateWindowExW status bar"));
    }

    Ok(())
}

/// Gives every switchbar button `width` pixels, whatever its label.
unsafe fn set_switchbar_button_width(switchbar: HWND, width: i32) {
    // The minimum and the maximum width.
    SendMessageW(
        switchbar,
        TB_SETBUTTONWIDTH,
        0,
        pack_size_lparam(width, width),
    );
    // A wrapping toolbar starts a new row only once a button's bitmap would
    // pass the edge (comctl32's `TOOLBAR_WrapToolbar`), and the buttons have
    // no bitmap: a bitmap as wide as the button wraps the button itself, so
    // no row ends in a clipped button. 16 is the default bitmap height.
    SendMessageW(switchbar, TB_SETBITMAPSIZE, 0, pack_size_lparam(width, 16));
}

unsafe fn create_popup_menu() -> Result<HMENU, String> {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return Err(last_os_error("CreatePopupMenu"));
    }
    Ok(menu)
}

unsafe fn create_menu_from_entries(entries: &[MenuEntry]) -> Result<HMENU, String> {
    let menu = create_popup_menu()?;
    for entry in entries {
        match *entry {
            MenuEntry::Item { id, label } => append_menu_text(menu, MF_STRING, id, label)?,
            MenuEntry::Separator => {
                if AppendMenuW(menu, MF_SEPARATOR, 0, null()) == 0 {
                    return Err(last_os_error("AppendMenuW"));
                }
            }
        }
    }
    Ok(menu)
}

unsafe fn create_frame_menu(hwnd: HWND) -> Result<(), String> {
    let room_menu = create_popup_menu()?;
    append_menu_text(
        room_menu,
        MF_STRING,
        MENU_ID_ROOM_ACCEPT_INVITE,
        "Accept Invite",
    )?;
    append_menu_text(
        room_menu,
        MF_STRING,
        MENU_ID_ROOM_REJECT_INVITE,
        "Reject Invite",
    )?;
    append_menu_text(room_menu, MF_STRING, MENU_ID_ROOM_PART, "Part Room")?;
    if AppendMenuW(room_menu, MF_SEPARATOR, 0, null()) == 0 {
        return Err(last_os_error("AppendMenuW"));
    }
    let room_notifications_menu = create_popup_menu()?;
    for (_, id, label) in ROOM_NOTIFICATIONS_MENU {
        append_menu_text(room_notifications_menu, MF_STRING, id, label)?;
    }
    append_menu_text(
        room_menu,
        MF_POPUP,
        room_notifications_menu as usize,
        "&Notifications",
    )?;

    let message_menu = create_popup_menu()?;
    for (id, label) in [
        (MENU_ID_MESSAGE_REPLY, "Reply to Message"),
        (MENU_ID_MESSAGE_GO_TO_ORIGINAL, "Go to Original Message"),
        (MENU_ID_MESSAGE_EDIT, "Edit Message"),
        (MENU_ID_MESSAGE_REACT, "React to Message"),
        (MENU_ID_MESSAGE_REDACT, "Redact Message"),
        (MENU_ID_MESSAGE_SAVE_ATTACHMENT, "Save Attachment..."),
    ] {
        append_menu_text(message_menu, MF_STRING, id, label)?;
    }

    let member_menu = create_menu_from_entries(MEMBER_MENU)?;
    append_menu_text(
        member_menu,
        MF_POPUP,
        create_menu_from_entries(MEMBER_CONTROL_MENU)? as usize,
        "Control",
    )?;

    let frame_menu = CreateMenu();
    if frame_menu.is_null() {
        return Err(last_os_error("CreateMenu"));
    }
    let (mut file_menu, mut view_menu, mut tools_menu) = (null_mut(), null_mut(), null_mut());
    for (bar_menu, label) in MENU_BAR {
        let popup = match bar_menu {
            BarMenu::File => {
                file_menu = create_menu_from_entries(FILE_MENU)?;
                file_menu
            }
            BarMenu::View => {
                view_menu = create_menu_from_entries(VIEW_MENU)?;
                view_menu
            }
            BarMenu::Tools => {
                tools_menu = create_menu_from_entries(TOOLS_MENU)?;
                tools_menu
            }
            BarMenu::Commands => {
                let menu = create_popup_menu()?;
                for (index, item) in COMMANDS_MENU.iter().enumerate() {
                    append_menu_text(menu, MF_STRING, MENU_ID_COMMANDS_FIRST + index, item.label)?;
                }
                menu
            }
            BarMenu::Window => create_menu_from_entries(WINDOW_MENU)?,
            BarMenu::Help => create_menu_from_entries(HELP_MENU)?,
        };
        append_menu_text(frame_menu, MF_POPUP, popup as usize, label)?;
    }

    if SetMenu(hwnd, frame_menu) == 0 {
        return Err(last_os_error("SetMenu"));
    }
    let accelerators = [
        ACCEL {
            fVirt: FCONTROL | FVIRTKEY,
            key: u16::from(b'F'),
            cmd: MENU_ID_FIND as u16,
        },
        // Under Wine too, Alt+E arrives as WM_SYSKEYDOWN, which FVIRTKEY
        // with FALT matches.
        ACCEL {
            fVirt: FALT | FVIRTKEY,
            key: u16::from(b'E'),
            cmd: MENU_ID_FILE_SELECT_SERVER as u16,
        },
        // mIRC's List Channels.
        ACCEL {
            fVirt: FALT | FVIRTKEY,
            key: u16::from(b'L'),
            cmd: MENU_ID_COMMANDS_LIST as u16,
        },
        ACCEL {
            fVirt: FALT | FVIRTKEY,
            key: u16::from(b'O'),
            cmd: MENU_ID_TOOLS_OPTIONS as u16,
        },
    ];
    let accelerator_table =
        CreateAcceleratorTableW(accelerators.as_ptr(), accelerators.len() as i32);
    if accelerator_table.is_null() {
        return Err(last_os_error("CreateAcceleratorTableW"));
    }
    let controls = &mut state_mut(hwnd).controls;
    controls.file_menu = file_menu;
    controls.view_menu = view_menu;
    controls.tools_menu = tools_menu;
    controls.room_menu = room_menu;
    controls.room_notifications_menu = room_notifications_menu;
    controls.message_menu = message_menu;
    controls.member_menu = member_menu;
    controls.accelerators = accelerator_table;
    if DrawMenuBar(hwnd) == 0 {
        return Err(last_os_error("DrawMenuBar"));
    }
    Ok(())
}

fn window_menu_command_id(arrangement: MdiArrangement) -> usize {
    match arrangement {
        MdiArrangement::Cascade => MENU_ID_WINDOW_CASCADE,
        MdiArrangement::TileHorizontal => MENU_ID_WINDOW_TILE_HORIZONTAL,
        MdiArrangement::TileVertical => MENU_ID_WINDOW_TILE_VERTICAL,
        MdiArrangement::ArrangeIcons => MENU_ID_WINDOW_ARRANGE_ICONS,
    }
}

/// Arranges the MDI windows. MDI itself restores a maximized window before
/// cascading or tiling.
unsafe fn arrange_mdi_windows(hwnd: HWND, arrangement: MdiArrangement) {
    let mdi_client = state_ref(hwnd).controls.mdi_client;
    let (message, wparam) = match arrangement {
        MdiArrangement::Cascade => (WM_MDICASCADE, 0),
        MdiArrangement::TileHorizontal => (WM_MDITILE, MDITILE_HORIZONTAL as WPARAM),
        MdiArrangement::TileVertical => (WM_MDITILE, MDITILE_VERTICAL as WPARAM),
        MdiArrangement::ArrangeIcons => (WM_MDIICONARRANGE, 0),
    };
    SendMessageW(mdi_client, message, wparam, 0);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RoomActionState {
    accept_invite_enabled: bool,
    reject_invite_enabled: bool,
    part_room_enabled: bool,
    /// The active joined room's mode, which Notifications checks; `None`
    /// greys Notifications (an invite, or no room).
    notification_mode: Option<RoomNotificationMode>,
}

fn active_room_action_state(state: &WindowState) -> RoomActionState {
    let joined_room = resolve_joined_room_target(state, None);
    RoomActionState {
        accept_invite_enabled: resolve_invite_command_room_id(state, "accept", None).is_ok(),
        reject_invite_enabled: resolve_invite_command_room_id(state, "reject", None).is_ok(),
        part_room_enabled: joined_room.is_ok(),
        notification_mode: joined_room.ok().map(|room_id| {
            joined_room_notification_mode(
                sendable_room_by_id(&state.model.rooms, &room_id)
                    .expect("the joined room target is a joined room"),
            )
        }),
    }
}

/// Notifications checks the room's mode, or nothing while it is greyed.
unsafe fn refresh_room_notifications_menu_state(
    hwnd: HWND,
    popup_menu: HMENU,
) -> Result<(), String> {
    let mode = active_room_action_state(state_ref(hwnd)).notification_mode;
    for (_, item_id, _) in ROOM_NOTIFICATIONS_MENU {
        let flags = MF_BYCOMMAND
            | if mode.is_some() {
                MF_ENABLED
            } else {
                MF_GRAYED
            };
        if EnableMenuItem(popup_menu, item_id as u32, flags) == -1 {
            return Err(last_os_error("EnableMenuItem"));
        }
    }
    match mode {
        Some(mode) => {
            let [(_, first, _), _, (_, last, _)] = ROOM_NOTIFICATIONS_MENU;
            if CheckMenuRadioItem(
                popup_menu,
                first as u32,
                last as u32,
                notification_menu_id(mode) as u32,
                MF_BYCOMMAND,
            ) == 0
            {
                return Err(last_os_error("CheckMenuRadioItem"));
            }
        }
        None => {
            for (_, item_id, _) in ROOM_NOTIFICATIONS_MENU {
                if CheckMenuItem(popup_menu, item_id as u32, MF_BYCOMMAND | MF_UNCHECKED)
                    == u32::MAX
                {
                    return Err(format!("CheckMenuItem: menu item {item_id} does not exist"));
                }
            }
        }
    }
    Ok(())
}

unsafe fn refresh_room_menu_state(hwnd: HWND, popup_menu: HMENU) -> Result<(), String> {
    let action_state = active_room_action_state(state_ref(hwnd));
    for (item_id, enabled) in [
        (
            MENU_ID_ROOM_ACCEPT_INVITE,
            action_state.accept_invite_enabled,
        ),
        (
            MENU_ID_ROOM_REJECT_INVITE,
            action_state.reject_invite_enabled,
        ),
        (MENU_ID_ROOM_PART, action_state.part_room_enabled),
    ] {
        let flags = MF_BYCOMMAND | if enabled { MF_ENABLED } else { MF_GRAYED };
        if EnableMenuItem(popup_menu, item_id as u32, flags) == -1 {
            return Err(last_os_error("EnableMenuItem"));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MessageActionState {
    selected_event_id: Option<String>,
    /// The selected message is the user's own text or emote.
    edit_enabled: bool,
    save_attachment_enabled: bool,
    /// The selected message is a reply with a quote line.
    go_to_original_enabled: bool,
}

/// The message the selected reply quotes, if it has a quote line.
fn selected_message_quote_target<'a>(
    state: &'a WindowState,
    selected_event_id: Option<&str>,
) -> Option<&'a str> {
    window_message(&state.model, &state.active_view, selected_event_id?)?
        .relation
        .as_ref()
        .and_then(replies::reply_quote_target)
}

fn selected_message_editable(state: &WindowState, selected_event_id: Option<&str>) -> bool {
    selected_event_id
        .and_then(|event_id| window_message(&state.model, &state.active_view, event_id))
        .is_some_and(|activity| {
            replies::editable_message(activity, state.model.own_user_id.as_deref()).is_ok()
        })
}

fn selected_timeline_media_attachment(state: &WindowState) -> Option<SelectedMediaAttachment> {
    let selected = state.selected_timeline_event.as_ref()?;
    if selected.view_id != state.active_view {
        return None;
    }
    let room_id = match &state.active_view {
        ViewId::Room(room_id) => room_id,
        ViewId::Thread { room_id, .. } => room_id,
        ViewId::Status | ViewId::RoomList => return None,
    };
    let activity = state
        .model
        .room_views
        .get(room_id)?
        .activities
        .iter()
        .find(|activity| match &activity.kind {
            RoomActivityKind::File(file) => file.event_id == selected.event_id,
            RoomActivityKind::Audio(audio) => audio.event_id == selected.event_id,
            RoomActivityKind::Video(video) => video.event_id == selected.event_id,
            _ => false,
        })?;
    let (event_id, filename, source) = match &activity.kind {
        RoomActivityKind::File(file) => {
            (&file.event_id, file.filename.clone(), file.source.clone())
        }
        RoomActivityKind::Audio(audio) => (
            &audio.event_id,
            audio.filename.clone(),
            audio.source.clone(),
        ),
        RoomActivityKind::Video(video) => (
            &video.event_id,
            video.filename.clone(),
            video.source.clone(),
        ),
        _ => return None,
    };
    Some(SelectedMediaAttachment {
        room_id: room_id.clone(),
        activity_id: activity.activity_id.clone(),
        event_id: event_id.clone(),
        filename,
        source,
    })
}

/// A row is selected only when there is a selection and the row is that
/// event. Rows without an event id (opening lines, typing, backfill status)
/// are never selected, even when nothing is selected.
fn timeline_row_is_selected(row_event_id: Option<&str>, selected_event_id: Option<&str>) -> bool {
    selected_event_id.is_some_and(|selected| row_event_id == Some(selected))
}

fn selected_timeline_event_id(state: &WindowState) -> Option<&str> {
    state.selected_timeline_event.as_ref().and_then(|selected| {
        (selected.view_id == state.active_view).then_some(selected.event_id.as_str())
    })
}

unsafe fn selected_timeline_event_id_for_automation(hwnd: HWND) -> Option<String> {
    let state = state_ref(hwnd);
    let selected = state.selected_timeline_event.as_ref()?;
    if selected.view_id != state.active_view {
        return None;
    }
    let content = state.views.get(&state.active_view)?.content;
    timeline_state_ref(room_content_timeline(content))
        .source_rows
        .iter()
        .any(|row| row.event_id.as_deref() == Some(&selected.event_id))
        .then(|| selected.event_id.clone())
}

#[cfg(test)]
fn active_message_action_state(state: &WindowState) -> MessageActionState {
    let selected_event_id = match state.active_view {
        ViewId::Room(_) | ViewId::Thread { .. } => {
            selected_timeline_event_id(state).map(str::to_owned)
        }
        ViewId::Status | ViewId::RoomList => None,
    };
    let save_attachment_enabled = selected_timeline_media_attachment(state).is_some();
    MessageActionState {
        edit_enabled: selected_message_editable(state, selected_event_id.as_deref()),
        go_to_original_enabled: selected_message_quote_target(state, selected_event_id.as_deref())
            .is_some(),
        selected_event_id,
        save_attachment_enabled,
    }
}

unsafe fn active_message_action_state_for_automation(hwnd: HWND) -> MessageActionState {
    let selected_event_id = match state_ref(hwnd).active_view {
        ViewId::Room(_) | ViewId::Thread { .. } => selected_timeline_event_id_for_automation(hwnd),
        ViewId::Status | ViewId::RoomList => None,
    };
    let state = state_ref(hwnd);
    let save_attachment_enabled = selected_timeline_media_attachment(state).is_some();
    MessageActionState {
        edit_enabled: selected_message_editable(state, selected_event_id.as_deref()),
        go_to_original_enabled: selected_message_quote_target(state, selected_event_id.as_deref())
            .is_some(),
        selected_event_id,
        save_attachment_enabled,
    }
}

unsafe fn refresh_message_menu_state(hwnd: HWND, popup_menu: HMENU) -> Result<(), String> {
    let action_state = active_message_action_state_for_automation(hwnd);
    let flags = MF_BYCOMMAND
        | if action_state.selected_event_id.is_some() {
            MF_ENABLED
        } else {
            MF_GRAYED
        };
    for item_id in [
        MENU_ID_MESSAGE_REPLY,
        MENU_ID_MESSAGE_REACT,
        MENU_ID_MESSAGE_REDACT,
    ] {
        if EnableMenuItem(popup_menu, item_id as u32, flags) == -1 {
            return Err(last_os_error("EnableMenuItem"));
        }
    }
    let flags = MF_BYCOMMAND
        | if action_state.edit_enabled {
            MF_ENABLED
        } else {
            MF_GRAYED
        };
    if EnableMenuItem(popup_menu, MENU_ID_MESSAGE_EDIT as u32, flags) == -1 {
        return Err(last_os_error("EnableMenuItem"));
    }
    let flags = MF_BYCOMMAND
        | if action_state.save_attachment_enabled {
            MF_ENABLED
        } else {
            MF_GRAYED
        };
    if EnableMenuItem(popup_menu, MENU_ID_MESSAGE_SAVE_ATTACHMENT as u32, flags) == -1 {
        return Err(last_os_error("EnableMenuItem"));
    }
    let flags = MF_BYCOMMAND
        | if action_state.go_to_original_enabled {
            MF_ENABLED
        } else {
            MF_GRAYED
        };
    if EnableMenuItem(popup_menu, MENU_ID_MESSAGE_GO_TO_ORIGINAL as u32, flags) == -1 {
        return Err(last_os_error("EnableMenuItem"));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MemberMenuActionState {
    /// Each member menu item, and whether it applies.
    items: Vec<(usize, bool)>,
    selected_user_id: Option<String>,
    selected_label: Option<String>,
}

impl MemberMenuActionState {
    fn enabled(&self, id: usize) -> bool {
        self.items
            .iter()
            .find(|(item, _)| *item == id)
            .unwrap_or_else(|| panic!("{id} is not a member menu item"))
            .1
    }
}

/// The members of the active view's room, in nick list order; a thread
/// shares its room's members and the status view has none.
fn active_view_members(state: &WindowState) -> &[RoomMemberSummary] {
    let room_id = match &state.active_view {
        ViewId::Room(room_id) | ViewId::Thread { room_id, .. } => room_id,
        ViewId::Status | ViewId::RoomList => return &[],
    };
    state
        .model
        .room_views
        .get(room_id)
        .map(|view| view.members.as_slice())
        .unwrap_or_default()
}

unsafe fn active_member_rows(hwnd: HWND) -> Vec<RoomMemberView> {
    room_member_view_rows(active_view_members(state_ref(hwnd)))
}

unsafe fn active_member_list(hwnd: HWND) -> Option<HWND> {
    match &state_ref(hwnd).active_view {
        ViewId::Status | ViewId::RoomList => None,
        ViewId::Room(_) | ViewId::Thread { .. } => state_ref(hwnd)
            .views
            .get(&state_ref(hwnd).active_view)
            .map(|view| room_content_member_list(view.content)),
    }
}

/// The nick list's selected member: the list shows the active view's
/// members in order.
unsafe fn selected_member(hwnd: HWND) -> Option<RoomMemberSummary> {
    let list = active_member_list(hwnd)?;
    let selected = SendMessageW(list, LB_GETCURSEL, 0, 0);
    if selected == LB_ERR as isize {
        return None;
    }
    active_view_members(state_ref(hwnd))
        .get(selected as usize)
        .cloned()
}

unsafe fn active_member_action_state(hwnd: HWND) -> MemberMenuActionState {
    let selected = selected_member(hwnd);
    let model = &state_ref(hwnd).model;
    MemberMenuActionState {
        items: member_menu_item_ids()
            .map(|id| {
                let enabled = selected.as_ref().is_some_and(|member| {
                    member_menu_item_enabled(
                        id,
                        member,
                        model.ignored_users.as_deref(),
                        model.own_user_id.as_deref(),
                    )
                });
                (id, enabled)
            })
            .collect(),
        selected_user_id: selected.as_ref().map(|member| member.user_id.clone()),
        selected_label: selected.as_ref().map(room_member_nick),
    }
}

unsafe fn refresh_member_menu_state(hwnd: HWND, popup_menu: HMENU) -> Result<(), String> {
    let action_state = active_member_action_state(hwnd);
    for (item, enabled) in action_state.items {
        let flags = MF_BYCOMMAND | if enabled { MF_ENABLED } else { MF_GRAYED };
        if EnableMenuItem(popup_menu, item as u32, flags) == -1 {
            return Err(last_os_error("EnableMenuItem"));
        }
    }
    Ok(())
}

unsafe fn select_member_for_automation(hwnd: HWND, user_id: &str) -> Result<(), String> {
    let index = active_member_rows(hwnd)
        .iter()
        .position(|member| member.user_id == user_id)
        .ok_or_else(|| format!("unknown_member: {user_id}"))?;
    let list = active_member_list(hwnd).ok_or_else(|| "member list is unavailable".to_owned())?;
    let selected = SendMessageW(list, LB_GETCURSEL, 0, 0);
    if selected == LB_ERR as isize {
        if SendMessageW(list, LB_SETCURSEL, index, 0) == LB_ERR as isize {
            return Err("member selection failed".to_owned());
        }
    } else {
        let key = if index > selected as usize {
            VK_DOWN_KEY
        } else {
            VK_UP_KEY
        };
        for _ in 0..index.abs_diff(selected as usize) {
            SendMessageW(list, WM_KEYDOWN, key, 0);
        }
    }
    Ok(())
}

unsafe fn append_menu_text(
    menu: HMENU,
    flags: u32,
    item_id: usize,
    text: &str,
) -> Result<(), String> {
    let wide = wide_null(text);
    if AppendMenuW(menu, flags, item_id, wide.as_ptr()) == 0 {
        return Err(last_os_error("AppendMenuW"));
    }
    Ok(())
}

/// Updates a menu's items before it opens, as `WM_INITMENUPOPUP` and the
/// automation dumps do. Other menus (Commands, Window, Help, the window
/// menu) have nothing that greys.
unsafe fn refresh_menu_popup(hwnd: HWND, popup_menu: HMENU) -> Result<(), String> {
    let controls = &state_ref(hwnd).controls;
    if popup_menu == controls.file_menu {
        refresh_file_menu_state(hwnd, popup_menu)
    } else if popup_menu == controls.view_menu {
        refresh_view_menu_checks(hwnd, popup_menu)
    } else if popup_menu == controls.tools_menu {
        refresh_tools_menu_state(hwnd, popup_menu)
    } else if popup_menu == controls.room_menu {
        refresh_room_menu_state(hwnd, popup_menu)
    } else if popup_menu == controls.room_notifications_menu {
        refresh_room_notifications_menu_state(hwnd, popup_menu)
    } else if popup_menu == controls.message_menu {
        refresh_message_menu_state(hwnd, popup_menu)
    } else {
        Ok(())
    }
}

unsafe fn refresh_file_menu_state(hwnd: HWND, popup_menu: HMENU) -> Result<(), String> {
    let state = file_menu_state(state_ref(hwnd).model.connection);
    for (item, enabled) in [
        (MENU_ID_FILE_CONNECT, state.connect),
        (MENU_ID_FILE_DISCONNECT, state.disconnect),
        (MENU_ID_FILE_SELECT_SERVER, state.select_server),
        (MENU_ID_LOGOUT, state.log_out),
    ] {
        let flags = MF_BYCOMMAND | if enabled { MF_ENABLED } else { MF_GRAYED };
        if EnableMenuItem(popup_menu, item as u32, flags) == -1 {
            return Err(last_os_error("EnableMenuItem"));
        }
    }
    Ok(())
}

unsafe fn refresh_view_menu_checks(hwnd: HWND, popup_menu: HMENU) -> Result<(), String> {
    for (item, checked) in view_menu_checks(state_ref(hwnd).settings.settings()) {
        let flags = MF_BYCOMMAND | if checked { MF_CHECKED } else { MF_UNCHECKED };
        if CheckMenuItem(popup_menu, item as u32, flags) == u32::MAX {
            return Err(format!("CheckMenuItem: menu item {item} does not exist"));
        }
    }
    Ok(())
}

unsafe fn refresh_tools_menu_state(hwnd: HWND, popup_menu: HMENU) -> Result<(), String> {
    let last_verification = state_ref(hwnd).model.last_verification.as_ref();

    let accept_flags = MF_BYCOMMAND
        | if verification_menu_action_enabled(last_verification, VerificationAction::Accept) {
            MF_ENABLED
        } else {
            MF_GRAYED
        };
    if EnableMenuItem(popup_menu, MENU_ID_VERIFY_ACCEPT as u32, accept_flags) == -1 {
        return Err(last_os_error("EnableMenuItem"));
    }

    let cancel_flags = MF_BYCOMMAND
        | if verification_menu_action_enabled(last_verification, VerificationAction::Cancel) {
            MF_ENABLED
        } else {
            MF_GRAYED
        };
    if EnableMenuItem(popup_menu, MENU_ID_VERIFY_CANCEL as u32, cancel_flags) == -1 {
        return Err(last_os_error("EnableMenuItem"));
    }

    let confirm_flags = MF_BYCOMMAND
        | if verification_menu_action_enabled(last_verification, VerificationAction::Confirm) {
            MF_ENABLED
        } else {
            MF_GRAYED
        };
    if EnableMenuItem(popup_menu, MENU_ID_VERIFY_CONFIRM_SAS as u32, confirm_flags) == -1 {
        return Err(last_os_error("EnableMenuItem"));
    }

    let reject_flags = MF_BYCOMMAND
        | if verification_menu_action_enabled(last_verification, VerificationAction::Reject) {
            MF_ENABLED
        } else {
            MF_GRAYED
        };
    if EnableMenuItem(popup_menu, MENU_ID_VERIFY_REJECT_SAS as u32, reject_flags) == -1 {
        return Err(last_os_error("EnableMenuItem"));
    }

    Ok(())
}

unsafe fn select_timeline_event_for_automation(hwnd: HWND, event_id: &str) -> Result<(), String> {
    let active_view = state_ref(hwnd).active_view.clone();
    if !matches!(active_view, ViewId::Room(_) | ViewId::Thread { .. }) {
        return Err("selected_message_unavailable: active view is not a room or thread".to_owned());
    }
    let content = state_ref(hwnd)
        .views
        .get(&active_view)
        .ok_or_else(|| "selected_message_unavailable: active view is unavailable".to_owned())?
        .content;
    let timeline = room_content_timeline(content);
    if !timeline_state_ref(timeline)
        .source_rows
        .iter()
        .any(|row| row.event_id.as_deref() == Some(event_id))
    {
        return Err(format!("unknown_event: {event_id}"));
    }
    state_mut(hwnd).selected_timeline_event = Some(SelectedTimelineEvent {
        view_id: active_view,
        event_id: event_id.to_owned(),
    });
    InvalidateRect(timeline, null(), 1);
    Ok(())
}

unsafe fn window_menu_command(
    hwnd: HWND,
    arrangement: MdiArrangement,
) -> (&'static str, Result<Option<()>, String>) {
    arrange_mdi_windows(hwnd, arrangement);
    ("Window", Ok(Some(())))
}

unsafe fn menu_command_result(
    hwnd: HWND,
    wparam: WPARAM,
) -> Option<(&'static str, Result<Option<()>, String>)> {
    let command_id = wparam & 0xffff;
    let (title, result) = match command_id {
        MENU_ID_FIND => ("Find", begin_find_dialog(hwnd).map(|_| Some(()))),
        MENU_ID_WINDOW_CASCADE => window_menu_command(hwnd, MdiArrangement::Cascade),
        MENU_ID_WINDOW_TILE_HORIZONTAL => window_menu_command(hwnd, MdiArrangement::TileHorizontal),
        MENU_ID_WINDOW_TILE_VERTICAL => window_menu_command(hwnd, MdiArrangement::TileVertical),
        MENU_ID_WINDOW_ARRANGE_ICONS => window_menu_command(hwnd, MdiArrangement::ArrangeIcons),
        MENU_ID_ROOM_ACCEPT_INVITE => (
            "Room",
            submit_room_menu_action(hwnd, submit_accept_slash_command).map(|_| Some(())),
        ),
        MENU_ID_ROOM_REJECT_INVITE => (
            "Room",
            submit_room_menu_action(hwnd, submit_reject_slash_command).map(|_| Some(())),
        ),
        MENU_ID_ROOM_PART => (
            "Room",
            submit_room_menu_action(hwnd, submit_part_slash_command).map(|_| Some(())),
        ),
        MENU_ID_ROOM_NOTIFY_ALL | MENU_ID_ROOM_NOTIFY_MENTIONS | MENU_ID_ROOM_NOTIFY_MUTE => {
            let mode = notification_menu_mode(command_id).expect("a Notifications item");
            (
                "Room",
                submit_room_menu_action(hwnd, |state, target| {
                    submit_room_notification_mode(state, target, mode)
                })
                .map(|_| Some(())),
            )
        }
        MENU_ID_MESSAGE_REPLY => (
            "Message",
            arm_compose_target_on_selected_message(hwnd, |event_id| ComposeTarget::Reply {
                event_id,
            })
            .map(|_| Some(())),
        ),
        MENU_ID_MESSAGE_GO_TO_ORIGINAL => (
            "Message",
            go_to_original_of_selected_message(hwnd).map(|_| Some(())),
        ),
        MENU_ID_MESSAGE_EDIT => (
            "Message",
            arm_compose_target_on_selected_message(hwnd, |event_id| ComposeTarget::Edit {
                event_id,
            })
            .map(|_| Some(())),
        ),
        MENU_ID_MESSAGE_REACT => (
            "Message",
            prefill_react_to_selected_message(hwnd).map(|_| Some(())),
        ),
        MENU_ID_MESSAGE_REDACT => (
            "Message",
            submit_redact_selected_message(hwnd).map(|_| Some(())),
        ),
        MENU_ID_MESSAGE_SAVE_ATTACHMENT => (
            "Message",
            save_selected_message_attachment(hwnd).map(|_| Some(())),
        ),
        MENU_ID_MEMBER_QUERY => ("Member", query_selected_member(hwnd).map(|_| Some(()))),
        MENU_ID_MEMBER_MENTION => ("Member", mention_selected_member(hwnd).map(|_| Some(()))),
        MENU_ID_MEMBER_WHOIS
        | MENU_ID_MEMBER_IGNORE
        | MENU_ID_MEMBER_UNIGNORE
        | MENU_ID_MEMBER_OP
        | MENU_ID_MEMBER_DEOP
        | MENU_ID_MEMBER_VOICE
        | MENU_ID_MEMBER_DEVOICE
        | MENU_ID_MEMBER_KICK
        | MENU_ID_MEMBER_BAN => (
            "Member",
            run_selected_member_command(hwnd, command_id).map(|_| Some(())),
        ),
        MENU_ID_FILE_CONNECT => (
            "File",
            connect(hwnd, None).map(|performed| performed.then_some(())),
        ),
        MENU_ID_FILE_DISCONNECT => ("File", disconnect(hwnd).map(|_| Some(()))),
        MENU_ID_FILE_SELECT_SERVER => (
            "File",
            show_select_server_dialog(hwnd, "").map(|performed| performed.then_some(())),
        ),
        MENU_ID_FILE_EXIT => (
            "File",
            run_local_command(hwnd, SlashCommand::Exit).map(|_| Some(())),
        ),
        MENU_ID_HELP_TOPICS => (
            "Help",
            run_local_command(hwnd, SlashCommand::Help { topic: None }).map(|_| Some(())),
        ),
        MENU_ID_HELP_ABOUT => ("Help", show_about_dialog(hwnd).map(|()| Some(()))),
        MENU_ID_TOOLS_OPTIONS => ("Options", run_options_dialog(hwnd).map(|()| Some(()))),
        MENU_ID_VIEW_SWITCHBAR | MENU_ID_VIEW_TREEBAR => {
            let bar = view_menu_bar(command_id).expect("View items show and hide a bar");
            let command = if state_ref(hwnd).settings.settings().bar_shown(bar) {
                BarCommand::Off
            } else {
                BarCommand::On
            };
            (
                "View",
                run_local_command(hwnd, SlashCommand::Bar { bar, command }).map(|_| Some(())),
            )
        }
        MENU_ID_RECOVER_E2EE => (
            "Tools",
            show_recover_e2ee_menu_dialog(hwnd).map(|handled| handled.then_some(())),
        ),
        MENU_ID_SETUP_E2EE_RECOVERY => (
            "Tools",
            show_setup_e2ee_menu_dialog(hwnd).map(|handled| handled.then_some(())),
        ),
        MENU_ID_VERIFY_ACCEPT => (
            "Tools",
            submit_menu_verification_action(hwnd, VerificationAction::Accept).map(|_| Some(())),
        ),
        MENU_ID_VERIFY_CANCEL => (
            "Tools",
            submit_menu_verification_action(hwnd, VerificationAction::Cancel).map(|_| Some(())),
        ),
        MENU_ID_VERIFY_CONFIRM_SAS => (
            "Tools",
            submit_menu_verification_action(hwnd, VerificationAction::Confirm).map(|_| Some(())),
        ),
        MENU_ID_VERIFY_REJECT_SAS => (
            "Tools",
            submit_menu_verification_action(hwnd, VerificationAction::Reject).map(|_| Some(())),
        ),
        MENU_ID_LOGOUT => ("File", submit_logout(state_mut(hwnd)).map(|_| Some(()))),
        _ => {
            let item = commands_menu_item(command_id)?;
            (
                "Commands",
                run_commands_menu_item(hwnd, item).map(|performed| performed.then_some(())),
            )
        }
    };

    Some((title, result))
}

unsafe fn handle_menu_command(hwnd: HWND, wparam: WPARAM) -> Option<Result<(), String>> {
    let (title, result) = menu_command_result(hwnd, wparam)?;

    if let Err(message) = &result {
        show_error_message(hwnd, title, message);
    }

    Some(
        result
            .and_then(|handled| handled.ok_or_else(|| "menu action was not performed".to_owned())),
    )
}

unsafe fn invoke_menu_command_for_automation(hwnd: HWND, command_id: usize) -> Result<(), String> {
    let room_action_enabled = match command_id {
        MENU_ID_ROOM_ACCEPT_INVITE => {
            active_room_action_state(state_ref(hwnd)).accept_invite_enabled
        }
        MENU_ID_ROOM_REJECT_INVITE => {
            active_room_action_state(state_ref(hwnd)).reject_invite_enabled
        }
        MENU_ID_ROOM_PART => active_room_action_state(state_ref(hwnd)).part_room_enabled,
        MENU_ID_ROOM_NOTIFY_ALL | MENU_ID_ROOM_NOTIFY_MENTIONS | MENU_ID_ROOM_NOTIFY_MUTE => {
            active_room_action_state(state_ref(hwnd))
                .notification_mode
                .is_some()
        }
        _ => true,
    };
    if !room_action_enabled {
        return Err("room menu action is disabled".to_owned());
    }
    let Some((_, result)) = menu_command_result(hwnd, command_id) else {
        return Err(format!("unknown menu command {command_id}"));
    };
    result.and_then(|handled| handled.ok_or_else(|| "menu action was not performed".to_owned()))
}

fn context_menu_command_id(command: &ContextMenuCommand) -> Option<usize> {
    Some(match command {
        ContextMenuCommand::AcceptInvite => MENU_ID_ROOM_ACCEPT_INVITE,
        ContextMenuCommand::RejectInvite => MENU_ID_ROOM_REJECT_INVITE,
        ContextMenuCommand::PartRoom => MENU_ID_ROOM_PART,
        ContextMenuCommand::NotifyAllMessages => MENU_ID_ROOM_NOTIFY_ALL,
        ContextMenuCommand::NotifyMentionsAndKeywords => MENU_ID_ROOM_NOTIFY_MENTIONS,
        ContextMenuCommand::NotifyMute => MENU_ID_ROOM_NOTIFY_MUTE,
        ContextMenuCommand::ReplyMessage => MENU_ID_MESSAGE_REPLY,
        ContextMenuCommand::GoToOriginal => MENU_ID_MESSAGE_GO_TO_ORIGINAL,
        ContextMenuCommand::EditMessage => MENU_ID_MESSAGE_EDIT,
        ContextMenuCommand::ReactMessage => MENU_ID_MESSAGE_REACT,
        ContextMenuCommand::RedactMessage => MENU_ID_MESSAGE_REDACT,
        ContextMenuCommand::SaveAttachment => MENU_ID_MESSAGE_SAVE_ATTACHMENT,
        ContextMenuCommand::WhoisMember => MENU_ID_MEMBER_WHOIS,
        ContextMenuCommand::QueryMember => MENU_ID_MEMBER_QUERY,
        ContextMenuCommand::MentionMember => MENU_ID_MEMBER_MENTION,
        ContextMenuCommand::IgnoreMember => MENU_ID_MEMBER_IGNORE,
        ContextMenuCommand::UnignoreMember => MENU_ID_MEMBER_UNIGNORE,
        ContextMenuCommand::OpMember => MENU_ID_MEMBER_OP,
        ContextMenuCommand::DeopMember => MENU_ID_MEMBER_DEOP,
        ContextMenuCommand::VoiceMember => MENU_ID_MEMBER_VOICE,
        ContextMenuCommand::DevoiceMember => MENU_ID_MEMBER_DEVOICE,
        ContextMenuCommand::KickMember => MENU_ID_MEMBER_KICK,
        ContextMenuCommand::BanMember => MENU_ID_MEMBER_BAN,
    })
}

unsafe fn invoke_navigation_context_menu_for_automation(
    hwnd: HWND,
    command: &ContextMenuCommand,
) -> Result<(), String> {
    let command_id = context_menu_command_id(command)
        .ok_or_else(|| "room context menu command is unavailable".to_owned())?;
    if !matches!(
        command_id,
        MENU_ID_ROOM_ACCEPT_INVITE
            | MENU_ID_ROOM_REJECT_INVITE
            | MENU_ID_ROOM_PART
            | MENU_ID_ROOM_NOTIFY_ALL
            | MENU_ID_ROOM_NOTIFY_MENTIONS
            | MENU_ID_ROOM_NOTIFY_MUTE
    ) {
        return Err("room context menu command is unavailable".to_owned());
    }
    show_navigation_room_context_menu(hwnd, -1, Some(command_id))
}

unsafe fn invoke_timeline_context_menu_for_automation(
    hwnd: HWND,
    command: &ContextMenuCommand,
    event_id: Option<&str>,
) -> Result<(), String> {
    let command_id = context_menu_command_id(command)
        .ok_or_else(|| "message context menu command is unavailable".to_owned())?;
    if !matches!(
        command_id,
        MENU_ID_MESSAGE_REPLY
            | MENU_ID_MESSAGE_GO_TO_ORIGINAL
            | MENU_ID_MESSAGE_EDIT
            | MENU_ID_MESSAGE_REACT
            | MENU_ID_MESSAGE_REDACT
            | MENU_ID_MESSAGE_SAVE_ATTACHMENT
    ) {
        return Err("message context menu command is unavailable".to_owned());
    }
    let active_view = state_ref(hwnd).active_view.clone();
    let content = state_ref(hwnd)
        .views
        .get(&active_view)
        .ok_or_else(|| "message context menu target is unavailable".to_owned())?
        .content;
    let timeline = view_timeline(&active_view, content);
    let lparam = match event_id {
        Some(event_id) => timeline_event_context_lparam_for_automation(timeline, event_id)?,
        None => -1,
    };
    show_timeline_message_context_menu(timeline, lparam, Some(command_id))
}

unsafe fn timeline_event_context_lparam_for_automation(
    timeline: HWND,
    event_id: &str,
) -> Result<LPARAM, String> {
    let state = timeline_state_ref(timeline);
    let line_index = state
        .paint_lines
        .iter()
        .position(|line| {
            line.source_row_index
                .and_then(|index| state.source_rows.get(index))
                .and_then(|row| row.event_id.as_deref())
                == Some(event_id)
        })
        .ok_or_else(|| format!("unknown_event: {event_id}"))?;
    let y = state.vertical_padding
        + (line_index.saturating_sub(state.top_line) as i32 * state.line_height)
        + state.line_height / 2;
    let mut point = POINT { x: 1, y };
    if ClientToScreen(timeline, &mut point) == 0 {
        return Err(last_os_error("ClientToScreen"));
    }
    Ok(((point.y as u32 as usize) << 16 | (point.x as u16 as usize)) as LPARAM)
}

unsafe fn submit_room_menu_action(
    hwnd: HWND,
    submit: impl FnOnce(&mut WindowState, Option<&str>) -> Result<String, String>,
) -> Result<(), String> {
    let result = submit(state_mut(hwnd), None);
    match result {
        Ok(message) => set_local_command_status(hwnd, message),
        Err(message) => {
            set_local_command_status(hwnd, message.clone())?;
            Err(message)
        }
    }
}

/// Message > Reply to Message and Edit Message: arm the editbox with the
/// selected message.
unsafe fn arm_compose_target_on_selected_message(
    hwnd: HWND,
    target: fn(String) -> ComposeTarget,
) -> Result<(), String> {
    let Some(event_id) = selected_timeline_event_id_for_automation(hwnd) else {
        set_local_command_status(hwnd, "no message selected".to_owned())?;
        return Err("no message selected".to_owned());
    };
    if let Err(message) = arm_compose_target(hwnd, target(event_id)) {
        set_local_command_status(hwnd, message.clone())?;
        return Err(message);
    }
    Ok(())
}

/// Arms the active window's editbox with `target` and focuses it. A reply
/// keeps what was typed; an edit replaces it with the message as one would
/// type it.
unsafe fn arm_compose_target(hwnd: HWND, target: ComposeTarget) -> Result<(), String> {
    let edit_text = {
        let state = state_ref(hwnd);
        let activity = window_message(&state.model, &state.active_view, target.event_id())
            .ok_or_else(|| "no such message in this window".to_owned())?;
        match &target {
            ComposeTarget::Reply { .. } => None,
            ComposeTarget::Edit { .. } => Some(replies::compose_text_for_edit(
                replies::editable_message(activity, state.model.own_user_id.as_deref())?,
            )),
        }
    };
    let view_id = state_ref(hwnd).active_view.clone();
    state_mut(hwnd)
        .views
        .get_mut(&view_id)
        .expect("the active view has a window")
        .compose_mut()
        .target = Some(target);
    let edit = compose_edit(hwnd);
    match edit_text {
        Some(text) => set_compose_text(hwnd, &text)?,
        None => {
            let len = read_window_text(edit).encode_utf16().count();
            SendMessageW(edit, EM_SETSEL, len, len as LPARAM);
        }
    }
    refresh_view_content(hwnd, &view_id)?;
    SetFocus(edit);
    Ok(())
}

/// Esc in the editbox: disarms it; an edit also drops the old text it put
/// there.
unsafe fn cancel_compose_target(hwnd: HWND) -> Result<(), String> {
    let view_id = state_ref(hwnd).active_view.clone();
    let target = state_mut(hwnd)
        .views
        .get_mut(&view_id)
        .expect("the active view has a window")
        .compose_mut()
        .target
        .take();
    match target {
        None => return Ok(()),
        Some(ComposeTarget::Edit { .. }) => set_compose_text(hwnd, "")?,
        Some(ComposeTarget::Reply { .. }) => {}
    }
    refresh_view_content(hwnd, &view_id)
}

unsafe fn prefill_react_to_selected_message(hwnd: HWND) -> Result<(), String> {
    let Some(event_id) = selected_timeline_event_id_for_automation(hwnd) else {
        set_local_command_status(hwnd, "no message selected".to_owned())?;
        return Err("no message selected".to_owned());
    };
    set_compose_text(hwnd, &react_compose_text(&event_id))?;
    SetFocus(compose_edit(hwnd));
    Ok(())
}

fn react_compose_text(event_id: &str) -> String {
    format!("/react {event_id} ")
}

unsafe fn submit_redact_selected_message(hwnd: HWND) -> Result<(), String> {
    let Some(event_id) = selected_timeline_event_id_for_automation(hwnd) else {
        set_local_command_status(hwnd, "no message selected".to_owned())?;
        return Err("no message selected".to_owned());
    };
    let result = submit_redact_slash_command(state_mut(hwnd), event_id);
    match result {
        Ok(message) => set_local_command_status(hwnd, message),
        Err(message) => {
            set_local_command_status(hwnd, message.clone())?;
            Err(message)
        }
    }
}

unsafe fn handle_switchbar_command(hwnd: HWND, wparam: WPARAM) -> bool {
    let command_id = wparam & 0xffff;
    if !(SWITCHBAR_COMMAND_FIRST..=SWITCHBAR_COMMAND_LAST).contains(&command_id) {
        return false;
    }

    let index = command_id - SWITCHBAR_COMMAND_FIRST;
    let Some(view_id) = state_ref(hwnd).switchbar_views.get(index).cloned() else {
        return false;
    };

    if let Err(message) = activate_view(hwnd, &view_id, None) {
        show_error_message(hwnd, "Switchbar", &message);
    }

    true
}

/// Returns whether the user logged in rather than cancelled.
unsafe fn show_select_server_dialog(hwnd: HWND, server: &str) -> Result<bool, String> {
    connection::check_sign_in(state_ref(hwnd).model.connection)?;
    let Some(form) = show_login_dialog(hwnd, server)? else {
        return Ok(false);
    };
    submit_login(
        state_mut(hwnd),
        form.server,
        form.username,
        form.password,
        form.device_display_name,
        true,
    )?;
    refresh_status_bar(hwnd)?;
    Ok(true)
}

/// File > Connect and `/server [server]`: the step that brings the account
/// online from where it is. Returns whether it ran rather than the user
/// cancelling Select Server.
unsafe fn connect(hwnd: HWND, server: Option<&str>) -> Result<bool, String> {
    let action = connection::connect_action(
        state_ref(hwnd).model.connection,
        server,
        session_path()?.is_file(),
    )?;
    match action {
        ConnectAction::RestoreSavedSession => submit_restore_saved_session(state_mut(hwnd), true)?,
        ConnectAction::SelectServer { server } => return show_select_server_dialog(hwnd, &server),
        ConnectAction::StartSync => submit_start_sync(state_mut(hwnd))?,
    }
    refresh_status_bar(hwnd)?;
    Ok(true)
}

/// File > Disconnect and `/quit`: stop syncing, keep the session.
unsafe fn disconnect(hwnd: HWND) -> Result<(), String> {
    connection::check_disconnect(state_ref(hwnd).model.connection)?;
    enqueue_command(state_mut(hwnd), Command::StopSync)
}

/// A Commands item runs the slash command it stands for, prompting first
/// for its parameters; failures echo in the active window, as typed
/// commands' do. Returns whether it ran rather than the user cancelling.
unsafe fn run_commands_menu_item(
    hwnd: HWND,
    item: &menus::CommandsMenuItem,
) -> Result<bool, String> {
    let input = match item.prompt {
        Some(label) => {
            match show_single_field_dialog(hwnd, &commands_menu_title(item), label, "")? {
                Some(input) => input,
                None => return Ok(false),
            }
        }
        None => String::new(),
    };
    match commands::parse(&commands_menu_line(item, &input)) {
        Ok(command) => run_local_command(hwnd, command)?,
        Err(error) => report_command_error(hwnd, error)?,
    }
    Ok(true)
}

unsafe fn show_recover_e2ee_menu_dialog(hwnd: HWND) -> Result<bool, String> {
    let Some(form) = show_recover_e2ee_dialog(hwnd)? else {
        return Ok(false);
    };
    submit_recover_e2ee(state_mut(hwnd), form.recovery_secret)?;
    refresh_status_bar(hwnd)?;
    Ok(true)
}

unsafe fn show_setup_e2ee_menu_dialog(hwnd: HWND) -> Result<bool, String> {
    let Some(form) = show_setup_e2ee_dialog(hwnd)? else {
        return Ok(false);
    };
    submit_setup_e2ee_recovery(
        state_mut(hwnd),
        form.passphrase,
        form.wait_for_backups_to_upload,
    )?;
    refresh_status_bar(hwnd)?;
    Ok(true)
}

unsafe fn layout_controls(hwnd: HWND) {
    let state = state_mut(hwnd);
    if state.controls.tree.is_null()
        || state.controls.mdi_client.is_null()
        || state.controls.status_bar.is_null()
        || state.controls.switchbar.is_null()
    {
        return;
    }

    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    // The bars are shown as the settings say. A hidden one is laid out
    // with no size, and the MDI client takes its place.
    let settings = state.settings.settings();
    let (show_switchbar, show_treebar) = (settings.switchbar.show, settings.treebar.show);
    ShowWindow(state.controls.switchbar, show_or_hide(show_switchbar));
    ShowWindow(state.controls.tree, show_or_hide(show_treebar));
    let tree_width = if show_treebar {
        TREE_WIDTH.min(width)
    } else {
        0
    };
    let switchbar_height = if show_switchbar {
        // The switchbar wraps its buttons on its own WM_SIZE, so it gets
        // the frame's width first and its height from the rows that gives.
        let mut switchbar_rect = RECT::default();
        GetWindowRect(state.controls.switchbar, &mut switchbar_rect);
        MoveWindow(
            state.controls.switchbar,
            0,
            0,
            width,
            switchbar_rect.bottom - switchbar_rect.top,
            1,
        );
        let rows = u32::try_from(SendMessageW(state.controls.switchbar, TB_GETROWS, 0, 0))
            .expect("a toolbar's row count is positive");
        let button_height =
            (SendMessageW(state.controls.switchbar, TB_GETBUTTONSIZE, 0, 0) as u32 >> 16) as i32;
        switchbar::switchbar_height(rows, button_height).min(height)
    } else {
        0
    };
    MoveWindow(state.controls.switchbar, 0, 0, width, switchbar_height, 1);
    // A status bar sizes and places itself at the bottom of its parent, so
    // the controls above it are laid out against its real height. Sizing it
    // to a guessed height instead left it overlapping them by a few pixels,
    // and which one painted last decided the snapshot.
    SendMessageW(state.controls.status_bar, WM_SIZE, 0, 0);
    let mut status_bar_rect = RECT::default();
    GetWindowRect(state.controls.status_bar, &mut status_bar_rect);
    let status_bar_height =
        (status_bar_rect.bottom - status_bar_rect.top).min((height - switchbar_height).max(0));
    let mdi_width = (width - tree_width).max(0);
    let mdi_height = (height - switchbar_height - status_bar_height).max(0);

    MoveWindow(
        state.controls.tree,
        0,
        switchbar_height,
        tree_width,
        mdi_height,
        1,
    );
    MoveWindow(
        state.controls.mdi_client,
        tree_width,
        switchbar_height,
        mdi_width,
        mdi_height,
        1,
    );
}

fn show_or_hide(show: bool) -> SHOW_WINDOW_CMD {
    if show { SW_SHOW } else { SW_HIDE }
}

unsafe fn handle_core_events(hwnd: HWND) {
    if let Err(message) = drain_core_events_and_refresh(hwnd) {
        state_mut(hwnd).exit_code.store(1, Ordering::Relaxed);
        eprintln!("{message}");
        DestroyWindow(hwnd);
    }
}

/// Returns whether the event should flash the frame.
fn apply_core_event(state: &mut WindowState, event: Event) -> Result<bool, String> {
    state.model.connection = connection::after_event(state.model.connection, &event);
    match event {
        Event::Started => {
            if matches!(
                state.model.lifecycle_state,
                LifecycleState::Offline | LifecycleState::CoreStarted
            ) {
                state.model.status = "core started".to_owned();
                state.model.lifecycle_state = LifecycleState::CoreStarted;
            }
        }
        Event::LoggedIn {
            session,
            homeserver_url,
        } => {
            if let Some(own_user_id) = &state.model.own_user_id
                && *own_user_id != session.user_id
            {
                return Err(format!(
                    "received LoggedIn for {} while signed in as {own_user_id}",
                    session.user_id
                ));
            }
            // A replayed event load must not overwrite the saved session.
            if !state.synthetic_load_in_progress {
                save_session(&homeserver_url, &session)?;
                state.current_session_homeserver_url = Some(homeserver_url);
            }
            log_status(
                state,
                StatusLineKind::Info,
                status_log::logged_in(&session.user_id, &session.device_id),
            );
            state.model.own_user_id = Some(session.user_id);
            state.model.status = "authenticated".to_owned();
            state.model.lifecycle_state = LifecycleState::Authenticated;
        }
        Event::SessionUpdated { session } => {
            let homeserver_url = state
                .current_session_homeserver_url
                .as_ref()
                .expect("session updates require an active homeserver URL");
            save_session(homeserver_url, &session)?;
        }
        Event::RestoredSession => {
            log_status(
                state,
                StatusLineKind::Info,
                status_log::SESSION_RESTORED.to_owned(),
            );
            state.model.status = "restored saved session".to_owned();
            state.model.lifecycle_state = LifecycleState::Authenticated;
        }
        Event::SyncedRooms(rooms) => {
            // The core re-sends the room list on every change; only the
            // first one after connecting is news.
            if state.model.lifecycle_state != LifecycleState::Ready {
                let invited = rooms.iter().filter(|room| is_invited_room(room)).count();
                log_status(
                    state,
                    StatusLineKind::Info,
                    status_log::synced(rooms.len() - invited, invited),
                );
            }
            state.model.status = format!("synced {} room(s)", rooms.len());
            state.model.lifecycle_state = LifecycleState::Ready;
            if state
                .model
                .selected_room
                .as_deref()
                .is_some_and(|room_id| navigable_room_by_id(&rooms, room_id).is_none())
            {
                state.model.selected_room = None;
            }
            state.room_activity.retain(|view_id, _| {
                view_id_is_valid_for_rooms(&rooms, &state.model.room_views, view_id)
            });
            state.closed_queries.retain(|room_id| {
                navigable_room_by_id(&rooms, room_id)
                    .is_some_and(|room| room_window_kind(room) == RoomWindowKind::Query)
            });
            let tracked_room_activity = state.room_activity.clone();
            state
                .room_activity_read_targets
                .retain(|view_id, _| tracked_room_activity.contains_key(view_id));
            for (room_id, room_view) in &mut state.model.room_views {
                if sendable_room_by_id(&rooms, room_id).is_none() {
                    room_view.typing_user_ids.clear();
                    room_view.members.clear();
                }
            }
            state
                .model
                .room_read_states
                .retain(|room_id, _| sendable_room_by_id(&rooms, room_id).is_some());
            for room in rooms
                .iter()
                .filter(|room| room.membership == RoomMembership::Join && !room.is_space)
            {
                state
                    .model
                    .room_views
                    .entry(room.room_id.clone())
                    .or_default()
                    .topic = room.topic.clone();
            }
            state.model.rooms = rooms;
        }
        Event::RoomActivity(activity) => return Ok(apply_room_activity_event(state, *activity)),
        Event::RoomActivityUpdate(update) => apply_room_activity_update_event(state, update),
        Event::RoomHistoryBackfill(backfill) => apply_room_history_backfill_event(state, backfill),
        Event::RoomMedia(media) => apply_room_media_event(state, media),
        Event::RoomEvent(fetch) => apply_room_event_fetch(state, fetch),
        Event::PublicRooms(page) => apply_public_rooms_page(state, page)?,
        Event::RoomJoined(joined) => apply_room_joined_event(state, &joined.room_id)?,
        Event::RoomTyping(typing) => apply_room_typing_event(state, typing),
        Event::RoomMembers(members) => apply_room_members_event(state, members),
        Event::RoomReadState(read_state) => apply_room_read_state_event(state, read_state),
        Event::HighlightWords(words) => state.model.highlight_words = Some(words),
        Event::IgnoredUsers(ignored_users) => {
            // The first list after signing in is no change.
            if let Some(old) = &state.model.ignored_users {
                let lines = ignore::change_lines(old, &ignored_users, |user_id| {
                    known_nick(&state.model, user_id)
                });
                echo_lines_in_active_view(state, lines);
            }
            state.model.ignored_users = Some(ignored_users);
        }
        Event::UserProfile(profile) => {
            let lines = whois_lines(&state.model, &profile);
            echo_lines_in_active_view(state, lines);
        }
        Event::E2eeStatus(status) => {
            for line in status_log::e2ee_status(&status) {
                log_status(state, StatusLineKind::Info, line);
            }
            state.model.last_e2ee_status = Some(status);
            state.model.status = "e2ee status updated".to_owned();
        }
        Event::E2eeRecoverySucceeded(_) => {
            log_status(
                state,
                StatusLineKind::Info,
                status_log::E2EE_RECOVERY_COMPLETE.to_owned(),
            );
            state.model.status = "e2ee recovery complete".to_owned();
        }
        Event::E2eeRecoverySetupSucceeded(_) => {
            log_status(
                state,
                StatusLineKind::Info,
                status_log::E2EE_RECOVERY_SET_UP.to_owned(),
            );
            state.model.last_e2ee_recovery_setup_result = true;
            state.model.status = "e2ee recovery setup complete".to_owned();
        }
        Event::VerificationRequest(request) => {
            apply_verification_event(state, verification_state_event_from_request(request));
        }
        Event::VerificationState(event) => apply_verification_event(state, event),
        Event::LoggedOut => {
            delete_saved_session()?;
            state.current_session_homeserver_url = None;
            state.model.own_user_id = None;
            state.model.status = "logged out".to_owned();
            state.model.lifecycle_state = LifecycleState::LoggedOut;
            state.model.rooms.clear();
            state.closed_queries.clear();
            state.model.selected_room = None;
            state.model.room_views.clear();
            state.model.last_activity = None;
            state.model.last_e2ee_status = None;
            state.model.last_e2ee_recovery_setup_result = false;
            state.model.last_verification = None;
            state.model.last_error = None;
            state.model.room_read_states.clear();
            state.model.room_list = None;
            state.model.highlight_words = None;
            state.model.ignored_users = None;
            state.room_activity.clear();
            state.room_activity_read_targets.clear();
            state.find_states.clear();
            state.decoded_images.clear();
            log_status(
                state,
                StatusLineKind::Info,
                status_log::LOGGED_OUT.to_owned(),
            );
        }
        Event::Stopped => {
            log_status(state, StatusLineKind::Info, status_log::STOPPED.to_owned());
            state.model.status = "core stopped".to_owned();
            state.model.lifecycle_state = LifecycleState::Stopped;
        }
        Event::SyncStarted => {
            log_status(
                state,
                StatusLineKind::Info,
                status_log::SYNC_STARTED.to_owned(),
            );
            state.model.status = "continuous sync started".to_owned();
            state.model.lifecycle_state = LifecycleState::Syncing;
        }
        Event::SyncStopped(reason) => {
            let (kind, line) = status_log::disconnected(&reason);
            log_status(state, kind, line);
            state.model.status = "continuous sync stopped".to_owned();
        }
        Event::Error(error) => {
            if error.operation == Operation::BackfillRoomHistory {
                let room_id = error
                    .room_id
                    .as_deref()
                    .expect("a backfill error names its room");
                apply_backfill_room_history_error(state, room_id, error.message.clone());
            }
            let line = status_log::error(operation_name(error.operation), &error.message);
            let now = state.clock.now_ms();
            match error_destination(&error) {
                ErrorDestination::ActiveWindow { command } => {
                    let error = CommandError::new(command, error.message.clone());
                    echo_lines_in_active_view(state, [error.to_string()]);
                }
                ErrorDestination::Room(room_id) => match state.model.room_views.get_mut(room_id) {
                    Some(room_view) => room_view.echo.push(now, None, context_line(&line)),
                    // The room was left, or never synced.
                    None => log_status(state, StatusLineKind::Error, line),
                },
                ErrorDestination::Status => log_status(state, StatusLineKind::Error, line),
            }
            if !preserves_submission_status(&state.model.status, error.operation) {
                state.model.status = format!("error during {}", operation_name(error.operation));
            }
            state.model.lifecycle_state = LifecycleState::Error;
            state.model.last_error = Some(error);
        }
    }
    Ok(false)
}

/// A directory page fills the Rooms List and asks for the next one. A page
/// for a listing the user closed or replaced is dropped.
fn apply_public_rooms_page(state: &mut WindowState, page: PublicRoomsPage) -> Result<(), String> {
    let Some(room_list) = &mut state.model.room_list else {
        return Ok(());
    };
    match room_list.apply_page(page) {
        Err(room_list::Ignored) | Ok(PageApplied::Finished) => {}
        Ok(PageApplied::Next(request)) => {
            enqueue_command(state, Command::ListPublicRooms(request))?;
        }
        Ok(PageApplied::Failed(message)) => {
            log_status(state, StatusLineKind::Error, format!("/list: {message}"));
        }
    }
    Ok(())
}

/// A join the user asked for opens the room's window and makes it active,
/// as `/join` does in mIRC; the next view sync realizes it. A space has no
/// window, so the Status window notes the join instead.
fn apply_room_joined_event(state: &mut WindowState, room_id: &str) -> Result<(), String> {
    // The core sends the snapshot listing the room right before.
    let room = state
        .model
        .rooms
        .iter()
        .find(|room| room.room_id == room_id)
        .ok_or_else(|| format!("joined room {room_id} is not in the room list"))?;
    if room.is_space {
        let line = status_log::joined_space(&room.display_name);
        log_status(state, StatusLineKind::Info, line);
        return Ok(());
    }
    let view_id = ViewId::Room(room_id.to_owned());
    state.closed_queries.remove(room_id);
    state.model.selected_room = Some(room_id.to_owned());
    if state.active_view != view_id {
        state.selected_timeline_event = None;
        state.active_view = view_id;
    }
    Ok(())
}

/// Appends a line to the Status window, colouring its switchbar button when
/// it is not the active window, as any window's new lines do.
fn log_status(state: &mut WindowState, kind: StatusLineKind, text: String) {
    state
        .model
        .status_log
        .push(state.clock.now_ms(), kind, context_line(&text));
    if state.active_view != ViewId::Status {
        let level = match kind {
            StatusLineKind::Info => ViewActivityLevel::Event,
            StatusLineKind::Error => ViewActivityLevel::Message,
        };
        raise_view_activity(state, ViewId::Status, level);
    }
}

fn preserves_submission_status(status: &str, operation: Operation) -> bool {
    match operation {
        Operation::JoinRoom => status.starts_with("join submitted: "),
        Operation::AcceptRoomInvite => status.starts_with("accept submitted: "),
        Operation::CreateDirectRoom => status.starts_with("query submitted: "),
        Operation::CreateRoom => status.starts_with("create submitted: "),
        Operation::LeaveRoom => status.starts_with("part submitted: "),
        Operation::RejectRoomInvite => status.starts_with("reject submitted: "),
        Operation::SetRoomTopic => status.starts_with("topic change submitted for "),
        _ => false,
    }
}

fn timeline_message_send_state(send_state: &RoomMessageSendState) -> TimelineMessageSendState {
    match send_state {
        RoomMessageSendState::Pending => TimelineMessageSendState::Pending,
        RoomMessageSendState::Sent => TimelineMessageSendState::Sent,
        RoomMessageSendState::Failed { .. } => TimelineMessageSendState::Failed,
    }
}

fn append_room_read_state_tokens(label: &mut String, read_state: Option<&RoomReadState>) {
    let Some(read_state) = read_state else {
        return;
    };

    let mut tokens = Vec::new();
    if read_state.num_mentions > 0 {
        tokens.push(format!("@{}", read_state.num_mentions));
    }
    if read_state.num_notifications > 0 {
        tokens.push(format!("!{}", read_state.num_notifications));
    }
    if read_state.num_unread > 0 {
        tokens.push(format!("*{}", read_state.num_unread));
    }
    if read_state.marked_unread {
        tokens.push("M".to_owned());
    }
    if tokens.is_empty() {
        return;
    }

    label.push_str(" [");
    label.push_str(&tokens.join(" "));
    label.push(']');
}

fn room_context_row(
    kind: TimelineRowKind,
    label: TimelineSyntheticRowLabel,
    body: &str,
) -> TimelineRow {
    let text = context_line(body);
    TimelineRow::log(kind, text.clone()).with_identity(TimelineRowIdentity::synthetic(label, &text))
}

fn build_invite_timeline_rows(room: &RoomSummary) -> Vec<TimelineRow> {
    vec![
        room_context_row(
            TimelineRowKind::Info,
            TimelineSyntheticRowLabel::Invite,
            &invited_text(&room.display_name),
        ),
        room_context_row(
            TimelineRowKind::Info,
            TimelineSyntheticRowLabel::Invite,
            INVITE_RESPONSE_TEXT,
        ),
    ]
}

/// The lines mIRC prints when a channel window opens.
fn push_room_context_rows(
    rows: &mut Vec<TimelineRow>,
    room: &RoomSummary,
    room_view: Option<&RoomViewModel>,
) {
    rows.push(room_context_row(
        TimelineRowKind::Event,
        TimelineSyntheticRowLabel::NowTalking,
        &now_talking_text(&room.display_name, &room.room_id),
    ));
    if let Some(topic) = room_view.and_then(|view| view.topic.as_deref()) {
        rows.push(room_context_row(
            TimelineRowKind::Event,
            TimelineSyntheticRowLabel::Topic,
            &topic_is_text(topic),
        ));
    }
    if room.encrypted {
        rows.push(room_context_row(
            TimelineRowKind::Info,
            TimelineSyntheticRowLabel::Encryption,
            ENCRYPTED_ROOM_TEXT,
        ));
    }
}

/// Who is typing, below the newest line.
fn push_room_typing_row(rows: &mut Vec<TimelineRow>, room_view: &RoomViewModel) {
    if room_view.typing_user_ids.is_empty() {
        return;
    }
    let users = room_view
        .typing_user_ids
        .iter()
        .map(|user_id| room_view.member_nick(user_id))
        .collect::<Vec<_>>()
        .join(", ");
    let verb = if room_view.typing_user_ids.len() == 1 {
        "is"
    } else {
        "are"
    };
    let logical_text = context_line(&format!("{users} {verb} typing"));
    rows.push(
        TimelineRow::full_width(TimelineRowKind::Typing, logical_text.clone()).with_identity(
            TimelineRowIdentity::synthetic(TimelineSyntheticRowLabel::Typing, &logical_text),
        ),
    );
}

fn push_room_history_backfill_row(rows: &mut Vec<TimelineRow>, history: &RoomHistoryBackfillState) {
    let text = match &history.status {
        RoomHistoryBackfillStatus::Idle => return,
        RoomHistoryBackfillStatus::Loading { .. } => "* Loading older history...".to_owned(),
        RoomHistoryBackfillStatus::Failed { message, .. } => {
            format!("* Unable to load older history: {message}. Scroll to top to retry.")
        }
        RoomHistoryBackfillStatus::Exhausted => "* No older history available.".to_owned(),
    };
    rows.push(
        TimelineRow::full_width(TimelineRowKind::Plain, text.clone()).with_identity(
            TimelineRowIdentity::synthetic(TimelineSyntheticRowLabel::Backfill, &text),
        ),
    );
}

fn push_room_activity_row(
    rows: &mut Vec<TimelineRow>,
    room_view: &RoomViewModel,
    activity: &RoomActivity,
    timestamps: TimestampStyle,
) {
    push_reply_quote_row(rows, room_view, activity);
    let ActivityLine {
        kind,
        text,
        styles,
        image_state,
    } = activity_line_text(
        room_view,
        activity,
        &timestamps.prefix(activity.timestamp_ms),
    );
    let mut row = TimelineRow::log(kind, text)
        .with_styles(styles)
        .with_identity(TimelineRowIdentity::activity_primary(&activity.activity_id))
        .with_event_id(room_activity_event_id(activity));
    row.image_state = image_state;
    rows.push(row);

    let Some(event_id) = room_activity_event_id(activity) else {
        return;
    };
    if matches!(activity.kind, RoomActivityKind::Redacted(_)) {
        return;
    }
    let summaries = room_view.reaction_summaries_for_event(event_id);
    if summaries.is_empty() {
        return;
    }
    // A log row: naming reactors makes the line long, and it wraps.
    rows.push(
        TimelineRow::log(
            TimelineRowKind::ReactionSummary,
            reactions::summary_line(&summaries, |user_id| room_view.member_nick(user_id)),
        )
        .with_identity(TimelineRowIdentity::activity_reaction_summary(
            &activity.activity_id,
        )),
    );
}

/// The line above a reply quoting what it answers. It carries the reply's
/// event, so clicking it selects the reply.
fn push_reply_quote_row(
    rows: &mut Vec<TimelineRow>,
    room_view: &RoomViewModel,
    activity: &RoomActivity,
) {
    let Some(target_event_id) = activity
        .relation
        .as_ref()
        .and_then(replies::reply_quote_target)
    else {
        return;
    };
    let quoted = match room_view.reply_target(target_event_id) {
        ReplyTarget::Known(RoomActivity {
            kind: RoomActivityKind::Redacted(_),
            ..
        }) => replies::QUOTE_REDACTED_TEXT.to_owned(),
        ReplyTarget::Known(target) => quoted_line(room_view, target),
        ReplyTarget::Loading => replies::QUOTE_LOADING_TEXT.to_owned(),
        ReplyTarget::Unavailable => replies::QUOTE_UNAVAILABLE_TEXT.to_owned(),
    };
    rows.push(
        TimelineRow::full_width(TimelineRowKind::ReplyQuote, replies::quote_line(&quoted))
            .with_identity(TimelineRowIdentity::activity_reply_quote(
                &activity.activity_id,
            ))
            .with_event_id(room_activity_event_id(activity)),
    );
}

/// A message as a quote or the compose indicator shows it.
fn quoted_line(room_view: &RoomViewModel, activity: &RoomActivity) -> String {
    replies::quote_excerpt(&activity_line_text(room_view, activity, "").text)
}

/// The bottom line of a window whose editbox is armed with `target`. It
/// wraps like a log line: the excerpt alone nearly fills a default window.
fn compose_target_row(model: &UiModel, view_id: &ViewId, target: &ComposeTarget) -> TimelineRow {
    let room_id = view_id
        .room_id()
        .expect("only room and thread windows compose replies and edits");
    let activity = window_message(model, view_id, target.event_id())
        .expect("refreshing a window disarms a target it no longer shows");
    room_context_row(
        TimelineRowKind::Info,
        TimelineSyntheticRowLabel::ComposeTarget,
        &replies::compose_target_indicator(
            target,
            &quoted_line(&model.room_views[room_id], activity),
        ),
    )
}

/// The message `event_id` names among the lines of the window `view_id`.
/// A redacted message is no longer one.
fn window_message<'a>(
    model: &'a UiModel,
    view_id: &ViewId,
    event_id: &str,
) -> Option<&'a RoomActivity> {
    let room_view = model.room_views.get(view_id.room_id()?)?;
    let activity = &room_view.activities[room_view.find_activity_index_by_event_id(event_id)?];
    let in_window = view_id
        .thread_key()
        .is_none_or(|(_, thread_root_event_id)| {
            room_activity_belongs_to_thread(activity, thread_root_event_id)
        });
    (in_window && !matches!(activity.kind, RoomActivityKind::Redacted(_))).then_some(activity)
}

/// An activity's line: its row kind, text after the timestamp prefix, the
/// text's styles and the image preview state.
struct ActivityLine {
    kind: TimelineRowKind,
    text: String,
    styles: Vec<TextStyleSpan>,
    image_state: Option<TimelineImageState>,
}

impl ActivityLine {
    fn plain(kind: TimelineRowKind, text: String, image_state: Option<TimelineImageState>) -> Self {
        Self {
            kind,
            text,
            styles: Vec::new(),
            image_state,
        }
    }
}

fn activity_line_text(
    room_view: &RoomViewModel,
    activity: &RoomActivity,
    timestamp: &str,
) -> ActivityLine {
    let (kind, text, image_state) = match &activity.kind {
        RoomActivityKind::Message(message) => {
            let suffix = match &message.send_state {
                RoomMessageSendState::Pending => " (sending)",
                RoomMessageSendState::Sent
                    if message
                        .event_id
                        .as_ref()
                        .is_some_and(|event_id| room_view.edited_event_ids.contains(event_id)) =>
                {
                    " (edited)"
                }
                RoomMessageSendState::Sent => "",
                RoomMessageSendState::Failed { .. } => " (failed)",
            };
            let (text, body_offset) = message_line(
                timestamp,
                message.presentation,
                &message.sender,
                &format!("{}{suffix}", message.body),
            );
            // The suffix is mirx's note, not the sender's text: unstyled.
            let styles = message
                .styles
                .iter()
                .map(|span| TextStyleSpan {
                    start: body_offset + span.start,
                    end: body_offset + span.end,
                    style: span.style,
                })
                .collect();
            return ActivityLine {
                kind: TimelineRowKind::Message {
                    presentation: message.presentation,
                    send_state: timeline_message_send_state(&message.send_state),
                    highlight: activity.highlight,
                },
                text,
                styles,
                image_state: None,
            };
        }
        RoomActivityKind::Image(message) => {
            let display_name = message.filename.as_deref().unwrap_or(message.body.as_str());
            let metadata_suffix = format_room_image_metadata_suffix(message.info.as_ref());
            let format = timeline_thumbnail_format();
            let image_state = Some(room_view.projected_preview_state(
                &activity.activity_id,
                &message.event_id,
                &format,
            ));
            (
                TimelineRowKind::Image {
                    highlight: activity.highlight,
                },
                action_line(
                    timestamp,
                    &message.sender,
                    &format!("sent image: {display_name}{metadata_suffix}"),
                ),
                image_state,
            )
        }
        RoomActivityKind::File(message) => {
            let display_name = message.filename.as_deref().unwrap_or(message.body.as_str());
            let metadata_suffix = format_room_file_metadata_suffix(message.info.as_ref());
            (
                TimelineRowKind::File {
                    highlight: activity.highlight,
                },
                action_line(
                    timestamp,
                    &message.sender,
                    &format!("sent file: {display_name}{metadata_suffix}"),
                ),
                None,
            )
        }
        RoomActivityKind::Audio(message) => {
            let display_name = message.filename.as_deref().unwrap_or(message.body.as_str());
            let metadata_suffix = format_room_audio_metadata_suffix(message.info.as_ref());
            (
                TimelineRowKind::Audio {
                    highlight: activity.highlight,
                },
                action_line(
                    timestamp,
                    &message.sender,
                    &format!("sent audio: {display_name}{metadata_suffix}"),
                ),
                None,
            )
        }
        RoomActivityKind::Video(message) => {
            let display_name = message.filename.as_deref().unwrap_or(message.body.as_str());
            let metadata_suffix = format_room_video_metadata_suffix(message.info.as_ref());
            let format = timeline_thumbnail_format();
            let image_state = timeline_video_preview_source(message).map(|_| {
                room_view.projected_preview_state(&activity.activity_id, &message.event_id, &format)
            });
            (
                TimelineRowKind::Video {
                    highlight: activity.highlight,
                },
                action_line(
                    timestamp,
                    &message.sender,
                    &format!("sent video: {display_name}{metadata_suffix}"),
                ),
                image_state,
            )
        }
        RoomActivityKind::UnsupportedMessage(message) => (
            TimelineRowKind::Unsupported {
                highlight: activity.highlight,
            },
            action_line(
                timestamp,
                &message.sender,
                &format!("sent unsupported {}: {}", message.msgtype, message.body),
            ),
            None,
        ),
        RoomActivityKind::UnableToDecrypt(message) => (
            TimelineRowKind::UnableToDecrypt,
            action_line(
                timestamp,
                &message.sender,
                "sent an encrypted message that could not be decrypted",
            ),
            None,
        ),
        RoomActivityKind::StateNotice(notice) => {
            let body = match &notice.content {
                RoomStateNoticeContent::Topic { topic } => topic_change_text(&notice.sender, topic),
                RoomStateNoticeContent::Member {
                    change,
                    target,
                    reason,
                } => member_change_text(&notice.sender, change, target, reason.as_deref()),
                RoomStateNoticeContent::PowerLevels {
                    changes,
                    users_default,
                    other_permissions_changed,
                } => power_levels_change_text(
                    &notice.sender,
                    changes,
                    users_default.as_ref(),
                    *other_permissions_changed,
                ),
            };
            (
                TimelineRowKind::StateNotice {
                    highlight: activity.highlight,
                },
                event_line(timestamp, &body),
                None,
            )
        }
        // The redacted line no longer says what made it a highlight.
        RoomActivityKind::Redacted(_) => (
            TimelineRowKind::StateNotice { highlight: false },
            event_line(timestamp, "message redacted"),
            None,
        ),
    };
    ActivityLine::plain(kind, text, image_state)
}

fn build_room_timeline_rows(
    model: &UiModel,
    room_id: &str,
    timestamps: TimestampStyle,
) -> Vec<TimelineRow> {
    let room = room_by_id(&model.rooms, room_id).expect("room view must target an existing room");
    let room_view = model.room_views.get(room_id);
    if is_invited_room(room) {
        let mut rows = build_invite_timeline_rows(room);
        if let Some(room_view) = room_view {
            rows.extend(
                room_view
                    .echo
                    .lines_for(None)
                    .map(|line| echo_row(line, timestamps)),
            );
        }
        return rows;
    }
    let mut rows = Vec::new();
    push_room_context_rows(&mut rows, room, room_view);

    if let Some(room_view) = room_view {
        push_room_history_backfill_row(&mut rows, &room_view.history);
        push_activity_rows_with_echo(
            &mut rows,
            room_view,
            room_view.activities.iter(),
            None,
            timestamps,
        );
        push_room_typing_row(&mut rows, room_view);
    }

    rows
}

/// The window's activity rows with its echo lines merged by time: an echo
/// line goes before the first activity newer than it, so it stays below
/// what the user saw when typing the command.
fn push_activity_rows_with_echo<'a>(
    rows: &mut Vec<TimelineRow>,
    room_view: &RoomViewModel,
    activities: impl Iterator<Item = &'a RoomActivity>,
    thread_root: Option<&str>,
    timestamps: TimestampStyle,
) {
    let mut echo = room_view.echo.lines_for(thread_root).peekable();
    for activity in activities {
        while let Some(line) = echo.next_if(|line| line.timestamp_ms < activity.timestamp_ms) {
            rows.push(echo_row(line, timestamps));
        }
        push_room_activity_row(rows, room_view, activity, timestamps);
    }
    rows.extend(echo.map(|line| echo_row(line, timestamps)));
}

fn echo_row(line: &EchoLine, timestamps: TimestampStyle) -> TimelineRow {
    let prefix = timestamps.prefix(line.timestamp_ms);
    TimelineRow::log(TimelineRowKind::Info, format!("{prefix}{}", line.text)).with_identity(
        TimelineRowIdentity::EchoLine {
            sequence: line.sequence,
        },
    )
}

fn build_thread_timeline_rows(
    model: &UiModel,
    room_id: &str,
    thread_root_event_id: &str,
    timestamps: TimestampStyle,
) -> Vec<TimelineRow> {
    let room = room_by_id(&model.rooms, room_id).expect("thread view must target an existing room");
    let room_view = model
        .room_views
        .get(room_id)
        .expect("thread view must target an existing room view");
    let mut rows = Vec::new();
    push_room_context_rows(&mut rows, room, Some(room_view));
    push_activity_rows_with_echo(
        &mut rows,
        room_view,
        room_view
            .activities
            .iter()
            .filter(|activity| room_activity_belongs_to_thread(activity, thread_root_event_id)),
        Some(thread_root_event_id),
        timestamps,
    );
    push_room_typing_row(&mut rows, room_view);
    rows
}

fn project_timeline_rows(rows: &[TimelineRow]) -> Vec<String> {
    rows.iter().map(|row| row.logical_text.clone()).collect()
}

#[cfg_attr(not(test), allow(dead_code))]
fn render_room_lines(state: &WindowState, room_id: &str) -> Vec<String> {
    project_timeline_rows(&build_room_timeline_rows(
        &state.model,
        room_id,
        state.timestamps(),
    ))
}

fn format_room_image_metadata_suffix(info: Option<&RoomImageInfo>) -> String {
    let Some(info) = info else {
        return String::new();
    };

    let mut parts = Vec::new();
    if let Some(mimetype) = info.mimetype.as_deref() {
        parts.push(mimetype.to_owned());
    }
    if let (Some(width), Some(height)) = (info.width, info.height) {
        parts.push(format!("{width}x{height}"));
    }
    if let Some(size) = info.size {
        parts.push(format!("{size} bytes"));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

fn format_room_file_metadata_suffix(info: Option<&RoomFileInfo>) -> String {
    let Some(info) = info else {
        return String::new();
    };

    let mut parts = Vec::new();
    if let Some(mimetype) = info.mimetype.as_deref() {
        parts.push(mimetype.to_owned());
    }
    if let Some(size) = info.size {
        parts.push(format!("{size} bytes"));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

fn format_room_audio_metadata_suffix(info: Option<&RoomAudioInfo>) -> String {
    let Some(info) = info else {
        return String::new();
    };

    let mut parts = Vec::new();
    if let Some(mimetype) = info.mimetype.as_deref() {
        parts.push(mimetype.to_owned());
    }
    if let Some(size) = info.size {
        parts.push(format!("{size} bytes"));
    }
    if let Some(duration_ms) = info.duration_ms {
        parts.push(format!("{duration_ms} ms"));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

fn format_room_video_metadata_suffix(info: Option<&RoomVideoInfo>) -> String {
    let Some(info) = info else {
        return String::new();
    };

    let mut parts = Vec::new();
    if let Some(mimetype) = info.mimetype.as_deref() {
        parts.push(mimetype.to_owned());
    }
    if let (Some(width), Some(height)) = (info.width, info.height) {
        parts.push(format!("{width}x{height}"));
    }
    if let Some(size) = info.size {
        parts.push(format!("{size} bytes"));
    }
    if let Some(duration_ms) = info.duration_ms {
        parts.push(format!("{duration_ms} ms"));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

fn build_status_timeline_rows(
    status_log: &StatusLog,
    timestamps: TimestampStyle,
) -> Vec<TimelineRow> {
    status_log
        .lines()
        .iter()
        .map(|line| {
            let kind = match line.kind {
                StatusLineKind::Info => TimelineRowKind::Info,
                StatusLineKind::Error => TimelineRowKind::Error,
            };
            let prefix = timestamps.prefix(line.timestamp_ms);
            TimelineRow::log(kind, format!("{prefix}{}", line.text))
                .with_identity(TimelineRowIdentity::status_line(line.sequence))
        })
        .collect()
}

fn frame_status_text(state: &WindowState) -> String {
    let model = &state.model;
    let mut text = match &state.active_view {
        ViewId::Room(room_id) => room_by_id(&model.rooms, room_id)
            .filter(|room| is_invited_room(room))
            .map(|room| format!("invited: {} ({})", room.display_name, room.room_id))
            .unwrap_or_else(|| model.status.clone()),
        _ => model.status.clone(),
    };

    if let Some(event) = &model.last_verification {
        text.push_str(" | verification: ");
        text.push_str(verification_state_label(&event.state));
        text.push(' ');
        text.push_str(&event.flow_id);
        text.push_str(" from ");
        text.push_str(&event.user_id);
    }

    if let Some(status) = &model.last_e2ee_status {
        text.push_str(" | e2ee: recovery ");
        text.push_str(e2ee_recovery_state_label(status.recovery));
        text.push_str(", ");
        text.push_str(e2ee_backup_health_summary(status));
        text.push_str(", cross-signing ");
        text.push_str(e2ee_cross_signing_label(status.cross_signing.as_ref()));
    }

    if let Some(error) = &model.last_error {
        text.push_str(" | error: ");
        text.push_str(operation_name(error.operation));
        text.push_str(": ");
        text.push_str(&error.message);
    }

    if let Some(feedback) = active_view_find_feedback(state) {
        text.push_str(" | ");
        text.push_str(feedback);
    }

    text
}

unsafe fn refresh_status_bar(hwnd: HWND) -> Result<(), String> {
    let status_bar = state_ref(hwnd).controls.status_bar;
    if status_bar.is_null() {
        return Err("status bar control was not created".to_owned());
    }

    let text = wide_null(&frame_status_text(state_ref(hwnd)));
    if SendMessageW(status_bar, SB_SETTEXTW, 0, text.as_ptr() as LPARAM) == 0 {
        return Err("failed to update status bar text".to_owned());
    }

    Ok(())
}

fn render_view_text(lines: &[String]) -> String {
    lines.join("\r\n")
}

/// The view to keep active: the active one if it still has a window, else
/// its room, the selected room, the first room or Status. A closed query
/// has no window, so it is never picked.
fn resolve_active_view(
    active_view: &ViewId,
    selected_room: Option<&str>,
    model: &UiModel,
    closed_queries: &BTreeSet<String>,
) -> ViewId {
    let has_window = |room_id: &str| {
        navigable_room_by_id(&model.rooms, room_id).is_some() && !closed_queries.contains(room_id)
    };
    match active_view {
        ViewId::Status => ViewId::Status,
        ViewId::RoomList if model.room_list.is_some() => ViewId::RoomList,
        ViewId::Room(room_id) if has_window(room_id) => ViewId::Room(room_id.clone()),
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } if projected_thread_by_id(model, room_id, thread_root_event_id).is_some() => {
            ViewId::Thread {
                room_id: room_id.clone(),
                thread_root_event_id: thread_root_event_id.clone(),
            }
        }
        ViewId::Thread { room_id, .. } if has_window(room_id) => ViewId::Room(room_id.clone()),
        ViewId::Room(_) | ViewId::Thread { .. } | ViewId::RoomList => {
            if let Some(room_id) = selected_room.filter(|room_id| has_window(room_id)) {
                ViewId::Room(room_id.to_owned())
            } else if let Some(room) = model.rooms.iter().find(|room| has_window(&room.room_id)) {
                ViewId::Room(room.room_id.clone())
            } else {
                ViewId::Status
            }
        }
    }
}

fn take_stale_room_views(state: &mut WindowState) -> Vec<MdiViewState> {
    let stale_view_ids = state
        .views
        .keys()
        .filter(|view_id| !view_id_is_valid(&state.model, view_id))
        .cloned()
        .collect::<Vec<_>>();

    for view_id in &stale_view_ids {
        state.find_states.remove(view_id);
        cancel_original_jump_for_view(state, view_id);
    }

    let removed = stale_view_ids
        .into_iter()
        .filter_map(|view_id| {
            let view = state.views.remove(&view_id)?;
            state.hwnd_views.remove(&(view.child as isize));
            Some(view)
        })
        .collect::<Vec<_>>();

    let resolved_active_view = resolve_active_view(
        &state.active_view,
        state.model.selected_room.as_deref(),
        &state.model,
        &state.closed_queries,
    );
    if state.active_view != resolved_active_view {
        state.selected_timeline_event = None;
    }
    state.active_view = resolved_active_view;

    removed
}

fn is_joined_message_room(room: &RoomSummary) -> bool {
    room.membership == RoomMembership::Join && !room.is_space
}

fn is_invited_room(room: &RoomSummary) -> bool {
    room.membership == RoomMembership::Invite && !room.is_space
}

fn is_navigable_room(room: &RoomSummary) -> bool {
    is_joined_message_room(room) || is_invited_room(room)
}

/// What kind of mIRC window a navigable room is. The order is mIRC's
/// window order: channels, then queries; invites, which mIRC lacks, last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RoomWindowKind {
    Channel,
    Query,
    Invite,
}

fn room_window_kind(room: &RoomSummary) -> RoomWindowKind {
    assert!(
        is_navigable_room(room),
        "only navigable rooms have windows: {}",
        room.room_id
    );
    match room.membership {
        RoomMembership::Invite => RoomWindowKind::Invite,
        RoomMembership::Join if room.is_direct => RoomWindowKind::Query,
        RoomMembership::Join => RoomWindowKind::Channel,
    }
}

/// The navigable rooms in window order, each kind in model order.
fn rooms_in_window_order(rooms: &[RoomSummary]) -> Vec<&RoomSummary> {
    let mut ordered = rooms
        .iter()
        .filter(|room| is_navigable_room(room))
        .collect::<Vec<_>>();
    ordered.sort_by_key(|room| room_window_kind(room));
    ordered
}

fn is_sendable_room(room: &RoomSummary) -> bool {
    is_joined_message_room(room)
}

fn is_invite_command_target(room: &RoomSummary) -> bool {
    is_invited_room(room)
}

fn room_by_id<'a>(rooms: &'a [RoomSummary], room_id: &str) -> Option<&'a RoomSummary> {
    rooms.iter().find(|room| room.room_id == room_id)
}

fn navigable_room_by_id<'a>(rooms: &'a [RoomSummary], room_id: &str) -> Option<&'a RoomSummary> {
    room_by_id(rooms, room_id).filter(|room| is_navigable_room(room))
}

fn sendable_room_by_id<'a>(rooms: &'a [RoomSummary], room_id: &str) -> Option<&'a RoomSummary> {
    room_by_id(rooms, room_id).filter(|room| is_sendable_room(room))
}

/// A joined room's notification mode, which the core reports for every
/// joined room.
fn joined_room_notification_mode(room: &RoomSummary) -> RoomNotificationMode {
    room.notification_mode
        .expect("a joined room has a notification mode")
}

fn view_id_is_valid_for_rooms(
    rooms: &[RoomSummary],
    room_views: &BTreeMap<String, RoomViewModel>,
    view_id: &ViewId,
) -> bool {
    match view_id {
        ViewId::Status => true,
        ViewId::RoomList => unreachable!("the Rooms List has no activity"),
        ViewId::Room(room_id) => navigable_room_by_id(rooms, room_id).is_some(),
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => {
            sendable_room_by_id(rooms, room_id).is_some()
                && room_views
                    .get(room_id)
                    .is_some_and(|room_view| room_view.threads.contains_key(thread_root_event_id))
        }
    }
}

fn projected_thread_by_id<'a>(
    model: &'a UiModel,
    room_id: &str,
    thread_root_event_id: &str,
) -> Option<&'a ThreadViewModel> {
    sendable_room_by_id(&model.rooms, room_id)?;
    model
        .room_views
        .get(room_id)?
        .threads
        .get(thread_root_event_id)
}

fn view_id_is_valid(model: &UiModel, view_id: &ViewId) -> bool {
    match view_id {
        ViewId::Status => true,
        ViewId::RoomList => model.room_list.is_some(),
        ViewId::Room(room_id) => navigable_room_by_id(&model.rooms, room_id).is_some(),
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => projected_thread_by_id(model, room_id, thread_root_event_id).is_some(),
    }
}

/// A room's name, as the treebar, switchbar and caption show it. An invite
/// is marked, since mIRC has no invite windows to tell it by.
fn room_tree_label(room: &RoomSummary) -> String {
    if is_invited_room(room) {
        format!("{} [invite]", room.display_name)
    } else {
        room.display_name.clone()
    }
}

fn room_navigation_label(model: &UiModel, room: &RoomSummary) -> String {
    let mut label = room_tree_label(room);
    append_room_read_state_tokens(&mut label, model.room_read_states.get(&room.room_id));
    label
}

fn space_tree_label(space: &RoomSummary) -> String {
    space.display_name.clone()
}

/// The Rooms List's switchbar and treebar label.
const ROOM_LIST_TITLE: &str = "Rooms List";

/// The MDI window caption: the short title, with the topic for channels and
/// the other user's ID for queries. A query ignores its room's topic, as
/// mIRC queries have none.
fn view_caption(model: &UiModel, view_id: &ViewId) -> String {
    let title = view_short_title(model, view_id);
    match view_id {
        ViewId::Room(room_id) => {
            let room = room_by_id(&model.rooms, room_id)
                .expect("room view caption must target an existing room");
            match (room_window_kind(room), room.direct_user_ids.as_slice()) {
                (RoomWindowKind::Query, [user_id]) => mdi::query_caption(&title, user_id),
                (RoomWindowKind::Query, _) => title,
                (RoomWindowKind::Channel | RoomWindowKind::Invite, _) => mdi::room_caption(
                    &title,
                    model
                        .room_views
                        .get(room_id)
                        .and_then(|room_view| room_view.topic.as_deref()),
                ),
            }
        }
        ViewId::Status => mdi::status_caption(model.own_user_id.as_deref()),
        ViewId::Thread { .. } => title,
        ViewId::RoomList => model
            .room_list
            .as_ref()
            .expect("the Rooms List window has its listing")
            .caption(),
    }
}

/// The short title the switchbar and treebar show.
fn view_short_title(model: &UiModel, view_id: &ViewId) -> String {
    if let Some((room_id, thread_root_event_id)) = view_id.thread_key() {
        return format!(
            "Thread: {}",
            projected_thread_by_id(model, room_id, thread_root_event_id)
                .expect("thread view activation must target an existing projected thread")
                .label
        );
    }

    match view_id {
        // mIRC labels a connection's Status window with its network; a
        // Matrix user ID names both the account and its server.
        ViewId::Status => model
            .own_user_id
            .clone()
            .unwrap_or_else(|| "Status".to_owned()),
        ViewId::Room(room_id) => {
            let room = room_by_id(&model.rooms, room_id)
                .expect("room view activation must target an existing room");
            room_tree_label(room)
        }
        ViewId::Thread { .. } => unreachable!("thread views are handled above"),
        ViewId::RoomList => ROOM_LIST_TITLE.to_owned(),
    }
}

fn switchbar_button_text(state: &WindowState, view_id: &ViewId) -> String {
    match view_id {
        ViewId::Room(room_id) => {
            let room = room_by_id(&state.model.rooms, room_id)
                .expect("room switchbar button must target an existing room");
            room_navigation_label(&state.model, room)
        }
        ViewId::Status | ViewId::Thread { .. } | ViewId::RoomList => {
            view_short_title(&state.model, view_id)
        }
    }
}

fn switchbar_button_state(state: &WindowState, view_id: &ViewId) -> u8 {
    let mut button_state = TBSTATE_ENABLED as u8;
    if state.active_view == *view_id {
        button_state |= TBSTATE_CHECKED as u8;
    }
    button_state
}

/// The text colour of a view's switchbar button and treebar items: its
/// activity colour, or `None` for the control's own colour.
fn view_activity_text_color(state: &WindowState, view_id: &ViewId) -> Option<u32> {
    let level = state.room_activity.get(view_id)?;
    assert_ne!(
        *view_id, state.active_view,
        "the active view never records activity"
    );
    Some(activity_level_color(*level))
}

/// The custom-draw reply that lets the control paint as usual.
const CDRF_DEFAULT: LRESULT = 0;

/// Asks for per-item notifications, then colours each item's text through
/// `item_color`. Those are the only stages requested.
fn activity_custom_draw(
    draw_stage: u32,
    clr_text: &mut u32,
    item_color: impl FnOnce() -> Option<u32>,
) -> LRESULT {
    match draw_stage {
        CDDS_PREPAINT => CDRF_NOTIFYITEMDRAW as LRESULT,
        CDDS_ITEMPREPAINT => {
            if let Some(color) = item_color() {
                *clr_text = color;
            }
            CDRF_DEFAULT
        }
        stage => panic!("unrequested custom-draw stage {stage:#x}"),
    }
}

/// Switchbar items are identified by their button command id.
fn switchbar_custom_draw(state: &WindowState, draw: &mut NMTBCUSTOMDRAW) -> LRESULT {
    let command_id = draw.nmcd.dwItemSpec;
    activity_custom_draw(draw.nmcd.dwDrawStage, &mut draw.clrText, || {
        let view_id = &state.switchbar_views[command_id - SWITCHBAR_COMMAND_FIRST];
        view_activity_text_color(state, view_id)
    })
}

/// Treebar items are identified by their `HTREEITEM`. Space folders map to
/// no view, so keep the tree's text colour.
fn tree_custom_draw(state: &WindowState, draw: &mut NMTVCUSTOMDRAW) -> LRESULT {
    let tree_item = draw.nmcd.dwItemSpec as isize;
    activity_custom_draw(draw.nmcd.dwDrawStage, &mut draw.clrText, || {
        let view_id = state.tree_items.get(&tree_item)?;
        view_activity_text_color(state, view_id)
    })
}

fn view_identity_from_view_id(view_id: &ViewId) -> ViewIdentity {
    match view_id {
        ViewId::Status => ViewIdentity::Status,
        ViewId::RoomList => ViewIdentity::RoomList,
        ViewId::Room(room_id) => ViewIdentity::Room {
            room_id: room_id.clone(),
        },
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => ViewIdentity::Thread {
            room_id: room_id.clone(),
            thread_root_event_id: thread_root_event_id.clone(),
        },
    }
}

fn view_id_from_identity(identity: &ViewIdentity) -> ViewId {
    match identity {
        ViewIdentity::Status => ViewId::Status,
        ViewIdentity::RoomList => ViewId::RoomList,
        ViewIdentity::Room { room_id } => ViewId::Room(room_id.clone()),
        ViewIdentity::Thread {
            room_id,
            thread_root_event_id,
        } => ViewId::Thread {
            room_id: room_id.clone(),
            thread_root_event_id: thread_root_event_id.clone(),
        },
    }
}

unsafe fn create_mdi_child(
    frame_hwnd: HWND,
    title: &str,
    view_id: &ViewId,
) -> Result<HWND, String> {
    let mdi_client = state_ref(frame_hwnd).controls.mdi_client;
    if mdi_client.is_null() {
        return Err("MDI client control was not created".to_owned());
    }

    let instance = GetModuleHandleW(null());
    let mut class_name = wide_null(MDI_CHILD_CLASS_NAME);
    let mut title_wide = wide_null(title);
    let create_state = Box::new(MdiChildCreateState {
        view_id: view_id.clone(),
    });
    let create_state_ptr = Box::into_raw(create_state);
    let create = MDICREATESTRUCTW {
        szClass: class_name.as_mut_ptr(),
        szTitle: title_wide.as_mut_ptr(),
        hOwner: instance,
        x: CW_USEDEFAULT,
        y: CW_USEDEFAULT,
        cx: CW_USEDEFAULT,
        cy: CW_USEDEFAULT,
        style: WS_CHILD
            | WS_VISIBLE
            | if new_child_should_maximize(mdi_client) {
                WS_MAXIMIZE
            } else {
                0
            },
        lParam: create_state_ptr as isize,
    };
    let child = SendMessageW(
        mdi_client,
        WM_MDICREATE,
        0,
        (&create as *const MDICREATESTRUCTW) as LPARAM,
    ) as HWND;
    drop(Box::from_raw(create_state_ptr));
    if child.is_null() {
        return Err(last_os_error("WM_MDICREATE"));
    }

    Ok(child)
}

/// The first window opens maximized, and later ones follow the active
/// window, as on Windows. Wine's MDI restores the active maximized window
/// instead of maximizing the new one unless the new one asks for it.
unsafe fn new_child_should_maximize(mdi_client: HWND) -> bool {
    let mut active_maximized = 0;
    let active = SendMessageW(
        mdi_client,
        WM_MDIGETACTIVE,
        0,
        &mut active_maximized as *mut i32 as LPARAM,
    );
    active == 0 || active_maximized != 0
}

/// Shows a view's window maximized, normal or minimized.
unsafe fn set_view_window_state(
    hwnd: HWND,
    view_id: &ViewId,
    window_state: MdiWindowState,
) -> Result<(), String> {
    let state = state_ref(hwnd);
    let child = state
        .views
        .get(view_id)
        .ok_or_else(|| format!("view has no window: {view_id:?}"))?
        .child;
    let mdi_client = state.controls.mdi_client;
    match window_state {
        MdiWindowState::Maximized => {
            SendMessageW(mdi_client, WM_MDIMAXIMIZE, child as WPARAM, 0);
        }
        MdiWindowState::Normal => {
            SendMessageW(mdi_client, WM_MDIRESTORE, child as WPARAM, 0);
        }
        MdiWindowState::Minimized => {
            ShowWindow(child, SW_MINIMIZE);
        }
    }
    Ok(())
}

unsafe fn mdi_window_placement(mdi_client: HWND, child: HWND) -> MdiWindowPlacement {
    let state = if IsZoomed(child) != 0 {
        MdiWindowState::Maximized
    } else if IsIconic(child) != 0 {
        MdiWindowState::Minimized
    } else {
        MdiWindowState::Normal
    };
    let mut rect = RECT::default();
    GetWindowRect(child, &mut rect);
    MapWindowPoints(
        null_mut(),
        mdi_client,
        (&mut rect as *mut RECT).cast::<POINT>(),
        2,
    );
    MdiWindowPlacement {
        state,
        x: rect.left,
        y: rect.top,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    }
}

/// Creates the child's content window for its view and its editbox, and
/// lays them out.
unsafe fn create_mdi_child_controls(child: HWND, view_id: &ViewId) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    // Status is a message window like any other, drawn by the same timeline.
    let (class_name, style, ex_style) = match view_id {
        ViewId::Status => (
            TIMELINE_CLASS_NAME,
            WS_CHILD | WS_VISIBLE | WS_VSCROLL,
            WS_EX_CLIENTEDGE,
        ),
        ViewId::Room(_) | ViewId::Thread { .. } => {
            (ROOM_CONTENT_CLASS_NAME, WS_CHILD | WS_VISIBLE, 0)
        }
        // An owner-data list: the rows live in `UiModel.room_list`, and the
        // list asks for each cell's text as it paints.
        ViewId::RoomList => (
            ROOM_LIST_CLASS_NAME,
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | LVS_REPORT
                | LVS_SINGLESEL
                | LVS_SHOWSELALWAYS
                | LVS_OWNERDATA,
            WS_EX_CLIENTEDGE,
        ),
    };
    let content = CreateWindowExW(
        ex_style,
        wide_null(class_name).as_ptr(),
        wide_null("").as_ptr(),
        style,
        0,
        0,
        0,
        0,
        child,
        VIEW_CONTENT_ID as usize as HMENU,
        instance,
        null_mut(),
    );
    if content.is_null() {
        return Err(last_os_error("CreateWindowExW MDI child content"));
    }

    let frame = frame_window_for(child);
    if view_has_editbox(view_id) {
        let edit = create_child_window(
            child,
            EDIT_CLASS_NAME,
            "",
            WS_VISIBLE | COMPOSE_EDIT_STYLE,
            COMPOSE_EDIT_EX_STYLE,
            VIEW_EDIT_ID,
            instance,
            null_mut(),
        )?;
        subclass_compose_edit(frame, edit)?;
    } else {
        add_room_list_columns(content)?;
    }
    // Before any rows: the timeline wraps them in its font.
    set_view_text_font(view_id, child, state_ref(frame).text_font.hfont());

    let system_menu = GetSystemMenu(child, 0);
    if matches!(view_id, ViewId::Status) {
        // As in mIRC, the Status window cannot be closed (see WM_CLOSE), so
        // there is always a window to activate when another one closes.
        if EnableMenuItem(system_menu, SC_CLOSE, MF_BYCOMMAND | MF_GRAYED) == -1 {
            return Err(last_os_error("EnableMenuItem Status SC_CLOSE"));
        }
    }
    // mIRC's window menu has Find Text and the font too. The Rooms List
    // has no text of its own to find. Only a room takes files: a thread
    // cannot hold an attachment yet.
    let find_label = wide_null(SYSMENU_FIND_LABEL);
    let send_file_label = wide_null(SYSMENU_SEND_FILE_LABEL);
    let font_label = wide_null(SYSMENU_FONT_LABEL);
    if AppendMenuW(system_menu, MF_SEPARATOR, 0, null()) == 0
        || (view_has_editbox(view_id)
            && AppendMenuW(system_menu, MF_STRING, SYSMENU_ID_FIND, find_label.as_ptr()) == 0)
        || (matches!(view_id, ViewId::Room(_))
            && AppendMenuW(
                system_menu,
                MF_STRING,
                SYSMENU_ID_SEND_FILE,
                send_file_label.as_ptr(),
            ) == 0)
        || AppendMenuW(system_menu, MF_STRING, SYSMENU_ID_FONT, font_label.as_ptr()) == 0
    {
        return Err(last_os_error(
            "AppendMenuW window menu Find Text..., Send File... and Font...",
        ));
    }

    layout_mdi_child(child);
    Ok(())
}

/// Every editbox shares the EDIT class procedure, so the procedure the
/// first subclass replaced is the one every later subclass replaces.
unsafe fn subclass_compose_edit(frame: HWND, edit: HWND) -> Result<(), String> {
    let previous_proc =
        SetWindowLongPtrW(edit, GWLP_WNDPROC, edit_window_proc as *const () as isize);
    if previous_proc == 0 {
        return Err(last_os_error("SetWindowLongPtrW edit subclass"));
    }
    let previous_proc = std::mem::transmute::<
        isize,
        unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    >(previous_proc);
    let state = state_mut(frame);
    match state.edit_window_proc {
        None => state.edit_window_proc = Some(previous_proc),
        Some(original) => assert!(
            std::ptr::fn_addr_eq(original, previous_proc),
            "every editbox must subclass the same EDIT procedure"
        ),
    }
    Ok(())
}

unsafe fn layout_mdi_child(child: HWND) {
    let mut rect = RECT::default();
    GetClientRect(child, &mut rect);
    // The Rooms List has no editbox: its list fills the window.
    let edit = mdi_child_optional_edit(child);
    let edit_height = edit.map_or(0, |_| {
        state_ref(frame_window_for(child)).text_font.edit_height
    });
    let layout = mdi::child_layout(rect.right - rect.left, rect.bottom - rect.top, edit_height);
    let place = layout.content;
    MoveWindow(
        mdi_child_content(child),
        place.x,
        place.y,
        place.width,
        place.height,
        1,
    );
    if let Some(edit) = edit {
        let place = layout.edit;
        MoveWindow(edit, place.x, place.y, place.width, place.height, 1);
    }
}

/// The child's editbox; only the Rooms List has none.
unsafe fn mdi_child_optional_edit(child: HWND) -> Option<HWND> {
    let edit = GetDlgItem(child, VIEW_EDIT_ID as i32);
    (!edit.is_null()).then_some(edit)
}

/// Where the keyboard goes in an active window: the editbox, as in mIRC,
/// or the Rooms List's list.
unsafe fn mdi_child_focus(child: HWND) -> HWND {
    mdi_child_optional_edit(child).unwrap_or_else(|| mdi_child_content(child))
}

unsafe fn mdi_child_content(child: HWND) -> HWND {
    let content = GetDlgItem(child, VIEW_CONTENT_ID as i32);
    assert!(!content.is_null(), "MDI child must have its content window");
    content
}

unsafe fn mdi_child_edit(child: HWND) -> HWND {
    let edit = GetDlgItem(child, VIEW_EDIT_ID as i32);
    assert!(!edit.is_null(), "MDI child must have its editbox");
    edit
}

unsafe fn room_content_timeline(content: HWND) -> HWND {
    room_content_state_ref(content).timeline
}

/// A view's timeline: the Status content is one, a room or thread content
/// holds one beside its nick list.
unsafe fn view_timeline(view_id: &ViewId, content: HWND) -> HWND {
    match view_id {
        ViewId::Status => content,
        ViewId::Room(_) | ViewId::Thread { .. } => room_content_timeline(content),
        ViewId::RoomList => unreachable!("the Rooms List has no timeline"),
    }
}

unsafe fn room_content_member_list(content: HWND) -> HWND {
    room_content_state_ref(content).members
}

fn unregister_view_by_child(state: &mut WindowState, child: HWND) -> Option<ViewId> {
    let view_id = state.hwnd_views.remove(&(child as isize))?;
    let removed = state
        .views
        .remove(&view_id)
        .expect("HWND-to-view registry must match tracked views");
    state.find_states.remove(&view_id);
    cancel_original_jump_for_view(state, &view_id);
    assert_eq!(
        removed.child, child,
        "tracked child HWND must match registry"
    );
    if state.active_view == view_id {
        state.selected_timeline_event = None;
        state.active_view = resolve_active_view(
            &state.active_view,
            state.model.selected_room.as_deref(),
            &state.model,
            &state.closed_queries,
        );
    }
    Some(view_id)
}

unsafe fn unregister_destroyed_view(child: HWND) {
    let _ = unregister_view_by_child(state_mut(frame_window_for(child)), child);
}

unsafe fn cached_view_is_alive(view: &MdiViewState) -> bool {
    IsWindow(view.child) != 0
        && GetDlgItem(view.child, VIEW_CONTENT_ID as i32) == view.content
        && GetDlgItem(view.child, VIEW_EDIT_ID as i32)
            == view
                .compose
                .as_ref()
                .map_or(null_mut(), |compose| compose.edit)
}

/// Every window has an editbox but the Rooms List, as in mIRC.
fn view_has_editbox(view_id: &ViewId) -> bool {
    match view_id {
        ViewId::Status | ViewId::Room(_) | ViewId::Thread { .. } => true,
        ViewId::RoomList => false,
    }
}

unsafe fn ensure_view(hwnd: HWND, view_id: ViewId) -> Result<HWND, String> {
    let stale_child = state_ref(hwnd)
        .views
        .get(&view_id)
        .and_then(|view| (!cached_view_is_alive(view)).then_some(view.child));
    if let Some(child) = stale_child {
        let removed = unregister_view_by_child(state_mut(hwnd), child)
            .expect("tracked view must exist before stale child cleanup");
        assert_eq!(
            removed, view_id,
            "stale child cleanup must match requested view"
        );
    } else if let Some(view) = state_ref(hwnd).views.get(&view_id) {
        return Ok(view.child);
    }

    let title = view_caption(&state_ref(hwnd).model, &view_id);
    let child = create_mdi_child(hwnd, &title, &view_id)?;
    let view = MdiViewState {
        child,
        content: mdi_child_content(child),
        compose: view_has_editbox(&view_id).then(|| ViewCompose {
            edit: mdi_child_edit(child),
            history: ComposeHistory::default(),
            target: None,
        }),
    };

    let state = state_mut(hwnd);
    state.hwnd_views.insert(child as isize, view_id.clone());
    state.views.insert(view_id, view);
    Ok(child)
}

unsafe fn ensure_status_view(hwnd: HWND) -> Result<HWND, String> {
    ensure_view(hwnd, ViewId::Status)
}

/// `/list`: opens the Rooms List, or reuses it, for a new listing and asks
/// for its first page. The window follows the model: the view sync creates
/// it.
unsafe fn start_room_list(hwnd: HWND, query: RoomListQuery) -> Result<String, String> {
    let state = state_mut(hwnd);
    let room_list = RoomListState::new(query, own_server(&state.model)?);
    enqueue_command(state, Command::ListPublicRooms(room_list.first_request()))?;
    state.model.room_list = Some(room_list);
    sync_views_and_tree(hwnd)?;
    let list = mdi_child_content(ensure_room_list_view(hwnd)?);
    // A new listing starts with nothing selected.
    let unselect = LVITEMW {
        stateMask: LVIS_SELECTED | LVIS_FOCUSED,
        ..LVITEMW::default()
    };
    SendMessageW(
        list,
        LVM_SETITEMSTATE,
        usize::MAX,
        &unselect as *const _ as LPARAM,
    );
    activate_view(hwnd, &ViewId::RoomList, None)?;
    Ok("listing rooms".to_owned())
}

/// Column widths in pixels, for the frame's Fixedsys text.
const ROOM_LIST_COLUMN_WIDTHS: [i32; 4] = [240, 64, 200, 640];

unsafe fn add_room_list_columns(list: HWND) -> Result<(), String> {
    SendMessageW(
        list,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        LVS_EX_FULLROWSELECT as WPARAM,
        LVS_EX_FULLROWSELECT as LPARAM,
    );
    for (index, column) in RoomListColumn::ALL.into_iter().enumerate() {
        let mut title = wide_null(column.title());
        let format = match column {
            RoomListColumn::Users => LVCFMT_RIGHT,
            RoomListColumn::Room | RoomListColumn::Name | RoomListColumn::Topic => LVCFMT_LEFT,
        };
        let lv_column = LVCOLUMNW {
            mask: LVCF_FMT | LVCF_TEXT | LVCF_WIDTH,
            fmt: format,
            cx: ROOM_LIST_COLUMN_WIDTHS[index],
            pszText: title.as_mut_ptr(),
            ..LVCOLUMNW::default()
        };
        if SendMessageW(
            list,
            LVM_INSERTCOLUMNW,
            index,
            &lv_column as *const _ as LPARAM,
        ) != index as LRESULT
        {
            return Err(format!("LVM_INSERTCOLUMNW {} failed", column.title()));
        }
    }
    Ok(())
}

/// Tells the owner-data list how many rows the listing has; it asks for
/// their text as it paints them.
unsafe fn refresh_room_list_rows(state: &WindowState, list: HWND) {
    let rows = state
        .model
        .room_list
        .as_ref()
        .expect("the Rooms List window has its listing")
        .len();
    SendMessageW(list, LVM_SETITEMCOUNT, rows, LVSICF_NOSCROLL as LPARAM);
    InvalidateRect(list, null(), 1);
}

/// The Rooms List's notifications; `None` for any other, which the MDI
/// child's default handling gets.
unsafe fn handle_room_list_notify(frame: HWND, lparam: LPARAM) -> Option<LRESULT> {
    let header = &*(lparam as *const NMHDR);
    match header.code {
        LVN_GETDISPINFOW => {
            let item = &mut (*(lparam as *mut NMLVDISPINFOW)).item;
            // Text is all the list asks the listing for.
            if item.mask & LVIF_TEXT != 0 {
                let text = state_ref(frame)
                    .model
                    .room_list
                    .as_ref()
                    .expect("the Rooms List window has its listing")
                    .cell_text(
                        item.iItem as usize,
                        RoomListColumn::from_index(item.iSubItem as usize),
                    );
                copy_text_to_buffer(&text, item.pszText, item.cchTextMax);
            }
            Some(0)
        }
        LVN_COLUMNCLICK => {
            let column = (*(lparam as *const NMLISTVIEW)).iSubItem as usize;
            state_mut(frame)
                .model
                .room_list
                .as_mut()
                .expect("the Rooms List window has its listing")
                .sort_by(RoomListColumn::from_index(column));
            if let Err(message) = refresh_view_content(frame, &ViewId::RoomList) {
                show_error_message(frame, ROOM_LIST_TITLE, &message);
            }
            Some(0)
        }
        // A double-click below the rows hits no item.
        NM_DBLCLK => {
            let item = (*(lparam as *const NMITEMACTIVATE)).iItem;
            Some(join_room_list_row_reporting(frame, item))
        }
        NM_RETURN => {
            let item = SendMessageW(
                header.hwndFrom,
                LVM_GETNEXTITEM,
                usize::MAX,
                LVNI_SELECTED as LPARAM,
            );
            Some(join_room_list_row_reporting(frame, item as i32))
        }
        _ => None,
    }
}

/// Joins the row at `item`, unless it is -1 (no row); errors go where a
/// typed command's would.
unsafe fn join_room_list_row_reporting(frame: HWND, item: i32) -> LRESULT {
    if item >= 0
        && let Err(message) = join_room_list_row(frame, item as usize)
    {
        let error = CommandError::new("list", message);
        if let Err(message) = report_command_error(frame, error) {
            show_error_message(frame, ROOM_LIST_TITLE, &message);
        }
    }
    0
}

/// Double-click or Enter on a row: a room already joined comes to the
/// front; any other is joined, and opens when the join lands.
unsafe fn join_room_list_row(frame: HWND, display_index: usize) -> Result<(), String> {
    let (room_id, target, via) = {
        let room_list = state_ref(frame)
            .model
            .room_list
            .as_ref()
            .expect("the Rooms List window has its listing");
        let (target, via) = room_list.join_target(display_index);
        (room_list.row(display_index).room_id.clone(), target, via)
    };
    if navigable_room_by_id(&state_ref(frame).model.rooms, &room_id).is_some() {
        return activate_view(frame, &ViewId::Room(room_id), None);
    }
    let status = submit_join_room(state_mut(frame), target, via)?;
    set_local_command_status(frame, status)
}

unsafe fn ensure_room_list_view(hwnd: HWND) -> Result<HWND, String> {
    if state_ref(hwnd).model.room_list.is_none() {
        return Err("no rooms are listed: use /list".to_owned());
    }
    ensure_view(hwnd, ViewId::RoomList)
}

unsafe fn ensure_room_view(hwnd: HWND, room_id: &str) -> Result<HWND, String> {
    if navigable_room_by_id(&state_ref(hwnd).model.rooms, room_id).is_none() {
        return Err(format!("unknown room id: {room_id}"));
    }

    ensure_view(hwnd, ViewId::Room(room_id.to_owned()))
}

unsafe fn ensure_thread_view(
    hwnd: HWND,
    room_id: &str,
    thread_root_event_id: &str,
) -> Result<HWND, String> {
    if sendable_room_by_id(&state_ref(hwnd).model.rooms, room_id).is_none() {
        return Err(format!("unknown room id: {room_id}"));
    }
    if projected_thread_by_id(&state_ref(hwnd).model, room_id, thread_root_event_id).is_none() {
        return Err(format!(
            "unknown thread root event id for {room_id}: {thread_root_event_id}"
        ));
    }

    ensure_view(
        hwnd,
        ViewId::Thread {
            room_id: room_id.to_owned(),
            thread_root_event_id: thread_root_event_id.to_owned(),
        },
    )
}

unsafe fn destroy_stale_room_views(hwnd: HWND) {
    let (mdi_client, stale_views) = {
        let state = state_mut(hwnd);
        (state.controls.mdi_client, take_stale_room_views(state))
    };

    // WM_MDIDESTROY, not WM_CLOSE: closing a room window asks to leave it.
    for view in stale_views {
        SendMessageW(mdi_client, WM_MDIDESTROY, view.child as WPARAM, 0);
    }
}

/// The replied-to messages `activities` quote that the room view neither
/// has nor has asked for.
fn collect_reply_target_requests<'a>(
    room_view: &RoomViewModel,
    activities: impl IntoIterator<Item = &'a RoomActivity>,
) -> BTreeSet<String> {
    activities
        .into_iter()
        .filter_map(|activity| activity.relation.as_ref())
        .filter_map(replies::reply_quote_target)
        .filter(|event_id| {
            room_view
                .find_activity_index_by_event_id(event_id)
                .is_none()
                && !room_view.reply_targets.contains_key(*event_id)
        })
        .map(str::to_owned)
        .collect()
}

/// Asks the server for replied-to messages. Only a connected session can:
/// events a synthetic load replays come from no server, and without a
/// session there is nobody to ask.
fn dispatch_reply_target_fetches(
    state: &mut WindowState,
    room_id: &str,
    event_ids: BTreeSet<String>,
) -> Result<(), String> {
    if state.synthetic_load_in_progress
        || !matches!(
            state.model.connection,
            ConnectionState::SignedIn | ConnectionState::Syncing
        )
    {
        return Ok(());
    }
    for event_id in event_ids {
        enqueue_command(
            state,
            Command::FetchRoomEvent(FetchRoomEventRequest {
                room_id: room_id.to_owned(),
                event_id: event_id.clone(),
            }),
        )?;
        state
            .model
            .room_views
            .get_mut(room_id)
            .expect("reply target fetches require an existing room view")
            .reply_targets
            .insert(event_id, ReplyTargetState::Pending);
    }
    Ok(())
}

/// A window's editbox stays armed only with a message the window shows: a
/// redaction, or `/clear`, disarms it.
fn disarm_vanished_compose_target(state: &mut WindowState, view_id: &ViewId) {
    let compose = state
        .views
        .get_mut(view_id)
        .expect("a refreshed view has a window")
        .compose_mut();
    if compose
        .target
        .as_ref()
        .is_some_and(|target| window_message(&state.model, view_id, target.event_id()).is_none())
    {
        compose.target = None;
    }
}

/// The indicator row of an armed editbox, if `view_id`'s is.
fn armed_compose_target_row(state: &WindowState, view_id: &ViewId) -> Option<TimelineRow> {
    state.views[view_id]
        .compose()
        .target
        .as_ref()
        .map(|target| compose_target_row(&state.model, view_id, target))
}

unsafe fn refresh_view_content(hwnd: HWND, view_id: &ViewId) -> Result<(), String> {
    let (child, content, title) = {
        let state = state_ref(hwnd);
        let view = state
            .views
            .get(view_id)
            .ok_or_else(|| "view state was not tracked".to_owned())?;
        let title = view_caption(&state.model, view_id);
        (view.child, view.content, title)
    };

    let title = wide_null(&title);
    SendMessageW(child, WM_SETTEXT, 0, title.as_ptr() as LPARAM);
    if *view_id == ViewId::RoomList {
        // No text and no editbox: only the list's rows change.
        refresh_room_list_rows(state_ref(hwnd), content);
        return Ok(());
    }
    disarm_vanished_compose_target(state_mut(hwnd), view_id);
    match view_id {
        ViewId::RoomList => unreachable!("the Rooms List returned above"),
        ViewId::Status => {
            let (rows, indent_sample) = {
                let state = state_ref(hwnd);
                let timestamps = state.timestamps();
                (
                    build_status_timeline_rows(&state.model.status_log, timestamps),
                    timestamps.indent_sample(),
                )
            };
            set_timeline_rows(content, &rows, indent_sample);
            recompute_find_for_view(state_mut(hwnd), view_id, &rows);
            apply_find_to_active_view(hwnd, view_id)?;
            InvalidateRect(content, null(), 1);
        }
        ViewId::Room(room_id) => {
            let (rows, indent_sample, member_rows, image_requests, reply_target_requests) = {
                let state = state_ref(hwnd);
                let room = room_by_id(&state.model.rooms, room_id)
                    .expect("room view must target an existing room");
                let room_view = state.model.room_views.get(room_id);
                let timestamps = state.timestamps();
                let mut rows = build_room_timeline_rows(&state.model, room_id, timestamps);
                rows.extend(armed_compose_target_row(state, view_id));
                let (image_requests, reply_target_requests) = match room_view {
                    Some(room_view) if is_sendable_room(room) => (
                        collect_lazy_room_preview_media_requests(
                            room_view,
                            room_view.activities.iter(),
                        ),
                        collect_reply_target_requests(room_view, room_view.activities.iter()),
                    ),
                    _ => (Vec::new(), BTreeSet::new()),
                };
                let member_rows = if is_sendable_room(room) {
                    room_member_view_rows(
                        room_view
                            .map(|room_view| room_view.members.as_slice())
                            .unwrap_or_default(),
                    )
                } else {
                    Vec::new()
                };
                (
                    rows,
                    timestamps.indent_sample(),
                    member_rows,
                    image_requests,
                    reply_target_requests,
                )
            };
            dispatch_lazy_room_preview_media_requests(state_mut(hwnd), room_id, image_requests)?;
            dispatch_reply_target_fetches(state_mut(hwnd), room_id, reply_target_requests)?;
            validate_selected_timeline_event(state_mut(hwnd), view_id, &rows);
            let timeline = room_content_timeline(content);
            set_timeline_rows(timeline, &rows, indent_sample);
            recompute_find_for_view(state_mut(hwnd), view_id, &rows);
            if state_ref(hwnd).active_view == *view_id {
                apply_find_to_active_view(hwnd, view_id)?;
            } else {
                clear_find_projection_for_timeline(timeline);
            }
            set_room_member_list(room_content_member_list(content), &member_rows);
            InvalidateRect(content, null(), 1);
            InvalidateRect(timeline, null(), 1);
            InvalidateRect(room_content_member_list(content), null(), 1);
        }
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => {
            let (rows, indent_sample, member_rows, image_requests, reply_target_requests) = {
                let state = state_ref(hwnd);
                let room_view = state
                    .model
                    .room_views
                    .get(room_id)
                    .expect("thread view must target an existing room view");
                let timestamps = state.timestamps();
                let mut rows = build_thread_timeline_rows(
                    &state.model,
                    room_id,
                    thread_root_event_id,
                    timestamps,
                );
                rows.extend(armed_compose_target_row(state, view_id));
                let thread_activities = || {
                    room_view.activities.iter().filter(|activity| {
                        room_activity_belongs_to_thread(activity, thread_root_event_id)
                    })
                };
                let image_requests =
                    collect_lazy_room_preview_media_requests(room_view, thread_activities());
                let reply_target_requests =
                    collect_reply_target_requests(room_view, thread_activities());
                let member_rows = room_member_view_rows(room_view.members.as_slice());
                (
                    rows,
                    timestamps.indent_sample(),
                    member_rows,
                    image_requests,
                    reply_target_requests,
                )
            };
            dispatch_lazy_room_preview_media_requests(state_mut(hwnd), room_id, image_requests)?;
            dispatch_reply_target_fetches(state_mut(hwnd), room_id, reply_target_requests)?;
            validate_selected_timeline_event(state_mut(hwnd), view_id, &rows);
            let timeline = room_content_timeline(content);
            set_timeline_rows(timeline, &rows, indent_sample);
            recompute_find_for_view(state_mut(hwnd), view_id, &rows);
            if state_ref(hwnd).active_view == *view_id {
                apply_find_to_active_view(hwnd, view_id)?;
            } else {
                clear_find_projection_for_timeline(timeline);
            }
            set_room_member_list(room_content_member_list(content), &member_rows);
            InvalidateRect(content, null(), 1);
            InvalidateRect(timeline, null(), 1);
            InvalidateRect(room_content_member_list(content), null(), 1);
        }
    }
    Ok(())
}

unsafe fn refresh_every_view_content(hwnd: HWND) -> Result<(), String> {
    let view_ids = state_ref(hwnd).views.keys().cloned().collect::<Vec<_>>();
    for view_id in &view_ids {
        refresh_view_content(hwnd, view_id)?;
    }
    Ok(())
}

struct AppliedActivation {
    child: HWND,
    cleared_view_activity: bool,
    rebuild_switchbar_after_activation: bool,
}

unsafe fn apply_view_activation_state(
    hwnd: HWND,
    view_id: &ViewId,
    preferred_tree_item: Option<isize>,
) -> Result<AppliedActivation, String> {
    let rebuild_switchbar_after_activation = !state_ref(hwnd).switchbar_views.contains(view_id);
    let child = match view_id {
        ViewId::Status => {
            state_mut(hwnd).model.selected_room = None;
            ensure_status_view(hwnd)?
        }
        ViewId::RoomList => {
            state_mut(hwnd).model.selected_room = None;
            ensure_room_list_view(hwnd)?
        }
        ViewId::Room(room_id) => {
            let state = state_mut(hwnd);
            select_room(state, room_id)?;
            // Activating a closed query reopens its window.
            state.closed_queries.remove(room_id);
            ensure_room_view(hwnd, room_id)?
        }
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => {
            select_room(state_mut(hwnd), room_id)?;
            ensure_thread_view(hwnd, room_id, thread_root_event_id)?
        }
    };
    let cleared_view_activity = handle_view_activation(state_mut(hwnd), view_id)?;

    {
        let state = state_mut(hwnd);
        if state.active_view != *view_id {
            state.selected_timeline_event = None;
        }
        state.active_view = view_id.clone();
    }

    refresh_tree_activity_colors(hwnd)?;
    refresh_view_content(hwnd, view_id)?;
    select_tree_item(hwnd, view_id, preferred_tree_item);

    Ok(AppliedActivation {
        child,
        cleared_view_activity,
        rebuild_switchbar_after_activation,
    })
}

unsafe fn finish_view_activation_chrome(
    hwnd: HWND,
    applied: &AppliedActivation,
) -> Result<(), String> {
    if applied.cleared_view_activity || applied.rebuild_switchbar_after_activation {
        rebuild_switchbar(hwnd)?;
    } else {
        refresh_switchbar_state(hwnd)?;
    }
    refresh_status_bar(hwnd)?;
    SetFocus(mdi_child_focus(applied.child));
    Ok(())
}

unsafe fn activate_view(
    hwnd: HWND,
    view_id: &ViewId,
    preferred_tree_item: Option<isize>,
) -> Result<(), String> {
    let applied = apply_view_activation_state(hwnd, view_id, preferred_tree_item)?;

    let mdi_client = state_ref(hwnd).controls.mdi_client;
    SendMessageW(mdi_client, WM_MDIACTIVATE, applied.child as WPARAM, 0);
    // Activating a minimized window must show it, as in mIRC. MDI itself
    // keeps a maximized window's successor maximized.
    if IsIconic(applied.child) != 0 {
        SendMessageW(mdi_client, WM_MDIRESTORE, applied.child as WPARAM, 0);
    }
    finish_view_activation_chrome(hwnd, &applied)?;
    Ok(())
}

unsafe fn handle_native_mdi_child_activation(child: HWND) -> Result<(), String> {
    let mdi_client = GetParent(child);
    let frame = GetParent(mdi_client);
    let Some(view_id) = state_ref(frame).hwnd_views.get(&(child as isize)).cloned() else {
        return Ok(());
    };
    if state_ref(frame).active_view == view_id {
        return Ok(());
    }

    let applied = apply_view_activation_state(frame, &view_id, None)?;
    finish_view_activation_chrome(frame, &applied)
}

unsafe fn sync_views_and_tree(hwnd: HWND) -> Result<(), String> {
    ensure_status_view(hwnd)?;

    let room_ids = {
        let state = state_ref(hwnd);
        state
            .model
            .rooms
            .iter()
            .filter(|room| is_navigable_room(room) && !state.closed_queries.contains(&room.room_id))
            .map(|room| room.room_id.clone())
            .collect::<Vec<_>>()
    };
    destroy_stale_room_views(hwnd);
    for room_id in room_ids {
        ensure_room_view(hwnd, &room_id)?;
    }
    if state_ref(hwnd).model.room_list.is_some() {
        ensure_room_list_view(hwnd)?;
    }

    refresh_every_view_content(hwnd)?;

    let active_view = {
        let state = state_mut(hwnd);
        let active = resolve_active_view(
            &state.active_view,
            state.model.selected_room.as_deref(),
            &state.model,
            &state.closed_queries,
        );
        if state.active_view != active {
            state.selected_timeline_event = None;
        }
        state.active_view = active.clone();
        active
    };
    refresh_tree(hwnd);
    rebuild_switchbar(hwnd)?;
    refresh_status_bar(hwnd)?;
    activate_view(hwnd, &active_view, None)
}

unsafe fn refresh_tree(hwnd: HWND) {
    let state = state_mut(hwnd);
    let tree = state.controls.tree;
    if tree.is_null() {
        return;
    }

    SendMessageW(tree, TVM_DELETEITEM, 0, TVI_ROOT as LPARAM);
    state.tree_items.clear();
    state.primary_tree_items.clear();

    let layout = navigation_layout(&state.model);
    insert_nav_item(state, TVI_ROOT, &layout);
}

/// Inserts `item` and its subtree, mapping each view's item, and expands
/// every item that has children.
unsafe fn insert_nav_item(state: &mut WindowState, parent: isize, item: &NavItem) {
    let tree = state.controls.tree;
    let tree_item = insert_tree_item(tree, parent, &item.label);
    if let Some(view_id) = &item.view {
        state.tree_items.insert(tree_item, view_id.clone());
        state
            .primary_tree_items
            .entry(view_id.clone())
            .or_insert(tree_item);
    }
    for child in &item.children {
        insert_nav_item(state, tree_item, child);
    }
    if !item.children.is_empty() {
        expand_tree_item(tree, tree_item);
    }
}

unsafe fn select_tree_item(hwnd: HWND, view_id: &ViewId, preferred_tree_item: Option<isize>) {
    let (tree, item) = {
        let state = state_ref(hwnd);
        let item = preferred_tree_item
            .filter(|item| {
                state
                    .tree_items
                    .get(item)
                    .is_some_and(|mapped_view| mapped_view == view_id)
            })
            .or_else(|| state.primary_tree_items.get(view_id).copied());
        (state.controls.tree, item)
    };
    let Some(item) = item else {
        return;
    };
    let current = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CARET as WPARAM, 0) as isize;
    if current == item {
        return;
    }

    SendMessageW(tree, TVM_SELECTITEM, TVGN_CARET as WPARAM, item as LPARAM);
}

unsafe fn insert_tree_item(tree: HWND, parent: isize, text: &str) -> isize {
    let mut text = wide_null(text);
    let item = TVITEMEXW {
        mask: TVIF_TEXT,
        pszText: text.as_mut_ptr(),
        cchTextMax: text.len() as i32,
        ..zeroed()
    };
    let insert = TVINSERTSTRUCTW {
        hParent: parent as _,
        hInsertAfter: TVI_LAST as _,
        Anonymous: TVINSERTSTRUCTW_0 { itemex: item },
    };

    SendMessageW(
        tree,
        TVM_INSERTITEMW,
        0,
        (&insert as *const TVINSERTSTRUCTW) as LPARAM,
    ) as isize
}

unsafe fn expand_tree_item(tree: HWND, item: isize) {
    SendMessageW(tree, TVM_EXPAND, TVE_EXPAND as WPARAM, item as LPARAM);
}

/// One treebar item: a window's view, or a space folder, which has none.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NavItem {
    label: String,
    view: Option<ViewId>,
    children: Vec<NavItem>,
}

/// The treebar, laid out like mIRC's: the Status window is the root, and the
/// windows sit under it. Joined spaces become folders holding their rooms
/// (a room in two spaces appears in both), then come the rooms in no space
/// in window order (channels, queries, invites). Threads sit under their
/// room. Closing a window does not remove its room: the tree is the room
/// navigator, and selecting an item opens its window.
fn navigation_layout(model: &UiModel) -> NavItem {
    let joined = model
        .rooms
        .iter()
        .filter(|room| room.membership == RoomMembership::Join)
        .map(|room| (room.room_id.as_str(), room))
        .collect::<BTreeMap<_, _>>();
    let space_children = joined
        .values()
        .filter(|room| room.is_space)
        .map(|space| {
            (
                space.room_id.as_str(),
                space
                    .space_children
                    .iter()
                    .filter(|child| joined.contains_key(child.room_id.as_str()))
                    .map(|child| child.room_id.as_str())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let spaced_ids = space_children
        .values()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();

    // Top-level folders: spaces that are nobody's child, in model order, then
    // one entry point into each cycle of spaces that only contain each other.
    let mut folder_ids = model
        .rooms
        .iter()
        .filter(|room| space_children.contains_key(room.room_id.as_str()))
        .filter(|space| !spaced_ids.contains(space.room_id.as_str()))
        .map(|space| space.room_id.as_str())
        .collect::<Vec<_>>();
    let mut covered = BTreeSet::new();
    for folder_id in &folder_ids {
        mark_reachable_spaces(&mut covered, &joined, &space_children, folder_id);
    }
    for space in model
        .rooms
        .iter()
        .filter(|room| space_children.contains_key(room.room_id.as_str()))
    {
        if !covered.contains(space.room_id.as_str()) {
            folder_ids.push(&space.room_id);
            mark_reachable_spaces(&mut covered, &joined, &space_children, &space.room_id);
        }
    }

    let mut children = folder_ids
        .into_iter()
        .map(|folder_id| space_nav_item(model, &joined, &space_children, folder_id, &[]))
        .collect::<Vec<_>>();
    children.extend(
        rooms_in_window_order(&model.rooms)
            .into_iter()
            .filter(|room| !spaced_ids.contains(room.room_id.as_str()))
            .map(|room| room_nav_item(model, room)),
    );
    if model.room_list.is_some() {
        children.push(NavItem {
            label: ROOM_LIST_TITLE.to_owned(),
            view: Some(ViewId::RoomList),
            children: Vec::new(),
        });
    }
    NavItem {
        label: view_short_title(model, &ViewId::Status),
        view: Some(ViewId::Status),
        children,
    }
}

fn mark_reachable_spaces<'a>(
    covered: &mut BTreeSet<&'a str>,
    joined: &BTreeMap<&str, &'a RoomSummary>,
    space_children: &BTreeMap<&str, Vec<&'a str>>,
    root_id: &'a str,
) {
    let mut stack = vec![root_id];
    while let Some(space_id) = stack.pop() {
        if !covered.insert(space_id) {
            continue;
        }
        stack.extend(
            space_children[space_id]
                .iter()
                .rev()
                .copied()
                .filter(|child_id| joined[child_id].is_space),
        );
    }
}

/// A space folder with its rooms and nested folders. A space already on the
/// path from the top-level folder shows as an empty folder, which ends a
/// cycle.
fn space_nav_item(
    model: &UiModel,
    joined: &BTreeMap<&str, &RoomSummary>,
    space_children: &BTreeMap<&str, Vec<&str>>,
    space_id: &str,
    ancestors: &[&str],
) -> NavItem {
    let mut chain = ancestors.to_vec();
    chain.push(space_id);
    let children = space_children[space_id]
        .iter()
        .map(|child_id| {
            let child = joined[child_id];
            if !child.is_space {
                room_nav_item(model, child)
            } else if chain.contains(child_id) {
                NavItem {
                    label: space_tree_label(child),
                    view: None,
                    children: Vec::new(),
                }
            } else {
                space_nav_item(model, joined, space_children, child_id, &chain)
            }
        })
        .collect();
    NavItem {
        label: space_tree_label(joined[space_id]),
        view: None,
        children,
    }
}

/// A room's item, with its threads under it; an invite has none.
fn room_nav_item(model: &UiModel, room: &RoomSummary) -> NavItem {
    let children = match (is_sendable_room(room), model.room_views.get(&room.room_id)) {
        (true, Some(room_view)) => room_view
            .threads
            .iter()
            .map(|(thread_root_event_id, thread)| NavItem {
                label: thread.label.clone(),
                view: Some(ViewId::Thread {
                    room_id: room.room_id.clone(),
                    thread_root_event_id: thread_root_event_id.clone(),
                }),
                children: Vec::new(),
            })
            .collect(),
        _ => Vec::new(),
    };
    NavItem {
        label: room_navigation_label(model, room),
        view: Some(ViewId::Room(room.room_id.clone())),
        children,
    }
}

fn split_rendered_lines(text: &str) -> Vec<String> {
    rendered_line_ranges(text)
        .into_iter()
        .map(|line| text[line].to_owned())
        .collect()
}

fn normalize_timeline_text(text: &str) -> (String, Vec<String>) {
    let lines = split_rendered_lines(text);
    let normalized = render_view_text(&lines);
    (normalized, lines)
}

/// The switchbar order, mIRC's: Status, channels, queries, then invites,
/// each room followed by its open threads; then views of rooms that left.
fn ordered_view_ids(state: &WindowState) -> Vec<ViewId> {
    let mut ordered = Vec::new();
    if state.views.contains_key(&ViewId::Status) {
        ordered.push(ViewId::Status);
    }

    for room in rooms_in_window_order(&state.model.rooms) {
        let view_id = ViewId::Room(room.room_id.clone());
        if state.views.contains_key(&view_id) {
            ordered.push(view_id);
        }
        if let Some(room_view) = state.model.room_views.get(&room.room_id) {
            for thread_root_event_id in room_view.threads.keys() {
                let thread_view_id = ViewId::Thread {
                    room_id: room.room_id.clone(),
                    thread_root_event_id: thread_root_event_id.clone(),
                };
                if state.views.contains_key(&thread_view_id) {
                    ordered.push(thread_view_id);
                }
            }
        }
    }

    let mut remaining = state
        .views
        .keys()
        .filter_map(|view_id| match view_id {
            ViewId::Status | ViewId::RoomList => None,
            ViewId::Room(room_id)
                if navigable_room_by_id(&state.model.rooms, room_id).is_none() =>
            {
                Some(room_id.clone())
            }
            ViewId::Room(_) => None,
            ViewId::Thread { .. } => None,
        })
        .collect::<Vec<_>>();
    remaining.sort();
    ordered.extend(remaining.into_iter().map(ViewId::Room));
    if state.views.contains_key(&ViewId::RoomList) {
        ordered.push(ViewId::RoomList);
    }
    ordered
}

unsafe fn rebuild_switchbar(hwnd: HWND) -> Result<(), String> {
    let switchbar = state_ref(hwnd).controls.switchbar;
    if switchbar.is_null() {
        return Err("switchbar control was not created".to_owned());
    }

    loop {
        let count = SendMessageW(switchbar, TB_BUTTONCOUNT, 0, 0);
        if count < 0 {
            return Err("failed to query switchbar button count".to_owned());
        }
        if count == 0 {
            break;
        }
        if SendMessageW(switchbar, TB_DELETEBUTTON, 0, 0) == 0 {
            return Err("failed to delete switchbar button".to_owned());
        }
    }

    let view_ids = ordered_view_ids(state_ref(hwnd));
    let button_count = view_ids.len();
    if SWITCHBAR_COMMAND_FIRST
        .checked_add(button_count.saturating_sub(1))
        .is_some_and(|command_id| command_id > SWITCHBAR_COMMAND_LAST)
    {
        return Err("switchbar command range exhausted".to_owned());
    }

    let labels = {
        let state = state_ref(hwnd);
        view_ids
            .iter()
            .map(|view_id| wide_null(&switchbar_button_text(state, view_id)))
            .collect::<Vec<_>>()
    };
    let button_states = {
        let state = state_ref(hwnd);
        view_ids
            .iter()
            .map(|view_id| switchbar_button_state(state, view_id))
            .collect::<Vec<_>>()
    };
    let buttons = view_ids
        .iter()
        .enumerate()
        .map(|(index, _)| TBBUTTON {
            iBitmap: I_IMAGENONE,
            idCommand: (SWITCHBAR_COMMAND_FIRST + index) as i32,
            fsState: button_states[index],
            fsStyle: (BTNS_CHECK | BTNS_SHOWTEXT) as u8,
            iString: labels[index].as_ptr() as isize,
            ..TBBUTTON::default()
        })
        .collect::<Vec<_>>();

    // `switchbar_views` mirrors the buttons, indexed by command id: switch
    // it while the toolbar is empty, so a paint never sees a button without
    // its view.
    state_mut(hwnd).switchbar_views = view_ids;
    if !buttons.is_empty()
        && SendMessageW(
            switchbar,
            TB_ADDBUTTONSW,
            buttons.len(),
            buttons.as_ptr() as LPARAM,
        ) == 0
    {
        return Err("failed to add switchbar buttons".to_owned());
    }
    // More or fewer buttons may take more or fewer rows.
    layout_controls(hwnd);
    refresh_switchbar_state(hwnd)
}

unsafe fn refresh_switchbar_state(hwnd: HWND) -> Result<(), String> {
    let switchbar = state_ref(hwnd).controls.switchbar;
    if switchbar.is_null() {
        return Err("switchbar control was not created".to_owned());
    }

    let view_ids = state_ref(hwnd).switchbar_views.clone();
    for (index, view_id) in view_ids.iter().enumerate() {
        let command_id = SWITCHBAR_COMMAND_FIRST + index;
        let checked = state_ref(hwnd).active_view == *view_id;
        // TB_SETSTATE would replace the whole state, wrap bit included,
        // which only the toolbar's layout sets.
        if SendMessageW(switchbar, TB_CHECKBUTTON, command_id, checked as LPARAM) == 0 {
            return Err(format!("failed to refresh switchbar button {command_id}"));
        }
    }
    // Repaint for the activity colours, which custom draw reads.
    InvalidateRect(switchbar, null(), 1);

    Ok(())
}

/// Repaints the treebar for the activity colours, which custom draw reads.
unsafe fn refresh_tree_activity_colors(hwnd: HWND) -> Result<(), String> {
    let tree = state_ref(hwnd).controls.tree;
    if tree.is_null() {
        return Err("tree control was not created".to_owned());
    }
    InvalidateRect(tree, null(), 1);
    Ok(())
}

unsafe fn read_tree_item_text(tree: HWND, item: isize) -> Result<String, String> {
    let mut buffer = vec![0u16; 1024];
    let mut tree_item = TVITEMEXW {
        mask: TVIF_TEXT,
        hItem: item as _,
        pszText: buffer.as_mut_ptr(),
        cchTextMax: buffer.len() as i32,
        ..zeroed()
    };

    if SendMessageW(
        tree,
        TVM_GETITEMW,
        0,
        (&mut tree_item as *mut TVITEMEXW) as LPARAM,
    ) == 0
    {
        return Err(format!(
            "failed to read tree item text for handle 0x{item:x}"
        ));
    }

    let text_len = buffer
        .iter()
        .position(|ch| *ch == 0)
        .unwrap_or(buffer.len());
    Ok(String::from_utf16_lossy(&buffer[..text_len]))
}

/// Space folders map to no view, so carry no activity.
unsafe fn read_navigation_node(
    state: &WindowState,
    tree: HWND,
    item: isize,
) -> Result<NavigationNode, String> {
    let text = read_tree_item_text(tree, item)?;
    let view_id = state.tree_items.get(&item);
    let activity = view_id
        .and_then(|view_id| state.room_activity.get(view_id))
        .copied();
    let mut children = Vec::new();
    let mut child =
        SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CHILD as WPARAM, item as LPARAM) as isize;
    while child != 0 {
        children.push(read_navigation_node(state, tree, child)?);
        child = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_NEXT as WPARAM, child as LPARAM) as isize;
    }

    Ok(NavigationNode {
        text,
        view: view_id.map(view_identity_from_view_id),
        activity,
        children,
    })
}

unsafe fn dump_navigation_tree(hwnd: HWND) -> Result<NavigationNode, String> {
    let tree = state_ref(hwnd).controls.tree;
    if tree.is_null() {
        return Err("tree control was not created".to_owned());
    }

    let root = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_ROOT as WPARAM, 0) as isize;
    if root == 0 {
        return Err("navigation tree root was missing".to_owned());
    }

    read_navigation_node(state_ref(hwnd), tree, root)
}

fn format_navigation_path(path: &[NavigationPathSegment]) -> String {
    path.iter()
        .map(|segment| {
            if segment.same_text_index == 0 {
                segment.text.clone()
            } else {
                format!("{} [{}]", segment.text, segment.same_text_index)
            }
        })
        .collect::<Vec<_>>()
        .join(" > ")
}

unsafe fn same_text_index_for_tree_item(tree: HWND, item: isize) -> Result<u32, String> {
    let parent =
        SendMessageW(tree, TVM_GETNEXTITEM, TVGN_PARENT as WPARAM, item as LPARAM) as isize;
    let target_text = read_tree_item_text(tree, item)?;
    let mut current = if parent == 0 {
        SendMessageW(tree, TVM_GETNEXTITEM, TVGN_ROOT as WPARAM, 0) as isize
    } else {
        SendMessageW(
            tree,
            TVM_GETNEXTITEM,
            TVGN_CHILD as WPARAM,
            parent as LPARAM,
        ) as isize
    };
    let mut same_text_index = 0u32;
    while current != 0 {
        if read_tree_item_text(tree, current)? == target_text {
            if current == item {
                return Ok(same_text_index);
            }
            same_text_index += 1;
        }
        current = SendMessageW(
            tree,
            TVM_GETNEXTITEM,
            TVGN_NEXT as WPARAM,
            current as LPARAM,
        ) as isize;
    }

    Err(format!(
        "tree item 0x{item:x} was not found among its siblings"
    ))
}

unsafe fn dump_selected_navigation_path(
    hwnd: HWND,
) -> Result<SelectedNavigationPathResponse, String> {
    let tree = state_ref(hwnd).controls.tree;
    if tree.is_null() {
        return Err("tree control was not created".to_owned());
    }

    let mut item = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CARET as WPARAM, 0) as isize;
    if item == 0 {
        return Err("navigation tree selection was missing".to_owned());
    }
    let view = state_ref(hwnd)
        .tree_items
        .get(&item)
        .map(view_identity_from_view_id);

    let mut path = Vec::new();
    while item != 0 {
        path.push(NavigationPathSegment {
            text: read_tree_item_text(tree, item)?,
            same_text_index: same_text_index_for_tree_item(tree, item)?,
        });
        item = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_PARENT as WPARAM, item as LPARAM) as isize;
    }
    path.reverse();
    Ok(SelectedNavigationPathResponse { path, view })
}

unsafe fn select_navigation_path(hwnd: HWND, path: &[NavigationPathSegment]) -> Result<(), String> {
    if path.is_empty() {
        return Err("navigation path must not be empty".to_owned());
    }

    let tree = state_ref(hwnd).controls.tree;
    if tree.is_null() {
        return Err("tree control was not created".to_owned());
    }

    let mut current = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_ROOT as WPARAM, 0) as isize;
    if current == 0 {
        return Err("navigation tree root was missing".to_owned());
    }

    let root_text = read_tree_item_text(tree, current)?;
    let root_same_text_index = same_text_index_for_tree_item(tree, current)?;
    if root_text != path[0].text || root_same_text_index != path[0].same_text_index {
        return Err(format!(
            "unknown navigation path: {}",
            format_navigation_path(path)
        ));
    }

    for segment in &path[1..] {
        let mut child = SendMessageW(
            tree,
            TVM_GETNEXTITEM,
            TVGN_CHILD as WPARAM,
            current as LPARAM,
        ) as isize;
        let mut matched_child = None;
        let mut same_text_index = 0u32;
        while child != 0 {
            if read_tree_item_text(tree, child)? == segment.text {
                if same_text_index == segment.same_text_index {
                    matched_child = Some(child);
                    break;
                }
                same_text_index += 1;
            }
            child =
                SendMessageW(tree, TVM_GETNEXTITEM, TVGN_NEXT as WPARAM, child as LPARAM) as isize;
        }

        current = matched_child
            .ok_or_else(|| format!("unknown navigation path: {}", format_navigation_path(path)))?;
    }

    select_navigation_item(hwnd, tree, current)
}

/// Selects the view's first tree item: a room listed under two spaces has two
/// items, and `primary_tree_items` holds the first.
unsafe fn select_navigation_view(hwnd: HWND, identity: &ViewIdentity) -> Result<(), String> {
    let tree = state_ref(hwnd).controls.tree;
    let item = *state_ref(hwnd)
        .primary_tree_items
        .get(&view_id_from_identity(identity))
        .ok_or_else(|| format!("unknown navigation view: {identity:?}"))?;
    select_navigation_item(hwnd, tree, item)
}

/// Selects a tree item like a click does, and activates its view. Activation
/// runs here too because a selection that is already the caret sends no
/// `TVN_SELCHANGEDW`, yet a click on it still brings its window forward.
unsafe fn select_navigation_item(hwnd: HWND, tree: HWND, item: isize) -> Result<(), String> {
    let selected = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CARET as WPARAM, 0) as isize;
    if selected != item {
        SendMessageW(tree, TVM_SELECTITEM, TVGN_CARET as WPARAM, item as LPARAM);
    }
    let view_id = state_ref(hwnd).tree_items.get(&item).cloned();
    if let Some(view_id) = view_id {
        activate_view(hwnd, &view_id, Some(item))?;
    }

    Ok(())
}

unsafe fn tree_item_at_screen_point(tree: HWND, x: i32, y: i32) -> Option<isize> {
    let mut hit = TVHITTESTINFO {
        pt: POINT { x, y },
        flags: 0,
        hItem: 0,
    };
    (ScreenToClient(tree, &mut hit.pt) != 0)
        .then(|| {
            SendMessageW(
                tree,
                TVM_HITTEST,
                0,
                (&mut hit as *mut TVHITTESTINFO) as LPARAM,
            ) as isize
        })
        .filter(|item| *item != 0)
}

unsafe fn selected_tree_item(tree: HWND) -> Option<isize> {
    let item = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CARET as WPARAM, 0) as isize;
    (item != 0).then_some(item)
}

unsafe fn tree_item_context_screen_point(tree: HWND, item: isize) -> Result<(i32, i32), String> {
    let mut rect = RECT::default();
    std::ptr::write_unaligned((&mut rect as *mut RECT).cast::<isize>(), item);
    if SendMessageW(tree, TVM_GETITEMRECT, 1, (&mut rect as *mut RECT) as LPARAM) == 0 {
        return Err("tree item rectangle is unavailable".to_owned());
    }
    let mut point = POINT {
        x: rect.left,
        y: rect.top + (rect.bottom - rect.top) / 2,
    };
    if ClientToScreen(tree, &mut point) == 0 {
        return Err(last_os_error("ClientToScreen"));
    }
    Ok((point.x, point.y))
}

unsafe fn show_navigation_room_context_menu(
    frame: HWND,
    lparam: LPARAM,
    requested_command: Option<usize>,
) -> Result<(), String> {
    let tree = state_ref(frame).controls.tree;
    let (item, point) = match context_menu_screen_point_for_lparam(lparam) {
        Some((x, y)) => {
            let item = tree_item_at_screen_point(tree, x, y)
                .ok_or_else(|| "room context menu target is unavailable".to_owned())?;
            SendMessageW(tree, TVM_SELECTITEM, TVGN_CARET as WPARAM, item as LPARAM);
            (item, (x, y))
        }
        None => {
            let item = selected_tree_item(tree)
                .ok_or_else(|| "room context menu target is unavailable".to_owned())?;
            (item, tree_item_context_screen_point(tree, item)?)
        }
    };
    let Some(view_id) = state_ref(frame).tree_items.get(&item).cloned() else {
        return Ok(());
    };
    if !matches!(view_id, ViewId::Room(_)) {
        return Ok(());
    }
    activate_view(frame, &view_id, Some(item))?;
    let menu = state_ref(frame).controls.room_menu;
    refresh_room_menu_state(frame, menu)?;
    let action_state = active_room_action_state(state_ref(frame));
    if requested_command.is_some_and(|command| {
        (command == MENU_ID_ROOM_ACCEPT_INVITE && !action_state.accept_invite_enabled)
            || (command == MENU_ID_ROOM_REJECT_INVITE && !action_state.reject_invite_enabled)
            || (command == MENU_ID_ROOM_PART && !action_state.part_room_enabled)
            || (notification_menu_mode(command).is_some()
                && action_state.notification_mode.is_none())
    }) {
        return Err("room context menu action is disabled".to_owned());
    }
    if let Some(command) = show_context_menu(frame, menu, point.0, point.1, requested_command)?
        && let Some(result) = handle_menu_command(frame, command)
    {
        result?;
    }
    Ok(())
}

/// The Rooms List as its list control shows it: the header's titles and
/// every row's text as the list asks the listing for it, through
/// `LVN_GETDISPINFOW`.
unsafe fn dump_room_list(list: HWND) -> UiRoomList {
    let header = SendMessageW(list, LVM_GETHEADER, 0, 0) as HWND;
    assert!(!header.is_null(), "a report list has a header");
    let column_count = SendMessageW(header, HDM_GETITEMCOUNT, 0, 0);
    let column_count = usize::try_from(column_count).expect("HDM_GETITEMCOUNT succeeds");
    let mut buffer = [0u16; 512];
    let mut columns = Vec::new();
    let mut column_click_points = Vec::new();
    for column in 0..column_count {
        let mut lv_column = LVCOLUMNW {
            mask: LVCF_TEXT,
            pszText: buffer.as_mut_ptr(),
            cchTextMax: buffer.len() as i32,
            ..LVCOLUMNW::default()
        };
        assert_ne!(
            SendMessageW(
                list,
                LVM_GETCOLUMNW,
                column,
                &mut lv_column as *mut _ as LPARAM
            ),
            0,
            "LVM_GETCOLUMNW must read column {column}"
        );
        columns.push(wide_buffer_text(&buffer));
        let mut rect = RECT::default();
        assert_ne!(
            SendMessageW(
                header,
                HDM_GETITEMRECT,
                column,
                &mut rect as *mut _ as LPARAM
            ),
            0,
            "HDM_GETITEMRECT must place column {column}"
        );
        column_click_points.push(rect_center(&rect));
    }

    let row_count = usize::try_from(SendMessageW(list, LVM_GETITEMCOUNT, 0, 0))
        .expect("LVM_GETITEMCOUNT succeeds");
    let rows = (0..row_count)
        .map(|row| {
            (0..column_count)
                .map(|column| {
                    let mut item = LVITEMW {
                        iSubItem: column as i32,
                        pszText: buffer.as_mut_ptr(),
                        cchTextMax: buffer.len() as i32,
                        ..LVITEMW::default()
                    };
                    SendMessageW(list, LVM_GETITEMTEXTW, row, &mut item as *mut _ as LPARAM);
                    wide_buffer_text(&buffer)
                })
                .collect()
        })
        .collect();

    let top_row = usize::try_from(SendMessageW(list, LVM_GETTOPINDEX, 0, 0))
        .expect("LVM_GETTOPINDEX succeeds");
    let per_page = usize::try_from(SendMessageW(list, LVM_GETCOUNTPERPAGE, 0, 0))
        .expect("LVM_GETCOUNTPERPAGE succeeds");
    let visible_row_click_points = (top_row..row_count.min(top_row + per_page))
        .map(|row| {
            let mut rect = RECT {
                left: LVIR_LABEL as i32,
                ..RECT::default()
            };
            assert_ne!(
                SendMessageW(list, LVM_GETITEMRECT, row, &mut rect as *mut _ as LPARAM),
                0,
                "LVM_GETITEMRECT must place row {row}"
            );
            rect_center(&rect)
        })
        .collect();
    let selected = SendMessageW(list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as LPARAM);

    UiRoomList {
        columns,
        rows,
        selected: usize::try_from(selected).ok(),
        column_click_points,
        visible_row_click_points,
        top_row,
    }
}

/// The text before the NUL a control wrote into `buffer`.
fn wide_buffer_text(buffer: &[u16]) -> String {
    let len = buffer
        .iter()
        .position(|&unit| unit == 0)
        .expect("the control NUL-terminates its text");
    String::from_utf16_lossy(&buffer[..len])
}

fn rect_center(rect: &RECT) -> UiPoint {
    UiPoint {
        x: (rect.left + rect.right) / 2,
        y: (rect.top + rect.bottom) / 2,
    }
}

unsafe fn drain_core_events(hwnd: HWND) -> Vec<Event> {
    let queue = &state_ref(hwnd).event_queue;
    let mut queue = queue.lock().expect("core event queue poisoned");
    queue.drain(..).collect()
}

unsafe fn apply_events_and_refresh(hwnd: HWND, events: Vec<Event>) -> Result<(), String> {
    if events.is_empty() {
        return Ok(());
    }

    let mut history_backfill_anchor = None;
    let mut history_backfill_views = Vec::new();
    let mut backfill_result = false;
    let mut failed_backfill_room_id = None;
    let mut wants_attention = false;
    for event in events {
        if let Event::E2eeRecoverySetupSucceeded(result) = &event {
            show_e2ee_recovery_key(hwnd, &result.recovery_key)?;
        }
        let state = state_mut(hwnd);
        if history_backfill_anchor.is_none()
            && let Event::RoomHistoryBackfill(backfill) = &event
        {
            history_backfill_anchor =
                capture_room_history_backfill_anchor(state, &backfill.room_id);
        }
        let backfilled_room_id = match &event {
            Event::RoomHistoryBackfill(backfill) => Some(backfill.room_id.clone()),
            Event::Error(error) if error.operation == Operation::BackfillRoomHistory => {
                let room_id = error
                    .room_id
                    .clone()
                    .expect("a backfill error names its room");
                failed_backfill_room_id = Some(room_id.clone());
                Some(room_id)
            }
            _ => None,
        };
        if let Some(backfilled_room_id) = &backfilled_room_id {
            history_backfill_views.extend(
                pending_timeline_find_history_views(state)
                    .into_iter()
                    .filter(|view_id| match view_id {
                        ViewId::Room(room_id) | ViewId::Thread { room_id, .. } => {
                            room_id == backfilled_room_id
                        }
                        ViewId::Status | ViewId::RoomList => false,
                    }),
            );
        }
        backfill_result |= backfilled_room_id.is_some();
        wants_attention |= apply_core_event(state, event)?;
        state.event_generation += 1;
    }

    sync_views_and_tree(hwnd)?;
    if wants_attention {
        request_frame_attention(hwnd);
    }
    if let Some(anchor) = history_backfill_anchor {
        restore_room_history_backfill_anchor(hwnd, anchor);
    }
    history_backfill_views.sort();
    history_backfill_views.dedup();
    for view_id in history_backfill_views {
        continue_timeline_find_history_search_after_refresh(hwnd, &view_id)?;
    }
    // After the anchor, so the jump's scroll wins.
    if backfill_result {
        continue_original_jump(hwnd, failed_backfill_room_id.as_deref())?;
    }
    Ok(())
}

/// Status bar text while Go to Original pages older history.
const FINDING_ORIGINAL_STATUS: &str = "finding the original message...";

/// The message menu's Go to Original Message, and a double-click on a
/// quote line (which selects its reply): scrolls to the message the
/// selected reply quotes, loading older history until it is found.
unsafe fn go_to_original_of_selected_message(hwnd: HWND) -> Result<(), String> {
    let state = state_ref(hwnd);
    let target_event_id = selected_message_quote_target(state, selected_timeline_event_id(state))
        .ok_or_else(|| "the selected message is not a reply".to_owned())?
        .to_owned();
    let view_id = state.active_view.clone();
    // A new jump replaces a pending one.
    state_mut(hwnd).pending_original_jump = Some(replies::OriginalJump {
        view_id,
        target_event_id,
    });
    continue_original_jump(hwnd, None)
}

/// Takes the pending Go to Original's next step, when it starts and after
/// every history result. `failed_backfill_room_id`: the room whose request
/// for older messages just failed.
unsafe fn continue_original_jump(
    hwnd: HWND,
    failed_backfill_room_id: Option<&str>,
) -> Result<(), String> {
    let Some(jump) = state_ref(hwnd).pending_original_jump.clone() else {
        return Ok(());
    };
    let room_id = jump
        .view_id
        .room_id()
        .expect("only room and thread windows quote")
        .to_owned();
    let step = {
        let state = state_ref(hwnd);
        let view = state
            .views
            .get(&jump.view_id)
            .expect("closing a window drops its jump");
        let in_view_rows = timeline_source_row_index_of_event(
            view_timeline(&jump.view_id, view.content),
            &jump.target_event_id,
        )
        .is_some();
        let room_view = state
            .model
            .room_views
            .get(&room_id)
            .expect("a reply's room has its timeline");
        let history = match &room_view.history.status {
            RoomHistoryBackfillStatus::Idle => replies::JumpHistory::Idle,
            RoomHistoryBackfillStatus::Loading { .. } => replies::JumpHistory::Loading,
            RoomHistoryBackfillStatus::Exhausted => replies::JumpHistory::Exhausted,
            RoomHistoryBackfillStatus::Failed { message, .. } => replies::JumpHistory::Failed {
                message: message.clone(),
                just_now: failed_backfill_room_id == Some(room_id.as_str()),
            },
        };
        let another_backfill_loading = loading_room_history_room_id(&state.model.room_views)
            .is_some_and(|loading_room_id| loading_room_id != room_id);
        replies::original_jump_step(
            &jump.view_id,
            in_view_rows,
            room_view
                .find_activity_index_by_event_id(&jump.target_event_id)
                .is_some(),
            history,
            another_backfill_loading,
        )
    };
    let status = match step {
        replies::OriginalJumpStep::Show(view_id) => {
            state_mut(hwnd).pending_original_jump = None;
            show_original_message(hwnd, &view_id, &jump.target_event_id)?;
            "original message found".to_owned()
        }
        replies::OriginalJumpStep::LoadOlder => {
            let submitted = submit_backfill_room_history_for_room(state_mut(hwnd), &room_id)?;
            assert!(
                submitted,
                "a joined room with history left and none loading takes a request"
            );
            FINDING_ORIGINAL_STATUS.to_owned()
        }
        replies::OriginalJumpStep::Wait => FINDING_ORIGINAL_STATUS.to_owned(),
        replies::OriginalJumpStep::NotFound => {
            state_mut(hwnd).pending_original_jump = None;
            "* the original message is not in this room's history".to_owned()
        }
        replies::OriginalJumpStep::Failed(message) => {
            state_mut(hwnd).pending_original_jump = None;
            format!("* could not load older messages: {message}")
        }
    };
    set_local_command_status(hwnd, status)
}

/// Scrolls `view_id`'s window, activated first, so the message's first line
/// is at the top (as low as the end allows), and selects it.
unsafe fn show_original_message(
    hwnd: HWND,
    view_id: &ViewId,
    event_id: &str,
) -> Result<(), String> {
    if state_ref(hwnd).active_view != *view_id {
        activate_view(hwnd, view_id, None)?;
    }
    let timeline = view_timeline(view_id, state_ref(hwnd).views[view_id].content);
    let source_row_index = timeline_source_row_index_of_event(timeline, event_id)
        .expect("the jump shows a message its window holds");
    scroll_timeline_to_source_row_index(timeline, source_row_index);
    state_mut(hwnd).selected_timeline_event = Some(SelectedTimelineEvent {
        view_id: view_id.clone(),
        event_id: event_id.to_owned(),
    });
    InvalidateRect(timeline, null(), 1);
    Ok(())
}

fn cancel_original_jump_for_view(state: &mut WindowState, view_id: &ViewId) {
    if state
        .pending_original_jump
        .as_ref()
        .is_some_and(|jump| jump.view_id == *view_id)
    {
        state.pending_original_jump = None;
    }
}

unsafe fn cancel_original_jump_for_timeline(timeline: HWND) {
    let frame = frame_window_for(timeline);
    let state = state_mut(frame);
    let Some(jump) = &state.pending_original_jump else {
        return;
    };
    if view_timeline(&jump.view_id, state.views[&jump.view_id].content) == timeline {
        state.pending_original_jump = None;
    }
}

/// Flashes the taskbar button until the user brings mIRC to the foreground;
/// Windows stops the flash on activation.
unsafe fn request_frame_attention(hwnd: HWND) {
    if GetForegroundWindow() == hwnd {
        return;
    }
    let info = FLASHWINFO {
        cbSize: size_of::<FLASHWINFO>() as u32,
        hwnd,
        dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
        uCount: 0,
        dwTimeout: 0,
    };
    // The return value is the window's previous active state, not an error.
    FlashWindowEx(&info);
}

unsafe fn apply_synthetic_events_and_refresh(hwnd: HWND, events: Vec<Event>) -> Result<(), String> {
    state_mut(hwnd).synthetic_load_in_progress = true;
    let result = apply_events_and_refresh(hwnd, events);
    state_mut(hwnd).synthetic_load_in_progress = false;
    result
}

unsafe fn apply_event_load(hwnd: HWND, events: Vec<Event>) -> Result<(usize, usize), String> {
    {
        let state = state_mut(hwnd);
        state.current_session_homeserver_url = None;
        state.room_activity.clear();
        state.closed_queries.clear();
        state.room_activity_read_targets.clear();
        state.find_states.clear();
        state.selected_timeline_event = None;
        state.model = initial_model();
        state.decoded_images.clear();
    }

    apply_synthetic_events_and_refresh(hwnd, events)?;

    let (rooms, timeline_events, active_view) = {
        let state = state_mut(hwnd);
        let rooms = state.model.rooms.len();
        let timeline_events = state
            .model
            .room_views
            .values()
            .map(|room_view| room_view.activities.len())
            .sum();
        let selected_room = state
            .model
            .rooms
            .iter()
            .find(|room| is_navigable_room(room))
            .map(|room| room.room_id.clone());

        let active_view = match selected_room {
            Some(room_id) => {
                state.model.selected_room = Some(room_id.clone());
                ViewId::Room(room_id)
            }
            None => {
                state.model.selected_room = None;
                ViewId::Status
            }
        };
        state.active_view = active_view.clone();

        (rooms, timeline_events, active_view)
    };

    sync_views_and_tree(hwnd)?;
    activate_view(hwnd, &active_view, None)?;

    Ok((rooms, timeline_events))
}

unsafe fn drain_core_events_and_refresh(hwnd: HWND) -> Result<usize, String> {
    let events = drain_core_events(hwnd);
    let event_count = events.len();
    apply_events_and_refresh(hwnd, events).map(|()| event_count)
}

unsafe fn handle_tree_selection(hwnd: HWND) {
    let tree = state_ref(hwnd).controls.tree;
    let selected_item = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CARET as WPARAM, 0) as isize;
    if selected_item == 0 {
        return;
    }

    let view_id = state_ref(hwnd).tree_items.get(&selected_item).cloned();
    if let Some(view_id) = view_id
        && let Err(message) = activate_view(hwnd, &view_id, Some(selected_item))
    {
        show_error_message(hwnd, "Treebar", &message);
    }
}

fn select_room(state: &mut WindowState, room_id: &str) -> Result<(), String> {
    if navigable_room_by_id(&state.model.rooms, room_id).is_none() {
        return Err(format!("unknown room id: {room_id}"));
    }

    state.model.selected_room = Some(room_id.to_owned());
    Ok(())
}

unsafe fn submit_compose_from_edit(hwnd: HWND) -> Result<(), String> {
    let edit = compose_edit(hwnd);
    // Commands may activate another window; the line still belongs to the
    // history of the editbox it was typed in.
    let view = state_ref(hwnd).active_view.clone();
    let input = read_window_text(edit);
    if input.is_empty() {
        return Err("message body must not be empty".to_owned());
    }

    let record = |entry: String| {
        state_mut(hwnd)
            .views
            .get_mut(&view)
            .expect("the submitting editbox's window must exist")
            .compose_mut()
            .history
            .record_submission(entry);
    };
    let empty = wide_null("");
    match parse_compose_submission(&input) {
        ComposeSubmission::Message { presentation, body } => {
            let target = state_ref(hwnd).views[&view].compose().target.clone();
            match &target {
                None => {
                    submit_compose(state_mut(hwnd), presentation, &body, None)?;
                }
                Some(ComposeTarget::Reply { event_id }) => {
                    send_reply(state_mut(hwnd), event_id, presentation, &body)?;
                }
                Some(ComposeTarget::Edit { event_id }) => {
                    send_edit(state_mut(hwnd), event_id, presentation, &body)?;
                }
            }
            record(input.clone());
            SendMessageW(edit, WM_SETTEXT, 0, empty.as_ptr() as LPARAM);
            if target.is_some() {
                state_mut(hwnd)
                    .views
                    .get_mut(&view)
                    .expect("the submitting editbox's window must exist")
                    .compose_mut()
                    .target = None;
                refresh_view_content(hwnd, &view)?;
            }
        }
        ComposeSubmission::LocalCommand(parsed) => {
            if let Some(entry) = local_command_history_entry(&input, &parsed) {
                record(entry);
            }
            // Cleared first: a command may fill the editbox, as /edit does.
            SendMessageW(edit, WM_SETTEXT, 0, empty.as_ptr() as LPARAM);
            match parsed {
                Ok(command) => run_local_command(hwnd, command)?,
                Err(error) => report_command_error(hwnd, error)?,
            }
        }
    }
    Ok(())
}

/// The history of the active window's editbox, which keys reach.
fn active_compose_history(state: &mut WindowState) -> &mut ComposeHistory {
    &mut state
        .views
        .get_mut(&state.active_view)
        .expect("the active view must have a window")
        .compose_mut()
        .history
}

/// The active window's editbox, which compose input and commands act on.
unsafe fn compose_edit(hwnd: HWND) -> HWND {
    let state = state_ref(hwnd);
    state
        .views
        .get(&state.active_view)
        .expect("the active view must have a window")
        .compose()
        .edit
}

unsafe fn dump_compose_text(hwnd: HWND) -> Result<String, String> {
    let edit = compose_edit(hwnd);
    Ok(read_window_text(edit))
}

unsafe fn set_compose_text(hwnd: HWND, text: &str) -> Result<(), String> {
    let edit = compose_edit(hwnd);
    active_compose_history(state_mut(hwnd)).reset_navigation();
    set_compose_edit_text(edit, text);
    Ok(())
}

unsafe fn send_compose_key(hwnd: HWND, key: ComposeKey) -> Result<(), String> {
    let edit = compose_edit(hwnd);
    let virtual_key = match key {
        ComposeKey::Return => VK_RETURN_KEY,
        ComposeKey::Up => VK_UP_KEY,
        ComposeKey::Down => VK_DOWN_KEY,
        ComposeKey::Tab => VK_TAB_KEY,
        ComposeKey::Escape => VK_ESCAPE_KEY,
        // A sent key carries no Shift state, so Shift+Tab goes straight to
        // the completion the edit box runs for it.
        ComposeKey::ShiftTab => return complete_compose_nick(hwnd, true),
        // Nor Ctrl state, for the format chords.
        ComposeKey::Format { key } => {
            insert_mirc_format_code(hwnd, edit, mirc_format::mirc_format_code(key));
            return Ok(());
        }
    };
    SendMessageW(edit, WM_KEYDOWN, virtual_key, 0);
    Ok(())
}

/// Types a formatting code at the editbox's caret, as one undoable edit.
unsafe fn insert_mirc_format_code(frame: HWND, edit: HWND, code: char) {
    active_compose_history(state_mut(frame)).reset_navigation();
    let text = wide_null(code.encode_utf8(&mut [0; 4]));
    // wParam TRUE: the replacement can be undone.
    SendMessageW(edit, EM_REPLACESEL, 1, text.as_ptr() as LPARAM);
}

unsafe fn submit_compose_from_control(
    hwnd: HWND,
    explicit_room_id: Option<&str>,
) -> Result<String, String> {
    let edit = compose_edit(hwnd);
    let input = read_window_text(edit);
    let room_id = submit_compose(
        state_mut(hwnd),
        RoomMessagePresentationKind::Text,
        &input,
        explicit_room_id,
    )?;
    let empty = wide_null("");
    SendMessageW(edit, WM_SETTEXT, 0, empty.as_ptr() as LPARAM);
    Ok(room_id)
}

/// The message an editbox line sends: its text without mIRC codes, in the
/// styles they set. Codes alone send nothing.
fn message_text_from_input(input: &str) -> Result<(String, Vec<TextStyleSpan>), String> {
    let (body, styles) = mirc_format::parse_mirc_input(input);
    if body.is_empty() {
        return Err("message body must not be empty".to_owned());
    }
    Ok((body, styles))
}

fn submit_compose(
    state: &mut WindowState,
    presentation: RoomMessagePresentationKind,
    input: &str,
    explicit_room_id: Option<&str>,
) -> Result<String, String> {
    let (body, styles) = message_text_from_input(input)?;
    let destination = resolve_compose_destination(state, explicit_room_id)?;

    let clears_active_thread_marker = explicit_room_id.is_none() && destination.thread.is_some();
    enqueue_send_message(
        state,
        SendMessageRequest {
            room_id: destination.room_id.clone(),
            presentation,
            body,
            styles,
            thread: destination.thread,
            reply: None,
        },
    )?;
    if clears_active_thread_marker {
        clear_view_activity_marker(state, &state.active_view.clone());
    }

    Ok(destination.room_id)
}

/// Sends `input` to a joined room's main timeline, from a command naming
/// the room; returns the text sent.
fn send_room_message(
    state: &mut WindowState,
    room_id: String,
    presentation: RoomMessagePresentationKind,
    input: &str,
) -> Result<String, String> {
    if sendable_room_by_id(&state.model.rooms, &room_id).is_none() {
        return Err(format!("room is not joined: {room_id}"));
    }
    let (body, styles) = message_text_from_input(input)?;
    enqueue_send_message(
        state,
        SendMessageRequest {
            room_id,
            presentation,
            body: body.clone(),
            styles,
            thread: None,
            reply: None,
        },
    )?;
    Ok(body)
}

fn enqueue_send_message(
    state: &mut WindowState,
    request: SendMessageRequest,
) -> Result<(), String> {
    let command = Command::SendMessage(request);
    state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue send message command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComposeDestination {
    room_id: String,
    thread: Option<SendMessageThreadTarget>,
}

/// The room `/sendfile` sends to: the active window's.
fn send_file_room_id(state: &WindowState) -> Result<String, String> {
    match &state.active_view {
        ViewId::Room(room_id) => Ok(room_id.clone()),
        ViewId::Thread { .. } => Err("file attachments cannot target threads yet".to_owned()),
        ViewId::Status | ViewId::RoomList => Err("no room selected".to_owned()),
    }
}

/// The joined room a file goes to; an invite or a room left meanwhile
/// takes none.
fn sendable_file_room<'a>(
    state: &'a WindowState,
    room_id: &str,
) -> Result<&'a RoomSummary, String> {
    sendable_room_by_id(&state.model.rooms, room_id)
        .ok_or_else(|| format!("room is not joined: {room_id}"))
}

/// `/sendfile <path>` and the Send File dialog's OK: uploads the file to
/// the room, with the core choosing its type from the name.
fn submit_send_file(
    state: &mut WindowState,
    room_id: String,
    path: PathBuf,
) -> Result<String, String> {
    sendable_file_room(state, &room_id)?;
    enqueue_command(
        state,
        Command::SendFile(SendFileRequest {
            room_id: room_id.clone(),
            path,
            filename: None,
            content_type: None,
        }),
    )?;
    Ok(format!("File queued for {room_id}"))
}

/// The Send File dialog for `room_id`, which the caller captured before
/// the dialog opens, so activating another window meanwhile does not
/// redirect the file. `None` when cancelled.
unsafe fn run_send_file_dialog(frame: HWND, room_id: String) -> Result<Option<String>, String> {
    let room_name = sendable_file_room(state_ref(frame), &room_id)?
        .display_name
        .clone();
    // Modal: core events keep arriving, so nothing is held across it.
    match send_file::choose_file_to_send(frame, &room_name)? {
        Some(path) => submit_send_file(state_mut(frame), room_id, path).map(Some),
        None => Ok(None),
    }
}

fn resolve_compose_destination(
    state: &WindowState,
    explicit_room_id: Option<&str>,
) -> Result<ComposeDestination, String> {
    if let Some(room_id) = explicit_room_id {
        if sendable_room_by_id(&state.model.rooms, room_id).is_some() {
            return Ok(ComposeDestination {
                room_id: room_id.to_owned(),
                thread: None,
            });
        }
        if let Some(room) = room_by_id(&state.model.rooms, room_id)
            && is_invited_room(room)
        {
            return Err(format!("room is not joined: {room_id}"));
        }
        return Err(format!("unknown room id: {room_id}"));
    }

    match &state.active_view {
        ViewId::Room(room_id) => {
            if sendable_room_by_id(&state.model.rooms, room_id).is_some() {
                Ok(ComposeDestination {
                    room_id: room_id.clone(),
                    thread: None,
                })
            } else {
                Err(format!("room is not joined: {room_id}"))
            }
        }
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => {
            projected_thread_by_id(&state.model, room_id, thread_root_event_id)
                .expect("active thread compose must target an existing projected thread");
            Ok(ComposeDestination {
                room_id: room_id.clone(),
                thread: Some(SendMessageThreadTarget {
                    thread_root_event_id: thread_root_event_id.clone(),
                    latest_event_id: latest_durable_thread_event_id(
                        &state.model,
                        room_id,
                        thread_root_event_id,
                    ),
                }),
            })
        }
        ViewId::Status | ViewId::RoomList => Err("no room selected".to_owned()),
    }
}

fn latest_durable_thread_event_id(
    model: &UiModel,
    room_id: &str,
    thread_root_event_id: &str,
) -> String {
    let room_view = model
        .room_views
        .get(room_id)
        .expect("thread compose must target an existing room view");
    let mut latest_event_id = thread_root_event_id;

    for activity in &room_view.activities {
        if room_activity_belongs_to_thread(activity, thread_root_event_id)
            && let Some(event_id) = room_activity_event_id(activity)
            && !event_id.starts_with("txn:")
        {
            latest_event_id = event_id;
        }
    }

    latest_event_id.to_owned()
}

/// Runs a command and reports how it went: its status, or its error in the
/// active window.
unsafe fn run_local_command(hwnd: HWND, command: SlashCommand) -> Result<(), String> {
    match execute_slash_command(hwnd, command) {
        Ok(Some(status)) => set_local_command_status(hwnd, status),
        Ok(None) => Ok(()),
        Err(error) => report_command_error(hwnd, error),
    }
}

/// Runs a typed command. `Ok(Some(status))` is feedback for the status bar;
/// `Ok(None)` means the command reported (or needs) nothing.
unsafe fn execute_slash_command(
    hwnd: HWND,
    command: SlashCommand,
) -> Result<Option<String>, CommandError> {
    let name = command.name();
    run_slash_command(hwnd, command).map_err(|reason| CommandError::new(name, reason))
}

unsafe fn run_slash_command(hwnd: HWND, command: SlashCommand) -> Result<Option<String>, String> {
    match command {
        SlashCommand::Status => {
            activate_view(hwnd, &ViewId::Status, None)?;
            Ok(Some("status window active".to_owned()))
        }
        SlashCommand::Find { query } => {
            execute_active_view_find(hwnd, &query)?;
            Ok(None)
        }
        SlashCommand::Window { target } => {
            let room_id =
                match resolve_room(&state_ref(hwnd).model.rooms, is_navigable_room, &target) {
                    Ok(room) => room.room_id.clone(),
                    Err(RoomTargetError::Unknown) => {
                        return Err(format!("unknown window target: {target}"));
                    }
                    Err(RoomTargetError::Ambiguous) => {
                        return Err(format!("ambiguous window target: {target}"));
                    }
                };
            activate_view(hwnd, &ViewId::Room(room_id), None)?;
            Ok(Some(format!("window active: {target}")))
        }
        SlashCommand::Join { target } => {
            submit_join_room(state_mut(hwnd), target, Vec::new()).map(Some)
        }
        SlashCommand::List { server, filter } => {
            start_room_list(hwnd, RoomListQuery { server, filter }).map(Some)
        }
        SlashCommand::Create(args) => submit_create_room(state_mut(hwnd), args).map(Some),
        SlashCommand::Query { target, message } => {
            let user_id =
                commands::resolve_user(all_room_members(&state_ref(hwnd).model), &target)?.user_id;
            match resolve_direct_room_query_target(&state_ref(hwnd).model, &user_id) {
                QueryTargetResolution::Single(room_id) => {
                    activate_view(hwnd, &ViewId::Room(room_id.clone()), None)?;
                    if let Some(input) = message {
                        send_room_message(
                            state_mut(hwnd),
                            room_id,
                            RoomMessagePresentationKind::Text,
                            &input,
                        )?;
                    }
                    Ok(Some(format!("query opened: {user_id}")))
                }
                QueryTargetResolution::Ambiguous => {
                    Err(format!("ambiguous query target: {user_id}"))
                }
                QueryTargetResolution::None => {
                    let status = submit_query_slash_command(state_mut(hwnd), user_id.clone())?;
                    if message.is_some() {
                        echo_in_active_view(
                            hwnd,
                            [format!(
                                "* /query: creating a query with {user_id}; message not sent"
                            )],
                        )?;
                    }
                    Ok(Some(status))
                }
            }
        }
        SlashCommand::Msg {
            target,
            body,
            presentation,
        } => {
            let room_id = msg_target_room_id(&state_ref(hwnd).model, &target)?;
            let sent = send_room_message(state_mut(hwnd), room_id, presentation, &body)?;
            echo_in_active_view(hwnd, [commands::msg_echo(presentation, &target, &sent)])?;
            Ok(None)
        }
        SlashCommand::Names { target } => {
            let lines = {
                let state = state_ref(hwnd);
                let room_id = resolve_joined_room_target(state, target.as_deref())?;
                let room = room_by_id(&state.model.rooms, &room_id)
                    .expect("a joined room target must exist");
                commands::names_lines(&room.display_name, room_members(&state.model, &room_id))
            };
            echo_in_active_view(hwnd, lines)?;
            Ok(None)
        }
        SlashCommand::Whois { nick } => {
            let state = state_mut(hwnd);
            let user_id = commands::resolve_user(all_room_members(&state.model), &nick)?.user_id;
            enqueue_command(state, Command::FetchUserProfile(user_id.clone()))?;
            Ok(Some(format!("whois requested: {user_id}")))
        }
        SlashCommand::Ignore { target: None } => {
            let lines = {
                let model = &state_ref(hwnd).model;
                ignore::list_lines(model.ignored_users.as_deref(), |user_id| {
                    known_nick(model, user_id)
                })?
            };
            echo_in_active_view(hwnd, lines)?;
            Ok(None)
        }
        SlashCommand::Ignore {
            target: Some(target),
        } => {
            let state = state_mut(hwnd);
            let user_id = ignore_target(&state.model, &target)?;
            ignore::check_ignore(
                state.model.ignored_users.as_deref(),
                state.model.own_user_id.as_deref(),
                &user_id,
                |user_id| known_nick(&state.model, user_id),
            )?;
            enqueue_command(state, Command::IgnoreUser(user_id.clone()))?;
            Ok(Some(format!("ignore submitted: {user_id}")))
        }
        SlashCommand::Unignore { target } => {
            let state = state_mut(hwnd);
            let user_id = ignore_target(&state.model, &target)?;
            ignore::check_unignore(state.model.ignored_users.as_deref(), &user_id, |user_id| {
                known_nick(&state.model, user_id)
            })?;
            enqueue_command(state, Command::UnignoreUser(user_id.clone()))?;
            Ok(Some(format!("unignore submitted: {user_id}")))
        }
        SlashCommand::Close => {
            let state = state_ref(hwnd);
            if close_action(&state.model, &state.active_view) == CloseAction::Refuse {
                return Err("the Status window cannot be closed".to_owned());
            }
            // Posted: closing may ask first, and destroys this editbox.
            let child = state.views[&state.active_view].child;
            if PostMessageW(child, WM_CLOSE, 0, 0) == 0 {
                return Err(last_os_error("PostMessageW WM_CLOSE"));
            }
            Ok(None)
        }
        SlashCommand::Kick { nick, reason } => {
            let state = state_mut(hwnd);
            let (room_id, member) = room_member_target(state, &nick)?;
            if !member.can_kick {
                return Err(format!("you cannot kick {nick}"));
            }
            let user_id = member.user_id.clone();
            enqueue_command(
                state,
                Command::KickRoomMember(KickRoomMemberRequest {
                    room_id,
                    user_id: user_id.clone(),
                    reason,
                }),
            )?;
            Ok(Some(format!("kick submitted: {user_id}")))
        }
        SlashCommand::Ban { nick, reason } => {
            let state = state_mut(hwnd);
            let (room_id, member) = room_member_target(state, &nick)?;
            if !member.can_ban {
                return Err(format!("you cannot ban {nick}"));
            }
            let user_id = member.user_id.clone();
            enqueue_command(
                state,
                Command::BanRoomMember(BanRoomMemberRequest {
                    room_id,
                    user_id: user_id.clone(),
                    reason,
                }),
            )?;
            Ok(Some(format!("ban submitted: {user_id}")))
        }
        SlashCommand::Unban { target, reason } => {
            let state = state_mut(hwnd);
            let room_id = resolve_joined_room_target(state, None)?;
            let user_id = commands::resolve_user(all_room_members(&state.model), &target)?.user_id;
            enqueue_command(
                state,
                Command::UnbanRoomMember(UnbanRoomMemberRequest {
                    room_id,
                    user_id: user_id.clone(),
                    reason,
                }),
            )?;
            Ok(Some(format!("unban submitted: {user_id}")))
        }
        SlashCommand::Invite { nick, room } => {
            let state = state_mut(hwnd);
            let room_id = resolve_joined_room_target(state, room.as_deref())?;
            let user_id = commands::resolve_user(all_room_members(&state.model), &nick)?.user_id;
            if room_members(&state.model, &room_id)
                .iter()
                .any(|member| member.user_id == user_id)
            {
                return Err(format!("{nick} is already in this room"));
            }
            enqueue_command(
                state,
                Command::InviteRoomMember(InviteRoomMemberRequest {
                    room_id,
                    user_id: user_id.clone(),
                }),
            )?;
            Ok(Some(format!("invite submitted: {user_id}")))
        }
        SlashCommand::SetRank { nick, change } => {
            let state = state_mut(hwnd);
            let (room_id, member) = room_member_target(state, &nick)?;
            let role = commands::rank_change_role(change, member)?;
            let user_id = member.user_id.clone();
            enqueue_command(
                state,
                Command::SetRoomMemberPowerLevel(SetRoomMemberPowerLevelRequest {
                    room_id,
                    user_id: user_id.clone(),
                    role,
                }),
            )?;
            Ok(Some(format!("power level change submitted: {user_id}")))
        }
        SlashCommand::Nick { display_name } => {
            enqueue_command(
                state_mut(hwnd),
                Command::SetDisplayName(SetDisplayNameRequest {
                    display_name: display_name.clone(),
                }),
            )?;
            Ok(Some(format!("nick change submitted: {display_name}")))
        }
        SlashCommand::Server { server } => {
            connect(hwnd, server.as_deref())?;
            Ok(None)
        }
        SlashCommand::Quit => {
            disconnect(hwnd)?;
            Ok(None)
        }
        SlashCommand::Exit => {
            if PostMessageW(hwnd, WM_CLOSE, 0, 0) == 0 {
                return Err(last_os_error("PostMessageW WM_CLOSE"));
            }
            Ok(None)
        }
        SlashCommand::Accept { target } => {
            submit_accept_slash_command(state_mut(hwnd), target.as_deref()).map(Some)
        }
        SlashCommand::Reject { target } => {
            submit_reject_slash_command(state_mut(hwnd), target.as_deref()).map(Some)
        }
        SlashCommand::Part { target } => {
            submit_part_slash_command(state_mut(hwnd), target.as_deref()).map(Some)
        }
        SlashCommand::React {
            target_event_id,
            key,
        } => submit_react_slash_command(state_mut(hwnd), target_event_id, key).map(Some),
        SlashCommand::Reply {
            target_event_id,
            body: None,
        } => {
            arm_compose_target(
                hwnd,
                ComposeTarget::Reply {
                    event_id: target_event_id,
                },
            )?;
            Ok(None)
        }
        SlashCommand::Reply {
            target_event_id,
            body: Some(body),
        } => {
            send_reply(
                state_mut(hwnd),
                &target_event_id,
                RoomMessagePresentationKind::Text,
                &body,
            )?;
            Ok(Some(format!("Reply queued for {target_event_id}")))
        }
        SlashCommand::SendFile { path } => {
            let room_id = send_file_room_id(state_ref(hwnd))?;
            match path {
                Some(path) => {
                    submit_send_file(state_mut(hwnd), room_id, PathBuf::from(path)).map(Some)
                }
                None => run_send_file_dialog(hwnd, room_id),
            }
        }
        SlashCommand::Edit {
            target_event_id,
            body: None,
        } => {
            arm_compose_target(
                hwnd,
                ComposeTarget::Edit {
                    event_id: target_event_id,
                },
            )?;
            Ok(None)
        }
        SlashCommand::Edit {
            target_event_id,
            body: Some(body),
        } => {
            // Typed after the event ID, the text keeps the message's kind:
            // an emote stays an emote.
            let presentation = {
                let state = state_ref(hwnd);
                let activity = window_message(&state.model, &state.active_view, &target_event_id)
                    .ok_or_else(|| "no such message in this window".to_owned())?;
                replies::editable_message(activity, state.model.own_user_id.as_deref())?
                    .presentation
            };
            send_edit(state_mut(hwnd), &target_event_id, presentation, &body)?;
            Ok(Some(format!("Edit queued for {target_event_id}")))
        }
        SlashCommand::Redact { event_id } => {
            submit_redact_slash_command(state_mut(hwnd), event_id).map(Some)
        }
        SlashCommand::Unreact { reaction_event_id } => {
            submit_unreact_slash_command(state_mut(hwnd), reaction_event_id).map(Some)
        }
        SlashCommand::Topic {
            room,
            topic: Some(topic),
        } => submit_topic_slash_command(state_mut(hwnd), room.as_deref(), topic).map(Some),
        SlashCommand::Topic { room, topic: None } => {
            let line = room_topic_line(state_ref(hwnd), room.as_deref())?;
            echo_in_active_view(hwnd, [line])?;
            Ok(None)
        }
        SlashCommand::Clear => match state_ref(hwnd).active_view.clone() {
            ViewId::Room(room_id) => {
                {
                    let state = state_mut(hwnd);
                    if let Some(view) = state.model.room_views.get_mut(&room_id) {
                        view.clear_visible_entries();
                    }
                    prune_decoded_images(state);
                }
                refresh_view_content(hwnd, &ViewId::Room(room_id.clone()))?;
                Ok(Some(format!("cleared window: {room_id}")))
            }
            ViewId::Thread { .. } => Err("nothing to clear in thread view".to_owned()),
            ViewId::Status => {
                state_mut(hwnd).model.status_log.clear();
                refresh_view_content(hwnd, &ViewId::Status)?;
                Ok(Some("cleared window: status".to_owned()))
            }
            ViewId::RoomList => unreachable!("/clear is typed in an editbox"),
        },
        SlashCommand::E2ee(command) => match command {
            E2eeSlashCommand::ShowStatus => {
                let state = state_mut(hwnd);
                let lines = match &state.model.last_e2ee_status {
                    Some(status) => status_log::e2ee_status(status),
                    None => vec![status_log::E2EE_STATUS_UNKNOWN.to_owned()],
                };
                for line in lines {
                    log_status(state, StatusLineKind::Info, line);
                }
                activate_view(hwnd, &ViewId::Status, None)?;
                Ok(Some("e2ee status shown".to_owned()))
            }
            E2eeSlashCommand::Refresh => {
                let state = state_mut(hwnd);
                let command = Command::RefreshE2eeStatus;
                state
                    .command_sender
                    .as_ref()
                    .ok_or_else(|| "core command sender is not available".to_owned())?
                    .send(command.clone())
                    .map_err(|err| {
                        format!("failed to enqueue refresh e2ee status command: {err}")
                    })?;
                #[cfg(test)]
                state.recorded_commands.push(command);
                Ok(Some("e2ee refresh submitted".to_owned()))
            }
            E2eeSlashCommand::RecoverDialog => {
                show_recover_e2ee_menu_dialog(hwnd)?;
                Ok(None)
            }
            E2eeSlashCommand::Setup {
                passphrase,
                wait_for_backups_to_upload,
            } => {
                submit_setup_e2ee_recovery(
                    state_mut(hwnd),
                    passphrase,
                    wait_for_backups_to_upload,
                )?;
                refresh_status_bar(hwnd)?;
                Ok(None)
            }
        },
        SlashCommand::Help { topic } => {
            echo_in_active_view(hwnd, commands::help_lines(topic.as_deref())?)?;
            Ok(None)
        }
        SlashCommand::Font(FontCommand::Dialog) => run_font_dialog(hwnd),
        SlashCommand::Font(FontCommand::Set(setting)) => {
            apply_font_setting(hwnd, setting).map(Some)
        }
        SlashCommand::Timestamp(command) => match command {
            TimestampCommand::Show => {
                let timestamp = &state_ref(hwnd).settings.settings().timestamp;
                let line = format!(
                    "* Timestamp is {} ({})",
                    on_off(timestamp.show),
                    timestamp.format
                );
                echo_in_active_view(hwnd, [line])?;
                Ok(None)
            }
            TimestampCommand::On | TimestampCommand::Off => {
                let show = command == TimestampCommand::On;
                change_timestamp_setting(hwnd, |timestamp| timestamp.show = show)?;
                Ok(Some(format!("timestamp: {}", on_off(show))))
            }
            TimestampCommand::Format(format) => {
                let status = format!("timestamp format: {format}");
                change_timestamp_setting(hwnd, |timestamp| timestamp.format = format)?;
                Ok(Some(status))
            }
        },
        SlashCommand::Bar { bar, command } => {
            let show = match command {
                BarCommand::Show => {
                    let shown = state_ref(hwnd).settings.settings().bar_shown(bar);
                    echo_in_active_view(hwnd, [bar_is_line(bar, shown)])?;
                    return Ok(None);
                }
                BarCommand::On => true,
                BarCommand::Off => false,
            };
            state_mut(hwnd)
                .settings
                .update(|settings| settings.set_bar_shown(bar, show))?;
            layout_controls(hwnd);
            Ok(Some(format!("{}: {}", bar.name(), on_off(show))))
        }
        SlashCommand::Verify { action } => {
            let state = state_mut(hwnd);
            match action {
                VerificationAction::Accept => submit_accept_verification(state),
                VerificationAction::Cancel => submit_cancel_verification(state),
                VerificationAction::Confirm => submit_confirm_verification_sas(state),
                VerificationAction::Reject => submit_reject_verification_sas(state),
            }?;
            Ok(Some(format!(
                "verification {} submitted",
                verification_action_label(action)
            )))
        }
    }
}

/// Saves a timestamp change and restamps every window's lines, which keep
/// their times rather than stamped text.
unsafe fn change_timestamp_setting(
    hwnd: HWND,
    change: impl FnOnce(&mut timestamp::TimestampSetting),
) -> Result<(), String> {
    state_mut(hwnd)
        .settings
        .update(|settings| change(&mut settings.timestamp))?;
    refresh_every_view_content(hwnd)
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// `* Switchbar is on`, what `/switchbar` alone prints.
fn bar_is_line(bar: Bar, shown: bool) -> String {
    format!("* {} is {}", bar.title(), on_off(shown))
}

/// A typed command's error, printed in the window it was typed in; the
/// status bar says the same.
unsafe fn report_command_error(hwnd: HWND, error: CommandError) -> Result<(), String> {
    let text = error.to_string();
    echo_in_active_view(hwnd, [text.clone()])?;
    set_local_command_status(hwnd, text)
}

/// The window a command's lines print in: the active one. The Rooms List
/// has no text of its own: what a Commands menu item run there prints goes
/// to Status.
fn echo_view(active_view: &ViewId) -> ViewId {
    match active_view {
        ViewId::RoomList => ViewId::Status,
        view => view.clone(),
    }
}

/// Prints a command's lines in the active window. They are its newest lines,
/// in the window the user is looking at, so they raise no activity. The
/// caller refreshes the window.
fn echo_lines_in_active_view(state: &mut WindowState, lines: impl IntoIterator<Item = String>) {
    let view = echo_view(&state.active_view);
    let now = state.clock.now_ms();
    for text in lines {
        match &view {
            ViewId::Status => state.model.status_log.push(now, StatusLineKind::Info, text),
            ViewId::RoomList => unreachable!("Rooms List output goes to Status"),
            ViewId::Room(room_id) => state
                .model
                .room_views
                .entry(room_id.clone())
                .or_default()
                .echo
                .push(now, None, text),
            ViewId::Thread {
                room_id,
                thread_root_event_id,
            } => state
                .model
                .room_views
                .get_mut(room_id)
                .expect("a thread window's room must have a view")
                .echo
                .push(now, Some(thread_root_event_id.clone()), text),
        }
    }
}

/// Prints a command's lines in the active window, and shows them.
unsafe fn echo_in_active_view(
    hwnd: HWND,
    lines: impl IntoIterator<Item = String>,
) -> Result<(), String> {
    echo_lines_in_active_view(state_mut(hwnd), lines);
    let view = echo_view(&state_ref(hwnd).active_view);
    refresh_view_content(hwnd, &view)
}

unsafe fn set_local_command_status(hwnd: HWND, message: String) -> Result<(), String> {
    let has_status_view = {
        let state = state_mut(hwnd);
        state.model.status = message;
        state.views.contains_key(&ViewId::Status)
    };

    refresh_status_bar(hwnd)?;
    if has_status_view {
        refresh_view_content(hwnd, &ViewId::Status)?;
    }
    Ok(())
}

fn resolve_direct_room_query_target(model: &UiModel, user_id: &str) -> QueryTargetResolution {
    let mut matches = model
        .rooms
        .iter()
        .filter(|room| room.is_direct && room.direct_user_ids.iter().any(|id| id == user_id));
    let Some(room) = matches.next() else {
        return QueryTargetResolution::None;
    };
    if matches.next().is_some() {
        return QueryTargetResolution::Ambiguous;
    }
    QueryTargetResolution::Single(room.room_id.clone())
}

/// The members the client knows in a room, in nick list order.
fn room_members<'m>(model: &'m UiModel, room_id: &str) -> &'m [RoomMemberSummary] {
    model
        .room_views
        .get(room_id)
        .map(|view| view.members.as_slice())
        .unwrap_or_default()
}

/// The joined room a member command acts on, and the member the nick names
/// there.
fn room_member_target<'s>(
    state: &'s WindowState,
    nick: &str,
) -> Result<(String, &'s RoomMemberSummary), String> {
    let room_id = resolve_joined_room_target(state, None)?;
    let member = commands::resolve_nick(room_members(&state.model, &room_id), nick)?;
    Ok((room_id, member))
}

/// Enqueues a command for the core, recording it for the automation dump.
fn enqueue_command(state: &mut WindowState, command: Command) -> Result<(), String> {
    state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(())
}

/// Every known member of every room; a user in several rooms is listed
/// once per room.
fn all_room_members(model: &UiModel) -> impl Iterator<Item = &RoomMemberSummary> {
    model
        .room_views
        .values()
        .flat_map(|view| view.members.iter())
}

/// The nick a member of a known room goes by, if any room knows them with
/// a display name.
fn known_nick(model: &UiModel, user_id: &str) -> Option<String> {
    all_room_members(model)
        .filter(|member| member.user_id == user_id)
        .find_map(|member| member.display_name.clone())
}

/// The user `/ignore` and `/unignore` name: a user ID, which need not be a
/// member of any known room, or a known member's nick. Ignored users stay
/// nameable by ID once they leave every room.
fn ignore_target(model: &UiModel, target: &str) -> Result<String, String> {
    Ok(commands::resolve_user(all_room_members(model), target)?.user_id)
}

/// Where `/msg` sends: a room by ID, alias or display name, else the query
/// with a user named by ID or nick. Like mIRC's `/msg`, it does not open a
/// query.
fn msg_target_room_id(model: &UiModel, target: &str) -> Result<String, String> {
    match resolve_room(&model.rooms, is_navigable_room, target) {
        Ok(room) => return Ok(room.room_id.clone()),
        Err(RoomTargetError::Ambiguous) => return Err(format!("ambiguous target: {target}")),
        Err(RoomTargetError::Unknown) => {}
    }
    let user_id = commands::resolve_user(all_room_members(model), target)?.user_id;
    match resolve_direct_room_query_target(model, &user_id) {
        QueryTargetResolution::Single(room_id) => Ok(room_id),
        QueryTargetResolution::Ambiguous => Err(format!("ambiguous query target: {user_id}")),
        QueryTargetResolution::None => Err(format!("no query with {target}; use /query {target}")),
    }
}

/// `/whois`'s answer: the server's profile, with the nick and the joined
/// rooms shared with the user from what the client knows.
fn whois_lines(model: &UiModel, profile: &UserProfile) -> Vec<String> {
    let shared_rooms = model
        .rooms
        .iter()
        .filter(|room| is_sendable_room(room))
        .filter_map(|room| {
            let member = model
                .room_views
                .get(&room.room_id)?
                .members
                .iter()
                .find(|member| member.user_id == profile.user_id)?;
            Some((member.rank, room.display_name.as_str()))
        })
        .collect::<Vec<_>>();
    let room_nick = known_nick(model, &profile.user_id);
    commands::whois_lines(profile, room_nick.as_deref(), &shared_rooms)
}

fn resolve_joined_room_target(state: &WindowState, target: Option<&str>) -> Result<String, String> {
    let Some(target) = target else {
        return match &state.active_view {
            ViewId::Room(room_id) => {
                if sendable_room_by_id(&state.model.rooms, room_id).is_some() {
                    Ok(room_id.clone())
                } else {
                    Err(format!("room is not joined: {room_id}"))
                }
            }
            ViewId::Thread { room_id, .. } => Ok(room_id.clone()),
            ViewId::Status | ViewId::RoomList => Err("no room selected".to_owned()),
        };
    };

    match resolve_room(&state.model.rooms, is_navigable_room, target) {
        Ok(room) if is_sendable_room(room) => Ok(room.room_id.clone()),
        Ok(_) => Err(format!("room is not joined: {target}")),
        Err(RoomTargetError::Unknown) => Err(format!("unknown room: {target}")),
        Err(RoomTargetError::Ambiguous) => Err(format!("ambiguous room: {target}")),
    }
}

fn resolve_invite_command_room_id(
    state: &WindowState,
    action: &str,
    target: Option<&str>,
) -> Result<String, String> {
    let Some(target) = target else {
        return match &state.active_view {
            ViewId::Room(room_id)
                if room_by_id(&state.model.rooms, room_id)
                    .is_some_and(is_invite_command_target) =>
            {
                Ok(room_id.clone())
            }
            _ => Err(format!("{action} failed: no invited room selected")),
        };
    };

    let rooms = &state.model.rooms;
    match resolve_room(rooms, is_invite_command_target, target) {
        Ok(room) => Ok(room.room_id.clone()),
        Err(RoomTargetError::Ambiguous) => Err(format!(
            "{action} failed: ambiguous invited room target: {target}"
        )),
        Err(RoomTargetError::Unknown) => {
            match resolve_room(rooms, is_joined_message_room, target) {
                Ok(_) | Err(RoomTargetError::Ambiguous) => {
                    Err(format!("{action} failed: room is not invited: {target}"))
                }
                Err(RoomTargetError::Unknown) => {
                    Err(format!("{action} failed: invited room not found: {target}"))
                }
            }
        }
    }
}

/// The user's homeserver name, for commands that need a session.
fn own_server(model: &UiModel) -> Result<&str, String> {
    if matches!(
        model.connection,
        ConnectionState::SignedOut | ConnectionState::SigningIn
    ) {
        return Err("not connected to server".to_owned());
    }
    let own_user_id = model
        .own_user_id
        .as_deref()
        .expect("a signed-in session knows its user");
    let (_, server) = own_user_id
        .split_once(':')
        .expect("a Matrix user ID names its server");
    Ok(server)
}

/// `/create`: a public room's alias must be on the user's own server, the
/// only one that can publish it.
fn submit_create_room(state: &mut WindowState, args: CreateRoomArgs) -> Result<String, String> {
    let own_server = own_server(&state.model)?;
    let request = match args {
        CreateRoomArgs::Private { name } => CreateRoomRequest {
            name,
            visibility: CreateRoomVisibility::Private,
        },
        CreateRoomArgs::Public {
            alias_localpart,
            alias_server,
            name,
        } => {
            if alias_server.is_some_and(|server| server != own_server) {
                return Err(format!("the alias must be on your server ({own_server})"));
            }
            CreateRoomRequest {
                name: name.unwrap_or_else(|| alias_localpart.clone()),
                visibility: CreateRoomVisibility::Public { alias_localpart },
            }
        }
    };
    let status = format!("create submitted: {}", request.name);
    enqueue_command(state, Command::CreateRoom(request))?;
    Ok(status)
}

fn submit_join_room(
    state: &mut WindowState,
    target: String,
    via: Vec<String>,
) -> Result<String, String> {
    let command = Command::JoinRoom(JoinRoomRequest {
        room_or_alias: target.clone(),
        via,
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue join room command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("join submitted: {target}"))
}

fn submit_accept_slash_command(
    state: &mut WindowState,
    target: Option<&str>,
) -> Result<String, String> {
    let room_id = resolve_invite_command_room_id(state, "accept", target)?;
    let command = Command::AcceptRoomInvite(AcceptRoomInviteRequest {
        room_id: room_id.clone(),
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue accept room invite command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("accept submitted: {room_id}"))
}

fn submit_reject_slash_command(
    state: &mut WindowState,
    target: Option<&str>,
) -> Result<String, String> {
    let room_id = resolve_invite_command_room_id(state, "reject", target)?;
    let command = Command::RejectRoomInvite(RejectRoomInviteRequest {
        room_id: room_id.clone(),
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue reject room invite command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("reject submitted: {room_id}"))
}

fn submit_part_slash_command(
    state: &mut WindowState,
    target: Option<&str>,
) -> Result<String, String> {
    let room_id = resolve_joined_room_target(state, target)?;
    let command = Command::LeaveRoom(LeaveRoomRequest {
        room_id: room_id.clone(),
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue leave room command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("part submitted: {room_id}"))
}

/// Room > Notifications: sets the room's mode on the server. The check
/// follows the next room snapshot, not the click.
fn submit_room_notification_mode(
    state: &mut WindowState,
    target: Option<&str>,
    mode: RoomNotificationMode,
) -> Result<String, String> {
    let room_id = resolve_joined_room_target(state, target)?;
    let display_name = sendable_room_by_id(&state.model.rooms, &room_id)
        .expect("the joined room target is a joined room")
        .display_name
        .clone();
    enqueue_command(
        state,
        Command::SetRoomNotificationMode(SetRoomNotificationModeRequest { room_id, mode }),
    )?;
    Ok(format!(
        "notifications for {display_name}: {}",
        notification_mode_label(mode)
    ))
}

fn submit_react_slash_command(
    state: &mut WindowState,
    target_event_id: String,
    key: String,
) -> Result<String, String> {
    let room_id = resolve_joined_room_target(state, None)?;
    let command = Command::SendReaction(SendReactionRequest {
        room_id: room_id.clone(),
        target_event_id: target_event_id.clone(),
        key,
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue send reaction command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("Reaction queued for {target_event_id}"))
}

/// Replaces the user's message `target_event_id` in the active window with
/// `presentation` and `input`; sends nothing when that is what it says.
fn send_edit(
    state: &mut WindowState,
    target_event_id: &str,
    presentation: RoomMessagePresentationKind,
    input: &str,
) -> Result<(), String> {
    let message = replies::editable_message(
        window_message(&state.model, &state.active_view, target_event_id)
            .ok_or_else(|| "no such message in this window".to_owned())?,
        state.model.own_user_id.as_deref(),
    )?;
    let (body, styles) = message_text_from_input(input)?;
    if message.presentation == presentation && message.body == body && message.styles == styles {
        return Ok(());
    }
    let room_id = resolve_joined_room_target(state, None)?;
    enqueue_command(
        state,
        Command::EditMessage(EditMessageRequest {
            room_id,
            target_event_id: target_event_id.to_owned(),
            presentation,
            body,
            styles,
        }),
    )
}

/// Sends a reply to the message `target_event_id` in the active window, in
/// its thread when the window is one.
fn send_reply(
    state: &mut WindowState,
    target_event_id: &str,
    presentation: RoomMessagePresentationKind,
    input: &str,
) -> Result<(), String> {
    let (body, styles) = message_text_from_input(input)?;
    let destination = resolve_compose_destination(state, None)?;
    let sender_user_id = room_activity_sender_user_id(
        window_message(&state.model, &state.active_view, target_event_id)
            .ok_or_else(|| "no such message in this window".to_owned())?,
    )
    .expect("a message that is not redacted has a sender")
    .to_owned();
    let clears_active_thread_marker = destination.thread.is_some();
    enqueue_send_message(
        state,
        SendMessageRequest {
            room_id: destination.room_id,
            presentation,
            body,
            styles,
            thread: destination.thread,
            reply: Some(SendMessageReplyTarget {
                event_id: target_event_id.to_owned(),
                sender_user_id,
            }),
        },
    )?;
    if clears_active_thread_marker {
        clear_view_activity_marker(state, &state.active_view.clone());
    }
    Ok(())
}

fn submit_redact_event_command(state: &mut WindowState, event_id: String) -> Result<(), String> {
    let room_id = resolve_joined_room_target(state, None)?;
    let command = Command::RedactEvent(RedactEventRequest { room_id, event_id });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue redact event command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(())
}

fn submit_redact_slash_command(
    state: &mut WindowState,
    event_id: String,
) -> Result<String, String> {
    submit_redact_event_command(state, event_id.clone())?;
    Ok(format!("Redaction queued for {event_id}"))
}

fn submit_unreact_slash_command(
    state: &mut WindowState,
    reaction_event_id: String,
) -> Result<String, String> {
    submit_redact_event_command(state, reaction_event_id.clone())?;
    Ok(format!("Reaction redaction queued for {reaction_event_id}"))
}

fn submit_query_slash_command(state: &mut WindowState, user_id: String) -> Result<String, String> {
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    let command = Command::CreateDirectRoom(CreateDirectRoomRequest {
        user_id: user_id.clone(),
    });
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue create direct room command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("query submitted: {user_id}"))
}

/// `/topic [#room]`: the room's topic, as mIRC prints it.
fn room_topic_line(state: &WindowState, room: Option<&str>) -> Result<String, String> {
    let room_id = resolve_joined_room_target(state, room)?;
    Ok(commands::topic_line(
        state.model.room_views[&room_id].topic.as_deref(),
    ))
}

fn submit_topic_slash_command(
    state: &mut WindowState,
    room: Option<&str>,
    topic: String,
) -> Result<String, String> {
    let room_id = resolve_joined_room_target(state, room)?;
    let command = Command::SetRoomTopic(SetRoomTopicRequest {
        room_id: room_id.clone(),
        topic,
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue set room topic command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("topic change submitted for {room_id}"))
}

unsafe fn navigate_compose_history(
    hwnd: HWND,
    direction: ComposeHistoryDirection,
) -> Result<(), String> {
    let edit = compose_edit(hwnd);
    let replacement = match direction {
        ComposeHistoryDirection::Previous => {
            let current_text = read_window_text(edit);
            active_compose_history(state_mut(hwnd))
                .previous(current_text)
                .map(str::to_owned)
        }
        ComposeHistoryDirection::Next => active_compose_history(state_mut(hwnd)).next(),
    };
    if let Some(text) = replacement {
        set_compose_edit_text(edit, &text);
    }
    Ok(())
}

unsafe extern "system" fn edit_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // The editbox is a child of its MDI child, which is a child of the MDI
    // client: the state lives on the frame.
    let parent = frame_window_for(hwnd);
    let original = state_ref(parent).edit_window_proc;

    if message == WM_SETFOCUS {
        let result = CallWindowProcW(original, hwnd, message, wparam, lparam);
        if state_ref(parent).render_state_mode {
            // Snapshots must not depend on where the blinking caret is.
            HideCaret(hwnd);
        }
        return result;
    }

    // Keys reach only the focused editbox, and focusing an editbox
    // activates its window, so it is always the active window's.
    let is_compose_key = message == WM_KEYDOWN
        && matches!(
            wparam,
            VK_RETURN_KEY | VK_UP_KEY | VK_DOWN_KEY | VK_TAB_KEY | VK_ESCAPE_KEY
        );
    if is_compose_key {
        assert_eq!(
            compose_edit(parent),
            hwnd,
            "compose keys must arrive at the active window's editbox"
        );
    }

    if message == WM_KEYDOWN && wparam == VK_RETURN_KEY {
        if let Err(message) = submit_compose_from_edit(parent) {
            let _ = set_local_command_status(parent, message);
        }
        return 0;
    }

    if message == WM_KEYDOWN && wparam == VK_UP_KEY {
        let _ = navigate_compose_history(parent, ComposeHistoryDirection::Previous);
        return 0;
    }

    if message == WM_KEYDOWN && wparam == VK_DOWN_KEY {
        let _ = navigate_compose_history(parent, ComposeHistoryDirection::Next);
        return 0;
    }

    if message == WM_KEYDOWN && wparam == VK_TAB_KEY {
        let reverse = GetKeyState(VK_SHIFT as i32) < 0;
        if let Err(message) = complete_compose_nick(parent, reverse) {
            let _ = set_local_command_status(parent, message);
        }
        return 0;
    }

    if message == WM_KEYDOWN && wparam == VK_ESCAPE_KEY {
        if let Err(message) = cancel_compose_target(parent) {
            let _ = set_local_command_status(parent, message);
        }
        return 0;
    }

    if message == WM_KEYDOWN
        && let Some(key) = mirc_format::mirc_format_key(
            GetKeyState(VK_CONTROL as i32) < 0,
            GetKeyState(VK_MENU as i32) < 0,
            wparam,
        )
    {
        assert_eq!(
            compose_edit(parent),
            hwnd,
            "format keys must arrive at the active window's editbox"
        );
        insert_mirc_format_code(parent, hwnd, mirc_format::mirc_format_code(key));
        return 0;
    }

    // TranslateMessage also turns the Tab and Esc keys, and the format
    // chords, into these characters: Ctrl+B, K, U, O and R, and Ctrl+I is
    // Tab's. The key down already acted, so the edit box must not see them:
    // a single-line edit box rejects them, with a beep on Windows.
    if message == WM_CHAR
        && matches!(
            wparam,
            VK_TAB_KEY | VK_ESCAPE_KEY | 0x02 | 0x0b | 0x15 | 0x0f | 0x12
        )
    {
        return 0;
    }

    if message == WM_CHAR
        || message == WM_PASTE
        || (message == WM_KEYDOWN && wparam == VK_DELETE_KEY)
    {
        active_compose_history(state_mut(parent)).reset_navigation();
    }

    CallWindowProcW(original, hwnd, message, wparam, lparam)
}

fn message_loop() {
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        if result <= 0 {
            break;
        }

        unsafe {
            if let Some(frame) = message_frame(&message) {
                if TranslateAcceleratorW(frame, state_ref(frame).controls.accelerators, &message)
                    != 0
                {
                    continue;
                }
                let mdi_client = state_ref(frame).controls.mdi_client;
                if !mdi_client.is_null() && TranslateMDISysAccel(mdi_client, &message) != 0 {
                    continue;
                }
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// The frame that `hwnd`, one of its descendants, belongs to.
unsafe fn frame_window_for(hwnd: HWND) -> HWND {
    let frame = GetAncestor(hwnd, GA_ROOT);
    assert!(
        window_data::get::<WindowState>(frame).is_some(),
        "window {hwnd:?} is in a frame"
    );
    frame
}

/// The frame whose accelerators apply to `message`: the frame or one of its
/// descendants is its target. Other windows of the thread (dialogs, menus,
/// the system's own) have none, nor have thread messages.
unsafe fn message_frame(message: &MSG) -> Option<HWND> {
    let root = GetAncestor(message.hwnd, GA_ROOT);
    window_class_is(root, FRAME_CLASS_NAME).then_some(root)
}

/// Whether `hwnd` is a window of class `class_name`; false for no window.
unsafe fn window_class_is(hwnd: HWND, class_name: &str) -> bool {
    let mut buffer = [0u16; 256];
    let len = GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
    buffer[..len as usize]
        .iter()
        .copied()
        .eq(class_name.encode_utf16())
}

unsafe fn state_ref(hwnd: HWND) -> &'static WindowState {
    state_mut(hwnd)
}

unsafe fn state_mut(hwnd: HWND) -> &'static mut WindowState {
    window_data::get(hwnd).expect("window state must exist")
}

unsafe fn default_window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Messages before WM_NCCREATE and after WM_NCDESTROY.
    let Some(state) = window_data::get::<WindowState>(hwnd) else {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    };

    let mdi_client = state.controls.mdi_client;
    if mdi_client.is_null() {
        DefWindowProcW(hwnd, message, wparam, lparam)
    } else {
        DefFrameProcW(hwnd, mdi_client, message, wparam, lparam)
    }
}

fn submit_mark_room_read(
    state: &mut WindowState,
    room_id: &str,
    event_id: String,
) -> Result<(), String> {
    let command = Command::MarkRoomRead(MarkRoomReadRequest {
        room_id: room_id.to_owned(),
        event_id,
    });
    let Some(sender) = state.command_sender.as_ref() else {
        return Ok(());
    };
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue mark room read command: {err}"))?;
    #[cfg(test)]
    state.recorded_commands.push(command);
    Ok(())
}

fn loading_room_history_room_id(room_views: &BTreeMap<String, RoomViewModel>) -> Option<String> {
    let mut loading_room_ids = room_views
        .iter()
        .filter(|(_, room_view)| {
            matches!(
                room_view.history.status,
                RoomHistoryBackfillStatus::Loading { .. }
            )
        })
        .map(|(room_id, _)| room_id.clone());
    let loading_room_id = loading_room_ids.next();
    #[cfg(test)]
    {
        assert!(
            loading_room_ids.next().is_none(),
            "room history backfill must keep a single in-flight loading room"
        );
    }
    loading_room_id
}

fn submit_backfill_room_history_for_room(
    state: &mut WindowState,
    room_id: &str,
) -> Result<bool, String> {
    if sendable_room_by_id(&state.model.rooms, room_id).is_none() {
        return Ok(false);
    }
    if loading_room_history_room_id(&state.model.room_views).is_some() {
        return Ok(false);
    }

    let Some(sender) = state.command_sender.as_ref().cloned() else {
        return Ok(false);
    };

    let command = {
        let room_view = state
            .model
            .room_views
            .entry(room_id.to_owned())
            .or_default();
        if matches!(
            room_view.history.status,
            RoomHistoryBackfillStatus::Exhausted
        ) {
            return Ok(false);
        }

        let from = room_view.history.next_token.clone();
        room_view.history.status = RoomHistoryBackfillStatus::Loading { from: from.clone() };
        Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
            room_id: room_id.to_owned(),
            from,
            limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
        })
    };

    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue backfill room history command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(true)
}

unsafe fn submit_backfill_room_history(hwnd: HWND) -> Result<(), String> {
    let room_content = GetParent(hwnd);
    if room_content.is_null() {
        return Ok(());
    }
    let mdi_child = GetParent(room_content);
    if mdi_child.is_null() {
        return Ok(());
    }

    let state = timeline_window_state_mut(hwnd);
    let room_id = match &state.active_view {
        ViewId::Room(room_id) | ViewId::Thread { room_id, .. } => room_id.clone(),
        ViewId::Status | ViewId::RoomList => return Ok(()),
    };
    let Some(active_view) = state.views.get(&state.active_view) else {
        return Ok(());
    };
    if active_view.child != mdi_child {
        return Ok(());
    }

    let _ = submit_backfill_room_history_for_room(state, &room_id)?;
    Ok(())
}

fn clear_view_activity_marker(state: &mut WindowState, view_id: &ViewId) -> (bool, Option<String>) {
    let read_target = state.room_activity_read_targets.remove(view_id);
    let cleared_view_activity = state.room_activity.remove(view_id).is_some();
    let read_target = if cleared_view_activity {
        read_target
    } else {
        None
    };
    (cleared_view_activity, read_target)
}

fn handle_view_activation(state: &mut WindowState, view_id: &ViewId) -> Result<bool, String> {
    let (cleared_view_activity, read_target) = clear_view_activity_marker(state, view_id);
    if let Some(event_id) = read_target {
        let room_id = view_id
            .room_id()
            .expect("room activity markers must target room or thread views");
        submit_mark_room_read(state, room_id, event_id)?;
    }
    Ok(cleared_view_activity)
}

fn resolve_timeline_room_id(
    state: &WindowState,
    explicit_room_id: Option<&str>,
) -> Result<String, String> {
    let room_id = match explicit_room_id {
        Some(room_id) => room_id.to_owned(),
        None => state
            .model
            .selected_room
            .clone()
            .ok_or_else(|| "no room selected".to_owned())?,
    };

    if navigable_room_by_id(&state.model.rooms, &room_id).is_some() {
        Ok(room_id)
    } else {
        Err(format!("unknown room id: {room_id}"))
    }
}

fn operation_name(operation: Operation) -> &'static str {
    match operation {
        Operation::Login => "login",
        Operation::RestoreSession => "restore session",
        Operation::SessionUpdate => "session update",
        Operation::StartSync => "start sync",
        Operation::StopSync => "stop sync",
        Operation::JoinRoom => "join room",
        Operation::AcceptRoomInvite => "accept room invitation",
        Operation::CreateDirectRoom => "create direct room",
        Operation::CreateRoom => "create room",
        Operation::LeaveRoom => "leave room",
        Operation::RejectRoomInvite => "reject room invitation",
        Operation::KickRoomMember => "kick room member",
        Operation::BanRoomMember => "ban room member",
        Operation::UnbanRoomMember => "unban room member",
        Operation::InviteRoomMember => "invite room member",
        Operation::SetRoomMemberPowerLevel => "set room member power level",
        Operation::SetDisplayName => "set display name",
        Operation::SetRoomTopic => "set room topic",
        Operation::SetRoomNotificationMode => "set room notifications",
        Operation::HighlightWords => "highlight words",
        Operation::AddHighlightWord => "add highlight word",
        Operation::RemoveHighlightWord => "remove highlight word",
        Operation::IgnoredUsers => "ignore list",
        Operation::IgnoreUser => "ignore user",
        Operation::UnignoreUser => "unignore user",
        Operation::FetchUserProfile => "fetch user profile",
        Operation::SendMessage => "send text",
        Operation::SendFile => "send file",
        Operation::EditMessage => "edit message",
        Operation::SendReaction => "send reaction",
        Operation::RedactEvent => "redact event",
        Operation::MarkRoomRead => "mark room read",
        Operation::BackfillRoomHistory => "backfill room history",
        Operation::FetchRoomMedia => "fetch room image media",
        Operation::FetchRoomEvent => "fetch message",
        Operation::ListPublicRooms => "list rooms",
        Operation::RoomList => "room list",
        Operation::RoomReadState => "room read state",
        Operation::SendQueue => "send queue",
        Operation::E2eeStatus => "e2ee status",
        Operation::RecoverE2ee => "e2ee recovery",
        Operation::SetupE2eeRecovery => "e2ee recovery setup",
        Operation::Verification => "verification",
        Operation::Logout => "logout",
        Operation::Shutdown => "shutdown",
    }
}

unsafe fn read_window_text(hwnd: HWND) -> String {
    let text_len = SendMessageW(hwnd, WM_GETTEXTLENGTH, 0, 0);
    if text_len <= 0 {
        return String::new();
    }

    let mut text = vec![0u16; text_len as usize + 1];
    SendMessageW(hwnd, WM_GETTEXT, text.len(), text.as_mut_ptr() as LPARAM);
    String::from_utf16_lossy(&text[..text_len as usize])
}

fn set_window_text(hwnd: HWND, text: &str) {
    let text = wide_null(text);
    unsafe {
        SendMessageW(hwnd, WM_SETTEXT, 0, text.as_ptr() as LPARAM);
    }
}

fn set_edit_text(hwnd: HWND, text: &str) {
    set_window_text(hwnd, text);
}

unsafe fn set_compose_edit_text(hwnd: HWND, text: &str) {
    set_edit_text(hwnd, text);
    let len = text.encode_utf16().count();
    SendMessageW(hwnd, EM_SETSEL, len, len as LPARAM);
}

fn show_error_message(hwnd: HWND, title: &str, message: &str) {
    let title = wide_null(title);
    let message = wide_null(message);
    unsafe {
        MessageBoxW(hwnd, message.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

fn wide_null(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain([0]).collect()
}

unsafe fn wide_ptr_to_string(text_ptr: *const u16) -> String {
    if text_ptr.is_null() {
        return String::new();
    }

    let mut len = 0usize;
    while *text_ptr.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(text_ptr, len))
}

fn last_os_error(operation: &str) -> String {
    let error = std::io::Error::last_os_error();
    format!("{operation} failed: {error}")
}

#[cfg(test)]
mod tests {
    use super::timeline_layout::{MIRC_BLUE, MIRC_MAROON, MIRC_RED, rgb};
    use std::fs;

    use mirx_core::{
        ListPublicRoomsRequest, PublicRoomsResult, RoomMemberChange, RoomMemberRank,
        RoomStateNoticeActivity, SyncStopReason,
    };

    use windows_sys::Win32::UI::Controls::{TB_GETSTATE, TBSTATE_WRAP};
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;

    use super::test_fixtures::*;
    use super::*;

    #[test]
    fn timeline_row_is_selected_requires_a_selection() {
        assert!(!timeline_row_is_selected(None, None));
        assert!(!timeline_row_is_selected(Some("$a"), None));
        assert!(!timeline_row_is_selected(None, Some("$a")));
        assert!(!timeline_row_is_selected(Some("$b"), Some("$a")));
        assert!(timeline_row_is_selected(Some("$a"), Some("$a")));
    }
    #[test]
    fn only_an_interactive_run_without_a_session_opens_select_server_at_startup() {
        let run = |automation, render_state: Option<&str>| {
            AppCommand::Run(RunOptions {
                automation,
                render_state: render_state.map(PathBuf::from),
                frame_size: None,
            })
        };
        assert!(opens_connect_dialog_at_startup(&run(false, None), false));
        assert!(!opens_connect_dialog_at_startup(&run(false, None), true));
        assert!(!opens_connect_dialog_at_startup(&run(true, None), false));
        assert!(!opens_connect_dialog_at_startup(
            &run(false, Some("state.json")),
            false
        ));
        for command in [
            AppCommand::AutoClose,
            AppCommand::SelfTest,
            AppCommand::Help,
        ] {
            assert!(!opens_connect_dialog_at_startup(&command, false));
        }
    }

    #[test]
    fn core_events_move_the_connection_state() {
        with_local_app_data(|_| {
            let mut state = test_state();
            let (sender, _commands) = inert_command_sender();
            state.command_sender = Some(sender);
            submit_restore_session(
                &mut state,
                "https://matrix.example/".to_owned(),
                stored_session(),
                true,
            )
            .unwrap();
            assert_eq!(state.model.connection, ConnectionState::SigningIn);
            for (event, connection) in [
                (Event::RestoredSession, ConnectionState::SignedIn),
                (
                    Event::Error(CoreErrorEvent {
                        operation: Operation::RestoreSession,
                        room_id: None,
                        message: "initial Matrix sync failed".to_owned(),
                    }),
                    ConnectionState::SignedIn,
                ),
                (Event::SyncStarted, ConnectionState::Syncing),
                (
                    Event::Error(CoreErrorEvent {
                        operation: Operation::StartSync,
                        room_id: None,
                        message: "continuous sync is already running".to_owned(),
                    }),
                    ConnectionState::Syncing,
                ),
                (
                    Event::SyncStopped(SyncStopReason::Requested),
                    ConnectionState::SignedIn,
                ),
                (Event::LoggedOut, ConnectionState::SignedOut),
            ] {
                apply_core_event(&mut state, event.clone()).unwrap();
                assert_eq!(state.model.connection, connection, "{event:?}");
            }
        });
    }

    #[test]
    fn frame_size_defaults_for_run_command() {
        assert_eq!(
            frame_size_for_command(&AppCommand::Run(RunOptions {
                automation: false,
                render_state: None,
                frame_size: None,
            })),
            DEFAULT_FRAME_SIZE
        );
    }

    #[test]
    fn frame_size_uses_explicit_run_value() {
        assert_eq!(
            frame_size_for_command(&AppCommand::Run(RunOptions {
                automation: true,
                render_state: None,
                frame_size: Some(FrameSize {
                    width: 640,
                    height: 768,
                }),
            })),
            FrameSize {
                width: 640,
                height: 768,
            }
        );
    }

    #[test]
    fn frame_size_defaults_for_self_test() {
        assert_eq!(
            frame_size_for_command(&AppCommand::SelfTest),
            DEFAULT_FRAME_SIZE
        );
    }

    #[test]
    fn frame_size_defaults_for_auto_close() {
        assert_eq!(
            frame_size_for_command(&AppCommand::AutoClose),
            DEFAULT_FRAME_SIZE
        );
    }

    fn outgoing_queued_with_presentation(
        room_id: &str,
        transaction_id: &str,
        presentation: RoomMessagePresentationKind,
        body: &str,
    ) -> Event {
        Event::RoomActivity(Box::new(pending_message_activity_with_presentation(
            room_id,
            transaction_id,
            presentation,
            body,
        )))
    }

    fn outgoing_failed(room_id: &str, transaction_id: &str, message: &str) -> Event {
        Event::RoomActivityUpdate(RoomActivityUpdate {
            room_id: room_id.to_owned(),
            target: RoomActivityUpdateTarget::Transaction {
                transaction_id: transaction_id.to_owned(),
            },
            kind: RoomActivityUpdateKind::MessageFailed {
                message: message.to_owned(),
                recoverable: true,
            },
        })
    }

    fn incoming_reaction_removed(
        room_id: &str,
        target_event_id: &str,
        reaction_event_id: &str,
        redaction_event_id: &str,
        key: &str,
    ) -> Event {
        Event::RoomActivityUpdate(RoomActivityUpdate {
            room_id: room_id.to_owned(),
            target: RoomActivityUpdateTarget::Event {
                event_id: target_event_id.to_owned(),
            },
            kind: RoomActivityUpdateKind::ReactionRemoved {
                reaction_event_id: reaction_event_id.to_owned(),
                redaction_event_id: redaction_event_id.to_owned(),
                key: key.to_owned(),
            },
        })
    }

    fn history_backfill_event_from_event(event: Event) -> RoomHistoryBackfillEvent {
        match event {
            Event::RoomActivity(activity) => RoomHistoryBackfillEvent::Activity(activity),
            Event::RoomActivityUpdate(update) => RoomHistoryBackfillEvent::ActivityUpdate(update),
            other => panic!("expected room history backfill event payload, got {other:?}"),
        }
    }

    #[test]
    fn synced_room_summary_topic_seeds_updates_and_clears_room_context() {
        let mut state = test_state();
        let mut summary = room("!ops:example", "Ops", false);
        summary.topic = Some("incident review".to_owned());

        apply_core_event(&mut state, Event::SyncedRooms(vec![summary.clone()])).unwrap();
        let initial = project_timeline_rows(&build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
        ));
        assert!(initial.contains(&"* Now talking in Ops (!ops:example)".to_owned()));
        assert!(initial.contains(&"* Topic is 'incident review'".to_owned()));
        assert!(
            !initial
                .iter()
                .any(|line| line.contains("changes topic to 'incident review'"))
        );

        apply_core_event(&mut state, topic_state("!ops:example", "deployment window")).unwrap();
        let updated = project_timeline_rows(&build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
        ));
        assert!(updated.contains(&"* Topic is 'deployment window'".to_owned()));
        assert!(
            updated
                .iter()
                .any(|line| line.contains("changes topic to 'deployment window'"))
        );
        assert!(!updated.contains(&"* Topic is 'incident review'".to_owned()));

        summary.topic = None;
        apply_core_event(&mut state, Event::SyncedRooms(vec![summary])).unwrap();
        let cleared = project_timeline_rows(&build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
        ));
        assert!(!cleared.iter().any(|line| line.starts_with("* Topic is ''")));
    }

    fn member_state(
        room_id: &str,
        state_key: &str,
        change: RoomMemberChange,
        display_name: Option<&str>,
    ) -> Event {
        let event_id = format!("$member:{room_id}:{state_key}:{change:?}");
        Event::RoomActivity(Box::new(RoomActivity {
            room_id: room_id.to_owned(),
            timestamp_ms: 1_700_000_120_000,
            highlight: false,
            activity_id: format!("state:{event_id}"),
            relation: None,
            kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
                event_id,
                sender: user_profile("@alice:example"),
                content: RoomStateNoticeContent::Member {
                    change,
                    target: RoomUserProfile {
                        user_id: state_key.to_owned(),
                        display_name: display_name.map(str::to_owned),
                        display_name_ambiguous: false,
                    },
                    reason: None,
                },
            }),
        }))
    }

    unsafe fn install_test_active_timeline_event(hwnd: HWND, event_id: &str) {
        let instance = GetModuleHandleW(null());
        register_room_content_class(instance).unwrap();
        register_timeline_class(instance).unwrap();
        let content = CreateWindowExW(
            0,
            wide_null(ROOM_CONTENT_CLASS_NAME).as_ptr(),
            wide_null("").as_ptr(),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            0,
            0,
            hwnd,
            null_mut(),
            instance,
            null_mut(),
        );
        assert!(!content.is_null(), "test room content must be created");
        let active_view = state_ref(hwnd).active_view.clone();
        state_mut(hwnd)
            .views
            .get_mut(&active_view)
            .expect("the test compose window must track the active view")
            .content = content;
        set_test_timeline_font(hwnd, &active_view, content);
        set_timeline_rows(
            room_content_timeline(content),
            &[TimelineRow::log(
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                "[00:00] @alice: message".to_owned(),
            )
            .with_event_id(Some(event_id))],
            String::new(),
        );
    }

    #[test]
    fn synthetic_event_validation_rejects_invalid_message_sent_sequence() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &BTreeMap::new(),
                &[outgoing_sent("!ops:example", "txn-1", "$event:example.org")]
            ),
            Err("outgoing sent requires an existing queued timeline row".to_owned())
        );
    }

    #[test]
    fn synthetic_event_validation_accepts_pending_then_sent_sequence() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &BTreeMap::new(),
                &[
                    outgoing_queued("!ops:example", "txn-1", "hello"),
                    outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
                ]
            ),
            Ok(())
        );
    }

    #[test]
    fn synthetic_event_validation_rejects_message_edit_with_transaction_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &BTreeMap::new(),
                &[Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id: "!ops:example".to_owned(),
                    target: RoomActivityUpdateTarget::Transaction {
                        transaction_id: "txn-1".to_owned(),
                    },
                    kind: RoomActivityUpdateKind::MessageReplaced {
                        replacement_event_id: "$edit:example.org".to_owned(),
                        sender: "@alice:example".to_owned(),
                        timestamp_ms: 1_700_000_000_500,
                        presentation: RoomMessagePresentationKind::Text,
                        body: "edited".to_owned(),
                        styles: Vec::new(),
                    },
                })]
            ),
            Err("message edit requires an event target".to_owned())
        );
    }

    #[test]
    fn synthetic_event_validation_rejects_message_edit_without_visible_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &BTreeMap::new(),
                &[incoming_replaced(
                    "!ops:example",
                    "$missing:example.org",
                    "$edit:example.org",
                    RoomMessagePresentationKind::Text,
                    "edited"
                )]
            ),
            Err("message edit requires an existing visible target event".to_owned())
        );
    }

    #[test]
    fn redaction_synthetic_event_validation_rejects_missing_event_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &BTreeMap::new(),
                &[incoming_redacted(
                    "!ops:example",
                    "$missing:example.org",
                    "$redaction:example.org",
                )]
            ),
            Err("activity redaction requires an existing visible target event".to_owned())
        );
    }

    #[test]
    fn redaction_synthetic_event_validation_rejects_transaction_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &BTreeMap::new(),
                &[Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id: "!ops:example".to_owned(),
                    target: RoomActivityUpdateTarget::Transaction {
                        transaction_id: "txn-1".to_owned(),
                    },
                    kind: RoomActivityUpdateKind::ActivityRedacted {
                        redaction_event_id: "$redaction:example.org".to_owned(),
                    },
                })]
            ),
            Err("activity redacted update requires an event target".to_owned())
        );
    }

    #[test]
    fn reaction_add_synthetic_event_validation_rejects_transaction_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &BTreeMap::new(),
                &[Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id: "!ops:example".to_owned(),
                    target: RoomActivityUpdateTarget::Transaction {
                        transaction_id: "txn-1".to_owned(),
                    },
                    kind: RoomActivityUpdateKind::ReactionAdded {
                        reaction_event_id: "$reaction:example.org".to_owned(),
                        sender: "@alice:example".to_owned(),
                        timestamp_ms: 1_700_000_000_500,
                        key: "+1".to_owned(),
                    },
                })]
            ),
            Err("reaction add requires an event target".to_owned())
        );
    }

    #[test]
    fn reaction_removed_synthetic_event_validation_rejects_transaction_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &BTreeMap::new(),
                &[Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id: "!ops:example".to_owned(),
                    target: RoomActivityUpdateTarget::Transaction {
                        transaction_id: "txn-1".to_owned(),
                    },
                    kind: RoomActivityUpdateKind::ReactionRemoved {
                        reaction_event_id: "$reaction:example.org".to_owned(),
                        redaction_event_id: "$redaction:example.org".to_owned(),
                        key: "+1".to_owned(),
                    },
                })]
            ),
            Err("reaction removal requires an event target".to_owned())
        );
    }

    #[test]
    fn room_activity_events_accumulate_per_room_in_order() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!one:example", "One", false),
                room("!two:example", "Two", true),
            ]),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!one:example", "first")).unwrap();
        apply_core_event(&mut state, timeline("!one:example", "second")).unwrap();
        apply_core_event(&mut state, timeline("!two:example", "third")).unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!one:example")
                .map(|view| &view.activities),
            Some(&vec![
                sent_message_activity(
                    "!one:example",
                    "$!one:example:first",
                    None,
                    "first",
                    1_700_000_000_000,
                ),
                sent_message_activity(
                    "!one:example",
                    "$!one:example:second",
                    None,
                    "second",
                    1_700_000_000_000,
                ),
            ])
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!two:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity(
                "!two:example",
                "$!two:example:third",
                None,
                "third",
                1_700_000_000_000,
            )])
        );
        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!two:example".to_owned(),
                event_id: Some("$!two:example:third".to_owned()),
                transaction_id: None,
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "third".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn pending_room_activity_creates_one_pending_row_keyed_by_transaction_id() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![pending_message_activity_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves"
            )])
        );
        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: None,
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                presentation: RoomMessagePresentationKind::Emote,
                body: "waves".to_owned(),
                send_state: RoomMessageSendState::Pending,
            })
        );
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] * @alice:example waves (sending)".to_owned(),
            ]
        );
    }

    #[test]
    fn room_activity_update_sent_updates_existing_row_and_clears_pending_marker() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                "hello",
                1_700_000_000_000,
            )])
        );
        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: Some("$event:example.org".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "hello".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] <@alice:example> hello".to_owned(),
            ]
        );
    }

    #[test]
    fn room_activity_update_replaced_updates_existing_row_in_place() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                None,
                RoomMessagePresentationKind::Text,
                "original body",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$event:example.org",
                "$edit:example.org",
                RoomMessagePresentationKind::Emote,
                "edited body",
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity_with_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                RoomMessagePresentationKind::Emote,
                "edited body",
                1_700_000_000_000,
            )])
        );
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] * @alice:example edited body (edited)".to_owned(),
            ]
        );
    }

    #[test]
    fn room_activity_update_replaced_refreshes_last_activity_only_for_target() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$older:example.org",
                None,
                None,
                RoomMessagePresentationKind::Text,
                "older body",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$latest:example.org",
                None,
                None,
                RoomMessagePresentationKind::Text,
                "latest body",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$older:example.org",
                "$older-edit:example.org",
                RoomMessagePresentationKind::Text,
                "older edited",
            ),
        )
        .unwrap();

        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: Some("$latest:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                presentation: RoomMessagePresentationKind::Text,
                body: "latest body".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );

        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$latest:example.org",
                "$latest-edit:example.org",
                RoomMessagePresentationKind::Notice,
                "latest edited",
            ),
        )
        .unwrap();

        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: Some("$latest:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                presentation: RoomMessagePresentationKind::Notice,
                body: "latest edited".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn redaction_room_activity_update_replaces_text_message_with_tombstone() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                None,
                RoomMessagePresentationKind::Text,
                "secret body",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$event:example.org",
                "$redaction:example.org",
            ),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        assert!(matches!(
            room_view.activities.as_slice(),
            [RoomActivity {
                activity_id,
                kind: RoomActivityKind::Redacted(RoomRedactedActivity { event_id }),
                ..
            }] if activity_id == "event:$event:example.org" && event_id == "$event:example.org"
        ));
        assert_eq!(state.model.last_activity, None);

        let rendered = render_room_lines(&state, "!ops:example").join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("secret body"));
    }

    #[test]
    fn redaction_room_activity_update_overrides_edited_message_and_blocks_late_replacement() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                None,
                RoomMessagePresentationKind::Text,
                "original body",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$event:example.org",
                "$edit:example.org",
                RoomMessagePresentationKind::Emote,
                "edited body",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$event:example.org",
                "$redaction:example.org",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$event:example.org",
                "$late-edit:example.org",
                RoomMessagePresentationKind::Text,
                "late body",
            ),
        )
        .unwrap();

        let rendered = render_room_lines(&state, "!ops:example").join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("original body"));
        assert!(!rendered.contains("edited body"));
        assert!(!rendered.contains("late body"));
    }

    fn room_lines(state: &WindowState) -> Vec<String> {
        project_timeline_rows(&build_room_timeline_rows(
            &state.model,
            "!ops:example",
            state.timestamps(),
        ))
    }

    #[test]
    fn a_reply_quotes_its_loaded_target_above_it_with_the_replys_event() {
        let state = reply_quote_state("$q");
        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        assert_eq!(
            project_timeline_rows(&rows)[2..],
            [
                "    > <@alice:example> are we deploying today?".to_owned(),
                "[22:14] <@bob:example> yes, after lunch".to_owned(),
            ]
        );
        assert_eq!(rows[2].kind, TimelineRowKind::ReplyQuote);
        assert_eq!(rows[2].event_id.as_deref(), Some("$a"));
        assert_eq!(
            rows[2].identity,
            TimelineRowIdentity::activity_reply_quote("event:$a")
        );
    }

    #[test]
    fn a_reply_quote_follows_its_targets_edits_and_redaction() {
        let mut state = reply_quote_state("$q");
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$q",
                "$q-edit",
                RoomMessagePresentationKind::Emote,
                "asks whether we deploy today, because the build looks green to me",
            ),
        )
        .unwrap();
        assert_eq!(
            room_lines(&state)[2],
            "    > * @alice:example asks whether we deploy today, because the b..."
        );
        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$q", "$q-gone"),
        )
        .unwrap();
        assert_eq!(room_lines(&state)[2], "    > message redacted");
    }

    #[test]
    fn a_reply_to_a_message_not_loaded_says_it_is_loading() {
        let state = reply_quote_state("$elsewhere");
        assert_eq!(
            room_lines(&state)[2],
            "    > (loading the replied-to message...)"
        );
    }

    #[test]
    fn an_edited_message_is_marked_until_it_is_redacted() {
        let mut state = reply_quote_state("$q");
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$a",
                "$a-edit",
                RoomMessagePresentationKind::Emote,
                "says yes",
            ),
        )
        .unwrap();
        assert_eq!(
            room_lines(&state)[3],
            "[22:14] * @bob:example says yes (edited)"
        );
        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$a", "$a-gone"),
        )
        .unwrap();
        assert_eq!(room_lines(&state)[3], "[22:14] * message redacted");
        assert!(
            state.model.room_views["!ops:example"]
                .edited_event_ids
                .is_empty()
        );
    }

    fn fetch_reply_targets(state: &mut WindowState) {
        let view = &state.model.room_views["!ops:example"];
        let requests = collect_reply_target_requests(view, view.activities.iter());
        dispatch_reply_target_fetches(state, "!ops:example", requests).unwrap();
    }

    fn reply_target_fetched(result: RoomEventFetchResult) -> Event {
        Event::RoomEvent(RoomEventFetch {
            room_id: "!ops:example".to_owned(),
            event_id: "$elsewhere".to_owned(),
            result,
        })
    }

    #[test]
    fn a_reply_target_not_loaded_is_fetched_once_while_connected() {
        let mut state = reply_quote_state("$elsewhere");
        let (sender, _commands) = inert_command_sender();
        state.command_sender = Some(sender);

        fetch_reply_targets(&mut state);
        assert!(state.recorded_commands.is_empty(), "no session to ask");

        state.model.connection = ConnectionState::Syncing;
        fetch_reply_targets(&mut state);
        fetch_reply_targets(&mut state);
        assert_eq!(
            state.recorded_commands,
            vec![Command::FetchRoomEvent(FetchRoomEventRequest {
                room_id: "!ops:example".to_owned(),
                event_id: "$elsewhere".to_owned(),
            })]
        );
        assert_eq!(
            room_lines(&state)[2],
            "    > (loading the replied-to message...)"
        );

        apply_core_event(
            &mut state,
            reply_target_fetched(RoomEventFetchResult::Loaded {
                activity: Box::new(sent_message_activity(
                    "!ops:example",
                    "$elsewhere",
                    None,
                    "from long ago",
                    1_600_000_000_000,
                )),
            }),
        )
        .unwrap();
        assert_eq!(
            room_lines(&state)[2],
            "    > <@alice:example> from long ago"
        );
    }

    #[test]
    fn a_reply_target_the_server_cannot_give_is_unavailable() {
        let mut state = reply_quote_state("$elsewhere");
        apply_core_event(
            &mut state,
            reply_target_fetched(RoomEventFetchResult::Failed {
                message: "unsupported event".to_owned(),
            }),
        )
        .unwrap();
        assert_eq!(
            room_lines(&state)[2],
            "    > (the replied-to message is unavailable)"
        );
    }

    #[test]
    fn a_fetched_reply_target_for_a_room_without_a_view_is_dropped() {
        let mut state = test_state();
        apply_core_event(
            &mut state,
            reply_target_fetched(RoomEventFetchResult::Failed {
                message: "joined room not found: !ops:example".to_owned(),
            }),
        )
        .unwrap();
        assert!(state.model.room_views.is_empty());
    }

    #[test]
    fn a_thread_reply_to_its_root_has_no_quote() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, timeline("!ops:example", "root")).unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$t1", "$!ops:example:root", "in thread"),
        )
        .unwrap();
        let rows = build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$!ops:example:root",
            state.timestamps(),
        );
        assert!(
            rows.iter()
                .all(|row| row.kind != TimelineRowKind::ReplyQuote),
            "{:?}",
            project_timeline_rows(&rows)
        );
    }

    #[test]
    fn reaction_projection_renders_single_summary_row_after_annotated_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event:example.org",
                None,
                "hello",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$event:example.org",
                "$reaction-1:example.org",
                "@bob:example.org",
                "+1",
                1_700_000_000_500,
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event,
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                TimelineRowKind::ReactionSummary,
            ]
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] <@alice:example> hello".to_owned(),
                "    reactions: +1 @bob:example.org".to_owned(),
            ]
        );
    }

    #[test]
    fn reaction_projection_aggregates_keys_in_reaction_time_into_one_summary_row() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-heart-1",
                "@alice:example.org",
                "heart",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-plus",
                "@bob:example.org",
                "+1",
                1_700_000_000_200,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-eyes",
                "@carol:example.org",
                "eyes",
                1_700_000_000_300,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-heart-2",
                "@dave:example.org",
                "heart",
                1_700_000_000_400,
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        assert_eq!(
            rows.iter()
                .filter(|row| row.kind == TimelineRowKind::ReactionSummary)
                .count(),
            1
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] <@alice:example> hello".to_owned(),
                "    reactions: heart @alice:example.org, @dave:example.org  +1 @bob:example.org  eyes @carol:example.org".to_owned(),
            ]
        );
    }

    #[test]
    fn reaction_row_names_reactors_by_member_nick_once_members_arrive() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        for (reaction_event_id, sender, timestamp_ms) in [
            ("$r1", "@alice:example", 1_700_000_000_300),
            ("$r2", "@ghost:example", 1_700_000_000_200),
        ] {
            apply_core_event(
                &mut state,
                incoming_reaction_added(
                    "!ops:example",
                    "$!ops:example:hello",
                    reaction_event_id,
                    sender,
                    "+1",
                    timestamp_ms,
                ),
            )
            .unwrap();
        }
        let reaction_row = |state: &WindowState| {
            build_room_timeline_rows(&state.model, "!ops:example", state.timestamps())
                .into_iter()
                .find(|row| row.kind == TimelineRowKind::ReactionSummary)
                .expect("the message has a reaction row")
        };
        assert_eq!(
            reaction_row(&state).logical_text,
            "    reactions: +1 @ghost:example, @alice:example"
        );

        // Members arrive after the reactions; the row is rebuilt from them.
        apply_core_event(
            &mut state,
            Event::RoomMembers(RoomMembers {
                room_id: "!ops:example".to_owned(),
                members: vec![member_summary("@alice:example", Some("Alice"), false)],
            }),
        )
        .unwrap();
        let row = reaction_row(&state);
        assert_eq!(row.logical_text, "    reactions: +1 @ghost:example, Alice");
        // It wraps like a log line instead of being clipped.
        assert_eq!(row.layout, TimelineRowLayout::Log);
    }

    #[test]
    fn same_reaction_event_id_does_not_double_count() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "@bob:example.org",
                "+1",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "@bob:example.org",
                "+1",
                1_700_000_000_100,
            ),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        assert_eq!(
            room_view.reaction_summaries_for_event("$!ops:example:hello"),
            vec![DumpRoomReactionSummary {
                key: "+1".to_owned(),
                count: 1,
                senders: vec!["@bob:example.org".to_owned()],
                reaction_event_ids: vec!["$reaction-1".to_owned()],
            }]
        );
    }

    #[test]
    fn different_reaction_event_ids_with_same_sender_and_key_count_separately() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "@bob:example.org",
                "+1",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-2",
                "@bob:example.org",
                "+1",
                1_700_000_000_200,
            ),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        assert_eq!(
            room_view.reaction_summaries_for_event("$!ops:example:hello"),
            vec![DumpRoomReactionSummary {
                key: "+1".to_owned(),
                count: 2,
                senders: vec!["@bob:example.org".to_owned(), "@bob:example.org".to_owned(),],
                reaction_event_ids: vec!["$reaction-1".to_owned(), "$reaction-2".to_owned()],
            }]
        );
    }

    #[test]
    fn reaction_removed_removes_only_matching_event_id_and_key() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "@bob:example.org",
                "+1",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-2",
                "@carol:example.org",
                "+1",
                1_700_000_000_200,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-3",
                "@dave:example.org",
                "heart",
                1_700_000_000_300,
            ),
        )
        .unwrap();

        apply_core_event(
            &mut state,
            incoming_reaction_removed(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "$redaction-wrong-key",
                "heart",
            ),
        )
        .unwrap();
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] <@alice:example> hello".to_owned(),
                "    reactions: +1 @bob:example.org, @carol:example.org  heart @dave:example.org"
                    .to_owned(),
            ]
        );

        apply_core_event(
            &mut state,
            incoming_reaction_removed(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "$redaction-right",
                "+1",
            ),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] <@alice:example> hello".to_owned(),
                "    reactions: +1 @carol:example.org  heart @dave:example.org".to_owned(),
            ]
        );
    }

    #[test]
    fn redacting_target_removes_reaction_summary_and_leaves_no_reaction_row() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event:example.org",
                None,
                "hello",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$event:example.org",
                "$reaction-1:example.org",
                "@bob:example.org",
                "+1",
                1_700_000_000_500,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$event:example.org",
                "$redaction:example.org",
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event,
                TimelineRowKind::StateNotice { highlight: false }
            ]
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] * message redacted".to_owned(),
            ]
        );
    }

    #[test]
    fn thread_timeline_projection_includes_reactions_for_root_and_reply() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "thread rep"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$thread-root",
                "$reaction-root",
                "@bob:example.org",
                "heart",
                1_700_000_060_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$thread-1",
                "$reaction-reply",
                "@carol:example.org",
                "eyes",
                1_700_000_120_100,
            ),
        )
        .unwrap();

        let rows = build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
            state.timestamps(),
        );
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event,
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                TimelineRowKind::ReactionSummary,
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                TimelineRowKind::ReactionSummary,
            ]
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:14] <@alice:example> root msg".to_owned(),
                "    reactions: heart @bob:example.org".to_owned(),
                "[22:15] <@alice:example> thread rep".to_owned(),
                "    reactions: eyes @carol:example.org".to_owned(),
            ]
        );
    }

    #[test]
    fn direct_message_edit_without_target_leaves_visible_timeline_unchanged() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        let before = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist")
            .activities
            .clone();

        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$missing:example.org",
                "$edit:example.org",
                RoomMessagePresentationKind::Text,
                "edited",
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&before)
        );
    }

    #[test]
    fn redaction_runtime_unknown_event_target_leaves_visible_timeline_unchanged() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        let before_activities = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist")
            .activities
            .clone();
        let before_last_activity = state.model.last_activity.clone();

        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$missing:example.org",
                "$redaction:example.org",
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&before_activities)
        );
        assert_eq!(state.model.last_activity, before_last_activity);
    }

    #[test]
    fn typed_sent_message_reconciles_by_transaction_id_without_duplicate_rows() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id_with_presentation(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_000_100,
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity_with_presentation(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_000_100,
            )])
        );
    }

    #[test]
    fn synced_message_before_sent_update_removes_duplicate_by_event_id_fallback() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id_with_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| view.activities.len()),
            Some(1)
        );
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] * @alice:example waves".to_owned(),
            ]
        );
        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: Some("$event:example.org".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                presentation: RoomMessagePresentationKind::Emote,
                body: "waves".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn synced_message_without_transaction_id_collapses_into_outgoing_row_after_sent_update() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id_with_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity_with_presentation(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_000_100,
            )])
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| view.activities.len()),
            Some(1)
        );
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] * @alice:example waves".to_owned(),
            ]
        );
    }

    #[test]
    fn failed_message_update_keeps_visible_failed_row_and_error_details() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_failed("!ops:example", "txn-1", "send failed"),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![failed_message_activity_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
                "send failed",
            )])
        );
        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: None,
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                presentation: RoomMessagePresentationKind::Emote,
                body: "waves".to_owned(),
                send_state: RoomMessageSendState::Failed {
                    message: "send failed".to_owned(),
                    recoverable: true,
                },
            })
        );
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] * @alice:example waves (failed)".to_owned(),
            ]
        );
    }

    #[test]
    fn failed_room_activity_loaded_directly_renders_failed_row() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            failed_message_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
                "send failed",
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![failed_message_activity_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
                "send failed",
            )])
        );
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[22:13] * @alice:example waves (failed)".to_owned(),
            ]
        );
    }

    #[test]
    fn clear_room_timeline_preserves_pending_room_activity_for_sent_update() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "first")).unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();

        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view must exist after timeline update")
            .clear_visible_entries();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![pending_message_activity(
                "!ops:example",
                "txn-1",
                "hello"
            )])
        );

        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                "hello",
                1_700_000_000_000,
            )])
        );
    }

    #[test]
    fn clear_room_timeline_removes_unsupported_placeholders() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            unsupported_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "com.example.custom",
                "clip.mp4",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();

        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view must exist after timeline update")
            .clear_visible_entries();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![pending_message_activity(
                "!ops:example",
                "txn-1",
                "hello"
            )])
        );
    }

    #[test]
    fn clear_room_timeline_removes_image_rows() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                Some("diagram.png"),
                Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: None,
                }),
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();

        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view must exist after timeline update")
            .clear_visible_entries();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![pending_message_activity(
                "!ops:example",
                "txn-1",
                "hello"
            )])
        );
    }

    #[test]
    #[should_panic(expected = "outgoing sent requires an existing queued timeline row")]
    fn room_activity_update_sent_without_existing_queued_row_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "outgoing sent requires an existing queued timeline row")]
    fn room_activity_update_sent_after_sent_row_with_same_transaction_id_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                "hello",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "outgoing sent requires an existing queued timeline row")]
    fn room_activity_update_sent_after_failed_row_with_same_transaction_id_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            failed_message("!ops:example", "txn-1", "hello", "send failed"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "outgoing failed requires an existing queued timeline row")]
    fn room_activity_update_failed_without_existing_queued_row_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_failed("!ops:example", "txn-1", "send failed"),
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "outgoing failed requires an existing queued timeline row")]
    fn room_activity_update_failed_after_sent_row_with_same_transaction_id_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                "hello",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_failed("!ops:example", "txn-1", "send failed"),
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "outgoing failed requires an existing queued timeline row")]
    fn room_activity_update_failed_after_failed_row_with_same_transaction_id_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            failed_message("!ops:example", "txn-1", "hello", "send failed"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_failed("!ops:example", "txn-1", "send failed again"),
        )
        .unwrap();
    }

    /// Each of `row`'s styled spans as the text it covers.
    fn styled_row_texts(row: &TimelineRow) -> Vec<(&str, mirx_core::TextStyle)> {
        row.styles
            .iter()
            .map(|span| (&row.logical_text[span.start..span.end], span.style))
            .collect()
    }

    #[test]
    fn message_rows_carry_their_styles_past_the_nick_and_never_on_the_suffix() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", true)];
        for (event_id, presentation) in [
            ("$text", RoomMessagePresentationKind::Text),
            ("$emote", RoomMessagePresentationKind::Emote),
            ("$notice", RoomMessagePresentationKind::Notice),
        ] {
            let mut activity = sent_message_activity_with_presentation(
                "!ops:example",
                event_id,
                None,
                presentation,
                "say bold",
                1_700_000_000_000,
            );
            let RoomActivityKind::Message(message) = &mut activity.kind else {
                unreachable!("a message activity");
            };
            message.styles = vec![TextStyleSpan {
                start: 4,
                end: 8,
                style: bold_style(),
            }];
            apply_core_event(&mut state, Event::RoomActivity(Box::new(activity))).unwrap();
        }
        let Event::RoomActivityUpdate(mut edit) = incoming_replaced(
            "!ops:example",
            "$text",
            "$edit",
            RoomMessagePresentationKind::Text,
            "now bold",
        ) else {
            unreachable!("a replacement");
        };
        let RoomActivityUpdateKind::MessageReplaced { styles, .. } = &mut edit.kind else {
            unreachable!("a replacement");
        };
        *styles = vec![TextStyleSpan {
            start: 0,
            end: 8,
            style: bold_style(),
        }];
        apply_core_event(&mut state, Event::RoomActivityUpdate(edit)).unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example", utc_timestamps())
            .into_iter()
            .filter(|row| row.event_id.is_some())
            .collect::<Vec<_>>();
        assert_eq!(
            rows.iter()
                .map(|row| (row.logical_text.as_str(), styled_row_texts(row)))
                .collect::<Vec<_>>(),
            vec![
                (
                    "[22:13] <@alice:example> now bold (edited)",
                    vec![("now bold", bold_style())]
                ),
                (
                    "[22:13] * @alice:example say bold",
                    vec![("bold", bold_style())]
                ),
                (
                    "[22:13] -@alice:example- say bold",
                    vec![("bold", bold_style())]
                ),
            ]
        );
    }

    #[test]
    fn build_room_timeline_rows_capture_kind_order_and_projection() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", true)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org", "@bob:example.org"]),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event-text:example.org",
                None,
                "<body>",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(sent_message_activity_with_presentation(
                "!ops:example",
                "$event-notice:example.org",
                None,
                RoomMessagePresentationKind::Notice,
                "maintenance window",
                1_700_000_030_000,
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(sent_message_activity_with_presentation(
                "!ops:example",
                "$event-emote:example.org",
                None,
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_090_000,
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-pending", "pending local echo"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            failed_message("!ops:example", "txn-failed", "failed local echo", "network"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "body fallback",
                Some("diagram.png"),
                Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: None,
                }),
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(file_activity(
                "!ops:example",
                "$file1:example.org",
                "@alice:example.org",
                1_710_000_060_000,
                "Quarterly report",
                Some("report.pdf"),
                Some(RoomFileInfo {
                    mimetype: Some("application/pdf".to_owned()),
                    size: Some(45_678),
                }),
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(audio_activity(
                "!ops:example",
                "$audio1:example.org",
                "@alice:example.org",
                1_710_000_120_000,
                "Voice note",
                Some("voice-note.ogg"),
                Some(RoomAudioInfo {
                    mimetype: Some("audio/ogg".to_owned()),
                    size: Some(23_456),
                    duration_ms: Some(12_000),
                }),
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            unsupported_message(
                "!ops:example",
                "$video1:example.org",
                "@alice:example.org",
                1_710_000_180_000,
                "com.example.custom",
                "clip.mp4",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            member_state(
                "!ops:example",
                "@carol:example",
                RoomMemberChange::Joined,
                Some("carol"),
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            member_state(
                "!ops:example",
                "@dave:example",
                RoomMemberChange::Left,
                None,
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event,
                TimelineRowKind::Event,
                TimelineRowKind::Info,
                TimelineRowKind::StateNotice { highlight: false },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Notice,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Emote,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Pending,
                    highlight: false,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Failed,
                    highlight: false,
                },
                TimelineRowKind::Image { highlight: false },
                TimelineRowKind::File { highlight: false },
                TimelineRowKind::Audio { highlight: false },
                TimelineRowKind::Unsupported { highlight: false },
                TimelineRowKind::StateNotice { highlight: false },
                TimelineRowKind::StateNotice { highlight: false },
                TimelineRowKind::Typing,
            ]
        );
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            project_timeline_rows(&rows)
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "* Topic is 'incident review'".to_owned(),
                "* Messages in this room are end-to-end encrypted".to_owned(),
                "[22:14] * @alice:example changes topic to 'incident review'".to_owned(),
                "[22:13] <@alice:example> <body>".to_owned(),
                "[22:13] -@alice:example- maintenance window".to_owned(),
                "[22:14] * @alice:example waves".to_owned(),
                "[22:13] <@alice:example> pending local echo (sending)".to_owned(),
                "[22:13] <@alice:example> failed local echo (failed)".to_owned(),
                "[16:00] * @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)"
                    .to_owned(),
                "[16:01] * @alice:example.org sent file: report.pdf (application/pdf, 45678 bytes)"
                    .to_owned(),
                "[16:02] * @alice:example.org sent audio: voice-note.ogg (audio/ogg, 23456 bytes, 12000 ms)"
                    .to_owned(),
                "[16:03] * @alice:example.org sent unsupported com.example.custom: clip.mp4"
                    .to_owned(),
                "[22:15] * Joins: carol (@carol:example)".to_owned(),
                "[22:15] * Parts: @dave:example".to_owned(),
                "* @alice:example.org, @bob:example.org are typing".to_owned(),
            ]
        );
        let (typing, log_rows) = rows.split_last().expect("rows end with the typing row");
        assert_eq!(typing.layout, TimelineRowLayout::FullWidth);
        for row in log_rows {
            assert_eq!(row.layout, TimelineRowLayout::Log, "{}", row.logical_text);
        }
    }

    #[test]
    fn typing_indicator_names_typists_by_member_nick() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            Event::RoomMembers(RoomMembers {
                room_id: "!ops:example".to_owned(),
                members: vec![
                    member_summary("@alice:example", Some("Alice"), false),
                    member_summary("@sam1:example", Some("Sam"), true),
                ],
            }),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            typing_event(
                "!ops:example",
                &["@alice:example", "@sam1:example", "@stranger:example"],
            ),
        )
        .unwrap();

        assert!(
            render_room_lines(&state, "!ops:example")
                .contains(&"* Alice, Sam (@sam1:example), @stranger:example are typing".to_owned())
        );
    }

    #[test]
    fn member_view_rows_disambiguate_by_the_core_ambiguity_flag() {
        let labels = room_member_view_rows(&[
            member_summary("@alex:example", Some("Alex"), true),
            member_summary("@bo1:example", Some("Bo"), false),
            member_summary("@bo2:example", Some("Bo"), false),
            member_summary("@nameless:example", None, false),
        ])
        .into_iter()
        .map(|member| member.label)
        .collect::<Vec<_>>();

        assert_eq!(
            labels,
            vec!["Alex (@alex:example)", "Bo", "Bo", "@nameless:example"]
        );
    }

    #[test]
    fn member_view_rows_keep_core_order_and_carry_rank_beside_the_bare_label() {
        let ranked = |user_id: &str, name: &str, rank| RoomMemberSummary {
            rank,
            ..member_summary(user_id, Some(name), false)
        };
        let rows = room_member_view_rows(&[
            ranked("@zed:example", "Zed", RoomMemberRank::Moderator),
            ranked("@amy:example", "Amy", RoomMemberRank::Voiced),
        ]);

        assert_eq!(
            rows.iter()
                .map(|row| (row.label.as_str(), row.rank))
                .collect::<Vec<_>>(),
            vec![
                ("Zed", RoomMemberRank::Moderator),
                ("Amy", RoomMemberRank::Voiced)
            ]
        );
    }

    #[test]
    fn room_lines_show_sender_display_names() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        let mut activity =
            sent_message_activity("!ops:example", "$named", None, "hello", 1_700_000_000_000);
        let RoomActivityKind::Message(message) = &mut activity.kind else {
            unreachable!("message activity helper builds messages");
        };
        message.sender = RoomUserProfile {
            user_id: "@sam1:example".to_owned(),
            display_name: Some("Sam".to_owned()),
            display_name_ambiguous: true,
        };
        apply_core_event(&mut state, Event::RoomActivity(Box::new(activity))).unwrap();

        assert!(
            render_room_lines(&state, "!ops:example")
                .iter()
                .any(|line| line.ends_with("<Sam (@sam1:example)> hello"))
        );
    }

    #[test]
    fn render_room_lines_include_text_notice_emote_and_state_activities() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", true)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org", "@bob:example.org"]),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event-text:example.org",
                None,
                "<body>",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(sent_message_activity_with_presentation(
                "!ops:example",
                "$event-notice:example.org",
                None,
                RoomMessagePresentationKind::Notice,
                "maintenance window",
                1_700_000_030_000,
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(sent_message_activity_with_presentation(
                "!ops:example",
                "$event-emote:example.org",
                None,
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_090_000,
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            member_state(
                "!ops:example",
                "@carol:example",
                RoomMemberChange::Joined,
                Some("carol"),
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            member_state(
                "!ops:example",
                "@dave:example",
                RoomMemberChange::Left,
                None,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            member_state(
                "!ops:example",
                "@erin:example",
                RoomMemberChange::Banned,
                Some("erin"),
            ),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "* Topic is 'incident review'".to_owned(),
                "* Messages in this room are end-to-end encrypted".to_owned(),
                "[22:14] * @alice:example changes topic to 'incident review'".to_owned(),
                "[22:13] <@alice:example> <body>".to_owned(),
                "[22:13] -@alice:example- maintenance window".to_owned(),
                "[22:14] * @alice:example waves".to_owned(),
                "[22:15] * Joins: carol (@carol:example)".to_owned(),
                "[22:15] * Parts: @dave:example".to_owned(),
                "[22:15] * erin was banned by @alice:example".to_owned(),
                "* @alice:example.org, @bob:example.org are typing".to_owned(),
            ]
        );
    }

    #[test]
    fn room_timeline_rows_show_loading_history_backfill_after_context_rows() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!ops:example", "latest row")).unwrap();
        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view exists after room activity")
            .history
            .status = RoomHistoryBackfillStatus::Loading { from: None };

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());

        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event,
                TimelineRowKind::Event,
                TimelineRowKind::Plain,
                TimelineRowKind::StateNotice { highlight: false },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                TimelineRowKind::Typing,
            ]
        );
        assert_eq!(rows[2].logical_text, "* Loading older history...");
    }

    #[test]
    fn room_timeline_rows_show_failed_history_backfill_after_context_rows() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!ops:example", "latest row")).unwrap();
        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view exists after room activity")
            .history
            .status = RoomHistoryBackfillStatus::Failed {
            from: Some("token-1".to_owned()),
            message: "homeserver unavailable".to_owned(),
        };

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());

        assert_eq!(rows[2].kind, TimelineRowKind::Plain);
        assert_eq!(
            rows[2].logical_text,
            "* Unable to load older history: homeserver unavailable. Scroll to top to retry."
        );
    }

    #[test]
    fn room_timeline_rows_show_exhausted_history_backfill_after_context_rows() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!ops:example", "latest row")).unwrap();
        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view exists after room activity")
            .history
            .status = RoomHistoryBackfillStatus::Exhausted;

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());

        assert_eq!(rows[2].kind, TimelineRowKind::Plain);
        assert_eq!(rows[2].logical_text, "* No older history available.");
    }

    #[test]
    fn room_timeline_rows_skip_idle_history_backfill_row() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!ops:example", "latest row")).unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());

        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Event,
                TimelineRowKind::Event,
                TimelineRowKind::StateNotice { highlight: false },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                TimelineRowKind::Typing,
            ]
        );
        assert!(
            rows.iter()
                .all(|row| !row.logical_text.contains("older history"))
        );
    }

    #[test]
    fn redaction_state_notice_renders_generic_tombstone_without_old_notice_text() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            member_state(
                "!ops:example",
                "@carol:example",
                RoomMemberChange::Joined,
                Some("carol"),
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$member:!ops:example:@carol:example:Joined",
                "$redaction:example.org",
            ),
        )
        .unwrap();

        let rendered = render_room_lines(&state, "!ops:example").join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("Joins: carol"));
    }

    #[test]
    fn room_tree_label_has_no_e2ee_or_query_markers() {
        let mut direct_room = room("!direct:example", "Direct", true);
        direct_room.is_direct = true;
        direct_room.direct_user_ids = vec!["@alice:example".to_owned()];
        assert_eq!(room_tree_label(&direct_room), "Direct");

        direct_room.direct_user_ids.clear();
        assert_eq!(room_tree_label(&direct_room), "Direct");

        assert_eq!(room_tree_label(&room("!ops:example", "Ops", true)), "Ops");
    }

    #[test]
    fn room_tree_label_marks_invites() {
        let mut invite = room("!invite:example", "Lobby", true);
        invite.membership = RoomMembership::Invite;
        invite.is_direct = true;
        assert_eq!(room_tree_label(&invite), "Lobby [invite]");
    }

    #[test]
    fn room_navigation_label_appends_read_state_suffix_to_the_name() {
        let mut state = test_state();
        let mut direct_room = room("!direct:example", "Direct", true);
        direct_room.is_direct = true;
        direct_room.direct_user_ids = vec!["@alice:example".to_owned()];
        state.model.rooms = vec![direct_room];
        state.model.room_read_states.insert(
            "!direct:example".to_owned(),
            RoomReadState {
                room_id: "!direct:example".to_owned(),
                latest_active_event_id: Some("$latest".to_owned()),
                fully_read_event_id: Some("$fully-read".to_owned()),
                marked_unread: true,
                num_unread: 3,
                num_notifications: 2,
                num_mentions: 1,
            },
        );

        assert_eq!(
            room_navigation_label(&state.model, &state.model.rooms[0]),
            "Direct [@1 !2 *3 M]"
        );
    }

    #[test]
    fn room_navigation_label_handles_sparse_and_absent_read_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!todo:example", "Todo", false)];

        assert_eq!(
            room_navigation_label(&state.model, &state.model.rooms[0]),
            "Todo"
        );

        state.model.room_read_states.insert(
            "!todo:example".to_owned(),
            RoomReadState {
                room_id: "!todo:example".to_owned(),
                latest_active_event_id: None,
                fully_read_event_id: None,
                marked_unread: true,
                num_unread: 0,
                num_notifications: 0,
                num_mentions: 0,
            },
        );
        assert_eq!(
            room_navigation_label(&state.model, &state.model.rooms[0]),
            "Todo [M]"
        );

        state.model.room_read_states.insert(
            "!todo:example".to_owned(),
            RoomReadState {
                room_id: "!todo:example".to_owned(),
                latest_active_event_id: None,
                fully_read_event_id: None,
                marked_unread: false,
                num_unread: 7,
                num_notifications: 0,
                num_mentions: 0,
            },
        );
        assert_eq!(
            room_navigation_label(&state.model, &state.model.rooms[0]),
            "Todo [*7]"
        );

        state.model.room_read_states.insert(
            "!todo:example".to_owned(),
            RoomReadState {
                room_id: "!todo:example".to_owned(),
                latest_active_event_id: Some("$latest".to_owned()),
                fully_read_event_id: Some("$latest".to_owned()),
                marked_unread: false,
                num_unread: 0,
                num_notifications: 0,
                num_mentions: 0,
            },
        );
        assert_eq!(
            room_navigation_label(&state.model, &state.model.rooms[0]),
            "Todo"
        );
    }

    #[test]
    fn room_opening_lines_leave_label_markers_to_the_caption() {
        let mut state = test_state();
        let mut direct_room = room("!direct:example", "Direct", true);
        direct_room.is_direct = true;
        direct_room.direct_user_ids = vec!["@alice:example".to_owned()];
        state.model.rooms = vec![direct_room];

        let rows = build_room_timeline_rows(&state.model, "!direct:example", state.timestamps());
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![TimelineRowKind::Event, TimelineRowKind::Info]
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "* Now talking in Direct (!direct:example)".to_owned(),
                "* Messages in this room are end-to-end encrypted".to_owned(),
            ]
        );
    }

    #[test]
    fn render_room_lines_include_unsupported_message_placeholder() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            unsupported_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "com.example.custom",
                "clip.mp4",
            ),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[16:00] * @alice:example.org sent unsupported com.example.custom: clip.mp4"
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn system_local_time_matches_utc_under_the_harness_tz() {
        // The Wine harness pins TZ=UTC, so snapshots and tests that go
        // through the app's own conversion see UTC times.
        for timestamp_ms in [0, 951_782_400_000, 1_700_000_000_000, 1_720_000_000_999] {
            assert_eq!(
                system_local_time(timestamp_ms),
                timestamp::utc_local_time(timestamp_ms)
            );
        }
    }

    #[test]
    fn room_and_status_rows_follow_the_timestamp_setting() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(sent_message_activity(
                "!ops:example",
                "$hi:example",
                None,
                "hi",
                1_700_000_000_000,
            ))),
        )
        .unwrap();
        state.model.status_log.push(
            1_700_000_000_000,
            StatusLineKind::Info,
            "* ready".to_owned(),
        );

        state
            .settings
            .update(|settings| settings.timestamp.show = false)
            .unwrap();
        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "<@alice:example> hi".to_owned(),
            ]
        );
        assert_eq!(
            project_timeline_rows(&build_status_timeline_rows(
                &state.model.status_log,
                state.timestamps()
            )),
            vec!["* ready".to_owned()]
        );
        assert_eq!(state.timestamps().indent_sample(), "");

        state
            .settings
            .update(|settings| {
                settings.timestamp = TimestampSetting {
                    show: true,
                    format: timestamp::TimestampFormat::parse("[HH:nn:ss]").unwrap(),
                }
            })
            .unwrap();
        assert_eq!(
            render_room_lines(&state, "!ops:example")[1],
            "[22:13:20] <@alice:example> hi"
        );
        assert_eq!(
            project_timeline_rows(&build_status_timeline_rows(
                &state.model.status_log,
                state.timestamps()
            )),
            vec!["[22:13:20] * ready".to_owned()]
        );
    }

    #[test]
    fn render_room_lines_include_unable_to_decrypt_placeholder() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", true)];
        apply_core_event(
            &mut state,
            unable_to_decrypt_activity(
                "!ops:example",
                "$encrypted1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
            ),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "* Messages in this room are end-to-end encrypted".to_owned(),
                "[16:00] * @alice:example.org sent an encrypted message that could not be decrypted"
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn redaction_unsupported_message_hides_msgtype_and_body() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            unsupported_message(
                "!ops:example",
                "$unsupported:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "com.example.custom",
                "clip.mp4",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$unsupported:example.org",
                "$redaction:example.org",
            ),
        )
        .unwrap();

        let rendered = render_room_lines(&state, "!ops:example").join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("com.example.custom"));
        assert!(!rendered.contains("clip.mp4"));
    }

    #[test]
    fn render_room_lines_include_typed_image_with_metadata() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "body fallback",
                Some("diagram.png"),
                Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: None,
                }),
            ),
        )
        .unwrap();

        let lines = render_room_lines(&state, "!ops:example");
        assert_eq!(
            lines,
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[16:00] * @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)"
                    .to_owned(),
            ]
        );
        assert!(!lines.join("\n").contains("unsupported m.image"));
    }

    #[test]
    fn render_room_lines_omit_empty_image_metadata_suffix() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "[16:00] * @alice:example.org sent image: diagram.png".to_owned(),
            ]
        );
    }

    #[test]
    fn image_rows_project_pending_media_state_without_media_event() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        let image_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Image { .. }))
            .expect("image row must be present");

        assert_eq!(image_row.image_state, Some(TimelineImageState::Pending));
        assert!(
            !render_room_lines(&state, "!ops:example")
                .join("\n")
                .contains("loading image...")
        );
    }

    #[test]
    fn failed_room_preview_media_event_projects_failed_image_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "unavailable".to_owned(),
                },
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .and_then(|room_view| room_view.preview_media.get("event:$image1:example.org"))
                .cloned(),
            Some(RoomPreviewMediaState::Failed {
                event_id: "$image1:example.org".to_owned(),
                format: timeline_thumbnail_format(),
                message: "unavailable".to_owned(),
            })
        );

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        let image_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Image { .. }))
            .expect("image row must be present");
        assert_eq!(
            image_row.image_state,
            Some(TimelineImageState::Failed {
                message: "unavailable".to_owned(),
            })
        );
        assert!(
            !render_room_lines(&state, "!ops:example")
                .join("\n")
                .contains("image unavailable")
        );
    }

    #[test]
    fn video_preview_media_projects_failure_and_ignores_wrong_format() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            video_message("!ops:example", "$video1:example.org"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$video1:example.org",
                "$video1:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "unavailable".to_owned(),
                },
            ),
        )
        .unwrap();
        assert!(
            build_room_timeline_rows(&state.model, "!ops:example", state.timestamps())
                .iter()
                .any(|row| matches!(
                    row.image_state,
                    Some(TimelineImageState::Failed { ref message }) if message == "unavailable"
                ))
        );
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$video1:example.org",
                "$video1:example.org",
                RoomMediaKind::Failed {
                    format: RoomMediaFormat::File,
                    message: "wrong".to_owned(),
                },
            ),
        )
        .unwrap();
        assert_eq!(
            state.model.room_views["!ops:example"].preview_media.len(),
            1
        );
    }

    #[test]
    fn invalid_loaded_room_preview_media_event_projects_failed_image_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomMediaKind::Loaded {
                    format: timeline_thumbnail_format(),
                    bytes: vec![0, 1, 2, 3],
                },
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        let image_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Image { .. }))
            .expect("image row must be present");
        assert!(matches!(
            image_row.image_state,
            Some(TimelineImageState::Failed { .. })
        ));
    }

    #[test]
    fn loaded_room_preview_media_event_projects_loaded_image_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomMediaKind::Loaded {
                    format: timeline_thumbnail_format(),
                    bytes: tiny_png_bytes(),
                },
            ),
        )
        .unwrap();

        let image_key = room_preview_media_storage_key(
            "!ops:example",
            "event:$image1:example.org",
            "$image1:example.org",
            &timeline_thumbnail_format(),
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .and_then(|room_view| room_view.preview_media.get("event:$image1:example.org"))
                .cloned(),
            Some(RoomPreviewMediaState::Loaded {
                event_id: "$image1:example.org".to_owned(),
                format: timeline_thumbnail_format(),
                image_key: image_key.clone(),
                width: 1,
                height: 1,
            })
        );
        assert!(state.decoded_images.contains_key(&image_key));

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        let image_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Image { .. }))
            .expect("image row must be present");
        assert_eq!(
            image_row.image_state,
            Some(TimelineImageState::Loaded {
                key: image_key,
                width: 1,
                height: 1,
            })
        );
    }

    #[test]
    fn thread_image_rows_use_room_backed_preview_media_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        let mut activity = image_activity(
            "!ops:example",
            "$thread-image:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "diagram.png",
            None,
            None,
        );
        activity.relation = Some(thread_relation("$thread-root:example.org"));
        apply_core_event(&mut state, Event::RoomActivity(Box::new(activity))).unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$thread-image:example.org",
                "$thread-image:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "unavailable".to_owned(),
                },
            ),
        )
        .unwrap();

        let rows = build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root:example.org",
            state.timestamps(),
        );
        let image_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Image { .. }))
            .expect("thread image row must be present");
        assert_eq!(
            image_row.image_state,
            Some(TimelineImageState::Failed {
                message: "unavailable".to_owned(),
            })
        );
    }

    #[test]
    fn thread_rows_open_like_the_room_and_end_with_typing() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", true)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        let mut event = video_message("!ops:example", "$thread-video:example.org");
        let Event::RoomActivity(activity) = &mut event else {
            unreachable!("video message must create a room activity");
        };
        activity.relation = Some(thread_relation("$thread-root:example.org"));
        apply_core_event(&mut state, event).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();

        let rows = build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root:example.org",
            state.timestamps(),
        );
        let lines = project_timeline_rows(&rows);
        assert_eq!(
            lines[..3],
            [
                "* Now talking in Ops (!ops:example)".to_owned(),
                "* Topic is 'incident review'".to_owned(),
                "* Messages in this room are end-to-end encrypted".to_owned(),
            ]
        );
        assert!(matches!(rows[3].kind, TimelineRowKind::Video { .. }));
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[4].kind, TimelineRowKind::Typing);
        assert_eq!(lines[4], "* @alice:example.org is typing");
    }

    #[test]
    fn thread_video_rows_use_room_backed_preview_media_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        let mut event = video_message("!ops:example", "$thread-video:example.org");
        let Event::RoomActivity(activity) = &mut event else {
            unreachable!("video message must create a room activity");
        };
        activity.relation = Some(thread_relation("$thread-root:example.org"));
        apply_core_event(&mut state, event).unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$thread-video:example.org",
                "$thread-video:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "unavailable".to_owned(),
                },
            ),
        )
        .unwrap();

        let rows = build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root:example.org",
            state.timestamps(),
        );
        let video_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Video { .. }))
            .expect("thread video row must be present");
        assert_eq!(
            video_row.image_state,
            Some(TimelineImageState::Failed {
                message: "unavailable".to_owned(),
            })
        );
    }

    #[test]
    fn room_typing_replaces_prior_user_list_and_empty_list_clears_it() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();
        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| view.typing_user_ids.clone()),
            Some(vec!["@alice:example.org".to_owned()])
        );

        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org", "@bob:example.org"]),
        )
        .unwrap();
        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| view.typing_user_ids.clone()),
            Some(vec![
                "@alice:example.org".to_owned(),
                "@bob:example.org".to_owned(),
            ])
        );

        apply_core_event(&mut state, typing_event("!ops:example", &[])).unwrap();
        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| view.typing_user_ids.clone()),
            Some(Vec::new())
        );
    }

    #[test]
    fn render_room_lines_use_singular_typing_verb_for_one_user() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state, "!ops:example"),
            vec![
                "* Now talking in Ops (!ops:example)".to_owned(),
                "* @alice:example.org is typing".to_owned(),
            ]
        );
    }

    #[test]
    fn room_typing_does_not_change_last_activity_or_room_activity_markers() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        state.active_view = ViewId::Status;

        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();

        assert_eq!(state.model.last_activity, None);
        assert!(state.room_activity.is_empty());
        assert_eq!(state.model.status, "offline");
    }

    #[test]
    fn verification_request_event_stores_requested_state_and_logs_actions() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            Event::VerificationRequest(VerificationRequest {
                flow_id: "flow-verify-123".to_owned(),
                user_id: "@alice:example".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(
            state.model.last_verification,
            Some(VerificationStateEvent {
                flow_id: "flow-verify-123".to_owned(),
                user_id: "@alice:example".to_owned(),
                state: VerificationState::Requested,
            })
        );
        assert_eq!(
            status_log_lines(&state),
            vec![
                "[22:12] * Verification requested by @alice:example (flow flow-verify-123)"
                    .to_owned(),
                "[22:12] * Actions: Tools > Accept Verification, Tools > Cancel Verification, /verify accept, /verify cancel"
                    .to_owned(),
            ]
        );
    }
    #[test]
    fn verification_sas_ready_and_errors_are_logged_to_status() {
        let mut state = test_state();
        apply_core_event(
            &mut state,
            Event::VerificationState(VerificationStateEvent {
                flow_id: "flow-verify-123".to_owned(),
                user_id: "@alice:example".to_owned(),
                state: VerificationState::SasReady {
                    emojis: vec![
                        VerificationEmoji {
                            symbol: "🐶".to_owned(),
                            description: "dog".to_owned(),
                        },
                        VerificationEmoji {
                            symbol: "🔥".to_owned(),
                            description: "fire".to_owned(),
                        },
                    ],
                    decimals: mirx_core::VerificationDecimals {
                        first: 1234,
                        second: 5678,
                        third: 9012,
                    },
                },
            }),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::SendMessage,
                room_id: Some("!left:example".to_owned()),
                message: "message send failed".to_owned(),
            }),
        )
        .unwrap();

        let rows = build_status_timeline_rows(&state.model.status_log, state.timestamps());
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "[22:12] * Verification with @alice:example: compare the emoji or the numbers"
                    .to_owned(),
                "[22:12] * Emoji: 🐶 dog | 🔥 fire".to_owned(),
                "[22:12] * Decimals: 1234-5678-9012".to_owned(),
                "[22:12] * Actions: Tools > Confirm SAS, Tools > Reject SAS, Tools > Cancel Verification, /verify confirm, /verify reject, /verify cancel"
                    .to_owned(),
                "[22:12] * Error during send text: message send failed".to_owned(),
            ]
        );
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            [vec![TimelineRowKind::Info; 4], vec![TimelineRowKind::Error],].concat()
        );
    }
    #[test]
    fn e2ee_status_event_updates_model_and_logs_status() {
        let mut state = test_state();
        let status = e2ee_status(
            E2eeRecoveryState::Incomplete,
            E2eeBackupState::Enabled,
            false,
            Some(true),
            Some(false),
        );

        apply_core_event(&mut state, Event::E2eeStatus(status.clone())).unwrap();

        assert_eq!(state.model.last_e2ee_status, Some(status));
        assert_eq!(state.model.status, "e2ee status updated");
        assert_eq!(
            status_log_lines(&state),
            vec![
                "[22:12] * E2EE: recovery incomplete, local backup disabled, server backup exists, cross-signing incomplete"
                    .to_owned(),
                "[22:12] * Use /e2ee refresh to check server backup, /e2ee setup to enable recovery, /e2ee recover to restore keys"
                    .to_owned(),
            ]
        );
    }
    #[test]
    fn e2ee_recovery_succeeded_event_sets_status_message() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            Event::E2eeRecoverySucceeded(mirx_core::E2eeRecoverySucceeded {
                operation: mirx_core::E2eeRecoveryOperation::Recover,
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "e2ee recovery complete");
    }

    #[test]
    fn e2ee_recovery_setup_succeeded_event_only_sets_non_secret_status() {
        let mut state = test_state();
        let recovery_key = "recovery-key-must-not-persist";

        apply_core_event(
            &mut state,
            Event::E2eeRecoverySetupSucceeded(mirx_core::E2eeRecoverySetupSucceeded {
                recovery_key: recovery_key.to_owned(),
            }),
        )
        .unwrap();

        let status_lines = status_log_lines(&state);
        assert_eq!(state.model.status, "e2ee recovery setup complete");
        assert!(!state.model.status.contains(recovery_key));
        assert_eq!(
            status_lines,
            vec![
                "[22:12] * E2EE recovery set up; the recovery key was shown in a dialog".to_owned()
            ]
        );
        assert!(!status_lines.iter().any(|line| line.contains(recovery_key)));
        assert!(!format!("{:?}", dump_model_from_state(&state)).contains(recovery_key));
        assert!(
            !state
                .views
                .values()
                .flat_map(|view| &view.compose().history.entries)
                .any(|entry| entry.contains(recovery_key))
        );
    }

    #[test]
    fn normalize_timeline_text_converts_embedded_newlines_to_crlf() {
        let text = "[22:13] <@bob:example> multiline row 00 line a\nmultiline row 00 line b";
        let (normalized, lines) = normalize_timeline_text(text);

        assert_eq!(
            normalized,
            "[22:13] <@bob:example> multiline row 00 line a\r\nmultiline row 00 line b"
        );
        assert_eq!(
            lines,
            vec![
                "[22:13] <@bob:example> multiline row 00 line a".to_owned(),
                "multiline row 00 line b".to_owned(),
            ]
        );
    }

    #[test]
    fn normalize_timeline_text_preserves_trailing_blank_line() {
        let text = "trailing blank line a\n";
        let (normalized, lines) = normalize_timeline_text(text);

        assert_eq!(normalized, "trailing blank line a\r\n");
        assert_eq!(
            lines,
            vec!["trailing blank line a".to_owned(), String::new()]
        );
    }

    #[test]
    fn clamped_timeline_top_line_respects_visible_row_growth() {
        assert_eq!(clamped_timeline_top_line(8, 10, 3), 7);
        assert_eq!(clamped_timeline_top_line(8, 10, 5), 5);
        assert_eq!(clamped_timeline_top_line(2, 10, 20), 0);
    }

    #[test]
    fn timeline_content_replacement_follows_tail_or_retains_scrolled_position() {
        assert!(timeline_is_tail_pinned(0, 0, 0, 3));
        assert_eq!(timeline_top_line_after_content_replaced(0, true, 10, 3), 7);

        assert!(timeline_is_tail_pinned(8, 5, 8, 3));
        assert_eq!(timeline_top_line_after_content_replaced(5, true, 12, 3), 9);

        assert!(!timeline_is_tail_pinned(8, 2, 8, 3));
        assert_eq!(timeline_top_line_after_content_replaced(2, false, 12, 3), 2);

        assert_eq!(timeline_top_line_after_content_replaced(8, false, 10, 5), 5);
    }

    #[test]
    fn history_backfill_anchor_skips_only_fixed_room_context() {
        let rows = vec![
            TimelineRow::log(
                TimelineRowKind::Event,
                "* Now talking in Beta (!beta:example)".to_owned(),
            ),
            TimelineRow::log(TimelineRowKind::Event, "* Topic is 'incident'".to_owned()),
            TimelineRow::log(
                TimelineRowKind::ReactionSummary,
                "    reactions: 👍 Alice".to_owned(),
            ),
            TimelineRow::log(
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                "[22:13] <@bob:example> current row".to_owned(),
            )
            .with_event_id(Some("$current")),
        ];

        assert_eq!(
            timeline_history_backfill_anchor_row_index(&rows, 0),
            Some(2)
        );
        assert_eq!(
            timeline_history_backfill_anchor_row_index(&rows, 2),
            Some(2)
        );
    }

    #[test]
    fn frame_status_text_returns_lifecycle_status_only_when_no_details_exist() {
        let mut state = test_state();
        state.model.status = "authenticated".to_owned();

        assert_eq!(frame_status_text(&state), "authenticated");
    }

    #[test]
    fn frame_status_text_appends_e2ee_suffix_only_after_status_event() {
        let mut state = test_state();
        state.model.status = "authenticated".to_owned();

        assert_eq!(frame_status_text(&state), "authenticated");

        apply_core_event(
            &mut state,
            Event::E2eeStatus(e2ee_status(
                E2eeRecoveryState::Incomplete,
                E2eeBackupState::Enabled,
                true,
                Some(true),
                Some(true),
            )),
        )
        .unwrap();

        assert_eq!(
            frame_status_text(&state),
            "e2ee status updated | e2ee: recovery incomplete, backup healthy, cross-signing complete"
        );
    }

    #[test]
    fn frame_status_text_includes_flow_user_and_compact_state() {
        let mut state = test_state();
        state.model.status = "verification accepted with @alice:example".to_owned();
        state.model.last_verification = Some(VerificationStateEvent {
            flow_id: "flow-verify-123".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::Accepted,
        });

        assert_eq!(
            frame_status_text(&state),
            "verification accepted with @alice:example | verification: accepted flow-verify-123 from @alice:example"
        );
    }

    #[test]
    fn frame_status_text_appends_error_details() {
        let mut state = test_state();
        state.model.status = "error during send text".to_owned();
        state.model.last_error = Some(CoreErrorEvent {
            operation: Operation::SendMessage,
            room_id: Some("!room:example".to_owned()),
            message: "message send failed".to_owned(),
        });

        assert_eq!(
            frame_status_text(&state),
            "error during send text | error: send text: message send failed"
        );
    }

    #[test]
    fn frame_status_text_appends_active_view_find_feedback_without_mutating_status() {
        let mut state = test_state();
        state.model.status = "authenticated".to_owned();
        state.find_states.insert(
            ViewId::Status,
            build_view_find_state("hello", "hello", Vec::new(), None),
        );

        assert_eq!(
            frame_status_text(&state),
            "authenticated | find: no matches for \"hello\""
        );
        assert_eq!(state.model.status, "authenticated");
    }

    #[test]
    fn frame_status_text_shows_active_invite_context() {
        let mut state = test_state();
        state.model.status = "synced 1 room(s)".to_owned();
        state.model.rooms = vec![invited_room("!invite:example", "Invite")];
        state.active_view = room_view_id("!invite:example");

        assert_eq!(
            frame_status_text(&state),
            "invited: Invite (!invite:example)"
        );
    }

    #[test]
    fn submission_status_survives_matching_error_event() {
        let mut state = test_state();
        state.model.status = "join submitted: #ops:example.org".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::JoinRoom,
                room_id: None,
                message: "join room requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "join submitted: #ops:example.org");
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::JoinRoom,
                room_id: None,
                message: "join room requires an authenticated session".to_owned(),
            })
        );

        state.model.status = "accept submitted: !invite:example".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::AcceptRoomInvite,
                room_id: Some("!invite:example".to_owned()),
                message: "accept room invitation requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "accept submitted: !invite:example");
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::AcceptRoomInvite,
                room_id: Some("!invite:example".to_owned()),
                message: "accept room invitation requires an authenticated session".to_owned(),
            })
        );

        state.model.status = "part submitted: !beta:example".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::LeaveRoom,
                room_id: Some("!beta:example".to_owned()),
                message: "leave room requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "part submitted: !beta:example");
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::LeaveRoom,
                room_id: Some("!beta:example".to_owned()),
                message: "leave room requires an authenticated session".to_owned(),
            })
        );

        state.model.status = "reject submitted: !invite:example".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::RejectRoomInvite,
                room_id: Some("!invite:example".to_owned()),
                message: "reject room invitation requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "reject submitted: !invite:example");
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::RejectRoomInvite,
                room_id: Some("!invite:example".to_owned()),
                message: "reject room invitation requires an authenticated session".to_owned(),
            })
        );

        state.model.status = "query submitted: @alice:example.org".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::CreateDirectRoom,
                room_id: None,
                message: "create direct room requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "query submitted: @alice:example.org");
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::CreateDirectRoom,
                room_id: None,
                message: "create direct room requires an authenticated session".to_owned(),
            })
        );

        state.model.status = "topic change submitted for !ops:example.org".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::SetRoomTopic,
                room_id: Some("!ops:example.org".to_owned()),
                message: "set room topic requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(
            state.model.status,
            "topic change submitted for !ops:example.org"
        );
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::SetRoomTopic,
                room_id: Some("!ops:example.org".to_owned()),
                message: "set room topic requires an authenticated session".to_owned(),
            })
        );
    }

    #[test]
    fn frame_status_text_appends_verification_before_error_when_both_exist() {
        let mut state = test_state();
        state.model.status = "verification accepted with @alice:example".to_owned();
        state.model.last_verification = Some(VerificationStateEvent {
            flow_id: "flow-verify-123".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::Accepted,
        });
        state.model.last_error = Some(CoreErrorEvent {
            operation: Operation::SendMessage,
            room_id: Some("!room:example".to_owned()),
            message: "message send failed".to_owned(),
        });

        assert_eq!(
            frame_status_text(&state),
            "verification accepted with @alice:example | verification: accepted flow-verify-123 from @alice:example | error: send text: message send failed"
        );
    }

    #[test]
    fn verification_done_logs_terminal_text_without_actions() {
        let mut state = test_state();
        apply_core_event(
            &mut state,
            Event::VerificationState(VerificationStateEvent {
                flow_id: "flow-verify-123".to_owned(),
                user_id: "@alice:example".to_owned(),
                state: VerificationState::Done,
            }),
        )
        .unwrap();

        assert_eq!(
            status_log_lines(&state),
            vec!["[22:12] * Verification done with @alice:example: verified".to_owned()]
        );
    }
    #[test]
    fn verification_cancelled_logs_reason_code_without_actions() {
        let mut state = test_state();
        apply_core_event(
            &mut state,
            Event::VerificationState(VerificationStateEvent {
                flow_id: "flow-verify-123".to_owned(),
                user_id: "@alice:example".to_owned(),
                state: VerificationState::Cancelled {
                    reason: "timed out".to_owned(),
                    code: "m.timeout".to_owned(),
                    cancelled_by_us: false,
                },
            }),
        )
        .unwrap();

        assert_eq!(
            status_log_lines(&state),
            vec![
                "[22:12] * Verification cancelled with @alice:example: timed out (m.timeout)"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn resolve_active_view_preserves_existing_active_room() {
        let model = UiModel {
            rooms: vec![
                room("!keep:example", "Keep", false),
                room("!other:example", "Other", false),
            ],
            ..UiModel::default()
        };

        assert_eq!(
            resolve_active_view(
                &ViewId::Room("!keep:example".to_owned()),
                Some("!other:example"),
                &model,
                &BTreeSet::new(),
            ),
            ViewId::Room("!keep:example".to_owned())
        );
    }

    #[test]
    fn resolve_active_view_falls_back_to_first_room_when_active_room_is_stale() {
        let model = UiModel {
            rooms: vec![
                room("!alpha:example", "Alpha", false),
                room("!beta:example", "Beta", false),
            ],
            ..UiModel::default()
        };

        assert_eq!(
            resolve_active_view(
                &ViewId::Room("!gone:example".to_owned()),
                None,
                &model,
                &BTreeSet::new(),
            ),
            ViewId::Room("!alpha:example".to_owned())
        );
    }

    #[test]
    fn resolve_active_view_skips_spaces_when_falling_back() {
        let model = UiModel {
            rooms: vec![
                space("!space:example", "Space"),
                room("!alpha:example", "Alpha", false),
                room("!beta:example", "Beta", false),
            ],
            ..UiModel::default()
        };

        assert_eq!(
            resolve_active_view(
                &ViewId::Room("!gone:example".to_owned()),
                None,
                &model,
                &BTreeSet::new(),
            ),
            ViewId::Room("!alpha:example".to_owned())
        );
    }

    #[test]
    fn resolve_active_view_falls_back_to_status_when_no_rooms_remain() {
        let model = UiModel::default();
        assert_eq!(
            resolve_active_view(
                &ViewId::Room("!gone:example".to_owned()),
                None,
                &model,
                &BTreeSet::new(),
            ),
            ViewId::Status
        );
    }

    #[test]
    fn resolve_active_view_preserves_existing_active_thread() {
        let mut room_view = RoomViewModel::default();
        room_view.threads.insert(
            "$thread-root".to_owned(),
            ThreadViewModel {
                label: "root msg".to_owned(),
            },
        );
        let model = UiModel {
            rooms: vec![room("!alpha:example", "Alpha", false)],
            room_views: BTreeMap::from([("!alpha:example".to_owned(), room_view)]),
            ..UiModel::default()
        };

        assert_eq!(
            resolve_active_view(
                &ViewId::Thread {
                    room_id: "!alpha:example".to_owned(),
                    thread_root_event_id: "$thread-root".to_owned(),
                },
                Some("!alpha:example"),
                &model,
                &BTreeSet::new(),
            ),
            ViewId::Thread {
                room_id: "!alpha:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            }
        );
    }

    #[test]
    fn resolve_active_view_falls_back_to_parent_room_when_active_thread_is_stale() {
        let model = UiModel {
            rooms: vec![room("!alpha:example", "Alpha", false)],
            ..UiModel::default()
        };

        assert_eq!(
            resolve_active_view(
                &ViewId::Thread {
                    room_id: "!alpha:example".to_owned(),
                    thread_root_event_id: "$thread-root".to_owned(),
                },
                Some("!alpha:example"),
                &model,
                &BTreeSet::new(),
            ),
            ViewId::Room("!alpha:example".to_owned())
        );
    }

    #[test]
    fn synced_rooms_preserves_only_valid_selection() {
        let mut state = test_state();
        state.model.selected_room = Some("!keep:example".to_owned());

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!keep:example", "Keep", false),
                room("!other:example", "Other", false),
            ]),
        )
        .unwrap();
        assert_eq!(state.model.selected_room, Some("!keep:example".to_owned()));

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!other:example", "Other", false)]),
        )
        .unwrap();
        assert_eq!(state.model.selected_room, None);
    }

    #[test]
    fn synced_rooms_clears_typing_for_non_actionable_rooms() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            typing_event("!keep:example", &["@alice:example.org"]),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            typing_event("!drop:example", &["@bob:example.org"]),
        )
        .unwrap();

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!keep:example", "Keep", false)]),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!keep:example")
                .map(|view| view.typing_user_ids.clone()),
            Some(vec!["@alice:example.org".to_owned()])
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!drop:example")
                .map(|view| view.typing_user_ids.clone()),
            Some(Vec::new())
        );
    }

    #[test]
    fn synced_rooms_clear_selected_room_when_room_becomes_space() {
        let mut state = test_state();
        state.model.selected_room = Some("!room:example".to_owned());

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![space("!room:example", "Room Space")]),
        )
        .unwrap();

        assert_eq!(state.model.selected_room, None);
    }

    #[test]
    fn logged_out_clears_selection_timelines_and_diagnostics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.model.selected_room = Some("!room:example".to_owned());
        state.active_view = ViewId::Room("!room:example".to_owned());
        state.model.room_views.insert(
            "!room:example".to_owned(),
            RoomViewModel {
                topic: Some("incident review".to_owned()),
                activities: vec![sent_message_activity(
                    "!room:example",
                    "$!room:example:hello",
                    None,
                    "hello",
                    1_700_000_000_000,
                )],
                reactions_by_target: BTreeMap::new(),
                threads: BTreeMap::new(),
                preview_media: BTreeMap::new(),
                reply_targets: BTreeMap::new(),
                edited_event_ids: BTreeSet::new(),
                typing_user_ids: vec!["@alice:example.org".to_owned()],
                members: Vec::new(),
                history: RoomHistoryBackfillState::default(),
                echo: EchoLog::default(),
            },
        );
        state.model.last_activity = Some(LastMessageActivity {
            room_id: "!room:example".to_owned(),
            event_id: Some("$!room:example:hello".to_owned()),
            transaction_id: None,
            sender: "@alice:example".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            presentation: RoomMessagePresentationKind::Text,
            body: "hello".to_owned(),
            send_state: RoomMessageSendState::Sent,
        });
        state.model.last_e2ee_status = Some(e2ee_status(
            E2eeRecoveryState::Enabled,
            E2eeBackupState::Enabled,
            true,
            Some(true),
            Some(true),
        ));
        state.model.last_verification = Some(VerificationStateEvent {
            flow_id: "flow".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::Requested,
        });
        state.model.last_error = Some(CoreErrorEvent {
            operation: Operation::SendMessage,
            room_id: Some("!room:example".to_owned()),
            message: "failed".to_owned(),
        });
        state.find_states.insert(
            ViewId::Room("!room:example".to_owned()),
            build_view_find_state("hello", "hello", Vec::new(), None),
        );
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(ViewId::Room("!room:example".to_owned()), tracked_view(20));
        state.hwnd_views.insert(10, ViewId::Status);
        state
            .hwnd_views
            .insert(20, ViewId::Room("!room:example".to_owned()));
        let words = vec![HighlightWord {
            word: "deploy".to_owned(),
            highlights: true,
        }];
        apply_core_event(&mut state, Event::HighlightWords(words.clone()))
            .expect("highlight words apply");
        assert_eq!(state.model.highlight_words, Some(words));

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut)
                .expect("logged out event must apply successfully");
        });
        let removed = take_stale_room_views(&mut state);

        assert!(state.model.rooms.is_empty());
        assert_eq!(state.model.highlight_words, None);
        assert_eq!(state.model.selected_room, None);
        assert!(state.model.room_views.is_empty());
        assert_eq!(state.model.last_activity, None);
        assert_eq!(state.model.last_e2ee_status, None);
        assert_eq!(state.model.last_verification, None);
        assert_eq!(state.model.last_error, None);
        assert!(state.find_states.is_empty());
        assert_eq!(state.active_view, ViewId::Status);
        assert_eq!(removed.len(), 1);
        assert!(state.views.contains_key(&ViewId::Status));
        assert!(
            !state
                .views
                .contains_key(&ViewId::Room("!room:example".to_owned()))
        );
    }

    #[test]
    fn logged_out_clears_last_e2ee_status() {
        let mut state = test_state();
        state.model.last_e2ee_status = Some(e2ee_status(
            E2eeRecoveryState::Incomplete,
            E2eeBackupState::Enabled,
            true,
            Some(true),
            Some(false),
        ));

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut)
                .expect("logged out event must apply successfully");
        });

        assert_eq!(state.model.last_e2ee_status, None);
    }

    #[test]
    fn ordered_view_ids_follow_status_then_room_model_order_then_stale_views() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!beta:example", "Beta", false),
            room("!alpha:example", "Alpha", false),
        ];
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(ViewId::Room("!alpha:example".to_owned()), tracked_view(20));
        state
            .views
            .insert(ViewId::Room("!orphan:example".to_owned()), tracked_view(30));
        state
            .views
            .insert(ViewId::Room("!beta:example".to_owned()), tracked_view(40));

        assert_eq!(
            ordered_view_ids(&state),
            vec![
                ViewId::Status,
                ViewId::Room("!beta:example".to_owned()),
                ViewId::Room("!alpha:example".to_owned()),
                ViewId::Room("!orphan:example".to_owned()),
            ]
        );
    }

    fn space_with_children(room_id: &str, display_name: &str, children: &[&str]) -> RoomSummary {
        let mut space = space(room_id, display_name);
        space.space_children = children
            .iter()
            .map(|child_id| mirx_core::SpaceChildSummary {
                room_id: (*child_id).to_owned(),
                suggested: false,
                order: None,
            })
            .collect();
        space
    }

    fn nav_folder(label: &str, children: Vec<NavItem>) -> NavItem {
        NavItem {
            label: label.to_owned(),
            view: None,
            children,
        }
    }

    #[test]
    fn navigation_layout_roots_at_status_labelled_with_the_user_id() {
        let mut state = test_state();
        assert_eq!(
            navigation_layout(&state.model),
            nav_view("Status", ViewId::Status, Vec::new())
        );

        state.model.own_user_id = Some("@me:example".to_owned());
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        assert_eq!(
            navigation_layout(&state.model),
            nav_view(
                "@me:example",
                ViewId::Status,
                vec![nav_room("Ops", "!ops:example")]
            )
        );
    }

    #[test]
    fn navigation_layout_lists_channels_then_queries_then_invites() {
        let mut state = test_state();
        state.model.rooms = vec![
            invited_room("!invite:example", "Lobby"),
            query("!alice:example", "Alice", "@alice:example"),
            room("!ops:example", "Ops", true),
            query("!bob:example", "Bob", "@bob:example"),
            room("!random:example", "Random", false),
        ];

        assert_eq!(
            navigation_layout(&state.model),
            nav_view(
                "Status",
                ViewId::Status,
                vec![
                    nav_room("Ops", "!ops:example"),
                    nav_room("Random", "!random:example"),
                    nav_room("Alice", "!alice:example"),
                    nav_room("Bob", "!bob:example"),
                    nav_room("Lobby [invite]", "!invite:example"),
                ]
            )
        );
    }

    #[test]
    fn navigation_layout_nests_rooms_in_space_folders_and_lists_unspaced_after() {
        let mut state = test_state();
        let mut invited_space = space_with_children("!invited-space:example", "Invited", &[]);
        invited_space.membership = RoomMembership::Invite;
        state.model.rooms = vec![
            query("!direct:example", "Direct", "@alice:example"),
            room("!lobby:example", "Lobby", false),
            space_with_children(
                "!work:example",
                "Work",
                &["!ops:example", "!team:example", "!missing:example"],
            ),
            space_with_children(
                "!team:example",
                "Team",
                &["!shared:example", "!team:example"],
            ),
            space_with_children(
                "!social:example",
                "Social",
                &["!shared:example", "!dm:example"],
            ),
            // Two spaces containing only each other, reachable from no root.
            space_with_children("!loop-a:example", "Loop A", &["!loop-b:example"]),
            space_with_children("!loop-b:example", "Loop B", &["!loop-a:example"]),
            room("!ops:example", "Ops", true),
            room("!shared:example", "Shared", false),
            query("!dm:example", "Dm", "@bob:example"),
            invited_space,
        ];

        assert_eq!(
            navigation_layout(&state.model),
            nav_view(
                "Status",
                ViewId::Status,
                vec![
                    nav_folder(
                        "Work",
                        vec![
                            nav_room("Ops", "!ops:example"),
                            nav_folder(
                                "Team",
                                vec![
                                    nav_room("Shared", "!shared:example"),
                                    // A space containing itself ends in an empty folder.
                                    nav_folder("Team", Vec::new()),
                                ]
                            ),
                        ]
                    ),
                    nav_folder(
                        "Social",
                        vec![
                            nav_room("Shared", "!shared:example"),
                            // Rooms under a space stay there whatever their kind.
                            nav_room("Dm", "!dm:example"),
                        ]
                    ),
                    nav_folder(
                        "Loop A",
                        vec![nav_folder("Loop B", vec![nav_folder("Loop A", Vec::new())])]
                    ),
                    nav_room("Lobby", "!lobby:example"),
                    nav_room("Direct", "!direct:example"),
                ]
            )
        );
    }

    #[test]
    fn navigation_layout_puts_threads_under_their_room() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        state.model.room_views.insert(
            "!ops:example".to_owned(),
            RoomViewModel {
                threads: BTreeMap::from([(
                    "$root".to_owned(),
                    ThreadViewModel {
                        label: "root msg".to_owned(),
                    },
                )]),
                ..RoomViewModel::default()
            },
        );

        assert_eq!(
            navigation_layout(&state.model),
            nav_view(
                "Status",
                ViewId::Status,
                vec![nav_view(
                    "Ops",
                    room_view_id("!ops:example"),
                    vec![nav_view(
                        "root msg",
                        thread_view_id("!ops:example", "$root"),
                        Vec::new()
                    )]
                )]
            )
        );
    }

    #[test]
    fn ordered_view_ids_group_channels_before_queries() {
        let mut state = test_state();
        state.model.rooms = vec![
            invited_room("!invite:example", "Lobby"),
            query("!alice:example", "Alice", "@alice:example"),
            room("!ops:example", "Ops", false),
        ];
        state.model.room_views.insert(
            "!alice:example".to_owned(),
            RoomViewModel {
                threads: BTreeMap::from([(
                    "$root".to_owned(),
                    ThreadViewModel {
                        label: "root msg".to_owned(),
                    },
                )]),
                ..RoomViewModel::default()
            },
        );
        for (index, view_id) in [
            ViewId::Status,
            room_view_id("!invite:example"),
            room_view_id("!alice:example"),
            thread_view_id("!alice:example", "$root"),
            room_view_id("!ops:example"),
        ]
        .into_iter()
        .enumerate()
        {
            state
                .views
                .insert(view_id, tracked_view(10 * (index as isize + 1)));
        }

        assert_eq!(
            ordered_view_ids(&state),
            vec![
                ViewId::Status,
                room_view_id("!ops:example"),
                room_view_id("!alice:example"),
                thread_view_id("!alice:example", "$root"),
                room_view_id("!invite:example"),
            ]
        );
    }

    #[test]
    fn ordered_view_ids_exclude_spaces_from_model_order() {
        let mut state = test_state();
        state.model.rooms = vec![
            space("!space:example", "Space"),
            room("!beta:example", "Beta", false),
            room("!alpha:example", "Alpha", false),
        ];
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(ViewId::Room("!space:example".to_owned()), tracked_view(20));
        state
            .views
            .insert(ViewId::Room("!alpha:example".to_owned()), tracked_view(30));
        state
            .views
            .insert(ViewId::Room("!beta:example".to_owned()), tracked_view(40));

        assert_eq!(
            ordered_view_ids(&state),
            vec![
                ViewId::Status,
                ViewId::Room("!beta:example".to_owned()),
                ViewId::Room("!alpha:example".to_owned()),
                ViewId::Room("!space:example".to_owned()),
            ]
        );
    }

    #[test]
    fn view_identity_round_trips_thread_view_id() {
        let view_id = ViewId::Thread {
            room_id: "!ops:example".to_owned(),
            thread_root_event_id: "$thread-root".to_owned(),
        };

        assert_eq!(
            view_identity_from_view_id(&view_id),
            ViewIdentity::Thread {
                room_id: "!ops:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            }
        );
        assert_eq!(
            view_id_from_identity(&ViewIdentity::Thread {
                room_id: "!ops:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            }),
            view_id
        );
    }

    #[test]
    fn thread_labels_use_observed_root_body_when_available_and_fall_back_to_root_event_id() {
        let mut room_view = RoomViewModel::default();

        room_view.apply_activity(sent_message_activity_with_relation_and_presentation(
            "!ops:example",
            "$thread-1",
            None,
            Some(thread_relation("$thread-root")),
            RoomMessagePresentationKind::Text,
            "thread rep",
            1_700_000_120_000,
        ));
        assert_eq!(
            room_view
                .threads
                .get("$thread-root")
                .map(|thread| thread.label.as_str()),
            Some("$thread-root")
        );

        room_view.apply_activity(sent_message_activity(
            "!ops:example",
            "$thread-root",
            None,
            "root msg",
            1_700_000_060_000,
        ));
        assert_eq!(
            room_view
                .threads
                .get("$thread-root")
                .map(|thread| thread.label.as_str()),
            Some("root msg")
        );

        let mut room_view = RoomViewModel::default();
        room_view.apply_activity(sent_message_activity(
            "!ops:example",
            "$thread-root",
            None,
            "root msg",
            1_700_000_060_000,
        ));
        room_view.apply_activity(sent_message_activity_with_relation_and_presentation(
            "!ops:example",
            "$thread-2",
            None,
            Some(thread_relation("$thread-root")),
            RoomMessagePresentationKind::Text,
            "thread rep",
            1_700_000_120_000,
        ));
        assert_eq!(
            room_view
                .threads
                .get("$thread-root")
                .map(|thread| thread.label.as_str()),
            Some("root msg")
        );
    }

    #[test]
    fn replacing_thread_root_updates_thread_label() {
        let mut room_view = RoomViewModel::default();
        room_view.apply_activity(sent_message_activity(
            "!ops:example",
            "$thread-root",
            None,
            "old root",
            1_700_000_060_000,
        ));
        room_view.apply_activity(sent_message_activity_with_relation_and_presentation(
            "!ops:example",
            "$thread-1",
            None,
            Some(thread_relation("$thread-root")),
            RoomMessagePresentationKind::Text,
            "thread rep",
            1_700_000_120_000,
        ));

        assert!(room_view.replace_message(
            "$thread-root",
            MessageReplacementFields {
                presentation: RoomMessagePresentationKind::Text,
                body: "edited root".to_owned(),
                styles: Vec::new(),
            }
        ));
        assert_eq!(
            room_view
                .threads
                .get("$thread-root")
                .map(|thread| thread.label.as_str()),
            Some("edited root")
        );
    }

    #[test]
    fn redaction_thread_root_keeps_thread_reachable_and_labels_it_message_redacted() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "old root",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "thread rep"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$thread-root", "$redaction:example.org"),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        assert_eq!(
            room_view
                .threads
                .get("$thread-root")
                .map(|thread| thread.label.as_str()),
            Some("Message redacted")
        );

        let lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
            state.timestamps(),
        ));
        let rendered = lines.join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("old root"));
    }

    #[test]
    fn thread_timeline_filters_room_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(&mut state, timeline("!ops:example", "lobby")).unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$reply-1",
                None,
                Some(reply_relation("$lobby-root")),
                RoomMessagePresentationKind::Text,
                "plain",
                1_700_000_090_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "thread rep"),
        )
        .unwrap();

        let lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
            state.timestamps(),
        ));

        assert!(
            lines
                .iter()
                .any(|line| line.contains("* Now talking in Ops (!ops:example)"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("* Topic is 'incident review'"))
        );
        assert!(lines.iter().any(|line| line.contains("root msg")));
        assert!(lines.iter().any(|line| line.contains("thread rep")));
        assert!(!lines.iter().any(|line| line.contains("lobby")));
        assert!(!lines.iter().any(|line| line.contains("plain")));
    }

    #[test]
    fn replacing_threaded_reply_updates_room_and_thread_timeline_projections() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "old reply"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$thread-1",
                "$thread-edit:example.org",
                RoomMessagePresentationKind::Text,
                "edited reply",
            ),
        )
        .unwrap();

        assert!(
            render_room_lines(&state, "!ops:example")
                .iter()
                .any(|line| line.contains("edited reply"))
        );
        let lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
            state.timestamps(),
        ));
        assert!(lines.iter().any(|line| line.contains("edited reply")));
        assert!(!lines.iter().any(|line| line.contains("old reply")));
    }

    #[test]
    fn redaction_threaded_reply_updates_room_and_thread_timeline_projections() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "old reply"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$thread-1", "$redaction:example.org"),
        )
        .unwrap();

        let room_rendered = render_room_lines(&state, "!ops:example").join("\n");
        assert!(room_rendered.contains("message redacted"));
        assert!(!room_rendered.contains("old reply"));

        let thread_lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
            state.timestamps(),
        ));
        let thread_rendered = thread_lines.join("\n");
        assert!(thread_rendered.contains("message redacted"));
        assert!(!thread_rendered.contains("old reply"));
    }

    #[test]
    fn latest_durable_thread_event_id_falls_back_to_root_when_root_is_only_durable_event() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root:example",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();

        assert_eq!(
            latest_durable_thread_event_id(&state.model, "!ops:example", "$thread-root:example"),
            "$thread-root:example"
        );
    }

    #[test]
    fn latest_durable_thread_event_id_ignores_newer_pending_local_thread_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root:example",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$thread-1:example",
                None,
                Some(thread_relation("$thread-root:example")),
                RoomMessagePresentationKind::Text,
                "durable reply",
                1_700_000_120_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued_in_thread("!ops:example", "thread-pending-1", "$thread-root:example"),
        )
        .unwrap();

        assert_eq!(
            latest_durable_thread_event_id(&state.model, "!ops:example", "$thread-root:example"),
            "$thread-1:example"
        );
    }

    #[test]
    fn ordered_view_ids_place_threads_after_parent_rooms() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!alpha:example", "Alpha", false),
            room("!beta:example", "Beta", false),
        ];
        state.model.room_views.insert(
            "!alpha:example".to_owned(),
            RoomViewModel {
                threads: BTreeMap::from([(
                    "$thread-root".to_owned(),
                    ThreadViewModel {
                        label: "root msg".to_owned(),
                    },
                )]),
                ..RoomViewModel::default()
            },
        );
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(ViewId::Room("!alpha:example".to_owned()), tracked_view(20));
        state.views.insert(
            ViewId::Thread {
                room_id: "!alpha:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            },
            tracked_view(30),
        );
        state
            .views
            .insert(ViewId::Room("!beta:example".to_owned()), tracked_view(40));

        assert_eq!(
            ordered_view_ids(&state),
            vec![
                ViewId::Status,
                ViewId::Room("!alpha:example".to_owned()),
                ViewId::Thread {
                    room_id: "!alpha:example".to_owned(),
                    thread_root_event_id: "$thread-root".to_owned(),
                },
                ViewId::Room("!beta:example".to_owned()),
            ]
        );
    }

    #[test]
    fn stale_thread_cleanup_falls_back_to_parent_room_when_possible() {
        let mut state = test_state();
        state.model.rooms = vec![room("!alpha:example", "Alpha", false)];
        state.model.selected_room = Some("!alpha:example".to_owned());
        state.active_view = ViewId::Thread {
            room_id: "!alpha:example".to_owned(),
            thread_root_event_id: "$thread-root".to_owned(),
        };
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(ViewId::Room("!alpha:example".to_owned()), tracked_view(20));
        state.views.insert(
            ViewId::Thread {
                room_id: "!alpha:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            },
            tracked_view(30),
        );
        state.hwnd_views.insert(10, ViewId::Status);
        state
            .hwnd_views
            .insert(20, ViewId::Room("!alpha:example".to_owned()));
        state.hwnd_views.insert(
            30,
            ViewId::Thread {
                room_id: "!alpha:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            },
        );

        let removed = take_stale_room_views(&mut state);

        assert_eq!(removed.len(), 1);
        assert_eq!(state.active_view, ViewId::Room("!alpha:example".to_owned()));
        assert!(!state.views.contains_key(&ViewId::Thread {
            room_id: "!alpha:example".to_owned(),
            thread_root_event_id: "$thread-root".to_owned(),
        }));
    }

    #[test]
    fn stale_thread_cleanup_removes_threads_when_room_disappears() {
        let mut state = test_state();
        state.model.selected_room = Some("!alpha:example".to_owned());
        state.active_view = ViewId::Thread {
            room_id: "!alpha:example".to_owned(),
            thread_root_event_id: "$thread-root".to_owned(),
        };
        state.views.insert(ViewId::Status, tracked_view(10));
        state.views.insert(
            ViewId::Thread {
                room_id: "!alpha:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            },
            tracked_view(20),
        );
        state.hwnd_views.insert(10, ViewId::Status);
        state.hwnd_views.insert(
            20,
            ViewId::Thread {
                room_id: "!alpha:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            },
        );

        let removed = take_stale_room_views(&mut state);

        assert_eq!(removed.len(), 1);
        assert_eq!(state.active_view, ViewId::Status);
        assert!(state.views.contains_key(&ViewId::Status));
    }

    #[test]
    fn switchbar_button_text_includes_room_read_suffix_without_changing_other_titles() {
        let mut state = test_state();
        let mut direct_room = room("!room:example", "Room", true);
        direct_room.is_direct = true;
        direct_room.direct_user_ids = vec!["@alice:example".to_owned()];
        state.model.rooms = vec![direct_room];
        state.model.room_read_states.insert(
            "!room:example".to_owned(),
            RoomReadState {
                room_id: "!room:example".to_owned(),
                latest_active_event_id: Some("$latest".to_owned()),
                fully_read_event_id: Some("$fully-read".to_owned()),
                marked_unread: true,
                num_unread: 3,
                num_notifications: 2,
                num_mentions: 1,
            },
        );
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);
        state.model.room_views.insert(
            "!room:example".to_owned(),
            RoomViewModel {
                threads: BTreeMap::from([(
                    "$thread-root".to_owned(),
                    ThreadViewModel {
                        label: "root msg".to_owned(),
                    },
                )]),
                ..RoomViewModel::default()
            },
        );
        state.room_activity.insert(
            thread_view_id("!room:example", "$thread-root"),
            ViewActivityLevel::Message,
        );

        assert_eq!(
            view_short_title(&state.model, &room_view_id("!room:example")),
            "Room"
        );
        assert_eq!(
            switchbar_button_text(&state, &room_view_id("!room:example")),
            "Room [@1 !2 *3 M]"
        );
        assert_eq!(
            switchbar_button_text(&state, &thread_view_id("!room:example", "$thread-root")),
            "Thread: root msg"
        );
        assert_eq!(switchbar_button_text(&state, &ViewId::Status), "Status");
        state.model.own_user_id = Some("@me:example".to_owned());
        assert_eq!(
            switchbar_button_text(&state, &ViewId::Status),
            "@me:example"
        );
    }

    #[test]
    fn view_caption_adds_the_room_topic_to_the_short_title() {
        let mut state = test_state();
        let mut invited_room = room("!invite:example", "Invite", false);
        invited_room.membership = RoomMembership::Invite;
        state.model.rooms = vec![
            room("!room:example", "Room", true),
            invited_room,
            room("!plain:example", "Plain", false),
        ];
        for (room_id, topic) in [
            ("!room:example", "pairing\nnotes"),
            ("!invite:example", "come join"),
        ] {
            state.model.room_views.insert(
                room_id.to_owned(),
                RoomViewModel {
                    topic: Some(topic.to_owned()),
                    threads: BTreeMap::from([(
                        "$thread-root".to_owned(),
                        ThreadViewModel {
                            label: "root msg".to_owned(),
                        },
                    )]),
                    ..RoomViewModel::default()
                },
            );
        }

        assert_eq!(
            view_caption(&state.model, &room_view_id("!room:example")),
            "Room: pairing notes"
        );
        assert_eq!(
            view_caption(&state.model, &room_view_id("!invite:example")),
            format!(
                "{}: come join",
                view_short_title(&state.model, &room_view_id("!invite:example"))
            )
        );
        assert_eq!(
            view_caption(&state.model, &room_view_id("!plain:example")),
            "Plain"
        );
        assert_eq!(view_caption(&state.model, &ViewId::Status), "Status");
        state.model.own_user_id = Some("@me:example".to_owned());
        assert_eq!(
            view_caption(&state.model, &ViewId::Status),
            "Status: @me:example"
        );
        assert_eq!(
            view_caption(
                &state.model,
                &thread_view_id("!room:example", "$thread-root")
            ),
            "Thread: root msg"
        );
    }

    #[test]
    fn view_caption_names_a_query_by_user_id() {
        let mut state = test_state();
        let mut query = room("!alice:example", "Alice", true);
        query.is_direct = true;
        query.direct_user_ids = vec!["@alice:example".to_owned()];
        let mut group_query = room("!group:example", "Group", false);
        group_query.is_direct = true;
        group_query.direct_user_ids = vec!["@alice:example".to_owned(), "@bob:example".to_owned()];
        state.model.rooms = vec![query, group_query];
        for room_id in ["!alice:example", "!group:example"] {
            state.model.room_views.insert(
                room_id.to_owned(),
                RoomViewModel {
                    topic: Some("ignored".to_owned()),
                    ..RoomViewModel::default()
                },
            );
        }

        assert_eq!(
            view_caption(&state.model, &room_view_id("!alice:example")),
            "Alice (@alice:example)"
        );
        assert_eq!(
            view_caption(&state.model, &room_view_id("!group:example")),
            "Group"
        );
    }

    #[test]
    fn close_action_follows_mirc_per_window_kind() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!ops:example", "Ops", false),
            query("!dm:example", "Alice", "@alice:example"),
            invited_room("!invite:example", "Invite"),
        ];

        assert_eq!(
            close_action(&state.model, &ViewId::Status),
            CloseAction::Refuse
        );
        assert_eq!(
            close_action(&state.model, &thread_view_id("!ops:example", "$root")),
            CloseAction::Destroy
        );
        assert_eq!(
            close_action(&state.model, &room_view_id("!ops:example")),
            CloseAction::Confirm {
                room_id: "!ops:example".to_owned(),
                leave: CloseLeave::Part,
            }
        );
        assert_eq!(
            close_action(&state.model, &room_view_id("!dm:example")),
            CloseAction::HideQuery("!dm:example".to_owned())
        );
        assert_eq!(
            close_action(&state.model, &room_view_id("!invite:example")),
            CloseAction::Confirm {
                room_id: "!invite:example".to_owned(),
                leave: CloseLeave::RejectInvite,
            }
        );
    }

    #[test]
    fn close_confirmation_text_names_the_room() {
        assert_eq!(
            close_confirmation_text(CloseLeave::Part, &room("!ops:example", "Ops", false)),
            "Leave Ops?\n\nYou may need a new invite to rejoin."
        );
        assert_eq!(
            close_confirmation_text(
                CloseLeave::RejectInvite,
                &invited_room("!invite:example", "Invite")
            ),
            "Reject the invite to Invite?"
        );
    }

    #[test]
    fn closed_query_reopens_on_message_activity_from_others() {
        let mut state = test_state();
        state.model.rooms = vec![query("!dm:example", "Alice", "@alice:example")];
        state.model.own_user_id = Some("@me:example".to_owned());
        state.closed_queries.insert("!dm:example".to_owned());
        let message_from = |event_id: &str, user_id: &str| {
            let mut activity = sent_message_activity("!dm:example", event_id, None, "hi", 1);
            let RoomActivityKind::Message(message) = &mut activity.kind else {
                unreachable!("a sent message activity is a message");
            };
            message.sender = user_profile(user_id);
            activity
        };

        apply_room_activity_event(&mut state, message_from("$own", "@me:example"));
        apply_core_event(&mut state, topic_state("!dm:example", "new topic")).unwrap();
        assert!(state.closed_queries.contains("!dm:example"));

        apply_room_activity_event(&mut state, message_from("$theirs", "@alice:example"));
        assert!(state.closed_queries.is_empty());
    }

    fn room_joined(room_id: &str) -> Event {
        Event::RoomJoined(mirx_core::RoomJoined {
            room_id: room_id.to_owned(),
        })
    }

    #[test]
    fn room_joined_makes_the_joined_room_the_active_view() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!ops:example", "Ops", false),
            room("!new:example", "New", false),
        ];
        state.active_view = room_view_id("!ops:example");
        state.model.selected_room = Some("!ops:example".to_owned());

        apply_core_event(&mut state, room_joined("!new:example")).unwrap();

        assert_eq!(state.active_view, room_view_id("!new:example"));
        assert_eq!(state.model.selected_room.as_deref(), Some("!new:example"));
        assert_eq!(
            resolve_active_view(
                &state.active_view,
                state.model.selected_room.as_deref(),
                &state.model,
                &state.closed_queries,
            ),
            room_view_id("!new:example"),
            "the view sync keeps the joined room active"
        );
    }

    #[test]
    fn room_joined_reopens_a_closed_query() {
        let mut state = test_state();
        state.model.rooms = vec![query("!dm:example", "Alice", "@alice:example")];
        state.closed_queries.insert("!dm:example".to_owned());
        state.active_view = ViewId::Status;

        apply_core_event(&mut state, room_joined("!dm:example")).unwrap();

        assert!(state.closed_queries.is_empty());
        assert_eq!(state.active_view, room_view_id("!dm:example"));
    }

    #[test]
    fn room_joined_for_a_space_logs_it_and_keeps_the_active_view() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!ops:example", "Ops", false),
            space("!space:example", "Rust Space"),
        ];
        state.active_view = room_view_id("!ops:example");

        apply_core_event(&mut state, room_joined("!space:example")).unwrap();

        assert_eq!(state.active_view, room_view_id("!ops:example"));
        assert_eq!(
            state
                .model
                .status_log
                .lines()
                .last()
                .map(|line| line.text.as_str()),
            Some("* Joined space Rust Space")
        );
    }

    #[test]
    fn room_joined_for_a_room_missing_from_the_room_list_is_an_error() {
        let mut state = test_state();

        assert_eq!(
            apply_core_event(&mut state, room_joined("!gone:example")),
            Err("joined room !gone:example is not in the room list".to_owned())
        );
    }

    fn listing_rooms(filter: Option<&str>) -> RoomListState {
        RoomListState::new(
            RoomListQuery {
                server: None,
                filter: filter.map(str::to_owned),
            },
            "example",
        )
    }

    fn public_rooms_page(
        request: ListPublicRoomsRequest,
        room_ids: &[&str],
        next_batch: Option<&str>,
    ) -> Event {
        Event::PublicRooms(PublicRoomsPage {
            request,
            result: PublicRoomsResult::Loaded {
                rooms: room_ids
                    .iter()
                    .map(|room_id| mirx_core::PublicRoom {
                        room_id: (*room_id).to_owned(),
                        canonical_alias: None,
                        name: None,
                        topic: None,
                        joined_members: 1,
                    })
                    .collect(),
                next_batch: next_batch.map(str::to_owned),
                total_estimate: None,
            },
        })
    }

    #[test]
    fn public_rooms_page_fills_the_listing_and_asks_for_the_next_page() {
        let (mut state, handle) = test_state_with_command_sender();
        let room_list = listing_rooms(Some("rust"));
        let first = room_list.first_request();
        state.model.room_list = Some(room_list);

        apply_core_event(
            &mut state,
            public_rooms_page(first, &["!a:example", "!b:example"], Some("page2")),
        )
        .unwrap();

        assert_eq!(state.model.room_list.as_ref().unwrap().len(), 2);
        assert_eq!(
            state.recorded_commands,
            vec![Command::ListPublicRooms(ListPublicRoomsRequest {
                server: None,
                filter: Some("rust".to_owned()),
                since: Some("page2".to_owned()),
                limit: room_list::ROOM_LIST_PAGE_SIZE,
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_room_list_requests,
            vec![DumpSubmittedRoomListRequest {
                server: None,
                filter: Some("rust".to_owned()),
                since: Some("page2".to_owned()),
                limit: room_list::ROOM_LIST_PAGE_SIZE,
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn public_rooms_page_after_the_listing_closed_is_dropped() {
        let mut state = test_state();
        let first = listing_rooms(None).first_request();

        apply_core_event(
            &mut state,
            public_rooms_page(first, &["!a:example"], Some("page2")),
        )
        .unwrap();

        assert!(state.model.room_list.is_none());
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn failed_public_rooms_page_logs_the_error_to_status() {
        let mut state = test_state();
        let room_list = listing_rooms(None);
        let request = room_list.first_request();
        state.model.room_list = Some(room_list);

        apply_core_event(
            &mut state,
            Event::PublicRooms(PublicRoomsPage {
                request,
                result: PublicRoomsResult::Failed {
                    message: "failed to list public rooms: forbidden".to_owned(),
                },
            }),
        )
        .unwrap();

        assert!(state.recorded_commands.is_empty());
        assert_eq!(
            state
                .model
                .status_log
                .lines()
                .last()
                .map(|line| line.text.as_str()),
            Some("* /list: failed to list public rooms: forbidden")
        );
        assert_eq!(
            view_caption(&state.model, &ViewId::RoomList),
            "Rooms List [example] - failed"
        );
    }

    #[test]
    fn the_rooms_list_view_exists_while_its_listing_does() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        let resolve = |state: &WindowState| {
            resolve_active_view(&ViewId::RoomList, None, &state.model, &state.closed_queries)
        };
        assert!(!view_id_is_valid(&state.model, &ViewId::RoomList));
        assert_eq!(resolve(&state), room_view_id("!ops:example"));
        assert_eq!(
            navigation_layout(&state.model),
            nav_view(
                "Status",
                ViewId::Status,
                vec![nav_room("Ops", "!ops:example")]
            )
        );

        state.model.room_list = Some(listing_rooms(None));

        assert!(view_id_is_valid(&state.model, &ViewId::RoomList));
        assert_eq!(resolve(&state), ViewId::RoomList);
        assert_eq!(
            navigation_layout(&state.model),
            nav_view(
                "Status",
                ViewId::Status,
                vec![
                    nav_room("Ops", "!ops:example"),
                    nav_view("Rooms List", ViewId::RoomList, Vec::new()),
                ]
            )
        );
        state.views.insert(ViewId::RoomList, tracked_view(30));
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(room_view_id("!ops:example"), tracked_view(20));
        assert_eq!(
            ordered_view_ids(&state),
            vec![
                ViewId::Status,
                room_view_id("!ops:example"),
                ViewId::RoomList
            ]
        );
    }

    #[test]
    fn logging_out_drops_the_rooms_list() {
        let mut state = test_state();
        state.model.room_list = Some(listing_rooms(None));
        state.views.insert(ViewId::Status, tracked_view(10));
        state.views.insert(ViewId::RoomList, tracked_view(20));
        state.hwnd_views.insert(10, ViewId::Status);
        state.hwnd_views.insert(20, ViewId::RoomList);

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut).unwrap();
        });
        let removed = take_stale_room_views(&mut state);

        assert!(state.model.room_list.is_none());
        assert_eq!(removed.len(), 1);
        assert!(!state.views.contains_key(&ViewId::RoomList));
    }

    #[test]
    fn closed_queries_drop_rooms_that_stop_being_queries() {
        let mut state = test_state();
        state.closed_queries = BTreeSet::from([
            "!dm:example".to_owned(),
            "!gone:example".to_owned(),
            "!now-channel:example".to_owned(),
        ]);

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                query("!dm:example", "Alice", "@alice:example"),
                room("!now-channel:example", "Channel", false),
            ]),
        )
        .unwrap();

        assert_eq!(
            state.closed_queries,
            BTreeSet::from(["!dm:example".to_owned()])
        );
    }

    #[test]
    fn resolve_active_view_skips_closed_queries_when_falling_back() {
        let model = UiModel {
            rooms: vec![
                query("!dm:example", "Alice", "@alice:example"),
                room("!ops:example", "Ops", false),
            ],
            ..UiModel::default()
        };
        let closed = BTreeSet::from(["!dm:example".to_owned()]);

        assert_eq!(
            resolve_active_view(&room_view_id("!dm:example"), None, &model, &closed),
            room_view_id("!ops:example")
        );
        assert_eq!(
            resolve_active_view(
                &room_view_id("!gone:example"),
                Some("!dm:example"),
                &model,
                &closed
            ),
            room_view_id("!ops:example")
        );
        assert_eq!(
            resolve_active_view(&room_view_id("!dm:example"), None, &model, &BTreeSet::new()),
            room_view_id("!dm:example")
        );
    }

    #[test]
    fn navigation_layout_keeps_closed_queries() {
        let mut state = test_state();
        state.model.rooms = vec![query("!dm:example", "Alice", "@alice:example")];
        let open_layout = navigation_layout(&state.model);
        state.closed_queries.insert("!dm:example".to_owned());

        assert_eq!(navigation_layout(&state.model), open_layout);
        assert_eq!(
            open_layout,
            nav_view(
                "Status",
                ViewId::Status,
                vec![nav_room("Alice", "!dm:example")]
            )
        );
    }

    #[test]
    fn switchbar_button_state_tracks_only_the_checked_bit() {
        let mut state = test_state();
        let room_view = room_view_id("!room:example");

        state
            .room_activity
            .insert(room_view.clone(), ViewActivityLevel::Highlight);
        assert_eq!(
            switchbar_button_state(&state, &room_view),
            TBSTATE_ENABLED as u8
        );

        state.room_activity.clear();
        state.active_view = room_view.clone();
        assert_eq!(
            switchbar_button_state(&state, &room_view),
            (TBSTATE_ENABLED | TBSTATE_CHECKED) as u8
        );
    }

    fn switchbar_draw(stage: u32, command_id: usize) -> NMTBCUSTOMDRAW {
        let mut draw = NMTBCUSTOMDRAW::default();
        draw.nmcd.dwDrawStage = stage;
        draw.nmcd.dwItemSpec = command_id;
        draw.clrText = rgb(1, 2, 3);
        draw
    }

    #[test]
    fn switchbar_custom_draw_colors_buttons_by_activity_level() {
        let mut state = test_state();
        state.active_view = ViewId::Status;
        state.switchbar_views = vec![
            ViewId::Status,
            room_view_id("!event:example"),
            room_view_id("!message:example"),
            room_view_id("!highlight:example"),
        ];
        state.room_activity = BTreeMap::from([
            (room_view_id("!event:example"), ViewActivityLevel::Event),
            (room_view_id("!message:example"), ViewActivityLevel::Message),
            (
                room_view_id("!highlight:example"),
                ViewActivityLevel::Highlight,
            ),
        ]);

        let mut prepaint = switchbar_draw(CDDS_PREPAINT, 0);
        assert_eq!(
            switchbar_custom_draw(&state, &mut prepaint),
            CDRF_NOTIFYITEMDRAW as LRESULT
        );
        assert_eq!(prepaint.clrText, rgb(1, 2, 3));

        for (index, expected) in [rgb(1, 2, 3), MIRC_MAROON, MIRC_RED, MIRC_BLUE]
            .into_iter()
            .enumerate()
        {
            let mut draw = switchbar_draw(CDDS_ITEMPREPAINT, SWITCHBAR_COMMAND_FIRST + index);
            assert_eq!(switchbar_custom_draw(&state, &mut draw), CDRF_DEFAULT);
            assert_eq!(draw.clrText, expected, "button {index}");
        }
    }

    #[test]
    #[should_panic(expected = "the active view never records activity")]
    fn switchbar_custom_draw_rejects_activity_on_the_active_view() {
        let mut state = test_state();
        state.active_view = room_view_id("!room:example");
        state.switchbar_views = vec![room_view_id("!room:example")];
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);

        switchbar_custom_draw(
            &state,
            &mut switchbar_draw(CDDS_ITEMPREPAINT, SWITCHBAR_COMMAND_FIRST),
        );
    }

    #[test]
    fn tree_custom_draw_colors_mapped_items_and_leaves_group_labels() {
        let mut state = test_state();
        state.active_view = ViewId::Status;
        state.tree_items = BTreeMap::from([
            (11, room_view_id("!room:example")),
            (12, thread_view_id("!room:example", "$root")),
        ]);
        state.room_activity.insert(
            thread_view_id("!room:example", "$root"),
            ViewActivityLevel::Highlight,
        );

        let draw = |stage, item: usize| {
            let mut draw = NMTVCUSTOMDRAW::default();
            draw.nmcd.dwDrawStage = stage;
            draw.nmcd.dwItemSpec = item;
            draw.clrText = rgb(1, 2, 3);
            let reply = tree_custom_draw(&state, &mut draw);
            (reply, draw.clrText)
        };

        assert_eq!(
            draw(CDDS_PREPAINT, 0),
            (CDRF_NOTIFYITEMDRAW as LRESULT, rgb(1, 2, 3))
        );
        assert_eq!(draw(CDDS_ITEMPREPAINT, 10), (CDRF_DEFAULT, rgb(1, 2, 3)));
        assert_eq!(draw(CDDS_ITEMPREPAINT, 11), (CDRF_DEFAULT, rgb(1, 2, 3)));
        assert_eq!(draw(CDDS_ITEMPREPAINT, 12), (CDRF_DEFAULT, MIRC_BLUE));
    }

    #[test]
    fn highlighted_activity_builds_highlighted_rows_but_redactions_do_not() {
        let mut rows = Vec::new();
        let mention = highlighted(sent_message_activity(
            "!room:example",
            "$m",
            None,
            "ping",
            1,
        ));
        push_room_activity_row(
            &mut rows,
            &RoomViewModel::default(),
            &mention,
            utc_timestamps(),
        );
        let redacted = RoomActivity {
            kind: RoomActivityKind::Redacted(RoomRedactedActivity {
                event_id: "$m".to_owned(),
            }),
            ..mention.clone()
        };
        push_room_activity_row(
            &mut rows,
            &RoomViewModel::default(),
            &redacted,
            utc_timestamps(),
        );

        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: true,
                },
                TimelineRowKind::StateNotice { highlight: false },
            ]
        );
    }

    fn event_room_activity(event: Event) -> RoomActivity {
        let Event::RoomActivity(activity) = event else {
            panic!("expected a room activity event, got {event:?}");
        };
        *activity
    }

    #[test]
    fn activity_level_for_maps_every_kind_and_highlight_overrides() {
        let message = sent_message_activity("!room:example", "$m", None, "hi", 1);
        let image = image_activity("!room:example", "$i", "@bob:example", 1, "i", None, None);
        let file = file_activity("!room:example", "$f", "@bob:example", 1, "f", None, None);
        let audio = audio_activity("!room:example", "$a", "@bob:example", 1, "a", None, None);
        let video = event_room_activity(video_message("!room:example", "$v"));
        let unsupported = unsupported_message_activity(
            "!room:example",
            "$u",
            "@bob:example",
            1,
            "org.example.custom",
            "u",
        );
        let undecryptable = event_room_activity(unable_to_decrypt_activity(
            "!room:example",
            "$e",
            "@bob:example",
            1,
        ));
        let state_notice = event_room_activity(topic_state("!room:example", "topic"));
        let redacted = RoomActivity {
            kind: RoomActivityKind::Redacted(RoomRedactedActivity {
                event_id: "$r".to_owned(),
            }),
            ..message.clone()
        };

        for activity in [
            &message,
            &image,
            &file,
            &audio,
            &video,
            &unsupported,
            &undecryptable,
        ] {
            assert_eq!(
                activity_level_for(activity, None),
                Some(ViewActivityLevel::Message),
                "{activity:?}"
            );
        }
        for activity in [&state_notice, &redacted] {
            assert_eq!(
                activity_level_for(activity, None),
                Some(ViewActivityLevel::Event),
                "{activity:?}"
            );
        }
        for activity in [message, image, state_notice, redacted] {
            assert_eq!(
                activity_level_for(&highlighted(activity), None),
                Some(ViewActivityLevel::Highlight)
            );
        }
    }

    #[test]
    fn activity_level_for_ignores_our_own_lines() {
        let me = Some("@alice:example");
        let own_message = sent_message_activity("!room:example", "$m", None, "hi", 1);
        let own_pending = pending_message_activity("!room:example", "txn", "draft");
        let own_image = image_activity("!room:example", "$i", "@alice:example", 1, "i", None, None);
        let own_notice = event_room_activity(topic_state("!room:example", "topic"));
        let others_image =
            image_activity("!room:example", "$o", "@bob:example", 1, "o", None, None);
        let redacted = RoomActivity {
            kind: RoomActivityKind::Redacted(RoomRedactedActivity {
                event_id: "$r".to_owned(),
            }),
            ..own_message.clone()
        };

        for activity in [&own_message, &own_pending, &own_image, &own_notice] {
            assert_eq!(activity_level_for(activity, me), None, "{activity:?}");
        }
        // Sender first: the core never highlights our own line, but a
        // highlight must not make it ours to look at.
        assert_eq!(
            activity_level_for(&highlighted(own_message.clone()), me),
            None
        );
        assert_eq!(
            activity_level_for(&others_image, me),
            Some(ViewActivityLevel::Message)
        );
        assert_eq!(
            activity_level_for(&redacted, me),
            Some(ViewActivityLevel::Event)
        );
        // Not signed in: nothing is ours.
        assert_eq!(
            activity_level_for(&own_message, None),
            Some(ViewActivityLevel::Message)
        );
    }

    #[test]
    fn own_direct_message_neither_colours_nor_flashes() {
        let mut state = test_state();
        let mut direct_room = room("!dm:example", "Dm", false);
        direct_room.is_direct = true;
        direct_room.direct_user_ids = vec!["@alice:example".to_owned()];
        state.model.rooms = vec![direct_room];
        state.model.own_user_id = Some("@me:example".to_owned());
        let message_from = |event_id: &str, user_id: &str| {
            let mut activity = sent_message_activity("!dm:example", event_id, None, "hi", 1);
            let RoomActivityKind::Message(message) = &mut activity.kind else {
                unreachable!("a sent message activity is a message");
            };
            message.sender = user_profile(user_id);
            activity
        };
        assert_ne!(state.active_view, room_view_id("!dm:example"));

        assert!(!apply_room_activity_event(
            &mut state,
            message_from("$own", "@me:example")
        ));
        assert_eq!(state.room_activity.get(&room_view_id("!dm:example")), None);
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!dm:example")),
            None
        );

        assert!(apply_room_activity_event(
            &mut state,
            message_from("$theirs", "@alice:example")
        ));
        assert_eq!(
            state.room_activity.get(&room_view_id("!dm:example")),
            Some(&ViewActivityLevel::Message)
        );
    }

    #[test]
    fn view_activity_only_escalates_until_the_view_is_activated() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = room_view_id("!active:example");

        apply_core_event(&mut state, timeline("!room:example", "hello")).unwrap();
        apply_core_event(&mut state, topic_state("!room:example", "later topic")).unwrap();
        assert_eq!(
            state.room_activity.get(&room_view_id("!room:example")),
            Some(&ViewActivityLevel::Message)
        );

        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(highlighted(sent_message_activity(
                "!room:example",
                "$ping",
                None,
                "Alice: ping",
                2,
            )))),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!room:example", "after the ping")).unwrap();
        assert_eq!(
            state.room_activity.get(&room_view_id("!room:example")),
            Some(&ViewActivityLevel::Highlight)
        );

        handle_view_activation(&mut state, &room_view_id("!room:example")).unwrap();
        assert!(state.room_activity.is_empty());
    }

    #[test]
    fn room_activity_wants_attention_even_in_the_active_view_but_not_during_loads() {
        let mut state = test_state();
        let mut query = room("!query:example", "Casey", false);
        query.is_direct = true;
        state.model.rooms = vec![room("!room:example", "Room", false), query];
        state.active_view = room_view_id("!query:example");
        let ping = || {
            Event::RoomActivity(Box::new(highlighted(sent_message_activity(
                "!room:example",
                "$ping",
                None,
                "Alice: ping",
                1,
            ))))
        };

        assert!(!apply_core_event(&mut state, timeline("!room:example", "hello")).unwrap());
        assert!(!apply_core_event(&mut state, topic_state("!query:example", "topic")).unwrap());
        assert!(apply_core_event(&mut state, timeline("!query:example", "hi")).unwrap());
        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!query:example"))
        );
        assert!(apply_core_event(&mut state, ping()).unwrap());
        assert!(!apply_core_event(&mut state, Event::SyncStarted).unwrap());

        state.synthetic_load_in_progress = true;
        assert!(!apply_core_event(&mut state, ping()).unwrap());
        assert!(!apply_core_event(&mut state, timeline("!query:example", "loaded")).unwrap());
    }

    fn with_mode(mut room: RoomSummary, mode: RoomNotificationMode) -> RoomSummary {
        room.notification_mode = Some(mode);
        room
    }

    #[test]
    fn a_muted_rooms_activity_stays_event_coloured_and_never_flashes() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            with_mode(
                room("!muted:example", "Muted", false),
                RoomNotificationMode::Mute,
            ),
            with_mode(
                query("!quiet-dm:example", "Casey", "@casey:example"),
                RoomNotificationMode::Mute,
            ),
        ];
        state.active_view = room_view_id("!active:example");

        assert!(!apply_core_event(&mut state, timeline("!muted:example", "hello")).unwrap());
        let ping = highlighted(sent_message_activity(
            "!muted:example",
            "$ping",
            None,
            "Alice: ping",
            1,
        ));
        assert!(!apply_core_event(&mut state, Event::RoomActivity(Box::new(ping))).unwrap());
        assert!(!apply_core_event(&mut state, timeline("!quiet-dm:example", "hi")).unwrap());
        assert_eq!(
            state.room_activity.get(&room_view_id("!muted:example")),
            Some(&ViewActivityLevel::Event)
        );
        assert_eq!(
            state.room_activity.get(&room_view_id("!quiet-dm:example")),
            Some(&ViewActivityLevel::Event)
        );
    }

    #[test]
    fn a_mentions_only_query_message_is_red_but_does_not_flash() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            with_mode(
                query("!dm:example", "Casey", "@casey:example"),
                RoomNotificationMode::MentionsAndKeywordsOnly,
            ),
        ];
        state.active_view = room_view_id("!active:example");

        assert!(!apply_core_event(&mut state, timeline("!dm:example", "hi")).unwrap());
        assert_eq!(
            state.room_activity.get(&room_view_id("!dm:example")),
            Some(&ViewActivityLevel::Message)
        );
        let ping = highlighted(sent_message_activity(
            "!dm:example",
            "$ping",
            None,
            "Casey: ping",
            1,
        ));
        assert!(apply_core_event(&mut state, Event::RoomActivity(Box::new(ping))).unwrap());
    }

    #[test]
    fn a_muted_closed_query_still_reopens_when_the_other_side_talks() {
        let mut state = test_state();
        state.model.rooms = vec![with_mode(
            query("!dm:example", "Alice", "@alice:example"),
            RoomNotificationMode::Mute,
        )];
        state.closed_queries.insert("!dm:example".to_owned());

        apply_core_event(&mut state, timeline("!dm:example", "hi")).unwrap();
        assert!(state.closed_queries.is_empty());
    }

    #[test]
    fn a_muted_rooms_unread_counts_are_event_coloured() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            with_mode(
                room("!muted:example", "Muted", false),
                RoomNotificationMode::Mute,
            ),
        ];
        state.active_view = room_view_id("!active:example");

        apply_core_event(
            &mut state,
            room_read_state("!muted:example", None, None, false, 3, 0, 1),
        )
        .unwrap();
        assert_eq!(
            state.room_activity.get(&room_view_id("!muted:example")),
            Some(&ViewActivityLevel::Event)
        );
    }

    #[test]
    fn read_state_marks_mentions_as_highlights_and_unread_as_messages() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!mentions:example", "Mentions", false),
            room("!unread:example", "Unread", false),
            room("!events:example", "Events", false),
        ];
        state.active_view = room_view_id("!active:example");

        apply_core_event(
            &mut state,
            room_read_state("!mentions:example", None, None, false, 2, 2, 1),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_read_state("!unread:example", None, None, false, 2, 0, 0),
        )
        .unwrap();
        apply_core_event(&mut state, topic_state("!events:example", "new topic")).unwrap();
        assert_eq!(
            state.room_activity.get(&room_view_id("!events:example")),
            Some(&ViewActivityLevel::Event)
        );
        // Zero counts clear the view, event-only activity included.
        apply_core_event(
            &mut state,
            room_read_state("!events:example", None, None, false, 0, 0, 0),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_read_state("!active:example", None, None, false, 4, 4, 4),
        )
        .unwrap();

        assert_eq!(
            state.room_activity,
            BTreeMap::from([
                (
                    room_view_id("!mentions:example"),
                    ViewActivityLevel::Highlight
                ),
                (room_view_id("!unread:example"), ViewActivityLevel::Message),
            ])
        );
    }

    #[test]
    fn inactive_synced_messages_failed_updates_and_state_notices_mark_room_activity() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(&mut state, timeline("!inactive:example", "hello")).unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued("!inactive:example", "txn-1", "draft"),
        )
        .unwrap();
        assert_eq!(
            state.room_activity.get(&room_view_id("!inactive:example")),
            Some(&ViewActivityLevel::Message)
        );

        state.room_activity.clear();

        apply_core_event(
            &mut state,
            outgoing_sent("!inactive:example", "txn-1", "$event:sent"),
        )
        .unwrap();
        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );

        apply_core_event(
            &mut state,
            outgoing_queued("!inactive:example", "txn-2", "retry"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_failed("!inactive:example", "txn-2", "send failed"),
        )
        .unwrap();
        assert_eq!(
            state.room_activity.get(&room_view_id("!inactive:example")),
            Some(&ViewActivityLevel::Message)
        );

        state.room_activity.clear();

        apply_core_event(
            &mut state,
            topic_state("!inactive:example", "incident review"),
        )
        .unwrap();

        assert_eq!(
            state.room_activity.get(&room_view_id("!inactive:example")),
            Some(&ViewActivityLevel::Event)
        );
    }

    #[test]
    fn inactive_event_backed_messages_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(&mut state, timeline("!inactive:example", "hello")).unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$!inactive:example:hello")
        );
    }

    #[test]
    fn synthetic_inactive_messages_mark_activity_without_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());
        state.synthetic_load_in_progress = true;

        apply_core_event(&mut state, timeline("!inactive:example", "hello")).unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            None
        );
    }

    #[test]
    fn inactive_unsupported_messages_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            unsupported_message(
                "!inactive:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "com.example.custom",
                "clip.mp4",
            ),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$image1:example.org")
        );
    }

    #[test]
    fn inactive_image_messages_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            image_message(
                "!inactive:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                Some("diagram.png"),
                Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: None,
                }),
            ),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$image1:example.org")
        );
    }

    #[test]
    fn inactive_file_messages_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(file_activity(
                "!inactive:example",
                "$file1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "Quarterly report",
                Some("report.pdf"),
                Some(RoomFileInfo {
                    mimetype: Some("application/pdf".to_owned()),
                    size: Some(45_678),
                }),
            ))),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$file1:example.org")
        );
    }

    #[test]
    fn inactive_audio_messages_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(audio_activity(
                "!inactive:example",
                "$audio1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "Voice note",
                Some("voice-note.ogg"),
                Some(RoomAudioInfo {
                    mimetype: Some("audio/ogg".to_owned()),
                    size: Some(23_456),
                    duration_ms: Some(12_000),
                }),
            ))),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$audio1:example.org")
        );
    }

    #[test]
    fn file_and_audio_room_activities_expose_event_ids_read_targets_and_thread_labels() {
        let file = file_activity(
            "!media:example.org",
            "$file1:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "Quarterly report",
            Some("report.pdf"),
            Some(RoomFileInfo {
                mimetype: Some("application/pdf".to_owned()),
                size: Some(45_678),
            }),
        );
        let audio = audio_activity(
            "!media:example.org",
            "$audio1:example.org",
            "@alice:example.org",
            1_710_000_060_000,
            "Voice note",
            Some("voice-note.ogg"),
            Some(RoomAudioInfo {
                mimetype: Some("audio/ogg".to_owned()),
                size: Some(23_456),
                duration_ms: Some(12_000),
            }),
        );

        assert_eq!(room_activity_event_id(&file), Some("$file1:example.org"));
        assert_eq!(
            read_target_for_room_activity(&file),
            Some("$file1:example.org")
        );
        assert_eq!(
            room_activity_thread_root_label(&file),
            Some("report.pdf".to_owned())
        );

        assert_eq!(room_activity_event_id(&audio), Some("$audio1:example.org"));
        assert_eq!(
            read_target_for_room_activity(&audio),
            Some("$audio1:example.org")
        );
        assert_eq!(
            room_activity_thread_root_label(&audio),
            Some("voice-note.ogg".to_owned())
        );
    }

    #[test]
    fn inactive_state_notices_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            topic_state("!inactive:example", "incident review"),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$topic:!inactive:example:incident review")
        );
    }

    #[test]
    fn pending_eventless_and_failed_send_activity_do_not_replace_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(&mut state, timeline("!inactive:example", "hello")).unwrap();
        let original_target = state
            .room_activity_read_targets
            .get(&room_view_id("!inactive:example"))
            .cloned();

        apply_core_event(
            &mut state,
            outgoing_queued("!inactive:example", "txn-1", "draft"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_failed("!inactive:example", "txn-1", "send failed"),
        )
        .unwrap();

        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example")),
            original_target.as_ref()
        );
    }

    #[test]
    fn active_room_events_do_not_mark_room_view_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = ViewId::Room("!room:example".to_owned());

        apply_core_event(&mut state, timeline("!room:example", "hello")).unwrap();
        apply_core_event(&mut state, topic_state("!room:example", "incident review")).unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
    }

    #[test]
    fn thread_view_is_not_treated_as_active_room_for_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = thread_view_id("!room:example", "$thread-root");

        apply_core_event(&mut state, timeline("!room:example", "hello")).unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!room:example"))
                .map(String::as_str),
            Some("$!room:example:hello")
        );
    }

    #[test]
    fn threaded_activity_marks_thread_view_and_not_parent_room() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        apply_core_event(
            &mut state,
            thread_timeline("!room:example", "$thread-1", "$thread-root", "thread reply"),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&thread_view_id("!room:example", "$thread-root"))
                .map(String::as_str),
            Some("$thread-1")
        );
    }

    #[test]
    fn active_matching_thread_view_does_not_mark_thread_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = thread_view_id("!room:example", "$thread-root");

        apply_core_event(
            &mut state,
            thread_timeline("!room:example", "$thread-1", "$thread-root", "thread reply"),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
    }

    #[test]
    fn active_parent_room_does_not_suppress_thread_activity_marker() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");

        apply_core_event(
            &mut state,
            thread_timeline("!room:example", "$thread-1", "$thread-root", "thread reply"),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
    }

    #[test]
    fn failed_thread_send_marks_inactive_thread_view() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = room_view_id("!active:example");

        apply_core_event(
            &mut state,
            outgoing_queued_in_thread("!room:example", "thread-pending-1", "$thread-root"),
        )
        .unwrap();
        state.room_activity.clear();

        apply_core_event(
            &mut state,
            outgoing_failed("!room:example", "thread-pending-1", "send failed"),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
    }

    #[test]
    fn unsupported_room_activity_updates_last_activity_preview() {
        let activity = unsupported_message_activity(
            "!media:example.org",
            "$image1:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "com.example.custom",
            "clip.mp4",
        );

        assert_eq!(
            last_activity_from_room_activity(&activity),
            Some(LastMessageActivity {
                room_id: "!media:example.org".to_owned(),
                event_id: Some("$image1:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example.org".to_owned(),
                timestamp_ms: 1_710_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "[unsupported com.example.custom] clip.mp4".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn image_room_activity_updates_last_activity_preview() {
        let activity = image_activity(
            "!media:example.org",
            "$image1:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "body fallback",
            Some("diagram.png"),
            Some(RoomImageInfo {
                mimetype: Some("image/png".to_owned()),
                size: Some(12_345),
                width: Some(640),
                height: Some(480),
                is_animated: Some(false),
                thumbnail_source: None,
            }),
        );

        assert_eq!(
            last_activity_from_room_activity(&activity),
            Some(LastMessageActivity {
                room_id: "!media:example.org".to_owned(),
                event_id: Some("$image1:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example.org".to_owned(),
                timestamp_ms: 1_710_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "[image] diagram.png".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn file_room_activity_updates_last_activity_preview() {
        let activity = file_activity(
            "!media:example.org",
            "$file1:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "Quarterly report",
            Some("report.pdf"),
            Some(RoomFileInfo {
                mimetype: Some("application/pdf".to_owned()),
                size: Some(45_678),
            }),
        );

        assert_eq!(
            last_activity_from_room_activity(&activity),
            Some(LastMessageActivity {
                room_id: "!media:example.org".to_owned(),
                event_id: Some("$file1:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example.org".to_owned(),
                timestamp_ms: 1_710_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "[file] report.pdf".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn audio_room_activity_updates_last_activity_preview() {
        let activity = audio_activity(
            "!media:example.org",
            "$audio1:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "Voice note",
            Some("voice-note.ogg"),
            Some(RoomAudioInfo {
                mimetype: Some("audio/ogg".to_owned()),
                size: Some(23_456),
                duration_ms: Some(12_000),
            }),
        );

        assert_eq!(
            last_activity_from_room_activity(&activity),
            Some(LastMessageActivity {
                room_id: "!media:example.org".to_owned(),
                event_id: Some("$audio1:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example.org".to_owned(),
                timestamp_ms: 1_710_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "[audio] voice-note.ogg".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn server_read_state_clears_matching_activity_read_target_without_command() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);
        state.room_activity.insert(
            thread_view_id("!room:example", "$thread-root"),
            ViewActivityLevel::Message,
        );
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event".to_owned());
        state.room_activity_read_targets.insert(
            thread_view_id("!room:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        apply_core_event(
            &mut state,
            room_read_state("!room:example", Some("$event"), None, false, 0, 0, 0),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(
            state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert_eq!(
            state.model.room_read_states.get("!room:example"),
            Some(&RoomReadState {
                room_id: "!room:example".to_owned(),
                latest_active_event_id: Some("$event".to_owned()),
                fully_read_event_id: None,
                marked_unread: false,
                num_unread: 0,
                num_notifications: 0,
                num_mentions: 0,
            })
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn server_fully_read_marker_clears_matching_activity_read_target_without_command() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);
        state.room_activity.insert(
            thread_view_id("!room:example", "$thread-root"),
            ViewActivityLevel::Message,
        );
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event".to_owned());
        state.room_activity_read_targets.insert(
            thread_view_id("!room:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        apply_core_event(
            &mut state,
            room_read_state("!room:example", None, Some("$event"), false, 0, 0, 0),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(
            state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn server_zero_unread_state_clears_only_room_activity() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);
        state.room_activity.insert(
            thread_view_id("!room:example", "$thread-root"),
            ViewActivityLevel::Message,
        );
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event".to_owned());
        state.room_activity_read_targets.insert(
            thread_view_id("!room:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        apply_core_event(
            &mut state,
            room_read_state("!room:example", None, None, false, 0, 0, 0),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
    }

    #[test]
    fn server_unread_state_marks_inactive_room_without_local_timeline_event() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            room_read_state("!room:example", None, None, false, 1, 0, 0),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
    }

    #[test]
    fn server_unread_state_does_not_mark_active_room() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = ViewId::Room("!room:example".to_owned());

        apply_core_event(
            &mut state,
            room_read_state("!room:example", None, None, false, 1, 0, 0),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
    }

    #[test]
    fn caught_up_zero_read_state_clears_navigation_suffixes() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        apply_core_event(
            &mut state,
            room_read_state(
                "!room:example",
                Some("$latest"),
                Some("$fully-read"),
                true,
                3,
                2,
                1,
            ),
        )
        .unwrap();
        assert_eq!(
            room_navigation_label(&state.model, &state.model.rooms[0]),
            "Room [@1 !2 *3 M]"
        );
        assert_eq!(
            switchbar_button_text(&state, &room_view_id("!room:example")),
            "Room [@1 !2 *3 M]"
        );

        apply_core_event(
            &mut state,
            room_read_state(
                "!room:example",
                Some("$latest"),
                Some("$latest"),
                false,
                0,
                0,
                0,
            ),
        )
        .unwrap();
        assert_eq!(
            room_navigation_label(&state.model, &state.model.rooms[0]),
            "Room"
        );
        assert_eq!(
            switchbar_button_text(&state, &room_view_id("!room:example")),
            "Room"
        );
    }

    #[test]
    fn server_read_state_ignores_unknown_room() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            room_read_state("!missing:example", None, None, false, 1, 0, 0),
        )
        .unwrap();

        assert!(state.room_activity.is_empty());
        assert!(state.room_activity_read_targets.is_empty());
        assert!(state.model.rooms.is_empty());
        assert!(state.model.room_read_states.is_empty());
        assert!(state.model.room_views.is_empty());
        assert!(state.views.is_empty());
    }

    #[test]
    fn synced_rooms_prunes_stale_room_activity() {
        let mut state = test_state();
        // Thread activity stays tracked only while the thread is projected.
        apply_core_event(
            &mut state,
            thread_timeline("!keep:example", "$thread-1", "$thread-root", "thread reply"),
        )
        .unwrap();
        state
            .room_activity
            .insert(room_view_id("!keep:example"), ViewActivityLevel::Message);
        state
            .room_activity
            .insert(room_view_id("!stale:example"), ViewActivityLevel::Message);
        state.room_activity.insert(
            thread_view_id("!keep:example", "$thread-root"),
            ViewActivityLevel::Message,
        );

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!keep:example", "Keep", false)]),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!keep:example"))
        );
        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!stale:example"))
        );
        assert!(
            state
                .room_activity
                .contains_key(&thread_view_id("!keep:example", "$thread-root"))
        );
    }

    #[test]
    fn synced_rooms_prune_room_activity_read_targets() {
        let mut state = test_state();
        // Thread activity stays tracked only while the thread is projected.
        apply_core_event(
            &mut state,
            thread_timeline("!keep:example", "$thread-1", "$thread-root", "thread reply"),
        )
        .unwrap();
        state
            .room_activity
            .insert(room_view_id("!keep:example"), ViewActivityLevel::Message);
        state
            .room_activity
            .insert(room_view_id("!stale:example"), ViewActivityLevel::Message);
        state.room_activity.insert(
            thread_view_id("!keep:example", "$thread-root"),
            ViewActivityLevel::Message,
        );
        state
            .room_activity_read_targets
            .insert(room_view_id("!keep:example"), "$keep:example".to_owned());
        state
            .room_activity_read_targets
            .insert(room_view_id("!stale:example"), "$stale:example".to_owned());
        state.room_activity_read_targets.insert(
            thread_view_id("!keep:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!keep:example", "Keep", false)]),
        )
        .unwrap();

        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!keep:example"))
                .map(String::as_str),
            Some("$keep:example")
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&thread_view_id("!keep:example", "$thread-root"))
                .map(String::as_str),
            Some("$thread-1")
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!stale:example"))
        );
    }

    #[test]
    fn synced_rooms_prune_stale_room_read_states() {
        let mut state = test_state();
        state.model.room_read_states.insert(
            "!keep:example".to_owned(),
            RoomReadState {
                room_id: "!keep:example".to_owned(),
                latest_active_event_id: Some("$keep:example".to_owned()),
                fully_read_event_id: None,
                marked_unread: true,
                num_unread: 1,
                num_notifications: 0,
                num_mentions: 0,
            },
        );
        state.model.room_read_states.insert(
            "!stale:example".to_owned(),
            RoomReadState {
                room_id: "!stale:example".to_owned(),
                latest_active_event_id: Some("$stale:example".to_owned()),
                fully_read_event_id: None,
                marked_unread: false,
                num_unread: 0,
                num_notifications: 0,
                num_mentions: 0,
            },
        );

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!keep:example", "Keep", false),
                space("!space:example", "Space"),
            ]),
        )
        .unwrap();

        assert!(state.model.room_read_states.contains_key("!keep:example"));
        assert!(!state.model.room_read_states.contains_key("!stale:example"));
    }

    #[test]
    fn synced_rooms_prune_room_read_states_when_room_becomes_space() {
        let mut state = test_state();
        state.model.room_read_states.insert(
            "!room:example".to_owned(),
            RoomReadState {
                room_id: "!room:example".to_owned(),
                latest_active_event_id: Some("$room:example".to_owned()),
                fully_read_event_id: None,
                marked_unread: true,
                num_unread: 1,
                num_notifications: 0,
                num_mentions: 0,
            },
        );

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![space("!room:example", "Room Space")]),
        )
        .unwrap();

        assert!(!state.model.room_read_states.contains_key("!room:example"));
    }

    #[test]
    fn synced_rooms_prune_space_room_activity() {
        let mut state = test_state();
        state
            .room_activity
            .insert(room_view_id("!keep:example"), ViewActivityLevel::Message);
        state
            .room_activity
            .insert(room_view_id("!space:example"), ViewActivityLevel::Message);
        state.room_activity.insert(
            thread_view_id("!space:example", "$thread-root"),
            ViewActivityLevel::Message,
        );

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!keep:example", "Keep", false),
                space("!space:example", "Space"),
            ]),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!keep:example"))
        );
        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!space:example"))
        );
        assert!(
            !state
                .room_activity
                .contains_key(&thread_view_id("!space:example", "$thread-root"))
        );
    }

    #[test]
    fn synced_rooms_prune_space_room_activity_read_targets() {
        let mut state = test_state();
        state
            .room_activity
            .insert(room_view_id("!keep:example"), ViewActivityLevel::Message);
        state
            .room_activity
            .insert(room_view_id("!space:example"), ViewActivityLevel::Message);
        state
            .room_activity_read_targets
            .insert(room_view_id("!keep:example"), "$keep:example".to_owned());
        state
            .room_activity_read_targets
            .insert(room_view_id("!space:example"), "$space:example".to_owned());
        state.room_activity_read_targets.insert(
            thread_view_id("!space:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!keep:example", "Keep", false),
                space("!space:example", "Space"),
            ]),
        )
        .unwrap();

        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!keep:example"))
                .map(String::as_str),
            Some("$keep:example")
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!space:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!space:example", "$thread-root"))
        );
    }

    #[test]
    fn logged_out_clears_room_activity_read_targets() {
        let mut state = test_state();
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event:example".to_owned());

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut)
                .expect("logged out event must apply successfully");
        });

        assert!(state.room_activity_read_targets.is_empty());
    }

    #[test]
    fn logged_out_clears_room_activity() {
        let mut state = test_state();
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut)
                .expect("logged out event must apply successfully");
        });

        assert!(state.room_activity.is_empty());
    }

    #[test]
    fn logged_out_clears_room_read_states_before_same_room_id_returns() {
        let mut state = test_state();
        state.model.room_read_states.insert(
            "!room:example".to_owned(),
            RoomReadState {
                room_id: "!room:example".to_owned(),
                latest_active_event_id: Some("$room:example".to_owned()),
                fully_read_event_id: Some("$fully-read:example".to_owned()),
                marked_unread: true,
                num_unread: 1,
                num_notifications: 2,
                num_mentions: 1,
            },
        );

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut)
                .expect("logged out event must apply successfully");
        });
        apply_core_event(&mut state, Event::Started).unwrap();
        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!room:example", "Room", false)]),
        )
        .unwrap();

        assert!(!state.model.room_read_states.contains_key("!room:example"));
    }

    #[test]
    fn room_activation_with_read_target_enqueues_mark_room_read() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event:example".to_owned());

        assert_eq!(
            handle_view_activation(&mut state, &room_view_id("!room:example")),
            Ok(true)
        );
        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::MarkRoomRead(MarkRoomReadRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$event:example".to_owned(),
            })]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn room_activation_without_room_activity_discards_stale_read_target_without_command() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event:example".to_owned());

        assert_eq!(
            handle_view_activation(&mut state, &room_view_id("!room:example")),
            Ok(false)
        );
        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(state.recorded_commands.is_empty());

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn room_activation_without_read_target_clears_room_activity_without_command() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);

        assert_eq!(
            handle_view_activation(&mut state, &room_view_id("!room:example")),
            Ok(true)
        );
        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(state.recorded_commands.is_empty());

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn thread_activation_with_read_target_clears_only_thread_activity_and_enqueues_mark_room_read()
    {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);
        state.room_activity.insert(
            thread_view_id("!room:example", "$thread-root"),
            ViewActivityLevel::Message,
        );
        state.room_activity_read_targets.insert(
            thread_view_id("!room:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        assert_eq!(
            handle_view_activation(&mut state, &thread_view_id("!room:example", "$thread-root")),
            Ok(true)
        );
        assert!(
            state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            !state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::MarkRoomRead(MarkRoomReadRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$thread-1".to_owned(),
            })]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn room_activation_leaves_thread_activity_intact() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .room_activity
            .insert(room_view_id("!room:example"), ViewActivityLevel::Message);
        state.room_activity.insert(
            thread_view_id("!room:example", "$thread-root"),
            ViewActivityLevel::Message,
        );

        assert_eq!(
            handle_view_activation(&mut state, &room_view_id("!room:example")),
            Ok(true)
        );
        assert!(
            !state
                .room_activity
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(state.recorded_commands.is_empty());

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn unregister_view_by_child_removes_destroyed_room_from_registries() {
        let mut state = test_state();
        state.model.rooms = vec![room("!alpha:example", "Alpha", false)];
        state.active_view = ViewId::Room("!beta:example".to_owned());
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(ViewId::Room("!beta:example".to_owned()), tracked_view(20));
        state.hwnd_views.insert(10, ViewId::Status);
        state
            .hwnd_views
            .insert(20, ViewId::Room("!beta:example".to_owned()));

        let removed = unregister_view_by_child(&mut state, 20 as HWND);

        assert_eq!(removed, Some(ViewId::Room("!beta:example".to_owned())));
        assert!(!state.hwnd_views.contains_key(&20));
        assert!(
            !state
                .views
                .contains_key(&ViewId::Room("!beta:example".to_owned()))
        );
        assert_eq!(state.active_view, ViewId::Room("!alpha:example".to_owned()));
    }

    #[test]
    fn select_room_rejects_spaces_without_changing_existing_selection() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!room:example", "Room", false),
            space("!space:example", "Space"),
        ];

        assert_eq!(select_room(&mut state, "!room:example"), Ok(()));
        assert_eq!(state.model.selected_room, Some("!room:example".to_owned()));
        assert_eq!(
            select_room(&mut state, "!space:example"),
            Err("unknown room id: !space:example".to_owned())
        );
        assert_eq!(
            select_room(&mut state, "!missing:example"),
            Err("unknown room id: !missing:example".to_owned())
        );
        assert_eq!(state.model.selected_room, Some("!room:example".to_owned()));
    }

    /// A channel Ops with Alice and Bob, and a query with Alice; Ops active.
    fn irc_command_state() -> (WindowState, CoreHandle) {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![
            room("!ops:example", "Ops", false),
            query("!dm:example", "Alice", "@alice:example"),
        ];
        let mut bob = member_summary("@bob:example", Some("Bob"), false);
        bob.rank = RoomMemberRank::Moderator;
        let ops = RoomViewModel {
            members: vec![member_summary("@alice:example", Some("Alice"), false), bob],
            ..Default::default()
        };
        state
            .model
            .room_views
            .insert("!ops:example".to_owned(), ops);
        let dm = RoomViewModel {
            members: vec![member_summary("@alice:example", Some("Alice"), false)],
            ..Default::default()
        };
        state.model.room_views.insert("!dm:example".to_owned(), dm);
        state.active_view = room_view_id("!ops:example");
        (state, handle)
    }

    fn sent_message(
        room_id: &str,
        presentation: RoomMessagePresentationKind,
        body: &str,
    ) -> Command {
        Command::SendMessage(SendMessageRequest {
            room_id: room_id.to_owned(),
            presentation,
            body: body.to_owned(),
            styles: Vec::new(),
            thread: None,
            reply: None,
        })
    }

    #[test]
    fn msg_and_notice_send_into_the_query_and_echo_in_the_active_window() {
        let (state, handle) = irc_command_state();
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/msg alice hi there");
                type_compose_line(hwnd, "/notice Alice heads up");
                type_compose_line(hwnd, "/msg Bob hi");
                type_compose_line(hwnd, "/msg Nobody hi");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![
                        sent_message("!dm:example", RoomMessagePresentationKind::Text, "hi there"),
                        sent_message(
                            "!dm:example",
                            RoomMessagePresentationKind::Notice,
                            "heads up"
                        ),
                    ]
                );
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    &lines[lines.len() - 4..],
                    [
                        "[22:12] -> *alice* hi there",
                        "[22:12] -> -Alice- heads up",
                        "[22:12] * /msg: no query with Bob; use /query Bob",
                        "[22:12] * /msg: no such nick: Nobody",
                    ]
                );
                assert_eq!(state_ref(hwnd).active_view, room_view_id("!ops:example"));
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn msg_sends_formatting_codes_as_styles_and_echoes_the_text() {
        let (state, handle) = irc_command_state();
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/msg alice \x034red\x03 alert");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![Command::SendMessage(SendMessageRequest {
                        room_id: "!dm:example".to_owned(),
                        presentation: RoomMessagePresentationKind::Text,
                        body: "red alert".to_owned(),
                        styles: vec![TextStyleSpan {
                            start: 0,
                            end: 3,
                            style: mirx_core::TextStyle {
                                foreground: Some(mirx_core::TextColor {
                                    red: 0xff,
                                    green: 0,
                                    blue: 0
                                }),
                                ..mirx_core::TextStyle::default()
                            },
                        }],
                        thread: None,
                        reply: None,
                    })]
                );
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(lines.last().unwrap(), "[22:12] -> *alice* red alert");
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn names_and_whois_print_member_data_in_the_active_window() {
        let (state, handle) = irc_command_state();
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/names");
                type_compose_line(hwnd, "/names Nowhere");
                // /whois asks the server, for members and strangers alike,
                // and prints its answer.
                type_compose_line(hwnd, "/whois Nobody");
                type_compose_line(hwnd, "/whois alice");
                type_compose_line(hwnd, "/whois @stranger:remote");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![
                        Command::FetchUserProfile("@alice:example".to_owned()),
                        Command::FetchUserProfile("@stranger:remote".to_owned()),
                    ]
                );
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "whois requested: @stranger:remote"
                );
                for profile in [
                    UserProfile {
                        user_id: "@alice:example".to_owned(),
                        display_name: Some("Alice Liddell".to_owned()),
                    },
                    UserProfile {
                        user_id: "@stranger:remote".to_owned(),
                        display_name: None,
                    },
                ] {
                    apply_core_event(state_mut(hwnd), Event::UserProfile(profile)).unwrap();
                }
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    &lines[lines.len() - 11..],
                    [
                        "[22:12] * Names Ops: Alice @Bob",
                        "[22:12] * End of /NAMES list",
                        "[22:12] * /names: unknown room: Nowhere",
                        "[22:12] * /whois: no such nick: Nobody",
                        "[22:12] * Alice is @alice:example * Alice Liddell",
                        "[22:12] * Alice on Ops Alice",
                        "[22:12] * Alice using example",
                        "[22:12] * Alice End of /WHOIS list",
                        "[22:12] * @stranger:remote is @stranger:remote",
                        "[22:12] * @stranger:remote using remote",
                        "[22:12] * @stranger:remote End of /WHOIS list",
                    ]
                );
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn ignore_and_unignore_check_locally_and_submit_core_commands() {
        let (mut state, handle) = irc_command_state();
        state.model.own_user_id = Some("@me:example".to_owned());
        state.model.ignored_users = Some(vec!["@spam:remote".to_owned()]);
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                for input in [
                    "/ignore alice",
                    "/ignore @me:example",
                    "/ignore @spam:remote",
                    "/unignore Bob",
                    "/unignore @spam:remote",
                    "/ignore",
                ] {
                    type_compose_line(hwnd, input);
                }
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![
                        Command::IgnoreUser("@alice:example".to_owned()),
                        Command::UnignoreUser("@spam:remote".to_owned()),
                    ]
                );
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "unignore submitted: @spam:remote"
                );
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    &lines[lines.len() - 5..],
                    [
                        "[22:12] * /ignore: you cannot ignore yourself",
                        "[22:12] * /ignore: @spam:remote is already ignored",
                        "[22:12] * /unignore: Bob is not ignored",
                        "[22:12] * Ignore list:",
                        "[22:12] *   @spam:remote",
                    ]
                );
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn query_with_a_message_creates_the_query_and_says_the_message_waits() {
        let (state, handle) = irc_command_state();
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/query bob hello");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![Command::CreateDirectRoom(CreateDirectRoomRequest {
                        user_id: "@bob:example".to_owned(),
                    })]
                );
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "query submitted: @bob:example"
                );
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    lines.last().map(String::as_str),
                    Some("[22:12] * /query: creating a query with @bob:example; message not sent")
                );
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn moderation_commands_check_the_member_and_submit_core_commands() {
        let (mut state, handle) = irc_command_state();
        let ops = state.model.room_views.get_mut("!ops:example").unwrap();
        ops.members[0].can_kick = true;
        ops.members[0].can_ban = true;
        let mut me = member_summary("@me:example", Some("Me"), false);
        me.rank = RoomMemberRank::Moderator;
        me.is_own_user = true;
        ops.members.push(me);
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                for input in [
                    "/kick alice spamming links",
                    "/kick Bob",
                    "/ban Alice",
                    "/kick",
                    "/kick Nobody",
                    "/unban @gone:example sorry",
                    "/invite Bob",
                    "/op Bob",
                    "/op alice",
                    "/voice Alice",
                    "/devoice Alice",
                    "/deop Bob",
                    "/deop Me",
                    "/nick New Name",
                ] {
                    type_compose_line(hwnd, input);
                }
                let room = || "!ops:example".to_owned();
                let power = |user_id: &str, role| {
                    Command::SetRoomMemberPowerLevel(SetRoomMemberPowerLevelRequest {
                        room_id: room(),
                        user_id: user_id.to_owned(),
                        role,
                    })
                };
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![
                        Command::KickRoomMember(KickRoomMemberRequest {
                            room_id: room(),
                            user_id: "@alice:example".to_owned(),
                            reason: Some("spamming links".to_owned()),
                        }),
                        Command::BanRoomMember(BanRoomMemberRequest {
                            room_id: room(),
                            user_id: "@alice:example".to_owned(),
                            reason: None,
                        }),
                        Command::UnbanRoomMember(UnbanRoomMemberRequest {
                            room_id: room(),
                            user_id: "@gone:example".to_owned(),
                            reason: Some("sorry".to_owned()),
                        }),
                        power("@alice:example", MemberPowerRole::Moderator),
                        power("@alice:example", MemberPowerRole::Voiced),
                        power("@bob:example", MemberPowerRole::Default),
                        Command::SetDisplayName(SetDisplayNameRequest {
                            display_name: "New Name".to_owned(),
                        }),
                    ]
                );
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "nick change submitted: New Name"
                );
                let lines = render_room_lines(state_ref(hwnd), "!ops:example");
                assert_eq!(
                    &lines[lines.len() - 7..],
                    [
                        "[22:12] * /kick: you cannot kick Bob",
                        "[22:12] * /kick: insufficient parameters",
                        "[22:12] * /kick: no such nick: Nobody",
                        "[22:12] * /invite: Bob is already in this room",
                        "[22:12] * /op: Bob is already an operator",
                        "[22:12] * /devoice: Alice is not voiced",
                        "[22:12] * /deop: you cannot deop yourself",
                    ]
                );
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn close_in_status_is_refused_in_the_window() {
        let state = test_state();
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/close");
                assert_eq!(
                    project_timeline_rows(&build_status_timeline_rows(
                        &state_ref(hwnd).model.status_log,
                        state_ref(hwnd).timestamps()
                    )),
                    vec!["[22:12] * /close: the Status window cannot be closed".to_owned()]
                );
            });
        }
    }

    #[test]
    fn command_error_echoes_in_the_room_window_and_the_status_bar() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .model
            .room_views
            .insert("!room:example".to_owned(), RoomViewModel::default());
        state.active_view = room_view_id("!room:example");

        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/react");
                let lines = render_room_lines(state_ref(hwnd), "!room:example");
                assert_eq!(
                    lines.last().map(String::as_str),
                    Some("[22:12] * /react: insufficient parameters")
                );
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "* /react: insufficient parameters"
                );
                assert!(state_ref(hwnd).model.status_log.lines().is_empty());
                assert_eq!(state_ref(hwnd).room_activity, BTreeMap::new());
                assert_eq!(dump_compose_text(hwnd).unwrap(), "");
            });
        }
    }

    #[test]
    fn command_error_in_status_echoes_in_the_status_log() {
        let state = test_state();

        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/wat now");
                let rows = build_status_timeline_rows(
                    &state_ref(hwnd).model.status_log,
                    state_ref(hwnd).timestamps(),
                );
                assert_eq!(
                    project_timeline_rows(&rows),
                    vec!["[22:12] * /wat: unknown command".to_owned()]
                );
                assert_eq!(rows[0].kind, TimelineRowKind::Info);
                assert_eq!(state_ref(hwnd).model.status, "* /wat: unknown command");
            });
        }
    }

    #[test]
    fn help_prints_the_command_list_in_the_active_window() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .model
            .room_views
            .insert("!room:example".to_owned(), RoomViewModel::default());
        state.active_view = room_view_id("!room:example");

        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/help");
                type_compose_line(hwnd, "/help clear");
                type_compose_line(hwnd, "/help nope");
                let lines = render_room_lines(state_ref(hwnd), "!room:example");
                let echo = &lines[lines.len() - 4..];
                assert!(echo[0].starts_with("[22:12] * Commands: /accept /ban /clear /close "));
                assert_eq!(echo[1], "[22:12] * /help <command> for usage");
                assert_eq!(echo[2], "[22:12] * /clear - clear this window");
                assert_eq!(echo[3], "[22:12] * /help: no such command: nope");
            });
        }
    }

    #[test]
    fn timestamp_command_restamps_every_window() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .model
            .room_views
            .insert("!room:example".to_owned(), RoomViewModel::default());
        state.active_view = room_view_id("!room:example");

        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/timestamp");
                type_compose_line(hwnd, "/timestamp off");
                assert_eq!(state_ref(hwnd).model.status, "timestamp: off");
                type_compose_line(hwnd, "/timestamp -f [HH:nn:ss]");
                assert_eq!(state_ref(hwnd).model.status, "timestamp format: [HH:nn:ss]");
                type_compose_line(hwnd, "/timestamp");
                type_compose_line(hwnd, "/timestamp on");
                let lines = render_room_lines(state_ref(hwnd), "!room:example");
                assert_eq!(
                    &lines[lines.len() - 2..],
                    [
                        "[22:12:00] * Timestamp is on ([HH:nn])",
                        "[22:12:00] * Timestamp is off ([HH:nn:ss])",
                    ]
                );
                let timeline = room_content_timeline(
                    state_ref(hwnd).views[&state_ref(hwnd).active_view].content,
                );
                assert_eq!(timeline_state_ref(timeline).indent_sample, "[00:00:00] ");
                assert_eq!(
                    timeline_state_ref(timeline)
                        .source_rows
                        .last()
                        .unwrap()
                        .logical_text,
                    "[22:12:00] * Timestamp is off ([HH:nn:ss])"
                );
            });
        }
    }

    #[test]
    fn settings_load_from_the_profile_except_in_render_state_mode() {
        with_local_app_data(|root| {
            let profile = root.join("mirx").join("ui-shell");
            fs::create_dir_all(&profile).unwrap();
            fs::write(profile.join(SETTINGS_FILE_NAME), "[timestamp]\nshow=off\n").unwrap();
            assert!(!load_settings(false).unwrap().settings().timestamp.show);
            assert!(load_settings(true).unwrap().settings().timestamp.show);

            fs::write(profile.join(SETTINGS_FILE_NAME), "[bogus]\n").unwrap();
            assert!(
                load_settings(false)
                    .err()
                    .unwrap()
                    .ends_with("mirx.ini line 1: unknown section [bogus]")
            );
        });
    }

    #[test]
    fn echo_lines_merge_into_the_room_timeline_by_time() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        for (event_id, body, timestamp_ms) in [
            ("$first", "first", 1_700_000_060_000),
            ("$second", "second", 1_700_000_180_000),
        ] {
            apply_core_event(
                &mut state,
                timeline_with_transaction_id("!ops:example", event_id, None, body, timestamp_ms),
            )
            .unwrap();
        }
        let echo = &mut state.model.room_views.get_mut("!ops:example").unwrap().echo;
        echo.push(1_700_000_000_000, None, "* before".to_owned());
        echo.push(1_700_000_120_000, None, "* between".to_owned());
        // A tie stays after the activity the user already saw.
        echo.push(1_700_000_180_000, None, "* tie".to_owned());
        echo.push(1_700_000_240_000, None, "* tie".to_owned());

        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps());
        let texts = project_timeline_rows(&rows);
        let tail = &texts[texts.len() - 6..];
        assert_eq!(
            tail,
            [
                "[22:13] * before",
                "[22:14] <@alice:example> first",
                "[22:15] * between",
                "[22:16] <@alice:example> second",
                "[22:16] * tie",
                "[22:17] * tie",
            ]
        );
        let identities = rows[rows.len() - 2..]
            .iter()
            .map(|row| row.identity.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            identities,
            vec![
                TimelineRowIdentity::EchoLine { sequence: 2 },
                TimelineRowIdentity::EchoLine { sequence: 3 },
            ]
        );
    }

    #[test]
    fn thread_echo_shows_only_in_its_thread_and_survives_room_clear() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "thread rep"),
        )
        .unwrap();
        let room_view = state.model.room_views.get_mut("!ops:example").unwrap();
        room_view.echo.push(
            1_700_000_200_000,
            Some("$thread-root".to_owned()),
            "* /clear: nothing to clear in thread view".to_owned(),
        );
        room_view
            .echo
            .push(1_700_000_200_000, None, "* room line".to_owned());

        let thread_lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
            state.timestamps(),
        ));
        assert!(
            thread_lines.contains(&"[22:16] * /clear: nothing to clear in thread view".to_owned())
        );
        assert!(!thread_lines.contains(&"[22:16] * room line".to_owned()));
        let room_lines = render_room_lines(&state, "!ops:example");
        assert!(room_lines.contains(&"[22:16] * room line".to_owned()));
        assert!(
            !room_lines
                .iter()
                .any(|line| line.contains("nothing to clear"))
        );

        state
            .model
            .room_views
            .get_mut("!ops:example")
            .unwrap()
            .clear_visible_entries();
        assert!(
            !render_room_lines(&state, "!ops:example").contains(&"[22:16] * room line".to_owned())
        );
        let thread_lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
            state.timestamps(),
        ));
        assert!(
            thread_lines.contains(&"[22:16] * /clear: nothing to clear in thread view".to_owned())
        );
    }

    #[test]
    fn each_window_recalls_only_what_was_typed_in_it() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .model
            .room_views
            .insert("!room:example".to_owned(), RoomViewModel::default());

        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                type_compose_line(hwnd, "/wat");

                // A second window, with its own editbox.
                let room_view = room_view_id("!room:example");
                let child = state_ref(hwnd).views[&ViewId::Status].child;
                let edit = CreateWindowExW(
                    0,
                    wide_null(EDIT_CLASS_NAME).as_ptr(),
                    wide_null("").as_ptr(),
                    WS_CHILD,
                    0,
                    0,
                    0,
                    0,
                    child,
                    null_mut(),
                    GetModuleHandleW(null()),
                    null_mut(),
                );
                assert!(!edit.is_null());
                state_mut(hwnd).views.insert(
                    room_view.clone(),
                    MdiViewState {
                        child,
                        content: null_mut(),
                        compose: Some(ViewCompose {
                            edit,
                            history: ComposeHistory::default(),
                            target: None,
                        }),
                    },
                );
                state_mut(hwnd).active_view = room_view.clone();
                install_test_view_content(hwnd);

                navigate_compose_history(hwnd, ComposeHistoryDirection::Previous).unwrap();
                assert_eq!(dump_compose_text(hwnd).unwrap(), "");
                type_compose_line(hwnd, "/help clear");

                state_mut(hwnd).active_view = ViewId::Status;
                navigate_compose_history(hwnd, ComposeHistoryDirection::Previous).unwrap();
                assert_eq!(dump_compose_text(hwnd).unwrap(), "/wat");
                navigate_compose_history(hwnd, ComposeHistoryDirection::Previous).unwrap();
                assert_eq!(dump_compose_text(hwnd).unwrap(), "/wat");

                state_mut(hwnd).active_view = room_view;
                navigate_compose_history(hwnd, ComposeHistoryDirection::Previous).unwrap();
                assert_eq!(dump_compose_text(hwnd).unwrap(), "/help clear");
            });
        }
    }

    #[test]
    fn invalid_e2ee_recover_form_is_not_recorded_or_recalled_in_compose_history() {
        let mut history = ComposeHistory::default();
        let input = "/e2ee recover secret phrase".to_owned();
        let ComposeSubmission::LocalCommand(command) = parse_compose_submission(&input) else {
            panic!("expected local command submission");
        };

        let history_entry = local_command_history_entry(&input, &command);
        assert_eq!(history_entry, None);
        if let Some(entry) = history_entry {
            history.record_submission(entry);
        }

        history.record_submission("safe draft".to_owned());

        assert!(!history.entries.iter().any(|entry| entry.contains("secret")));
        assert_eq!(
            history.previous("later draft".to_owned()),
            Some("safe draft")
        );
        assert_eq!(history.previous("ignored".to_owned()), Some("safe draft"));
    }

    #[test]
    fn invalid_e2ee_setup_form_is_not_recorded_or_recalled_in_compose_history() {
        let mut history = ComposeHistory::default();
        let malformed = "/e2ee setup wait passphrases secret phrase";
        let ComposeSubmission::LocalCommand(command) = parse_compose_submission(malformed) else {
            panic!("expected local command submission");
        };
        assert_eq!(local_command_history_entry(malformed, &command), None);
        history.record_submission("safe draft".to_owned());
        assert!(!history.entries.iter().any(|entry| entry.contains("secret")));
    }

    #[test]
    fn valid_e2ee_setup_passphrase_form_is_not_recorded_in_compose_history() {
        let mut history = ComposeHistory::default();
        let input = "/e2ee setup wait passphrase secret phrase";
        let ComposeSubmission::LocalCommand(command) = parse_compose_submission(input) else {
            panic!("expected local command submission");
        };

        assert_eq!(local_command_history_entry(input, &command), None);
        history.record_submission("safe draft".to_owned());
        assert!(
            !history
                .entries
                .iter()
                .any(|entry| entry.contains("secret phrase"))
        );
    }

    #[test]
    fn resolve_direct_room_query_target_distinguishes_none_single_and_ambiguous() {
        let mut single_room = room("!single:example", "Single", false);
        single_room.is_direct = true;
        single_room.direct_user_ids = vec!["@alice:example.org".to_owned()];

        let mut ambiguous_one = room("!ambiguous-one:example", "Ambiguous One", false);
        ambiguous_one.is_direct = true;
        ambiguous_one.direct_user_ids = vec!["@carol:example.org".to_owned()];

        let mut ambiguous_two = room("!ambiguous-two:example", "Ambiguous Two", false);
        ambiguous_two.is_direct = true;
        ambiguous_two.direct_user_ids = vec![
            "@carol:example.org".to_owned(),
            "@other:example.org".to_owned(),
        ];

        let model = UiModel {
            rooms: vec![
                room("!plain:example", "Plain", false),
                single_room,
                ambiguous_one,
                ambiguous_two,
            ],
            ..Default::default()
        };

        assert_eq!(
            resolve_direct_room_query_target(&model, "@missing:example.org"),
            QueryTargetResolution::None
        );
        assert_eq!(
            resolve_direct_room_query_target(&model, "@alice:example.org"),
            QueryTargetResolution::Single("!single:example".to_owned())
        );
        assert_eq!(
            resolve_direct_room_query_target(&model, "@carol:example.org"),
            QueryTargetResolution::Ambiguous
        );
    }

    #[test]
    fn resolve_invite_command_room_id_rejects_ambiguous_joined_unknown_and_missing_active_invites()
    {
        let mut state = test_state();
        state.model.rooms = vec![
            invited_room("!invite-one:example", "Shared"),
            invited_room("!invite-two:example", "Shared"),
            invited_room("!invite-unique:example", "Unique"),
            room("!joined:example", "Joined", false),
        ];

        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", Some("Shared")),
            Err("accept failed: ambiguous invited room target: Shared".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", Some("!joined:example")),
            Err("accept failed: room is not invited: !joined:example".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", Some("Joined")),
            Err("accept failed: room is not invited: Joined".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", Some("Missing")),
            Err("accept failed: invited room not found: Missing".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", None),
            Err("accept failed: no invited room selected".to_owned())
        );

        state.active_view = room_view_id("!joined:example");
        assert_eq!(
            resolve_invite_command_room_id(&state, "reject", None),
            Err("reject failed: no invited room selected".to_owned())
        );
    }

    #[test]
    fn submit_join_slash_command_records_typed_join_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();

        let status =
            submit_join_room(&mut state, "#ops:example.org".to_owned(), Vec::new()).unwrap();

        assert_eq!(status, "join submitted: #ops:example.org");
        assert_eq!(
            state.recorded_commands,
            vec![Command::JoinRoom(JoinRoomRequest {
                room_or_alias: "#ops:example.org".to_owned(),
                via: Vec::new(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_create_room_records_a_private_or_public_room_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.connection = ConnectionState::Syncing;
        state.model.own_user_id = Some("@me:example.org".to_owned());

        let private = CreateRoomArgs::Private {
            name: "Team room".to_owned(),
        };
        let public = |server: Option<&str>, name: Option<&str>| CreateRoomArgs::Public {
            alias_localpart: "foo".to_owned(),
            alias_server: server.map(str::to_owned),
            name: name.map(str::to_owned),
        };
        assert_eq!(
            submit_create_room(&mut state, private),
            Ok("create submitted: Team room".to_owned())
        );
        assert_eq!(
            submit_create_room(&mut state, public(None, None)),
            Ok("create submitted: foo".to_owned())
        );
        assert_eq!(
            submit_create_room(&mut state, public(Some("example.org"), Some("Foo Bar"))),
            Ok("create submitted: Foo Bar".to_owned())
        );
        assert_eq!(
            submit_create_room(&mut state, public(Some("other.example"), None)),
            Err("the alias must be on your server (example.org)".to_owned())
        );

        let public_request = |name: &str| {
            Command::CreateRoom(CreateRoomRequest {
                name: name.to_owned(),
                visibility: CreateRoomVisibility::Public {
                    alias_localpart: "foo".to_owned(),
                },
            })
        };
        assert_eq!(
            state.recorded_commands,
            vec![
                Command::CreateRoom(CreateRoomRequest {
                    name: "Team room".to_owned(),
                    visibility: CreateRoomVisibility::Private,
                }),
                public_request("foo"),
                public_request("Foo Bar"),
            ]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_create_room_while_signed_out_records_nothing() {
        let mut state = test_state();

        assert_eq!(
            submit_create_room(
                &mut state,
                CreateRoomArgs::Private {
                    name: "Team room".to_owned()
                }
            ),
            Err("not connected to server".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn create_room_errors_keep_the_submission_status() {
        assert!(preserves_submission_status(
            "create submitted: Team room",
            Operation::CreateRoom
        ));
        assert_eq!(operation_name(Operation::CreateRoom), "create room");
    }

    #[test]
    fn submit_accept_slash_command_records_typed_accept_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![invited_room("!invite:example", "Invite")];
        state.active_view = room_view_id("!invite:example");

        let status = submit_accept_slash_command(&mut state, None).unwrap();

        assert_eq!(status, "accept submitted: !invite:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::AcceptRoomInvite(AcceptRoomInviteRequest {
                room_id: "!invite:example".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_membership_commands,
            vec![DumpSubmittedMembershipCommand {
                action: DumpMembershipCommandAction::AcceptInvite,
                target: "!invite:example".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_reject_slash_command_records_typed_reject_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![invited_room("!invite:example", "Invite")];
        state.active_view = room_view_id("!invite:example");

        let status = submit_reject_slash_command(&mut state, None).unwrap();

        assert_eq!(status, "reject submitted: !invite:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::RejectRoomInvite(RejectRoomInviteRequest {
                room_id: "!invite:example".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_membership_commands,
            vec![DumpSubmittedMembershipCommand {
                action: DumpMembershipCommandAction::RejectInvite,
                target: "!invite:example".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_query_slash_command_records_typed_create_direct_room_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();

        let status =
            submit_query_slash_command(&mut state, "@alice:example.org".to_owned()).unwrap();

        assert_eq!(status, "query submitted: @alice:example.org");
        assert_eq!(
            state.recorded_commands,
            vec![Command::CreateDirectRoom(CreateDirectRoomRequest {
                user_id: "@alice:example.org".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn room_history_backfill_submit_records_initial_loading_command() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: None,
                limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
            })]
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|room_view| &room_view.history),
            Some(&RoomHistoryBackfillState {
                next_token: None,
                status: RoomHistoryBackfillStatus::Loading { from: None },
            })
        );

        stop_test_core(handle);
    }

    #[test]
    fn room_history_backfill_submit_ignores_repeat_submit_while_loading() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(false)
        );
        assert_eq!(state.recorded_commands.len(), 1);
        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|room_view| &room_view.history),
            Some(&RoomHistoryBackfillState {
                next_token: None,
                status: RoomHistoryBackfillStatus::Loading { from: None },
            })
        );

        stop_test_core(handle);
    }

    #[test]
    fn room_history_backfill_success_sets_next_token_and_reuses_it_on_retry() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        apply_core_event(
            &mut state,
            room_history_backfill(
                "!room:example",
                "token-1",
                Some("token-2"),
                vec![history_backfill_event_from_event(timeline(
                    "!room:example",
                    "older row",
                ))],
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|room_view| &room_view.history),
            Some(&RoomHistoryBackfillState {
                next_token: Some("token-2".to_owned()),
                status: RoomHistoryBackfillStatus::Idle,
            })
        );

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        assert_eq!(
            state.recorded_commands.last(),
            Some(&Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: Some("token-2".to_owned()),
                limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
            }))
        );

        stop_test_core(handle);
    }

    #[test]
    fn room_history_backfill_success_with_no_end_exhausts_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        apply_core_event(
            &mut state,
            room_history_backfill(
                "!room:example",
                "token-1",
                None,
                vec![history_backfill_event_from_event(timeline(
                    "!room:example",
                    "older row",
                ))],
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|room_view| &room_view.history),
            Some(&RoomHistoryBackfillState {
                next_token: None,
                status: RoomHistoryBackfillStatus::Exhausted,
            })
        );
        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(false)
        );
        assert_eq!(state.recorded_commands.len(), 1);

        stop_test_core(handle);
    }

    #[test]
    fn room_history_backfill_error_preserves_retry_token_and_allows_retry() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .model
            .room_views
            .entry("!room:example".to_owned())
            .or_default()
            .history
            .next_token = Some("token-2".to_owned());

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::BackfillRoomHistory,
                room_id: Some("!room:example".to_owned()),
                message: "temporary backfill failure".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|room_view| &room_view.history),
            Some(&RoomHistoryBackfillState {
                next_token: Some("token-2".to_owned()),
                status: RoomHistoryBackfillStatus::Failed {
                    from: Some("token-2".to_owned()),
                    message: "temporary backfill failure".to_owned(),
                },
            })
        );

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        assert_eq!(
            state.recorded_commands.last(),
            Some(&Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: Some("token-2".to_owned()),
                limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
            }))
        );

        stop_test_core(handle);
    }

    fn status_log_texts(state: &WindowState) -> Vec<&str> {
        state
            .model
            .status_log
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect()
    }

    fn room_echo_texts<'s>(state: &'s WindowState, room_id: &str) -> Vec<&'s str> {
        state.model.room_views[room_id]
            .echo
            .lines_for(None)
            .map(|line| line.text.as_str())
            .collect()
    }

    fn room_error(operation: Operation, room_id: &str, message: &str) -> Event {
        Event::Error(CoreErrorEvent {
            operation,
            room_id: Some(room_id.to_owned()),
            message: message.to_owned(),
        })
    }

    #[test]
    fn a_rooms_error_prints_in_its_window_or_status_when_it_has_none() {
        let mut state = test_state();
        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!ops:example", "Ops", false),
                room("!lobby:example", "Lobby", false),
            ]),
        )
        .unwrap();
        let status_lines = state.model.status_log.lines().len();

        apply_core_event(
            &mut state,
            room_error(Operation::KickRoomMember, "!lobby:example", "not an op"),
        )
        .unwrap();
        assert_eq!(
            room_echo_texts(&state, "!lobby:example"),
            vec!["* Error during kick room member: not an op"]
        );
        assert!(room_echo_texts(&state, "!ops:example").is_empty());
        assert_eq!(state.model.status_log.lines().len(), status_lines);
        assert_eq!(state.model.status, "error during kick room member");

        apply_core_event(
            &mut state,
            room_error(Operation::SendMessage, "!left:example", "gone"),
        )
        .unwrap();
        assert_eq!(
            status_log_texts(&state).last(),
            Some(&"* Error during send text: gone")
        );

        // Room loads show their failure in the window themselves.
        apply_core_event(
            &mut state,
            room_error(Operation::FetchRoomEvent, "!ops:example", "offline"),
        )
        .unwrap();
        assert!(room_echo_texts(&state, "!ops:example").is_empty());
        assert_eq!(
            status_log_texts(&state).last(),
            Some(&"* Error during fetch message: offline")
        );
    }

    #[test]
    fn ignore_list_changes_and_user_command_errors_print_in_the_active_window() {
        let mut state = test_state();
        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!ops:example", "Ops", false)]),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomMembers(RoomMembers {
                room_id: "!ops:example".to_owned(),
                members: vec![member_summary("@bob:example", Some("Bob"), false)],
            }),
        )
        .unwrap();
        state.active_view = ViewId::Room("!ops:example".to_owned());

        // The list after signing in is no change.
        apply_core_event(
            &mut state,
            Event::IgnoredUsers(vec!["@spam:remote".to_owned()]),
        )
        .unwrap();
        assert!(room_echo_texts(&state, "!ops:example").is_empty());
        assert_eq!(
            state.model.ignored_users,
            Some(vec!["@spam:remote".to_owned()])
        );

        apply_core_event(
            &mut state,
            Event::IgnoredUsers(vec!["@bob:example".to_owned()]),
        )
        .unwrap();
        assert_eq!(
            room_echo_texts(&state, "!ops:example"),
            vec![
                "* Added Bob (@bob:example) to ignore list",
                "* Removed @spam:remote from ignore list",
            ]
        );

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::UnignoreUser,
                room_id: None,
                message: "failed to unignore @bob:example: offline".to_owned(),
            }),
        )
        .unwrap();
        assert_eq!(
            room_echo_texts(&state, "!ops:example").last(),
            Some(&"* /unignore: failed to unignore @bob:example: offline")
        );
    }

    #[test]
    fn a_backfill_error_fails_the_named_rooms_load_only() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![
            room("!room:example", "Room", false),
            room("!other:example", "Other", false),
        ];
        for room_id in ["!room:example", "!other:example"] {
            state
                .model
                .room_views
                .entry(room_id.to_owned())
                .or_default()
                .history
                .next_token = Some("token-2".to_owned());
        }
        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );

        // A late error for a room that is not loading changes nothing.
        apply_core_event(
            &mut state,
            room_error(Operation::BackfillRoomHistory, "!other:example", "late"),
        )
        .unwrap();
        assert_eq!(
            state.model.room_views["!other:example"].history.status,
            RoomHistoryBackfillStatus::Idle
        );
        assert!(matches!(
            state.model.room_views["!room:example"].history.status,
            RoomHistoryBackfillStatus::Loading { .. }
        ));

        apply_core_event(
            &mut state,
            room_error(Operation::BackfillRoomHistory, "!room:example", "offline"),
        )
        .unwrap();
        assert_eq!(
            state.model.room_views["!room:example"].history.status,
            RoomHistoryBackfillStatus::Failed {
                from: Some("token-2".to_owned()),
                message: "offline".to_owned(),
            }
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_part_slash_command_from_active_room_records_typed_leave_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");

        let status = submit_part_slash_command(&mut state, None).unwrap();

        assert_eq!(status, "part submitted: !room:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::LeaveRoom(LeaveRoomRequest {
                room_id: "!room:example".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_part_slash_command_from_thread_targets_parent_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = thread_view_id("!room:example", "$thread-root");

        let status = submit_part_slash_command(&mut state, None).unwrap();

        assert_eq!(status, "part submitted: !room:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::LeaveRoom(LeaveRoomRequest {
                room_id: "!room:example".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn active_room_action_state_matches_active_invite_and_joined_views() {
        let mut state = test_state();
        state.model.rooms = vec![
            invited_room("!invite:example", "Invite"),
            room("!joined:example", "Joined", false),
        ];

        state.active_view = room_view_id("!invite:example");
        assert_eq!(
            active_room_action_state(&state),
            RoomActionState {
                accept_invite_enabled: true,
                reject_invite_enabled: true,
                part_room_enabled: false,
                notification_mode: None,
            }
        );

        state.active_view = thread_view_id("!joined:example", "$thread:example");
        assert_eq!(
            active_room_action_state(&state),
            RoomActionState {
                accept_invite_enabled: false,
                reject_invite_enabled: false,
                part_room_enabled: true,
                notification_mode: Some(RoomNotificationMode::AllMessages),
            }
        );

        state.model.rooms[1].notification_mode = Some(RoomNotificationMode::Mute);
        state.active_view = room_view_id("!joined:example");
        assert_eq!(
            active_room_action_state(&state).notification_mode,
            Some(RoomNotificationMode::Mute)
        );

        state.active_view = ViewId::Status;
        assert_eq!(active_room_action_state(&state).notification_mode, None);
    }

    #[test]
    fn submit_room_notification_mode_records_the_command_and_names_the_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");

        assert_eq!(
            submit_room_notification_mode(&mut state, None, RoomNotificationMode::Mute),
            Ok("notifications for Room: Mute".to_owned())
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::SetRoomNotificationMode(
                SetRoomNotificationModeRequest {
                    room_id: "!room:example".to_owned(),
                    mode: RoomNotificationMode::Mute,
                }
            )]
        );
        // The check follows the server, not the click.
        assert_eq!(
            state.model.rooms[0].notification_mode,
            Some(RoomNotificationMode::AllMessages)
        );

        stop_test_core(handle);
    }

    #[test]
    fn active_message_action_state_requires_a_selected_rendered_event() {
        let mut state = test_state();
        state.active_view = room_view_id("!room:example");
        assert_eq!(
            active_message_action_state(&state),
            MessageActionState {
                selected_event_id: None,
                edit_enabled: false,
                save_attachment_enabled: false,
                go_to_original_enabled: false,
            }
        );

        state.selected_timeline_event = Some(SelectedTimelineEvent {
            view_id: state.active_view.clone(),
            event_id: "$missing:example".to_owned(),
        });
        let rows = vec![TimelineRow::full_width(
            TimelineRowKind::Plain,
            "context".to_owned(),
        )];
        validate_selected_timeline_event(&mut state, &room_view_id("!room:example"), &rows);
        assert_eq!(
            active_message_action_state(&state),
            MessageActionState {
                selected_event_id: None,
                edit_enabled: false,
                save_attachment_enabled: false,
                go_to_original_enabled: false,
            }
        );

        let rows = vec![
            TimelineRow::log(
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                    highlight: false,
                },
                "[00:00] @alice: message".to_owned(),
            )
            .with_event_id(Some("$event:example")),
        ];
        state.selected_timeline_event = Some(SelectedTimelineEvent {
            view_id: state.active_view.clone(),
            event_id: "$event:example".to_owned(),
        });
        validate_selected_timeline_event(&mut state, &room_view_id("!room:example"), &rows);
        assert_eq!(
            active_message_action_state(&state),
            MessageActionState {
                selected_event_id: Some("$event:example".to_owned()),
                edit_enabled: false,
                save_attachment_enabled: false,
                go_to_original_enabled: false,
            }
        );
    }

    #[test]
    fn sendfile_goes_to_the_active_joined_room_and_lets_the_core_type_it() {
        let mut state = test_state();
        let (sender, commands) = inert_command_sender();
        std::mem::forget(commands);
        state.command_sender = Some(sender);
        state.model.rooms = vec![
            room("!ops:example", "Ops", false),
            invited_room("!new:example", "New"),
        ];

        state.active_view = ViewId::Room("!ops:example".to_owned());
        let room_id = send_file_room_id(&state).unwrap();
        assert_eq!(
            submit_send_file(&mut state, room_id, PathBuf::from("C:\\shot.png")),
            Ok("File queued for !ops:example".to_owned())
        );
        assert_eq!(
            state.recorded_commands,
            [Command::SendFile(SendFileRequest {
                room_id: "!ops:example".to_owned(),
                path: PathBuf::from("C:\\shot.png"),
                filename: None,
                content_type: None,
            })]
        );

        assert_eq!(
            submit_send_file(&mut state, "!new:example".to_owned(), PathBuf::from("a")),
            Err("room is not joined: !new:example".to_owned())
        );
        state.active_view = ViewId::Thread {
            room_id: "!ops:example".to_owned(),
            thread_root_event_id: "$root".to_owned(),
        };
        assert_eq!(
            send_file_room_id(&state),
            Err("file attachments cannot target threads yet".to_owned())
        );
        state.active_view = ViewId::Status;
        assert_eq!(
            send_file_room_id(&state),
            Err("no room selected".to_owned())
        );
        assert_eq!(state.recorded_commands.len(), 1);
    }

    fn reply_to(event_id: &str) -> ComposeTarget {
        ComposeTarget::Reply {
            event_id: event_id.to_owned(),
        }
    }

    fn edit_of(event_id: &str) -> ComposeTarget {
        ComposeTarget::Edit {
            event_id: event_id.to_owned(),
        }
    }

    #[test]
    fn the_reply_menu_arms_the_editbox_and_enter_sends_the_reply() {
        unsafe {
            with_compose_target_window(|hwnd| {
                state_mut(hwnd).selected_timeline_event = Some(SelectedTimelineEvent {
                    view_id: room_view_id("!ops:example"),
                    event_id: "$q".to_owned(),
                });
                set_compose_text(hwnd, "half typed").unwrap();
                arm_compose_target_on_selected_message(hwnd, |event_id| ComposeTarget::Reply {
                    event_id,
                })
                .unwrap();
                assert_eq!(active_compose_target(hwnd), Some(reply_to("$q")));
                assert_eq!(dump_compose_text(hwnd).unwrap(), "half typed");
                assert!(state_ref(hwnd).recorded_commands.is_empty());
                assert_eq!(
                    active_timeline_lines(hwnd).last().unwrap(),
                    "* Replying to <@alice:example> are we deploying today? (Esc cancels)"
                );

                type_compose_line(hwnd, "sure");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![reply_command(RoomMessagePresentationKind::Text, "sure")]
                );
                assert_eq!(active_compose_target(hwnd), None);
                assert_eq!(dump_compose_text(hwnd).unwrap(), "");
                assert!(
                    !active_timeline_lines(hwnd)
                        .iter()
                        .any(|line| line.starts_with("* Replying"))
                );
            });
        }
    }

    #[test]
    fn other_commands_leave_a_reply_armed_and_me_replies_with_an_emote() {
        unsafe {
            with_compose_target_window(|hwnd| {
                type_compose_line(hwnd, "/reply $q");
                assert_eq!(active_compose_target(hwnd), Some(reply_to("$q")));
                type_compose_line(hwnd, "/names");
                assert_eq!(active_compose_target(hwnd), Some(reply_to("$q")));
                type_compose_line(hwnd, "/me nods");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![reply_command(RoomMessagePresentationKind::Emote, "nods")]
                );
                assert_eq!(active_compose_target(hwnd), None);
            });
        }
    }

    #[test]
    fn editing_an_emote_prefills_it_and_keeps_it_an_emote() {
        unsafe {
            with_compose_target_window(|hwnd| {
                state_mut(hwnd).selected_timeline_event = Some(SelectedTimelineEvent {
                    view_id: room_view_id("!ops:example"),
                    event_id: "$wave".to_owned(),
                });
                assert!(active_message_action_state(state_ref(hwnd)).edit_enabled);
                arm_compose_target_on_selected_message(hwnd, |event_id| ComposeTarget::Edit {
                    event_id,
                })
                .unwrap();
                assert_eq!(active_compose_target(hwnd), Some(edit_of("$wave")));
                assert_eq!(dump_compose_text(hwnd).unwrap(), "/me waves");
                assert_eq!(
                    active_timeline_lines(hwnd).last().unwrap(),
                    "* Editing * @me:example waves (Esc cancels)"
                );

                type_compose_line(hwnd, "/me waves back");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![Command::EditMessage(EditMessageRequest {
                        room_id: "!ops:example".to_owned(),
                        target_event_id: "$wave".to_owned(),
                        presentation: RoomMessagePresentationKind::Emote,
                        body: "waves back".to_owned(),
                        styles: Vec::new(),
                    })]
                );
                assert_eq!(active_compose_target(hwnd), None);
            });
        }
    }

    #[test]
    fn an_unchanged_edit_sends_nothing_and_disarms() {
        unsafe {
            with_compose_target_window(|hwnd| {
                type_compose_line(hwnd, "/edit $mine");
                assert_eq!(dump_compose_text(hwnd).unwrap(), "deploy at 14:00");
                submit_compose_from_edit(hwnd).unwrap();
                assert!(state_ref(hwnd).recorded_commands.is_empty());
                assert_eq!(active_compose_target(hwnd), None);
                assert_eq!(dump_compose_text(hwnd).unwrap(), "");
            });
        }
    }

    #[test]
    fn edit_text_after_the_event_id_keeps_the_messages_kind() {
        unsafe {
            with_compose_target_window(|hwnd| {
                type_compose_line(hwnd, "/edit $wave waves twice");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![Command::EditMessage(EditMessageRequest {
                        room_id: "!ops:example".to_owned(),
                        target_event_id: "$wave".to_owned(),
                        presentation: RoomMessagePresentationKind::Emote,
                        body: "waves twice".to_owned(),
                        styles: Vec::new(),
                    })]
                );
                assert_eq!(active_compose_target(hwnd), None);
            });
        }
    }

    fn bold_span(start: usize, end: usize) -> TextStyleSpan {
        TextStyleSpan {
            start,
            end,
            style: bold_style(),
        }
    }

    #[test]
    fn formatting_codes_send_as_styles_and_codes_alone_send_nothing() {
        unsafe {
            with_compose_target_window(|hwnd| {
                type_compose_line(hwnd, "\x02hi\x02 there");
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![Command::SendMessage(SendMessageRequest {
                        room_id: "!ops:example".to_owned(),
                        presentation: RoomMessagePresentationKind::Text,
                        body: "hi there".to_owned(),
                        styles: vec![bold_span(0, 2)],
                        thread: None,
                        reply: None,
                    })]
                );
                assert_eq!(
                    dump_model_from_state(state_ref(hwnd)).submitted_message_commands,
                    vec![DumpSubmittedMessageCommand {
                        room_id: "!ops:example".to_owned(),
                        presentation: RoomMessagePresentationKind::Text,
                        body: "hi there".to_owned(),
                        styles: vec![bold_span(0, 2)],
                        thread: None,
                    }]
                );

                set_compose_text(hwnd, "\x02\x02").unwrap();
                assert_eq!(
                    submit_compose_from_edit(hwnd),
                    Err("message body must not be empty".to_owned())
                );
                type_compose_line(hwnd, "/me \x1fwaves\x0f");
                type_compose_line(hwnd, "/reply $q \x02yes");
                let styled = |command: &Command| match command {
                    Command::SendMessage(request) => (request.body.clone(), request.styles.clone()),
                    other => panic!("unexpected command {other:?}"),
                };
                let underline = TextStyleSpan {
                    start: 0,
                    end: 5,
                    style: mirx_core::TextStyle {
                        underline: true,
                        ..mirx_core::TextStyle::default()
                    },
                };
                assert_eq!(
                    state_ref(hwnd).recorded_commands[1..]
                        .iter()
                        .map(styled)
                        .collect::<Vec<_>>(),
                    [
                        ("waves".to_owned(), vec![underline]),
                        ("yes".to_owned(), vec![bold_span(0, 3)]),
                    ]
                );
                assert_eq!(
                    dump_model_from_state(state_ref(hwnd)).submitted_reply_commands[0].styles,
                    [bold_span(0, 3)]
                );
            });
        }
    }

    #[test]
    fn editing_a_formatted_message_prefills_its_codes() {
        let mut state = compose_target_state();
        let mut activity = sent_message_activity_with_presentation(
            "!ops:example",
            "$styled",
            None,
            RoomMessagePresentationKind::Text,
            "deploy now",
            1_700_000_180_000,
        );
        let RoomActivityKind::Message(message) = &mut activity.kind else {
            unreachable!()
        };
        message.sender = user_profile("@me:example");
        message.styles = vec![bold_span(7, 10)];
        apply_core_event(&mut state, Event::RoomActivity(Box::new(activity))).unwrap();
        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_view_content(hwnd);
                refresh_view_content(hwnd, &room_view_id("!ops:example")).unwrap();
                type_compose_line(hwnd, "/edit $styled");
                assert_eq!(dump_compose_text(hwnd).unwrap(), "deploy \x02now");
                // The same text in other codes is the same message.
                set_compose_text(hwnd, "deploy \x02now\x02").unwrap();
                submit_compose_from_edit(hwnd).unwrap();
                assert!(state_ref(hwnd).recorded_commands.is_empty());

                type_compose_line(hwnd, "/edit $styled");
                set_compose_text(hwnd, "deploy now").unwrap();
                submit_compose_from_edit(hwnd).unwrap();
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![Command::EditMessage(EditMessageRequest {
                        room_id: "!ops:example".to_owned(),
                        target_event_id: "$styled".to_owned(),
                        presentation: RoomMessagePresentationKind::Text,
                        body: "deploy now".to_owned(),
                        styles: Vec::new(),
                    })]
                );
                type_compose_line(hwnd, "/edit $styled \x02deploy\x02 now");
                assert_eq!(
                    dump_model_from_state(state_ref(hwnd)).submitted_edit_commands[1].styles,
                    [bold_span(0, 6)]
                );
            });
        }
    }

    #[test]
    fn format_chords_type_codes_and_the_edit_box_never_sees_their_characters() {
        use windows_sys::Win32::UI::Controls::EM_UNDO;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardState, SetKeyboardState};
        unsafe {
            with_compose_target_window(|hwnd| {
                let edit = compose_edit(hwnd);
                subclass_compose_edit(hwnd, edit).unwrap();
                let mut keys = [0u8; 256];
                assert_ne!(GetKeyboardState(keys.as_mut_ptr()), 0);
                let saved = keys;
                // What TranslateMessage makes of each chord, after its key.
                let chord = |letter: u8, ch: usize| {
                    SendMessageW(edit, WM_KEYDOWN, usize::from(letter), 0);
                    SendMessageW(edit, WM_CHAR, ch, 0);
                };

                keys[VK_CONTROL as usize] = 0x80;
                assert_ne!(SetKeyboardState(keys.as_ptr()), 0);
                for (letter, ch) in [
                    (b'B', 0x02),
                    (b'K', 0x0b),
                    (b'U', 0x15),
                    (b'I', 0x09),
                    (b'O', 0x0f),
                    (b'R', 0x12),
                ] {
                    chord(letter, ch);
                }
                assert_eq!(read_window_text(edit), "\x02\x03\x1f\x1d\x0f\x16");
                // The codes undo as typing does: a run of them at once.
                SendMessageW(edit, EM_UNDO, 0, 0);
                assert_eq!(read_window_text(edit), "");
                chord(b'B', 0x02);

                // AltGr is Ctrl+Alt: it types, it does not format.
                keys[VK_MENU as usize] = 0x80;
                assert_ne!(SetKeyboardState(keys.as_ptr()), 0);
                SendMessageW(edit, WM_KEYDOWN, usize::from(b'B'), 0);
                assert_eq!(read_window_text(edit), "\x02");

                keys = saved;
                keys[VK_CONTROL as usize] = 0;
                keys[VK_MENU as usize] = 0;
                assert_ne!(SetKeyboardState(keys.as_ptr()), 0);
                chord(b'B', usize::from(b'b'));
                assert_eq!(read_window_text(edit), "\x02b");
                assert_ne!(SetKeyboardState(saved.as_ptr()), 0);
            });
        }
    }

    #[test]
    fn the_automation_format_key_types_its_code() {
        unsafe {
            with_compose_target_window(|hwnd| {
                set_compose_text(hwnd, "a").unwrap();
                let edit = compose_edit(hwnd);
                SendMessageW(edit, EM_SETSEL, 1, 1);
                send_compose_key(
                    hwnd,
                    ComposeKey::Format {
                        key: crate::automation::MircFormatKey::Color,
                    },
                )
                .unwrap();
                assert_eq!(read_window_text(edit), "a\x03");
            });
        }
    }

    #[test]
    fn escape_cancels_an_edit_with_its_text_but_keeps_a_replys_text() {
        unsafe {
            with_compose_target_window(|hwnd| {
                type_compose_line(hwnd, "/edit $mine");
                cancel_compose_target(hwnd).unwrap();
                assert_eq!(active_compose_target(hwnd), None);
                assert_eq!(dump_compose_text(hwnd).unwrap(), "");

                type_compose_line(hwnd, "/reply $q");
                set_compose_text(hwnd, "draft").unwrap();
                cancel_compose_target(hwnd).unwrap();
                assert_eq!(active_compose_target(hwnd), None);
                assert_eq!(dump_compose_text(hwnd).unwrap(), "draft");
                assert!(state_ref(hwnd).recorded_commands.is_empty());
            });
        }
    }

    #[test]
    fn redacting_the_target_disarms_the_editbox() {
        unsafe {
            with_compose_target_window(|hwnd| {
                type_compose_line(hwnd, "/reply $q");
                apply_core_event(
                    state_mut(hwnd),
                    incoming_redacted("!ops:example", "$q", "$q-gone"),
                )
                .unwrap();
                refresh_view_content(hwnd, &room_view_id("!ops:example")).unwrap();
                assert_eq!(active_compose_target(hwnd), None);
            });
        }
    }

    #[test]
    fn only_the_users_own_messages_in_the_window_can_be_targeted() {
        unsafe {
            with_compose_target_window(|hwnd| {
                state_mut(hwnd).selected_timeline_event = Some(SelectedTimelineEvent {
                    view_id: room_view_id("!ops:example"),
                    event_id: "$q".to_owned(),
                });
                assert!(!active_message_action_state(state_ref(hwnd)).edit_enabled);
                type_compose_line(hwnd, "/edit $q");
                type_compose_line(hwnd, "/reply $elsewhere");
                type_compose_line(hwnd, "/edit $q hijacked");
                // The test clock prints them before the fixture's later lines.
                let lines = active_timeline_lines(hwnd);
                assert_eq!(
                    lines
                        .iter()
                        .filter(|line| line.starts_with("[22:12] * /"))
                        .collect::<Vec<_>>(),
                    [
                        "[22:12] * /edit: you can only edit your own messages",
                        "[22:12] * /reply: no such message in this window",
                        "[22:12] * /edit: you can only edit your own messages",
                    ]
                );
                assert_eq!(active_compose_target(hwnd), None);
                assert!(state_ref(hwnd).recorded_commands.is_empty());
            });
        }
    }

    #[test]
    fn react_menu_prefill_does_not_submit_a_command() {
        let mut state = test_state();
        state.active_view = room_view_id("!room:example");
        state.selected_timeline_event = Some(SelectedTimelineEvent {
            view_id: state.active_view.clone(),
            event_id: "$event-id".to_owned(),
        });

        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_active_timeline_event(hwnd, "$event-id");
                prefill_react_to_selected_message(hwnd).unwrap();
                assert_eq!(dump_compose_text(hwnd).unwrap(), "/react $event-id ");
                assert!(state_ref(hwnd).recorded_commands.is_empty());
            });
        }
    }

    #[test]
    fn redact_menu_submission_matches_slash_command_submission() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");
        state.selected_timeline_event = Some(SelectedTimelineEvent {
            view_id: state.active_view.clone(),
            event_id: "$event-id".to_owned(),
        });

        unsafe {
            with_test_compose_window(state, |hwnd| {
                install_test_active_timeline_event(hwnd, "$event-id");
                submit_redact_selected_message(hwnd).unwrap();
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "Redaction queued for $event-id"
                );
                assert_eq!(
                    state_ref(hwnd).recorded_commands,
                    vec![Command::RedactEvent(RedactEventRequest {
                        room_id: "!room:example".to_owned(),
                        event_id: "$event-id".to_owned(),
                    })]
                );
            });
        }
        stop_test_core(handle);
    }

    #[test]
    fn submit_part_slash_command_from_status_returns_no_room_selected() {
        let mut state = test_state();

        assert_eq!(
            submit_part_slash_command(&mut state, None),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_part_slash_command_resolves_explicit_room_target_like_window() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![
            room("!alpha:example", "Alpha", false),
            room("!beta:example", "Beta", false),
        ];

        let status = submit_part_slash_command(&mut state, Some("Beta")).unwrap();

        assert_eq!(status, "part submitted: !beta:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::LeaveRoom(LeaveRoomRequest {
                room_id: "!beta:example".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_part_slash_command_rejects_ambiguous_display_name_without_recording() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!shared-one:example", "Shared", false),
            room("!shared-two:example", "Shared", false),
        ];

        assert_eq!(
            submit_part_slash_command(&mut state, Some("Shared")),
            Err("ambiguous room: Shared".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_react_slash_command_from_active_room_records_typed_request_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");

        let status =
            submit_react_slash_command(&mut state, "$react-normal".to_owned(), "+1".to_owned())
                .unwrap();

        assert_eq!(status, "Reaction queued for $react-normal");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SendReaction(SendReactionRequest {
                room_id: "!room:example".to_owned(),
                target_event_id: "$react-normal".to_owned(),
                key: "+1".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_reaction_commands,
            vec![DumpSubmittedReactionCommand {
                room_id: "!room:example".to_owned(),
                target_event_id: "$react-normal".to_owned(),
                key: "+1".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_react_slash_command_from_thread_targets_parent_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = thread_view_id("!room:example", "$thread-root");

        let status = submit_react_slash_command(
            &mut state,
            "$react-normal".to_owned(),
            "two words".to_owned(),
        )
        .unwrap();

        assert_eq!(status, "Reaction queued for $react-normal");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SendReaction(SendReactionRequest {
                room_id: "!room:example".to_owned(),
                target_event_id: "$react-normal".to_owned(),
                key: "two words".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_react_slash_command_from_status_returns_no_room_selected() {
        let mut state = test_state();

        assert_eq!(
            submit_react_slash_command(&mut state, "$react-normal".to_owned(), "+1".to_owned(),),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn send_reply_from_the_active_room_names_the_replied_to_sender() {
        let mut state = compose_target_state();

        send_reply(
            &mut state,
            "$q",
            RoomMessagePresentationKind::Text,
            "hello there",
        )
        .unwrap();

        assert_eq!(
            state.recorded_commands,
            vec![reply_command(
                RoomMessagePresentationKind::Text,
                "hello there"
            )]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_reply_commands,
            vec![DumpSubmittedReplyCommand {
                room_id: "!ops:example".to_owned(),
                target_event_id: "$q".to_owned(),
                sender_user_id: "@alice:example".to_owned(),
                body: "hello there".to_owned(),
                styles: Vec::new(),
                thread: None,
            }]
        );
    }

    #[test]
    fn send_reply_from_a_thread_uses_the_compose_thread_destination() {
        let mut state = test_state();
        let (sender, _commands) = inert_command_sender();
        state.command_sender = Some(sender);
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root:example",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$thread-1:example",
                None,
                Some(thread_relation("$thread-root:example")),
                RoomMessagePresentationKind::Text,
                "durable reply",
                1_700_000_120_000,
            ),
        )
        .unwrap();
        state.active_view = thread_view_id("!ops:example", "$thread-root:example");
        state.room_activity.insert(
            thread_view_id("!ops:example", "$thread-root:example"),
            ViewActivityLevel::Message,
        );
        state.room_activity_read_targets.insert(
            thread_view_id("!ops:example", "$thread-root:example"),
            "$thread-1:example".to_owned(),
        );

        send_reply(
            &mut state,
            "$thread-1:example",
            RoomMessagePresentationKind::Text,
            "threaded acknowledgement",
        )
        .unwrap();

        assert_eq!(
            state.recorded_commands,
            vec![Command::SendMessage(SendMessageRequest {
                room_id: "!ops:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "threaded acknowledgement".to_owned(),
                styles: Vec::new(),
                thread: Some(SendMessageThreadTarget {
                    thread_root_event_id: "$thread-root:example".to_owned(),
                    latest_event_id: "$thread-1:example".to_owned(),
                }),
                reply: Some(SendMessageReplyTarget {
                    event_id: "$thread-1:example".to_owned(),
                    sender_user_id: "@alice:example".to_owned(),
                }),
            })]
        );
        assert!(
            !state
                .room_activity
                .contains_key(&thread_view_id("!ops:example", "$thread-root:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!ops:example", "$thread-root:example"))
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_reply_commands,
            vec![DumpSubmittedReplyCommand {
                room_id: "!ops:example".to_owned(),
                target_event_id: "$thread-1:example".to_owned(),
                sender_user_id: "@alice:example".to_owned(),
                body: "threaded acknowledgement".to_owned(),
                styles: Vec::new(),
                thread: Some(DumpSubmittedReplyThreadTarget {
                    thread_root_event_id: "$thread-root:example".to_owned(),
                    latest_event_id: "$thread-1:example".to_owned(),
                }),
            }]
        );
    }

    #[test]
    fn send_reply_and_send_edit_need_the_message_in_the_active_window() {
        let mut state = compose_target_state();
        assert_eq!(
            send_reply(
                &mut state,
                "$elsewhere",
                RoomMessagePresentationKind::Text,
                "hello"
            ),
            Err("no such message in this window".to_owned())
        );
        assert_eq!(
            send_edit(
                &mut state,
                "$elsewhere",
                RoomMessagePresentationKind::Text,
                "hello"
            ),
            Err("no such message in this window".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn send_reply_and_send_edit_require_an_available_command_sender() {
        let mut state = compose_target_state();
        state.command_sender = None;
        for result in [
            send_reply(&mut state, "$q", RoomMessagePresentationKind::Text, "hello"),
            send_edit(
                &mut state,
                "$mine",
                RoomMessagePresentationKind::Text,
                "hello",
            ),
        ] {
            assert_eq!(
                result,
                Err("core command sender is not available".to_owned())
            );
        }
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn send_edit_from_the_active_room_records_the_typed_request() {
        let mut state = compose_target_state();

        send_edit(
            &mut state,
            "$mine",
            RoomMessagePresentationKind::Text,
            "replacement text",
        )
        .unwrap();

        assert_eq!(
            state.recorded_commands,
            vec![Command::EditMessage(EditMessageRequest {
                room_id: "!ops:example".to_owned(),
                target_event_id: "$mine".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "replacement text".to_owned(),
                styles: Vec::new(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_edit_commands,
            vec![DumpSubmittedEditCommand {
                room_id: "!ops:example".to_owned(),
                target_event_id: "$mine".to_owned(),
                body: "replacement text".to_owned(),
                styles: Vec::new(),
            }]
        );
    }

    #[test]
    fn send_edit_from_a_thread_targets_the_parent_room() {
        let mut state = compose_target_state();
        let mut in_thread = sent_message_activity_with_relation_and_presentation(
            "!ops:example",
            "$mine-in-thread",
            None,
            Some(thread_relation("$q")),
            RoomMessagePresentationKind::Text,
            "thread draft",
            1_700_000_180_000,
        );
        let RoomActivityKind::Message(message) = &mut in_thread.kind else {
            unreachable!()
        };
        message.sender = user_profile("@me:example");
        apply_core_event(&mut state, Event::RoomActivity(Box::new(in_thread))).unwrap();
        state.active_view = thread_view_id("!ops:example", "$q");

        send_edit(
            &mut state,
            "$mine-in-thread",
            RoomMessagePresentationKind::Text,
            "thread final",
        )
        .unwrap();

        assert_eq!(
            state.recorded_commands,
            vec![Command::EditMessage(EditMessageRequest {
                room_id: "!ops:example".to_owned(),
                target_event_id: "$mine-in-thread".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "thread final".to_owned(),
                styles: Vec::new(),
            })]
        );
    }

    #[test]
    fn submit_redact_slash_command_from_active_room_records_typed_request_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");

        let status =
            submit_redact_slash_command(&mut state, "$event-to-redact".to_owned()).unwrap();

        assert_eq!(status, "Redaction queued for $event-to-redact");
        assert_eq!(
            state.recorded_commands,
            vec![Command::RedactEvent(RedactEventRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$event-to-redact".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_redaction_commands,
            vec![DumpSubmittedRedactionCommand {
                room_id: "!room:example".to_owned(),
                event_id: "$event-to-redact".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_redact_slash_command_from_thread_targets_parent_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = thread_view_id("!room:example", "$thread-root");

        let status =
            submit_redact_slash_command(&mut state, "$event-to-redact".to_owned()).unwrap();

        assert_eq!(status, "Redaction queued for $event-to-redact");
        assert_eq!(
            state.recorded_commands,
            vec![Command::RedactEvent(RedactEventRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$event-to-redact".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_unreact_slash_command_from_active_room_records_typed_request_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");

        let status =
            submit_unreact_slash_command(&mut state, "$reaction-alice-plus-one".to_owned())
                .unwrap();

        assert_eq!(
            status,
            "Reaction redaction queued for $reaction-alice-plus-one"
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::RedactEvent(RedactEventRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$reaction-alice-plus-one".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_redaction_commands,
            vec![DumpSubmittedRedactionCommand {
                room_id: "!room:example".to_owned(),
                event_id: "$reaction-alice-plus-one".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_unreact_slash_command_from_thread_targets_parent_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = thread_view_id("!room:example", "$thread-root");

        let status =
            submit_unreact_slash_command(&mut state, "$reaction-alice-plus-one".to_owned())
                .unwrap();

        assert_eq!(
            status,
            "Reaction redaction queued for $reaction-alice-plus-one"
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::RedactEvent(RedactEventRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$reaction-alice-plus-one".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_unreact_slash_command_from_status_returns_no_room_selected() {
        let mut state = test_state();

        assert_eq!(
            submit_unreact_slash_command(&mut state, "$reaction-alice-plus-one".to_owned()),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_topic_slash_command_from_active_room_records_typed_request_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");

        let status =
            submit_topic_slash_command(&mut state, None, "incident review".to_owned()).unwrap();

        assert_eq!(status, "topic change submitted for !room:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SetRoomTopic(SetRoomTopicRequest {
                room_id: "!room:example".to_owned(),
                topic: "incident review".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_room_topic_commands,
            vec![DumpSubmittedRoomTopicCommand {
                room_id: "!room:example".to_owned(),
                topic: "incident review".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_topic_slash_command_from_thread_targets_parent_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = thread_view_id("!room:example", "$thread-root");

        let status =
            submit_topic_slash_command(&mut state, None, "incident review".to_owned()).unwrap();

        assert_eq!(status, "topic change submitted for !room:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SetRoomTopic(SetRoomTopicRequest {
                room_id: "!room:example".to_owned(),
                topic: "incident review".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_topic_slash_command_from_status_returns_no_room_selected() {
        let mut state = test_state();

        assert_eq!(
            submit_topic_slash_command(&mut state, None, "incident review".to_owned()),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    fn aliased_rooms() -> Vec<RoomSummary> {
        let mut ops = room("!ops:example", "Ops", false);
        ops.canonical_alias = Some("#ops:example".to_owned());
        ops.topic = Some("incident review".to_owned());
        let mut lobby = room("!lobby:example", "Lobby", false);
        lobby.canonical_alias = Some("#lobby:example".to_owned());
        lobby.alt_aliases = vec!["#hall:example".to_owned()];
        let mut invite = invited_room("!invite:example", "Invite");
        invite.canonical_alias = Some("#party:example".to_owned());
        vec![ops, lobby, invite]
    }

    #[test]
    fn submit_topic_slash_command_names_the_room_by_alias() {
        let (mut state, handle) = test_state_with_command_sender();
        apply_core_event(&mut state, Event::SyncedRooms(aliased_rooms())).unwrap();
        state.active_view = room_view_id("!ops:example");

        let status =
            submit_topic_slash_command(&mut state, Some("#hall"), "new".to_owned()).unwrap();

        assert_eq!(status, "topic change submitted for !lobby:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SetRoomTopic(SetRoomTopicRequest {
                room_id: "!lobby:example".to_owned(),
                topic: "new".to_owned(),
            })]
        );
        assert_eq!(
            submit_topic_slash_command(&mut state, Some("#nope"), "x".to_owned()),
            Err("unknown room: #nope".to_owned())
        );
        assert_eq!(
            submit_topic_slash_command(&mut state, Some("#party"), "x".to_owned()),
            Err("room is not joined: #party".to_owned())
        );

        stop_test_core(handle);
    }

    #[test]
    fn room_topic_line_shows_the_active_or_named_rooms_topic() {
        let mut state = test_state();
        apply_core_event(&mut state, Event::SyncedRooms(aliased_rooms())).unwrap();

        assert_eq!(
            room_topic_line(&state, None),
            Err("no room selected".to_owned())
        );
        state.active_view = room_view_id("!ops:example");
        assert_eq!(
            room_topic_line(&state, None),
            Ok("* Topic is 'incident review'".to_owned())
        );
        assert_eq!(
            room_topic_line(&state, Some("#lobby:example")),
            Ok("* No topic is set".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn invite_and_msg_targets_accept_aliases() {
        let mut state = test_state();
        apply_core_event(&mut state, Event::SyncedRooms(aliased_rooms())).unwrap();

        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", Some("#party")),
            Ok("!invite:example".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", Some("#ops:example")),
            Err("accept failed: room is not invited: #ops:example".to_owned())
        );
        assert_eq!(
            msg_target_room_id(&state.model, "#lobby"),
            Ok("!lobby:example".to_owned())
        );
    }

    #[test]
    fn submit_topic_slash_command_does_not_mutate_local_topic_or_activity_before_inbound_event() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");
        apply_core_event(&mut state, topic_state("!room:example", "existing topic")).unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!room:example",
                "$event:example",
                None,
                "hello",
                1_700_000_000_000,
            ),
        )
        .unwrap();

        let before_activities = state
            .model
            .room_views
            .get("!room:example")
            .expect("room view must exist")
            .activities
            .clone();

        let status =
            submit_topic_slash_command(&mut state, None, "incident review".to_owned()).unwrap();

        assert_eq!(status, "topic change submitted for !room:example");
        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .and_then(|view| view.topic.as_deref()),
            Some("existing topic")
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|view| view.activities.clone()),
            Some(before_activities)
        );
        assert_eq!(
            render_room_lines(&state, "!room:example"),
            vec![
                "* Now talking in Room (!room:example)".to_owned(),
                "* Topic is 'existing topic'".to_owned(),
                "[22:14] * @alice:example changes topic to 'existing topic'".to_owned(),
                "[22:13] <@alice:example> hello".to_owned(),
            ]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_compose_enqueues_text_and_emote_messages_with_exact_requests() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!selected:example", "Selected", false)];
        state.model.selected_room = Some("!selected:example".to_owned());
        state.active_view = ViewId::Room("!selected:example".to_owned());

        let selected_room_id =
            submit_compose(&mut state, RoomMessagePresentationKind::Text, "hello", None).unwrap();
        assert_eq!(selected_room_id, "!selected:example");
        let emote_room_id = submit_compose(
            &mut state,
            RoomMessagePresentationKind::Emote,
            "waves",
            None,
        )
        .unwrap();
        assert_eq!(emote_room_id, "!selected:example");
        assert_eq!(
            state.recorded_commands,
            vec![
                Command::SendMessage(SendMessageRequest {
                    room_id: "!selected:example".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello".to_owned(),
                    styles: Vec::new(),
                    thread: None,
                    reply: None,
                }),
                Command::SendMessage(SendMessageRequest {
                    room_id: "!selected:example".to_owned(),
                    presentation: RoomMessagePresentationKind::Emote,
                    body: "waves".to_owned(),
                    styles: Vec::new(),
                    thread: None,
                    reply: None,
                }),
            ]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_compose_sends_a_literal_slash_body() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!beta:example", "Beta", false)];
        state.model.selected_room = Some("!beta:example".to_owned());
        state.active_view = ViewId::Room("!beta:example".to_owned());

        let room_id = submit_compose(
            &mut state,
            RoomMessagePresentationKind::Text,
            "/status",
            None,
        )
        .unwrap();
        assert_eq!(room_id, "!beta:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SendMessage(SendMessageRequest {
                room_id: "!beta:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "/status".to_owned(),
                styles: Vec::new(),
                thread: None,
                reply: None,
            })]
        );
        let dump = dump_model_from_state(&state);
        assert!(dump.submitted_reaction_commands.is_empty());
        assert!(dump.submitted_redaction_commands.is_empty());

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_compose_uses_active_thread_view_for_text_and_emote_messages() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root:example",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$thread-1:example",
                None,
                Some(thread_relation("$thread-root:example")),
                RoomMessagePresentationKind::Text,
                "durable reply",
                1_700_000_120_000,
            ),
        )
        .unwrap();
        state.active_view = thread_view_id("!ops:example", "$thread-root:example");
        state.room_activity.insert(
            thread_view_id("!ops:example", "$thread-root:example"),
            ViewActivityLevel::Message,
        );
        state.room_activity_read_targets.insert(
            thread_view_id("!ops:example", "$thread-root:example"),
            "$thread-1:example".to_owned(),
        );

        let room_id =
            submit_compose(&mut state, RoomMessagePresentationKind::Text, "hello", None).unwrap();
        assert_eq!(room_id, "!ops:example");
        assert!(
            !state
                .room_activity
                .contains_key(&thread_view_id("!ops:example", "$thread-root:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!ops:example", "$thread-root:example"))
        );
        let emote_room_id = submit_compose(
            &mut state,
            RoomMessagePresentationKind::Emote,
            "waves",
            None,
        )
        .unwrap();
        assert_eq!(emote_room_id, "!ops:example");
        assert_eq!(
            state.recorded_commands,
            vec![
                Command::SendMessage(SendMessageRequest {
                    room_id: "!ops:example".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello".to_owned(),
                    styles: Vec::new(),
                    thread: Some(SendMessageThreadTarget {
                        thread_root_event_id: "$thread-root:example".to_owned(),
                        latest_event_id: "$thread-1:example".to_owned(),
                    }),
                    reply: None,
                }),
                Command::SendMessage(SendMessageRequest {
                    room_id: "!ops:example".to_owned(),
                    presentation: RoomMessagePresentationKind::Emote,
                    body: "waves".to_owned(),
                    styles: Vec::new(),
                    thread: Some(SendMessageThreadTarget {
                        thread_root_event_id: "$thread-root:example".to_owned(),
                        latest_event_id: "$thread-1:example".to_owned(),
                    }),
                    reply: None,
                }),
            ]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_compose_with_explicit_room_id_ignores_active_thread_view() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$thread-1:example",
                None,
                Some(thread_relation("$thread-root:example")),
                RoomMessagePresentationKind::Text,
                "durable reply",
                1_700_000_120_000,
            ),
        )
        .unwrap();
        state.active_view = ViewId::Thread {
            room_id: "!ops:example".to_owned(),
            thread_root_event_id: "$thread-root:example".to_owned(),
        };

        let room_id = submit_compose(
            &mut state,
            RoomMessagePresentationKind::Text,
            "hello",
            Some("!ops:example"),
        )
        .unwrap();
        assert_eq!(room_id, "!ops:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SendMessage(SendMessageRequest {
                room_id: "!ops:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "hello".to_owned(),
                styles: Vec::new(),
                thread: None,
                reply: None,
            })]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_compose_rejects_text_and_emote_for_empty_body_invalid_room_and_missing_active_view() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!room:example", "Room", false),
            invited_room("!invite:example", "Invite"),
            space("!space:example", "Space"),
        ];

        assert_eq!(
            submit_compose(
                &mut state,
                RoomMessagePresentationKind::Text,
                "",
                Some("!room:example"),
            ),
            Err("message body must not be empty".to_owned())
        );
        state.model.selected_room = Some("!room:example".to_owned());
        assert_eq!(
            submit_compose(
                &mut state,
                RoomMessagePresentationKind::Emote,
                "waves",
                Some("!missing:example"),
            ),
            Err("unknown room id: !missing:example".to_owned())
        );
        assert_eq!(
            submit_compose(
                &mut state,
                RoomMessagePresentationKind::Emote,
                "waves",
                Some("!invite:example"),
            ),
            Err("room is not joined: !invite:example".to_owned())
        );
        assert_eq!(
            submit_compose(
                &mut state,
                RoomMessagePresentationKind::Emote,
                "waves",
                Some("!space:example"),
            ),
            Err("unknown room id: !space:example".to_owned())
        );
        assert_eq!(
            submit_compose(
                &mut state,
                RoomMessagePresentationKind::Emote,
                "waves",
                None,
            ),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn resolve_timeline_room_id_rejects_spaces() {
        let mut state = test_state();
        state.model.rooms = vec![
            space("!space:example", "Space"),
            room("!room:example", "Room", false),
        ];
        state.model.selected_room = Some("!room:example".to_owned());

        assert_eq!(
            resolve_timeline_room_id(&state, Some("!space:example")),
            Err("unknown room id: !space:example".to_owned())
        );
        assert_eq!(
            resolve_timeline_room_id(&state, Some("!room:example")),
            Ok("!room:example".to_owned())
        );
    }

    #[test]
    fn submit_compose_requires_available_command_sender() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.model.selected_room = Some("!room:example".to_owned());
        state.active_view = ViewId::Room("!room:example".to_owned());

        assert_eq!(
            submit_compose(&mut state, RoomMessagePresentationKind::Text, "hello", None,),
            Err("core command sender is not available".to_owned())
        );
    }

    #[test]
    fn compose_history_navigation_is_noop_when_empty() {
        let mut history = ComposeHistory::default();

        assert_eq!(history.previous("draft".to_owned()), None);
        assert_eq!(history.next(), None);
        assert!(history.navigation.is_none());
    }

    #[test]
    fn compose_history_navigates_entries_and_restores_draft() {
        let mut history = ComposeHistory {
            entries: vec!["first".to_owned(), "second".to_owned()],
            navigation: None,
        };

        assert_eq!(history.previous("draft".to_owned()), Some("second"));
        assert_eq!(history.previous("ignored".to_owned()), Some("first"));
        assert_eq!(history.previous("ignored".to_owned()), Some("first"));
        assert_eq!(history.next(), Some("second".to_owned()));
        assert_eq!(history.next(), Some("draft".to_owned()));
        assert_eq!(history.next(), None);
        assert!(history.navigation.is_none());
    }

    #[test]
    fn compose_history_reset_navigation_restarts_from_newest_with_fresh_draft() {
        let mut history = ComposeHistory {
            entries: vec!["first".to_owned(), "second".to_owned()],
            navigation: None,
        };

        assert_eq!(history.previous("draft".to_owned()), Some("second"));
        history.reset_navigation();
        assert!(history.navigation.is_none());
        assert_eq!(
            history.previous("replacement draft".to_owned()),
            Some("second")
        );
        assert_eq!(history.next(), Some("replacement draft".to_owned()));
    }

    #[test]
    fn lifecycle_events_update_model_and_persist_login_session() {
        with_local_app_data(|root| {
            let mut state = test_state();

            apply_core_event(&mut state, Event::Started).unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::CoreStarted);

            apply_core_event(
                &mut state,
                Event::LoggedIn {
                    session: stored_session(),
                    homeserver_url: "https://matrix.example".to_owned(),
                },
            )
            .unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Authenticated);
            assert_eq!(state.model.own_user_id.as_deref(), Some("@alice:example"));
            assert_eq!(
                state.current_session_homeserver_url,
                Some("https://matrix.example".to_owned())
            );

            let session_path = session_file_path(&root);
            let session_bytes = fs::read(&session_path).expect("saved session file must exist");
            assert_eq!(load_saved_session().unwrap(), saved_session_payload());
            assert_bytes_do_not_contain(&session_bytes, "secret-token");
            assert_bytes_do_not_contain(&session_bytes, "refresh-token");
            assert_bytes_do_not_contain(&session_bytes, "@alice:example");
            assert_bytes_do_not_contain(&session_bytes, "DEVICE");
            assert_bytes_do_not_contain(&session_bytes, "\"access_token\"");

            apply_core_event(&mut state, Event::SyncStarted).unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Syncing);

            apply_core_event(
                &mut state,
                Event::SyncedRooms(vec![room("!room:example", "Room", false)]),
            )
            .unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Ready);

            apply_core_event(&mut state, Event::RestoredSession).unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Authenticated);

            apply_core_event(
                &mut state,
                Event::Error(CoreErrorEvent {
                    operation: Operation::RestoreSession,
                    room_id: None,
                    message: "restore failed".to_owned(),
                }),
            )
            .unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
            assert_eq!(
                state.model.last_error,
                Some(CoreErrorEvent {
                    operation: Operation::RestoreSession,
                    room_id: None,
                    message: "restore failed".to_owned(),
                })
            );

            apply_core_event(&mut state, Event::LoggedOut).unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::LoggedOut);
            assert_eq!(state.model.own_user_id, None);
            assert_eq!(state.current_session_homeserver_url, None);
            assert!(!session_path.exists());

            apply_core_event(&mut state, Event::Stopped).unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Stopped);
        });
    }

    #[test]
    fn connection_events_are_logged_to_status_and_survive_logout() {
        with_local_app_data(|_| {
            let mut state = test_state();
            state.synthetic_load_in_progress = true;

            apply_core_event(&mut state, Event::Started).unwrap();
            apply_core_event(
                &mut state,
                Event::LoggedIn {
                    session: stored_session(),
                    homeserver_url: "https://matrix.example".to_owned(),
                },
            )
            .unwrap();
            apply_core_event(&mut state, Event::SyncStarted).unwrap();
            let rooms = vec![
                room("!room:example", "Room", false),
                invited_room("!invite:example", "Invite"),
            ];
            apply_core_event(&mut state, Event::SyncedRooms(rooms.clone())).unwrap();
            // The core re-sends the room list on every change.
            apply_core_event(&mut state, Event::SyncedRooms(rooms)).unwrap();
            apply_core_event(&mut state, Event::SyncStopped(SyncStopReason::Requested)).unwrap();
            apply_core_event(&mut state, Event::SyncStarted).unwrap();
            apply_core_event(
                &mut state,
                Event::SyncStopped(SyncStopReason::Failed(
                    "continuous Matrix sync failed: boom".to_owned(),
                )),
            )
            .unwrap();
            apply_core_event(&mut state, Event::LoggedOut).unwrap();
            apply_core_event(&mut state, Event::Stopped).unwrap();

            assert_eq!(state.model.own_user_id, None);
            assert_eq!(
                status_log_lines(&state),
                vec![
                    "[22:12] * Logged in as @alice:example (device DEVICE)".to_owned(),
                    "[22:12] * Syncing with the homeserver".to_owned(),
                    "[22:12] * Synced rooms: 1 joined, 1 invited".to_owned(),
                    "[22:12] * Disconnected".to_owned(),
                    "[22:12] * Syncing with the homeserver".to_owned(),
                    "[22:12] * Disconnected: continuous Matrix sync failed: boom".to_owned(),
                    "[22:12] * Logged out".to_owned(),
                    "[22:12] * Disconnected".to_owned(),
                ]
            );
        });
    }

    #[test]
    fn status_lines_mark_an_inactive_status_window() {
        let mut state = test_state();
        apply_core_event(&mut state, Event::SyncStarted).unwrap();
        assert_eq!(state.room_activity.get(&ViewId::Status), None);

        state.active_view = room_view_id("!room:example");
        apply_core_event(&mut state, Event::SyncStarted).unwrap();
        assert_eq!(
            state.room_activity.get(&ViewId::Status),
            Some(&ViewActivityLevel::Event)
        );
        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::SendMessage,
                room_id: Some("!left:example".to_owned()),
                message: "message send failed".to_owned(),
            }),
        )
        .unwrap();
        assert_eq!(
            state.room_activity.get(&ViewId::Status),
            Some(&ViewActivityLevel::Message)
        );
        let rows = build_status_timeline_rows(&state.model.status_log, state.timestamps());
        assert_eq!(rows.last().unwrap().kind, TimelineRowKind::Error);
    }

    #[test]
    fn logged_in_event_for_another_account_is_rejected() {
        let mut state = test_state();
        state.synthetic_load_in_progress = true;
        state.model.own_user_id = Some("@me:example".to_owned());

        assert_eq!(
            apply_core_event(
                &mut state,
                Event::LoggedIn {
                    session: stored_session(),
                    homeserver_url: "https://matrix.example".to_owned(),
                },
            ),
            Err("received LoggedIn for @alice:example while signed in as @me:example".to_owned())
        );
        assert_eq!(state.model.own_user_id.as_deref(), Some("@me:example"));
    }

    #[test]
    fn synthetic_logged_in_event_updates_model_without_persisting_session() {
        with_local_app_data(|root| {
            let mut state = test_state();
            state.synthetic_load_in_progress = true;

            apply_core_event(
                &mut state,
                Event::LoggedIn {
                    session: stored_session(),
                    homeserver_url: "https://matrix.example".to_owned(),
                },
            )
            .unwrap();

            assert_eq!(state.model.status, "authenticated");
            assert_eq!(state.model.lifecycle_state, LifecycleState::Authenticated);
            assert_eq!(state.current_session_homeserver_url, None);
            assert!(!root.join(PROFILE_SUBDIR).join(SESSION_FILE_NAME).exists());
        });
    }

    #[test]
    fn session_updated_event_persists_refreshed_tokens_without_leaking_them() {
        with_local_app_data(|root| {
            save_session("https://matrix.example", &stored_session()).unwrap();
            let mut state = test_state();
            state.current_session_homeserver_url = Some("https://matrix.example".to_owned());

            let updated_session = StoredSession {
                access_token: "updated-access-token".to_owned(),
                refresh_token: Some("updated-refresh-token".to_owned()),
                ..stored_session()
            };

            apply_core_event(
                &mut state,
                Event::SessionUpdated {
                    session: updated_session.clone(),
                },
            )
            .unwrap();

            assert_eq!(
                load_saved_session().unwrap(),
                SavedSession {
                    session: updated_session.clone(),
                    ..saved_session_payload()
                }
            );
            let session_bytes = fs::read(session_file_path(&root)).unwrap();
            assert_bytes_do_not_contain(&session_bytes, "updated-access-token");
            assert_bytes_do_not_contain(&session_bytes, "updated-refresh-token");
            assert!(
                !serde_json::to_string(&dump_model_from_state(&state))
                    .unwrap()
                    .contains("updated-access-token")
            );
            assert!(
                !serde_json::to_string(&state.model.status)
                    .unwrap()
                    .contains("updated-access-token")
            );
        });
    }

    #[test]
    fn startup_restore_keeps_restoring_state_when_started_arrives_after_submit() {
        with_local_app_data(|_| {
            let mut state = test_state();
            let (sender, _commands) = inert_command_sender();
            state.command_sender = Some(sender);
            save_session("https://matrix.example", &stored_session()).unwrap();

            submit_restore_saved_session(&mut state, false).unwrap();
            assert_eq!(state.model.status, "restoring saved session");
            assert_eq!(state.model.lifecycle_state, LifecycleState::Restoring);

            apply_core_event(&mut state, Event::Started).unwrap();
            assert_eq!(state.model.status, "restoring saved session");
            assert_eq!(state.model.lifecycle_state, LifecycleState::Restoring);
        });
    }

    #[test]
    fn selected_file_and_audio_enable_attachment_saving() {
        for activity in [
            file_activity(
                "!room:example",
                "$file:example",
                "@file:example",
                0,
                "file",
                None,
                None,
            ),
            audio_activity(
                "!room:example",
                "$audio:example",
                "@audio:example",
                0,
                "audio",
                None,
                None,
            ),
        ] {
            let event_id = match &activity.kind {
                RoomActivityKind::File(file) => file.event_id.clone(),
                RoomActivityKind::Audio(audio) => audio.event_id.clone(),
                _ => unreachable!(),
            };
            let mut state = test_state();
            state.active_view = room_view_id("!room:example");
            state
                .model
                .room_views
                .entry("!room:example".to_owned())
                .or_default()
                .activities
                .push(activity);
            state.selected_timeline_event = Some(SelectedTimelineEvent {
                view_id: state.active_view.clone(),
                event_id,
            });
            let menu_state = active_message_action_state(&state);
            assert!(menu_state.selected_event_id.is_some());
            assert!(menu_state.save_attachment_enabled);
        }
    }

    #[test]
    fn message_frame_is_the_frame_of_its_descendants_only() {
        unsafe {
            with_test_frame(600, 400, |hwnd| {
                let message_to = |target: HWND| MSG {
                    hwnd: target,
                    ..MSG::default()
                };
                let compose = state_ref(hwnd).views[&ViewId::Status].compose().edit;
                assert_eq!(message_frame(&message_to(compose)), Some(hwnd));
                assert_eq!(message_frame(&message_to(hwnd)), Some(hwnd));
                assert_eq!(message_frame(&message_to(null_mut())), None);

                register_about_dialog_class(GetModuleHandleW(null())).unwrap();
                let dialog = CreateWindowExW(
                    0,
                    wide_null(ABOUT_DIALOG_CLASS_NAME).as_ptr(),
                    wide_null("About mirx").as_ptr(),
                    WS_OVERLAPPED,
                    0,
                    0,
                    380,
                    230,
                    hwnd,
                    null_mut(),
                    GetModuleHandleW(null()),
                    null(),
                );
                assert!(!dialog.is_null());
                let ok = GetDlgItem(dialog, ABOUT_DIALOG_OK_ID as i32);
                assert!(!ok.is_null());
                assert_eq!(message_frame(&message_to(ok)), None);
                assert_eq!(message_frame(&message_to(dialog)), None);
                DestroyWindow(dialog);
            });
        }
    }

    #[test]
    #[should_panic(expected = "holds another kind of state")]
    fn a_window_does_not_give_its_state_as_another_kind() {
        unsafe {
            register_find_dialog_class(GetModuleHandleW(null())).unwrap();
            let mut dialog_state = window_data::CreateParam::new(FindDialogState {
                result: Arc::new(Mutex::new(None)),
                label: "Find".to_owned(),
                query_label: null_mut(),
                query_edit: null_mut(),
                ok_button: null_mut(),
                cancel_button: null_mut(),
            });
            let dialog = CreateWindowExW(
                0,
                wide_null(FIND_DIALOG_CLASS_NAME).as_ptr(),
                wide_null("Find").as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                420,
                170,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                dialog_state.as_param(),
            );
            assert!(!dialog.is_null());
            // What the dialog loops used to do with their own controls'
            // messages: take the root window for a frame.
            // The window goes with this test's thread.
            window_data::get::<WindowState>(dialog);
        }
    }

    /// A frame child's rectangle in the frame's client coordinates.
    unsafe fn frame_child_layout(hwnd: HWND, child: HWND) -> UiRect {
        frame_child_rect(hwnd, child).expect("the child window is shown")
    }

    unsafe fn switchbar_button_height(switchbar: HWND) -> i32 {
        (SendMessageW(switchbar, TB_GETBUTTONSIZE, 0, 0) as u32 >> 16) as i32
    }

    #[test]
    fn the_switchbar_wraps_onto_rows_that_the_frame_lays_out() {
        unsafe {
            with_test_frame(600, 400, |hwnd| {
                let room_ids = (0..8)
                    .map(|index| format!("!room{index}:example"))
                    .collect::<Vec<_>>();
                for (index, room_id) in room_ids.iter().enumerate() {
                    state_mut(hwnd).model.rooms.push(room(
                        room_id,
                        &format!("Room {index}"),
                        false,
                    ));
                }
                sync_views_and_tree(hwnd).unwrap();

                let controls = &state_ref(hwnd).controls;
                let switchbar = controls.switchbar;
                let rows = SendMessageW(switchbar, TB_GETROWS, 0, 0) as u32;
                assert!(rows >= 2, "9 buttons of 120 px wrap in 600 px: {rows}");
                let bar = frame_child_layout(hwnd, switchbar);
                assert_eq!(
                    bar.height,
                    switchbar::switchbar_height(rows, switchbar_button_height(switchbar))
                );
                let mdi_client = frame_child_layout(hwnd, controls.mdi_client);
                assert_eq!(mdi_client.y, bar.y + bar.height);
                assert_eq!(frame_child_layout(hwnd, controls.tree).y, mdi_client.y);
                for index in 0..room_ids.len() + 1 {
                    let mut button = RECT::default();
                    SendMessageW(
                        switchbar,
                        TB_GETITEMRECT,
                        index,
                        &mut button as *mut RECT as LPARAM,
                    );
                    assert_eq!(
                        button.right - button.left,
                        state_ref(hwnd)
                            .settings
                            .settings()
                            .switchbar
                            .buttons
                            .pixels()
                    );
                    assert!(button.right <= bar.width, "button {index} is clipped");
                }

                // Activating a view checks its button and keeps the wrap
                // the layout set on the last button of each row.
                let wrapped = |hwnd: HWND| {
                    (0..room_ids.len() + 1)
                        .filter(|index| {
                            let command_id = SWITCHBAR_COMMAND_FIRST + index;
                            SendMessageW(
                                state_ref(hwnd).controls.switchbar,
                                TB_GETSTATE,
                                command_id,
                                0,
                            ) as u32
                                & TBSTATE_WRAP
                                != 0
                        })
                        .collect::<Vec<_>>()
                };
                let wrapped_before = wrapped(hwnd);
                assert_eq!(wrapped_before.len(), rows as usize - 1);
                activate_view(hwnd, &room_view_id(&room_ids[7]), None).unwrap();
                assert_eq!(wrapped(hwnd), wrapped_before);

                state_mut(hwnd).model.rooms.truncate(1);
                sync_views_and_tree(hwnd).unwrap();
                let switchbar = state_ref(hwnd).controls.switchbar;
                assert_eq!(SendMessageW(switchbar, TB_GETROWS, 0, 0), 1);
                let bar = frame_child_layout(hwnd, switchbar);
                assert_eq!(bar.height, switchbar::SWITCHBAR_ROW_HEIGHT);
                assert_eq!(
                    frame_child_layout(hwnd, state_ref(hwnd).controls.mdi_client).y,
                    bar.height
                );
            });
        }
    }

    #[test]
    fn hidden_bars_give_their_place_to_the_mdi_client() {
        unsafe {
            with_test_frame(800, 600, |hwnd| {
                let bar_command = |bar, command| {
                    run_slash_command(hwnd, SlashCommand::Bar { bar, command }).unwrap()
                };
                let mut client = RECT::default();
                GetClientRect(hwnd, &mut client);
                let controls = &state_ref(hwnd).controls;
                let (switchbar, tree, mdi_client) =
                    (controls.switchbar, controls.tree, controls.mdi_client);
                let shown = dump_frame_layout(hwnd);

                assert_eq!(
                    bar_command(Bar::Switchbar, BarCommand::Off),
                    Some("switchbar: off".to_owned())
                );
                assert!(!state_ref(hwnd).settings.settings().switchbar.show);
                let layout = dump_frame_layout(hwnd);
                assert_eq!(layout.switchbar, None);
                assert_eq!(layout.mdi_client.y, 0);
                assert_eq!(layout.treebar.unwrap().y, 0);

                // A hidden switchbar still mirrors the views.
                state_mut(hwnd)
                    .model
                    .rooms
                    .push(room("!room:example", "Room", false));
                sync_views_and_tree(hwnd).unwrap();
                assert_eq!(
                    SendMessageW(switchbar, TB_BUTTONCOUNT, 0, 0) as usize,
                    state_ref(hwnd).switchbar_views.len()
                );
                assert_eq!(state_ref(hwnd).switchbar_views.len(), 2);

                assert_eq!(
                    bar_command(Bar::Treebar, BarCommand::Off),
                    Some("treebar: off".to_owned())
                );
                let layout = dump_frame_layout(hwnd);
                assert_eq!(layout.treebar, None);
                assert_eq!(
                    layout.mdi_client,
                    UiRect {
                        x: 0,
                        y: 0,
                        width: client.right,
                        height: layout.mdi_client.height,
                    }
                );
                assert_eq!(IsWindowVisible(tree), 0);

                bar_command(Bar::Switchbar, BarCommand::On);
                bar_command(Bar::Treebar, BarCommand::On);
                let layout = dump_frame_layout(hwnd);
                assert_eq!(layout, shown);
                assert_eq!(
                    frame_child_layout(hwnd, mdi_client).y,
                    layout.switchbar.unwrap().height
                );
            });
        }
    }

    #[test]
    fn bar_lines_name_the_bar() {
        assert_eq!(bar_is_line(Bar::Switchbar, true), "* Switchbar is on");
        assert_eq!(bar_is_line(Bar::Treebar, false), "* Treebar is off");
    }

    /// A real frame showing Ops: Alice's question `$q`, filler lines, and
    /// Bob's reply `$a` to `target` last, selected, with a sender whose
    /// commands wait.
    unsafe fn with_original_jump_frame(target: &str, test: impl FnOnce(HWND)) {
        with_test_frame(600, 400, |hwnd| {
            let (sender, commands) = inert_command_sender();
            std::mem::forget(commands);
            state_mut(hwnd).command_sender = Some(sender);
            state_mut(hwnd).model.rooms = vec![room("!ops:example", "Ops", false)];
            sync_views_and_tree(hwnd).unwrap();
            let mut events = vec![timeline_with_transaction_id(
                "!ops:example",
                "$q",
                None,
                "are we deploying today?",
                1_700_000_000_000,
            )];
            for index in 0..40 {
                events.push(timeline_with_transaction_id(
                    "!ops:example",
                    &format!("$filler{index}"),
                    None,
                    &format!("filler {index}"),
                    1_700_000_001_000 + index * 1_000,
                ));
            }
            events.push(timeline_with_relation_and_presentation(
                "!ops:example",
                "$a",
                None,
                Some(reply_relation(target)),
                RoomMessagePresentationKind::Text,
                "yes, after lunch",
                1_700_000_100_000,
            ));
            apply_events_and_refresh(hwnd, events).unwrap();
            activate_view(hwnd, &room_view_id("!ops:example"), None).unwrap();
            state_mut(hwnd).selected_timeline_event = Some(SelectedTimelineEvent {
                view_id: room_view_id("!ops:example"),
                event_id: "$a".to_owned(),
            });
            test(hwnd);
        });
    }

    unsafe fn ops_timeline(hwnd: HWND, view_id: &ViewId) -> HWND {
        view_timeline(view_id, state_ref(hwnd).views[view_id].content)
    }

    /// The event whose line is the timeline's top line.
    unsafe fn top_line_event_id(timeline: HWND) -> Option<String> {
        let state = timeline_state_ref(timeline);
        state.paint_lines[state.top_line]
            .source_row_index
            .and_then(|index| state.source_rows[index].event_id.clone())
    }

    fn backfill_requests(state: &WindowState) -> usize {
        state
            .recorded_commands
            .iter()
            .filter(|command| matches!(command, Command::BackfillRoomHistory(_)))
            .count()
    }

    #[test]
    fn go_to_original_scrolls_to_a_loaded_target_and_selects_it() {
        unsafe {
            with_original_jump_frame("$q", |hwnd| {
                let timeline = ops_timeline(hwnd, &room_view_id("!ops:example"));
                assert_ne!(
                    top_line_event_id(timeline).as_deref(),
                    Some("$q"),
                    "the window starts at its end"
                );
                go_to_original_of_selected_message(hwnd).unwrap();
                assert_eq!(top_line_event_id(timeline).as_deref(), Some("$q"));
                assert_eq!(selected_timeline_event_id(state_ref(hwnd)), Some("$q"));
                assert_eq!(state_ref(hwnd).pending_original_jump, None);
                assert_eq!(state_ref(hwnd).model.status, "original message found");
                assert_eq!(backfill_requests(state_ref(hwnd)), 0);
            });
        }
    }

    #[test]
    fn go_to_original_pages_history_until_the_target_arrives() {
        unsafe {
            with_original_jump_frame("$old", |hwnd| {
                go_to_original_of_selected_message(hwnd).unwrap();
                assert_eq!(
                    state_ref(hwnd).pending_original_jump,
                    Some(replies::OriginalJump {
                        view_id: room_view_id("!ops:example"),
                        target_event_id: "$old".to_owned(),
                    })
                );
                assert_eq!(backfill_requests(state_ref(hwnd)), 1);
                assert_eq!(state_ref(hwnd).model.status, FINDING_ORIGINAL_STATUS);

                // A page without it asks for the next.
                apply_events_and_refresh(
                    hwnd,
                    vec![room_history_backfill(
                        "!ops:example",
                        "t0",
                        Some("t1"),
                        vec![],
                    )],
                )
                .unwrap();
                assert!(state_ref(hwnd).pending_original_jump.is_some());
                assert_eq!(backfill_requests(state_ref(hwnd)), 2);

                apply_events_and_refresh(
                    hwnd,
                    vec![room_history_backfill(
                        "!ops:example",
                        "t1",
                        Some("t2"),
                        vec![history_backfill_event_from_event(
                            timeline_with_transaction_id(
                                "!ops:example",
                                "$old",
                                None,
                                "the old question",
                                1_699_999_000_000,
                            ),
                        )],
                    )],
                )
                .unwrap();
                let timeline = ops_timeline(hwnd, &room_view_id("!ops:example"));
                // The jump's scroll wins over the backfill anchor.
                assert_eq!(top_line_event_id(timeline).as_deref(), Some("$old"));
                assert_eq!(selected_timeline_event_id(state_ref(hwnd)), Some("$old"));
                assert_eq!(state_ref(hwnd).pending_original_jump, None);
                assert_eq!(state_ref(hwnd).model.status, "original message found");
                assert_eq!(backfill_requests(state_ref(hwnd)), 2);
            });
        }
    }

    #[test]
    fn go_to_original_stops_at_the_start_of_history_or_a_failure() {
        unsafe {
            with_original_jump_frame("$old", |hwnd| {
                go_to_original_of_selected_message(hwnd).unwrap();
                apply_events_and_refresh(
                    hwnd,
                    vec![room_history_backfill("!ops:example", "t0", None, vec![])],
                )
                .unwrap();
                assert_eq!(state_ref(hwnd).pending_original_jump, None);
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "* the original message is not in this room's history"
                );
                assert_eq!(selected_timeline_event_id(state_ref(hwnd)), Some("$a"));
            });
            with_original_jump_frame("$old", |hwnd| {
                go_to_original_of_selected_message(hwnd).unwrap();
                apply_events_and_refresh(
                    hwnd,
                    vec![Event::Error(CoreErrorEvent {
                        operation: Operation::BackfillRoomHistory,
                        room_id: Some("!ops:example".to_owned()),
                        message: "offline".to_owned(),
                    })],
                )
                .unwrap();
                assert_eq!(state_ref(hwnd).pending_original_jump, None);
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "* could not load older messages: offline"
                );
                // A failure left from before is retried by the next jump.
                go_to_original_of_selected_message(hwnd).unwrap();
                assert!(state_ref(hwnd).pending_original_jump.is_some());
                assert_eq!(backfill_requests(state_ref(hwnd)), 2);
            });
        }
    }

    #[test]
    fn scrolling_the_window_drops_its_pending_go_to_original() {
        unsafe {
            with_original_jump_frame("$old", |hwnd| {
                go_to_original_of_selected_message(hwnd).unwrap();
                let timeline = ops_timeline(hwnd, &room_view_id("!ops:example"));
                SendMessageW(timeline, WM_VSCROLL, SB_LINEUP as WPARAM, 0);
                assert_eq!(state_ref(hwnd).pending_original_jump, None);
                // The page it asked for arrives and changes nothing.
                let top_line = timeline_state_ref(timeline).top_line;
                apply_events_and_refresh(
                    hwnd,
                    vec![room_history_backfill("!ops:example", "t0", None, vec![])],
                )
                .unwrap();
                assert_ne!(top_line_event_id(timeline).as_deref(), Some("$old"));
                assert!(timeline_state_ref(timeline).top_line >= top_line);
            });
        }
    }

    #[test]
    fn go_to_original_from_a_thread_shows_a_target_outside_it_in_the_room() {
        unsafe {
            with_original_jump_frame("$q", |hwnd| {
                let thread = thread_view_id("!ops:example", "$filler0");
                apply_events_and_refresh(
                    hwnd,
                    vec![timeline_with_relation_and_presentation(
                        "!ops:example",
                        "$t1",
                        None,
                        Some(RoomActivityRelation::Thread {
                            thread_root_event_id: "$filler0".to_owned(),
                            in_reply_to_event_id: Some("$q".to_owned()),
                            is_falling_back: false,
                        }),
                        RoomMessagePresentationKind::Text,
                        "in the thread",
                        1_700_000_200_000,
                    )],
                )
                .unwrap();
                activate_view(hwnd, &thread, None).unwrap();
                state_mut(hwnd).selected_timeline_event = Some(SelectedTimelineEvent {
                    view_id: thread.clone(),
                    event_id: "$t1".to_owned(),
                });
                assert!(active_message_action_state(state_ref(hwnd)).go_to_original_enabled);
                go_to_original_of_selected_message(hwnd).unwrap();
                assert_eq!(state_ref(hwnd).active_view, room_view_id("!ops:example"));
                assert_eq!(selected_timeline_event_id(state_ref(hwnd)), Some("$q"));
                let timeline = ops_timeline(hwnd, &room_view_id("!ops:example"));
                assert_eq!(top_line_event_id(timeline).as_deref(), Some("$q"));
            });
        }
    }

    #[test]
    fn go_to_original_is_offered_only_for_a_reply_with_a_quote_line() {
        let mut state = reply_quote_state("$q");
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$t", "$q", "thread reply"),
        )
        .unwrap();
        state.active_view = room_view_id("!ops:example");
        let enabled = |state: &mut WindowState, view_id: ViewId, event_id: &str| {
            state.active_view = view_id.clone();
            state.selected_timeline_event = Some(SelectedTimelineEvent {
                view_id,
                event_id: event_id.to_owned(),
            });
            active_message_action_state(state).go_to_original_enabled
        };
        assert!(enabled(&mut state, room_view_id("!ops:example"), "$a"));
        assert!(!enabled(&mut state, room_view_id("!ops:example"), "$q"));
        // A thread message replying to its root has no quote line.
        assert!(!enabled(
            &mut state,
            thread_view_id("!ops:example", "$q"),
            "$t"
        ));
    }
}
