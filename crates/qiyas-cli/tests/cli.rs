//! `qiyas-cli` run as the user runs it: the built binary, its exit code, stdout and stderr. The
//! drawings are generated here with opencadcodec, as `qiyas-core/examples/write_test_plan.rs`
//! writes the test plan: three blocks, DOOR 3 times, WINDOW 5, COLUMN 6, in metres.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use calamine::{open_workbook, Data, Reader, Xlsx};
use opencadcodec::entities::Insert;
use opencadcodec::{
    BlockRecord, CadDocument, DwgWriter, DxfVersion, EntityType, Layer, Line, Vector3,
};
use qiyas_core::Session;

/// The plan: layers A-DOOR, A-WINDOW, S-COLUMN; blocks DOOR, WINDOW, COLUMN (one line each);
/// 3 doors, 5 windows and 6 columns in model space.
fn plan() -> CadDocument {
    let mut doc = CadDocument::new();
    doc.header.insertion_units = 6;
    for name in ["A-DOOR", "A-WINDOW", "S-COLUMN"] {
        let mut layer = Layer::new(name);
        layer.handle = doc.allocate_handle();
        doc.layers.add(layer).unwrap();
    }
    for (name, length) in [("DOOR", 0.9), ("WINDOW", 1.2), ("COLUMN", 0.4)] {
        let mut record = BlockRecord::new(name);
        record.handle = doc.allocate_handle();
        record.block_entity_handle = doc.allocate_handle();
        record.block_end_handle = doc.allocate_handle();
        let owner = record.handle;
        doc.block_records.add(record).unwrap();
        let mut line = EntityType::Line(Line::from_coords(0.0, 0.0, 0.0, length, 0.0, 0.0));
        line.common_mut().owner_handle = owner;
        doc.add_entity(line).unwrap();
    }
    for (name, layer, n) in [
        ("DOOR", "A-DOOR", 3),
        ("WINDOW", "A-WINDOW", 5),
        ("COLUMN", "S-COLUMN", 6),
    ] {
        for i in 0..n {
            let at = Vector3::new(f64::from(i) * 2.0, 0.0, 0.0);
            let mut insert = EntityType::Insert(Insert::new(name, at));
            insert.common_mut().layer = layer.to_string();
            doc.add_entity(insert).unwrap();
        }
    }
    doc
}

/// The plan as DWG bytes, in `version`.
fn plan_bytes(version: DxfVersion) -> Vec<u8> {
    let mut doc = plan();
    doc.version = version;
    DwgWriter::write_to_vec(&doc).unwrap()
}

fn write_plan(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, plan_bytes(DxfVersion::AC1032)).unwrap();
    path
}

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_qiyas-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The run succeeded with nothing on stderr; its stdout.
fn ok(out: Output) -> String {
    assert!(
        out.status.success(),
        "exit {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        stdout(&out),
        stderr(&out)
    );
    assert_eq!(stderr(&out), "", "nothing on stderr");
    stdout(&out)
}

/// The run failed (exit 1) with one line on stderr; that line.
fn failed(out: Output) -> String {
    one_line(out, 1)
}

/// The run finished with warnings (exit 2: a drawing opened with problems) and said so in one
/// line on stderr; that line.
fn warned(out: Output) -> String {
    one_line(out, 2)
}

fn one_line(out: Output, code: i32) -> String {
    assert!(
        out.status.code() == Some(code),
        "exit {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        stdout(&out),
        stderr(&out)
    );
    let err = stderr(&out);
    assert_eq!(err.lines().count(), 1, "one line on stderr: {err:?}");
    assert!(err.starts_with("qiyas-cli: "), "{err}");
    err.trim_end().to_string()
}

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

/// Block counts of a drawing, read back through the core.
fn counts(path: &Path) -> Vec<f64> {
    let mut session = Session::open(path).unwrap();
    ["DOOR", "WINDOW", "COLUMN"]
        .into_iter()
        .map(|name| {
            match session
                .run(qiyas_core::Command::CountBlock {
                    block_name: name.to_string(),
                    layer: None,
                })
                .unwrap()
            {
                qiyas_core::Outcome::Measured { measurement } => measurement.shown,
                other => panic!("{other:?}"),
            }
        })
        .collect()
}

/// The take-off rows of a workbook: description and quantity.
fn rows(xlsx: &Path) -> Vec<(String, f64)> {
    let mut book: Xlsx<_> = open_workbook(xlsx).unwrap();
    let sheet = book.worksheet_range("Take-off").unwrap();
    sheet
        .rows()
        .skip(1)
        .take_while(|r| !matches!(r.first(), None | Some(Data::Empty)))
        .map(|r| {
            let quantity = match &r[2] {
                Data::Float(v) => *v,
                Data::Int(v) => *v as f64,
                other => panic!("{other:?}"),
            };
            (r[0].to_string(), quantity)
        })
        .collect()
}

