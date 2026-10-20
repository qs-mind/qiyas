//! Draw commands: each `DrawSpec` adds the right entity with the right fields on the current
//! layer, as one undo step; the current drawing state; the hatch boundary found from a point.

use std::f64::consts::{FRAC_PI_2, PI};

use opencadcodec::entities::{BoundaryEdge, HatchPatternType, Insert};
use opencadcodec::{
    BlockRecord, CadDocument, Circle, Color, EntityType, Handle, Layer, LineWeight, LwPolyline,
    Vector2, Vector3,
};
use qiyas_core::fixtures::{write_drawing, write_sample};
use qiyas_core::{Boundary, Command, CoreError, Current, DrawSpec, HatchPattern, Outcome, Session};

const MODEL: &str = "*Model_Space";

fn sample_session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let session = Session::open(&write_sample(dir.path(), "dxf")).unwrap();
    (dir, session)
}

/// Runs the draw and returns the new handles.
fn draw(s: &mut Session, spec: DrawSpec) -> Vec<u64> {
    match s.run(Command::Draw(spec)).unwrap() {
        Outcome::Added { handles } => handles,
        other => panic!("a draw adds entities, got {other:?}"),
    }
}

/// Runs a draw that must be refused, and checks nothing changed.
fn refused(s: &mut Session, spec: DrawSpec) -> String {
    let before = s.drawing.doc.entities_in_block(MODEL).count();
    let undo = s.can_undo();
    let err = s.run(Command::Draw(spec)).unwrap_err();
    assert_eq!(s.drawing.doc.entities_in_block(MODEL).count(), before);
    assert_eq!(s.can_undo(), undo, "a refused draw is no step");
    match err {
        CoreError::InvalidCommand(m) => m,
        other => panic!("{other:?}"),
    }
}

/// The one entity a draw added.
fn one(s: &mut Session, spec: DrawSpec) -> EntityType {
    let handles = draw(s, spec);
    assert_eq!(handles.len(), 1, "{handles:?}");
    s.drawing
        .doc
        .get_entity(Handle::new(handles[0]))
        .cloned()
        .unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    close(a[0], b[0]) && close(a[1], b[1])
}

#[test]
fn a_line_is_drawn_on_the_current_layer_by_layer_and_undo_removes_it() {
    let (_dir, mut s) = sample_session();
    let drawn = s.batch().ranges.len();
    let v0 = s.batch_version();
    let handles = draw(
        &mut s,
        DrawSpec::Line {
            a: [1.0, 2.0],
            b: [4.0, 6.0],
        },
    );
    let h = handles[0];
    let EntityType::Line(line) = s.drawing.doc.get_entity(Handle::new(h)).unwrap() else {
        panic!("a line");
    };
    assert_eq!([line.start.x, line.start.y], [1.0, 2.0]);
    assert_eq!([line.end.x, line.end.y], [4.0, 6.0]);
    assert_eq!(line.common.layer, "0");
    assert_eq!(line.common.color, Color::ByLayer);
    assert_eq!(line.common.linetype, "");
    assert_eq!(line.common.line_weight, LineWeight::ByLayer);
    assert!(s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .any(|e| e.common().handle.value() == h));
    assert!(s.batch().ranges.iter().any(|r| r.entity == h));
    assert_ne!(s.batch_version(), v0);
    assert!(s.is_dirty());

    assert!(s.undo());
    assert!(s.drawing.doc.get_entity(Handle::new(h)).is_none());
    assert_eq!(s.batch().ranges.len(), drawn);
    assert!(s.redo());
    assert!(s.drawing.doc.get_entity(Handle::new(h)).is_some());
}

#[test]
fn the_current_state_styles_new_entities_is_saved_and_is_no_undo_step() {
    let (dir, mut s) = sample_session();
    assert_eq!(s.current, Current::default());
    assert_eq!(s.info().current.layer, "0");
    let current = Current {
        layer: "a-wall".to_string(),
        color: "ACI 3".to_string(),
        linetype: "Continuous".to_string(),
        lineweight: 35,
    };
    s.run(Command::SetCurrent { current }).unwrap();
    assert!(!s.can_undo(), "UI state, not an undo step");
    assert!(!s.is_dirty());
    // Kept as the drawing spells the layer.
    assert_eq!(s.current.layer, "A-WALL");
    let EntityType::Circle(c) = one(
        &mut s,
        DrawSpec::Circle {
            centre: [0.0, 0.0],
            radius: 1.0,
        },
    ) else {
        panic!("a circle");
    };
    assert_eq!(c.common.layer, "A-WALL");
    assert_eq!(c.common.color, Color::Index(3));
    assert_eq!(c.common.linetype, "Continuous");
    assert_eq!(c.common.line_weight, LineWeight::Value(35));

    // Saved in the project file, read back on the next open.
    s.save_project().unwrap();
    let again = Session::open(&dir.path().join("sample.dxf")).unwrap();
    assert_eq!(again.current.layer, "A-WALL");
    assert_eq!(again.current.color, "ACI 3");

    // Refused: a layer the drawing lacks, a colour, a linetype or a weight that isn't one.
    for bad in [
        Current {
            layer: "NOPE".into(),
            ..Current::default()
        },
        Current {
            color: "red".into(),
            ..Current::default()
        },
        Current {
            linetype: "DASHDOTX".into(),
            ..Current::default()
        },
        Current {
            lineweight: 999,
            ..Current::default()
        },
    ] {
        let err = s.run(Command::SetCurrent { current: bad }).unwrap_err();
        assert!(matches!(err, CoreError::InvalidCommand(_)), "{err:?}");
    }
    assert_eq!(s.current.layer, "A-WALL");
    // A true colour too.
    s.run(Command::SetCurrent {
        current: Current {
            color: "#ff8000".into(),
            ..Current::default()
        },
    })
    .unwrap();
    let EntityType::Point(p) = one(&mut s, DrawSpec::Point { p: [1.0, 1.0] }) else {
        panic!("a point");
    };
    assert_eq!(
        p.common.color,
        Color::Rgb {
            r: 255,
            g: 128,
            b: 0
        }
    );
    assert_eq!(p.common.layer, "0");
}

#[test]
fn a_project_whose_current_layer_is_gone_falls_back_to_layer_0() {
    let (dir, mut s) = sample_session();
    s.run(Command::SetCurrent {
        current: Current {
            layer: "A-DOOR".into(),
            ..Current::default()
        },
    })
    .unwrap();
    s.save_project().unwrap();
    let project = dir.path().join("sample.qiyas");
    let text = std::fs::read_to_string(&project).unwrap();
    std::fs::write(&project, text.replace("\"A-DOOR\"", "\"GONE\"")).unwrap();
    let again = Session::open(&dir.path().join("sample.dxf")).unwrap();
    assert_eq!(again.current.layer, "0");
    // An older project file without the field loads too.
    let text = std::fs::read_to_string(&project).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json.as_object_mut().unwrap().remove("current");
    std::fs::write(&project, json.to_string()).unwrap();
    let old = Session::open(&dir.path().join("sample.dxf")).unwrap();
    assert_eq!(old.current, Current::default());
}

