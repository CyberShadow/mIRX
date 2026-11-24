#![allow(unsafe_op_in_unsafe_fn)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::{OsStr, c_void};
use std::fs;
use std::io::ErrorKind;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::ptr::{copy_nonoverlapping, null, null_mut};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, mpsc::RecvTimeoutError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(any(test, feature = "automation"))]
use mirx_core::MemberPowerRole;
use mirx_core::{
    AcceptRoomInviteRequest, BackfillRoomHistoryRequest, BanRoomMemberRequest, Command, CommandId,
    CommandSender, ComposeFormat, CoreConfig, CoreErrorEvent, CoreHandle, CreateDirectRoomRequest,
    CreateRoomRequest, CreateRoomVisibility, E2eeBackupState, E2eeBackupStatus,
    E2eeCrossSigningStatus, E2eeIdentityResetOutcome, E2eeRecoveryState, E2eeSessionState,
    E2eeStatus, EditMessageRequest, Event, ExportRoomKeysRequest, FetchRoomEventRequest,
    FetchRoomMediaRequest, HighlightWord, HistoryFrom, ImportRoomKeysRequest,
    InviteRoomMemberRequest, JoinRoomRequest, KickRoomMemberRequest, LeaveRoomRequest, LoginMethod,
    LoginRequest, LogoutRefusal, LogoutRequest, LogoutScope, MarkRoomReadRequest, Operation,
    PresenceKind, PublicRoomsPage, ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT, RecoverE2eeRequest,
    RedactEventRequest, RejectRoomInviteRequest, RenameDeviceRequest, RequestVerificationRequest,
    ResetE2eeIdentityRequest, RestoreSessionRequest, RetryDecryptionRequest, RoomActivity,
    RoomActivityKind, RoomActivityRelation, RoomActivityUpdate, RoomActivityUpdateKind,
    RoomActivityUpdateTarget, RoomAudioInfo, RoomEventFetch, RoomEventFetchResult,
    RoomEventsDecrypted, RoomFileInfo, RoomHistoryBackfill, RoomHistoryBackfillEvent,
    RoomImageInfo, RoomKeysAtRisk, RoomMedia, RoomMediaFormat, RoomMediaKind, RoomMediaSource,
    RoomMemberSummary, RoomMembers, RoomMembership, RoomMessageActivity,
    RoomMessagePresentationKind, RoomMessageSendState, RoomNotificationMode, RoomPollActivity,
    RoomPollContent, RoomReadState, RoomRedactedActivity, RoomStateNoticeContent, RoomSummary,
    RoomTyping, RoomUserProfile, RoomVideoInfo, SendFileRequest, SendMessageReplyTarget,
    SendMessageRequest, SendMessageThreadTarget, SendReactionRequest, SessionAuth,
    SetDisplayNameRequest, SetReadMarkerRequest, SetRoomNotificationModeRequest,
    SetRoomTopicRequest, SetupE2eeRecoveryRequest, SignOutDeviceRequest, StoredSession,
    TextStyleSpan, UnbanRoomMemberRequest, UpdateRoomSettingsRequest, UserDevices, UserLinkSpan,
    UserProfile, VerificationDirection, VerificationEmoji, VerificationFlowId, VerificationRequest,
    VerificationState, VerificationStateEvent, login_method, login_server, start,
};
#[cfg(any(test, feature = "automation"))]
use serde::Serialize;
#[cfg(any(test, feature = "automation"))]
use serde_json::{Value, json};
#[cfg(test)]
use tokio::sync::mpsc::unbounded_channel;
use windows_sys::Win32::Foundation::S_OK;
#[cfg(any(test, feature = "automation"))]
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Foundation::{
    FILETIME, GetLastError, GlobalFree, HWND, LPARAM, LRESULT, POINT, RECT, SYSTEMTIME, WPARAM,
};
#[cfg(any(test, feature = "automation"))]
use windows_sys::Win32::Graphics::Gdi::GetTextFaceW;
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, ClientToScreen,
    CreateFontIndirectW, CreateSolidBrush, DEFAULT_CHARSET, DeleteObject, DrawFocusRect, EndPaint,
    FW_BOLD, FW_NORMAL, FillRect, FrameRect, GetDC, GetSysColor, GetSysColorBrush, GetTextMetricsW,
    HBRUSH, HFONT, InvalidateRect, LOGFONTW, MapWindowPoints, PAINTSTRUCT, ReleaseDC,
    ScreenToClient, SelectObject, SetBkMode, SetTextColor, TEXTMETRICW, TRANSPARENT, TextOutW,
};
use windows_sys::Win32::Graphics::GdiPlus::{
    GdipCreateFromHDC, GdipDeleteGraphics, GdipDisposeImage, GdipDrawImageRectI,
    GdipGetImageHeight, GdipGetImageWidth, GdipLoadImageFromStream, GdiplusShutdown,
    GdiplusStartup, GdiplusStartupInput, GpGraphics, GpImage,
};
use windows_sys::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows_sys::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
};
#[cfg(any(test, feature = "automation"))]
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(any(test, feature = "automation"))]
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
    PAGE_READWRITE, UnmapViewOfFile,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use windows_sys::Win32::UI::Controls::Dialogs::{
    CF_INITTOLOGFONTSTRUCT, CF_LIMITSIZE, CF_NOVERTFONTS, CF_SCREENFONTS, CHOOSEFONTW, ChooseFontW,
    CommDlgExtendedError,
};
use windows_sys::Win32::UI::Controls::{
    BTNS_CHECK, BTNS_GROUP, BTNS_SHOWTEXT, CCS_NOPARENTALIGN, CCS_NORESIZE, CDDS_ITEMPREPAINT,
    CDDS_PREPAINT, CDRF_NOTIFYITEMDRAW, DRAWITEMSTRUCT, EM_GETSEL, EM_REPLACESEL, EM_SETSEL,
    ICC_BAR_CLASSES, ICC_LISTVIEW_CLASSES, ICC_TAB_CLASSES, ICC_TREEVIEW_CLASSES,
    INITCOMMONCONTROLSEX, InitCommonControlsEx, LVCF_FMT, LVCF_TEXT, LVCF_WIDTH, LVCFMT_LEFT,
    LVCFMT_RIGHT, LVCOLUMNW, LVIF_TEXT, LVIS_FOCUSED, LVIS_SELECTED, LVITEMW, LVM_GETNEXTITEM,
    LVM_INSERTCOLUMNW, LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMCOUNT, LVM_SETITEMSTATE,
    LVN_COLUMNCLICK, LVN_GETDISPINFOW, LVNI_SELECTED, LVS_EX_FULLROWSELECT, LVS_OWNERDATA,
    LVS_REPORT, LVS_SHOWSELALWAYS, LVS_SINGLESEL, LVSICF_NOSCROLL, MEASUREITEMSTRUCT,
    NM_CUSTOMDRAW, NM_DBLCLK, NM_RETURN, NMHDR, NMITEMACTIVATE, NMLISTVIEW, NMLVDISPINFOW,
    NMTBCUSTOMDRAW, NMTREEVIEWW, NMTVCUSTOMDRAW, ODS_FOCUS, ODS_SELECTED, SB_SETTEXTW,
    SBARS_SIZEGRIP, STATUSCLASSNAMEW, SetScrollInfo, TB_ADDBUTTONSW, TB_BUTTONCOUNT,
    TB_BUTTONSTRUCTSIZE, TB_CHECKBUTTON, TB_DELETEBUTTON, TB_GETBUTTONSIZE, TB_GETROWS,
    TB_SETBITMAPSIZE, TB_SETBUTTONWIDTH, TB_SETEXTENDEDSTYLE, TB_SETMAXTEXTROWS, TBBUTTON,
    TBSTATE_CHECKED, TBSTATE_ENABLED, TBSTYLE_EX_MIXEDBUTTONS, TBSTYLE_FLAT, TBSTYLE_WRAPABLE,
    TOOLBARCLASSNAMEW, TVC_BYKEYBOARD, TVC_BYMOUSE, TVE_EXPAND, TVGN_CARET, TVGN_CHILD, TVGN_NEXT,
    TVGN_PARENT, TVGN_ROOT, TVHITTESTINFO, TVI_LAST, TVI_ROOT, TVIF_IMAGE, TVIF_SELECTEDIMAGE,
    TVIF_TEXT, TVINSERTSTRUCTW, TVINSERTSTRUCTW_0, TVIS_EXPANDED, TVITEMEXW, TVM_DELETEITEM,
    TVM_EXPAND, TVM_GETITEMRECT, TVM_GETITEMSTATE, TVM_GETNEXTITEM, TVM_HITTEST, TVM_INSERTITEMW,
    TVM_SELECTITEM, TVN_ITEMEXPANDEDW, TVN_SELCHANGEDW, TVS_HASBUTTONS, TVS_HASLINES,
    TVS_LINESATROOT,
};
#[cfg(any(test, feature = "automation"))]
use windows_sys::Win32::UI::Controls::{
    HDM_GETITEMCOUNT, HDM_GETITEMRECT, LVIR_LABEL, LVM_GETCOLUMNW, LVM_GETCOUNTPERPAGE,
    LVM_GETHEADER, LVM_GETITEMCOUNT, LVM_GETITEMRECT, LVM_GETITEMTEXTW, LVM_GETTOPINDEX,
    TB_GETITEMRECT, TVE_COLLAPSE, TVM_GETITEMW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, GetKeyState, SetFocus, VK_CONTROL, VK_MENU, VK_SHIFT, VK_TAB,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ACCEL, CreateAcceleratorTableW, DestroyAcceleratorTable, FALT, FCONTROL, FSHIFT, FVIRTKEY,
    HACCEL, TranslateAcceleratorW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, AppendMenuW, BM_GETCHECK, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_PUSHBUTTON,
    CLIENTCREATESTRUCT, CREATESTRUCTW, CS_DBLCLKS, CW_USEDEFAULT, CallWindowProcW, CheckMenuItem,
    CheckMenuRadioItem, CreateMenu, CreatePopupMenu, CreateWindowExW, DefFrameProcW,
    DefMDIChildProcW, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, DrawMenuBar,
    ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE, ES_PASSWORD, ES_READONLY, EnableMenuItem,
    FLASHW_TIMERNOFG, FLASHW_TRAY, FLASHWINFO, FlashWindowEx, GA_ROOT, GWL_STYLE, GWLP_WNDPROC,
    GetAncestor, GetClassNameW, GetClientRect, GetDlgItem, GetForegroundWindow, GetMessageW,
    GetParent, GetScrollInfo, GetSystemMenu, GetWindowLongPtrW, GetWindowRect, HMENU, HideCaret,
    IDC_ARROW, IDYES, IsIconic, IsWindow, IsWindowVisible, IsZoomed, KillTimer, LB_ADDSTRING,
    LB_ERR, LB_GETCARETINDEX, LB_GETSEL, LB_GETSELCOUNT, LB_GETSELITEMS, LB_GETTEXT, LB_GETTEXTLEN,
    LB_GETTOPINDEX, LB_ITEMFROMPOINT, LB_RESETCONTENT, LB_SETCARETINDEX, LB_SETITEMHEIGHT,
    LB_SETSEL, LB_SETTOPINDEX, LBN_DBLCLK, LBS_EXTENDEDSEL, LBS_HASSTRINGS, LBS_NOINTEGRALHEIGHT,
    LBS_NOTIFY, LBS_OWNERDRAWFIXED, LoadCursorW, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION,
    MB_OK, MB_YESNO, MDICREATESTRUCTW, MDITILE_HORIZONTAL, MDITILE_VERTICAL, MESSAGEBOX_STYLE,
    MF_BYCOMMAND, MF_CHECKED, MF_ENABLED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING,
    MF_UNCHECKED, MSG, MessageBoxW, MoveWindow, PostMessageW, PostQuitMessage, RegisterClassExW,
    SB_BOTTOM, SB_LINEDOWN, SB_LINEUP, SB_PAGEDOWN, SB_PAGEUP, SB_THUMBPOSITION, SB_THUMBTRACK,
    SB_TOP, SB_VERT, SC_CLOSE, SC_MINIMIZE, SCROLLINFO, SHOW_WINDOW_CMD, SIF_ALL, SIF_PAGE,
    SIF_POS, SIF_RANGE, SIF_TRACKPOS, SIZE_MINIMIZED, SW_HIDE, SW_SHOW, SW_SHOWMINNOACTIVE,
    SendMessageW, SetForegroundWindow, SetMenu, SetTimer, SetWindowLongPtrW, ShowWindow,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMDISysAccel, TranslateMessage,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_ACTIVATEAPP, WM_APP, WM_CHAR, WM_CLOSE, WM_COMMAND,
    WM_CONTEXTMENU, WM_CREATE, WM_DESTROY, WM_DRAWITEM, WM_GETFONT, WM_GETTEXT, WM_GETTEXTLENGTH,
    WM_INITMENUPOPUP, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_MDIACTIVATE, WM_MDICASCADE,
    WM_MDICREATE, WM_MDIDESTROY, WM_MDIGETACTIVE, WM_MDIICONARRANGE, WM_MDIMAXIMIZE, WM_MDIRESTORE,
    WM_MDITILE, WM_MEASUREITEM, WM_NCCREATE, WM_NCDESTROY, WM_NOTIFY, WM_PAINT, WM_PASTE,
    WM_SETFOCUS, WM_SETFONT, WM_SETREDRAW, WM_SETTEXT, WM_SIZE, WM_SYSCOMMAND, WM_TIMER,
    WM_VSCROLL, WNDCLASSEXW, WNDPROC, WS_BORDER, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN,
    WS_EX_CLIENTEDGE, WS_MAXIMIZE, WS_OVERLAPPED, WS_OVERLAPPEDWINDOW, WS_SYSMENU, WS_TABSTOP,
    WS_VISIBLE, WS_VSCROLL,
};
#[cfg(any(test, feature = "automation"))]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetMenu, GetMenuItemInfoW, MENUITEMINFOW, MFS_CHECKED, MFS_DISABLED, MFT_BITMAP, MFT_SEPARATOR,
    MIIM_FTYPE, MIIM_STATE, MIIM_STRING, MIIM_SUBMENU,
};
#[cfg(any(test, feature = "automation"))]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetMenuItemCount, GetMenuState, GetQueueStatus, LB_GETITEMRECT, PM_QS_POSTMESSAGE, PM_REMOVE,
    PeekMessageW, QS_ALLPOSTMESSAGE, SW_MINIMIZE, WM_COPYDATA, WM_QUIT,
};

