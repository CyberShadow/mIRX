//! Matrix links found in message text: matrix.to permalinks, `matrix:` URIs
//! and bare room aliases or room IDs. Pure, so the UI can hit-test links
//! without a ruma dependency of its own.

use matrix_sdk::ruma::{MatrixToUri, MatrixUri, RoomOrAliasId, matrix_uri::MatrixId};

/// What a Matrix link in a message points to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatrixLink {
    /// A room by ID or alias, with the servers to join it through.
    Room {
        room_id_or_alias: String,
        via: Vec<String>,
    },
    /// An event in a room. The UI opens the room; jumping to the event is a
    /// later step.
    Event {
        room_id_or_alias: String,
        event_id: String,
        via: Vec<String>,
    },
    User {
        user_id: String,
    },
}

/// `text` as a Matrix link: a `https://matrix.to/#/...` permalink, a
/// `matrix:` URI, or a bare room alias (`#room:server`) or room ID
/// (`!id:server`). `None` for anything else, web addresses included.
///
/// A bare room ID must name its server: in running text `!word` is not a
/// room, while a permalink may carry a server-less (room version 12) ID.
pub fn matrix_link(text: &str) -> Option<MatrixLink> {
    if let Ok(uri) = MatrixToUri::parse(text) {
        return link_from_id(uri.id(), uri.via());
    }
    if let Ok(uri) = MatrixUri::parse(text) {
        return link_from_id(uri.id(), uri.via());
    }
    let room = <&RoomOrAliasId>::try_from(text).ok()?;
    room.server_name()?;
    Some(MatrixLink::Room {
        room_id_or_alias: room.to_string(),
        via: Vec::new(),
    })
}

fn link_from_id(id: &MatrixId, via: &[matrix_sdk::ruma::OwnedServerName]) -> Option<MatrixLink> {
    let via = || via.iter().map(ToString::to_string).collect();
    match id {
        MatrixId::Room(room_id) => Some(MatrixLink::Room {
            room_id_or_alias: room_id.to_string(),
            via: via(),
        }),
        MatrixId::RoomAlias(alias) => Some(MatrixLink::Room {
            room_id_or_alias: alias.to_string(),
            via: via(),
        }),
        MatrixId::Event(room, event_id) => Some(MatrixLink::Event {
            room_id_or_alias: room.to_string(),
            event_id: event_id.to_string(),
            via: via(),
        }),
        MatrixId::User(user_id) => Some(MatrixLink::User {
            user_id: user_id.to_string(),
        }),
        // `MatrixId` is non-exhaustive: a kind of link a later ruma adds is
        // not one mirx knows how to follow, so it is not a link here.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{MatrixLink, matrix_link};

    fn room(target: &str, via: &[&str]) -> Option<MatrixLink> {
        Some(MatrixLink::Room {
            room_id_or_alias: target.to_owned(),
            via: via.iter().map(|server| (*server).to_owned()).collect(),
        })
    }

    #[test]
    fn matrix_to_permalinks_name_rooms_events_and_users() {
        assert_eq!(
            matrix_link("https://matrix.to/#/#rust:matrix.org"),
            room("#rust:matrix.org", &[])
        );
        assert_eq!(
            matrix_link("https://matrix.to/#/%23rust%3Amatrix.org"),
            room("#rust:matrix.org", &[])
        );
        assert_eq!(
            matrix_link("https://matrix.to/#/!abc:example.org?via=example.org&via=other.org"),
            room("!abc:example.org", &["example.org", "other.org"])
        );
        assert_eq!(
            matrix_link("https://matrix.to/#/!abc:example.org/$event1?via=example.org"),
            Some(MatrixLink::Event {
                room_id_or_alias: "!abc:example.org".to_owned(),
                event_id: "$event1".to_owned(),
                via: vec!["example.org".to_owned()],
            })
        );
        assert_eq!(
            matrix_link("https://matrix.to/#/@alice:example.org"),
            Some(MatrixLink::User {
                user_id: "@alice:example.org".to_owned()
            })
        );
    }

    #[test]
    fn matrix_uris_name_rooms_and_users() {
        assert_eq!(
            matrix_link("matrix:r/rust:matrix.org"),
            room("#rust:matrix.org", &[])
        );
        assert_eq!(
            matrix_link("matrix:roomid/abc:example.org?via=example.org"),
            room("!abc:example.org", &["example.org"])
        );
        assert_eq!(
            matrix_link("matrix:u/alice:example.org"),
            Some(MatrixLink::User {
                user_id: "@alice:example.org".to_owned()
            })
        );
    }

    #[test]
    fn bare_aliases_and_room_ids_with_a_server_are_rooms() {
        assert_eq!(matrix_link("#a:b.c"), room("#a:b.c", &[]));
        assert_eq!(matrix_link("!x:b.c"), room("!x:b.c", &[]));
    }

    #[test]
    fn anything_else_is_not_a_matrix_link() {
        for text in [
            "https://example.org",
            "https://matrix.to/#/",
            "https://matrix.to/#/garbage",
            "https://matrix.to/#/#nocolon",
            "matrix:x/whatever",
            "#nocolon",
            "!x",
            "!important",
            "@alice:example.org",
            "C#",
            "",
        ] {
            assert_eq!(matrix_link(text), None, "{text:?}");
        }
    }
}