#[test]
fn convert_goes_dwg_to_dxf_and_back_in_a_chosen_version() {
    let dir = tempfile::tempdir().unwrap();
    let dwg = write_plan(dir.path(), "plan.dwg");
    let dxf = dir.path().join("plan.dxf");
    let text = ok(cli(&["convert", s(&dwg), s(&dxf)]));
    assert_eq!(text.lines().count(), 1, "{text}");
    assert!(text.contains("plan.dxf"), "{text}");
    assert_eq!(counts(&dxf), [3.0, 5.0, 6.0]);
    // A conversion is not a project: no project file comes beside the output.
    assert!(!dir.path().join("plan.qiyas").exists());
    assert!(!dir.path().join("plan.dwg.bak").exists());

    let old = dir.path().join("old.dwg");
    let out = cli(&["convert", s(&dxf), s(&old), "--version", "2004", "--json"]);
    let v = json(&ok(out));
    assert_eq!(v["version"], "AC1018");
    assert_eq!(v["output"], s(&old));
    let session = Session::open(&old).unwrap();
    assert_eq!(session.drawing.doc.version, DxfVersion::AC1018);
    assert_eq!(counts(&old), [3.0, 5.0, 6.0]);
    assert!(!dir.path().join("old.qiyas").exists());

    assert_eq!(ok(cli(&["--quiet", "convert", s(&old), s(&dxf)])), "");
    // Written over: the old file is kept as .bak, as the app does.
    assert!(dir.path().join("plan.dxf.bak").exists());
}

#[test]
fn convert_writes_the_drawing_only_and_never_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let dwg = write_plan(dir.path(), "plan.dwg");
    // The drawing's own project file, with a take-off the QS made.
    let mut session = Session::open(&dwg).unwrap();
    session
        .run(qiyas_core::Command::MeasureDistance {
            points: vec![[0.0, 0.0], [3.0, 4.0]].into(),
        })
        .unwrap();
    let project = session.save_project().unwrap();
    drop(session);
    let before = std::fs::read(&project).unwrap();

    // plan.dxf would share plan.qiyas: it is not rewritten for it.
    ok(cli(&["convert", s(&dwg), s(&dir.path().join("plan.dxf"))]));
    assert_eq!(std::fs::read(&project).unwrap(), before);
    // Another folder gets the drawing alone: no project (its take-off, BOQ and rates) beside it.
    let other = dir.path().join("for-consultant");
    std::fs::create_dir_all(&other).unwrap();
    ok(cli(&["convert", s(&dwg), s(&other.join("plan.dxf"))]));
    let names: Vec<String> = std::fs::read_dir(&other)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["plan.dxf"]);
    // Nor is the take-off embedded: the converted drawing has no QIYAS-* layers.
    let converted = Session::open(&other.join("plan.dxf")).unwrap();
    assert!(converted.takeoff.items.is_empty());
    assert!(converted
        .layers()
        .iter()
        .all(|l| !l.name.starts_with("QIYAS")));
}

#[test]
fn convert_errors_are_one_line() {
    let dir = tempfile::tempdir().unwrap();
    let dwg = write_plan(dir.path(), "plan.dwg");
    let missing = dir.path().join("none.dwg");
    let err = failed(cli(&["convert", s(&missing), s(&dir.path().join("x.dxf"))]));
    assert!(err.contains("none.dwg"), "{err}");
    let err = failed(cli(&["convert", s(&dwg), s(&dir.path().join("x.txt"))]));
    assert!(err.contains("x.txt"), "{err}");
    let err = failed(cli(&[
        "convert",
        s(&dwg),
        s(&dir.path().join("x.dwg")),
        "--version",
        "1999",
    ]));
    assert!(err.contains("2018"), "{err}");
    std::fs::write(dir.path().join("junk.dwg"), b"not a drawing").unwrap();
    let err = failed(cli(&[
        "convert",
        s(&dir.path().join("junk.dwg")),
        s(&dir.path().join("y.dxf")),
    ]));
    assert!(err.contains("can't be opened"), "{err}");
    assert!(!dir.path().join("y.dxf").exists());
}

#[test]
fn usage_errors_are_one_line_and_help_is_not_an_error() {
    failed(cli(&[]));
    failed(cli(&["frobnicate"]));
    failed(cli(&["convert", "only-one.dwg"]));
    let err = failed(cli(&[
        "batch", "folder", "--script", "x.scr", "--jobs", "0",
    ]));
    assert!(err.contains("--jobs") && !err.contains("--help"), "{err}");
    let help = ok(cli(&["--help"]));
    for word in ["convert", "run", "takeoff", "batch"] {
        assert!(help.contains(word), "{help}");
    }
    assert!(ok(cli(&["convert", "--help"])).contains("--version"));
}

