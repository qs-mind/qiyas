//! Command scripts (`qiyas_core::script`): the `.scr` grammar is read and checked whole before
//! anything runs, paths stay inside the script's and the drawing's folders, and the runner
//! drives a `Session` without a window, writing only on SAVE, SAVEAS and EXPORT.

use std::path::{Path, PathBuf};

use opencadcodec::EntityType;
use qiyas_core::fixtures::write_sample;
use qiyas_core::script::{self, Action, Context, Host, Script, ScriptError};
use qiyas_core::{Boq, Command, Kind, LayerProp, Session};

/// A host that records what the script asked for, with no Excel behind it.
#[derive(Default)]
struct Stub {
    exported: Vec<PathBuf>,
    compared: usize,
}

impl Host for Stub {
    fn export_takeoff(&mut self, _session: &Session, path: &Path) -> Result<(), String> {
        std::fs::write(path, b"xlsx").map_err(|e| e.to_string())?;
        self.exported.push(path.to_path_buf());
        Ok(())
    }

    fn import_boq(&mut self, path: &Path) -> Result<Boq, String> {
        if !path.exists() {
            return Err(format!("{} is not there", path.display()));
        }
        Ok(Boq::new(path.display().to_string(), Vec::new(), 0))
    }

    fn compare(&mut self, _session: &mut Session) -> Result<usize, String> {
        self.compared += 1;
        Ok(2)
    }
}

/// Parses `text` as a script at `dir/run.scr`, on `drawing`.
fn parse(text: &str, dir: &Path, drawing: Option<&Path>) -> Result<Script, ScriptError> {
    let script = dir.join("run.scr");
    script::parse(
        text,
        &Context {
            script: &script,
            drawing,
            out_dir: None,
        },
    )
}

/// The error a run stops with.
fn fails(result: Result<script::Run, ScriptError>) -> ScriptError {
    match result {
        Err(e) => e,
        Ok(_) => panic!("the run was expected to fail"),
    }
}

fn parsed(text: &str, dir: &Path, drawing: Option<&Path>) -> Script {
    parse(text, dir, drawing).unwrap_or_else(|e| panic!("{e}"))
}

/// The line number and message a script fails with.
fn error(text: &str, dir: &Path, drawing: Option<&Path>) -> (usize, String) {
    let e = parse(text, dir, drawing).expect_err("the script is refused");
    (e.line, e.message)
}

fn points(action: &Action) -> &[[f64; 2]] {
    match action {
        Action::Command(Command::MeasureDistance { points }) => points,
        Action::Command(Command::MeasureArea { points }) => points,
        other => panic!("expected a measure, got {other:?}"),
    }
}

fn near(a: [f64; 2], b: [f64; 2]) {
    assert!(
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9,
        "{a:?} is not {b:?}"
    );
}

