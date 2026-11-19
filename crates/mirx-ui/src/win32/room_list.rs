//! The Rooms List window's listing: mIRC's Channels List over a server's
//! public room directory, filled page by page, sorted by a column.
//!
//! Pure code, no Win32 calls.

use std::cmp::Ordering;

use mirx_core::{ListPublicRoomsRequest, PublicRoom, PublicRoomsPage, PublicRoomsResult};

/// Rooms asked for per directory request.
pub(super) const ROOM_LIST_PAGE_SIZE: u32 = 100;
/// The most rooms a listing holds; a large directory (matrix.org has tens
/// of thousands) stops here, and the caption asks for a narrower match.
pub(super) const ROOM_LIST_CAP: usize = 1000;

/// What `/list` asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RoomListQuery {
    /// Another server's directory; `None` lists the user's homeserver.
    pub(super) server: Option<String>,
    /// The server-side match text; `None` lists every room.
    pub(super) filter: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RoomListColumn {
    Room,
    Users,
    Name,
    Topic,
}

impl RoomListColumn {
    /// The columns in display order; a column's index is its ListView
    /// subitem.
    pub(super) const ALL: [Self; 4] = [Self::Room, Self::Users, Self::Name, Self::Topic];

    pub(super) fn from_index(index: usize) -> Self {
        Self::ALL[index]
    }

    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Room => "Room",
            Self::Users => "Users",
            Self::Name => "Name",
            Self::Topic => "Topic",
        }
    }

    /// Counts read best biggest first, text from A.
    fn sorts_descending_first(self) -> bool {
        match self {
            Self::Users => true,
            Self::Room | Self::Name | Self::Topic => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RoomListPhase {
    /// Waiting for the page that starts at `awaiting` (`None`: the first).
    Listing {
        awaiting: Option<String>,
    },
    Complete,
    /// `ROOM_LIST_CAP` reached with more rooms on the server.
    Capped {
        total_estimate: Option<u64>,
    },
    Failed,
}

/// A page that was not the one awaited: for another query, a stale or
/// repeated `since`, or after the listing ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Ignored;

/// What applying the awaited page leads to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PageApplied {
    /// More rooms: send this request for the next page.
    Next(ListPublicRoomsRequest),
    /// The listing is complete or capped.
    Finished,
    /// The page failed with this message; the rooms listed stay.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RoomListState {
    query: RoomListQuery,
    /// The server the caption names: the query's, else the user's own.
    server_label: String,
    /// In server order.
    rooms: Vec<PublicRoom>,
    /// Display order: indices into `rooms`.
    order: Vec<usize>,
    /// The sorted column and whether it sorts descending; `None` keeps
    /// server order.
    sort: Option<(RoomListColumn, bool)>,
    phase: RoomListPhase,
}

impl RoomListState {
    /// An empty listing awaiting its first page; `own_server` is the user's
    /// homeserver name.
    pub(super) fn new(query: RoomListQuery, own_server: &str) -> Self {
        let server_label = query
            .server
            .clone()
            .unwrap_or_else(|| own_server.to_owned());
        Self {
            query,
            server_label,
            rooms: Vec::new(),
            order: Vec::new(),
            sort: None,
            phase: RoomListPhase::Listing { awaiting: None },
        }
    }

    fn request(&self, since: Option<String>) -> ListPublicRoomsRequest {
        ListPublicRoomsRequest {
            server: self.query.server.clone(),
            filter: self.query.filter.clone(),
            since,
            limit: ROOM_LIST_PAGE_SIZE,
        }
    }

    pub(super) fn first_request(&self) -> ListPublicRoomsRequest {
        self.request(None)
    }

    /// Applies the awaited page; any other page is ignored, so a page
    /// delivered twice, or one for an earlier `/list`, changes nothing.
    pub(super) fn apply_page(&mut self, page: PublicRoomsPage) -> Result<PageApplied, Ignored> {
        let RoomListPhase::Listing { awaiting } = &self.phase else {
            return Err(Ignored);
        };
        if page.request != self.request(awaiting.clone()) {
            return Err(Ignored);
        }
        let (rooms, next_batch, total_estimate) = match page.result {
            PublicRoomsResult::Loaded {
                rooms,
                next_batch,
                total_estimate,
            } => (rooms, next_batch, total_estimate),
            PublicRoomsResult::Failed { message } => {
                self.phase = RoomListPhase::Failed;
                return Ok(PageApplied::Failed(message));
            }
        };

        // A page with no rooms ends the listing even with a `next_batch`,
        // or a server that keeps answering empty pages would be asked
        // forever.
        let page_was_empty = rooms.is_empty();
        let room_capacity = ROOM_LIST_CAP - self.rooms.len();
        self.rooms.extend(rooms.into_iter().take(room_capacity));
        self.resort();
        match next_batch.filter(|_| !page_was_empty) {
            None => {
                self.phase = RoomListPhase::Complete;
                Ok(PageApplied::Finished)
            }
            Some(_) if self.rooms.len() == ROOM_LIST_CAP => {
                self.phase = RoomListPhase::Capped { total_estimate };
                Ok(PageApplied::Finished)
            }
            Some(next_batch) => {
                let request = self.request(Some(next_batch.clone()));
                self.phase = RoomListPhase::Listing {
                    awaiting: Some(next_batch),
                };
                Ok(PageApplied::Next(request))
            }
        }
    }

    /// A column header click: sort by that column, or flip the direction
    /// of the column already sorted by.
    pub(super) fn sort_by(&mut self, column: RoomListColumn) {
        let descending = match self.sort {
            Some((sorted, descending)) if sorted == column => !descending,
            _ => column.sorts_descending_first(),
        };
        self.sort = Some((column, descending));
        self.resort();
    }

    /// Recomputes the display order; the sort is stable, so equal rows keep
    /// server order.
    fn resort(&mut self) {
        self.order = (0..self.rooms.len()).collect();
        let Some((column, descending)) = self.sort else {
            return;
        };
        let rooms = &self.rooms;
        self.order.sort_by(|&left, &right| {
            let ordering = compare(column, &rooms[left], &rooms[right]);
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }

    pub(super) fn len(&self) -> usize {
        self.order.len()
    }

    pub(super) fn row(&self, display_index: usize) -> &PublicRoom {
        &self.rooms[self.order[display_index]]
    }

    pub(super) fn cell_text(&self, display_index: usize, column: RoomListColumn) -> String {
        cell_text(self.row(display_index), column)
    }

    /// What `/join` takes for a row, and the servers to join a room ID
    /// through. An alias resolves its own servers; a room on the user's own
    /// homeserver's directory is one it participates in. Only a room ID
    /// from another server's directory needs that server. The server is
    /// never taken from the room ID: newer room IDs have none.
    pub(super) fn join_target(&self, display_index: usize) -> (String, Vec<String>) {
        let room = self.row(display_index);
        match &room.canonical_alias {
            Some(alias) => (alias.clone(), Vec::new()),
            None => (
                room.room_id.clone(),
                self.query.server.iter().cloned().collect(),
            ),
        }
    }

    pub(super) fn caption(&self) -> String {
        let filter = self
            .query
            .filter
            .as_ref()
            .map(|filter| format!(" \"{filter}\""))
            .unwrap_or_default();
        let count = self.rooms.len();
        let progress = match &self.phase {
            RoomListPhase::Listing { .. } => "listing...".to_owned(),
            RoomListPhase::Complete => rooms_count(count),
            RoomListPhase::Capped {
                total_estimate: Some(total),
            } => format!("{count} of ~{total} rooms"),
            RoomListPhase::Capped {
                total_estimate: None,
            } => format!("first {count} rooms"),
            RoomListPhase::Failed => "failed".to_owned(),
        };
        format!("Rooms List [{}]{filter} - {progress}", self.server_label)
    }
}

fn rooms_count(count: usize) -> String {
    match count {
        1 => "1 room".to_owned(),
        count => format!("{count} rooms"),
    }
}

fn cell_text(room: &PublicRoom, column: RoomListColumn) -> String {
    match column {
        RoomListColumn::Room => room
            .canonical_alias
            .clone()
            .unwrap_or_else(|| room.room_id.clone()),
        RoomListColumn::Users => room.joined_members.to_string(),
        RoomListColumn::Name => room.name.clone().unwrap_or_default(),
        RoomListColumn::Topic => room
            .topic
            .as_deref()
            .and_then(|topic| topic.lines().next())
            .unwrap_or_default()
            .to_owned(),
    }
}

fn compare(column: RoomListColumn, left: &PublicRoom, right: &PublicRoom) -> Ordering {
    match column {
        RoomListColumn::Users => left.joined_members.cmp(&right.joined_members),
        RoomListColumn::Room | RoomListColumn::Name | RoomListColumn::Topic => {
            cell_text(left, column)
                .to_lowercase()
                .cmp(&cell_text(right, column).to_lowercase())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public_room(
        room_id: &str,
        alias: Option<&str>,
        name: Option<&str>,
        topic: Option<&str>,
        joined_members: u64,
    ) -> PublicRoom {
        PublicRoom {
            room_id: room_id.to_owned(),
            canonical_alias: alias.map(str::to_owned),
            name: name.map(str::to_owned),
            topic: topic.map(str::to_owned),
            joined_members,
        }
    }

    fn query(server: Option<&str>, filter: Option<&str>) -> RoomListQuery {
        RoomListQuery {
            server: server.map(str::to_owned),
            filter: filter.map(str::to_owned),
        }
    }

    fn loaded(
        request: ListPublicRoomsRequest,
        rooms: Vec<PublicRoom>,
        next_batch: Option<&str>,
        total_estimate: Option<u64>,
    ) -> PublicRoomsPage {
        PublicRoomsPage {
            request,
            result: PublicRoomsResult::Loaded {
                rooms,
                next_batch: next_batch.map(str::to_owned),
                total_estimate,
            },
        }
    }

    fn numbered_rooms(start: usize, count: usize) -> Vec<PublicRoom> {
        (start..start + count)
            .map(|index| public_room(&format!("!r{index}:example"), None, None, None, 1))
            .collect()
    }

    fn room_column(state: &RoomListState) -> Vec<String> {
        (0..state.len())
            .map(|index| state.cell_text(index, RoomListColumn::Room))
            .collect()
    }

    /// Five rooms whose columns sort differently; `#b` and `#c` tie on
    /// users, `#a` and `!d` on an empty name.
    fn sortable_state() -> RoomListState {
        let mut state = RoomListState::new(query(None, None), "example");
        let request = state.first_request();
        state
            .apply_page(loaded(
                request,
                vec![
                    public_room("!1:example", Some("#c:example"), Some("Zeta"), Some("b"), 5),
                    public_room(
                        "!2:example",
                        Some("#b:example"),
                        Some("alpha"),
                        Some("A"),
                        5,
                    ),
                    public_room("!3:example", Some("#a:example"), None, None, 50),
                    public_room("!d:example", None, None, Some("c"), 1),
                    public_room("!5:example", Some("#E:example"), Some("Beta"), None, 9),
                ],
                None,
                None,
            ))
            .unwrap();
        state
    }

    #[test]
    fn first_request_asks_for_a_page_of_the_query() {
        let state = RoomListState::new(query(Some("matrix.org"), Some("rust")), "example");
        assert_eq!(
            state.first_request(),
            ListPublicRoomsRequest {
                server: Some("matrix.org".to_owned()),
                filter: Some("rust".to_owned()),
                since: None,
                limit: ROOM_LIST_PAGE_SIZE,
            }
        );
    }

    #[test]
    fn pages_with_next_batch_request_the_next_page() {
        let mut state = RoomListState::new(query(None, Some("rust")), "example");
        let first = state.first_request();

        let next = state
            .apply_page(loaded(first, numbered_rooms(0, 100), Some("b1"), Some(150)))
            .unwrap();
        let PageApplied::Next(second) = next else {
            panic!("expected a next request, got {next:?}");
        };
        assert_eq!(second.since.as_deref(), Some("b1"));
        assert_eq!(second.filter.as_deref(), Some("rust"));
        assert_eq!(state.len(), 100);
        assert_eq!(
            state.caption(),
            "Rooms List [example] \"rust\" - listing..."
        );

        assert_eq!(
            state.apply_page(loaded(second, numbered_rooms(100, 50), None, Some(150))),
            Ok(PageApplied::Finished)
        );
        assert_eq!(state.len(), 150);
        assert_eq!(state.caption(), "Rooms List [example] \"rust\" - 150 rooms");
        assert_eq!(state.cell_text(149, RoomListColumn::Room), "!r149:example");
    }

    #[test]
    fn the_cap_stops_paging() {
        let mut state = RoomListState::new(query(Some("matrix.org"), None), "example");
        let mut request = state.first_request();
        for page in 0..9 {
            let next = state
                .apply_page(loaded(
                    request,
                    numbered_rooms(page * 100, 100),
                    Some(&format!("b{page}")),
                    Some(38214),
                ))
                .unwrap();
            let PageApplied::Next(next) = next else {
                panic!("page {page} must request the next, got {next:?}");
            };
            request = next;
        }

        // The server sends more than asked; the cap still holds.
        assert_eq!(
            state.apply_page(loaded(
                request,
                numbered_rooms(900, 150),
                Some("b9"),
                Some(38214)
            )),
            Ok(PageApplied::Finished)
        );
        assert_eq!(state.len(), ROOM_LIST_CAP);
        assert_eq!(
            state.caption(),
            "Rooms List [matrix.org] - 1000 of ~38214 rooms"
        );
    }

    #[test]
    fn capped_without_an_estimate_says_first() {
        let mut state = RoomListState::new(query(None, None), "example");
        let mut request = state.first_request();
        for page in 0..10 {
            match state
                .apply_page(loaded(
                    request.clone(),
                    numbered_rooms(page * 100, 100),
                    Some(&format!("b{page}")),
                    None,
                ))
                .unwrap()
            {
                PageApplied::Next(next) => request = next,
                PageApplied::Finished => assert_eq!(page, 9),
                PageApplied::Failed(message) => panic!("unexpected failure {message}"),
            }
        }
        assert_eq!(state.caption(), "Rooms List [example] - first 1000 rooms");
    }

    #[test]
    fn a_page_without_next_batch_or_rooms_completes_the_listing() {
        let mut state = RoomListState::new(query(None, None), "example");
        let request = state.first_request();
        assert_eq!(
            state.apply_page(loaded(request, Vec::new(), Some("b1"), None)),
            Ok(PageApplied::Finished)
        );
        assert_eq!(state.caption(), "Rooms List [example] - 0 rooms");

        let mut state = RoomListState::new(query(None, None), "example");
        let request = state.first_request();
        state
            .apply_page(loaded(request, numbered_rooms(0, 1), None, None))
            .unwrap();
        assert_eq!(state.caption(), "Rooms List [example] - 1 room");
    }

    #[test]
    fn pages_for_another_query_or_a_stale_since_are_ignored() {
        let mut state = RoomListState::new(query(None, Some("b")), "example");
        let other_query = RoomListState::new(query(None, Some("a")), "example").first_request();
        assert_eq!(
            state.apply_page(loaded(other_query, numbered_rooms(0, 3), None, None)),
            Err(Ignored)
        );
        assert_eq!(state.len(), 0);

        let first = state.first_request();
        let PageApplied::Next(second) = state
            .apply_page(loaded(
                first.clone(),
                numbered_rooms(0, 3),
                Some("b1"),
                None,
            ))
            .unwrap()
        else {
            panic!("expected a next request");
        };
        // The first page again, as a duplicate delivery would bring it.
        assert_eq!(
            state.apply_page(loaded(first, numbered_rooms(0, 3), Some("b1"), None)),
            Err(Ignored)
        );
        let mut stale = second.clone();
        stale.since = Some("b0".to_owned());
        assert_eq!(
            state.apply_page(loaded(stale, numbered_rooms(3, 3), None, None)),
            Err(Ignored)
        );
        assert_eq!(state.len(), 3);

        state
            .apply_page(loaded(second.clone(), numbered_rooms(3, 3), None, None))
            .unwrap();
        // Once complete, nothing more applies.
        assert_eq!(
            state.apply_page(loaded(second, numbered_rooms(3, 3), None, None)),
            Err(Ignored)
        );
        assert_eq!(state.len(), 6);
    }

    #[test]
    fn a_failed_page_keeps_the_rooms_listed() {
        let mut state = RoomListState::new(query(Some("matrix.org"), None), "example");
        let first = state.first_request();
        let PageApplied::Next(second) = state
            .apply_page(loaded(first, numbered_rooms(0, 2), Some("b1"), None))
            .unwrap()
        else {
            panic!("expected a next request");
        };

        assert_eq!(
            state.apply_page(PublicRoomsPage {
                request: second,
                result: PublicRoomsResult::Failed {
                    message: "directory down".to_owned(),
                },
            }),
            Ok(PageApplied::Failed("directory down".to_owned()))
        );
        assert_eq!(state.len(), 2);
        assert_eq!(state.caption(), "Rooms List [matrix.org] - failed");
    }

    #[test]
    fn users_sorts_descending_first_and_flips_on_a_second_click() {
        let mut state = sortable_state();
        assert_eq!(
            room_column(&state),
            [
                "#c:example",
                "#b:example",
                "#a:example",
                "!d:example",
                "#E:example"
            ],
            "server order before any click"
        );

        state.sort_by(RoomListColumn::Users);
        // `#c` and `#b` tie at 5 and keep server order.
        assert_eq!(
            room_column(&state),
            [
                "#a:example",
                "#E:example",
                "#c:example",
                "#b:example",
                "!d:example"
            ]
        );

        state.sort_by(RoomListColumn::Users);
        assert_eq!(
            room_column(&state),
            [
                "!d:example",
                "#c:example",
                "#b:example",
                "#E:example",
                "#a:example"
            ]
        );
    }

    #[test]
    fn text_columns_sort_ascending_case_insensitively() {
        let mut state = sortable_state();

        state.sort_by(RoomListColumn::Room);
        assert_eq!(
            room_column(&state),
            [
                "!d:example",
                "#a:example",
                "#b:example",
                "#c:example",
                "#E:example"
            ]
        );
        state.sort_by(RoomListColumn::Room);
        assert_eq!(
            room_column(&state),
            [
                "#E:example",
                "#c:example",
                "#b:example",
                "#a:example",
                "!d:example"
            ]
        );

        // Empty names tie and keep server order: `#a` before `!d`.
        state.sort_by(RoomListColumn::Name);
        assert_eq!(
            room_column(&state),
            [
                "#a:example",
                "!d:example",
                "#b:example",
                "#E:example",
                "#c:example"
            ]
        );

        state.sort_by(RoomListColumn::Topic);
        assert_eq!(
            room_column(&state),
            [
                "#a:example",
                "#E:example",
                "#b:example",
                "#c:example",
                "!d:example"
            ]
        );
    }

    #[test]
    fn a_later_page_joins_the_current_sort() {
        let mut state = RoomListState::new(query(None, None), "example");
        state.sort_by(RoomListColumn::Users);
        let first = state.first_request();
        let PageApplied::Next(second) = state
            .apply_page(loaded(
                first,
                vec![public_room("!a:example", None, None, None, 2)],
                Some("b1"),
                None,
            ))
            .unwrap()
        else {
            panic!("expected a next request");
        };
        state
            .apply_page(loaded(
                second,
                vec![public_room("!b:example", None, None, None, 7)],
                None,
                None,
            ))
            .unwrap();

        assert_eq!(room_column(&state), ["!b:example", "!a:example"]);
    }

    #[test]
    fn join_target_uses_the_alias_else_the_id_via_the_listed_server() {
        let rooms = vec![
            public_room(
                "!a:other.example",
                Some("#ops:other.example"),
                None,
                None,
                3,
            ),
            public_room("!opaque", None, None, None, 2),
        ];
        let listed = |server: Option<&str>| {
            let mut state = RoomListState::new(query(server, None), "example");
            let request = state.first_request();
            state
                .apply_page(loaded(request, rooms.clone(), None, None))
                .unwrap();
            state
        };

        let other = listed(Some("other.example"));
        assert_eq!(
            other.join_target(0),
            ("#ops:other.example".to_owned(), Vec::new())
        );
        assert_eq!(
            other.join_target(1),
            ("!opaque".to_owned(), vec!["other.example".to_owned()])
        );
        assert_eq!(
            listed(None).join_target(1),
            ("!opaque".to_owned(), Vec::new())
        );
    }

    #[test]
    fn cells_show_alias_users_name_and_the_topic_first_line() {
        let mut state = RoomListState::new(query(None, None), "example");
        let request = state.first_request();
        state
            .apply_page(loaded(
                request,
                vec![
                    public_room(
                        "!a:example",
                        Some("#rust:example"),
                        Some("Rust"),
                        Some("Systems programming\nRules: be kind"),
                        4200,
                    ),
                    public_room("!b:example", None, None, None, 0),
                ],
                None,
                None,
            ))
            .unwrap();

        let cells = |index| RoomListColumn::ALL.map(|column| state.cell_text(index, column));
        assert_eq!(
            cells(0),
            ["#rust:example", "4200", "Rust", "Systems programming"]
        );
        assert_eq!(cells(1), ["!b:example", "0", "", ""]);
    }

    #[test]
    fn caption_names_the_listed_server_and_the_match_text() {
        assert_eq!(
            RoomListState::new(query(None, None), "example").caption(),
            "Rooms List [example] - listing..."
        );
        assert_eq!(
            RoomListState::new(query(Some("matrix.org"), Some("rust lang")), "example").caption(),
            "Rooms List [matrix.org] \"rust lang\" - listing..."
        );
    }
}