#[test]
fn run_drives_a_script_on_a_drawing_and_writes_only_its_export() {
    let dir = tempfile::tempdir().unwrap();
    let dwg = write_plan(dir.path(), "plan.dwg");
    let script = dir.path().join("count.scr");
    std::fs::write(
        &script,
        // Saved by Notepad as "UTF-8 with BOM".
        "\u{feff}; count the three blocks\nCOUNT DOOR\nCOUNT WINDOW\nCOUNT COLUMN\nEXPORT {name}.xlsx\nQUIT\nCOUNT DOOR\n",
    )
    .unwrap();
    let text = ok(cli(&["run", s(&script), s(&dwg)]));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "line 2: DOOR count = 3 pcs");
    assert_eq!(lines[1], "line 3: WINDOW count = 5 pcs");
    assert_eq!(lines[2], "line 4: COLUMN count = 6 pcs");
    assert!(lines[3].starts_with("line 5: exported "), "{text}");
    assert_eq!(lines[4], "line 6: quit");
    assert_eq!(lines.len(), 5);
    let xlsx = dir.path().join("plan.xlsx");
    assert_eq!(
        rows(&xlsx),
        [
            ("DOOR count".to_string(), 3.0),
            ("WINDOW count".to_string(), 5.0),
            ("COLUMN count".to_string(), 6.0)
        ]
    );
    // No SAVE or SAVEAS: no project file, no .bak, the drawing untouched.
    assert!(!dir.path().join("plan.qiyas").exists());
    assert!(!dir.path().join("plan.dwg.bak").exists());

    let v = json(&ok(cli(&["run", s(&script), s(&dwg), "--json"])));
    assert_eq!(v["counts"]["DOOR"], 3.0);
    assert_eq!(v["counts"]["COLUMN"], 6.0);
    assert_eq!(v["steps"].as_array().unwrap().len(), 5);
    assert_eq!(v["problems"], 0);
    assert_eq!(ok(cli(&["run", s(&script), s(&dwg), "--quiet"])), "");
}

#[test]
fn run_reports_defaults_and_unknown_fields_for_json_and_typed_commands() {
    let dir = tempfile::tempdir().unwrap();
    let dwg = dir.path().join("plan.dwg");
    // The new-drawing template has a 1:100 annotation scale: 2.5 mm on paper is 0.25 m.
    Session::new_blank(&dwg, 6).save_as_headless(&dwg).unwrap();
    let script = dir.path().join("input-notes.scr");
    std::fs::write(
        &script,
        "{\"type\":\"text\",\"at\":[0,0],\"value\":\"Room\",\"typo\":true}\nTEXT 2,0 Door\n",
    )
    .unwrap();
    let text = ok(cli(&["run", s(&script), s(&dwg)]));
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    for line in &lines {
        assert!(line.contains("/height=0.25 m"), "{line}");
        assert!(line.contains("/rotation=0"), "{line}");
    }
    assert!(lines[0].contains("/typo"), "{text}");
}

#[test]
fn run_reads_the_project_beside_the_drawing_and_writes_it_only_on_save() {
    let dir = tempfile::tempdir().unwrap();
    let dwg = write_plan(dir.path(), "plan.dwg");
    // A project file beside the drawing with one measurement in its take-off.
    let mut session = Session::open(&dwg).unwrap();
    session
        .run(qiyas_core::Command::MeasureDistance {
            points: vec![[0.0, 0.0], [3.0, 4.0]].into(),
        })
        .unwrap();
    let project = session.save_project().unwrap();
    drop(session);
    let before = std::fs::read(&project).unwrap();

    let script = dir.path().join("export.scr");
    std::fs::write(&script, "COUNT DOOR\nEXPORT t.xlsx\n").unwrap();
    ok(cli(&["run", s(&script), s(&dwg)]));
    let rows = rows(&dir.path().join("t.xlsx"));
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0].1, 5.0);
    assert_eq!(std::fs::read(&project).unwrap(), before, "not written");

    // SAVEAS writes the copy and its project file, with the count in it.
    std::fs::write(&script, "COUNT DOOR\nSAVEAS copy.dxf\n").unwrap();
    ok(cli(&["run", s(&script), s(&dwg)]));
    assert!(dir.path().join("copy.dxf").exists());
    assert_eq!(
        Session::open(&dir.path().join("copy.dxf"))
            .unwrap()
            .takeoff
            .items
            .len(),
        2
    );
    assert_eq!(
        std::fs::read(&project).unwrap(),
        before,
        "still not written"
    );
}

#[test]
fn run_errors_name_the_script_and_line_and_nothing_runs() {
    let dir = tempfile::tempdir().unwrap();
    let dwg = write_plan(dir.path(), "plan.dwg");
    let script = dir.path().join("bad.scr");
    std::fs::write(&script, "EXPORT first.xlsx\nFOO bar\n").unwrap();
    let err = failed(cli(&["run", s(&script), s(&dwg)]));
    assert!(
        err.ends_with("bad.scr: line 2: unknown command FOO"),
        "{err}"
    );
    assert!(
        !dir.path().join("first.xlsx").exists(),
        "checked whole first"
    );

    std::fs::write(&script, "EXPORT ../outside.xlsx\n").unwrap();
    let err = failed(cli(&["run", s(&script), s(&dwg)]));
    assert!(err.contains("line 1") && err.contains("outside"), "{err}");

    // A step that fails stops the run on its line.
    std::fs::write(&script, "COUNT DOOR\nMEASURE FFFF\n").unwrap();
    let err = failed(cli(&["run", s(&script), s(&dwg), "--json"]));
    assert!(err.ends_with("line 2: No entity with handle FFFF"), "{err}");

    // The script must OPEN a drawing when none is given.
    std::fs::write(&script, "COUNT DOOR\n").unwrap();
    let err = failed(cli(&["run", s(&script)]));
    assert!(err.contains("OPEN"), "{err}");
    let err = failed(cli(&["run", s(&dir.path().join("none.scr"))]));
    assert!(err.contains("none.scr"), "{err}");
}