#[test]
fn polylines_keep_their_bulges_and_closing_and_bad_ones_are_refused() {
    let (_dir, mut s) = sample_session();
    let EntityType::LwPolyline(p) = one(
        &mut s,
        DrawSpec::Polyline {
            points: vec![[0.0, 0.0], [2.0, 0.0], [2.0, 2.0]].into(),
            bulges: vec![0.0, 1.0, 0.0],
            closed: true,
        },
    ) else {
        panic!("a polyline");
    };
    assert!(p.is_closed);
    let v: Vec<_> = p
        .vertices
        .iter()
        .map(|v| (v.location.x, v.location.y, v.bulge))
        .collect();
    assert_eq!(v, [(0.0, 0.0, 0.0), (2.0, 0.0, 1.0), (2.0, 2.0, 0.0)]);
    // Bulges left out are straight.
    let EntityType::LwPolyline(p) = one(
        &mut s,
        DrawSpec::Polyline {
            points: vec![[0.0, 0.0], [1.0, 1.0]].into(),
            bulges: Vec::new(),
            closed: false,
        },
    ) else {
        panic!("a polyline");
    };
    assert!(!p.is_closed && p.vertices.iter().all(|v| v.bulge == 0.0));

    refused(
        &mut s,
        DrawSpec::Polyline {
            points: vec![[0.0, 0.0]].into(),
            bulges: Vec::new(),
            closed: false,
        },
    );
    refused(
        &mut s,
        DrawSpec::Polyline {
            points: vec![[0.0, 0.0], [1.0, 0.0]].into(),
            bulges: vec![0.5],
            closed: false,
        },
    );
    refused(
        &mut s,
        DrawSpec::Polyline {
            points: vec![[0.0, 0.0], [f64::NAN, 0.0]].into(),
            bulges: Vec::new(),
            closed: false,
        },
    );
}

#[test]
fn rectangles_and_polygons_are_closed_polylines() {
    let (_dir, mut s) = sample_session();
    let EntityType::LwPolyline(r) = one(
        &mut s,
        DrawSpec::Rectangle {
            a: [5.0, 4.0],
            b: [1.0, 1.0],
        },
    ) else {
        panic!("a polyline");
    };
    assert!(r.is_closed);
    let corners: Vec<_> = r
        .vertices
        .iter()
        .map(|v| [v.location.x, v.location.y])
        .collect();
    assert_eq!(corners, [[5.0, 4.0], [1.0, 4.0], [1.0, 1.0], [5.0, 1.0]]);
    refused(
        &mut s,
        DrawSpec::Rectangle {
            a: [1.0, 1.0],
            b: [1.0, 5.0],
        },
    );

    // Inscribed: the corners on the circle; circumscribed: the sides touch it. A flat bottom edge.
    for (inscribed, corner_radius) in [(true, 2.0), (false, 2.0 / (PI / 6.0).cos())] {
        let EntityType::LwPolyline(p) = one(
            &mut s,
            DrawSpec::Polygon {
                centre: [10.0, 10.0],
                sides: 6,
                radius: 2.0,
                inscribed,
            },
        ) else {
            panic!("a polyline");
        };
        assert!(p.is_closed);
        assert_eq!(p.vertices.len(), 6);
        for v in &p.vertices {
            let d = (v.location.x - 10.0).hypot(v.location.y - 10.0);
            assert!(close(d, corner_radius), "{d} {corner_radius}");
        }
        assert!(close(p.vertices[0].location.y, p.vertices[1].location.y));
        assert!(p.vertices[0].location.y < 10.0);
    }
    refused(
        &mut s,
        DrawSpec::Polygon {
            centre: [0.0, 0.0],
            sides: 2,
            radius: 1.0,
            inscribed: true,
        },
    );
}

#[test]
fn circles_arcs_and_ellipses_get_their_centres_radii_and_angles() {
    let (_dir, mut s) = sample_session();
    let EntityType::Circle(c) = one(
        &mut s,
        DrawSpec::Circle {
            centre: [3.0, 4.0],
            radius: 2.5,
        },
    ) else {
        panic!("a circle");
    };
    assert_eq!([c.center.x, c.center.y, c.radius], [3.0, 4.0, 2.5]);
    refused(
        &mut s,
        DrawSpec::Circle {
            centre: [0.0, 0.0],
            radius: 0.0,
        },
    );

    // Angles are degrees on the wire, radians in the drawing.
    let EntityType::Arc(a) = one(
        &mut s,
        DrawSpec::Arc {
            centre: [1.0, 1.0],
            radius: 2.0,
            start: 90.0,
            end: 180.0,
        },
    ) else {
        panic!("an arc");
    };
    assert!(close(a.start_angle, FRAC_PI_2) && close(a.end_angle, PI));
    assert_eq!(a.radius, 2.0);

    // Through three points, counter-clockwise from the first to the last through the second.
    let EntityType::Arc(a) = one(
        &mut s,
        DrawSpec::ThreePointArc {
            a: [2.0, 0.0],
            b: [0.0, 2.0],
            c: [-2.0, 0.0],
        },
    ) else {
        panic!("an arc");
    };
    assert!(near([a.center.x, a.center.y], [0.0, 0.0]) && close(a.radius, 2.0));
    assert!(close(a.start_angle, 0.0) && close(a.end_angle, PI));
    // Clockwise through the points: stored counter-clockwise from the last to the first.
    let EntityType::Arc(a) = one(
        &mut s,
        DrawSpec::ThreePointArc {
            a: [-2.0, 0.0],
            b: [0.0, 2.0],
            c: [2.0, 0.0],
        },
    ) else {
        panic!("an arc");
    };
    assert!(close(a.start_angle, 0.0) && close(a.end_angle, PI));
    refused(
        &mut s,
        DrawSpec::ThreePointArc {
            a: [0.0, 0.0],
            b: [1.0, 1.0],
            c: [2.0, 2.0],
        },
    );

    let EntityType::Ellipse(e) = one(
        &mut s,
        DrawSpec::Ellipse {
            centre: [5.0, 5.0],
            major: [3.0, 0.0],
            ratio: 0.5,
        },
    ) else {
        panic!("an ellipse");
    };
    assert_eq!([e.center.x, e.center.y], [5.0, 5.0]);
    assert_eq!([e.major_axis.x, e.major_axis.y], [3.0, 0.0]);
    assert_eq!(e.minor_axis_ratio, 0.5);
    assert!(e.is_full());
    // A ratio above 1 is refused (the major axis is the longer one).
    refused(
        &mut s,
        DrawSpec::Ellipse {
            centre: [0.0, 0.0],
            major: [1.0, 0.0],
            ratio: 1.5,
        },
    );
}