#[test]
fn reads_words_points_and_arabic_digits() {
    let dir = tempfile::tempdir().unwrap();
    let text = "; a comment\n\
                COUNT DOOR\n\
                \n\
                di 0,0 3,4\n\
                AA ١٠،١٠ @٢,٠\n\
                @2<90\n\
                \n\
                LAYOFF A-WALL \"A DOOR\"\n\
                LAYFRZ A-WALL\n\
                LAYER CURRENT A-DOOR\n\
                MEASURE 2F\n\
                ZOOM E\n\
                ECHO all done\n\
                DELAY ٥\n\
                {\"type\":\"count_block\",\"block_name\":\"WINDOW\",\"layer\":null}\n\
                QUIT\n";
    let script = parsed(text, dir.path(), None);
    let lines: Vec<usize> = script.steps.iter().map(|s| s.line).collect();
    assert_eq!(lines, [2, 4, 5, 8, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
    let actions: Vec<&Action> = script.steps.iter().map(|s| &s.action).collect();
    assert!(matches!(
        actions[0],
        Action::Command(Command::CountBlock { block_name, layer: None }) if block_name == "DOOR"
    ));
    assert_eq!(points(actions[1]), [[0.0, 0.0], [3.0, 4.0]]);
    // Arabic digits and separators; `@` from the point before, `@d<a` polar in degrees.
    let area = points(actions[2]);
    assert_eq!(area.len(), 3);
    near(area[0], [10.0, 10.0]);
    near(area[1], [12.0, 10.0]);
    near(area[2], [12.0, 12.0]);
    assert!(matches!(
        actions[3],
        Action::Command(Command::SetLayerHidden { name, hidden: true }) if name == "A-WALL"
    ));
    assert!(matches!(
        actions[4],
        Action::Command(Command::SetLayerHidden { name, hidden: true }) if name == "A DOOR"
    ));
    assert!(matches!(
        actions[5],
        Action::Command(Command::SetLayer { name, prop: LayerProp::Frozen, value })
            if name == "A-WALL" && *value == serde_json::Value::Bool(true)
    ));
    assert!(matches!(actions[6], Action::CurrentLayer(name) if name == "A-DOOR"));
    assert!(matches!(
        actions[7],
        Action::Command(Command::MeasureEntity { handle: 0x2F, chain }) if chain.is_empty()
    ));
    assert!(matches!(actions[8], Action::Zoom));
    assert!(matches!(actions[9], Action::Echo(text) if text == "all done"));
    assert!(matches!(actions[10], Action::Delay(5)));
    assert!(matches!(
        actions[11],
        Action::Command(Command::CountBlock { block_name, .. }) if block_name == "WINDOW"
    ));
    assert!(matches!(actions[12], Action::Quit));
}

#[test]
fn a_relative_point_needs_a_point_before_it() {
    let dir = tempfile::tempdir().unwrap();
    let (line, message) = error("ECHO first\nDI @1,1 2,2", dir.path(), None);
    assert_eq!(line, 2);
    assert!(message.contains("@"), "{message}");
    // The last point carries across commands.
    let script = parsed("DI 1,1 2,2\nDI @1,0 @0,1", dir.path(), None);
    assert_eq!(points(&script.steps[1].action), [[3.0, 2.0], [3.0, 3.0]]);
}

#[test]
fn registry_words_share_points_with_the_legacy_grammar() {
    let dir = tempfile::tempdir().unwrap();
    let script = parsed(
        "DI 0,0 2,3\nL @١،٠ @٢<٩٠\nDI @1,0 @0,1\nPLINE 0,0 10,0 C",
        dir.path(),
        None,
    );
    let Action::Command(command) = &script.steps[1].action else {
        panic!("a registry command");
    };
    let command = serde_json::to_value(command).unwrap();
    assert_eq!(command["type"], "draw");
    assert_eq!(command["kind"], "line");
    assert_eq!(command["a"], serde_json::json!([3.0, 3.0]));
    assert_eq!(command["b"], serde_json::json!([3.0, 5.0]));
    assert_eq!(points(&script.steps[2].action), [[4.0, 5.0], [4.0, 6.0]]);
    let Action::Command(command) = &script.steps[3].action else {
        panic!("a polyline");
    };
    let command = serde_json::to_value(command).unwrap();
    assert_eq!(command["kind"], "polyline");
    assert_eq!(command["closed"], true);
}

#[test]
fn json_and_typed_script_lines_keep_omissions_and_report_input_notes() {
    let dir = tempfile::tempdir().unwrap();
    let script = parsed(
        "{\"type\":\"text\",\"at\":[0,0],\"value\":\"Room\",\"typo\":true}\nTEXT 2,0 Door\n",
        dir.path(),
        None,
    );
    for step in &script.steps {
        assert!(step.input.as_ref().unwrap().get("height").is_none());
        assert!(matches!(step.action, Action::Command(Command::Text { .. })));
    }
    assert_eq!(script.steps[0].input.as_ref().unwrap()["typo"], true);
    let session = Session::new_blank(&dir.path().join("input-notes.dxf"), 6);
    let run = script::run(&script, Some(session), &mut Stub::default()).unwrap();
    assert!(run.steps[0].text.contains("/typo"), "{}", run.steps[0].text);
    for step in run.steps {
        assert!(step.text.contains("height=0.25 m"), "{}", step.text);
        assert!(step.text.contains("rotation=0"), "{}", step.text);
    }
}

#[test]
fn registry_prompts_consume_following_lines_and_report_bad_answers() {
    let dir = tempfile::tempdir().unwrap();
    let script = parsed(
        "LINE\n; first point\n١٫٥،٢\n@٣،٤\nCIRCLE\n0,0\n٢",
        dir.path(),
        None,
    );
    assert_eq!(
        script.steps.iter().map(|s| s.line).collect::<Vec<_>>(),
        [1, 5]
    );
    let Action::Command(command) = &script.steps[0].action else {
        panic!("a line");
    };
    let command = serde_json::to_value(command).unwrap();
    assert_eq!(command["a"], serde_json::json!([1.5, 2.0]));
    assert_eq!(command["b"], serde_json::json!([4.5, 6.0]));
    assert_eq!(error("LINE\n0,0\nbad", dir.path(), None).0, 3);
    assert!(parse("COUNT DOOR\nLINE 0,0", dir.path(), None).is_err());
}

#[test]
fn new_registry_words_work_without_script_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let script = parsed(
        "NEWLAYER \"Typed layer\"\nRENAMELAYOUT \"Old sheet\" \"New sheet\"",
        dir.path(),
        None,
    );
    assert!(matches!(
        &script.steps[0].action,
        Action::Command(Command::NewLayer { name }) if name == "Typed layer"
    ));
    assert!(matches!(
        &script.steps[1].action,
        Action::Command(Command::RenameLayout { from, to })
            if from == "Old sheet" && to == "New sheet"
    ));
    let script = parsed("WBLOCK DOOR blocks/door.dwg", dir.path(), None);
    assert!(matches!(
        &script.steps[0].action,
        Action::Command(Command::WriteBlock { path, .. })
            if Path::new(path) == dir.path().join("blocks/door.dwg")
    ));
    assert!(parse("WBLOCK DOOR ../outside.dwg", dir.path(), None).is_err());
}

#[test]
fn errors_name_their_line() {
    let dir = tempfile::tempdir().unwrap();
    let (line, message) = error("COUNT DOOR\n\nFOO bar", dir.path(), None);
    assert_eq!((line, message.as_str()), (3, "unknown command FOO"));
    assert_eq!(error("DI 0,0 x,y", dir.path(), None).0, 1);
    assert_eq!(error("DI 0,0\n1,1 2,2", dir.path(), None).0, 2);
    assert_eq!(error("DI 0,0\n\n1,1", dir.path(), None).0, 1);
    assert_eq!(error("AA 0,0 1,0\n\nCOUNT DOOR", dir.path(), None).0, 1);
    assert_eq!(error("MEASURE zz", dir.path(), None).0, 1);
    assert_eq!(error("DELAY soon", dir.path(), None).0, 1);
    assert_eq!(error("LAYER OFF A-WALL", dir.path(), None).0, 1);
    assert_eq!(error("ZOOM W", dir.path(), None).0, 1);
    assert_eq!(error("{\"type\":\"no_such\"}", dir.path(), None).0, 1);
    assert_eq!(error("COUNT", dir.path(), None).0, 1);
    let e = parse("SAVE\nQUIT now", dir.path(), None).unwrap_err();
    assert_eq!(e.to_string(), "line 2: QUIT takes nothing after it");
}

#[test]
fn paths_resolve_from_the_scripts_folder_and_stay_inside() {
    let dir = tempfile::tempdir().unwrap();
    let scripts = dir.path().join("scripts");
    let plans = dir.path().join("plans");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::create_dir_all(&plans).unwrap();
    let drawing = write_sample(&plans, "dwg");

    // Relative to the script's folder; the drawing's folder is allowed once a drawing is known.
    let script = parsed("OPEN ../plans/sample.dwg", &scripts, Some(&drawing));
    assert!(
        matches!(&script.steps[0].action, Action::Open(p) if p == &drawing.canonicalize().unwrap())
    );
    let (line, message) = error("OPEN ../plans/sample.dwg", &scripts, None);
    assert_eq!(line, 1);
    assert!(message.contains("outside"), "{message}");
    // Climbing out of both folders is refused, written or read, absolute or not.
    assert!(error("EXPORT ../../out.xlsx", &scripts, Some(&drawing))
        .1
        .contains("outside"));
    assert!(error("IMPORTBOQ ../../boq.xlsx", &scripts, Some(&drawing))
        .1
        .contains("outside"));
    let outside = dir.path().parent().unwrap().join("x.xlsx");
    assert!(error(
        &format!("EXPORT \"{}\"", outside.display()),
        &scripts,
        Some(&drawing)
    )
    .1
    .contains("outside"));
    // `{name}` is the drawing's file name without its extension.
    let script = parsed("EXPORT {name}-takeoff.xlsx", &scripts, Some(&drawing));
    assert!(matches!(
        &script.steps[0].action,
        Action::Export(p) if p == &scripts.canonicalize().unwrap().join("sample-takeoff.xlsx")
    ));
    assert_eq!(error("EXPORT {name}.xlsx", &scripts, None).0, 1);
}

#[test]
fn saveas_takes_a_format_word_and_an_out_folder_takes_written_files() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let drawing = write_sample(dir.path(), "dwg");
    let script = dir.path().join("run.scr");
    let cx = Context {
        script: &script,
        drawing: Some(&drawing),
        out_dir: Some(&out),
    };
    let real = dir.path().canonicalize().unwrap();
    let s = script::parse(
        "SAVEAS copy dxf\nSAVEAS \"copy 2.dwg\"\nEXPORT t.xlsx\nIMPORTBOQ boq.xlsx",
        &cx,
    )
    .unwrap();
    let paths: Vec<&PathBuf> = s
        .steps
        .iter()
        .map(|s| match &s.action {
            Action::SaveAs(p) | Action::Export(p) | Action::ImportBoq(p) => p,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(paths[0], &real.join("out/copy.dxf"));
    assert_eq!(paths[1], &real.join("out/copy 2.dwg"));
    assert_eq!(paths[2], &real.join("out/t.xlsx"));
    // Read files come from the script's folder still.
    assert_eq!(paths[3], &real.join("boq.xlsx"));
    for bad in [
        "SAVEAS copy",
        "SAVEAS copy.dwg dxf",
        "SAVEAS copy.txt",
        "SAVEAS a b c",
    ] {
        assert_eq!(script::parse(bad, &cx).unwrap_err().line, 1, "{bad}");
    }
}

/// Opens the sample in `dir` and runs `text` on it with a stub host.
fn run_on_sample(dir: &Path, text: &str) -> (script::Run, Stub) {
    let drawing = write_sample(dir, "dwg");
    let session = Session::open(&drawing).unwrap();
    let script = parsed(text, dir, Some(&drawing));
    let mut host = Stub::default();
    let run = script::run(&script, Some(session), &mut host).unwrap_or_else(|e| panic!("{e}"));
    (run, host)
}

#[test]
fn runs_measures_layers_and_export_then_stops_at_quit() {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::open(&write_sample(dir.path(), "dwg")).unwrap();
    let line = s
        .drawing
        .entities_in("*Model_Space")
        .find(|e| matches!(e, EntityType::Line(_)))
        .unwrap()
        .common()
        .handle
        .value();
    drop(s);
    let text = format!(
        "COUNT DOOR\n\
         DI 0,0 3,4\n\
         AA 0,0 4,0 4,3 0,3\n\
         \n\
         MEASURE {line:X}\n\
         LAYOFF A-WALL\n\
         LAYFRZ A-DOOR\n\
         LAYER CURRENT A-DOOR\n\
         ECHO half way\n\
         EXPORT {{name}}.xlsx\n\
         LAYON A-WALL\n\
         LAYTHW A-DOOR\n\
         ZOOM E\n\
         QUIT\n\
         COUNT DOOR\n"
    );
    let (run, host) = run_on_sample(dir.path(), &text);
    let session = run.session.as_ref().expect("the session stays open");
    assert_eq!(run.counts.get("DOOR"), Some(&3.0));
    let kinds: Vec<Kind> = session.takeoff.items.iter().map(|m| m.kind).collect();
    assert_eq!(kinds, [Kind::Count, Kind::Length, Kind::Area, Kind::Length]);
    assert_eq!(session.takeoff.items[1].value, 5.0);
    assert_eq!(session.takeoff.items[2].value, 12.0);
    assert_eq!(session.takeoff.items[3].value, 10.0);
    assert!(session.hidden_layers().is_empty());
    assert!(session.layers().iter().all(|l| !l.frozen));
    assert_eq!(session.current.layer, "A-DOOR");
    assert_eq!(
        host.exported,
        [dir.path().canonicalize().unwrap().join("sample.xlsx")]
    );
    // QUIT ends the run: the COUNT after it never runs, and QUIT itself is the last step.
    assert_eq!(run.steps.len(), 13);
    assert_eq!(run.steps.last().unwrap().line, 14);
    let texts: Vec<&str> = run.steps.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts[0], "DOOR count = 3 pcs");
    assert_eq!(texts[1], "Distance = 5.000 m");
    assert_eq!(texts[2], "Area = 12.000 m2");
    assert_eq!(texts[3], "Line on A-WALL = 10.000 m");
    assert_eq!(texts[4], "layer A-WALL off");
    assert_eq!(texts[5], "layer A-DOOR frozen");
    assert_eq!(texts[6], "current layer A-DOOR");
    assert_eq!(texts[7], "half way");
    assert!(texts[8].starts_with("exported "), "{}", texts[8]);
    // Nothing is saved behind the user's back: no project file beside the drawing.
    assert!(!dir.path().join("sample.qiyas").exists());
}

#[test]
fn open_saveas_and_save_write_like_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let drawing = write_sample(dir.path(), "dwg");
    let script = parsed(
        "OPEN sample.dwg\nCOUNT DOOR\nSAVEAS copy.dxf\n",
        dir.path(),
        None,
    );
    let run = script::run(&script, None, &mut Stub::default()).unwrap_or_else(|e| panic!("{e}"));
    let copy = dir.path().join("copy.dxf");
    assert!(copy.exists());
    // SAVEAS saves the project file beside the new file and keeps no .bak (there was nothing).
    assert!(dir.path().join("copy.qiyas").exists());
    assert!(!dir.path().join("copy.dxf.bak").exists());
    assert!(!dir.path().join("sample.qiyas").exists());
    assert!(
        run.steps[0].text.starts_with("opened "),
        "{}",
        run.steps[0].text
    );
    // SAVE writes over the file it saved as, keeping a .bak as the app does.
    let script = parsed("SAVE\n", dir.path(), Some(&copy));
    let run =
        script::run(&script, run.session, &mut Stub::default()).unwrap_or_else(|e| panic!("{e}"));
    assert!(dir.path().join("copy.dxf.bak").exists());
    let session = run.session.unwrap();
    assert_eq!(session.drawing.path, copy.canonicalize().unwrap());
    assert!(
        run.steps[0].text.starts_with("saved "),
        "{}",
        run.steps[0].text
    );
    // The saved copy carries the count.
    let again = Session::open(&copy).unwrap();
    assert_eq!(again.takeoff.items.len(), 1);
    assert!(drawing.exists());
}