use self::app_icon::{IconSize, app_icon};
use self::command_acks::AnsweredCommands;
use self::dpi::Dpi;
use self::glyph_fonts::GlyphFonts;
use self::harness::Harness;
use self::script::ScriptRuns;
use crate::shared::{
    ConnectionState, LifecycleState, MdiArrangement, ReactionSummary, ViewActivityLevel,
};
use crate::{AppCommand, FrameSize, HELP};
// The automation harness: its protocol, transport, dump and self-test.
#[cfg(any(test, feature = "automation"))]
use crate::automation::{
    AcceptedResponse, AutomationDialog, COMMAND_ID_DUMP_COMPOSE_TEXT,
    COMMAND_ID_DUMP_ROOM_ACTIVITY, COMMAND_ID_LOAD_SYNTHETIC_STATE, COMMAND_ID_PING,
    COMMAND_ID_WAIT_IDLE, ComposeKey, ComposeTextResponse, ContextMenuCommand,
    DumpE2eeRecoverySetupResult, DumpIdentityWarning, DumpIdentityWarningKind, DumpMemberMenuState,
    DumpMemberModerationAction, DumpMemberPowerRole, DumpMembershipCommandAction,
    DumpMessageMenuState, DumpModel, DumpPowerLevelChange, DumpRoomActivity, DumpRoomActivityKind,
    DumpRoomActivityResponse, DumpRoomMenuState, DumpRoomMessageActivityLine,
    DumpRoomPreviewMediaState, DumpSubmittedAwayCommand, DumpSubmittedDirectRoomCommand,
    DumpSubmittedEditCommand, DumpSubmittedFileCommand, DumpSubmittedHistoryCommand,
    DumpSubmittedMemberModerationCommand, DumpSubmittedMembershipCommand,
    DumpSubmittedMessageCommand, DumpSubmittedNotificationCommand, DumpSubmittedProfileCommand,
    DumpSubmittedReactionCommand, DumpSubmittedRecoveryCommand, DumpSubmittedRedactionCommand,
    DumpSubmittedReplyCommand, DumpSubmittedReplyThreadTarget, DumpSubmittedRoomListRequest,
    DumpSubmittedRoomSettingsCommand, DumpSubmittedRoomTopicCommand, DumpSubmittedTypingCommand,
    DumpSubmittedUserCommand, DumpSubmittedVerificationAction, DumpSubmittedVerificationRequest,
    DumpUserCommandAction, DumpVerificationActionKind, DumpVerificationMenuState, FailureEnvelope,
    FindActiveViewResponse, GetVersionResponse, IdleProbe, ListRoomsResponse, LoadEventsResponse,
    MdiState, MdiView, MdiWindowPlacement, MdiWindowState, NavigationNode, NavigationNodeKey,
    NavigationPathSegment, ParsedCommand, PongResponse, ResponseEnvelope, RoomMemberView,
    SelectRoomResponse, SelectViewResponse, SelectedNavigationPathResponse, SendTextResponse,
    StatusBarState, SwitchbarItem, SwitchbarState, UiActiveTimeline, UiComposeTarget,
    UiComposeTargetKind, UiFindHistorySearchState, UiFindState, UiFrameLayout, UiMenu, UiMenuItem,
    UiPoint, UiRect, UiRoomHistoryBackfillState, UiRoomList, UiRoomState, UiState, UiTextFont,
    UiTimelineLine, UiTimelineLineKind, UiTimelineLink, UiTimestamp, UiToolbarButton,
    VerificationActionArgs, ViewIdentity, WaitIdleResponse, dump_error_from_core,
    dump_room_activity_from_activity, dump_room_from_summary,
    dump_room_message_send_state_from_core, dump_ui_room_read_state_from_core, failure_response,
    parse_command_request, success_response,
};

