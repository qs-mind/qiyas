//! Crash recovery: a lock file beside each open drawing (`<file>.qlock`: pid, host and the
//! process's start time as JSON), a dead one meaning Qiyas closed unexpectedly; the crash's
//! autosave kept aside as a crash copy when the drawing is opened again; the autosave after every
//! 20 document changes; every open drawing autosaved in one go.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as Process, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use opencadcodec::Handle;
use qiyas_core::fixtures::{sample_doc, write_drawing, write_sample};
use qiyas_core::{
    autosave_all, autosave_copy_of, autosave_path, discard_autosave, discard_recoverable,
    lock_path, read_lock, recoverable, release_lock, take_lock, Command, CopyKind, DrawSpec,
    DrawingLock, LockStatus, Outcome, Session, AUTOSAVE_EVERY,
};

/// The drawing the child process opens (see `recovery_child`).
const CHILD: &str = "QIYAS_RECOVERY_CHILD";

fn line(s: &mut Session, y: f64) -> u64 {
    match s
        .run(Command::Draw(DrawSpec::Line {
            a: [100.0, y],
            b: [101.0, y],
        }))
        .unwrap()
    {
        Outcome::Added { handles } => handles[0],
        other => panic!("{other:?}"),
    }
}

fn measure(s: &mut Session) {
    s.run(Command::MeasureDistance {
        points: vec![[0.0, 0.0], [3.0, 4.0]].into(),
    })
    .unwrap();
}

/// Makes the file at `path` look `secs` seconds old.
fn modified_ago(path: &Path, secs: u64) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(secs))
        .unwrap();
}

fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The crash copies beside `sample.dwg` (`sample.dwg.sv$.crash-…`, not their projects), oldest
/// first.
fn crash_copies(dir: &Path) -> Vec<String> {
    files(dir)
        .into_iter()
        .filter(|n| n.starts_with("sample.dwg.sv$.crash-") && !n.ends_with(".qiyas"))
        .collect()
}

/// The test's name as the test binary knows it: its module path without the binary's name
/// (all the tests run in one binary, see tests/all.rs).
fn test_path(name: &str) -> String {
    match module_path!().split_once("::") {
        Some((_, module)) => format!("{module}::{name}"),
        None => name.to_owned(),
    }
}

/// This test binary run again as a child process; `drawing` set makes it `recovery_child`'s.
fn child(drawing: Option<&Path>) -> Process {
    let mut p = Process::new(std::env::current_exe().unwrap());
    p.args([
        test_path("recovery_child").as_str(),
        "--exact",
        "--nocapture",
        "--test-threads=1",
    ]);
    p.env_remove(CHILD);
    if let Some(drawing) = drawing {
        p.env(CHILD, drawing);
    }
    p
}

/// The pid of a process that has ended (and been reaped).
fn dead_pid() -> u32 {
    let mut c = child(None).stdout(Stdio::null()).spawn().unwrap();
    let pid = c.id();
    c.wait().unwrap();
    pid
}

/// This computer's name as a lock records it.
fn this_host() -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dxf");
    assert!(take_lock(&path).unwrap());
    let host = read_lock(&path).unwrap().host;
    release_lock(&path).unwrap();
    host
}

/// A lock left beside `drawing` by a process on this computer that has ended.
fn write_dead_lock(drawing: &Path) {
    let dead = DrawingLock {
        pid: dead_pid(),
        host: this_host(),
        started_at: 1_760_000_000,
    };
    std::fs::write(lock_path(drawing), serde_json::to_vec(&dead).unwrap()).unwrap();
}

/// Not a test by itself: the child process of
/// `a_killed_qiyas_leaves_a_dead_lock_and_its_autosave_is_offered`. Does nothing unless
/// `QIYAS_RECOVERY_CHILD` names a drawing: then opens it, draws a line, measures, autosaves,
/// locks it as the app does, says `READY:<the line's handle>` and waits to be killed.
#[test]
fn recovery_child() {
    let Some(path) = std::env::var_os(CHILD) else {
        return;
    };
    let path = PathBuf::from(path);
    let mut s = Session::open(&path).unwrap();
    let added = line(&mut s, 100.0);
    measure(&mut s);
    s.autosave().unwrap().expect("an autosave");
    // As the app does; under another's lock (a foreign lock written first) it is not taken.
    take_lock(&path).unwrap();
    let mut out = std::io::stdout();
    writeln!(out, "READY:{added}").unwrap();
    out.flush().unwrap();
    // Killed long before; should the parent die first, it ends by itself within 120 s.
    std::thread::sleep(Duration::from_secs(120));
}