#[test]
fn a_command_before_any_open_and_a_failing_command_name_their_line() {
    let dir = tempfile::tempdir().unwrap();
    let script = parsed("ECHO hi\nCOUNT DOOR", dir.path(), None);
    let e = fails(script::run(&script, None, &mut Stub::default()));
    assert_eq!(e.line, 2);
    assert!(e.message.contains("OPEN"), "{e}");

    let drawing = write_sample(dir.path(), "dwg");
    let script = parsed(
        "COUNT DOOR\nMEASURE FFFF\nCOUNT DOOR",
        dir.path(),
        Some(&drawing),
    );
    let session = Session::open(&drawing).unwrap();
    let e = fails(script::run(&script, Some(session), &mut Stub::default()));
    assert_eq!(e.to_string(), "line 2: No entity with handle FFFF");
    // OPEN of a file that is not a drawing fails on its line.
    std::fs::write(dir.path().join("junk.dwg"), b"not a drawing").unwrap();
    let script = parsed("OPEN junk.dwg", dir.path(), None);
    let e = fails(script::run(&script, None, &mut Stub::default()));
    assert_eq!(e.line, 1);
    assert!(e.message.contains("can't be opened"), "{e}");
}

#[test]
fn import_boq_and_compare_go_through_the_host() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("boq.xlsx"), b"xlsx").unwrap();
    let (run, host) = run_on_sample(dir.path(), "IMPORTBOQ boq.xlsx\nCOMPARE\n");
    let session = run.session.as_ref().unwrap();
    assert!(session.boq.is_some());
    assert_eq!(host.compared, 1);
    assert_eq!(run.steps[1].text, "2 conflicts");
    assert!(run.steps[0].text.starts_with("imported 0 BOQ items"));
    // A missing workbook is the host's error, on the script's line.
    let drawing = dir.path().join("sample.dwg");
    let script = parsed("IMPORTBOQ none.xlsx", dir.path(), Some(&drawing));
    let session = Session::open(&drawing).unwrap();
    let e = fails(script::run(&script, Some(session), &mut Stub::default()));
    assert_eq!(e.line, 1);
}