mod account;
mod address_book;
mod address_book_dialog;
mod aliases;
mod aliases_dialog;
mod app_icon;
#[cfg(test)]
mod app_resources;
mod attachments;
#[cfg(any(test, feature = "automation"))]
mod automation_dump;
#[cfg(any(test, feature = "automation"))]
mod automation_host;
mod channel_central;
mod channel_central_dialog;
mod clipboard;
mod cmap;
mod color_drop_down;
mod colors;
mod colors_dialog;
mod command_acks;
mod command_runner;
mod commands;
mod compose;
mod connection;
mod core_events;
mod crash;
mod dialogs;
mod dpi;
mod echo;
mod errors;
mod file_dialogs;
mod find;
mod fonts;
mod frame;
mod frame_menus;
mod glyph_fonts;
mod glyph_runs;
#[cfg(any(test, feature = "automation"))]
mod harness;
#[cfg(not(any(test, feature = "automation")))]
#[path = "win32/no_harness.rs"]
mod harness;
mod hotlinks;
mod identity;
mod ignore;
mod images;
mod line_edits;
mod line_marker;
mod list_view;
mod logging;
mod mdi;
mod menus;
mod navigation;
mod nick_colors;
mod nick_completion;
mod nick_list;
mod notifications;
mod notify;
mod open_link;
mod options;
mod options_dialog;
mod perform;
mod perform_dialog;
mod presence;
mod reactions;
mod read_receipts;
mod reconnect;
mod replies;
mod room_list;
mod room_list_window;
mod room_model;
mod room_targets;
mod save_buffer;
mod script;
#[cfg(any(test, feature = "automation"))]
mod self_test;
mod send_file;
mod sessions;
mod sessions_dialog;
mod settings;
mod splitter;
mod status_log;
mod switchbar;
#[cfg(test)]
mod test_fixtures;
mod text_selection;
mod timeline_layout;
mod timeline_rows;
mod timeline_text;
mod timeline_window;
mod timestamp;
mod toolbar;
mod tray;
mod typing;
mod verification;
mod views;
mod window_buffer;
mod window_data;
mod window_icons;