#[test]
fn splines_points_and_donuts() {
    let (_dir, mut s) = sample_session();
    let fit = vec![[0.0, 0.0], [1.0, 1.0], [2.0, 0.0], [3.0, 1.0]];
    let EntityType::Spline(sp) = one(
        &mut s,
        DrawSpec::Spline {
            fit_points: fit.clone().into(),
        },
    ) else {
        panic!("a spline");
    };
    let stored: Vec<_> = sp.fit_points.iter().map(|p| [p.x, p.y]).collect();
    assert_eq!(stored, fit);
    // Its control points and knots too, so AutoCAD reads it as a cubic through the points.
    assert_eq!(sp.degree, 3);
    assert!(sp.control_points.len() >= 4);
    assert_eq!(sp.knots.len(), sp.control_points.len() + 4);
    assert!(near(
        [sp.control_points[0].x, sp.control_points[0].y],
        [0.0, 0.0]
    ));
    refused(
        &mut s,
        DrawSpec::Spline {
            fit_points: vec![[0.0, 0.0]].into(),
        },
    );

    let EntityType::Point(p) = one(&mut s, DrawSpec::Point { p: [7.0, 8.0] }) else {
        panic!("a point");
    };
    assert_eq!([p.location.x, p.location.y], [7.0, 8.0]);
    assert_eq!(p.common.layer, "0", "on the current layer, not QIYAS-POINT");

    // Inner and outer diameters, as AutoCAD asks: a closed two-arc polyline with a width.
    let EntityType::LwPolyline(d) = one(
        &mut s,
        DrawSpec::Donut {
            centre: [0.0, 0.0],
            inner: 2.0,
            outer: 4.0,
        },
    ) else {
        panic!("a polyline");
    };
    assert!(d.is_closed);
    assert_eq!(d.vertices.len(), 2);
    assert!(d.vertices.iter().all(|v| v.bulge == 1.0));
    assert!(close(d.constant_width, 1.0));
    let r = d.vertices[0].location.x.hypot(d.vertices[0].location.y);
    assert!(close(r, 1.5));
    refused(
        &mut s,
        DrawSpec::Donut {
            centre: [0.0, 0.0],
            inner: 4.0,
            outer: 2.0,
        },
    );
}

#[test]
fn rays_and_construction_lines_keep_a_unit_direction() {
    let (_dir, mut s) = sample_session();
    let EntityType::Ray(r) = one(
        &mut s,
        DrawSpec::Ray {
            origin: [1.0, 1.0],
            dir: [3.0, 4.0],
        },
    ) else {
        panic!("a ray");
    };
    assert_eq!([r.base_point.x, r.base_point.y], [1.0, 1.0]);
    assert!(near([r.direction.x, r.direction.y], [0.6, 0.8]));
    let EntityType::XLine(x) = one(
        &mut s,
        DrawSpec::XLine {
            origin: [0.0, 5.0],
            dir: [0.0, -2.0],
        },
    ) else {
        panic!("an xline");
    };
    assert!(near([x.direction.x, x.direction.y], [0.0, -1.0]));
    refused(
        &mut s,
        DrawSpec::Ray {
            origin: [0.0, 0.0],
            dir: [0.0, 0.0],
        },
    );
}

#[test]
fn a_revision_cloud_is_a_closed_polyline_of_outward_arcs() {
    let (_dir, mut s) = sample_session();
    // Clockwise on purpose: the arcs still bulge outward.
    let square = vec![[0.0, 0.0], [0.0, 4.0], [4.0, 4.0], [4.0, 0.0]];
    let EntityType::LwPolyline(c) = one(
        &mut s,
        DrawSpec::RevCloud {
            points: square.into(),
            arc_len: Some(1.0),
        },
    ) else {
        panic!("a polyline");
    };
    assert!(c.is_closed);
    assert_eq!(c.vertices.len(), 16, "four arcs a side");
    assert!(c.vertices.iter().all(|v| v.bulge > 0.0));
    // Counter-clockwise, so a positive bulge is outward.
    let ring: Vec<[f64; 2]> = c
        .vertices
        .iter()
        .map(|v| [v.location.x, v.location.y])
        .collect();
    assert!(signed_area(&ring) > 0.0);
    refused(
        &mut s,
        DrawSpec::RevCloud {
            points: vec![[0.0, 0.0], [1.0, 0.0]].into(),
            arc_len: Some(1.0),
        },
    );
    refused(
        &mut s,
        DrawSpec::RevCloud {
            points: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]].into(),
            arc_len: Some(0.0),
        },
    );
}

/// The ring found, with no islands.
fn ring_of(found: Boundary) -> Vec<[f64; 2]> {
    match found {
        Boundary::Ring { ring, holes, .. } if holes.is_empty() => ring,
        other => panic!("a ring without islands, got {other:?}"),
    }
}

fn signed_area(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        / 2.0
}

#[test]
fn a_wipeout_covers_its_polygon() {
    let (_dir, mut s) = sample_session();
    let points = vec![[1.0, 1.0], [5.0, 1.0], [3.0, 4.0]];
    let EntityType::Wipeout(w) = one(
        &mut s,
        DrawSpec::Wipeout {
            points: points.clone().into(),
        },
    ) else {
        panic!("a wipeout");
    };
    let world: Vec<[f64; 2]> = w
        .world_boundary_vertices()
        .iter()
        .map(|v| [v.x, v.y])
        .collect();
    // Closed as AutoCAD keeps a polygonal boundary: the first point again at the end.
    assert_eq!(world.len(), 4);
    for (got, want) in world.iter().zip(points.iter().chain([&points[0]])) {
        assert!(near(*got, *want), "{got:?} {want:?}");
    }
    assert!(s.drawing.doc.classes.contains("WIPEOUT"));
    assert!(s.undo());
    assert!(s.drawing.doc.get_entity(w.common.handle).is_none());
    refused(
        &mut s,
        DrawSpec::Wipeout {
            points: vec![[0.0, 0.0], [1.0, 0.0]].into(),
        },
    );
}