/// Kills the child process when the test ends, passed or failed.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_killed_qiyas_leaves_a_dead_lock_and_its_autosave_is_offered() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    modified_ago(&path, 3600);
    let mut running = Running(child(Some(&path)).stdout(Stdio::piped()).spawn().unwrap());
    let pid = running.0.id();
    let out = BufReader::new(running.0.stdout.take().unwrap());
    let added: u64 = out
        .lines()
        .map_while(Result::ok)
        .find_map(|l| {
            l.split_once("READY:")
                .map(|(_, h)| h.trim().parse().unwrap())
        })
        .expect("the child opened, changed, autosaved and locked the drawing");

    // While it runs, the drawing is open in another Qiyas: reported, not a crash.
    let live = recoverable(&path).expect("its autosave is newer than the drawing");
    assert!(!live.crashed);
    assert_eq!(live.locked_elsewhere.as_ref().map(|l| l.pid), Some(pid));
    assert_eq!(read_lock(&path).unwrap().status(), LockStatus::Alive);
    assert!(live.older_copies.is_empty());
    // Opening it here too leaves the other's lock alone (no read-only open: only a warning).
    assert!(!take_lock(&path).unwrap());
    assert_eq!(read_lock(&path).unwrap().pid, pid);
    // Open anyway: this process autosaves beside the other's autosave, never over it.
    let theirs = std::fs::read(autosave_path(&path)).unwrap();
    let mut here = Session::open(&path).unwrap();
    line(&mut here, 500.0);
    let beside = here.autosave().unwrap().expect("an autosave");
    let ours = format!("sample.dwg.sv$.{}", std::process::id());
    assert_eq!(beside, dir.path().join(&ours));
    assert!(dir.path().join(format!("{ours}.qiyas")).exists());
    assert_eq!(std::fs::read(autosave_path(&path)).unwrap(), theirs);
    // Its autosave is the other Qiyas's safety net: Discard is refused and keeps it; Don't Save
    // here still deletes this process's own.
    assert!(discard_autosave(&path).is_err());
    assert!(!beside.exists());
    assert!(discard_recoverable(&path).is_err());
    assert_eq!(std::fs::read(autosave_path(&path)).unwrap(), theirs);
    assert_eq!(read_lock(&path).unwrap().pid, pid);

    // SIGKILL: no clean exit, so the lock stays behind with a dead pid.
    running.0.kill().unwrap();
    running.0.wait().unwrap();
    assert!(lock_path(&path).exists());
    assert_eq!(read_lock(&path).unwrap().status(), LockStatus::Dead);

    // Offered even when the drawing is newer than the autosave: Qiyas closed unexpectedly.
    modified_ago(&autosave_path(&path), 3600);
    modified_ago(&path, 0);
    let crashed = recoverable(&path).expect("a crash offers its autosave");
    assert!(crashed.crashed);
    assert_eq!(crashed.locked_elsewhere, None);
    assert!(crashed.drawing_at.expect("the drawing is there") > crashed.saved_at.unwrap());
    assert_eq!(
        crashed.autosave,
        Some(autosave_path(&path).display().to_string())
    );

    // Recover brings back the child's line and its take-off row.
    let r = Session::open_autosave(&path).unwrap();
    assert!(r.drawing.doc.get_entity(Handle::new(added)).is_some());
    assert_eq!(r.takeoff.items.len(), 1);
    assert!(r.is_dirty());

    // Opening the drawing again (Later) cleans the stale lock up and keeps the crash's autosave
    // aside as a crash copy, offered once already: listed as an older copy, no new crash; this
    // process holds the drawing now.
    assert!(take_lock(&path).unwrap());
    let mine = read_lock(&path).unwrap();
    assert_eq!(mine.pid, std::process::id());
    assert_eq!(mine.status(), LockStatus::Mine);
    assert!(!autosave_path(&path).exists());
    let copies = crash_copies(dir.path());
    assert_eq!(copies.len(), 1, "{:?}", files(dir.path()));
    let copy = dir.path().join(&copies[0]);
    assert!(dir.path().join(format!("{}.qiyas", copies[0])).exists());
    let kept = recoverable(&path).expect("the crash copy is listed");
    assert!(!kept.crashed);
    assert_eq!(kept.autosave, None);
    assert_eq!(kept.older_copies.len(), 1);
    assert_eq!(kept.older_copies[0].path, copy.display().to_string());
    assert_eq!(autosave_copy_of(&copy), Some(path.clone()));

    // Recover from the crash copy by its path; its first save deletes the copy and nothing else.
    let mut r = Session::open_autosave(&copy).unwrap();
    assert!(r.drawing.doc.get_entity(Handle::new(added)).is_some());
    assert_eq!(r.takeoff.items.len(), 1);
    r.save_as(&dir.path().join("kept.dwg")).unwrap();
    assert!(
        crash_copies(dir.path()).is_empty(),
        "{:?}",
        files(dir.path())
    );
    assert!(recoverable(&path).is_none());
    release_lock(&path).unwrap();
    assert!(!lock_path(&path).exists());
}