#[cfg(any(test, feature = "automation"))]
use account::submit_restore_session;
use account::{
    CRASH_LOG_FILE_NAME, PROFILE_SUBDIR, delete_saved_session, load_or_create_store_passphrase,
    profile_dir, save_session, session_path, submit_cancel_identity_reset, submit_device_rename,
    submit_device_sign_out, submit_identity_reset, submit_login, submit_logout,
    submit_recover_e2ee, submit_restore_saved_session, submit_room_key_transfer,
    submit_setup_e2ee_recovery, submit_start_sync,
};
#[cfg(test)]
use account::{SAVED_SESSION_VERSION, SESSION_FILE_NAME, SavedSession, load_saved_session};
use address_book::AddressBookTab;
use aliases::{ALIASES_FILE_NAME, AliasStore};
use attachments::{
    PendingAttachmentSave, apply_attachment_save_media_event, save_selected_message_attachment,
};
#[cfg(any(test, feature = "automation"))]
use automation_dump::{
    active_view_identity, dump_member_menu_state, dump_message_menu_state, dump_model_from_state,
    dump_room_activity_entry_from_room_entry, dump_room_menu_state, dump_ui_state,
    dump_verification_menu_state,
};
#[cfg(test)]
use automation_dump::{dump_frame_layout, ui_rect};
#[cfg(any(test, feature = "automation"))]
use automation_host::{handle_copydata, send_automation_request};
use colors::{ColorItem, ColorScheme, EventItem, SchemeBrushes};
use command_runner::clear_window;
#[cfg(test)]
use command_runner::run_slash_command;
use command_runner::{
    answer_view, channel_central_room, devices_lines, echo_in_window, echo_lines_in_front_window,
    echo_lines_in_view, echo_view, enqueue_command, known_nick, open_room_link, own_server,
    push_lines_in_view, report_command_error, report_error_line, resolve_invite_command_room_id,
    resolve_joined_room_target, run_local_command, send_edit, send_reply, set_local_command_status,
    submit_accept_room_invite, submit_accept_slash_command, submit_join_room,
    submit_part_slash_command, submit_query_slash_command, submit_redact_slash_command,
    submit_reject_slash_command, submit_room_notification_mode, whois_lines,
};
use commands::{
    AliasCommand, BarCommand, CommandError, ComposeInput, ComposeSubmission, CreateRoomArgs,
    E2eeSlashCommand, EchoCommand, EchoPlace, FontCommand, LogCommand, NickColorCommand,
    NickColorTarget, NotifyCommand, RoomKeyTransfer, SlashCommand, TimestampCommand,
    VerificationAction, classify_input, local_command_history_entry,
};
#[cfg(any(test, feature = "automation"))]
use compose::automation::{dump_compose_text, send_compose_key, submit_compose_from_control};
#[cfg(any(test, feature = "automation"))]
use compose::set_compose_text;
#[cfg(test)]
use compose::submit_compose_from_edit;
use compose::{
    ComposeHistory, ComposeNickCompletion, active_compose_history, arm_compose_target,
    arm_compose_target_on_selected_message, armed_compose_target_row, away_notice, away_view,
    compose_edit, disarm_vanished_compose_target, enqueue_send_message, message_text_from_input,
    prefill_react_to_selected_message, replace_edit_selection, resolve_compose_destination,
    run_send_file_dialog, send_file_room_id, send_room_message, subclass_compose_edit,
    submit_redact_selected_message, submit_send_file,
};
use connection::{ConnectAction, DisconnectAction};
#[cfg(test)]
use core_events::apply_core_event;
#[cfg(any(test, feature = "automation"))]
use core_events::automation::{apply_event_load, resolve_timeline_room_id};
use core_events::{
    announce_notify_changes, handle_core_events, handle_view_activation, operation_name,
    submit_mark_room_read, submit_set_read_marker,
};
#[cfg(any(test, feature = "automation"))]
use core_events::{apply_events_and_refresh, drain_core_events_and_refresh};
#[cfg(test)]
use dialogs::{
    ABOUT_DIALOG_OK_ID, SINGLE_FIELD_DIALOG_CLASS_NAME, SingleFieldDialogState, create_about_dialog,
};
use dialogs::{
    create_child_window, pack_size_lparam, register_dialog_classes, room_keys_dialog_title,
    run_colors_dialog, run_modal_dialog, run_options_dialog, show_about_dialog,
    show_e2ee_recovery_key, show_log_out_dialog, show_login_dialog, show_recover_e2ee_dialog,
    show_reset_e2ee_dialog, show_room_keys_dialog, show_setup_e2ee_dialog,
    show_sign_out_session_dialog, show_single_field_dialog,
};
use echo::{EchoLine, EchoLog};
use errors::{ErrorDestination, error_destination};
use file_dialogs::FileDialog;
#[cfg(any(test, feature = "automation"))]
use find::automation::{dump_active_find_state, find_in_active_view};
#[cfg(test)]
use find::build_view_find_state;
use find::{
    ViewFindState, active_view_find_feedback, apply_find_to_active_view, begin_find_dialog,
    continue_timeline_find_history_search_after_refresh, execute_view_find,
    pending_timeline_find_history_views, recompute_find_for_view,
    scroll_timeline_to_source_row_index,
};
use fonts::{
    OwnedFont, TextFont, apply_font_setting, font_line_height, run_font_dialog, set_timeline_font,
    set_view_text_font,
};
#[cfg(any(test, feature = "automation"))]
use frame::frame_status_text;
#[cfg(test)]
use frame::window_class_is;
use frame::{
    apply_color_scheme, apply_nick_colors, create_main_window, default_window_proc,
    frame_child_rect, frame_window_for, init_common_controls, layout_controls, message_loop,
    redraw_every_window, refresh_status_bar, register_frame_class, set_switchbar_button_width,
    show_nick_colors,
};
#[cfg(test)]
use frame_menus::active_message_action_state;
#[cfg(any(test, feature = "automation"))]
use frame_menus::automation::{
    context_menu_command_id, invoke_menu_command_for_automation,
    invoke_navigation_context_menu_for_automation, invoke_timeline_context_menu_for_automation,
    open_dialog_for_automation, select_timeline_event_for_automation, window_menu_command_id,
};
use frame_menus::{
    DoubleClick, active_room_action_state, append_menu_text, connect,
    context_menu_screen_point_for_lparam, create_frame_menu, default_context_point_for_window,
    disconnect, double_click_action, frame_menu_item_enabled, handle_menu_command,
    handle_switchbar_command, handle_window_key_command, log_out_warning, refresh_menu_popup,
    refresh_room_menu_state, run_room_keys_dialogs, selected_message_quote_target,
    selected_timeline_event_id, selected_timeline_event_id_for_automation,
    selected_timeline_media_attachment, show_context_menu, show_recover_e2ee_menu_dialog,
    show_reset_e2ee_menu_dialog, show_timeline_message_context_menu, timeline_row_is_selected,
};
#[cfg(any(test, feature = "automation"))]
use frame_menus::{
    active_message_action_state_for_automation, log_out, refresh_message_menu_state,
    refresh_tools_menu_state,
};
use identity::IdentityWarning;
#[cfg(test)]
use images::room_preview_media_storage_key;
use images::{
    GdiplusImage, GdiplusSession, apply_room_media_event, can_fetch_room_media,
    collect_lazy_room_preview_media_requests, dispatch_lazy_room_preview_media_requests,
    initialize_gdiplus_state, prune_decoded_images, timeline_thumbnail_format,
    timeline_video_preview_source,
};
use logging::{
    ViewLog, decide_auto_log, log_view, logging_is_line, logging_item_checked, set_view_logging,
    view_log_path, write_view_log,
};
use menus::{
    ACCEL_ID_LINE_MARKER, ACCEL_ID_NEXT_CHANNEL, ACCEL_ID_NEXT_QUERY, ACCEL_ID_TOGGLE_MAXIMIZE,
    ACCEL_ID_TREEBAR_FOCUS, ACCEL_ID_WINDOW_FIRST, ACCEL_ID_WINDOW_LAST, ACCEL_ID_WINDOW_NEXT,
    ACCEL_ID_WINDOW_PREVIOUS, BarMenu, COMMANDS_MENU, FILE_MENU, HELP_MENU, MEMBER_CONTROL_MENU,
    MEMBER_MENU, MENU_BAR, MENU_ID_COMMANDS_FIRST, MENU_ID_COMMANDS_JOIN, MENU_ID_COMMANDS_LIST,
    MENU_ID_EXPORT_ROOM_KEYS, MENU_ID_FILE_CONNECT, MENU_ID_FILE_DISCONNECT, MENU_ID_FILE_EXIT,
    MENU_ID_FILE_SELECT_SERVER, MENU_ID_FIND, MENU_ID_HELP_ABOUT, MENU_ID_HELP_TOPICS,
    MENU_ID_IMPORT_ROOM_KEYS, MENU_ID_LOGOUT, MENU_ID_MEMBER_ACCEPT_IDENTITY, MENU_ID_MEMBER_BAN,
    MENU_ID_MEMBER_DEOP, MENU_ID_MEMBER_DEVICES, MENU_ID_MEMBER_DEVOICE, MENU_ID_MEMBER_IGNORE,
    MENU_ID_MEMBER_KICK, MENU_ID_MEMBER_MENTION, MENU_ID_MEMBER_OP, MENU_ID_MEMBER_QUERY,
    MENU_ID_MEMBER_UNIGNORE, MENU_ID_MEMBER_VERIFY, MENU_ID_MEMBER_VOICE, MENU_ID_MEMBER_WHOIS,
    MENU_ID_MESSAGE_EDIT, MENU_ID_MESSAGE_GO_TO_ORIGINAL, MENU_ID_MESSAGE_REACT,
    MENU_ID_MESSAGE_REDACT, MENU_ID_MESSAGE_REPLY, MENU_ID_MESSAGE_SAVE_ATTACHMENT,
    MENU_ID_RECOVER_E2EE, MENU_ID_RESET_E2EE, MENU_ID_ROOM_ACCEPT_INVITE,
    MENU_ID_ROOM_CHANNEL_CENTRAL, MENU_ID_ROOM_NOTIFY_ALL, MENU_ID_ROOM_NOTIFY_MENTIONS,
    MENU_ID_ROOM_NOTIFY_MUTE, MENU_ID_ROOM_PART, MENU_ID_ROOM_REJECT_INVITE,
    MENU_ID_SETUP_E2EE_RECOVERY, MENU_ID_TOOLS_ADDRESS_BOOK, MENU_ID_TOOLS_ALIASES,
    MENU_ID_TOOLS_COLORS, MENU_ID_TOOLS_OPTIONS, MENU_ID_TOOLS_SESSIONS, MENU_ID_VERIFY_ACCEPT,
    MENU_ID_VERIFY_CANCEL, MENU_ID_VERIFY_CONFIRM_SAS, MENU_ID_VERIFY_REJECT_SAS,
    MENU_ID_VERIFY_THIS_SESSION, MENU_ID_VIEW_SWITCHBAR, MENU_ID_VIEW_TOOLBAR,
    MENU_ID_VIEW_TREEBAR, MENU_ID_WINDOW_ARRANGE_ICONS, MENU_ID_WINDOW_CASCADE,
    MENU_ID_WINDOW_TILE_HORIZONTAL, MENU_ID_WINDOW_TILE_VERTICAL, MemberMenuAction,
    MemberMenuContext, MenuEntry, ROOM_NOTIFICATIONS_MENU, TOOLBAR_ID_SEND_FILE, TOOLS_MENU,
    VIEW_MENU, WINDOW_MENU, WindowKey, commands_menu_item, commands_menu_line, commands_menu_title,
    file_menu_state, member_menu_action, member_menu_item_ids, notification_menu_id,
    notification_menu_mode, notification_mode_label, verification_menu_action_enabled,
    view_menu_bar, view_menu_checks, window_key,
};
#[cfg(any(test, feature = "automation"))]
use navigation::automation::{
    dump_navigation_tree, dump_selected_navigation_path, read_tree_item_text,
    select_navigation_path, select_navigation_view, set_navigation_expanded, view_id_from_identity,
    view_identity_from_view_id,
};
#[cfg(test)]
use navigation::navigation_layout;
use navigation::{
    NavItem, NavNode, ROOM_LIST_TITLE, SwitchbarFace, TreeItem, cycled_window, expand_tree_item,
    handle_tree_selection, insert_tree_item, nth_window, ordered_view_ids,
    refresh_tree_activity_colors, select_room, select_tree_item, selected_tree_item,
    show_navigation_room_context_menu, switchbar_custom_draw, sync_switchbar, sync_views_and_tree,
    tree_custom_draw, tree_expansion_changed, view_caption, view_short_title,
};
use nick_completion::NickCompletion;
#[cfg(any(test, feature = "automation"))]
use nick_list::automation::{
    invoke_member_context_menu_for_automation, invoke_selected_member_context_menu_for_automation,
    room_member_view_rows, select_member_for_automation,
};
use nick_list::{
    ListedMember, active_view_members, draw_nick_list_item, nick_list_items,
    query_double_clicked_member, room_member_nick, run_member_menu_item, show_member_context_menu,
    sync_nick_list, text_font_item_height,
};
#[cfg(any(test, feature = "automation"))]
use nick_list::{active_member_action_state, refresh_member_menu_state};
use presence::PresenceBook;
use replies::ComposeTarget;
use room_list::{PageApplied, RoomListColumn, RoomListQuery, RoomListState};
#[cfg(any(test, feature = "automation"))]
use room_list_window::automation::dump_room_list;
use room_list_window::{
    add_room_list_columns, apply_public_rooms_page, ensure_room_list_view, handle_room_list_notify,
    refresh_room_list_rows, start_room_list,
};
#[cfg(any(test, feature = "automation"))]
use room_model::automation::validate_synthetic_room_activity_events;
use room_model::{
    BufferTrim, LastMessageActivity, ReplyTarget, ReplyTargetState, RoomHistoryBackfillState,
    RoomIndex, RoomPreviewMediaState, RoomViewModel, RoomViews, RoomWindowKind, activity_level_for,
    failed_room_activity_update_view_id, is_invite_command_target, is_invited_room,
    is_joined_message_room, is_navigable_room, is_sendable_room, joined_room_notification_mode,
    last_activity_from_room_activity, navigable_room_by_id, projected_thread_by_id,
    raise_view_activity, read_target_for_room_activity, room_activity_belongs_to_thread,
    room_activity_event_id, room_activity_sender_user_id, room_activity_target_view_id,
    room_activity_update_marks_inactive_room, room_by_id, room_window_kind, rooms_in_window_order,
    sendable_room_by_id, view_id_is_valid, view_id_is_valid_for_rooms, view_id_is_valid_in,
    window_message,
};
#[cfg(test)]
use room_model::{RoomArrivals, ThreadViewModel};
use room_targets::{RoomTargetError, resolve_room};
use save_buffer::{run_save_buffer_dialog, save_window_buffer};
#[cfg(any(test, feature = "automation"))]
use self_test::run_self_test;
use settings::{Bar, FontSetting, SETTINGS_FILE_NAME, SettingsStore};
use status_log::{StatusLineKind, StatusLog};
#[cfg(any(test, feature = "automation"))]
use timeline_layout::TimelinePaintLineVisual;
use timeline_layout::{
    TimelineMetrics, TimelinePaintLine, WrapCache, activity_level_color, paint_timeline,
    plain_timeline_paint_lines, rendered_line_ranges, timeline_row_paint_lines,
};
use timeline_rows::{
    TimelineImageState, TimelineMessageSendState, TimelineRow, TimelineRowIdentity,
    TimelineRowKind, TimelineRowLayout, build_room_timeline_rows, build_status_timeline_rows,
    build_thread_timeline_rows, can_fetch_room_events, collect_reply_target_requests,
    compose_target_row, dispatch_reply_target_fetches, project_timeline_rows, room_line_marker,
};
#[cfg(test)]
use timeline_rows::{TimelineSyntheticRowLabel, render_room_lines};
use timeline_text::{
    ENCRYPTED_ROOM_TEXT, INVITE_RESPONSE_TEXT, action_line, context_line, encryption_change_text,
    event_line, invited_text, join_rule_change_text, member_change_text, message_line,
    name_change_text, nick_label, now_talking_text, power_levels_change_text, rank_prefix,
    topic_change_text, topic_is_text,
};
#[cfg(test)]
use timeline_window::clamped_timeline_top_line;
#[cfg(any(test, feature = "automation"))]
use timeline_window::split_rendered_lines;
use timeline_window::{
    ScrollGesture, cancel_jump_for_view, capture_room_history_backfill_anchor, continue_event_jump,
    go_to_original_of_selected_message, loading_room_history_room_id, register_timeline_class,
    relayout_timeline, restore_room_history_backfill_anchor, scroll_timeline_by,
    select_timeline_event_at_client_y, set_timeline_rows, start_event_jump,
    submit_backfill_room_history_for_room, timeline_max_top_line, timeline_shows_end,
    timeline_source_row_index_for_top_line, timeline_state_mut, timeline_state_ref,
    timeline_top_line_for_source_row_index, timeline_visible_rows, update_timeline_scrollbar,
    validate_selected_timeline_event,
};
use timestamp::{LocalTime, LocalTimeFn, TimestampSetting, TimestampStyle};
#[cfg(any(test, feature = "automation"))]
use tray::automation::{choose_tray_menu_item, dump_tray};
use tray::{
    TipCandidate, TrayState, hide_to_tray, minimize_frame, mirx_is_foreground, on_taskbar_created,
    on_tray_icon, remove_tray_icon, show_tip, sync_tray_icon, taskbar_created_message,
};
#[cfg(any(test, feature = "automation"))]
use verification::automation::submit_automation_verification_action;
use verification::{
    VerificationTarget, Verifications, apply_verification_event, apply_verification_request,
    e2ee_backup_health_summary, e2ee_cross_signing_label, e2ee_local_backup_label,
    e2ee_recovery_state_label, e2ee_server_backup_label, e2ee_session_label,
    format_verification_emojis, refuse_second_verification, submit_menu_verification_action,
    submit_verification_answer, verification_action_label,
};
#[cfg(any(test, feature = "automation"))]
use views::automation::cached_view_is_alive;
#[cfg(any(test, feature = "automation"))]
use views::automation::{mdi_window_placement, set_view_window_state};
use views::{
    CloseAction, ViewRowsKey, activate_view, check_window_menu_logging, close_action,
    destroy_stale_room_views, ensure_status_view, ensure_view, layout_mdi_child, mdi_child_content,
    mdi_child_edit, refresh_every_view_content, refresh_view_content, register_mdi_child_class,
    register_room_content_class, room_content_member_list, room_content_timeline, show_view,
    sync_views_content, timeline_view, view_timeline,
};
#[cfg(test)]
use views::{ROOM_CONTENT_CLASS_NAME, take_stale_room_views};
use window_buffer::trim_window_buffers;
use window_icons::{WINDOW_ICON_SIZE, WindowIcon, create_window_icons};

