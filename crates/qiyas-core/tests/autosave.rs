//! Autosave and recovery: a drawing with unsaved changes is written to `<file>.sv$` (a whole
//! DWG) with its project beside it as `<file>.sv$.qiyas`, atomically; an autosave newer than its
//! drawing is offered back; a save deletes it. Recover opens a damaged file through the failsafe
//! reader and says what it dropped.

use std::time::{Duration, SystemTime};

use opencadcodec::{CadDocument, EntityType, Line};
use qiyas_core::fixtures::{sample_doc, write_drawing, write_sample};
use qiyas_core::{
    autosave_path, discard_autosave, recoverable, Command, DrawSpec, Kind, Outcome, Session, Target,
};

fn line(s: &mut Session) -> u64 {
    match s
        .run(Command::Draw(DrawSpec::Line {
            a: [100.0, 100.0],
            b: [101.0, 100.0],
        }))
        .unwrap()
    {
        Outcome::Added { handles } => handles[0],
        other => panic!("{other:?}"),
    }
}

/// Makes the file at `path` look an hour old.
fn age(path: &std::path::Path) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
}

fn files(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn autosave_writes_the_drawing_and_its_project_and_recovery_finds_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    age(&path);
    let mut s = Session::open(&path).unwrap();
    // Nothing to keep yet.
    assert_eq!(s.autosave().unwrap(), None);
    assert!(recoverable(&path).is_none());

    let added = line(&mut s);
    s.run(Command::MeasureDistance {
        points: vec![[0.0, 0.0], [3.0, 4.0]].into(),
    })
    .unwrap();
    let written = s.autosave().unwrap().expect("an autosave");
    assert_eq!(written, autosave_path(&path));
    assert_eq!(written, dir.path().join("sample.dwg.sv$"));
    // Atomic: the temp files are gone, the drawing and project files sit side by side.
    assert_eq!(
        files(dir.path()),
        ["sample.dwg", "sample.dwg.sv$", "sample.dwg.sv$.qiyas"]
    );
    // The drawing itself is untouched, and still has unsaved changes.
    assert!(s.is_dirty());

    // Again, over the last one.
    line(&mut s);
    s.autosave().unwrap();

    let found = recoverable(&path).expect("newer than the drawing");
    assert_eq!(found.autosave, Some(written.display().to_string()));
    assert_eq!(found.drawing, path.display().to_string());

    // Recover: the autosave as a new drawing beside the original, with its take-off.
    let r = Session::open_autosave(&path).unwrap();
    assert!(r
        .drawing
        .doc
        .get_entity(opencadcodec::Handle::new(added))
        .is_some());
    assert_eq!(r.takeoff.items.len(), 1);
    assert_eq!(r.takeoff.items[0].kind, Kind::Length);
    assert_eq!(r.drawing.path, dir.path().join("sample (recovered).dwg"));
    assert!(r.is_dirty());
    assert!(r.info().problems.is_empty(), "{:?}", r.info().problems);

    // Discard: both files go; nothing left to offer.
    discard_autosave(&path).unwrap();
    assert!(recoverable(&path).is_none());
    assert!(!written.exists());
    assert!(!dir.path().join("sample.dwg.sv$.qiyas").exists());
}

#[test]
fn an_autosave_older_than_its_drawing_is_reported_as_older_and_a_save_deletes_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    let mut s = Session::open(&path).unwrap();
    line(&mut s);
    let written = s.autosave().unwrap().unwrap();
    // The drawing saved after the autosave: reported as older, no crash (the window decides).
    age(&written);
    let older = recoverable(&path).expect("an autosave is there");
    assert!(older.saved_at.unwrap() < older.drawing_at.unwrap());
    assert!(!older.crashed);

    // A save deletes the autosave.
    s.save().unwrap();
    assert!(!written.exists());
    assert!(!dir.path().join("sample.dwg.sv$.qiyas").exists());
    assert_eq!(s.autosave().unwrap(), None);
}

