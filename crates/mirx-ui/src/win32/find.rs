//! Find Text: its dialog, matching the active view's lines, and searching
//! further back in history.

use super::*;

/// A timeline row matching a find query. Every view draws a timeline, so
/// every match is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FindMatch {
    row_identity: TimelineRowIdentity,
    source_row_index: usize,
    logical_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RoomFindHistorySearch {
    LocalOnly,
    Loading,
    Exhausted,
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ViewFindState {
    raw_query: String,
    folded_query: String,
    matches: Vec<FindMatch>,
    selected_match_index: Option<usize>,
    history_search: RoomFindHistorySearch,
    feedback: String,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ActiveViewFindOutcome {
    active_view: ViewId,
    raw_query: String,
    matched: bool,
    match_count: usize,
    current_match_index: Option<usize>,
    selected_text: Option<String>,
    history_search: UiFindHistorySearchState,
}

/// Find searches a window's text; the Rooms List has none of its own.
const FIND_NOT_AVAILABLE: &str = "Find: not available in this window";

pub(super) unsafe fn begin_find_dialog(hwnd: HWND) -> Result<(), String> {
    let active_view = state_ref(hwnd).active_view.clone();
    if active_view == ViewId::RoomList {
        log_status(
            state_mut(hwnd),
            StatusLineKind::Error,
            FIND_NOT_AVAILABLE.to_owned(),
        );
        return refresh_view_content(hwnd, &ViewId::Status).map(|_| ());
    }
    let initial_query = state_ref(hwnd)
        .find_states
        .get(&active_view)
        .map(|find_state| find_state.raw_query.clone())
        .unwrap_or_default();
    let Some(query) = show_single_field_dialog(hwnd, "Find", "Find", &initial_query)? else {
        return Ok(());
    };
    execute_active_view_find(hwnd, &query)?;
    Ok(())
}

fn fold_find_query(query: &str) -> String {
    query.chars().map(|ch| ch.to_ascii_lowercase()).collect()
}

fn ascii_case_insensitive_match_range(
    haystack: &str,
    folded_query: &str,
) -> Option<(usize, usize)> {
    if folded_query.is_empty() {
        return Some((0, 0));
    }

    let expected = folded_query.chars().collect::<Vec<_>>();
    for (start, _) in haystack.char_indices() {
        let mut next_end = start;
        let mut matched = true;
        let mut actual = haystack[start..].chars();
        for query_char in &expected {
            let Some(actual_char) = actual.next() else {
                matched = false;
                break;
            };
            if actual_char.to_ascii_lowercase() != *query_char {
                matched = false;
                break;
            }
            next_end += actual_char.len_utf8();
        }
        if matched {
            return Some((start, next_end));
        }
    }

    None
}

fn ascii_case_insensitive_contains(haystack: &str, folded_query: &str) -> bool {
    ascii_case_insensitive_match_range(haystack, folded_query).is_some()
}

fn compute_timeline_find_matches(rows: &[TimelineRow], folded_query: &str) -> Vec<FindMatch> {
    rows.iter()
        .enumerate()
        .filter(|(_, row)| ascii_case_insensitive_contains(&row.logical_text, folded_query))
        .map(|(source_row_index, row)| FindMatch {
            row_identity: row.identity.clone(),
            source_row_index,
            logical_text: row.logical_text.clone(),
        })
        .collect()
}

enum FindSelectionMode<'a> {
    Execute {
        folded_query: &'a str,
        previous_state: Option<&'a ViewFindState>,
        /// The first source row in view: find searches down from there.
        start_source_row_index: usize,
    },
    Recompute {
        previous_state: &'a ViewFindState,
    },
}

fn preserved_find_match_index(matches: &[FindMatch], previous_match: &FindMatch) -> Option<usize> {
    matches
        .iter()
        .position(|candidate| candidate.row_identity == previous_match.row_identity)
}

fn choose_find_match(matches: &[FindMatch], mode: FindSelectionMode<'_>) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }

    match mode {
        FindSelectionMode::Execute {
            folded_query,
            previous_state,
            start_source_row_index,
        } => {
            if let Some(previous_state) = previous_state
                && previous_state.folded_query == folded_query
                && let Some(previous_index) = previous_state.selected_match_index
                && previous_index < previous_state.matches.len()
            {
                return Some((previous_index + 1) % matches.len());
            }

            matches
                .iter()
                .position(|match_item| match_item.source_row_index >= start_source_row_index)
                .or(Some(0))
        }
        FindSelectionMode::Recompute { previous_state } => previous_state
            .selected_match_index
            .and_then(|previous_index| previous_state.matches.get(previous_index))
            .and_then(|previous_match| preserved_find_match_index(matches, previous_match))
            .or(Some(0)),
    }
}

fn find_feedback(
    raw_query: &str,
    matches: &[FindMatch],
    selected_match_index: Option<usize>,
    history_search: &RoomFindHistorySearch,
) -> String {
    if matches.is_empty() {
        return match history_search {
            RoomFindHistorySearch::LocalOnly => format!("find: no matches for \"{raw_query}\""),
            RoomFindHistorySearch::Loading => {
                format!("find: loading older history for \"{raw_query}\"")
            }
            RoomFindHistorySearch::Exhausted => {
                format!("find: no matches for \"{raw_query}\" in loaded or older history")
            }
            RoomFindHistorySearch::Failed { message } => {
                format!("find: unable to load older history for \"{raw_query}\": {message}")
            }
        };
    }

    let selected_match_index =
        selected_match_index.expect("non-empty find match lists must keep a selected match");
    format!(
        "find: {}/{} \"{}\"",
        selected_match_index + 1,
        matches.len(),
        raw_query
    )
}

fn find_match_selected_text(match_item: &FindMatch) -> String {
    match_item.logical_text.clone()
}

pub(super) fn build_view_find_state(
    raw_query: &str,
    folded_query: &str,
    matches: Vec<FindMatch>,
    selected_match_index: Option<usize>,
) -> ViewFindState {
    build_view_find_state_with_history(
        raw_query,
        folded_query,
        matches,
        selected_match_index,
        RoomFindHistorySearch::LocalOnly,
    )
}

fn build_view_find_state_with_history(
    raw_query: &str,
    folded_query: &str,
    matches: Vec<FindMatch>,
    selected_match_index: Option<usize>,
    history_search: RoomFindHistorySearch,
) -> ViewFindState {
    let feedback = find_feedback(raw_query, &matches, selected_match_index, &history_search);
    ViewFindState {
        raw_query: raw_query.to_owned(),
        folded_query: folded_query.to_owned(),
        matches,
        selected_match_index,
        history_search,
        feedback,
    }
}

fn rebuild_view_find_feedback(find_state: &mut ViewFindState) {
    find_state.feedback = find_feedback(
        &find_state.raw_query,
        &find_state.matches,
        find_state.selected_match_index,
        &find_state.history_search,
    );
}

pub(super) fn active_view_find_feedback(state: &WindowState) -> Option<&str> {
    state
        .find_states
        .get(&state.active_view)
        .map(|find_state| find_state.feedback.as_str())
}

#[cfg_attr(not(test), allow(dead_code))]
fn view_find_outcome(view_id: &ViewId, find_state: &ViewFindState) -> ActiveViewFindOutcome {
    let selected_text = find_state
        .selected_match_index
        .and_then(|index| find_state.matches.get(index))
        .map(find_match_selected_text);
    ActiveViewFindOutcome {
        active_view: view_id.clone(),
        raw_query: find_state.raw_query.clone(),
        matched: !find_state.matches.is_empty(),
        match_count: find_state.matches.len(),
        current_match_index: find_state.selected_match_index,
        selected_text,
        history_search: dump_ui_find_history_search_state(&find_state.history_search),
    }
}

fn dump_find_state(view_id: &ViewId, find_state: &ViewFindState) -> UiFindState {
    let outcome = view_find_outcome(view_id, find_state);
    UiFindState {
        active: view_identity_from_view_id(&outcome.active_view),
        query: outcome.raw_query,
        matched: outcome.matched,
        match_count: outcome.match_count,
        current_match_index: outcome.current_match_index,
        selected_text: outcome.selected_text,
        history_search: outcome.history_search,
    }
}

pub(super) fn find_active_view_response(outcome: ActiveViewFindOutcome) -> FindActiveViewResponse {
    FindActiveViewResponse {
        active: view_identity_from_view_id(&outcome.active_view),
        query: outcome.raw_query,
        matched: outcome.matched,
        match_count: outcome.match_count,
        current_match_index: outcome.current_match_index,
        selected_text: outcome.selected_text,
        history_search: outcome.history_search,
    }
}

fn active_view_find_outcome_without_state(view_id: &ViewId, query: &str) -> ActiveViewFindOutcome {
    ActiveViewFindOutcome {
        active_view: view_id.clone(),
        raw_query: query.to_owned(),
        matched: false,
        match_count: 0,
        current_match_index: None,
        selected_text: None,
        history_search: UiFindHistorySearchState::LocalOnly,
    }
}

unsafe fn handle_invalid_find_query(
    hwnd: HWND,
    query: &str,
) -> Result<ActiveViewFindOutcome, String> {
    set_local_command_status(hwnd, "usage: /find <text>".to_owned())?;
    let active_view = state_ref(hwnd).active_view.clone();
    Ok(state_ref(hwnd)
        .find_states
        .get(&active_view)
        .map(|find_state| view_find_outcome(&active_view, find_state))
        .unwrap_or_else(|| active_view_find_outcome_without_state(&active_view, query)))
}

pub(super) fn recompute_find_for_view(
    state: &mut WindowState,
    view_id: &ViewId,
    rows: &[TimelineRow],
) {
    let Some(previous_state) = state.find_states.get(view_id).cloned() else {
        return;
    };

    let matches = compute_timeline_find_matches(rows, &previous_state.folded_query);
    let selected_match_index = choose_find_match(
        &matches,
        FindSelectionMode::Recompute {
            previous_state: &previous_state,
        },
    );
    let history_search =
        if matches.is_empty() && matches!(view_id, ViewId::Room(_) | ViewId::Thread { .. }) {
            previous_state.history_search
        } else {
            RoomFindHistorySearch::LocalOnly
        };
    let next_state = build_view_find_state_with_history(
        &previous_state.raw_query,
        &previous_state.folded_query,
        matches,
        selected_match_index,
        history_search,
    );
    state.find_states.insert(view_id.clone(), next_state);
}

pub(super) unsafe fn clear_find_projection_for_timeline(hwnd: HWND) {
    if timeline_state_ref(hwnd)
        .selected_find_source_row_index
        .is_none()
    {
        return;
    }

    timeline_state_mut(hwnd).selected_find_source_row_index = None;
    InvalidateRect(hwnd, null(), 1);
}

pub(super) unsafe fn apply_find_to_active_view(hwnd: HWND, view_id: &ViewId) -> Result<(), String> {
    let active_view = state_ref(hwnd).active_view.clone();
    let view = state_ref(hwnd)
        .views
        .get(view_id)
        .ok_or_else(|| "view state was not tracked".to_owned())?;

    let timeline = view_timeline(view_id, view.content);
    if active_view != *view_id {
        clear_find_projection_for_timeline(timeline);
        return Ok(());
    }

    let selected_source_row_index = state_ref(hwnd)
        .find_states
        .get(view_id)
        .and_then(|find_state| {
            find_state
                .selected_match_index
                .and_then(|index| find_state.matches.get(index))
        })
        .map(|item| item.source_row_index);
    if timeline_state_ref(timeline).selected_find_source_row_index != selected_source_row_index {
        timeline_state_mut(timeline).selected_find_source_row_index = selected_source_row_index;
        InvalidateRect(timeline, null(), 1);
    }

    Ok(())
}

pub(super) unsafe fn scroll_timeline_to_source_row_index(hwnd: HWND, source_row_index: usize) {
    let top_line = timeline_top_line_for_source_row_index(hwnd, source_row_index);
    let visible_rows = timeline_visible_rows(hwnd);
    let max_top_line =
        timeline_max_top_line(timeline_state_ref(hwnd).paint_lines.len(), visible_rows);
    timeline_state_mut(hwnd).top_line = top_line.min(max_top_line);
    update_timeline_scrollbar(hwnd);
    InvalidateRect(hwnd, null(), 1);
}

fn build_timeline_find_state_and_maybe_submit_history_backfill(
    state: &mut WindowState,
    view_id: &ViewId,
    raw_query: &str,
    folded_query: &str,
    matches: Vec<FindMatch>,
    selected_match_index: Option<usize>,
) -> Result<ViewFindState, String> {
    let room_id = match view_id {
        ViewId::Room(room_id) | ViewId::Thread { room_id, .. } => room_id,
        ViewId::Status => unreachable!("status find does not use room history"),
        ViewId::RoomList => unreachable!("the Rooms List has no find"),
    };
    let history_search = if !matches.is_empty() {
        RoomFindHistorySearch::LocalOnly
    } else {
        let history_status = state
            .model
            .room_views
            .get(room_id)
            .map(|room_view| room_view.history.status.clone())
            .unwrap_or(RoomHistoryBackfillStatus::Idle);
        match history_status {
            RoomHistoryBackfillStatus::Exhausted => RoomFindHistorySearch::Exhausted,
            RoomHistoryBackfillStatus::Loading { .. } => RoomFindHistorySearch::Loading,
            RoomHistoryBackfillStatus::Failed { message, .. } => {
                if submit_backfill_room_history_for_room(state, room_id)?
                    || loading_room_history_room_id(&state.model.room_views).is_some()
                {
                    RoomFindHistorySearch::Loading
                } else {
                    RoomFindHistorySearch::Failed { message }
                }
            }
            RoomHistoryBackfillStatus::Idle => {
                if submit_backfill_room_history_for_room(state, room_id)?
                    || loading_room_history_room_id(&state.model.room_views).is_some()
                {
                    RoomFindHistorySearch::Loading
                } else {
                    RoomFindHistorySearch::LocalOnly
                }
            }
        }
    };
    Ok(build_view_find_state_with_history(
        raw_query,
        folded_query,
        matches,
        selected_match_index,
        history_search,
    ))
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) unsafe fn execute_active_view_find(
    hwnd: HWND,
    query: &str,
) -> Result<ActiveViewFindOutcome, String> {
    if query.trim().is_empty() {
        return handle_invalid_find_query(hwnd, query);
    }

    let active_view = state_ref(hwnd).active_view.clone();
    if active_view == ViewId::RoomList {
        return Err(FIND_NOT_AVAILABLE.to_owned());
    }
    let previous_state = state_ref(hwnd).find_states.get(&active_view).cloned();
    let folded_query = fold_find_query(query);

    let (matches, selected_match_index) = {
        let view = state_ref(hwnd)
            .views
            .get(&active_view)
            .ok_or_else(|| "view state was not tracked".to_owned())?;
        let timeline = view_timeline(&active_view, view.content);
        let timeline_state = timeline_state_ref(timeline);
        let start_source_row_index =
            timeline_source_row_index_for_top_line(timeline, timeline_state.top_line).unwrap_or(0);
        let matches = compute_timeline_find_matches(&timeline_state.source_rows, &folded_query);
        let selected_match_index = choose_find_match(
            &matches,
            FindSelectionMode::Execute {
                folded_query: &folded_query,
                previous_state: previous_state.as_ref(),
                start_source_row_index,
            },
        );
        (matches, selected_match_index)
    };
    // Only rooms have older history to search.
    let next_state = match &active_view {
        ViewId::Status => {
            build_view_find_state(query, &folded_query, matches, selected_match_index)
        }
        ViewId::RoomList => unreachable!("the Rooms List has no find"),
        ViewId::Room(_) | ViewId::Thread { .. } => {
            build_timeline_find_state_and_maybe_submit_history_backfill(
                state_mut(hwnd),
                &active_view,
                query,
                &folded_query,
                matches,
                selected_match_index,
            )?
        }
    };

    state_mut(hwnd)
        .find_states
        .insert(active_view.clone(), next_state);
    apply_find_to_active_view(hwnd, &active_view)?;

    if let Some(find_state) = state_ref(hwnd).find_states.get(&active_view)
        && let Some(selected_match_index) = find_state.selected_match_index
        && let Some(selected_match) = find_state.matches.get(selected_match_index)
    {
        let view = state_ref(hwnd)
            .views
            .get(&active_view)
            .ok_or_else(|| "view state was not tracked".to_owned())?;
        scroll_timeline_to_source_row_index(
            view_timeline(&active_view, view.content),
            selected_match.source_row_index,
        );
    }

    refresh_status_bar(hwnd)?;
    Ok(view_find_outcome(
        &active_view,
        state_ref(hwnd)
            .find_states
            .get(&active_view)
            .expect("active view find state must be stored"),
    ))
}

pub(super) fn dump_active_find_state(state: &WindowState) -> Option<UiFindState> {
    state
        .find_states
        .get(&state.active_view)
        .map(|find_state| dump_find_state(&state.active_view, find_state))
}

fn dump_ui_find_history_search_state(
    history_search: &RoomFindHistorySearch,
) -> UiFindHistorySearchState {
    match history_search {
        RoomFindHistorySearch::LocalOnly => UiFindHistorySearchState::LocalOnly,
        RoomFindHistorySearch::Loading => UiFindHistorySearchState::Loading,
        RoomFindHistorySearch::Exhausted => UiFindHistorySearchState::Exhausted,
        RoomFindHistorySearch::Failed { message } => UiFindHistorySearchState::Failed {
            message: message.clone(),
        },
    }
}

pub(super) fn pending_timeline_find_history_views(state: &WindowState) -> Vec<ViewId> {
    state
        .find_states
        .iter()
        .filter(|(view_id, find_state)| {
            matches!(view_id, ViewId::Room(_) | ViewId::Thread { .. })
                && find_state.history_search == RoomFindHistorySearch::Loading
        })
        .map(|(view_id, _)| view_id.clone())
        .collect()
}

fn continue_timeline_find_history_search_after_refresh_state(
    state: &mut WindowState,
    view_id: &ViewId,
) -> Result<bool, String> {
    let room_id = match view_id {
        ViewId::Room(room_id) | ViewId::Thread { room_id, .. } => room_id,
        ViewId::Status => unreachable!("status find does not use room history"),
        ViewId::RoomList => unreachable!("the Rooms List has no find"),
    };
    let Some(find_state) = state.find_states.get(view_id) else {
        return Ok(false);
    };
    if find_state.history_search != RoomFindHistorySearch::Loading && find_state.matches.is_empty()
    {
        return Ok(false);
    }

    if !find_state.matches.is_empty() {
        let find_state = state
            .find_states
            .get_mut(view_id)
            .expect("matched timeline find state must remain tracked after refresh");
        find_state.history_search = RoomFindHistorySearch::LocalOnly;
        rebuild_view_find_feedback(find_state);
        return Ok(true);
    }

    let history_status = state
        .model
        .room_views
        .get(room_id)
        .map(|room_view| room_view.history.status.clone())
        .unwrap_or(RoomHistoryBackfillStatus::Idle);
    let history_search = match history_status {
        RoomHistoryBackfillStatus::Idle => {
            let _ = submit_backfill_room_history_for_room(state, room_id)?;
            RoomFindHistorySearch::Loading
        }
        RoomHistoryBackfillStatus::Loading { .. } => RoomFindHistorySearch::Loading,
        RoomHistoryBackfillStatus::Exhausted => RoomFindHistorySearch::Exhausted,
        RoomHistoryBackfillStatus::Failed { message, .. } => {
            RoomFindHistorySearch::Failed { message }
        }
    };
    let find_state = state
        .find_states
        .get_mut(view_id)
        .expect("loading timeline find state must remain tracked after refresh");
    find_state.history_search = history_search;
    rebuild_view_find_feedback(find_state);
    Ok(false)
}

pub(super) unsafe fn continue_timeline_find_history_search_after_refresh(
    hwnd: HWND,
    view_id: &ViewId,
) -> Result<(), String> {
    let has_matches =
        continue_timeline_find_history_search_after_refresh_state(state_mut(hwnd), view_id)?;

    apply_find_to_active_view(hwnd, view_id)?;
    if has_matches && state_ref(hwnd).active_view == *view_id {
        let selected_match = state_ref(hwnd)
            .find_states
            .get(view_id)
            .and_then(|find_state| {
                find_state
                    .selected_match_index
                    .and_then(|index| find_state.matches.get(index))
            })
            .cloned()
            .expect("matched timeline find state must retain a selected match");
        let view = state_ref(hwnd)
            .views
            .get(view_id)
            .expect("timeline view must remain tracked after refresh");
        scroll_timeline_to_source_row_index(
            room_content_timeline(view.content),
            selected_match.source_row_index,
        );
    }
    refresh_status_bar(hwnd)
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;

    #[test]
    fn ascii_find_matching_folds_ascii_only() {
        assert_eq!(fold_find_query("HeLLo Ä"), "hello Ä");
        assert!(ascii_case_insensitive_contains(
            "Hello world",
            &fold_find_query("hello")
        ));
        assert!(ascii_case_insensitive_contains(
            "AÄ",
            &fold_find_query("aÄ")
        ));
        assert!(!ascii_case_insensitive_contains("Ä", &fold_find_query("ä")));
    }

    #[test]
    fn status_find_matches_keep_identical_lines_distinct() {
        let mut log = StatusLog::default();
        log.push(
            0,
            StatusLineKind::Info,
            "* Synced rooms: 1 joined, 0 invited".to_owned(),
        );
        log.push(
            0,
            StatusLineKind::Info,
            "* E2EE status not yet known".to_owned(),
        );
        log.push(
            0,
            StatusLineKind::Info,
            "* E2EE status not yet known".to_owned(),
        );
        let rows = build_status_timeline_rows(&log, utc_timestamps());

        let matches = compute_timeline_find_matches(&rows, &fold_find_query("e2ee"));
        assert_eq!(
            matches,
            vec![
                FindMatch {
                    row_identity: TimelineRowIdentity::status_line(1),
                    source_row_index: 1,
                    logical_text: "[00:00] * E2EE status not yet known".to_owned(),
                },
                FindMatch {
                    row_identity: TimelineRowIdentity::status_line(2),
                    source_row_index: 2,
                    logical_text: "[00:00] * E2EE status not yet known".to_owned(),
                },
            ]
        );
    }
    #[test]
    fn find_selection_starts_at_the_first_row_in_view() {
        let mut log = StatusLog::default();
        for text in ["alpha", "beta", "needle", "needle again"] {
            log.push(0, StatusLineKind::Info, text.to_owned());
        }
        let matches = compute_timeline_find_matches(
            &build_status_timeline_rows(&log, utc_timestamps()),
            &fold_find_query("needle"),
        );

        assert_eq!(
            choose_find_match(
                &matches,
                FindSelectionMode::Execute {
                    folded_query: "needle",
                    previous_state: None,
                    start_source_row_index: 1,
                },
            ),
            Some(0)
        );
        assert_eq!(
            choose_find_match(
                &matches,
                FindSelectionMode::Execute {
                    folded_query: "needle",
                    previous_state: None,
                    start_source_row_index: 3,
                },
            ),
            Some(1)
        );
    }
    #[test]
    fn timeline_find_matches_are_per_logical_row() {
        let rows = vec![
            TimelineRow::full_width(TimelineRowKind::Plain, "alpha needle needle".to_owned())
                .with_identity(TimelineRowIdentity::synthetic(
                    TimelineSyntheticRowLabel::Plain,
                    "alpha needle needle",
                )),
            TimelineRow::full_width(TimelineRowKind::Plain, "beta needle".to_owned())
                .with_identity(TimelineRowIdentity::synthetic(
                    TimelineSyntheticRowLabel::Plain,
                    "beta needle",
                )),
        ];

        let matches = compute_timeline_find_matches(&rows, &fold_find_query("needle"));
        assert_eq!(matches.len(), 2);
        assert_eq!(
            matches,
            vec![
                FindMatch {
                    row_identity: TimelineRowIdentity::synthetic(
                        TimelineSyntheticRowLabel::Plain,
                        "alpha needle needle",
                    ),
                    source_row_index: 0,
                    logical_text: "alpha needle needle".to_owned(),
                },
                FindMatch {
                    row_identity: TimelineRowIdentity::synthetic(
                        TimelineSyntheticRowLabel::Plain,
                        "beta needle",
                    ),
                    source_row_index: 1,
                    logical_text: "beta needle".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn repeated_find_query_advances_and_wraps() {
        let matches = vec![
            FindMatch {
                row_identity: TimelineRowIdentity::activity_primary("activity-1"),
                source_row_index: 1,
                logical_text: "first needle".to_owned(),
            },
            FindMatch {
                row_identity: TimelineRowIdentity::activity_primary("activity-2"),
                source_row_index: 4,
                logical_text: "second needle".to_owned(),
            },
        ];
        let previous_state = build_view_find_state(
            "needle",
            &fold_find_query("needle"),
            matches.clone(),
            Some(1),
        );

        assert_eq!(
            choose_find_match(
                &matches,
                FindSelectionMode::Execute {
                    folded_query: "needle",
                    previous_state: Some(&previous_state),
                    start_source_row_index: 0,
                },
            ),
            Some(0)
        );
    }

    #[test]
    fn recompute_find_selection_preserves_timeline_identity_and_reselects_first_match_when_missing()
    {
        let previous_state = build_view_find_state(
            "needle",
            &fold_find_query("needle"),
            vec![
                FindMatch {
                    row_identity: TimelineRowIdentity::activity_primary("activity-1"),
                    source_row_index: 0,
                    logical_text: "first needle".to_owned(),
                },
                FindMatch {
                    row_identity: TimelineRowIdentity::activity_primary("activity-2"),
                    source_row_index: 1,
                    logical_text: "second needle".to_owned(),
                },
            ],
            Some(1),
        );
        let reordered_matches = vec![
            FindMatch {
                row_identity: TimelineRowIdentity::activity_primary("activity-2"),
                source_row_index: 0,
                logical_text: "second needle".to_owned(),
            },
            FindMatch {
                row_identity: TimelineRowIdentity::activity_primary("activity-1"),
                source_row_index: 1,
                logical_text: "first needle".to_owned(),
            },
        ];
        let missing_selected_row_matches = vec![FindMatch {
            row_identity: TimelineRowIdentity::activity_primary("activity-1"),
            source_row_index: 0,
            logical_text: "first needle".to_owned(),
        }];

        assert_eq!(
            choose_find_match(
                &reordered_matches,
                FindSelectionMode::Recompute {
                    previous_state: &previous_state,
                },
            ),
            Some(0)
        );
        assert_eq!(
            choose_find_match(
                &missing_selected_row_matches,
                FindSelectionMode::Recompute {
                    previous_state: &previous_state,
                },
            ),
            Some(0)
        );
        assert_eq!(
            choose_find_match(
                &[],
                FindSelectionMode::Recompute {
                    previous_state: &previous_state,
                },
            ),
            None
        );
    }

    #[test]
    fn no_match_find_state_is_deterministic() {
        let state = build_view_find_state("missing", "missing", Vec::new(), None);

        assert!(state.matches.is_empty());
        assert_eq!(state.selected_match_index, None);
        assert_eq!(state.history_search, RoomFindHistorySearch::LocalOnly);
        assert_eq!(state.feedback, "find: no matches for \"missing\"");
    }

    #[test]
    fn room_find_local_match_stays_local_without_history_backfill() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        let matches = vec![FindMatch {
            row_identity: TimelineRowIdentity::activity_primary("activity-1"),
            source_row_index: 0,
            logical_text: "needle".to_owned(),
        }];

        let find_state = build_timeline_find_state_and_maybe_submit_history_backfill(
            &mut state,
            &room_view_id("!room:example"),
            "needle",
            "needle",
            matches,
            Some(0),
        )
        .unwrap();

        assert_eq!(find_state.history_search, RoomFindHistorySearch::LocalOnly);
        assert_eq!(find_state.feedback, "find: 1/1 \"needle\"");
        assert!(state.recorded_commands.is_empty());
        stop_test_core(handle);
    }

    #[test]
    fn room_find_miss_from_idle_submits_history_backfill_and_shows_loading_feedback() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        let find_state = build_timeline_find_state_and_maybe_submit_history_backfill(
            &mut state,
            &room_view_id("!room:example"),
            "needle",
            "needle",
            Vec::new(),
            None,
        )
        .unwrap();

        assert_eq!(find_state.history_search, RoomFindHistorySearch::Loading);
        assert_eq!(
            find_state.feedback,
            "find: loading older history for \"needle\""
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: None,
                limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
            })]
        );
        stop_test_core(handle);
    }

    #[test]
    fn room_find_miss_while_history_is_loading_does_not_submit_a_duplicate() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .model
            .room_views
            .entry("!room:example".to_owned())
            .or_default()
            .history
            .status = RoomHistoryBackfillStatus::Loading { from: None };

        let find_state = build_timeline_find_state_and_maybe_submit_history_backfill(
            &mut state,
            &room_view_id("!room:example"),
            "needle",
            "needle",
            Vec::new(),
            None,
        )
        .unwrap();

        assert_eq!(find_state.history_search, RoomFindHistorySearch::Loading);
        assert_eq!(
            find_state.feedback,
            "find: loading older history for \"needle\""
        );
        assert!(state.recorded_commands.is_empty());
        stop_test_core(handle);
    }

    #[test]
    fn thread_find_blocked_by_another_room_backfill_stays_pending_and_retries() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![
            room("!room-a:example", "Room A", false),
            room("!room-b:example", "Room B", false),
        ];
        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room-a:example"),
            Ok(true)
        );

        let view_id = thread_view_id("!room-b:example", "$thread-root");
        let find_state = build_timeline_find_state_and_maybe_submit_history_backfill(
            &mut state,
            &view_id,
            "needle",
            "needle",
            Vec::new(),
            None,
        )
        .unwrap();
        assert_eq!(find_state.history_search, RoomFindHistorySearch::Loading);
        assert_eq!(state.recorded_commands.len(), 1);
        state.find_states.insert(view_id.clone(), find_state);
        assert_eq!(
            pending_timeline_find_history_views(&state),
            vec![view_id.clone()]
        );

        apply_core_event(
            &mut state,
            room_history_backfill("!room-a:example", "token-a", Some("token-b"), Vec::new()),
        )
        .unwrap();
        assert!(
            !continue_timeline_find_history_search_after_refresh_state(&mut state, &view_id,)
                .unwrap()
        );

        assert_eq!(state.recorded_commands.len(), 2);
        assert_eq!(
            state.recorded_commands.last(),
            Some(&Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
                room_id: "!room-b:example".to_owned(),
                from: None,
                limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
            }))
        );
        assert_eq!(
            state
                .find_states
                .get(&view_id)
                .map(|find_state| &find_state.history_search),
            Some(&RoomFindHistorySearch::Loading)
        );
        stop_test_core(handle);
    }

    #[test]
    fn pending_room_find_transitions_to_exhausted_or_failed_after_refresh() {
        let mut state = test_state();
        let view_id = room_view_id("!room:example");
        state.find_states.insert(
            view_id.clone(),
            build_view_find_state_with_history(
                "needle",
                "needle",
                Vec::new(),
                None,
                RoomFindHistorySearch::Loading,
            ),
        );
        let room_view = state
            .model
            .room_views
            .entry("!room:example".to_owned())
            .or_default();
        room_view.history.status = RoomHistoryBackfillStatus::Exhausted;

        assert!(
            !continue_timeline_find_history_search_after_refresh_state(&mut state, &view_id,)
                .unwrap()
        );
        let find_state = state.find_states.get(&view_id).unwrap();
        assert_eq!(find_state.history_search, RoomFindHistorySearch::Exhausted);
        assert_eq!(
            find_state.feedback,
            "find: no matches for \"needle\" in loaded or older history"
        );

        state.find_states.insert(
            view_id.clone(),
            build_view_find_state_with_history(
                "needle",
                "needle",
                Vec::new(),
                None,
                RoomFindHistorySearch::Loading,
            ),
        );
        state
            .model
            .room_views
            .get_mut("!room:example")
            .unwrap()
            .history
            .next_token = Some("retry-token".to_owned());
        state
            .model
            .room_views
            .get_mut("!room:example")
            .unwrap()
            .history
            .status = RoomHistoryBackfillStatus::Failed {
            from: Some("retry-token".to_owned()),
            message: "homeserver unavailable".to_owned(),
        };

        assert!(
            !continue_timeline_find_history_search_after_refresh_state(&mut state, &view_id,)
                .unwrap()
        );
        let find_state = state.find_states.get(&view_id).unwrap();
        assert_eq!(
            find_state.history_search,
            RoomFindHistorySearch::Failed {
                message: "homeserver unavailable".to_owned(),
            }
        );
        assert_eq!(
            find_state.feedback,
            "find: unable to load older history for \"needle\": homeserver unavailable"
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|room_view| room_view.history.next_token.as_deref()),
            Some(Some("retry-token"))
        );
    }

    #[test]
    fn room_find_recompute_with_new_match_clears_loading_history_search() {
        let mut state = test_state();
        let view_id = room_view_id("!room:example");
        state.find_states.insert(
            view_id.clone(),
            build_view_find_state_with_history(
                "needle",
                "needle",
                Vec::new(),
                None,
                RoomFindHistorySearch::Loading,
            ),
        );
        let rows = vec![
            TimelineRow::full_width(TimelineRowKind::Plain, "older needle".to_owned())
                .with_identity(TimelineRowIdentity::synthetic(
                    TimelineSyntheticRowLabel::Plain,
                    "older needle",
                )),
        ];

        recompute_find_for_view(&mut state, &view_id, &rows);

        let find_state = state.find_states.get(&view_id).unwrap();
        assert_eq!(find_state.history_search, RoomFindHistorySearch::LocalOnly);
        assert_eq!(find_state.feedback, "find: 1/1 \"needle\"");
    }

    #[test]
    fn room_find_history_feedback_reports_exhaustion_and_failure() {
        let exhausted = build_view_find_state_with_history(
            "needle",
            "needle",
            Vec::new(),
            None,
            RoomFindHistorySearch::Exhausted,
        );
        let failed = build_view_find_state_with_history(
            "needle",
            "needle",
            Vec::new(),
            None,
            RoomFindHistorySearch::Failed {
                message: "homeserver unavailable".to_owned(),
            },
        );

        assert_eq!(
            exhausted.feedback,
            "find: no matches for \"needle\" in loaded or older history"
        );
        assert_eq!(
            failed.feedback,
            "find: unable to load older history for \"needle\": homeserver unavailable"
        );
    }

    #[test]
    fn thread_find_miss_submits_parent_room_history_backfill() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        let view_id = thread_view_id("!room:example", "$thread-root");

        let find_state = build_timeline_find_state_and_maybe_submit_history_backfill(
            &mut state,
            &view_id,
            "needle",
            "needle",
            Vec::new(),
            None,
        )
        .unwrap();

        assert_eq!(find_state.history_search, RoomFindHistorySearch::Loading);
        assert_eq!(
            state.recorded_commands,
            vec![Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: None,
                limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
            })]
        );
        stop_test_core(handle);
    }

    #[test]
    fn thread_find_local_match_stays_local_without_history_backfill() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        let find_state = build_timeline_find_state_and_maybe_submit_history_backfill(
            &mut state,
            &thread_view_id("!room:example", "$thread-root"),
            "needle",
            "needle",
            vec![FindMatch {
                row_identity: TimelineRowIdentity::activity_primary("activity-1"),
                source_row_index: 0,
                logical_text: "needle".to_owned(),
            }],
            Some(0),
        )
        .unwrap();

        assert_eq!(find_state.history_search, RoomFindHistorySearch::LocalOnly);
        assert!(state.recorded_commands.is_empty());
        stop_test_core(handle);
    }

    #[test]
    fn pending_thread_find_transitions_after_refresh() {
        let mut state = test_state();
        let view_id = thread_view_id("!room:example", "$thread-root");
        state.find_states.insert(
            view_id.clone(),
            build_view_find_state_with_history(
                "needle",
                "needle",
                Vec::new(),
                None,
                RoomFindHistorySearch::Loading,
            ),
        );
        let rows = vec![
            TimelineRow::full_width(TimelineRowKind::Plain, "older needle".to_owned())
                .with_identity(TimelineRowIdentity::synthetic(
                    TimelineSyntheticRowLabel::Plain,
                    "older needle",
                )),
        ];

        recompute_find_for_view(&mut state, &view_id, &rows);

        assert!(
            continue_timeline_find_history_search_after_refresh_state(&mut state, &view_id)
                .unwrap()
        );
        assert_eq!(
            state
                .find_states
                .get(&view_id)
                .map(|find_state| &find_state.history_search),
            Some(&RoomFindHistorySearch::LocalOnly)
        );
    }

    #[test]
    fn pending_thread_find_transitions_to_exhausted_or_failed_after_refresh() {
        let mut state = test_state();
        let view_id = thread_view_id("!room:example", "$thread-root");
        state.find_states.insert(
            view_id.clone(),
            build_view_find_state_with_history(
                "needle",
                "needle",
                Vec::new(),
                None,
                RoomFindHistorySearch::Loading,
            ),
        );
        state
            .model
            .room_views
            .entry("!room:example".to_owned())
            .or_default()
            .history
            .status = RoomHistoryBackfillStatus::Exhausted;

        assert!(
            !continue_timeline_find_history_search_after_refresh_state(&mut state, &view_id)
                .unwrap()
        );
        assert_eq!(
            state
                .find_states
                .get(&view_id)
                .map(|find_state| &find_state.history_search),
            Some(&RoomFindHistorySearch::Exhausted)
        );

        state.find_states.insert(
            view_id.clone(),
            build_view_find_state_with_history(
                "needle",
                "needle",
                Vec::new(),
                None,
                RoomFindHistorySearch::Loading,
            ),
        );
        state
            .model
            .room_views
            .get_mut("!room:example")
            .unwrap()
            .history
            .status = RoomHistoryBackfillStatus::Failed {
            from: None,
            message: "homeserver unavailable".to_owned(),
        };
        assert!(
            !continue_timeline_find_history_search_after_refresh_state(&mut state, &view_id)
                .unwrap()
        );
        assert_eq!(
            state
                .find_states
                .get(&view_id)
                .map(|find_state| &find_state.history_search),
            Some(&RoomFindHistorySearch::Failed {
                message: "homeserver unavailable".to_owned(),
            })
        );
    }
}