const TIMELINE_CLASS_NAME: &str = "mirxWin32Timeline";
const FRAME_TITLE: &str = "mirx";
/// At 96 dpi.
const DEFAULT_FRAME_SIZE: FrameSize = FrameSize {
    width: 1024,
    height: 768,
};
const TREE_CLASS_NAME: &str = "SysTreeView32";
const EDIT_CLASS_NAME: &str = "EDIT";
const STATIC_CLASS_NAME: &str = "STATIC";
const BUTTON_CLASS_NAME: &str = "BUTTON";
const COMBOBOX_CLASS_NAME: &str = "COMBOBOX";
const COMPOSE_EDIT_STYLE: WINDOW_STYLE = WS_CHILD | WS_BORDER | ES_AUTOHSCROLL as u32;
const COMPOSE_EDIT_EX_STYLE: WINDOW_EX_STYLE = WS_EX_CLIENTEDGE;
/// A switchbar button's command id is this plus its index. Every window
/// has a button, and every joined room a window, so the range is all the
/// 16-bit `WM_COMMAND` ids left up to MDI's own child ids
/// (`CLIENTCREATESTRUCT.idFirstChild`, 50000): 8999 windows, beyond what
/// Windows' 10,000 USER handles a process lets it create (a room window
/// is eight of them).
const SWITCHBAR_COMMAND_FIRST: usize = 41001;
const SWITCHBAR_COMMAND_LAST: usize = MDI_FIRST_CHILD_ID - 1;
/// MDI numbers its children from here (`CLIENTCREATESTRUCT.idFirstChild`).
const MDI_FIRST_CHILD_ID: usize = 50000;
/// The core's events wait in `WindowState.event_queue`: posted by the
/// bridge when the queue gets its first, it starts `CORE_EVENTS_TIMER_ID`.
const WM_CORE_EVENT: u32 = WM_APP + 1;
#[cfg(any(test, feature = "automation"))]
const WM_SELF_TEST: u32 = WM_APP + 2;
/// Posted at startup without a saved session, so Select Server opens once
/// the frame is shown, as mIRC opens its connect options on a first run.
const WM_CONNECT_AT_STARTUP: u32 = WM_APP + 3;
/// The tray icon's callback message (`NOTIFYICONDATAW.uCallbackMessage`).
const WM_TRAY_ICON: u32 = WM_APP + 4;
/// A scroll changed what the windows show: the window asked for older
/// history (its loading line), or came back to its end (where it drops what
/// its buffer kept while it was scrolled back, `window_buffer`). Posted: the
/// sync must not run inside the scrolled window's message.
const WM_SYNC_VIEWS: u32 = WM_APP + 5;
/// The core refused to log out (`log_out_warning`): its dialog opens.
/// Posted: a modal must not run inside the events' application, which an
/// automation `WM_COPYDATA` may be running.
const WM_LOG_OUT_WARNING: u32 = WM_APP + 6;
/// A connect happened: mIRC's Perform on connect runs (`script::run_perform`).
/// Posted after the connect's events are applied, for the same reason.
const WM_PERFORM: u32 = WM_APP + 7;
/// A script run that waited for the core can go on: the core answered
/// what its last command sent (`script::resume_ready_run`). Posted after
/// the answers are applied, for the same reason.
const WM_SCRIPT_RESUME: u32 = WM_APP + 8;
/// A channel window was double-clicked: Channel Central opens for it.
/// `wparam` is the window's MDI child, which may have closed by the time
/// this is handled.
const WM_CHANNEL_CENTRAL_OF: u32 = WM_APP + 9;
const AUTO_CLOSE_TIMER_ID: usize = 1;
const AUTO_CLOSE_DELAY_MS: u32 = 250;
/// A connect retry is due (`reconnect`).
const RECONNECT_TIMER_ID: usize = 2;
/// The core's queued events are applied. A timer, as Windows makes its
/// message only when no input, paint or posted message waits: a stream
/// of events (a big account's sync) never starves the keyboard, the mouse
/// or the windows' painting, and the events that came meanwhile go as
/// one batch.
const CORE_EVENTS_TIMER_ID: usize = 3;
/// `USER_TIMER_MINIMUM`: the timer is due as soon as Windows allows.
const CORE_EVENTS_DELAY_MS: u32 = 10;
// The timeline's lengths at 96 dpi; `TimelineMetrics` scales them.
const TIMELINE_LEFT_PADDING: i32 = 8;
const TIMELINE_RIGHT_PADDING: i32 = 8;
const TIMELINE_VERTICAL_PADDING: i32 = 6;
const TIMELINE_IMAGE_THUMBNAIL_MAX_WIDTH: i32 = 160;
const TIMELINE_IMAGE_THUMBNAIL_MAX_HEIGHT: i32 = 120;
/// Space around a thumbnail inside its slot, over the slot's lines.
const TIMELINE_IMAGE_SLOT_PADDING: i32 = 20;
const TIMELINE_IMAGE_LEFT_INDENT: i32 = 24;
/// Where an image placeholder's text starts inside its box.
const TIMELINE_IMAGE_PLACEHOLDER_INSET: i32 = 8;
const TIMELINE_FIND_HIGHLIGHT_COLOR: u32 = 0x00f0f8d8;
const TIMELINE_SELECTION_COLOR: u32 = 0x00f5e6cc;
const DEFAULT_DEVICE_DISPLAY_NAME: &str = "mirx";
const VK_RETURN_KEY: WPARAM = 13;
const VK_UP_KEY: WPARAM = 38;
const VK_DOWN_KEY: WPARAM = 40;
const VK_ESCAPE_KEY: WPARAM = 27;