#[test]
fn no_autosave_while_a_block_is_edited_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_drawing(&sample_doc(), &dir.path().join("plan.dwg"));
    let mut s = Session::open(&path).unwrap();
    line(&mut s);
    let door = s
        .drawing
        .doc
        .entities_in_block("*Model_Space")
        .find(|e| matches!(e, EntityType::Insert(_)))
        .unwrap()
        .common()
        .handle
        .value();
    s.run(Command::EditBlockInPlace {
        target: Target {
            handle: door,
            chain: Vec::new(),
        },
    })
    .unwrap();
    assert_eq!(s.autosave().unwrap(), None);
    assert!(!autosave_path(&path).exists());
}

/// The sample with 400 more lines, as a DWG at `dir/big.dwg`, and its bytes.
fn big(dir: &std::path::Path) -> (std::path::PathBuf, Vec<u8>) {
    let mut doc: CadDocument = sample_doc();
    for i in 0..400 {
        let x = f64::from(i);
        let line = Line::from_coords(x, 0.0, 0.0, x, 5.0, 0.0);
        doc.add_entity(EntityType::Line(line)).unwrap();
    }
    let path = write_drawing(&doc, &dir.join("big.dwg"));
    let bytes = std::fs::read(&path).unwrap();
    (path, bytes)
}

#[test]
fn recover_reports_what_a_damaged_drawing_lost() {
    let dir = tempfile::tempdir().unwrap();
    let (path, bytes) = big(dir.path());
    let whole = Session::open(&path).unwrap().read_report();
    assert!(whole.dropped.is_empty());
    assert_eq!(whole.skipped, 0);

    // Damage 400 bytes near the end, where the objects are.
    let mut damaged = bytes.clone();
    let at = damaged.len() * 9 / 10;
    damaged[at..at + 400].fill(0xAA);
    let damaged_path = dir.path().join("damaged.dwg");
    std::fs::write(&damaged_path, &damaged).unwrap();
    let s = Session::open(&damaged_path).unwrap();
    let report = s.read_report();
    assert!(report.skipped > 0);
    assert!(report.entities < whole.entities, "{report:?}");
    let counted: usize = report.dropped.iter().map(|d| d.count).sum();
    assert!(counted > 0);
    assert!(report.dropped.iter().all(|d| !d.kind.is_empty()));

    // A truncated DWG loses its file header: nothing can be read from it.
    let cut = dir.path().join("cut.dwg");
    std::fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
    assert!(Session::open(&cut).is_err());
}

#[test]
fn a_dwg_and_a_dxf_of_one_name_keep_their_own_autosaves() {
    let dir = tempfile::tempdir().unwrap();
    let dwg = write_sample(dir.path(), "dwg");
    let dxf = write_sample(dir.path(), "dxf");
    let mut a = Session::open(&dwg).unwrap();
    let mut b = Session::open(&dxf).unwrap();
    line(&mut a);
    line(&mut b);
    let (wa, wb) = (
        a.autosave().unwrap().unwrap(),
        b.autosave().unwrap().unwrap(),
    );
    assert_ne!(wa, wb);
    assert_eq!(wb, dir.path().join("sample.dxf.sv$"));
}

#[test]
fn the_recovered_drawings_first_save_deletes_the_autosave_it_came_from() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    age(&path);
    let mut s = Session::open(&path).unwrap();
    line(&mut s);
    let written = s.autosave().unwrap().unwrap();
    let mut r = Session::open_autosave(&path).unwrap();
    // Not yet saved: still offered.
    assert!(recoverable(&path).is_some());
    let target = dir.path().join("kept.dwg");
    r.save_as(&target).unwrap();
    assert!(!written.exists());
    assert!(recoverable(&path).is_none());
    assert!(!dir.path().join("sample.dwg.sv$.qiyas").exists());
}