#[test]
fn a_lock_file_holds_pid_host_and_start_time_and_is_released() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    assert_eq!(read_lock(&path), None);
    assert!(take_lock(&path).unwrap());
    assert_eq!(lock_path(&path), dir.path().join("sample.dwg.qlock"));

    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(lock_path(&path)).unwrap()).unwrap();
    assert_eq!(json["pid"], std::process::id());
    assert!(json["host"].is_string());
    // When this process started.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let started = json["started_at"].as_u64().unwrap();
    assert!(started <= now + 2 && now - started < 600, "{started} {now}");
    let lock = read_lock(&path).unwrap();
    assert_eq!(lock.pid, std::process::id());
    assert_eq!(lock.status(), LockStatus::Mine);
    // Written whole through a temp file: nothing else beside the drawing.
    assert_eq!(files(dir.path()), ["sample.dwg", "sample.dwg.qlock"]);

    // Taking it again keeps it; a lock of our own is no crash and offers nothing.
    assert!(take_lock(&path).unwrap());
    assert!(recoverable(&path).is_none());

    release_lock(&path).unwrap();
    assert_eq!(read_lock(&path), None);
    // Gone already: no error.
    release_lock(&path).unwrap();
    assert_eq!(files(dir.path()), ["sample.dwg"]);
}

#[test]
fn a_lock_whose_pid_now_belongs_to_another_process_is_dead() {
    let host = this_host();
    // Pid 1 runs, but it started long before this lock's process.
    let reused = DrawingLock {
        pid: 1,
        host: host.clone(),
        started_at: 1_000,
    };
    assert_eq!(reused.status(), LockStatus::Dead);
    // This process's own pid with another start time: an earlier Qiyas that had the same pid.
    let earlier = DrawingLock {
        pid: std::process::id(),
        host,
        started_at: 1_000,
    };
    assert_eq!(earlier.status(), LockStatus::Dead);

    // Such a lock means a crash: its autosave is offered, and opening the drawing takes over.
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    let mut s = Session::open(&path).unwrap();
    line(&mut s, 100.0);
    s.autosave().unwrap().unwrap();
    std::fs::write(lock_path(&path), serde_json::to_vec(&reused).unwrap()).unwrap();
    let found = recoverable(&path).unwrap();
    assert!(found.crashed);
    assert_eq!(found.locked_elsewhere, None);
    assert!(take_lock(&path).unwrap());
    assert_eq!(read_lock(&path).unwrap().status(), LockStatus::Mine);
}

#[test]
fn a_lock_from_another_computer_is_locked_elsewhere_never_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    modified_ago(&path, 3600);
    let mut s = Session::open(&path).unwrap();
    line(&mut s, 100.0);
    s.autosave().unwrap().unwrap();
    std::fs::write(
        lock_path(&path),
        r#"{"pid": 1, "host": "elsewhere", "started_at": 1760000000}"#,
    )
    .unwrap();
    let other = DrawingLock {
        pid: 1,
        host: "elsewhere".into(),
        started_at: 1_760_000_000,
    };
    assert_eq!(other.status(), LockStatus::OtherHost);

    let found = recoverable(&path).expect("the autosave is newer");
    assert!(!found.crashed);
    assert_eq!(found.locked_elsewhere, Some(other.clone()));
    // Not ours: never taken over, never removed, its autosave never discarded.
    assert!(!take_lock(&path).unwrap());
    release_lock(&path).unwrap();
    assert_eq!(read_lock(&path), Some(other.clone()));
    assert!(discard_autosave(&path).is_err());
    assert!(discard_recoverable(&path).is_err());
    assert!(autosave_path(&path).exists());

    // An older autosave: still reported, the window decides what to show.
    modified_ago(&autosave_path(&path), 7200);
    let older = recoverable(&path).expect("a lock is there");
    assert!(!older.crashed);
    assert!(older.saved_at.unwrap() < older.drawing_at.unwrap());
    assert_eq!(older.locked_elsewhere, Some(other));
}

