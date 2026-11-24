//! The UI's own states that the automation protocol shows as they are:
//! the account's lifecycle and connection, a window's activity, the Window
//! menu's arrangements, the compose formatting chords and a message's
//! reaction groups. Every build has them; the serde derives are the
//! protocol's, and emit no code in a build without it.

use serde::{Deserialize, Serialize};

/// The Window menu's arrangements of the MDI windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MdiArrangement {
    Cascade,
    TileHorizontal,
    TileVertical,
    ArrangeIcons,
}

/// What happened in a view the user is not looking at, in mIRC's
/// switchbar/treebar colour precedence: a higher level replaces a lower one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ViewActivityLevel {
    /// Joins, parts, topic and power-level changes.
    Event,
    /// Someone talked.
    Message,
    /// A line the user's push rules highlight, e.g. one mentioning them.
    Highlight,
}

/// The formatting chords: Ctrl+B (`**`), Ctrl+I (`*`) and Ctrl+E (`~~`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FormatKey {
    Bold,
    Italic,
    Strikethrough,
}

/// Where the core and the session are, from the core's events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LifecycleState {
    #[default]
    Offline,
    CoreStarted,
    LoggingIn,
    Restoring,
    Authenticated,
    Syncing,
    Ready,
    LoggedOut,
    Stopped,
    Error,
}

/// Where the account is on mIRC's connect/disconnect axis: File > Connect
/// and Disconnect move it, and the File menu greys what does not apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConnectionState {
    #[default]
    SignedOut,
    /// A login or session restore is in flight.
    SigningIn,
    /// A browser sign-in waits for the user to finish on the homeserver's
    /// page.
    WaitingForBrowser,
    /// Signed in, not syncing: offline, with the session kept.
    SignedIn,
    /// Signed in, not syncing, and a connect retry is due: mIRC's
    /// `Retry...` delay between attempts.
    WaitingToReconnect,
    Syncing,
    /// A Log Out is under way: the core waits for the key backup and the
    /// server, with sync stopped.
    LoggingOut,
}

/// One reaction key under a message: who reacted with it, in reaction
/// time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReactionSummary {
    pub(crate) key: String,
    pub(crate) count: u64,
    pub(crate) senders: Vec<String>,
    pub(crate) reaction_event_ids: Vec<String>,
}