#[test]
fn takeoff_counts_blocks_and_exports_without_writing_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let dwg = write_plan(dir.path(), "plan.dwg");
    let xlsx = dir.path().join("out.xlsx");
    let text = ok(cli(&[
        "takeoff",
        s(&dwg),
        "--count",
        "DOOR",
        "--count",
        "WINDOW",
        "--count",
        "COLUMN",
        "--export",
        s(&xlsx),
    ]));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[..3],
        [
            "DOOR count = 3 pcs",
            "WINDOW count = 5 pcs",
            "COLUMN count = 6 pcs"
        ]
    );
    assert!(lines[3].starts_with("exported "), "{text}");
    assert_eq!(rows(&xlsx).len(), 3);
    assert!(!dir.path().join("plan.qiyas").exists());

    let v = json(&ok(cli(&[
        "takeoff",
        s(&dwg),
        "--count",
        "WINDOW",
        "--json",
    ])));
    assert_eq!(v["counts"]["WINDOW"], 5.0);
    assert_eq!(v["export"], serde_json::Value::Null);
    // Nothing to do is a usage error.
    failed(cli(&["takeoff", s(&dwg)]));
}

/// The script of Review Focus 3: count three blocks, export the take-off (a bare EXPORT:
/// `{name}.{ext}.xlsx`), quit.
const COUNT_SCRIPT: &str = "COUNT DOOR\nCOUNT WINDOW\nCOUNT COLUMN\nEXPORT\nQUIT\n";

#[test]
fn batch_of_fifty_drawings_skips_the_corrupt_one_and_fails_the_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let plans = dir.path().join("plans");
    let out = dir.path().join("out");
    std::fs::create_dir_all(&plans).unwrap();
    let bytes = plan_bytes(DxfVersion::AC1032);
    for i in 1..=50 {
        let name = format!("plan-{i:02}.dwg");
        if i == 17 {
            // Cut off in the middle: the header is there, the rest of the file is not, and the
            // failsafe reader finds nothing usable, so it does not open at all.
            std::fs::write(plans.join(&name), &bytes[..bytes.len() / 2]).unwrap();
        } else {
            std::fs::write(plans.join(&name), &bytes).unwrap();
        }
    }
    // Not drawings: left out of the batch.
    std::fs::write(plans.join("notes.txt"), "x").unwrap();
    std::fs::write(plans.join("plan-01.dwg.bak"), "x").unwrap();
    let script = dir.path().join("count.scr");
    std::fs::write(&script, COUNT_SCRIPT).unwrap();

    let run = cli(&[
        "batch",
        s(&plans),
        "--script",
        s(&script),
        "--out",
        s(&out),
        "--jobs",
        "2",
    ]);
    let text = stdout(&run);
    let err = failed(run);
    assert_eq!(err, "qiyas-cli: 1 of 50 drawings failed");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.len(),
        51,
        "a line per drawing and the summary: {text}"
    );
    for (i, line) in lines[..50].iter().enumerate() {
        let name = format!("plan-{:02}.dwg", i + 1);
        assert!(
            line.starts_with(s(&plans.join(&name))),
            "folder order: {line}"
        );
        if i + 1 == 17 {
            assert!(
                line.contains(" error ") && line.contains("can't be opened"),
                "{line}"
            );
        } else {
            // Counts by block name, sorted.
            assert!(line.ends_with(" ok COLUMN=6 DOOR=3 WINDOW=5"), "{line}");
        }
    }
    assert_eq!(lines[50], "50 drawings: 49 ok, 0 with warnings, 1 failed");
    let workbooks = std::fs::read_dir(&out).unwrap().count();
    assert_eq!(workbooks, 49, "every good drawing has its Excel");
    assert!(!out.join("plan-17.dwg.xlsx").exists());
    assert_eq!(rows(&out.join("plan-50.dwg.xlsx")).len(), 3);
    // Nothing written beside the drawings.
    assert_eq!(std::fs::read_dir(&plans).unwrap().count(), 52);

    let run = cli(&[
        "batch",
        s(&plans),
        "--script",
        s(&script),
        "--out",
        s(&out),
        "--json",
    ]);
    let v = json(&stdout(&run));
    failed(run);
    assert_eq!(
        (v["total"].as_u64(), v["ok"].as_u64(), v["failed"].as_u64()),
        (Some(50), Some(49), Some(1))
    );
    let files = v["files"].as_array().unwrap();
    assert_eq!(files[16]["status"], "error");
    assert!(files[16]["error"]
        .as_str()
        .unwrap()
        .contains("can't be opened"));
    assert_eq!(files[0]["status"], "ok");
    assert_eq!(files[0]["counts"]["WINDOW"], 5.0);
    // --quiet: only the summary on stdout, the failure still on stderr.
    let run = cli(&[
        "batch",
        s(&plans),
        "--script",
        s(&script),
        "--out",
        s(&out),
        "--quiet",
    ]);
    assert_eq!(
        stdout(&run),
        "50 drawings: 49 ok, 0 with warnings, 1 failed\n"
    );
    failed(run);
}

