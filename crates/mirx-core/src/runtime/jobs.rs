//! The core's jobs: network work that runs as tasks beside the actor, in
//! lanes. A lane runs its work in arrival order, at most `width` at a time;
//! lanes do not wait for each other.
//!
//! Work is started from the actor's state `S` (the session) when it
//! reaches the head of its lane, so whatever it checks in that state sees
//! the work queued before it done. A job's output `F` goes back to the
//! actor, which applies it before the job's ack and only then releases the
//! lane.

use std::collections::{HashMap, VecDeque};

use futures_util::future::LocalBoxFuture;
use tokio::task::{self, JoinSet};

/// Where a job runs, by what it must stay in order with.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Lane {
    /// A room's sends, state writes and membership, in the order typed.
    Room(String),
    /// A room's read receipts: in order, but never behind an upload.
    Receipts(String),
    /// The account's read-modify-writes: display name, highlight words,
    /// ignore list.
    Account,
    /// The device list and its changes, so a listing shows earlier changes.
    Devices,
    /// The E2EE state: recovery, the identity reset and the status.
    E2ee,
    Verification,
    /// Media downloads, a few at a time.
    Media,
    /// Independent reads and new rooms, all at once.
    Unordered,
}

impl Lane {
    fn width(&self) -> usize {
        match self {
            Self::Media => 4,
            Self::Unordered => usize::MAX,
            Self::Room(_)
            | Self::Receipts(_)
            | Self::Account
            | Self::Devices
            | Self::E2ee
            | Self::Verification => 1,
        }
    }
}

/// Whether a piece of work answers a command, which is acked when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Owes {
    Ack,
    Nothing,
}

/// Builds a job from the actor's state when the work starts.
pub(super) type Start<S, F> = Box<dyn FnOnce(&mut S) -> LocalBoxFuture<'static, F>>;

/// Work waiting for its lane.
pub(super) struct Work<S, F> {
    owes: Owes,
    start: Start<S, F>,
}

impl<S, F> Work<S, F> {
    /// A command's work: acked when it ends, or when it is dropped.
    pub(super) fn command(start: Start<S, F>) -> Self {
        Self {
            owes: Owes::Ack,
            start,
        }
    }

    /// Work the core gave itself.
    pub(super) fn internal(start: Start<S, F>) -> Self {
        Self {
            owes: Owes::Nothing,
            start,
        }
    }
}

/// A job that ended. Its lane stays taken until `Jobs::release`.
#[must_use = "a finished job's lane is released after its output is applied"]
pub(super) struct Finished<F> {
    pub(super) lane: Lane,
    pub(super) owes: Owes,
    pub(super) output: F,
}

pub(super) struct Jobs<S, F> {
    running: JoinSet<F>,
    meta: HashMap<task::Id, (Lane, Owes)>,
    /// Only lanes with running or queued work.
    lanes: HashMap<Lane, LaneQueue<S, F>>,
}

struct LaneQueue<S, F> {
    /// Running jobs, and finished ones not released yet.
    taken: usize,
    queued: VecDeque<Work<S, F>>,
}