#[derive(Default)]
struct Controls {
    /// mIRC's toolbar, its image list (which the toolbar does not own).
    toolbar: HWND,
    toolbar_images: windows_sys::Win32::UI::Controls::HIMAGELIST,
    /// The treebar's and switchbar's window icons, which neither owns.
    window_icons: windows_sys::Win32::UI::Controls::HIMAGELIST,
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
    /// The window's log; dropping the view closes it.
    log: ViewLog,
    /// The inputs its rows were last built from; `None` before the first
    /// build, and for the Rooms List, which has no rows to gate.
    rows_key: Option<ViewRowsKey>,
    /// What the window's nick list lists, item by item: the one map from
    /// an item to its member. Empty for Status and the Rooms List, which
    /// have none, and before the first build.
    nick_list: Vec<ListedMember>,
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

#[derive(Default)]
struct UiModel {
    status: String,
    /// The signed-in account, known from the login result or the session
    /// being restored.
    own_account: Option<OwnAccount>,
    /// Kept across logouts, as mIRC keeps Status text across disconnects.
    status_log: StatusLog,
    lifecycle_state: LifecycleState,
    connection: ConnectionState,
    rooms: Vec<RoomSummary>,
    room_read_states: BTreeMap<String, RoomReadState>,
    selected_room: Option<String>,
    room_views: RoomViews,
    last_activity: Option<LastMessageActivity>,
    last_e2ee_status: Option<E2eeStatus>,
    last_e2ee_recovery_setup_result: bool,
    verifications: Verifications,
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
    /// Other users' presence, as the session's sync reported it.
    presence: PresenceBook,
    /// Alias -> room ID: the aliases the session joined rooms by
    /// (`RoomJoined.alias`), which the rooms answer to (`room_targets`)
    /// before their first sync brings their published aliases.
    joined_aliases: BTreeMap<String, String>,
    /// Whether `rooms` is a list the core sent this session: before the
    /// first, and after logout, it is not, so the first list's invites are
    /// no news.
    rooms_listed: bool,
}

/// Who is signed in, and how the session authenticates: a reset of the
/// E2EE identity confirms with the account password, or in the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OwnAccount {
    user_id: String,
    auth: SessionAuth,
}

#[cfg(test)]
impl OwnAccount {
    fn matrix(user_id: String) -> Self {
        Self {
            user_id,
            auth: SessionAuth::Matrix,
        }
    }
}

impl UiModel {
    fn own_user_id(&self) -> Option<&str> {
        self.own_account
            .as_ref()
            .map(|account| account.user_id.as_str())
    }
}

struct WindowState {
    controls: Controls,
    event_queue: Arc<Mutex<VecDeque<Event>>>,
    command_sender: Option<CommandSender>,
    /// The thread that brings the core's events, which ends once the core
    /// has stopped. The frame's end waits for it (`WM_NCDESTROY`): mirx
    /// exits only after the core shut down.
    core_thread: Option<thread::JoinHandle<()>>,
    /// The automation harness's transport, switches and record.
    harness: Harness,
    clock: UiClock,
    settings: SettingsStore,
    aliases: AliasStore,
    /// The time zone timestamps are shown in.
    local_time: LocalTimeFn,
    text_font: TextFont,
    /// The brushes of `settings`' colour scheme; `apply_color_scheme`
    /// changes both.
    scheme_brushes: SchemeBrushes,
    exit_code: Arc<AtomicI32>,
    event_generation: u64,
    /// The core's acks applied (`Event::CommandHandled`): the commands the
    /// core has answered, out of `command_sender`'s `sent()`.
    core_answers: AnsweredCommands,
    /// The alias calls, `//lines` and perform runs waiting for the core.
    script_runs: ScriptRuns,
    current_session_homeserver_url: Option<String>,
    tree_items: BTreeMap<isize, TreeItem>,
    primary_tree_items: BTreeMap<ViewId, isize>,
    views: BTreeMap<ViewId, MdiViewState>,
    hwnd_views: BTreeMap<isize, ViewId>,
    /// The window the user is in: written only by an activation
    /// (`views::apply_view_activation_state`), which enters it and leaves
    /// the one before. A window whose room went is replaced by an
    /// activation before its child is destroyed
    /// (`views::destroy_stale_room_views`), so this names a registered
    /// view except inside that function and at frame teardown.
    active_view: ViewId,
    /// A window to activate at the next view sync, which takes it: a room
    /// the user joined. Joining is applied before the sync creates the
    /// room's window, so it can only be activated there.
    pending_activation: Option<ViewId>,
    selected_timeline_event: Option<SelectedTimelineEvent>,
    find_states: BTreeMap<ViewId, ViewFindState>,
    /// The Go to Original waiting for older history; one at a time.
    pending_jump: Option<replies::EventJump>,
    /// The switchbar's buttons' windows, indexed by command id, and their
    /// labels and icons; only `rebuild_switchbar` sets them, both together.
    switchbar_views: Vec<ViewId>,
    switchbar_faces: Vec<SwitchbarFace>,
    /// The treebar's items as `sync_tree` last built them.
    applied_navigation: Option<NavItem>,
    room_activity: BTreeMap<ViewId, ViewActivityLevel>,
    /// The read receipt mirx last sent per room, until the server echoes
    /// it.
    sent_receipts: read_receipts::SentReceipts,
    /// Joined queries whose window the user closed. Like mIRC's closed query
    /// windows, they stay closed until a new message from the other side or
    /// an explicit activation. Holds only joined DM rooms in `model.rooms`.
    closed_queries: BTreeSet<String>,
    /// Invites Auto-join channel on invite accepted, until their join
    /// lands: their windows open without becoming active. The user's own
    /// accept of one takes it out.
    quiet_joins: BTreeSet<String>,
    /// The last Tab completion, for further Tabs to cycle.
    nick_completion: Option<ComposeNickCompletion>,
    edit_window_proc: WNDPROC,
    model: UiModel,
    decoded_images: BTreeMap<String, GdiplusImage>,
    gdiplus_session: Option<GdiplusSession>,
    gdiplus_startup_error: Option<String>,
    /// The typing notices said live, by room.
    typing_notices: typing::TypingNotices,
    pending_attachment_saves: BTreeMap<String, PendingAttachmentSave>,
    /// Channel Central while it is open: the core's answer to its read
    /// goes to it, and the automation dump reads it.
    channel_central_dialog: Option<HWND>,
    /// The Sessions dialog while it is open: your sessions and the errors
    /// of what it does go to it, and the automation dump reads it.
    sessions_dialog: Option<HWND>,
    tray: TrayState,
    /// The retry bookkeeping; `reconnect` and the sync events move it.
    reconnect: connection::Reconnect,
    /// Which notify list users Status said are on, while connected.
    notify_watch: notify::NotifyWatch,
    /// The session's first room list is still to come: Status counts it
    /// (`* Synced rooms:`); the later ones, every change, it does not.
    room_count_due: bool,
    /// The core's refusal to log out, with the request it refused, waiting
    /// for its Log Out dialog: only the posted open (`WM_LOG_OUT_WARNING`)
    /// takes it.
    log_out_warning: Option<(LogoutRequest, LogoutRefusal)>,
    /// The `model.presence` revision the nick lists were last painted at;
    /// they draw presence at paint time, so a change only repaints them.
    painted_presence_revision: u64,
    /// The treebar splitter's drag while the frame has the mouse for it.
    treebar_drag: Option<splitter::TreebarDrag>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectedTimelineEvent {
    view_id: ViewId,
    event_id: String,
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
    let interactive = is_interactive(&command);
    match run_inner(command) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            // A user who started mirx from Explorer has no console to read
            // stderr in. Automation runs never open a modal: nothing would
            // close it.
            if interactive {
                show_error_message(null_mut(), FRAME_TITLE, &message);
            }
            ExitCode::from(1)
        }
    }
}