/// The test plan in AC1018 written into `dir` damaged at 64 places in turn (128 bytes of junk
/// each, `d00.dwg` to `d63.dwg`); the undamaged bytes.
fn damaged_copies(dir: &Path) -> Vec<u8> {
    std::fs::create_dir_all(dir).unwrap();
    let bytes = plan_bytes(DxfVersion::AC1018);
    let step = bytes.len() / 64;
    for k in 0..64 {
        let mut damaged = bytes.clone();
        let at = k * step;
        for (i, b) in damaged[at..at + 128].iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(37) ^ 0xA5;
        }
        std::fs::write(dir.join(format!("d{k:02}.dwg")), damaged).unwrap();
    }
    bytes
}

/// Damaged copies the failsafe reader opens with problems (a warning), alone in `dir`.
fn repaired_copies(dir: &Path, scratch: &Path) -> Vec<PathBuf> {
    let sweep = scratch.join("sweep");
    damaged_copies(&sweep);
    let script = scratch.join("sweep.scr");
    std::fs::write(&script, COUNT_SCRIPT).unwrap();
    let out = scratch.join("sweep-out");
    let run = cli(&[
        "batch",
        s(&sweep),
        "--script",
        s(&script),
        "--out",
        s(&out),
        "--json",
    ]);
    let v = json(&stdout(&run));
    std::fs::create_dir_all(dir).unwrap();
    let copies: Vec<PathBuf> = v["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["status"] == "warning")
        .map(|f| {
            let from = Path::new(f["path"].as_str().unwrap());
            let to = dir.join(from.file_name().unwrap());
            std::fs::copy(from, &to).unwrap();
            to
        })
        .collect();
    assert!(!copies.is_empty(), "some damaged copies open with problems");
    copies
}

#[test]
fn batch_reports_damaged_drawings_without_stopping() {
    // One drawing damaged at 64 places in turn (128 bytes of junk each), plus one whose header is
    // junk. The failsafe reader opens some damaged copies cleanly, some with problems (a warning:
    // the script still runs), refuses others, and the pinned codec panics on some: each one is a
    // line of the report and the batch goes on.
    let dir = tempfile::tempdir().unwrap();
    let plans = dir.path().join("plans");
    let bytes = damaged_copies(&plans);
    let mut junk = bytes.clone();
    junk[..6].copy_from_slice(b"JUNK!!");
    std::fs::write(plans.join("junk.dwg"), junk).unwrap();
    let script = dir.path().join("count.scr");
    std::fs::write(&script, COUNT_SCRIPT).unwrap();
    let out = dir.path().join("out");

    let run = cli(&[
        "batch",
        s(&plans),
        "--script",
        s(&script),
        "--out",
        s(&out),
        "--jobs",
        "2",
        "--json",
    ]);
    let v = json(&stdout(&run));
    let err = failed(run);
    let files = v["files"].as_array().unwrap();
    assert_eq!(files.len(), 65);
    let status = |f: &serde_json::Value| f["status"].as_str().unwrap().to_string();
    let junk = files
        .iter()
        .find(|f| f["path"].as_str().unwrap().ends_with("junk.dwg"))
        .unwrap();
    assert_eq!(status(junk), "error");
    let warned: Vec<&serde_json::Value> = files.iter().filter(|f| status(f) == "warning").collect();
    assert!(!warned.is_empty(), "some copies open with problems");
    for f in &warned {
        assert!(f["problems"].as_u64().unwrap() > 0);
        let name = Path::new(f["path"].as_str().unwrap()).file_name().unwrap();
        let xlsx = out.join(format!("{}.xlsx", name.to_string_lossy()));
        assert!(xlsx.exists(), "the script ran on {f}");
    }
    let crashed = files
        .iter()
        .filter(|f| {
            f["error"]
                .as_str()
                .is_some_and(|e| e.starts_with("crashed: "))
        })
        .count();
    // If the codec stops panicking on these, this test no longer shows that a crash is caught.
    assert!(
        crashed > 0,
        "the pinned reader panics on some damaged copies"
    );
    let failed_n = files.iter().filter(|f| status(f) == "error").count();
    assert_eq!(v["failed"].as_u64(), Some(failed_n as u64));
    assert_eq!(err, format!("qiyas-cli: {failed_n} of 65 drawings failed"));
    let workbooks = std::fs::read_dir(&out).unwrap().count();
    assert_eq!(workbooks, 65 - failed_n);
}

#[test]
fn batch_runs_a_sheet_sets_drawings_and_refuses_a_bad_script_up_front() {
    let dir = tempfile::tempdir().unwrap();
    write_plan(dir.path(), "a.dwg");
    write_plan(dir.path(), "b.dwg");
    write_plan(dir.path(), "c.dwg");
    let qss = dir.path().join("set.qss");
    std::fs::write(
        &qss,
        r#"{"name":"Set","drawings":[{"path":"b.dwg"},{"path":"a.dwg"},{"path":"gone.dwg"}]}"#,
    )
    .unwrap();
    let script = dir.path().join("count.scr");
    std::fs::write(&script, COUNT_SCRIPT).unwrap();

    let run = cli(&["batch", s(&qss), "--script", s(&script)]);
    let text = stdout(&run);
    assert_eq!(failed(run), "qiyas-cli: 1 of 3 drawings failed");
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].contains("b.dwg ok "), "the set's order: {text}");
    assert!(lines[1].contains("a.dwg ok "), "{text}");
    assert!(lines[2].contains("gone.dwg error "), "{text}");
    // Without --out the workbooks go to the script's folder.
    assert!(dir.path().join("a.dwg.xlsx").exists() && dir.path().join("b.dwg.xlsx").exists());
    assert!(!dir.path().join("c.dwg.xlsx").exists());

    // A bad script is refused before any drawing is touched.
    std::fs::write(&script, "COUNT DOOR\nEXPORT {name}.xlsx\nBOGUS\n").unwrap();
    let out = dir.path().join("out");
    let run = cli(&[
        "batch",
        s(dir.path()),
        "--script",
        s(&script),
        "--out",
        s(&out),
    ]);
    assert_eq!(stdout(&run), "");
    let err = failed(run);
    assert!(
        err.ends_with("count.scr: line 3: unknown command BOGUS"),
        "{err}"
    );
    assert!(!out.exists());
    // A folder with no drawings is an error too.
    let empty = dir.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let err = failed(cli(&["batch", s(&empty), "--script", s(&script)]));
    assert!(err.contains("no drawings"), "{err}");
}

