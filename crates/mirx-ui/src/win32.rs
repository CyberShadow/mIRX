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
mod frame;
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
mod views;
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
use frame::window_class_is;
use frame::{
    create_main_window, frame_status_text, frame_window_for, init_common_controls, layout_controls,
    message_loop, refresh_status_bar, register_frame_class, set_switchbar_button_width,
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
use settings::{Bar, FontSetting, SETTINGS_FILE_NAME, SettingsStore};
use status_log::{StatusLineKind, StatusLog};
use timeline_layout::{
    TimelinePaintLine, TimelinePaintLineVisual, activity_level_color, paint_timeline,
    plain_timeline_paint_lines, rendered_line_ranges, timeline_row_paint_lines,
};
use timeline_rows::{
    TimelineImageState, TimelineMessageSendState, TimelineRow, TimelineRowIdentity,
    TimelineRowKind, TimelineRowLayout, build_room_timeline_rows, build_status_timeline_rows,
    build_thread_timeline_rows, collect_reply_target_requests, compose_target_row,
    dispatch_reply_target_fetches, project_timeline_rows,
};
#[cfg(test)]
use timeline_rows::{TimelineSyntheticRowLabel, render_room_lines};
use timeline_text::{
    ENCRYPTED_ROOM_TEXT, INVITE_RESPONSE_TEXT, action_line, context_line, event_line, invited_text,
    member_change_text, message_line, nick_label, now_talking_text, power_levels_change_text,
    rank_prefix, topic_change_text, topic_is_text,
};
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
#[cfg(test)]
use timestamp::TimestampSetting;
use timestamp::{LocalTime, LocalTimeFn, TimestampStyle};
use verification::{
    apply_verification_event, e2ee_backup_health_summary, e2ee_cross_signing_label,
    e2ee_local_backup_label, e2ee_recovery_state_label, e2ee_server_backup_label,
    format_verification_emojis, submit_accept_verification, submit_automation_verification_action,
    submit_cancel_verification, submit_confirm_verification_sas, submit_menu_verification_action,
    submit_reject_verification_sas, verification_action_label,
    verification_state_event_from_request, verification_state_label,
};
use views::{
    CloseAction, activate_view, cached_view_is_alive, close_action, destroy_stale_room_views,
    ensure_room_view, ensure_status_view, ensure_view, layout_mdi_child, mdi_child_content,
    mdi_child_edit, mdi_window_placement, refresh_every_view_content, refresh_view_content,
    register_mdi_child_class, register_room_content_class, resolve_active_view,
    room_content_member_list, room_content_timeline, set_view_window_state, view_timeline,
};
#[cfg(test)]
use views::{ROOM_CONTENT_CLASS_NAME, take_stale_room_views};

const TIMELINE_CLASS_NAME: &str = "mirxWin32Timeline";
const FRAME_TITLE: &str = "mirx";
const DEFAULT_FRAME_SIZE: FrameSize = FrameSize {
    width: 1024,
    height: 768,
};
const TREE_CLASS_NAME: &str = "SysTreeView32";
const EDIT_CLASS_NAME: &str = "EDIT";
const STATIC_CLASS_NAME: &str = "STATIC";
const BUTTON_CLASS_NAME: &str = "BUTTON";
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
}
