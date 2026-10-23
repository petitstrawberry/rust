//! Exercise a real pending write while a tracked query is paused mid-execution.
//! Channel handshakes force the overlap; sleeps are not used for ordering.

use salsa::{Cancelled, Database, EventKind, Setter, Storage};
use std::panic::AssertUnwindSafe;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender, TryRecvError},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(10);

struct Pause {
    once: AtomicBool,
    entered: Sender<()>,
    resume: Mutex<Receiver<()>>,
}

#[salsa::db]
trait TestDb: Database {
    fn pause_first_query(&self);
}

#[salsa::db]
#[derive(Clone)]
struct TestDatabase {
    storage: Storage<Self>,
    pause: Arc<Pause>,
}

#[salsa::db]
impl Database for TestDatabase {}

#[salsa::db]
impl TestDb for TestDatabase {
    fn pause_first_query(&self) {
        if self.pause.once.swap(false, Ordering::AcqRel) {
            self.pause.entered.send(()).unwrap();
            self.pause.resume.lock().unwrap().recv_timeout(WAIT).unwrap();
        }
    }
}

#[salsa::input]
struct Input {
    value: u32,
}

#[salsa::tracked]
fn read_value(db: &dyn TestDb, input: Input) -> u32 {
    input.value(db)
}

#[salsa::tracked]
fn answer(db: &dyn TestDb, input: Input) -> u32 {
    let before = input.value(db);
    db.pause_first_query();
    // The explicit checkpoint and nested tracked fetch must both defer the
    // pending-write unwind, while still reading the same immutable revision.
    db.unwind_if_revision_cancelled();
    let after = read_value(db, input);
    assert_eq!(before, after, "writer changed an active snapshot's inputs");
    before + after
}

#[test]
fn pending_write_preserves_snapshot_and_updates_next_revision() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let (cancel_tx, cancel_rx) = mpsc::channel();
    let (answer_tx, answer_rx) = mpsc::channel();
    let (drop_tx, drop_rx) = mpsc::channel();
    let (written_tx, written_rx) = mpsc::channel();
    let mut db = TestDatabase {
        storage: Storage::new(Some(Box::new(move |event| {
            if matches!(event.kind, EventKind::DidSetCancellationFlag) {
                cancel_tx.send(()).unwrap();
            }
        }))),
        pause: Arc::new(Pause {
            once: AtomicBool::new(true),
            entered: entered_tx,
            resume: Mutex::new(resume_rx),
        }),
    };
    let input = Input::new(&db, 21);
    let snapshot = db.clone();
    let reader = thread::spawn(move || {
        // Query guards restore Salsa's local state on unwind; the pause mutex
        // is released before either cancellation checkpoint is reached.
        let result = Cancelled::catch(AssertUnwindSafe(|| answer(&snapshot, input)));
        answer_tx.send(result).unwrap();
        // Retain the snapshot after the query completes, proving the barrier
        // waits for handle release, not merely for query completion.
        drop_rx.recv_timeout(WAIT).unwrap();
        drop(snapshot);
    });
    entered_rx.recv_timeout(WAIT).expect("query did not start");

    let writer = thread::spawn(move || {
        input.set_value(&mut db).to(50);
        // Requery the same tracked function/input after mutation to verify memo
        // invalidation. The default host path also checks cancellation reset.
        let updated = answer(&db, input);
        written_tx.send(updated).unwrap();
    });
    cancel_rx.recv_timeout(WAIT).expect("writer did not set cancellation");
    assert_eq!(written_rx.try_recv(), Err(TryRecvError::Empty));
    resume_tx.send(()).unwrap();

    let result = answer_rx.recv_timeout(WAIT).expect("active query did not finish");
    if cfg!(any(target_os = "scarlet", feature = "scarlet-defer-cancellation-test")) {
        assert_eq!(result.expect("pending write unwound on the abort-platform path"), 42);
    } else {
        // Running this same focused test without the feature checks that the
        // upstream host cancellation policy remains unchanged.
        assert!(matches!(result, Err(Cancelled::PendingWrite { .. })));
    }
    assert_eq!(written_rx.try_recv(), Err(TryRecvError::Empty));
    drop_tx.send(()).unwrap();
    assert_eq!(written_rx.recv_timeout(WAIT).expect("writer did not resume"), 100);
    reader.join().unwrap();
    writer.join().unwrap();
}
