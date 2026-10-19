//! The unit matrix: the same small plan drawn in millimetres, centimetres, metres and inches must
//! look, pick, snap and measure the same. A constant that forgets the drawing's units (the screen
//! tolerance once drew a 1 m circle as a 10-gon in metres) fails here.

use qiyas_core::{
    AutoSettings, Boundary, Command, ConstraintKind, DrawSpec, HatchPattern, Outcome, Session,
    SnapModes, Target,
};

/// INSUNITS and drawing units per metre.
const UNITS: [(i16, f64); 4] = [(4, 1000.0), (5, 100.0), (6, 1.0), (1, 1.0 / 0.0254)];

/// A new drawing in `insunits` holding the plan, in metres scaled by `m` (units per metre): a
/// 4 x 3 m room, a 1 m circle at (10, 0), a 90 degree arc of 2 m at (20, 0) and a bulged
/// polyline. Returns the room's and the circle's handles.
fn plan(dir: &std::path::Path, insunits: i16, m: f64) -> (Session, u64, u64) {
    let mut s = Session::new_blank(&dir.join(format!("plan-{insunits}.dwg")), insunits);
    let mut add = |spec: DrawSpec| match s.run(Command::Draw(spec)).unwrap() {
        Outcome::Added { handles } => handles[0],
        other => panic!("{other:?}"),
    };
    let room = add(DrawSpec::Rectangle {
        a: [0.0, 0.0],
        b: [4.0 * m, 3.0 * m],
    });
    let circle = add(DrawSpec::Circle {
        centre: [10.0 * m, 0.0],
        radius: 1.0 * m,
    });
    add(DrawSpec::Arc {
        centre: [20.0 * m, 0.0],
        radius: 2.0 * m,
        start: 0.0,
        end: 90.0,
    });
    add(DrawSpec::Polyline {
        points: vec![[0.0, -5.0 * m], [3.0 * m, -5.0 * m], [3.0 * m, -2.0 * m]],
        bulges: vec![0.4, 0.0, 0.0],
        closed: false,
    });
    (s, room, circle)
}

