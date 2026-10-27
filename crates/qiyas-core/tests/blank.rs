//! A new drawing (`Session::new_blank`): empty, in the units asked for, clean and untitled until
//! its first Save As, which gives it a file and a project file like any other drawing.

use opencadcodec::EntityType;
use qiyas_core::{Command, CoreError, DrawSpec, Outcome, Session, UNTITLED};

const MODEL: &str = "*Model_Space";

#[test]
fn a_new_drawing_is_empty_untitled_and_clean() {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_blank(&dir.path().join("Untitled-1.dwg"), 6);
    assert!(s.is_untitled());
    assert!(!s.is_dirty(), "nothing to save until it changes");
    assert_eq!(s.drawing.doc.entities_in_block(MODEL).count(), 0);
    let info = s.info();
    assert!(info.untitled);
    assert_eq!(info.units, "metres");
    assert!(
        info.problems.is_empty(),
        "no 'no geometry' warning: {:?}",
        info.problems
    );
    assert_eq!(info.sheets[0].block, MODEL);
    assert!(info.layers.iter().any(|l| l.name == "0"));
}

#[test]
fn units_follow_the_header() {
    let dir = tempfile::tempdir().unwrap();
    for (insunits, name) in [(6, "metres"), (5, "centimetres"), (4, "millimetres")] {
        let s = Session::new_blank(&dir.path().join("Untitled-1.dwg"), insunits);
        assert_eq!(s.info().units, name);
        assert_eq!(s.drawing.doc.header.insertion_units, insunits);
    }
}

#[test]
fn save_is_refused_and_no_project_file_is_written_until_save_as() {
    let dir = tempfile::tempdir().unwrap();
    let placeholder = dir.path().join("Untitled-1.dwg");
    let mut s = Session::new_blank(&placeholder, 6);
    match s.save().unwrap_err() {
        CoreError::InvalidCommand(m) => assert_eq!(m, UNTITLED),
        other => panic!("{other:?}"),
    }
    s.save_project().unwrap();
    assert!(!placeholder.exists());
    assert!(!placeholder.with_extension("qiyas").exists());
}

#[test]
fn a_drawn_line_survives_save_as_and_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("Untitled-1.dwg"), 6);
    let added = s
        .run(Command::Draw(DrawSpec::Line {
            a: [0.0, 0.0],
            b: [5.0, 0.0],
        }))
        .unwrap();
    assert!(matches!(added, Outcome::Added { ref handles } if handles.len() == 1));
    assert!(s.is_dirty());

    let target = dir.path().join("plan.dwg");
    s.save_as(&target).unwrap();
    assert!(!s.is_untitled());
    assert!(!s.is_dirty());
    assert!(target.with_extension("qiyas").exists());
    // Saved under its name now: plain Save works.
    s.save().unwrap();

    let again = Session::open(&target).unwrap();
    assert!(!again.is_untitled());
    assert_eq!(again.info().units, "metres");
    let lines: Vec<_> = again
        .drawing
        .doc
        .entities_in_block(MODEL)
        .filter(|e| matches!(e, EntityType::Line(_)))
        .collect();
    assert_eq!(lines.len(), 1);
}

/// Note 68 (26 October 2026): the founder saved an untitled drawing and thought no `.qiyas` file
/// was written. It is: `<name>.qiyas`, the drawing's stem (not `<name>.dwg.qiyas`). The take-off
/// measured in the untitled drawing must be in it and come back on reopening.
#[test]
fn an_untitled_drawings_take_off_is_written_beside_its_first_save_and_comes_back() {
    let dir = tempfile::tempdir().unwrap();
    let placeholder = dir.path().join("config").join("Untitled-5.dwg");
    std::fs::create_dir_all(placeholder.parent().unwrap()).unwrap();
    let mut s = Session::new_blank(&placeholder, 6);
    s.run(Command::Draw(DrawSpec::Line {
        a: [0.0, 0.0],
        b: [3.0, 4.0],
    }))
    .unwrap();
    s.run(Command::MeasureDistance {
        points: vec![[0.0, 0.0], [3.0, 4.0]].into(),
    })
    .unwrap();
    assert_eq!(s.takeoff.items.len(), 1);

    let target = dir.path().join("Cairo villa.dwg");
    s.save_as(&target).unwrap();
    let sidecar = dir.path().join("Cairo villa.qiyas");
    assert!(sidecar.exists(), "the project file sits beside the drawing");
    assert!(
        !placeholder.with_extension("qiyas").exists(),
        "nothing is left beside the placeholder"
    );
    assert!(!placeholder.exists());

    let again = Session::open(&target).unwrap();
    assert_eq!(again.takeoff.items.len(), 1);
    assert!((again.takeoff.items[0].value - 5.0).abs() < 1e-9);
}

