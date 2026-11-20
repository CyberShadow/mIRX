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
mod command_runner;
mod commands;
mod compose;
mod connection;
mod core_events;
mod dialogs;
mod echo;
mod errors;
mod find;
mod fonts;
mod frame_menus;
mod glyph_fonts;
mod glyph_runs;
mod ignore;
mod images;
mod mdi;
mod menus;
mod mirc_format;
mod navigation;
mod nick_completion;
mod nick_list;
mod notifications;
mod options;
mod options_dialog;
mod reactions;
mod replies;
mod room_list;
mod room_list_window;
mod room_model;
mod room_targets;
mod self_test;
mod send_file;
mod settings;
mod status_log;
mod switchbar;
#[cfg(test)]
mod test_fixtures;
mod timeline_layout;
mod timeline_rows;
mod timeline_text;
mod timeline_window;
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
#[cfg(test)]
use command_runner::run_slash_command;
use command_runner::{
    echo_lines_in_active_view, enqueue_command, known_nick, own_server, report_command_error,
    resolve_invite_command_room_id, resolve_joined_room_target, run_local_command, send_edit,
    send_reply, set_local_command_status, submit_accept_slash_command, submit_join_room,
    submit_part_slash_command, submit_query_slash_command, submit_redact_slash_command,
    submit_reject_slash_command, submit_room_notification_mode, whois_lines,
};
use commands::{
    BarCommand, CommandError, ComposeSubmission, CreateRoomArgs, E2eeSlashCommand, FontCommand,
    SlashCommand, TimestampCommand, VerificationAction, local_command_history_entry,
    parse_compose_submission,
};
#[cfg(test)]
use compose::submit_compose_from_edit;
use compose::{
    ComposeHistory, ComposeNickCompletion, active_compose_history, arm_compose_target,
    arm_compose_target_on_selected_message, armed_compose_target_row, compose_edit,
    disarm_vanished_compose_target, dump_compose_text, enqueue_send_message,
    message_text_from_input, prefill_react_to_selected_message, replace_edit_selection,
    resolve_compose_destination, run_send_file_dialog, send_compose_key, send_file_room_id,
    send_room_message, set_compose_text, subclass_compose_edit, submit_compose_from_control,
    submit_redact_selected_message, submit_send_file,
};
use connection::ConnectAction;
#[cfg(test)]
use core_events::apply_core_event;
use core_events::{
    apply_event_load, apply_events_and_refresh, clear_view_activity_marker,
    drain_core_events_and_refresh, handle_core_events, handle_view_activation, operation_name,
    resolve_timeline_room_id,
};
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
use frame_menus::active_message_action_state;
use frame_menus::{
    active_message_action_state_for_automation, active_room_action_state, connect,
    context_menu_command_id, context_menu_screen_point_for_lparam, create_frame_menu,
    default_context_point_for_window, disconnect, handle_menu_command, handle_switchbar_command,
    invoke_menu_command_for_automation, invoke_navigation_context_menu_for_automation,
    invoke_timeline_context_menu_for_automation, refresh_menu_popup, refresh_message_menu_state,
    refresh_room_menu_state, refresh_tools_menu_state, select_timeline_event_for_automation,
    selected_message_quote_target, selected_timeline_event_id,
    selected_timeline_event_id_for_automation, selected_timeline_media_attachment,
    show_context_menu, show_recover_e2ee_menu_dialog, show_timeline_message_context_menu,
    timeline_row_is_selected, window_menu_command_id,
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
#[cfg(test)]
use navigation::{NavItem, navigation_layout};
use navigation::{
    ROOM_LIST_TITLE, dump_navigation_tree, dump_selected_navigation_path, expand_tree_item,
    handle_tree_selection, insert_tree_item, ordered_view_ids, read_tree_item_text,
    rebuild_switchbar, refresh_switchbar_state, refresh_tree_activity_colors,
    select_navigation_path, select_navigation_view, select_room, select_tree_item,
    selected_tree_item, show_navigation_room_context_menu, switchbar_button_text,
    switchbar_custom_draw, sync_views_and_tree, tree_custom_draw, view_caption,
    view_id_from_identity, view_identity_from_view_id,
};
use nick_completion::{NickCompletion, member_mention_text};
use nick_list::{
    active_member_action_state, active_view_members, draw_nick_list_item,
    invoke_member_context_menu_for_automation, invoke_selected_member_context_menu_for_automation,
    mention_selected_member, query_selected_member, refresh_member_menu_state, room_member_nick,
    room_member_view_rows, run_selected_member_command, select_member_for_automation,
    set_room_member_list, show_member_context_menu, text_font_item_height,
};
use replies::ComposeTarget;
use room_list::{PageApplied, RoomListColumn, RoomListQuery, RoomListState};
use room_list_window::{
    add_room_list_columns, apply_public_rooms_page, dump_room_list, ensure_room_list_view,
    handle_room_list_notify, refresh_room_list_rows, start_room_list,
};
#[cfg(test)]
use room_model::ThreadViewModel;
use room_model::{
    LastMessageActivity, MessageReplacementFields, ReplyTarget, ReplyTargetState,
    RoomHistoryBackfillState, RoomHistoryBackfillStatus, RoomPreviewMediaState, RoomViewModel,
    RoomWindowKind, activity_level_for, failed_room_activity_update_view_id,
    is_invite_command_target, is_invited_room, is_joined_message_room, is_navigable_room,
    is_sendable_room, joined_room_notification_mode, last_activity_from_room_activity,
    navigable_room_by_id, projected_thread_by_id, raise_view_activity,
    read_target_for_room_activity, room_activity_belongs_to_thread, room_activity_event_id,
    room_activity_sender_user_id, room_activity_target_view_id,
    room_activity_update_marks_inactive_room, room_by_id, room_window_kind, rooms_in_window_order,
    sendable_room_by_id, validate_synthetic_room_activity_events, view_id_is_valid,
    view_id_is_valid_for_rooms, window_message,
};
use room_targets::{RoomTargetError, resolve_room};
use self_test::run_self_test;
use status_log::{StatusLineKind, StatusLog};
use timeline_rows::{
    TimelineImageState, TimelineMessageSendState, TimelineRow, TimelineRowIdentity,
    TimelineRowKind, TimelineRowLayout, build_room_timeline_rows, build_status_timeline_rows,
    build_thread_timeline_rows, collect_reply_target_requests, compose_target_row,
    dispatch_reply_target_fetches, project_timeline_rows,
};
#[cfg(test)]
use timeline_rows::{TimelineSyntheticRowLabel, render_room_lines};
#[cfg(test)]
use timeline_window::clamped_timeline_top_line;
use timeline_window::{
    cancel_original_jump_for_view, capture_room_history_backfill_anchor, clamp_timeline_top_line,
    continue_original_jump, go_to_original_of_selected_message, loading_room_history_room_id,
    rebuild_timeline_paint_lines, register_timeline_class, restore_room_history_backfill_anchor,
    select_timeline_event_at_client_y, set_timeline_rows, split_rendered_lines,
    submit_backfill_room_history_for_room, timeline_max_top_line,
    timeline_source_row_index_for_top_line, timeline_state_mut, timeline_state_ref,
    timeline_top_line_for_source_row_index, timeline_visible_rows, timeline_window_state,
    update_timeline_scrollbar, validate_selected_timeline_event,
};
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
const AUTO_CLOSE_TIMER_ID: usize = 1;
const AUTO_CLOSE_DELAY_MS: u32 = 250;
const TREE_WIDTH: i32 = 240;
const TIMELINE_LEFT_PADDING: i32 = 8;
const TIMELINE_RIGHT_PADDING: i32 = 8;
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

    use windows_sys::Win32::UI::Controls::{TB_GETSTATE, TBSTATE_WRAP};
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;

    use super::test_fixtures::*;
    use super::*;
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
}