#[test]
fn hatches_from_points_or_closed_entities_with_solid_or_pattern_fills() {
    let (_dir, mut s) = sample_session();
    let ring = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]];
    let EntityType::Hatch(h) = one(
        &mut s,
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: ring.clone().into(),
            holes: Vec::new(),
            bulges: Vec::new(),
            pattern: HatchPattern::Solid,
        },
    ) else {
        panic!("a hatch");
    };
    assert!(h.is_solid);
    assert_eq!(h.pattern.name, "SOLID");
    assert_eq!(h.paths.len(), 1);
    let BoundaryEdge::Polyline(edge) = &h.paths[0].edges[0] else {
        panic!("a polyline edge");
    };
    assert!(edge.is_closed);
    let stored: Vec<_> = edge.vertices.iter().map(|v| [v.x, v.y]).collect();
    assert_eq!(stored, ring);

    // ANSI31: 45 degree lines 3.175 apart, stored rotated and scaled as DXF keeps them.
    let EntityType::Hatch(h) = one(
        &mut s,
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: ring.clone().into(),
            holes: Vec::new(),
            bulges: Vec::new(),
            pattern: HatchPattern::Ansi31,
        },
    ) else {
        panic!("a hatch");
    };
    assert!(!h.is_solid);
    assert_eq!(h.pattern.name, "ANSI31");
    assert_eq!(h.pattern_type, HatchPatternType::Predefined);
    assert_eq!(h.pattern.lines.len(), 1);
    let line = &h.pattern.lines[0];
    assert!(close(line.angle, PI / 4.0));
    let spacing = 3.175 * (PI / 4.0).sin();
    assert!(near([line.offset.x, line.offset.y], [-spacing, spacing]));

    let EntityType::Hatch(h) = one(
        &mut s,
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: ring.clone().into(),
            holes: Vec::new(),
            bulges: Vec::new(),
            pattern: HatchPattern::Ansi37,
        },
    ) else {
        panic!("a hatch");
    };
    assert_eq!(h.pattern.lines.len(), 2);

    // A custom pattern from the built-in table, turned and scaled.
    let EntityType::Hatch(h) = one(
        &mut s,
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: ring.clone().into(),
            holes: Vec::new(),
            bulges: Vec::new(),
            pattern: HatchPattern::Custom {
                name: "line".into(),
                angle: 90.0,
                scale: 10.0,
            },
        },
    ) else {
        panic!("a hatch");
    };
    assert_eq!(h.pattern.name, "LINE");
    assert!(close(h.pattern_angle, FRAC_PI_2) && close(h.pattern_scale, 10.0));
    let line = &h.pattern.lines[0];
    assert!(close(line.angle, FRAC_PI_2));
    assert!(near([line.offset.x, line.offset.y], [-31.75, 0.0]));
    refused(
        &mut s,
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: ring.clone().into(),
            holes: Vec::new(),
            bulges: Vec::new(),
            pattern: HatchPattern::Custom {
                name: "NO-SUCH".into(),
                angle: 0.0,
                scale: 1.0,
            },
        },
    );

    // From the sample's closed rectangle: its vertices make the boundary.
    let rectangle = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::LwPolyline(p) if p.is_closed))
        .unwrap()
        .common()
        .handle
        .value();
    let EntityType::Hatch(h) = one(
        &mut s,
        DrawSpec::Hatch {
            boundary_handles: vec![rectangle],
            points: Vec::new().into(),
            holes: Vec::new(),
            bulges: Vec::new(),
            pattern: HatchPattern::Solid,
        },
    ) else {
        panic!("a hatch");
    };
    assert_eq!(h.paths.len(), 1);
    let BoundaryEdge::Polyline(edge) = &h.paths[0].edges[0] else {
        panic!("a polyline edge");
    };
    assert_eq!(edge.vertices.len(), 4);

    // An open entity can't bound a hatch; neither can nothing, or both kinds at once.
    let line = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Line(_)))
        .unwrap()
        .common()
        .handle
        .value();
    for (handles, points) in [
        (vec![line], Vec::new()),
        (Vec::new(), Vec::new()),
        (vec![rectangle], ring.clone()),
        (Vec::new(), ring[..2].to_vec()),
    ] {
        refused(
            &mut s,
            DrawSpec::Hatch {
                boundary_handles: handles,
                points: points.into(),
                holes: Vec::new(),
                bulges: Vec::new(),
                pattern: HatchPattern::Solid,
            },
        );
    }
}

#[test]
fn the_hatch_boundary_inside_the_sample_rectangle_is_its_ring() {
    let (_dir, s) = sample_session();
    let ring = ring_of(s.boundary_at([2.0, 2.0], 50.0));
    assert!(close(signed_area(&ring).abs(), 12.0), "{ring:?}");
    for p in &ring {
        assert!(p[0] > -1e-6 && p[0] < 4.0 + 1e-6 && p[1] > -1e-6 && p[1] < 3.0 + 1e-6);
    }
    // Outside any closed shape there is none.
    assert_eq!(s.boundary_at([30.0, 30.0], 50.0), Boundary::None);
    // A hatch drawn from it fills the room.
    let mut s = s;
    let EntityType::Hatch(h) = one(
        &mut s,
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: ring.into(),
            holes: Vec::new(),
            bulges: Vec::new(),
            pattern: HatchPattern::Solid,
        },
    ) else {
        panic!("a hatch");
    };
    assert_eq!(h.paths.len(), 1);
}

#[test]
fn solids_traces_multilines_and_sketches() {
    let (_dir, mut s) = sample_session();
    let EntityType::Solid(sd) = one(
        &mut s,
        DrawSpec::Solid2D {
            points: vec![[0.0, 0.0], [2.0, 0.0], [0.0, 1.0], [2.0, 1.0]].into(),
        },
    ) else {
        panic!("a solid");
    };
    assert!(!sd.is_trace);
    let corners: Vec<_> = sd.corners().iter().map(|v| [v.x, v.y]).collect();
    assert_eq!(corners, [[0.0, 0.0], [2.0, 0.0], [0.0, 1.0], [2.0, 1.0]]);
    let EntityType::Solid(tri) = one(
        &mut s,
        DrawSpec::Solid2D {
            points: vec![[0.0, 0.0], [2.0, 0.0], [1.0, 1.0]].into(),
        },
    ) else {
        panic!("a solid");
    };
    assert!(tri.is_triangle());
    refused(
        &mut s,
        DrawSpec::Solid2D {
            points: vec![[0.0, 0.0], [2.0, 0.0]].into(),
        },
    );

    // One trace quad per segment, `width` wide about it.
    let handles = draw(
        &mut s,
        DrawSpec::Trace {
            points: vec![[0.0, 0.0], [4.0, 0.0], [4.0, 3.0]].into(),
            width: 0.5,
        },
    );
    assert_eq!(handles.len(), 2);
    let EntityType::Solid(t) = s.drawing.doc.get_entity(Handle::new(handles[0])).unwrap() else {
        panic!("a trace");
    };
    assert!(t.is_trace);
    let corners: Vec<_> = t.corners().iter().map(|v| [v.x, v.y]).collect();
    assert_eq!(
        corners,
        [[0.0, 0.25], [0.0, -0.25], [4.0, 0.25], [4.0, -0.25]]
    );
    assert!(s.undo(), "both traces are one step");
    assert!(handles
        .iter()
        .all(|&h| s.drawing.doc.get_entity(Handle::new(h)).is_none()));

    let EntityType::MLine(m) = one(
        &mut s,
        DrawSpec::MLine {
            points: vec![[0.0, 0.0], [5.0, 0.0], [5.0, 5.0]].into(),
            style: None,
            scale: None,
        },
    ) else {
        panic!("a multiline");
    };
    assert_eq!(m.vertices.len(), 3);
    assert_eq!(m.style_name.to_uppercase(), "STANDARD");
    assert!(m.style_handle.is_some());
    assert_eq!(m.style_element_count, 2);
    assert!(m.vertices.iter().all(|v| v.segments.len() == 2));
    refused(
        &mut s,
        DrawSpec::MLine {
            points: vec![[0.0, 0.0], [5.0, 0.0]].into(),
            style: Some("NO-SUCH".into()),
            scale: None,
        },
    );

    let EntityType::LwPolyline(k) = one(
        &mut s,
        DrawSpec::Sketch {
            points: vec![[0.0, 0.0], [0.1, 0.1], [0.3, 0.1]].into(),
        },
    ) else {
        panic!("a polyline");
    };
    assert!(!k.is_closed);
    assert_eq!(k.vertices.len(), 3);
}