/// The segments the screen draws for a circle of `radius_m` metres in a new drawing of INSUNITS
/// `insunits` (`per_metre` drawing units a metre).
fn circle_segments(insunits: i16, per_metre: f64, radius_m: f64) -> usize {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("Untitled-1.dwg"), insunits);
    s.run(Command::Draw(DrawSpec::Circle {
        centre: [0.0, 0.0],
        radius: radius_m * per_metre,
    }))
    .unwrap();
    s.batch().segments.len() / 4
}

/// A metre drawing's circle was a 10-gon (the screen's tolerance was 0.05 drawing units, 5 cm):
/// curves are drawn within 0.05 mm whatever the units, so the same circle gets the same
/// segments in every unit.
#[test]
fn a_circle_is_as_round_in_metres_as_in_millimetres() {
    let mm = circle_segments(4, 1000.0, 1.0);
    assert!(mm >= 200, "a 1 m circle in millimetres: {mm} segments");
    for (insunits, per_metre) in [(6, 1.0), (5, 100.0), (1, 1.0 / 0.0254)] {
        let n = circle_segments(insunits, per_metre, 1.0);
        assert!(
            n.abs_diff(mm) <= 1,
            "INSUNITS {insunits}: {n} segments, millimetres {mm}"
        );
    }
    // Small circles too: a 10 cm circle in metres.
    assert!(circle_segments(6, 1.0, 0.1) >= 60);
}

/// Zoomed in close on a small circle, the sheet on screen draws it again within a quarter pixel
/// (AutoCAD's regen); zoomed out again, back to the drawing's own tolerance.
#[test]
fn zooming_in_close_draws_curves_finer_and_zooming_out_lets_them_go() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("Untitled-1.dwg"), 4);
    s.run(Command::Draw(DrawSpec::Circle {
        centre: [0.0, 0.0],
        radius: 6.0,
    }))
    .unwrap();
    let base = s.batch().segments.len() / 4;
    let version = s.batch_version();
    assert_eq!(s.screen_tol(), s.batch_tol());

    // A 12 mm bar 4000 px across: 333 px a millimetre.
    let close = 4000.0 / 12.0;
    let bytes = s.batch_bytes_if(None, Some(close)).unwrap();
    assert!(!bytes.is_empty());
    assert!(s.screen_tol() < s.batch_tol());
    assert_ne!(s.batch_version(), version, "finer curves are another batch");
    let fine = s.batch().segments.len() / 4;
    assert!(
        fine > 2 * base,
        "{fine} segments close up, {base} at the drawing's tolerance"
    );
    // Every chord within a quarter pixel.
    let sagitta = 6.0 * (1.0 - (std::f64::consts::PI / fine as f64).cos());
    assert!(
        sagitta * close <= 0.25 + 1e-9,
        "{sagitta} mm is {} px",
        sagitta * close
    );

    // Zooming a little more needs nothing new.
    let again = s.batch_version();
    s.batch_bytes_if(None, Some(close * 1.5));
    assert_eq!(s.batch_version(), again);

    // Back out: the drawing's own tolerance again.
    s.batch_bytes_if(None, Some(1.0));
    assert_eq!(s.screen_tol(), s.batch_tol());
    assert_eq!(s.batch().segments.len() / 4, base);
    assert_eq!(s.batch_version(), version);
}

#[test]
fn zoom_tol_asks_only_when_far_off() {
    use qiyas_core::zoom_tol;
    // At the base and a normal zoom: nothing.
    assert_eq!(zoom_tol(0.05, 0.05, 1.0), None);
    // Far in: a quarter pixel.
    assert_eq!(zoom_tol(0.05, 0.05, 100.0), Some(0.0025));
    // Already fine enough, a bit further in: nothing.
    assert_eq!(zoom_tol(0.05, 0.0025, 150.0), None);
    // Back out: the base.
    assert_eq!(zoom_tol(0.05, 0.0025, 1.0), Some(0.05));
    assert_eq!(zoom_tol(0.05, 0.05, f64::NAN), None);
}

/// A new drawing's dimension style and scale are the metric template's, in its units: a
/// dimension drawn at once has text 2.5 mm on paper at 1:100, 0.25 m in metres, 250 mm in
/// millimetres, never the codec's stock 0.18 (18 m at 1:100 in metres).
#[test]
fn a_dimension_in_a_new_drawing_is_readable_in_every_unit() {
    for (insunits, per_metre) in [(6, 1.0), (5, 100.0), (4, 1000.0)] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("Untitled-1.dwg"), insunits);
        assert_eq!(s.annotation_scale().0, "1:100");
        let outcome = s
            .run(Command::Dimension(qiyas_core::DimSpec::Linear {
                p1: [0.0, 0.0],
                p2: [5.0 * per_metre, 0.0],
                line_point: [2.5 * per_metre, 1.0 * per_metre],
                rotation: None,
            }))
            .unwrap();
        let Outcome::Added { handles } = outcome else {
            panic!("a dimension: {outcome:?}");
        };
        let height = s.entity_info(handles[0], &[]).unwrap().height.unwrap();
        let metres = height / per_metre;
        assert!(
            (metres - 0.25).abs() < 1e-9,
            "INSUNITS {insunits}: text {metres} m"
        );
    }
}