#[test]
fn a_lock_elsewhere_is_reported_without_any_autosave() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    std::fs::write(
        lock_path(&path),
        r#"{"pid": 7, "host": "elsewhere", "started_at": 1760000000}"#,
    )
    .unwrap();
    let found = recoverable(&path).expect("someone has it open");
    assert_eq!(found.autosave, None);
    assert_eq!(found.saved_at, None);
    assert!(found.drawing_at.is_some());
    assert!(!found.crashed);
    assert_eq!(found.locked_elsewhere.map(|l| l.pid), Some(7));

    // Our own lock alone is no news.
    std::fs::remove_file(lock_path(&path)).unwrap();
    assert!(take_lock(&path).unwrap());
    assert!(recoverable(&path).is_none());
}

#[test]
fn a_dead_lock_offers_an_older_autosave_and_discarding_it_cleans_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    write_dead_lock(&path);
    assert_eq!(read_lock(&path).unwrap().status(), LockStatus::Dead);
    // A crash with nothing autosaved: reported, nothing to recover.
    let bare = recoverable(&path).expect("a crash");
    assert!(bare.crashed);
    assert_eq!(bare.autosave, None);

    // Written straight to `.sv$`, so the test controls what the crash left.
    std::fs::copy(&path, autosave_path(&path)).unwrap();
    modified_ago(&autosave_path(&path), 3600);
    let found = recoverable(&path).expect("crashed: offered though older");
    assert!(found.crashed);
    assert_eq!(found.locked_elsewhere, None);

    // Discard: the autosave goes, and the dead lock with it.
    discard_recoverable(&path).unwrap();
    assert_eq!(read_lock(&path), None);
    assert_eq!(files(dir.path()), ["sample.dwg"]);
    assert!(recoverable(&path).is_none());
}

#[test]
fn an_autosave_before_the_lock_is_taken_keeps_the_crash_autosave_aside() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    let mut crashed = Session::open(&path).unwrap();
    // Three lines: the session below draws two, so the third is only ever the crashed one's.
    line(&mut crashed, 100.0);
    line(&mut crashed, 110.0);
    let lost = line(&mut crashed, 120.0);
    crashed.autosave().unwrap().unwrap();
    write_dead_lock(&path);

    // Opened again (Later) and autosaved before the app's watch took the lock.
    let mut s = Session::open(&path).unwrap();
    line(&mut s, 200.0);
    s.autosave().unwrap().unwrap();
    assert_eq!(crash_copies(dir.path()).len(), 1);
    assert_eq!(read_lock(&path), None, "the dead lock is cleaned up");
    // The next autosave is this session's own: no second copy.
    line(&mut s, 300.0);
    s.autosave().unwrap().unwrap();
    assert_eq!(crash_copies(dir.path()).len(), 1);

    // The crash copy still holds what the crashed session drew.
    std::fs::remove_file(autosave_path(&path)).unwrap();
    let found = recoverable(&path).unwrap();
    assert_eq!(found.autosave, None);
    let copy = PathBuf::from(&found.older_copies[0].path);
    let r = Session::open_autosave(&copy).unwrap();
    assert!(r.drawing.doc.get_entity(Handle::new(lost)).is_some());
}