fn measured(s: &mut Session, handle: u64) -> f64 {
    match s
        .run(Command::MeasureEntity {
            handle,
            chain: Vec::new(),
        })
        .unwrap()
    {
        // In metres, square metres or pieces, whatever the drawing's units.
        Outcome::Measured { measurement } => measurement.shown,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_same_plan_draws_and_measures_the_same_in_every_unit() {
    let dir = tempfile::tempdir().unwrap();
    let mut seen: Option<(usize, f64, f64)> = None;
    for (insunits, m) in UNITS {
        let (mut s, room, circle) = plan(dir.path(), insunits, m);

        // The screen: the same segments for the same curves, give or take one per curve.
        let segments = s.batch().segments.len() / 4;

        // Measurements in square metres whatever the units (closed shapes measure their area).
        let room_area = measured(&mut s, room);
        let circle_area = measured(&mut s, circle);
        assert!(
            (room_area - 12.0).abs() < 1e-6,
            "INSUNITS {insunits}: {room_area} m²"
        );
        assert!(
            (circle_area - std::f64::consts::PI).abs() < 1e-6,
            "INSUNITS {insunits}: {circle_area} m²"
        );
        if let Some((n, a, c)) = seen {
            assert!(
                segments.abs_diff(n) <= 4,
                "INSUNITS {insunits}: {segments} segments, before {n}"
            );
            assert!((room_area - a).abs() < 1e-9 && (circle_area - c).abs() < 1e-9);
        }
        seen = Some((segments, room_area, circle_area));

        // Picking and snapping at the same screen distance: 5 pixels at 1 cm a pixel.
        let px = 0.01 * m;
        let picked = s
            .pick([4.0 * m + 2.0 * px, 1.5 * m], 5.0 * px)
            .expect("the room's side");
        assert_eq!(picked.top, room, "INSUNITS {insunits}");
        let snap = s
            .snap(
                [4.0 * m + 2.0 * px, 3.0 * m + px],
                5.0 * px,
                &SnapModes::default(),
                None,
            )
            .expect("the room's corner");
        assert!(
            (snap.point[0] - 4.0 * m).abs() < 1e-9 * m
                && (snap.point[1] - 3.0 * m).abs() < 1e-9 * m,
            "INSUNITS {insunits}: {:?}",
            snap.point
        );
        // A window round the circle takes it alone.
        let window = s.pick_window([[8.9 * m, -1.1 * m], [11.1 * m, 1.1 * m]], false);
        assert_eq!(window.len(), 1, "INSUNITS {insunits}");
        assert_eq!(window[0].top, circle);

        // A click inside the room finds it, and the hatch made from it measures 12 m².
        let Boundary::Ring {
            ring,
            holes,
            bulges,
        } = s.boundary_at([2.0 * m, 1.5 * m], 25.0 * px)
        else {
            panic!("INSUNITS {insunits}: no ring");
        };
        let hatch = match s
            .run(Command::Draw(DrawSpec::Hatch {
                boundary_handles: Vec::new(),
                points: ring,
                holes,
                bulges,
                pattern: HatchPattern::Solid,
            }))
            .unwrap()
        {
            Outcome::Added { handles } => handles[0],
            other => panic!("{other:?}"),
        };
        let area = measured(&mut s, hatch);
        assert!((area - 12.0).abs() < 1e-6, "INSUNITS {insunits}: {area} m²");
    }
}

#[test]
fn a_block_from_a_millimetre_library_comes_in_at_its_size() {
    let dir = tempfile::tempdir().unwrap();
    // The library: a 900 mm door leaf as the block DOOR, in a millimetre drawing.
    let mut lib = Session::new_blank(&dir.path().join("lib.dwg"), 4);
    let leaf = match lib
        .run(Command::Draw(DrawSpec::Line {
            a: [0.0, 0.0],
            b: [900.0, 0.0],
        }))
        .unwrap()
    {
        Outcome::Added { handles } => handles[0],
        other => panic!("{other:?}"),
    };
    lib.run(Command::CreateBlock {
        name: "DOOR".into(),
        base: [0.0, 0.0],
        targets: vec![Target {
            handle: leaf,
            chain: Vec::new(),
        }],
        convert_to_insert: false,
    })
    .unwrap();
    let library = dir.path().join("doors.dwg");
    lib.save_as(&library).unwrap();

    for (insunits, m) in UNITS {
        let mut s = Session::new_blank(&dir.path().join(format!("p{insunits}.dwg")), insunits);
        s.run(Command::InsertBlock {
            name: "DOOR".into(),
            at: [0.0, 0.0],
            scale: 1.0,
            rotation: 0.0,
            attribute_values: Default::default(),
            from_path: Some(library.display().to_string()),
        })
        .unwrap();
        let [[x0, _], [x1, _]] = s.batch().bounds;
        let metres = (x1 - x0) / m;
        assert!(
            (metres - 0.9).abs() < 1e-9,
            "INSUNITS {insunits}: the door is {metres} m wide"
        );
    }
}

#[test]
fn autoconstrain_never_welds_ends_five_centimetres_apart_in_metres() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("p.dwg"), 6);
    let mut line =
        |a: [f64; 2], b: [f64; 2]| match s.run(Command::Draw(DrawSpec::Line { a, b })).unwrap() {
            Outcome::Added { handles } => Target {
                handle: handles[0],
                chain: Vec::new(),
            },
            other => panic!("{other:?}"),
        };
    // Two walls whose ends are 4 cm apart: a gap, not a joint.
    let targets = vec![line([0.0, 0.0], [3.0, 0.0]), line([3.04, 0.0], [3.04, 2.0])];
    let outcome = s
        .run(Command::AutoConstrain {
            targets,
            settings: AutoSettings {
                kinds: vec![ConstraintKind::Coincident],
                ..AutoSettings::default()
            },
        })
        .unwrap();
    match outcome {
        Outcome::Constrained { ids } => assert!(ids.is_empty(), "welded: {ids:?}"),
        Outcome::Nothing => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_grid_is_a_metre_in_every_unit() {
    let dir = tempfile::tempdir().unwrap();
    for (insunits, m) in UNITS {
        let s = Session::new_blank(&dir.path().join(format!("g{insunits}.dwg")), insunits);
        assert!(
            (s.drafting.grid_spacing - m).abs() < 1e-9 * m,
            "INSUNITS {insunits}"
        );
    }
}

/// The villa a tester drew in metres: the drawing's style was the codec's stock one (text 0.18,
/// offset 0.625, extension 1.25), so at 1:100 its dimensions were 18 m tall with 125 m
/// extension lines. Opened again, the style takes the metric template's sizes and the dimension
/// Qiyas made is drawn again: text 0.25 m.
#[test]
fn a_stock_dimension_style_is_sized_on_open_and_its_giant_dimensions_redrawn() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("villa.dwg");
    let mut s = Session::new_blank(&dir.path().join("Untitled-1.dwg"), 6);
    // Back to the stock style, as the drawing had it.
    s.run(Command::SetDimStyle {
        name: "Standard".into(),
        fields: qiyas_core::DimStyleFields {
            arrow_size: 0.18,
            text_height: 0.18,
            precision: 2,
            ext_offset: 0.625,
            ext_extension: 1.25,
            gap: 0.625,
            scale: 1.0,
            show_units: true,
        },
        current: true,
    })
    .unwrap();
    s.run(Command::AnnotationScale {
        name: "1:100".into(),
    })
    .unwrap();
    let Outcome::Added { handles } = s
        .run(Command::Dimension(qiyas_core::DimSpec::Linear {
            p1: [10.0, 0.0],
            p2: [24.0, 0.0],
            line_point: [17.0, -0.5],
            rotation: None,
        }))
        .unwrap()
    else {
        panic!("a dimension");
    };
    let giant = s.entity_info(handles[0], &[]).unwrap().height.unwrap();
    assert!(
        (giant - 18.0).abs() < 1e-9,
        "as the tester saw it: {giant} m"
    );
    s.save_as(&path).unwrap();

    let again = Session::open(&path).unwrap();
    let height = again.entity_info(handles[0], &[]).unwrap().height.unwrap();
    assert!(
        (height - 0.25).abs() < 1e-9,
        "after opening again: {height} m"
    );
    let [[_, y0], [_, y1]] = again.batch().bounds;
    assert!(
        y1 - y0 < 5.0,
        "no extension line runs off the sheet: {} m tall",
        y1 - y0
    );
}

/// At a UTM northing in millimetres (3.3e9) float steps are 5e-7: two walls whose ends meet
/// differ there by a few steps, more than the old fixed 1e-6, and JOIN refused them. The
/// tolerance now grows with the coordinates.
#[test]
fn join_works_at_survey_coordinates() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("site.dwg"), 4);
    let (x, y): (f64, f64) = (3.3e9, 1.1e9);
    let meet = x + 5000.0;
    // The second wall starts three float steps past where the first ends.
    let mut start = meet;
    for _ in 0..3 {
        start = f64::from_bits(start.to_bits() + 1);
    }
    assert!(start - meet > 1e-6, "more than the old tolerance apart");
    let mut line =
        |a: [f64; 2], b: [f64; 2]| match s.run(Command::Draw(DrawSpec::Line { a, b })).unwrap() {
            Outcome::Added { handles } => Target {
                handle: handles[0],
                chain: Vec::new(),
            },
            other => panic!("{other:?}"),
        };
    let first = line([x, y], [meet, y]);
    let second = line([start, y], [start, y + 4000.0]);
    let outcome = s
        .run(Command::Edit(qiyas_core::EditSpec::Join {
            targets: vec![first, second],
        }))
        .unwrap();
    match outcome {
        Outcome::Edited { removed, .. } => assert_eq!(removed.len(), 1, "joined into one"),
        other => panic!("{other:?}"),
    }
}