/// A lock beside `drawing` from a process of this computer that has ended: Qiyas crashed.
fn dead_lock(drawing: &Path) {
    assert!(qiyas_core::take_lock(drawing).unwrap());
    let mut lock = qiyas_core::read_lock(drawing).unwrap();
    let mut child = Command::new("true").spawn().unwrap();
    lock.pid = child.id();
    child.wait().unwrap();
    std::fs::write(
        qiyas_core::lock_path(drawing),
        serde_json::to_vec(&lock).unwrap(),
    )
    .unwrap();
    assert_eq!(
        qiyas_core::read_lock(drawing).unwrap().status(),
        qiyas_core::LockStatus::Dead
    );
}

/// The app's autosave and its project beside `drawing` (work newer than the file).
fn autosave(drawing: &Path) -> [PathBuf; 2] {
    let sv = qiyas_core::autosave_path(drawing);
    let mut project = sv.clone().into_os_string();
    project.push(".qiyas");
    std::fs::write(&sv, b"newer work").unwrap();
    std::fs::write(&project, b"{}").unwrap();
    [sv, PathBuf::from(project)]
}

#[test]
fn the_apps_autosaves_and_locks_survive_every_headless_save() {
    let dir = tempfile::tempdir().unwrap();
    let dwg = write_plan(dir.path(), "plan.dwg");
    let dxf = dir.path().join("plan.dxf");
    let copy = dir.path().join("copy.dwg");
    let mut kept: Vec<PathBuf> = [&dwg, &dxf, &copy]
        .into_iter()
        .flat_map(|d| autosave(d))
        .collect();
    dead_lock(&dwg);
    kept.push(qiyas_core::lock_path(&dwg));

    ok(cli(&["convert", s(&dwg), s(&dxf)]));
    let script = dir.path().join("save.scr");
    std::fs::write(&script, "COUNT DOOR\nSAVE\nSAVEAS copy.dwg\n").unwrap();
    ok(cli(&["run", s(&script), s(&dwg)]));
    std::fs::write(&script, "COUNT DOOR\nSAVE\n").unwrap();
    let text = ok(cli(&["batch", s(dir.path()), "--script", s(&script)]));
    assert!(
        text.ends_with("3 drawings: 3 ok, 0 with warnings, 0 failed\n"),
        "{text}"
    );
    assert!(dir.path().join("plan.dwg.bak").exists() && dir.path().join("copy.dwg.bak").exists());
    for path in kept {
        assert!(path.exists(), "{} is the app's: left alone", path.display());
    }
}