/// One of every draw, on a fresh sample.
fn every_draw() -> Vec<DrawSpec> {
    let ring = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]];
    vec![
        DrawSpec::Line {
            a: [0.0, 0.0],
            b: [1.0, 1.0],
        },
        DrawSpec::Polyline {
            points: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]].into(),
            bulges: vec![0.0, 0.5, 0.0],
            closed: false,
        },
        DrawSpec::Rectangle {
            a: [0.0, 0.0],
            b: [2.0, 1.0],
        },
        DrawSpec::Polygon {
            centre: [0.0, 0.0],
            sides: 5,
            radius: 1.0,
            inscribed: true,
        },
        DrawSpec::Circle {
            centre: [0.0, 0.0],
            radius: 1.0,
        },
        DrawSpec::Arc {
            centre: [0.0, 0.0],
            radius: 1.0,
            start: 0.0,
            end: 90.0,
        },
        DrawSpec::ThreePointArc {
            a: [1.0, 0.0],
            b: [0.0, 1.0],
            c: [-1.0, 0.0],
        },
        DrawSpec::Ellipse {
            centre: [0.0, 0.0],
            major: [2.0, 0.0],
            ratio: 0.5,
        },
        DrawSpec::Spline {
            fit_points: vec![[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]].into(),
        },
        DrawSpec::Point { p: [1.0, 1.0] },
        DrawSpec::Donut {
            centre: [0.0, 0.0],
            inner: 0.5,
            outer: 1.0,
        },
        DrawSpec::Ray {
            origin: [0.0, 0.0],
            dir: [1.0, 1.0],
        },
        DrawSpec::XLine {
            origin: [0.0, 0.0],
            dir: [1.0, 0.0],
        },
        DrawSpec::RevCloud {
            points: ring.clone().into(),
            arc_len: Some(1.0),
        },
        DrawSpec::Wipeout {
            points: ring.clone().into(),
        },
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: ring.clone().into(),
            holes: Vec::new(),
            bulges: Vec::new(),
            pattern: HatchPattern::Solid,
        },
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: ring.clone().into(),
            holes: Vec::new(),
            bulges: Vec::new(),
            pattern: HatchPattern::Ansi37,
        },
        DrawSpec::Solid2D {
            points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]].into(),
        },
        DrawSpec::Trace {
            points: vec![[0.0, 0.0], [2.0, 0.0]].into(),
            width: 0.2,
        },
        DrawSpec::MLine {
            points: vec![[0.0, 0.0], [3.0, 0.0], [3.0, 3.0]].into(),
            style: None,
            scale: Some(0.5),
        },
        DrawSpec::Sketch {
            points: vec![[0.0, 0.0], [0.5, 0.2], [1.0, 0.0]].into(),
        },
    ]
}

#[test]
fn every_draw_is_undone_one_step_at_a_time() {
    let (_dir, mut s) = sample_session();
    let before = s.drawing.doc.entities_in_block(MODEL).count();
    let specs = every_draw();
    let mut added = 0;
    for spec in specs.clone() {
        added += draw(&mut s, spec).len();
    }
    assert_eq!(
        s.drawing.doc.entities_in_block(MODEL).count(),
        before + added
    );
    for _ in &specs {
        assert!(s.undo());
    }
    assert_eq!(s.drawing.doc.entities_in_block(MODEL).count(), before);
    assert!(!s.can_undo());
}

#[test]
fn drawn_entities_survive_a_save_and_reopen_in_dxf_and_dwg() {
    for ext in ["dxf", "dwg"] {
        let dir = tempfile::tempdir().unwrap();
        let path = write_sample(dir.path(), ext);
        let mut s = Session::open(&path).unwrap();
        let mut handles = Vec::new();
        for spec in every_draw() {
            handles.extend(draw(&mut s, spec));
        }
        let kinds = |s: &Session, handles: &[u64]| -> Vec<String> {
            let mut kinds: Vec<String> = handles
                .iter()
                .filter_map(|&h| s.drawing.doc.get_entity(Handle::new(h)))
                .map(|e| format!("{e:?}").split('(').next().unwrap().to_string())
                .collect();
            kinds.sort();
            kinds
        };
        let want = kinds(&s, &handles);
        s.save().unwrap();
        let reopened = Session::open(&path).unwrap();
        assert_eq!(kinds(&reopened, &handles), want, "{ext}");
    }
}

#[test]
fn draws_go_on_the_sheet_on_screen() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = qiyas_core::fixtures::sample_doc();
    doc.add_paper_space_entity(EntityType::Line(opencadcodec::Line::from_coords(
        0.0, 0.0, 0.0, 1.0, 0.0, 0.0,
    )))
    .unwrap();
    let path = qiyas_core::fixtures::write_drawing(&doc, &dir.path().join("sheets.dxf"));
    let mut s = Session::open(&path).unwrap();
    let paper = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sh| !sh.is_model)
        .unwrap();
    s.run(Command::SetSheet {
        block: paper.block.clone(),
    })
    .unwrap();
    let h = draw(
        &mut s,
        DrawSpec::Line {
            a: [0.0, 0.0],
            b: [0.0, 1.0],
        },
    )[0];
    assert!(s
        .drawing
        .doc
        .entities_in_block(&paper.block)
        .any(|e| e.common().handle.value() == h));
    assert!(!s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .any(|e| e.common().handle.value() == h));
}

#[test]
fn the_wire_form_is_the_command_type_with_the_draw_kind_beside_it() {
    let json = r#"{"type":"draw","kind":"line","a":[0,0],"b":[1,2]}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    assert!(matches!(
        cmd,
        Command::Draw(DrawSpec::Line { b: [1.0, 2.0], .. })
    ));
    let hatch = r#"{"type":"draw","kind":"hatch","points":[[0,0],[1,0],[1,1]],
        "pattern":{"custom":{"name":"NET","angle":0,"scale":2}}}"#;
    let cmd: Command = serde_json::from_str(hatch).unwrap();
    let Command::Draw(DrawSpec::Hatch {
        boundary_handles,
        pattern: HatchPattern::Custom { name, .. },
        ..
    }) = &cmd
    else {
        panic!("{cmd:?}");
    };
    assert!(boundary_handles.is_empty());
    assert_eq!(name, "NET");
    let back = serde_json::to_value(&cmd).unwrap();
    assert_eq!(back["type"], "draw");
    assert_eq!(back["kind"], "hatch");
    let solid: DrawSpec =
        serde_json::from_str(r#"{"kind":"hatch","points":[[0,0],[1,0],[1,1]],"pattern":"solid"}"#)
            .unwrap();
    assert!(matches!(
        solid,
        DrawSpec::Hatch {
            pattern: HatchPattern::Solid,
            ref holes,
            ..
        } if holes.is_empty()
    ));
    let set: Command = serde_json::from_str(
        r#"{"type":"set_current","current":{"layer":"A-WALL","color":"ByLayer","linetype":"ByLayer","lineweight":-1}}"#,
    )
    .unwrap();
    assert!(matches!(set, Command::SetCurrent { .. }));
}

#[test]
fn drawn_hatches_solids_and_wipeouts_fill_in_the_batch() {
    let (_dir, mut s) = sample_session();
    let ring = vec![[30.0, 0.0], [34.0, 0.0], [34.0, 3.0], [30.0, 3.0]];
    let hatch = draw(
        &mut s,
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: ring.clone().into(),
            holes: Vec::new(),
            bulges: Vec::new(),
            pattern: HatchPattern::Solid,
        },
    )[0];
    let wipeout = draw(
        &mut s,
        DrawSpec::Wipeout {
            points: ring.into(),
        },
    )[0];
    let pattern = draw(
        &mut s,
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: vec![[40.0, 0.0], [50.0, 0.0], [50.0, 10.0], [40.0, 10.0]].into(),
            holes: Vec::new(),
            bulges: Vec::new(),
            pattern: HatchPattern::Ansi31,
        },
    )[0];
    let batch = s.batch();
    let filled = |h: u64| batch.fill_ranges.iter().any(|r| r.entity == h);
    assert!(filled(hatch) && filled(wipeout) && !filled(pattern));
    let wipe = batch
        .fill_ranges
        .iter()
        .find(|r| r.entity == wipeout)
        .unwrap();
    assert!(batch.fill_colors[wipe.first as usize] & qiyas_render::FILL_BACKGROUND != 0);
    assert!(batch
        .ranges
        .iter()
        .any(|r| r.entity == pattern && r.flags & qiyas_render::PATTERN != 0));
    // The pattern lines don't break the boundary search inside the hatched room.
    let ring = ring_of(s.boundary_at([45.0, 5.0], 20.0));
    assert!(close(signed_area(&ring).abs(), 100.0), "{ring:?}");
    assert!(s.undo() && s.undo() && s.undo());
    assert!(s.batch().fills.is_empty());
}