#[test]
fn a_json_line_reaches_the_whole_command_bus() {
    let dir = tempfile::tempdir().unwrap();
    let text = "{\"type\":\"draw\",\"kind\":\"line\",\"a\":[0,0],\"b\":[0,5]}\n\
                {\"type\":\"measure_distance\",\"points\":[[0,0],[0,5]]}\n";
    let (run, _) = run_on_sample(dir.path(), text);
    assert_eq!(run.steps[0].text, "added 1 entities");
    assert_eq!(run.steps[1].text, "Distance = 5.000 m");
}

#[test]
fn a_bom_at_the_start_is_skipped() {
    // Notepad's "UTF-8 with BOM": the first line still reads as its word, or as a comment.
    let dir = tempfile::tempdir().unwrap();
    let script = parsed("\u{feff}COUNT DOOR\n", dir.path(), None);
    assert!(matches!(
        &script.steps[0].action,
        Action::Command(Command::CountBlock { block_name, .. }) if block_name == "DOOR"
    ));
    assert_eq!(script.steps[0].line, 1);
    let script = parsed("\u{feff}; a comment\nCOUNT DOOR\n", dir.path(), None);
    assert_eq!(script.steps.len(), 1);
    assert_eq!(script.steps[0].line, 2);
}

/// The path a JSON line's command was given after the folder rule.
fn json_path(action: &Action) -> String {
    let Action::Command(cmd) = action else {
        panic!("{action:?}")
    };
    let v = serde_json::to_value(cmd).unwrap();
    v.get("path")
        .or_else(|| v.get("from_path"))
        .and_then(|p| p.as_str())
        .unwrap()
        .to_string()
}

