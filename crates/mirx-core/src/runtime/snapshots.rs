//! The order of the session's room snapshots (`SyncedRooms`). Several are
//! read at once (a join's, the room-list task's, the account data
//! handlers'), and reading one may wait for the server. Each is numbered
//! when it starts reading; one is emitted only if no snapshot that started
//! later was emitted before it, because that one already shows every
//! change made before it started.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Shared by everything in a session that emits a room snapshot. `Send`,
/// for the SDK's event handlers.
#[derive(Debug, Clone, Default)]
pub(crate) struct RoomSnapshots(Arc<Order>);

#[derive(Debug, Default)]
struct Order {
    started: AtomicU64,
    emitted: AtomicU64,
}

/// A snapshot being read: its place in the start order.
#[derive(Debug, Clone, Copy)]
#[must_use = "a started snapshot is emitted only if it supersedes the emitted ones"]
pub(crate) struct SnapshotStart(u64);

impl RoomSnapshots {
    /// Before reading a snapshot.
    pub(crate) fn start(&self) -> SnapshotStart {
        SnapshotStart(self.0.started.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// Once read: whether to emit it, which is so if it started after every
    /// snapshot emitted so far. It is then the newest emitted.
    pub(crate) fn supersede(&self, snapshot: SnapshotStart) -> bool {
        self.0.emitted.fetch_max(snapshot.0, Ordering::Relaxed) < snapshot.0
    }
}

#[cfg(test)]
mod tests {
    use super::RoomSnapshots;

    #[test]
    fn a_later_snapshot_supersedes_an_earlier_one() {
        let snapshots = RoomSnapshots::default();
        let a = snapshots.start();
        let b = snapshots.start();
        assert!(snapshots.supersede(b));
        // A started before B, so B already shows what A read.
        assert!(!snapshots.supersede(a));
        let c = snapshots.start();
        assert!(snapshots.supersede(c));
    }
}