/// A closed square polyline from `lo` to `hi`.
fn square(s: &mut Session, lo: [f64; 2], hi: [f64; 2]) {
    draw(s, DrawSpec::Rectangle { a: lo, b: hi });
}

#[test]
fn the_boundary_search_grows_until_the_region_fits_and_finds_its_islands() {
    let (_dir, mut s) = sample_session();
    // A room 100 x 100 at 1000, a column in it far from the click and a column inside that.
    square(&mut s, [1000.0, 1000.0], [1100.0, 1100.0]);
    square(&mut s, [1080.0, 1080.0], [1090.0, 1090.0]);
    square(&mut s, [1083.0, 1083.0], [1086.0, 1086.0]);
    // Looked for 1 unit around first: the room is found all the same, with the outer column as
    // its one island (the inner one is the column's own).
    let Boundary::Ring { ring, holes, .. } = s.boundary_at([1010.0, 1010.0], 1.0) else {
        panic!("a ring");
    };
    assert!(close(signed_area(&ring).abs(), 10_000.0), "{ring:?}");
    assert_eq!(holes.len(), 1, "{holes:?}");
    assert!(close(signed_area(&holes[0]).abs(), 100.0), "{holes:?}");
    // Hatched with it, the column stays empty.
    let EntityType::Hatch(h) = one(
        &mut s,
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: ring.into(),
            holes: holes.into_iter().map(Into::into).collect(),
            bulges: Vec::new(),
            pattern: HatchPattern::Solid,
        },
    ) else {
        panic!("a hatch");
    };
    assert_eq!(h.paths.len(), 2);
    assert!(h.paths[0].flags.is_external());
    assert!(!h.paths[1].flags.is_external());
    // Inside the column, the column with its own island; outside everything, nothing even once
    // the box holds the whole sheet.
    let Boundary::Ring { ring, holes, .. } = s.boundary_at([1081.0, 1081.0], 0.5) else {
        panic!("the column");
    };
    assert!(close(signed_area(&ring).abs(), 100.0));
    assert!(close(signed_area(&holes[0]).abs(), 9.0));
    assert_eq!(s.boundary_at([5000.0, 5000.0], 1.0), Boundary::None);
}

#[test]
fn too_much_line_work_around_the_point_is_refused_never_searched_in_part() {
    let (_dir, mut s) = sample_session();
    // A circle inside the sample's 4 x 3 rectangle: drawn as many segments.
    draw(
        &mut s,
        DrawSpec::Circle {
            centre: [2.0, 1.5],
            radius: 1.0,
        },
    );
    assert_eq!(s.boundary_at_most([2.0, 1.5], 0.5, 8), Boundary::TooDense);
    // With room for them, the circle's inside.
    let Boundary::Ring { ring, .. } = s.boundary_at([2.0, 1.5], 0.5) else {
        panic!("a ring");
    };
    // The circle as drawn: a polygon a little inside it.
    let area = signed_area(&ring).abs();
    assert!(area > 0.9 * PI && area < PI, "{area}");
    // The wire form names what was found.
    let json = serde_json::to_value(Boundary::TooDense).unwrap();
    assert_eq!(json["found"], "too_dense");
    assert_eq!(
        serde_json::to_value(Boundary::None).unwrap()["found"],
        "none"
    );
}

#[test]
fn a_multiline_measures_along_its_centre_line() {
    let (_dir, mut s) = sample_session();
    let h = draw(
        &mut s,
        DrawSpec::MLine {
            points: vec![[0.0, 0.0], [5.0, 0.0], [5.0, 5.0]].into(),
            style: None,
            scale: Some(2.0),
        },
    )[0];
    let info = s.entity_info(h, &[]).unwrap();
    assert!(close(info.length.unwrap(), 10.0), "{:?}", info.length);
}

#[test]
fn the_current_lineweight_is_a_standard_one_and_the_linetype_takes_the_drawing_spelling() {
    let (_dir, mut s) = sample_session();
    for (weight, ok) in [
        (25, true),
        (211, true),
        (-1, true),
        (-3, true),
        (26, false),
        (1, false),
    ] {
        let result = s.run(Command::SetCurrent {
            current: Current {
                lineweight: weight,
                ..Current::default()
            },
        });
        assert_eq!(result.is_ok(), ok, "{weight}");
    }
    s.run(Command::SetCurrent {
        current: Current {
            linetype: "continuous".into(),
            ..Current::default()
        },
    })
    .unwrap();
    assert_eq!(s.current.linetype, "Continuous");
    s.run(Command::SetCurrent {
        current: Current {
            linetype: "bylayer".into(),
            ..Current::default()
        },
    })
    .unwrap();
    assert_eq!(s.current.linetype, "ByLayer");
}

#[test]
fn pattern_lines_near_the_click_do_not_count_toward_too_dense() {
    let (_dir, mut s) = sample_session();
    // Two finely hatched rooms beside a plain one: their pattern lines are left out first.
    // Away from the sample's own drawing.
    for x in [110.0, 120.0] {
        draw(
            &mut s,
            DrawSpec::Hatch {
                boundary_handles: Vec::new(),
                points: vec![[x, 0.0], [x + 5.0, 0.0], [x + 5.0, 5.0], [x, 5.0]].into(),
                holes: Vec::new(),
                bulges: Vec::new(),
                pattern: HatchPattern::Custom {
                    name: "ANSI31".into(),
                    angle: 0.0,
                    scale: 0.05,
                },
            },
        );
    }
    square(&mut s, [116.0, 0.0], [119.0, 5.0]);
    let batch = s.batch();
    let pattern = batch
        .ranges
        .iter()
        .filter(|r| r.flags & qiyas_render::PATTERN != 0)
        .map(|r| r.count as usize)
        .sum::<usize>();
    assert!(pattern > 40, "{pattern}");
    // A cap below the pattern lines' count, above the outlines' count.
    let ring = ring_of(s.boundary_at_most([117.5, 2.5], 20.0, 40));
    assert!(close(signed_area(&ring).abs(), 15.0), "{ring:?}");
}