impl<S, F: 'static> Jobs<S, F> {
    pub(super) fn new() -> Self {
        Self {
            running: JoinSet::new(),
            meta: HashMap::new(),
            lanes: HashMap::new(),
        }
    }

    /// Queues `work` behind its lane's, and starts it at once if the lane
    /// has room.
    pub(super) fn enqueue(&mut self, lane: Lane, work: Work<S, F>, state: &mut S) {
        self.lanes
            .entry(lane.clone())
            .or_insert_with(|| LaneQueue {
                taken: 0,
                queued: VecDeque::new(),
            })
            .queued
            .push_back(work);
        self.start_fitting(&lane, state);
    }

    /// The next job to end, once one does; `None` at once when none runs.
    /// Cancel-safe.
    pub(super) async fn next(&mut self) -> Option<Finished<F>> {
        let (id, output) = match self.running.join_next_with_id().await? {
            Ok(done) => done,
            Err(err) => {
                assert!(
                    err.is_panic(),
                    "only abort_all cancels jobs, and it joins them itself"
                );
                std::panic::resume_unwind(err.into_panic())
            }
        };
        let (lane, owes) = self.meta.remove(&id).expect("every job has its meta");
        Some(Finished { lane, owes, output })
    }

    /// After a finished job's output is applied: frees its place in the lane
    /// and starts the work that now fits.
    pub(super) fn release(&mut self, lane: &Lane, state: &mut S) {
        self.lanes
            .get_mut(lane)
            .expect("a finished job's lane is taken")
            .taken -= 1;
        self.start_fitting(lane, state);
    }

    fn start_fitting(&mut self, lane: &Lane, state: &mut S) {
        let queue = self.lanes.get_mut(lane).expect("the lane has work");
        while queue.taken < lane.width()
            && let Some(work) = queue.queued.pop_front()
        {
            queue.taken += 1;
            let handle = self.running.spawn_local((work.start)(state));
            self.meta.insert(handle.id(), (lane.clone(), work.owes));
        }
        if queue.taken == 0 {
            self.lanes.remove(lane);
        }
    }

    /// Aborts every job and waits until each is gone, and drops the queued
    /// work: the session they belong to ends. Returns how many acks the
    /// commands among them are owed, finished or not.
    pub(super) async fn abort_all(&mut self) -> usize {
        self.running.abort_all();
        let mut owed = 0;
        while let Some(joined) = self.running.join_next_with_id().await {
            let id = match joined {
                Ok((id, _)) => id,
                Err(err) if err.is_cancelled() => err.id(),
                Err(err) => std::panic::resume_unwind(err.into_panic()),
            };
            let (_, owes) = self.meta.remove(&id).expect("every job has its meta");
            owed += usize::from(owes == Owes::Ack);
        }
        owed += self
            .lanes
            .drain()
            .flat_map(|(_, queue)| queue.queued)
            .filter(|work| work.owes == Owes::Ack)
            .count();
        owed
    }
}