/// A drawing open in a running Qiyas (its lock alive: here this test process's own, with its pid
/// and start time; or another computer's) is never written by SAVE, SAVEAS or convert: the
/// app's next Save would write its copy back over it. The lock is read, never written.
#[test]
fn a_drawing_open_in_a_running_qiyas_is_never_written() {
    let dir = tempfile::tempdir().unwrap();
    let dwg = write_plan(dir.path(), "plan.dwg");
    let other = write_plan(dir.path(), "other.dwg");
    assert!(qiyas_core::take_lock(&dwg).unwrap());
    let lock = std::fs::read(qiyas_core::lock_path(&dwg)).unwrap();
    assert_eq!(
        qiyas_core::read_lock(&dwg).unwrap().status(),
        qiyas_core::LockStatus::Mine,
        "the test process's own: alive for the command line"
    );
    let bytes = std::fs::read(&dwg).unwrap();
    let script = dir.path().join("save.scr");
    let refused = |out: Output| {
        let line = failed(out);
        assert!(line.contains("is open in Qiyas"), "{line}");
        assert!(line.contains(&std::process::id().to_string()), "{line}");
    };

    std::fs::write(&script, "COUNT DOOR\nSAVE\n").unwrap();
    refused(cli(&["run", s(&script), s(&dwg)]));
    std::fs::write(&script, "COUNT DOOR\nSAVEAS plan.dwg\n").unwrap();
    refused(cli(&["run", s(&script), s(&other)]));
    refused(cli(&["convert", s(&other), s(&dwg)]));
    std::fs::write(&script, "COUNT DOOR\nSAVE\n").unwrap();
    let run = cli(&["batch", s(dir.path()), "--script", s(&script)]);
    let text = stdout(&run);
    assert_eq!(failed(run), "qiyas-cli: 1 of 2 drawings failed");
    assert!(
        text.contains("plan.dwg error ") && text.contains("is open in Qiyas"),
        "{text}"
    );
    assert!(text.contains("other.dwg ok "), "{text}");

    // Another computer's lock counts as open there.
    std::fs::write(
        qiyas_core::lock_path(&dwg),
        r#"{"pid": 7, "host": "elsewhere", "started_at": 1}"#,
    )
    .unwrap();
    let line = failed(cli(&["convert", s(&other), s(&dwg)]));
    assert!(
        line.contains("open in Qiyas (process 7 on elsewhere)"),
        "{line}"
    );

    assert_eq!(std::fs::read(&dwg).unwrap(), bytes, "the drawing as it was");
    assert!(!dir.path().join("plan.dwg.bak").exists());
    assert!(!dir.path().join("plan.qiyas").exists());
    std::fs::write(qiyas_core::lock_path(&dwg), &lock).unwrap();
    assert_eq!(
        std::fs::read(qiyas_core::lock_path(&dwg)).unwrap(),
        lock,
        "the lock is read, never written"
    );
}

#[test]
fn batch_refuses_outputs_that_collide_before_running_them() {
    let dir = tempfile::tempdir().unwrap();
    let plans = dir.path().join("plans");
    std::fs::create_dir_all(&plans).unwrap();
    let dwg = write_plan(&plans, "plan.dwg");
    ok(cli(&["convert", s(&dwg), s(&plans.join("plan.dxf"))]));
    write_plan(&plans, "other.dwg");
    let script = dir.path().join("x.scr");
    let out = dir.path().join("out");
    let batch = |text: &str| {
        std::fs::write(&script, text).unwrap();
        cli(&[
            "batch",
            s(&plans),
            "--script",
            s(&script),
            "--out",
            s(&out),
            "--jobs",
            "2",
        ])
    };

    // plan.dwg and plan.dxf would both write out/plan.xlsx: both refused, the others run.
    let run = batch("COUNT DOOR\nEXPORT {name}.xlsx\n");
    let text = stdout(&run);
    assert_eq!(failed(run), "qiyas-cli: 2 of 3 drawings failed");
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].contains("other.dwg ok "), "{text}");
    for (line, other) in [(lines[1], "plan.dxf"), (lines[2], "plan.dwg")] {
        assert!(line.contains(" error output "), "{line}");
        assert!(
            line.contains("plan.xlsx collides with ") && line.ends_with(other),
            "{line}"
        );
    }
    assert!(!out.join("plan.xlsx").exists(), "neither ran");
    assert!(out.join("other.xlsx").exists());

    // One workbook for every drawing: all refused, nothing written.
    let run = batch("COUNT DOOR\nEXPORT summary.xlsx\n");
    let text = stdout(&run);
    assert_eq!(failed(run), "qiyas-cli: 3 of 3 drawings failed");
    assert_eq!(text.matches("collides with").count(), 3, "{text}");
    assert!(!out.join("summary.xlsx").exists());

    // The default export name keeps the extension: no collision.
    let run = batch("COUNT DOOR\nEXPORT\n");
    ok(run);
    for name in ["plan.dwg.xlsx", "plan.dxf.xlsx", "other.dwg.xlsx"] {
        assert_eq!(rows(&out.join(name)).len(), 1, "{name}");
    }
}