#[test]
fn hatch_by_click_inside_a_circle_follows_the_circle_not_its_chords() {
    let (_dir, mut s) = sample_session();
    // A 1.5 m circle (the sample is in metres), far from the sample's own entities.
    draw(
        &mut s,
        DrawSpec::Circle {
            centre: [500.0, 500.0],
            radius: 0.75,
        },
    );
    let Boundary::Ring {
        ring,
        holes,
        bulges,
    } = s.boundary_at([500.2, 500.1], 0.5)
    else {
        panic!("a ring");
    };
    assert!(holes.is_empty());
    assert_eq!(bulges.len(), 1);
    assert_eq!(bulges[0].len(), ring.len());
    assert!(bulges[0].iter().all(|b| *b > 0.0), "every edge is an arc");
    let EntityType::Hatch(h) = one(
        &mut s,
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: ring.into(),
            holes: holes.into_iter().map(Into::into).collect(),
            bulges,
            pattern: HatchPattern::Solid,
        },
    ) else {
        panic!("a hatch");
    };
    let Outcome::Measured { measurement } = s
        .run(Command::MeasureEntity {
            handle: h.common.handle.value(),
            chain: Vec::new(),
        })
        .unwrap()
    else {
        panic!("measured");
    };
    let want = PI * 0.75 * 0.75;
    let rel = (measurement.value - want).abs() / want;
    assert!(rel < 1e-3, "{} vs {want}", measurement.value);

    // Around it, a 3 x 3 square: the circle is an island, taken away as a circle.
    draw(
        &mut s,
        DrawSpec::Rectangle {
            a: [498.5, 498.5],
            b: [501.5, 501.5],
        },
    );
    let Boundary::Ring {
        ring,
        holes,
        bulges,
    } = s.boundary_at([498.7, 498.7], 0.5)
    else {
        panic!("a ring");
    };
    assert_eq!(holes.len(), 1);
    let EntityType::Hatch(h) = one(
        &mut s,
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: ring.into(),
            holes: holes.into_iter().map(Into::into).collect(),
            bulges,
            pattern: HatchPattern::Solid,
        },
    ) else {
        panic!("a hatch");
    };
    let Outcome::Measured { measurement } = s
        .run(Command::MeasureEntity {
            handle: h.common.handle.value(),
            chain: Vec::new(),
        })
        .unwrap()
    else {
        panic!("measured");
    };
    let want = 9.0 - PI * 0.75 * 0.75;
    let rel = (measurement.value - want).abs() / want;
    assert!(rel < 1e-4, "{} vs {want}", measurement.value);
}

/// A drawing in metres with block CELL (a 1 x 1 square on layer 0 and a circle of radius 0.3 at
/// its middle on layer OWN), placed once at (300, 300) at scale 2 on A-WALL, and as a 2 x 2
/// array 5 apart at (400, 300).
fn block_session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = CadDocument::new();
    doc.header.insertion_units = 6;
    for name in ["A-WALL", "OWN"] {
        let mut layer = Layer::new(name);
        layer.handle = doc.allocate_handle();
        doc.layers.add(layer).unwrap();
    }
    let mut record = BlockRecord::new("CELL");
    record.handle = doc.allocate_handle();
    record.block_entity_handle = doc.allocate_handle();
    record.block_end_handle = doc.allocate_handle();
    let owner = record.handle;
    doc.block_records.add(record).unwrap();
    let mut square = LwPolyline::new();
    for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        square.add_point(Vector2::new(x, y));
    }
    square.is_closed = true;
    let mut circle = Circle::from_center_radius(Vector3::new(0.5, 0.5, 0.0), 0.3);
    circle.common.layer = "OWN".to_string();
    for mut e in [EntityType::LwPolyline(square), EntityType::Circle(circle)] {
        e.common_mut().owner_handle = owner;
        doc.add_entity(e).unwrap();
    }
    let mut once = Insert::new("CELL", Vector3::new(300.0, 300.0, 0.0)).with_scale(2.0, 2.0, 2.0);
    once.common.layer = "A-WALL".to_string();
    doc.add_entity(EntityType::Insert(once)).unwrap();
    let mut array = Insert::new("CELL", Vector3::new(400.0, 300.0, 0.0));
    array.column_count = 2;
    array.row_count = 2;
    array.column_spacing = 5.0;
    array.row_spacing = 5.0;
    doc.add_entity(EntityType::Insert(array)).unwrap();
    let path = write_drawing(&doc, &dir.path().join("cells.dxf"));
    let session = Session::open(&path).unwrap();
    (dir, session)
}

#[test]
fn hatch_by_click_inside_a_block_follows_its_entities_and_an_array_cell_as_drawn() {
    let (_dir, mut s) = block_session();
    // Inside the single reference: the square at scale 2, with the circle an exact island.
    let Boundary::Ring {
        ring,
        holes,
        bulges,
    } = s.boundary_at([300.2, 300.2], 0.5)
    else {
        panic!("a ring");
    };
    assert!(close(signed_area(&ring).abs(), 4.0), "{ring:?}");
    assert_eq!(holes.len(), 1, "{holes:?}");
    // The island's ring is the circle flattened at the measuring tolerance; its bulges make it exact.
    let want = PI * 0.6 * 0.6;
    let hole = signed_area(&holes[0]).abs();
    assert!((hole - want).abs() / want < 5e-3, "{hole} vs {want}");
    assert!(bulges[0].iter().all(|b| *b == 0.0), "straight sides");
    assert!(
        bulges[1].iter().all(|b| *b != 0.0),
        "the island is the circle"
    );
    let EntityType::Hatch(h) = one(
        &mut s,
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: ring.into(),
            holes: holes.into_iter().map(Into::into).collect(),
            bulges,
            pattern: HatchPattern::Solid,
        },
    ) else {
        panic!("a hatch");
    };
    let Outcome::Measured { measurement } = s
        .run(Command::MeasureEntity {
            handle: h.common.handle.value(),
            chain: Vec::new(),
        })
        .unwrap()
    else {
        panic!("measured");
    };
    let want = 4.0 - want;
    let rel = (measurement.value - want).abs() / want;
    assert!(rel < 1e-4, "{} vs {want}", measurement.value);
    // The measurement is a step of its own: two undos take the hatch off the sheet.
    assert!(
        s.undo() && s.undo(),
        "the hatch goes before the layers are hidden"
    );
    assert!(!s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .any(|e| matches!(e, EntityType::Hatch(_))));
    // The square is on layer 0 inside the block, so it is drawn on the reference's layer: it
    // stays when layer 0 is hidden. The circle on OWN goes with its layer.
    s.set_layer_hidden("0", true);
    let Boundary::Ring { ring, holes, .. } = s.boundary_at([300.2, 300.2], 0.5) else {
        panic!("a ring with layer 0 hidden");
    };
    assert!(close(signed_area(&ring).abs(), 4.0), "{ring:?}");
    assert_eq!(holes.len(), 1);
    s.set_layer_hidden("0", false);
    s.set_layer_hidden("OWN", true);
    let ring = ring_of(s.boundary_at([300.2, 300.2], 0.5));
    assert!(close(signed_area(&ring).abs(), 4.0), "{ring:?}");
    s.set_layer_hidden("OWN", false);
    // Inside a cell of the array, the first or another: the cell as drawn.
    for cell in [[400.2, 300.2], [405.2, 305.2]] {
        let Boundary::Ring { ring, holes, .. } = s.boundary_at(cell, 0.5) else {
            panic!("a ring in the cell at {cell:?}");
        };
        let area = signed_area(&ring).abs();
        assert!((area - 1.0).abs() < 1e-3, "{area} at {cell:?}");
        assert_eq!(holes.len(), 1, "{holes:?}");
    }
}