#[test]
fn json_lines_follow_the_folder_rule_for_their_paths() {
    let dir = tempfile::tempdir().unwrap();
    let scripts = dir.path().join("scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    let drawing = write_sample(dir.path(), "dwg");
    let outside = dir.path().parent().unwrap().join("elsewhere.dwg");
    let lines = [
        r#"{"type":"redefine_block","name":"DOOR","from_path":"P"}"#,
        r#"{"type":"insert_block","name":"DOOR","at":[0,0],"from_path":"P"}"#,
        r#"{"type":"translate_layers","from_path":"P","mappings":[]}"#,
        r#"{"type":"attach_image","path":"P","insert_at":[0,0],"width_units":1}"#,
        r#"{"type":"import_pdf","path":"P","page":1,"insert_at":[0,0],"scale":1}"#,
        r#"{"type":"attach_pdf_underlay","path":"P","page":1,"insert_at":[0,0],"scale":1}"#,
        r#"{"type":"write_block","name":"DOOR","path":"P"}"#,
    ];
    let real = scripts.canonicalize().unwrap();
    for line in lines {
        // Relative paths are taken from the script's folder, never from where it runs.
        let script = parsed(&line.replace('P', "door.dwg"), &scripts, Some(&drawing));
        assert_eq!(
            json_path(&script.steps[0].action),
            real.join("door.dwg").to_string_lossy(),
            "{line}"
        );
        // The drawing's folder is allowed, as for the words.
        let script = parsed(&line.replace('P', "../other.dwg"), &scripts, Some(&drawing));
        assert!(json_path(&script.steps[0].action).ends_with("other.dwg"));
        // Out of both folders: refused, relative or absolute.
        for bad in ["../../x.dwg".to_string(), outside.display().to_string()] {
            let (n, message) = error(&line.replace('P', &bad), &scripts, Some(&drawing));
            assert_eq!(n, 1, "{line}");
            assert!(message.contains("outside"), "{line}: {message}");
        }
    }
    // A batch's commands go through it too.
    let batch = |p: &str| {
        format!(
            r#"{{"type":"batch","commands":[{{"type":"insert_block","name":"DOOR","at":[0,0],"from_path":"{p}"}}]}}"#
        )
    };
    let script = parsed(&batch("door.dwg"), &scripts, Some(&drawing));
    let Action::Command(Command::Batch { commands, .. }) = &script.steps[0].action else {
        panic!("{:?}", script.steps[0].action)
    };
    let inner = serde_json::to_value(&commands[0]).unwrap();
    assert_eq!(
        inner["from_path"].as_str().unwrap(),
        real.join("door.dwg").to_string_lossy()
    );
    let (n, message) = error(&batch("../../x.dwg"), &scripts, Some(&drawing));
    assert_eq!(n, 1);
    assert!(message.contains("outside"), "{message}");
    // A command without a path is untouched.
    let script = parsed(
        r#"{"type":"count_block","block_name":"DOOR","layer":null}"#,
        &scripts,
        None,
    );
    assert_eq!(script.steps.len(), 1);
}

#[test]
fn ext_names_the_drawings_extension_and_a_bare_export_uses_both() {
    let dir = tempfile::tempdir().unwrap();
    let drawing = write_sample(dir.path(), "dwg");
    let real = dir.path().canonicalize().unwrap();
    for text in ["EXPORT {name}.{ext}.xlsx", "EXPORT"] {
        let script = parsed(text, dir.path(), Some(&drawing));
        assert!(
            matches!(&script.steps[0].action, Action::Export(p) if p == &real.join("sample.dwg.xlsx")),
            "{text}: {:?}",
            script.steps[0].action
        );
    }
    let (line, message) = error("EXPORT", dir.path(), None);
    assert_eq!(line, 1);
    assert!(message.contains("no drawing"), "{message}");
}

/// A lock beside `drawing` from a process of this computer that has ended: a crash.
fn dead_lock(drawing: &Path) {
    assert!(qiyas_core::take_lock(drawing).unwrap());
    let mut lock = qiyas_core::read_lock(drawing).unwrap();
    let mut child = std::process::Command::new("true").spawn().unwrap();
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

#[test]
fn save_and_saveas_leave_the_apps_autosaves_and_locks_alone() {
    let dir = tempfile::tempdir().unwrap();
    let drawing = write_sample(dir.path(), "dwg");
    let copy = dir.path().join("copy.dxf");
    let mut kept = Vec::new();
    for d in [&drawing, &copy] {
        let sv = qiyas_core::autosave_path(d);
        let mut sv_project = sv.clone().into_os_string();
        sv_project.push(".qiyas");
        std::fs::write(&sv, b"newer work").unwrap();
        std::fs::write(&sv_project, b"{}").unwrap();
        kept.extend([sv, PathBuf::from(sv_project)]);
    }
    dead_lock(&drawing);
    kept.push(qiyas_core::lock_path(&drawing));

    let script = parsed(
        "COUNT DOOR\nSAVE\nSAVEAS copy.dxf\nSAVE\n",
        dir.path(),
        Some(&drawing),
    );
    let session = Session::open(&drawing).unwrap();
    script::run(&script, Some(session), &mut Stub::default()).unwrap_or_else(|e| panic!("{e}"));
    assert!(dir.path().join("sample.dwg.bak").exists(), "SAVE wrote");
    assert!(copy.exists(), "SAVEAS wrote");
    for path in kept {
        assert!(path.exists(), "{} is the app's: left alone", path.display());
    }
}

/// A host that lets SAVE write over a drawing the reader repaired.
struct Allowing(Stub);

impl Host for Allowing {
    fn export_takeoff(&mut self, session: &Session, path: &Path) -> Result<(), String> {
        self.0.export_takeoff(session, path)
    }
    fn import_boq(&mut self, path: &Path) -> Result<Boq, String> {
        self.0.import_boq(path)
    }
    fn compare(&mut self, session: &mut Session) -> Result<usize, String> {
        self.0.compare(session)
    }
    fn allow_repaired_save(&self) -> bool {
        true
    }
}

/// The sample, opened as if the failsafe reader had dropped a record of it.
fn repaired(drawing: &Path) -> Session {
    let mut session = Session::open(drawing).unwrap();
    session.drawing.report.dropped.push(qiyas_core::Dropped {
        kind: "LINE".to_string(),
        count: 1,
    });
    session
}

#[test]
fn save_refuses_a_drawing_the_reader_repaired() {
    let dir = tempfile::tempdir().unwrap();
    let drawing = write_sample(dir.path(), "dwg");
    let before = std::fs::read(&drawing).unwrap();
    let script = parsed("COUNT DOOR\nSAVE\n", dir.path(), Some(&drawing));
    let e = fails(script::run(
        &script,
        Some(repaired(&drawing)),
        &mut Stub::default(),
    ));
    assert_eq!(e.line, 2);
    assert!(e.message.contains("repaired on open"), "{e}");
    assert!(e.message.contains("SAVEAS"), "{e}");
    assert_eq!(
        std::fs::read(&drawing).unwrap(),
        before,
        "the original stays"
    );
    assert!(!dir.path().join("sample.dwg.bak").exists());

    // SAVEAS to a new path is fine, and SAVE over that copy too.
    let script = parsed("SAVEAS copy.dwg\nSAVE\n", dir.path(), Some(&drawing));
    script::run(&script, Some(repaired(&drawing)), &mut Stub::default())
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(dir.path().join("copy.dwg.bak").exists());
    // The host may allow it (`--allow-repaired-save`).
    let script = parsed("SAVE\n", dir.path(), Some(&drawing));
    script::run(
        &script,
        Some(repaired(&drawing)),
        &mut Allowing(Stub::default()),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(dir.path().join("sample.dwg.bak").exists());
}

#[test]
fn a_project_file_that_cannot_be_read_is_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let drawing = write_sample(dir.path(), "dwg");
    std::fs::write(dir.path().join("sample.qiyas"), b"not json {").unwrap();
    let session = Session::open(&drawing).unwrap();
    assert!(
        session.takeoff.items.is_empty(),
        "opens with an empty take-off"
    );
    let script = parsed("COUNT DOOR\nSAVE\n", dir.path(), Some(&drawing));
    let run =
        script::run(&script, Some(session), &mut Stub::default()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(run.warnings.len(), 1, "{:?}", run.warnings);
    assert!(
        run.warnings[0].contains("Project file could not be read"),
        "{:?}",
        run.warnings
    );
    // Not a repaired drawing: SAVE still writes (the broken project was backed up on open).
    assert!(dir.path().join("sample.dwg.bak").exists());

    // An OPEN in the script warns the same way.
    std::fs::write(dir.path().join("sample.qiyas"), b"not json {").unwrap();
    let script = parsed("OPEN sample.dwg\n", dir.path(), None);
    let run = script::run(&script, None, &mut Stub::default()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(run.warnings.len(), 1, "{:?}", run.warnings);
}