#[test]
fn drawings_opened_with_problems_warn_with_exit_2_and_are_not_saved_over() {
    let dir = tempfile::tempdir().unwrap();
    let warn = dir.path().join("warn");
    let copies = repaired_copies(&warn, dir.path());
    let n = copies.len();
    let script = dir.path().join("count.scr");
    std::fs::write(&script, COUNT_SCRIPT).unwrap();
    let out = dir.path().join("out");

    // Each is a warning with its counts; warnings only: exit 2, one line on stderr.
    let run = cli(&["batch", s(&warn), "--script", s(&script), "--out", s(&out)]);
    let text = stdout(&run);
    let err = warned(run);
    assert_eq!(
        err,
        format!("qiyas-cli: {n} of {n} drawings opened with problems: check their counts")
    );
    for line in text.lines().take(n) {
        assert!(
            line.contains(" warning (") && line.contains("reading it) "),
            "{line}"
        );
        assert!(line.contains("DOOR="), "with its counts: {line}");
    }
    // --quiet still prints the summary.
    let run = cli(&[
        "batch",
        s(&warn),
        "--script",
        s(&script),
        "--out",
        s(&out),
        "--quiet",
    ]);
    assert_eq!(
        stdout(&run),
        format!("{n} drawings: 0 ok, {n} with warnings, 0 failed\n")
    );
    warned(run);
    // The single-drawing commands warn the same way.
    let one = &copies[0];
    assert!(warned(cli(&["run", s(&script), s(one)])).contains("opened with"));
    let err = warned(cli(&["takeoff", s(one), "--count", "DOOR"]));
    assert!(err.contains("opened with"), "{err}");
    let err = warned(cli(&["convert", s(one), s(&dir.path().join("fixed.dxf"))]));
    assert!(err.contains("opened with"), "{err}");

    // SAVE would put what the reader made of each over the original and its .bak: refused.
    let before: Vec<Vec<u8>> = copies.iter().map(|c| std::fs::read(c).unwrap()).collect();
    std::fs::write(&script, "COUNT DOOR\nSAVE\n").unwrap();
    let run = cli(&["batch", s(&warn), "--script", s(&script)]);
    let text = stdout(&run);
    assert_eq!(
        failed(run),
        format!("qiyas-cli: {n} of {n} drawings failed")
    );
    assert_eq!(
        text.matches("line 2: the drawing was repaired on open; use SAVEAS")
            .count(),
        n,
        "{text}"
    );
    for (copy, bytes) in copies.iter().zip(&before) {
        assert_eq!(&std::fs::read(copy).unwrap(), bytes, "{}", copy.display());
        assert!(!Path::new(&format!("{}.bak", copy.display())).exists());
    }
    // Unless asked for.
    let run = cli(&[
        "batch",
        s(&warn),
        "--script",
        s(&script),
        "--allow-repaired-save",
    ]);
    let text = stdout(&run);
    assert!(!text.contains("repaired on open"), "{text}");
    assert!(copies
        .iter()
        .any(|c| Path::new(&format!("{}.bak", c.display())).exists()));
}

#[test]
fn a_project_file_that_cannot_be_read_is_a_warning_on_its_line() {
    let dir = tempfile::tempdir().unwrap();
    let plans = dir.path().join("plans");
    std::fs::create_dir_all(&plans).unwrap();
    write_plan(&plans, "a.dwg");
    write_plan(&plans, "b.dwg");
    std::fs::write(plans.join("b.qiyas"), b"not json {").unwrap();
    let script = dir.path().join("count.scr");
    std::fs::write(&script, COUNT_SCRIPT).unwrap();
    let out = dir.path().join("out");

    let run = cli(&["batch", s(&plans), "--script", s(&script), "--out", s(&out)]);
    let text = stdout(&run);
    assert_eq!(
        warned(run),
        "qiyas-cli: 1 of 2 drawings opened with problems: check their counts"
    );
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0].ends_with("a.dwg ok COLUMN=6 DOOR=3 WINDOW=5"),
        "{text}"
    );
    assert!(lines[1].contains("b.dwg warning ("), "{text}");
    assert!(
        lines[1].contains("Project file could not be read"),
        "{text}"
    );
    assert!(lines[1].ends_with(" COLUMN=6 DOOR=3 WINDOW=5"), "{text}");
    assert_eq!(lines[2], "2 drawings: 1 ok, 1 with warnings, 0 failed");
    // It still opened, with an empty take-off: the workbook has the counts alone.
    assert_eq!(rows(&out.join("b.dwg.xlsx")).len(), 3);
    let v = json(&stdout(&cli(&[
        "batch",
        s(&plans),
        "--script",
        s(&script),
        "--out",
        s(&out),
        "--json",
    ])));
    let b = &v["files"][1];
    assert_eq!(b["status"], "warning");
    assert!(b["warnings"][0]
        .as_str()
        .unwrap()
        .contains("Project file could not be read"));
    // takeoff and run warn too.
    let err = warned(cli(&[
        "takeoff",
        s(&plans.join("b.dwg")),
        "--count",
        "DOOR",
    ]));
    assert!(err.contains("Project file could not be read"), "{err}");
    let err = warned(cli(&["run", s(&script), s(&plans.join("b.dwg"))]));
    assert!(err.contains("Project file could not be read"), "{err}");
}

#[test]
fn outputs_that_differ_only_in_case_collide() {
    // Windows and macOS see Plan.xlsx and plan.xlsx as one file: refused everywhere.
    let dir = tempfile::tempdir().unwrap();
    let plans = dir.path().join("plans");
    std::fs::create_dir_all(&plans).unwrap();
    write_plan(&plans, "Plan.dwg");
    write_plan(&plans, "plan.dwg");
    let script = dir.path().join("x.scr");
    std::fs::write(&script, "COUNT DOOR\nEXPORT {name}.xlsx\n").unwrap();
    let run = cli(&["batch", s(&plans), "--script", s(&script)]);
    let text = stdout(&run);
    assert_eq!(failed(run), "qiyas-cli: 2 of 2 drawings failed");
    assert_eq!(text.matches("collides with").count(), 2, "{text}");
}