pub(crate) fn usage_error(usage: &str) -> ExitCode {
    cli_text(CliStream::Stderr, usage);
    ExitCode::from(2)
}

#[derive(Clone, Copy)]
enum CliStream {
    Stdout,
    Stderr,
}

/// Text for the person at the command line. mirx is a GUI-subsystem program,
/// so it has std handles only when its starter gave it some (a redirection,
/// or a parent such as the testkit that passes its own); started from `cmd`
/// without one, or from a shortcut, it has none, and printing would vanish,
/// so the text goes to a box instead.
fn cli_text(stream: CliStream, text: &str) {
    let has_handle = match stream {
        CliStream::Stdout => !std::io::stdout().as_raw_handle().is_null(),
        CliStream::Stderr => !std::io::stderr().as_raw_handle().is_null(),
    };
    match (stream, has_handle) {
        (CliStream::Stdout, true) => println!("{text}"),
        (CliStream::Stderr, true) => eprintln!("{text}"),
        (CliStream::Stdout, false) => show_cli_box(text, MB_ICONINFORMATION),
        (CliStream::Stderr, false) => show_cli_box(text, MB_ICONERROR),
    }
}

fn show_cli_box(text: &str, icon: MESSAGEBOX_STYLE) {
    let title = wide_null(FRAME_TITLE);
    let text = wide_null(text);
    unsafe {
        MessageBoxW(null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | icon);
    }
}

fn run_inner(command: AppCommand) -> Result<ExitCode, String> {
    if matches!(command, AppCommand::Help) {
        cli_text(CliStream::Stdout, HELP);
        return Ok(ExitCode::SUCCESS);
    }
    // First, so a panic during startup is reported too.
    crash::install(
        crash::crash_presentation(&command),
        profile_dir()?.join(CRASH_LOG_FILE_NAME),
    );

    // What the launch asks of the automation harness: a render-state run
    // reads its events here. A build without the harness has none.
    let harness = Harness::for_launch(&command)?;
    let render_state = harness.render_state();
    let automation_enabled = automation_enabled_for_command(&command);

    // The shell, which opens web links, needs COM on the thread that calls
    // it: this one, before any window exists.
    let com = unsafe {
        CoInitializeEx(
            null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        )
    };
    assert_eq!(com, S_OK, "COM starts once, first, on the UI thread");

    let instance = unsafe { GetModuleHandleW(null()) };
    if instance.is_null() {
        return Err(last_os_error("GetModuleHandleW"));
    }

    init_common_controls()?;
    register_frame_class(instance)?;
    register_mdi_child_class(instance)?;
    register_room_content_class(instance)?;
    register_timeline_class(instance)?;
    register_dialog_classes(instance)?;

    let exit_code = Arc::new(AtomicI32::new(0));
    let event_queue = Arc::new(Mutex::new(VecDeque::new()));
    let (gdiplus_session, gdiplus_startup_error) = initialize_gdiplus_state();
    let settings = load_settings(render_state)?;
    let aliases = load_aliases(render_state)?;
    // The frame does not exist yet; it will be laid out at the system DPI.
    let text_font = unsafe { TextFont::create(&settings.settings().text_font, Dpi::system())? };
    let scheme_brushes = unsafe { SchemeBrushes::create(&settings.settings().colors) };

    let state = Box::new(WindowState {
        controls: Controls::default(),
        event_queue: Arc::clone(&event_queue),
        command_sender: None,
        core_thread: None,
        harness,
        clock: if render_state {
            UiClock::Fixed(RENDER_STATE_CLOCK_MS)
        } else {
            UiClock::System
        },
        settings,
        aliases,
        local_time: system_local_time,
        text_font,
        scheme_brushes,
        exit_code: Arc::clone(&exit_code),
        event_generation: 0,
        core_answers: AnsweredCommands::default(),
        script_runs: ScriptRuns::default(),
        current_session_homeserver_url: None,
        tree_items: BTreeMap::new(),
        primary_tree_items: BTreeMap::new(),
        views: BTreeMap::new(),
        hwnd_views: BTreeMap::new(),
        active_view: ViewId::Status,
        selected_timeline_event: None,
        find_states: BTreeMap::new(),
        pending_jump: None,
        switchbar_views: Vec::new(),
        switchbar_faces: Vec::new(),
        applied_navigation: None,
        room_activity: BTreeMap::new(),
        sent_receipts: Default::default(),
        closed_queries: BTreeSet::new(),
        quiet_joins: BTreeSet::new(),
        pending_activation: None,
        nick_completion: None,
        edit_window_proc: None,
        model: initial_model(),
        decoded_images: BTreeMap::new(),
        gdiplus_session,
        gdiplus_startup_error,
        typing_notices: typing::TypingNotices::default(),
        pending_attachment_saves: BTreeMap::new(),
        channel_central_dialog: None,
        sessions_dialog: None,
        tray: TrayState::default(),
        reconnect: connection::Reconnect::default(),
        notify_watch: notify::NotifyWatch::default(),
        room_count_due: false,
        log_out_warning: None,
        painted_presence_revision: 0,
        treebar_drag: None,
    });

    let hwnd = create_main_window(instance, state, frame_size_for_command(&command))?;

    let startup = (|| -> Result<(), String> {
        unsafe { harness::attach(hwnd, &command)? };

        if should_start_core_bridge(&command) {
            let CoreBridge {
                command_sender,
                join_handle,
            } = start_core_bridge(hwnd, Arc::clone(&event_queue))?;
            unsafe {
                let state = state_mut(hwnd);
                state.command_sender = Some(command_sender);
                state.core_thread = Some(join_handle);
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
        }

        unsafe {
            if state_ref(hwnd).settings.settings().tray.startup {
                // On startup minimize to tray: shown minimized, then hidden
                // behind the icon if the shell takes it.
                ShowWindow(hwnd, SW_SHOWMINNOACTIVE);
                hide_to_tray(hwnd)?;
            } else {
                ShowWindow(hwnd, SW_SHOW);
                if automation_enabled {
                    SetForegroundWindow(hwnd);
                }
                // Always show the icon in the tray.
                sync_tray_icon(hwnd)?;
            }
            harness::load(hwnd)?;
        }

        Ok(())
    })();

    if let Err(message) = startup {
        unsafe {
            DestroyWindow(hwnd);
        }
        return Err(message);
    }

    unsafe {
        match command {
            AppCommand::AutoClose => {
                SetTimer(hwnd, AUTO_CLOSE_TIMER_ID, AUTO_CLOSE_DELAY_MS, None);
            }
            AppCommand::Run | AppCommand::Help => {}
            #[cfg(any(test, feature = "automation"))]
            AppCommand::Automation(_) => {}
            #[cfg(any(test, feature = "automation"))]
            AppCommand::SelfTest => {
                PostMessageW(hwnd, WM_SELF_TEST, 0, 0);
            }
        }
    }

    // The frame's end waited for the core.
    message_loop();

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

/// The user's `aliases.ini`, as `load_settings` loads `mirx.ini`.
fn load_aliases(render_state_mode: bool) -> Result<AliasStore, String> {
    if render_state_mode {
        return Ok(AliasStore::in_memory());
    }
    AliasStore::load(profile_dir()?.join(ALIASES_FILE_NAME))
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
        AppCommand::Run | AppCommand::AutoClose | AppCommand::Help => false,
        #[cfg(any(test, feature = "automation"))]
        AppCommand::Automation(_) | AppCommand::SelfTest => true,
    }
}

fn should_start_core_bridge(command: &AppCommand) -> bool {
    match command {
        AppCommand::Run | AppCommand::AutoClose => true,
        AppCommand::Help => false,
        // A render-state run's core is the harness's inert one.
        #[cfg(any(test, feature = "automation"))]
        AppCommand::Automation(launch) => launch.render_state.is_none(),
        #[cfg(any(test, feature = "automation"))]
        AppCommand::SelfTest => false,
    }
}

/// A run for a person at the screen: the only kind that may open a modal on
/// its own (Select Server at startup, the startup-error and crash boxes).
/// Automation launches, `--auto-close` and `--help` never do: nothing would
/// close it.
fn is_interactive(command: &AppCommand) -> bool {
    matches!(command, AppCommand::Run)
}

/// Only an interactive run without a saved session opens Select Server by
/// itself.
fn opens_connect_dialog_at_startup(command: &AppCommand, session_saved: bool) -> bool {
    is_interactive(command) && !session_saved
}

/// The frame's size: `--frame-size`, an exact geometry in physical pixels,
/// or the default for the system.
fn frame_size_for_command(command: &AppCommand) -> FrameSize {
    let explicit = match command {
        AppCommand::Run | AppCommand::AutoClose | AppCommand::Help => None,
        #[cfg(any(test, feature = "automation"))]
        AppCommand::Automation(launch) => launch.frame_size,
        #[cfg(any(test, feature = "automation"))]
        AppCommand::SelfTest => None,
    };
    explicit.unwrap_or_else(|| default_frame_size(Dpi::system(), primary_work_area()))
}

/// `DEFAULT_FRAME_SIZE` at `dpi`, but never larger than the screen's
/// `work_area`: scaled, the default may not fit a small screen.
fn default_frame_size(dpi: Dpi, work_area: FrameSize) -> FrameSize {
    FrameSize {
        width: dpi.px(DEFAULT_FRAME_SIZE.width).min(work_area.width),
        height: dpi.px(DEFAULT_FRAME_SIZE.height).min(work_area.height),
    }
}

/// The primary monitor's size without the taskbar.
fn primary_work_area() -> FrameSize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETWORKAREA, SystemParametersInfoW};
    let mut area = RECT::default();
    let read =
        unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut area as *mut RECT).cast(), 0) };
    assert!(read != 0, "SPI_GETWORKAREA must succeed");
    FrameSize {
        width: area.right - area.left,
        height: area.bottom - area.top,
    }
}