#[test]
fn at_most_three_crash_copies_are_kept_listed_newest_first_and_discarded_by_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    let mut s = Session::open(&path).unwrap();
    line(&mut s, 100.0);
    let written = s.autosave().unwrap().unwrap();
    let bytes = std::fs::read(&written).unwrap();
    std::fs::remove_file(&written).unwrap();
    // Four crashes, each opened again afterwards.
    let mut seen = Vec::new();
    for age in [4000, 3000, 2000, 1000] {
        std::fs::write(autosave_path(&path), &bytes).unwrap();
        modified_ago(&autosave_path(&path), age);
        write_dead_lock(&path);
        assert!(take_lock(&path).unwrap());
        release_lock(&path).unwrap();
        seen = crash_copies(dir.path());
    }
    assert_eq!(seen.len(), 3, "{:?}", files(dir.path()));
    // Named by when each was written (UTC), so the oldest went.
    let names: Vec<&str> = seen.iter().map(String::as_str).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);

    // Listed, newest first; never offered as a fresh crash.
    let found = recoverable(&path).unwrap();
    assert!(!found.crashed);
    assert_eq!(found.autosave, None);
    let listed: Vec<String> = found.older_copies.iter().map(|c| c.path.clone()).collect();
    let newest_first: Vec<String> = seen
        .iter()
        .rev()
        .map(|n| dir.path().join(n).display().to_string())
        .collect();
    assert_eq!(listed, newest_first);

    // Discard one by its path: it and its project go, the others stay listed.
    let newest = dir.path().join(&seen[2]);
    discard_autosave(&newest).unwrap();
    assert!(!newest.exists());
    assert!(!dir.path().join(format!("{}.qiyas", seen[2])).exists());
    assert_eq!(recoverable(&path).unwrap().older_copies.len(), 2);
    // Don't Save of a session of the drawing, or the dialog's Discard of the drawing, deletes
    // its autosave, never a crash copy.
    discard_autosave(&path).unwrap();
    discard_recoverable(&path).unwrap();
    assert_eq!(crash_copies(dir.path()).len(), 2);
}

#[test]
fn under_another_computers_lock_the_autosave_goes_beside_and_a_save_deletes_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    std::fs::write(
        lock_path(&path),
        r#"{"pid": 7, "host": "elsewhere", "started_at": 1760000000}"#,
    )
    .unwrap();
    let mut s = Session::open(&path).unwrap();
    line(&mut s, 100.0);
    let beside = s.autosave().unwrap().unwrap();
    assert_eq!(
        beside,
        dir.path()
            .join(format!("sample.dwg.sv$.{}", std::process::id()))
    );
    assert!(!autosave_path(&path).exists());
    assert_eq!(autosave_copy_of(&beside), Some(path.clone()));

    // This process runs: its own instance autosave is no news to anyone.
    let found = recoverable(&path).unwrap();
    assert_eq!(found.autosave, None);
    assert!(found.older_copies.is_empty());
    assert!(found.locked_elsewhere.is_some());
    std::fs::copy(&beside, autosave_path(&path)).unwrap();
    assert_eq!(
        recoverable(&path).unwrap().autosave,
        Some(autosave_path(&path).display().to_string())
    );

    // A save deletes this process's autosave and leaves the other Qiyas's.
    s.save().unwrap();
    assert!(!beside.exists());
    assert!(autosave_path(&path).exists());
}

#[test]
fn an_unreadable_lock_file_counts_as_no_lock() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    let mut s = Session::open(&path).unwrap();
    line(&mut s, 100.0);
    s.autosave().unwrap().unwrap();
    modified_ago(&autosave_path(&path), 3600);
    std::fs::write(lock_path(&path), "{ \"pid\": 12").unwrap();
    assert_eq!(read_lock(&path), None);
    // The older autosave is reported as it is: no crash, no lock.
    let found = recoverable(&path).unwrap();
    assert!(!found.crashed);
    assert_eq!(found.locked_elsewhere, None);
    // Taken over.
    assert!(take_lock(&path).unwrap());
    assert_eq!(read_lock(&path).unwrap().pid, std::process::id());
}

#[test]
fn autosave_is_due_after_twenty_document_changes_and_starts_again_on_save_and_autosave() {
    assert_eq!(AUTOSAVE_EVERY, 20);
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    let mut s = Session::open(&path).unwrap();
    for i in 0..19 {
        line(&mut s, f64::from(i));
    }
    assert_eq!(s.changes_since_autosave(), 19);
    assert!(!s.autosave_due());
    // A take-off row is no change to the drawing.
    measure(&mut s);
    assert!(!s.autosave_due());
    line(&mut s, 19.0);
    assert_eq!(s.changes_since_autosave(), 20);
    assert!(s.autosave_due());

    s.autosave().unwrap().expect("an autosave");
    assert_eq!(s.changes_since_autosave(), 0);
    assert!(!s.autosave_due());

    // Undo counts as a change.
    for i in 0..19 {
        line(&mut s, f64::from(i));
    }
    assert!(s.undo());
    assert!(s.autosave_due());
    s.save().unwrap();
    assert_eq!(s.changes_since_autosave(), 0);
    assert!(!s.autosave_due());
}