impl<S, F> Drop for Jobs<S, F> {
    fn drop(&mut self) {
        // A job owns a `Client` clone, which must drop inside the runtime:
        // `abort_all` first.
        assert!(
            std::thread::panicking() || self.running.is_empty(),
            "jobs dropped while running"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use tokio::sync::oneshot;
    use tokio::task::LocalSet;

    use super::{Finished, Jobs, Lane, Owes, Work};

    /// The test's actor state: the names of the work started, in order.
    type Started = Vec<&'static str>;

    /// Work that records its start and ends when its gate opens.
    fn gated(name: &'static str) -> (Work<Started, &'static str>, oneshot::Sender<()>) {
        let (open, gate) = oneshot::channel::<()>();
        let work = Work::command(Box::new(move |started: &mut Started| {
            started.push(name);
            Box::pin(async move {
                let _ = gate.await;
                name
            })
        }));
        (work, open)
    }

    fn run_local(test: impl std::future::Future<Output = ()>) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        LocalSet::new().block_on(&runtime, test);
    }

    async fn next(jobs: &mut Jobs<Started, &'static str>) -> Finished<&'static str> {
        jobs.next().await.expect("a running job")
    }

    #[test]
    fn a_width_one_lane_runs_its_work_in_order() {
        run_local(async {
            let mut jobs = Jobs::new();
            let mut started = Started::new();
            let lane = Lane::Room("!a".to_owned());
            let (first, open_first) = gated("first");
            let (second, open_second) = gated("second");
            let (third, open_third) = gated("third");
            jobs.enqueue(lane.clone(), first, &mut started);
            jobs.enqueue(lane.clone(), second, &mut started);
            jobs.enqueue(lane.clone(), third, &mut started);
            assert_eq!(started, ["first"]);

            open_first.send(()).expect("first runs");
            let finished = next(&mut jobs).await;
            assert_eq!((finished.output, finished.owes), ("first", Owes::Ack));
            // Not before the release: the actor applies the output first.
            assert_eq!(started, ["first"]);
            jobs.release(&finished.lane, &mut started);
            assert_eq!(started, ["first", "second"]);

            open_third.send(()).expect("third is queued");
            open_second.send(()).expect("second runs");
            let finished = next(&mut jobs).await;
            assert_eq!(finished.output, "second");
            jobs.release(&finished.lane, &mut started);
            assert_eq!(started, ["first", "second", "third"]);
            let finished = next(&mut jobs).await;
            assert_eq!(finished.output, "third");
            jobs.release(&finished.lane, &mut started);
            assert!(jobs.next().await.is_none());
            assert!(jobs.lanes.is_empty());
        });
    }

    #[test]
    fn lanes_do_not_wait_for_each_other() {
        run_local(async {
            let mut jobs = Jobs::new();
            let mut started = Started::new();
            let (busy, _open_busy) = gated("!a");
            jobs.enqueue(Lane::Room("!a".to_owned()), busy, &mut started);
            let mut gates = Vec::new();
            for (lane, name) in [
                (Lane::Room("!b".to_owned()), "!b"),
                (Lane::Receipts("!a".to_owned()), "receipts !a"),
                (Lane::Account, "account"),
                (Lane::Unordered, "unordered"),
            ] {
                let (work, open) = gated(name);
                gates.push(open);
                jobs.enqueue(lane, work, &mut started);
            }
            assert_eq!(started, ["!a", "!b", "receipts !a", "account", "unordered"]);
            assert_eq!(jobs.abort_all().await, 5);
        });
    }

    #[test]
    fn the_media_lane_runs_four_at_a_time() {
        run_local(async {
            let mut jobs = Jobs::new();
            let mut started = Started::new();
            let mut gates = Vec::new();
            for name in ["1", "2", "3", "4", "5"] {
                let (work, open) = gated(name);
                gates.push(open);
                jobs.enqueue(Lane::Media, work, &mut started);
            }
            assert_eq!(started, ["1", "2", "3", "4"]);
            gates.remove(0).send(()).expect("1 runs");
            let finished = next(&mut jobs).await;
            jobs.release(&finished.lane, &mut started);
            assert_eq!(started, ["1", "2", "3", "4", "5"]);
            assert_eq!(jobs.abort_all().await, 4);
        });
    }

    #[test]
    fn unordered_has_no_limit() {
        run_local(async {
            let mut jobs = Jobs::new();
            let mut started = Started::new();
            let mut gates = Vec::new();
            for _ in 0..100 {
                let (work, open) = gated("job");
                gates.push(open);
                jobs.enqueue(Lane::Unordered, work, &mut started);
            }
            assert_eq!(started.len(), 100);
            assert_eq!(jobs.abort_all().await, 100);
        });
    }

    /// Records that its job's future was dropped.
    struct DropGuard(Rc<RefCell<bool>>);

    impl Drop for DropGuard {
        fn drop(&mut self) {
            *self.0.borrow_mut() = true;
        }
    }

    #[test]
    fn abort_all_awaits_its_jobs_and_counts_what_it_owes() {
        run_local(async {
            let mut jobs = Jobs::new();
            let mut started = Started::new();
            let (hung, _open_hung) = gated("hung");
            jobs.enqueue(Lane::Room("!a".to_owned()), hung, &mut started);
            let dropped = Rc::new(RefCell::new(false));
            let guard = DropGuard(Rc::clone(&dropped));
            jobs.enqueue(
                Lane::Account,
                Work::command(Box::new(move |_: &mut Started| {
                    Box::pin(async move {
                        let _guard = guard;
                        std::future::pending::<()>().await;
                        "guarded"
                    })
                })),
                &mut started,
            );
            for lane in [Lane::Room("!a".to_owned()), Lane::Account, Lane::Account] {
                let (queued, _open) = gated("queued");
                jobs.enqueue(lane, queued, &mut started);
            }
            jobs.enqueue(
                Lane::E2ee,
                Work::internal(Box::new(|_: &mut Started| Box::pin(std::future::pending()))),
                &mut started,
            );
            // Two running commands, three queued ones, and the internal job.
            assert_eq!(jobs.abort_all().await, 5);
            assert!(
                *dropped.borrow(),
                "the guard drops before abort_all returns"
            );
            assert!(jobs.next().await.is_none());
        });
    }

    #[test]
    #[should_panic(expected = "jobs dropped while running")]
    fn dropping_running_jobs_panics() {
        run_local(async {
            let mut jobs = Jobs::new();
            let (hung, _open) = gated("hung");
            jobs.enqueue(Lane::Unordered, hung, &mut Started::new());
            drop(jobs);
        });
    }
}