fn initial_model() -> UiModel {
    UiModel {
        status: "offline".to_owned(),
        lifecycle_state: LifecycleState::Offline,
        ..UiModel::default()
    }
}

fn bridge_core_events(hwnd: HWND, queue: Arc<Mutex<VecDeque<Event>>>, handle: CoreHandle) {
    loop {
        match handle.events().recv_timeout(Duration::from_millis(100)) {
            Ok(event) => {
                let should_stop = matches!(event, Event::Stopped);
                let first = {
                    let mut queue = queue.lock().expect("core event queue poisoned");
                    queue.push_back(event);
                    queue.len() == 1
                };
                // Later events join it: the UI takes the whole queue at once.
                if first {
                    unsafe {
                        PostMessageW(hwnd, WM_CORE_EVENT, 0, 0);
                    }
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

/// Appends a line to the Status window, colouring its switchbar button when
/// it is not the active window, as any window's new lines do.
fn log_status(state: &mut WindowState, kind: StatusLineKind, text: String) {
    state
        .model
        .status_log
        .push(state.clock.now_ms(), kind, context_line(&text));
    if state.active_view != ViewId::Status {
        let level = match kind {
            StatusLineKind::Info
            | StatusLineKind::Notify
            | StatusLineKind::Invite
            | StatusLineKind::Echo { .. } => ViewActivityLevel::Event,
            StatusLineKind::Error => ViewActivityLevel::Message,
        };
        raise_view_activity(state, ViewId::Status, level);
    }
}

unsafe fn state_ref(hwnd: HWND) -> &'static WindowState {
    state_mut(hwnd)
}

unsafe fn state_mut(hwnd: HWND) -> &'static mut WindowState {
    window_data::get(hwnd).expect("window state must exist")
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
    use std::fs;

    use super::test_fixtures::*;
    use super::*;
    use crate::automation_launch::AutomationLaunch;

    fn automation_launch(render_state: Option<&str>) -> AppCommand {
        AppCommand::Automation(AutomationLaunch {
            render_state: render_state.map(PathBuf::from),
            frame_size: None,
        })
    }

    #[test]
    fn only_an_interactive_run_without_a_session_opens_select_server_at_startup() {
        assert!(opens_connect_dialog_at_startup(&AppCommand::Run, false));
        assert!(!opens_connect_dialog_at_startup(&AppCommand::Run, true));
        for command in [
            automation_launch(None),
            automation_launch(Some("state.json")),
            AppCommand::AutoClose,
            AppCommand::SelfTest,
            AppCommand::Help,
        ] {
            assert!(!opens_connect_dialog_at_startup(&command, false));
        }
    }

    #[test]
    fn only_a_plain_run_is_interactive() {
        assert!(is_interactive(&AppCommand::Run));
        for command in [
            automation_launch(None),
            automation_launch(Some("state.json")),
            AppCommand::SelfTest,
            AppCommand::AutoClose,
            AppCommand::Help,
        ] {
            assert!(!is_interactive(&command));
        }
    }

    fn system_default_frame_size() -> FrameSize {
        default_frame_size(Dpi::system(), primary_work_area())
    }

    #[test]
    fn frame_size_defaults_for_run_command() {
        assert_eq!(
            frame_size_for_command(&AppCommand::Run),
            system_default_frame_size()
        );
    }

    #[test]
    fn default_frame_size_scales_and_fits_the_work_area() {
        let size = |width, height| FrameSize { width, height };
        assert_eq!(
            default_frame_size(Dpi::STANDARD, size(1920, 1040)),
            size(1024, 768)
        );
        assert_eq!(
            default_frame_size(Dpi::new(144), size(1920, 1040)),
            size(1536, 1040)
        );
        assert_eq!(
            default_frame_size(Dpi::new(144), size(1280, 1024)),
            size(1280, 1024)
        );
    }

    #[test]
    fn frame_size_uses_explicit_launch_value() {
        assert_eq!(
            frame_size_for_command(&AppCommand::Automation(AutomationLaunch {
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
            system_default_frame_size()
        );
    }

    #[test]
    fn frame_size_defaults_for_auto_close() {
        assert_eq!(
            frame_size_for_command(&AppCommand::AutoClose),
            system_default_frame_size()
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
    fn aliases_load_from_the_profile_except_in_render_state_mode() {
        with_local_app_data(|root| {
            let profile = root.join("mirx").join("ui-shell");
            fs::create_dir_all(&profile).unwrap();
            assert!(load_aliases(false).unwrap().aliases().get("j").is_some());
            fs::write(profile.join(ALIASES_FILE_NAME), "[aliases]\nn0=/x say x\n").unwrap();
            assert!(load_aliases(false).unwrap().aliases().get("j").is_none());
            assert!(load_aliases(true).unwrap().aliases().get("j").is_some());

            fs::write(profile.join(ALIASES_FILE_NAME), "[aliases]\nn0=/x\n").unwrap();
            let error = load_aliases(false).err().unwrap();
            assert!(
                error.ends_with("aliases.ini line 2 (n0): /x has no commands"),
                "{error}"
            );
        });
    }

    #[test]
    #[should_panic(expected = "holds another kind of state")]
    fn a_window_does_not_give_its_state_as_another_kind() {
        unsafe {
            register_dialog_classes(GetModuleHandleW(null())).unwrap();
            let mut dialog_state = window_data::CreateParam::new(SingleFieldDialogState {
                result: Arc::new(Mutex::new(None)),
            });
            let dialog = CreateWindowExW(
                0,
                wide_null(SINGLE_FIELD_DIALOG_CLASS_NAME).as_ptr(),
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
}