#[test]
fn autosave_all_writes_every_drawing_with_changes_and_says_which_failed() {
    let dir = tempfile::tempdir().unwrap();
    let clean = Session::open(&write_drawing(&sample_doc(), &dir.path().join("a.dwg"))).unwrap();
    let mut dirty =
        Session::open(&write_drawing(&sample_doc(), &dir.path().join("b.dwg"))).unwrap();
    line(&mut dirty, 100.0);
    let mut broken =
        Session::open(&write_drawing(&sample_doc(), &dir.path().join("c.dwg"))).unwrap();
    for i in 0..20 {
        line(&mut broken, f64::from(i));
    }
    broken.drawing.path = dir.path().join("gone").join("c.dwg");
    assert!(broken.autosave_due());

    let run = autosave_all([&clean, &dirty, &broken]);
    assert_eq!(
        run.written,
        [autosave_path(&dir.path().join("b.dwg"))
            .display()
            .to_string()]
    );
    assert_eq!(run.failed.len(), 1);
    assert!(
        run.failed[0].starts_with(&broken.drawing.path.display().to_string()),
        "{:?}",
        run.failed
    );
    // A failed attempt counts: the 1 s watch does not try it again every second (the timer does).
    assert!(!broken.autosave_due());
    assert!(!autosave_path(&dir.path().join("a.dwg")).exists());
}

#[test]
fn a_qiyas_that_crashed_under_another_lock_leaves_its_autosave_in_older_copies() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    // Another computer has the drawing open and has autosaved it.
    std::fs::write(
        lock_path(&path),
        r#"{"pid": 7, "host": "elsewhere", "started_at": 1760000000}"#,
    )
    .unwrap();
    std::fs::copy(&path, autosave_path(&path)).unwrap();
    let theirs = std::fs::read(autosave_path(&path)).unwrap();

    // A child Qiyas opens it anyway, changes it and autosaves beside theirs, then is killed.
    let mut running = Running(child(Some(&path)).stdout(Stdio::piped()).spawn().unwrap());
    let pid = running.0.id();
    let out = BufReader::new(running.0.stdout.take().unwrap());
    let added: u64 = out
        .lines()
        .map_while(Result::ok)
        .find_map(|l| {
            l.split_once("READY:")
                .map(|(_, h)| h.trim().parse().unwrap())
        })
        .expect("the child autosaved");
    let instance = dir.path().join(format!("sample.dwg.sv$.{pid}"));
    assert!(instance.exists(), "{:?}", files(dir.path()));
    assert_eq!(std::fs::read(autosave_path(&path)).unwrap(), theirs);
    // While it runs, its autosave is its own: not listed, not deleted.
    assert!(recoverable(&path).unwrap().older_copies.is_empty());
    assert!(discard_autosave(&instance).is_err());
    assert!(instance.exists());

    running.0.kill().unwrap();
    running.0.wait().unwrap();
    // Listed beside the other computer's `.sv$`, which stays the autosave offered.
    let found = recoverable(&path).unwrap();
    assert_eq!(
        found.autosave,
        Some(autosave_path(&path).display().to_string())
    );
    assert!(found.locked_elsewhere.is_some());
    assert_eq!(found.older_copies.len(), 1);
    assert_eq!(found.older_copies[0].path, instance.display().to_string());
    assert_eq!(found.older_copies[0].kind, CopyKind::Instance);

    // Recover brings its work back; Discard by its path deletes it, theirs stays.
    let r = Session::open_autosave(&instance).unwrap();
    assert!(r.drawing.doc.get_entity(Handle::new(added)).is_some());
    assert_eq!(r.takeoff.items.len(), 1);
    discard_autosave(&instance).unwrap();
    assert!(!instance.exists());
    assert!(!dir
        .path()
        .join(format!("sample.dwg.sv$.{pid}.qiyas"))
        .exists());
    assert_eq!(std::fs::read(autosave_path(&path)).unwrap(), theirs);
    assert!(recoverable(&path).unwrap().older_copies.is_empty());
}