#[test]
fn hatch_by_click_beside_a_multiline_wall_stops_at_its_drawn_face() {
    let (_dir, mut s) = sample_session();
    // A room closed on its left by a multiline wall along x = 2000, scale 2: the Standard
    // style's elements at +-0.5 put its faces at x = 1999 and x = 2001. The three other sides
    // are lines meeting the wall's centre line.
    for (a, b) in [
        ([2000.0, 2000.0], [2010.0, 2000.0]),
        ([2010.0, 2000.0], [2010.0, 2010.0]),
        ([2010.0, 2010.0], [2000.0, 2010.0]),
    ] {
        draw(&mut s, DrawSpec::Line { a, b });
    }
    draw(
        &mut s,
        DrawSpec::MLine {
            points: vec![[2000.0, 1995.0], [2000.0, 2015.0]].into(),
            style: None,
            scale: Some(2.0),
        },
    );
    // The room as drawn runs to the wall's face (9 x 10), not to its centre line (10 x 10).
    let ring = ring_of(s.boundary_at([2005.0, 2005.0], 0.5));
    assert!(close(signed_area(&ring).abs(), 90.0), "{ring:?}");
    assert!(ring.iter().all(|p| p[0] >= 2001.0 - 1e-9), "{ring:?}");
}

#[test]
fn hatch_by_click_is_closed_by_a_construction_line_or_a_ray_crossing_the_box() {
    let (_dir, mut s) = sample_session();
    // A U open at the top: on its own, nothing closes around a click inside it.
    for (a, b) in [
        ([3000.0, 3000.0], [3010.0, 3000.0]),
        ([3000.0, 3000.0], [3000.0, 3010.0]),
        ([3010.0, 3000.0], [3010.0, 3010.0]),
    ] {
        draw(&mut s, DrawSpec::Line { a, b });
    }
    let click = [3005.0, 3004.0];
    assert_eq!(s.boundary_at(click, 0.5), Boundary::None);
    // A construction line across it at y = 3008, its base point outside the U: a 10 x 8 room.
    draw(
        &mut s,
        DrawSpec::XLine {
            origin: [3020.0, 3008.0],
            dir: [1.0, 0.0],
        },
    );
    let ring = ring_of(s.boundary_at(click, 0.5));
    assert!(close(signed_area(&ring).abs(), 80.0), "{ring:?}");
    assert!(s.undo());
    // A ray from the same point towards the U closes it the same way; one pointing away does not.
    draw(
        &mut s,
        DrawSpec::Ray {
            origin: [3020.0, 3008.0],
            dir: [-1.0, 0.0],
        },
    );
    let ring = ring_of(s.boundary_at(click, 0.5));
    assert!(close(signed_area(&ring).abs(), 80.0), "{ring:?}");
    assert!(s.undo());
    draw(
        &mut s,
        DrawSpec::Ray {
            origin: [3020.0, 3008.0],
            dir: [1.0, 0.0],
        },
    );
    assert_eq!(s.boundary_at(click, 0.5), Boundary::None);
}

#[test]
fn a_hatch_by_click_with_an_island_keeps_its_paths_through_dwg_and_dxf() {
    let (dir, mut s) = sample_session();
    // A 3 x 3 square with a circle in it: hatched by a click, the circle is a bulged island.
    draw(
        &mut s,
        DrawSpec::Circle {
            centre: [500.0, 500.0],
            radius: 0.75,
        },
    );
    square(&mut s, [498.5, 498.5], [501.5, 501.5]);
    let Boundary::Ring {
        ring,
        holes,
        bulges,
    } = s.boundary_at([498.7, 498.7], 0.5)
    else {
        panic!("a ring");
    };
    assert_eq!(holes.len(), 1);
    let EntityType::Hatch(drawn) = one(
        &mut s,
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: ring.into(),
            holes: holes.into_iter().map(Into::into).collect(),
            bulges,
            pattern: HatchPattern::Solid,
        },
    ) else {
        panic!("a hatch");
    };
    // Each path is one closed polyline edge, flagged as one; only the first is external.
    assert!(drawn.paths.iter().all(|p| p.is_polyline()));
    assert!(drawn.paths[0].flags.is_external());
    assert!(!drawn.paths[1].flags.is_external());
    let handle = drawn.common.handle.value();
    let area = |s: &mut Session| match s
        .run(Command::MeasureEntity {
            handle,
            chain: Vec::new(),
        })
        .unwrap()
    {
        Outcome::Measured { measurement } => measurement.value,
        other => panic!("{other:?}"),
    };
    let want = area(&mut s);
    assert!((want - (9.0 - PI * 0.75 * 0.75)).abs() < 1e-6, "{want}");
    for ext in ["dwg", "dxf"] {
        let path = dir.path().join(format!("island.{ext}"));
        s.save_as(&path).unwrap();
        let mut back = Session::open(&path).unwrap();
        let Some(EntityType::Hatch(read)) = back.drawing.doc.get_entity(Handle::new(handle)) else {
            panic!("the hatch back from {ext}");
        };
        assert_eq!(read.paths.len(), drawn.paths.len(), "{ext}");
        for (a, b) in drawn.paths.iter().zip(&read.paths) {
            assert_eq!(a.flags, b.flags, "{ext}");
            let ([BoundaryEdge::Polyline(a)], [BoundaryEdge::Polyline(b)]) =
                (a.edges.as_slice(), b.edges.as_slice())
            else {
                panic!("one polyline edge each from {ext}: {b:?}");
            };
            assert_eq!(a.vertices.len(), b.vertices.len(), "{ext}");
            assert!(a.vertices.iter().zip(&b.vertices).all(|(p, q)| [
                p.x - q.x,
                p.y - q.y,
                p.z - q.z
            ]
            .iter()
            .all(|d| d.abs() < 1e-9)));
        }
        // The sheet draws (no stray coordinates in the pick index) and measures the same.
        assert!(!back.batch().segments.is_empty());
        let got = area(&mut back);
        assert!((got - want).abs() < 1e-9, "{ext}: {got} vs {want}");
    }
}

#[test]
fn hatch_by_click_beside_an_island_touching_the_wall_leaves_it_out() {
    let (_dir, mut s) = sample_session();
    // A 10 x 10 room with a triangle (area 3) against its left wall, touching it at one point:
    // the region found walks round the triangle, touching itself there.
    square(&mut s, [4000.0, 4000.0], [4010.0, 4010.0]);
    for (a, b) in [
        ([4000.0, 4005.0], [4003.0, 4004.0]),
        ([4003.0, 4004.0], [4003.0, 4006.0]),
        ([4003.0, 4006.0], [4000.0, 4005.0]),
    ] {
        draw(&mut s, DrawSpec::Line { a, b });
    }
    let Boundary::Ring {
        ring,
        holes,
        bulges,
    } = s.boundary_at([4008.0, 4008.0], 0.5)
    else {
        panic!("a ring");
    };
    assert!(close(signed_area(&ring).abs(), 97.0), "{ring:?}");
    let EntityType::Hatch(h) = one(
        &mut s,
        DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: ring.into(),
            holes: holes.into_iter().map(Into::into).collect(),
            bulges,
            pattern: HatchPattern::Solid,
        },
    ) else {
        panic!("a hatch");
    };
    let Outcome::Measured { measurement } = s
        .run(Command::MeasureEntity {
            handle: h.common.handle.value(),
            chain: Vec::new(),
        })
        .unwrap()
    else {
        panic!("measured");
    };
    assert!(close(measurement.value, 97.0), "{}", measurement.value);
}
