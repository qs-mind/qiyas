//! Return-to-start means a boundary only for commands whose input type declares it.

use opencadcodec::EntityType;
use qiyas_core::{Command, PointRun, Session, Units};
use serde_json::json;

#[test]
fn closure_tolerance_is_physical_and_survives_survey_coordinates() {
    for units in [
        Units::Millimetres,
        Units::Centimetres,
        Units::Metres,
        Units::Inches,
    ] {
        let scale = 1.0 / units.to_metres();
        for origin in [0.0, 3_300_000.0] {
            let a = [origin * scale, origin * scale];
            let near = if origin == 0.0 {
                qiyas_geom::SAME_POINT * 0.001 * scale * 0.5
            } else {
                origin * scale * 5e-14
            };
            let mut boundary: PointRun<true> = vec![
                a,
                [a[0] + scale, a[1]],
                [a[0], a[1] + scale],
                [a[0] + near, a[1]],
            ]
            .into();
            let mut open: PointRun = boundary.to_vec().into();
            assert!(boundary.normalize(units, false), "{units:?}, {origin}");
            assert_eq!(boundary.len(), 3);
            assert!(!open.normalize(units, false));
            assert_eq!(open.len(), 4);
            assert!(open.normalize(units, true));
            assert_eq!(open.len(), 3);
        }
    }
}

#[test]
fn diagonal_returns_share_geometry_same_point_comparison() {
    for units in [
        Units::Millimetres,
        Units::Centimetres,
        Units::Metres,
        Units::Inches,
    ] {
        let tolerance = qiyas_geom::SAME_POINT * 0.001 / units.to_metres();
        let mut points: PointRun<true> = vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.8 * tolerance, 0.8 * tolerance],
        ]
        .into();
        assert!(points.normalize(units, false));
        assert_eq!(points.len(), 3);
    }
}

#[test]
fn every_closed_drawing_input_accepts_a_repeated_start_in_every_unit() {
    let dir = tempfile::tempdir().unwrap();
    for unit in [4, 5, 6, 1] {
        let scale = 1.0 / Units::from_header(unit).to_metres();
        let points = json!([
            [0.0, 0.0],
            [4.0 * scale, 0.0],
            [4.0 * scale, 3.0 * scale],
            [0.0, 3.0 * scale],
            [0.0, 0.0]
        ]);
        for mut command in [
            json!({"type":"draw","kind":"polyline","closed":true}),
            json!({"type":"draw","kind":"revcloud","arc_len":0.5*scale}),
            json!({"type":"draw","kind":"wipeout"}),
            json!({"type":"draw","kind":"hatch","pattern":"solid"}),
            json!({"type":"comment","text":"Check boundary"}),
            json!({"type":"measure_area"}),
            json!({"type":"add_measurement","kind":"area","handles":[],"description":"Area","note":""}),
            json!({"type":"add_markup","kind":"cloud","text":"Check boundary"}),
        ] {
            let mut session = Session::new_blank(&dir.path().join("run.dxf"), unit);
            command["points"] = points.clone();
            let label = command.to_string();
            let command: Command = serde_json::from_value(command).unwrap();
            session
                .run(command)
                .unwrap_or_else(|e| panic!("{label}: {e}"));
            for entity in session.drawing.doc.entities() {
                if let EntityType::LwPolyline(p) = entity {
                    assert!(p.is_closed);
                    assert_ne!(
                        p.vertices.first().unwrap().location,
                        p.vertices.last().unwrap().location
                    );
                }
            }
            for row in &session.takeoff.items {
                assert_eq!(row.points.len(), 4, "{label}");
            }
        }
    }
}

#[test]
fn plain_polyline_keeps_its_return_vertex_and_wall_centrelines_close() {
    let dir = tempfile::tempdir().unwrap();
    for unit in [4, 5, 6, 1] {
        let scale = 1.0 / Units::from_header(unit).to_metres();
        let points = json!([
            [0.0, 0.0],
            [4.0 * scale, 0.0],
            [4.0 * scale, 3.0 * scale],
            [0.0, 3.0 * scale],
            [0.0, 0.0]
        ]);
        let mut session = Session::new_blank(&dir.path().join("run.dxf"), unit);
        session
            .run(
                serde_json::from_value(json!({"type":"draw","kind":"polyline","points":points}))
                    .unwrap(),
            )
            .unwrap();
        let p = session
            .drawing
            .doc
            .entities()
            .find_map(|e| match e {
                EntityType::LwPolyline(p) => Some(p),
                _ => None,
            })
            .unwrap();
        assert!(!p.is_closed);
        assert_eq!(p.vertices.len(), 5);
        let mut session = Session::new_blank(&dir.path().join("walls.dxf"), unit);
        session
            .run(
                serde_json::from_value(
                    json!({"type":"walls","runs":[{"points":points}],"thickness":0.2*scale}),
                )
                .unwrap(),
            )
            .unwrap();
        for e in session.drawing.doc.entities() {
            if let EntityType::LwPolyline(p) = e {
                assert!(p.is_closed);
                assert_ne!(
                    p.vertices.first().unwrap().location,
                    p.vertices.last().unwrap().location
                );
            }
        }
    }
}

#[test]
fn registry_path_fields_explain_their_closure_policy() {
    let get = |name: &str| &qiyas_core::registry::registry().get(name).unwrap().schema;
    for name in [
        "draw.polyline",
        "draw.spline",
        "draw.revcloud",
        "draw.wipeout",
        "draw.hatch",
        "draw.solid2d",
        "draw.trace",
        "draw.mline",
        "draw.sketch",
        "measure_area",
        "measure_distance",
        "comment",
        "leader",
        "mleader",
    ] {
        let field = if name == "draw.spline" {
            "fit_points"
        } else {
            "points"
        };
        let description = get(name)["properties"][field]["description"]
            .as_str()
            .unwrap_or("");
        assert!(
            description.contains("Returning to the start"),
            "{name}: {description}"
        );
    }
    assert!(
        get("walls")["properties"]["runs"]["items"]["properties"]["points"]["description"]
            .as_str()
            .unwrap()
            .contains("closes")
    );
}

#[test]
fn hatch_islands_and_selection_polygons_use_the_same_closure_policy() {
    let dir = tempfile::tempdir().unwrap();
    for unit in [4, 5, 6, 1] {
        let scale = 1.0 / Units::from_header(unit).to_metres();
        let mut session = Session::new_blank(&dir.path().join("island.dxf"), unit);
        let outer = vec![
            [0.0, 0.0],
            [4.0 * scale, 0.0],
            [4.0 * scale, 4.0 * scale],
            [0.0, 4.0 * scale],
        ];
        let mut repeated = outer.clone();
        repeated.push([qiyas_geom::SAME_POINT * 0.0005 * scale, 0.0]);
        session.run(serde_json::from_value(json!({
            "type":"draw", "kind":"hatch", "points":repeated,
            "holes":[[[scale,scale],[2.0*scale,scale],[2.0*scale,2.0*scale],[scale,2.0*scale],[scale,scale]]]
        })).unwrap()).unwrap();
        let hatch = session
            .drawing
            .doc
            .entities()
            .find(|e| matches!(e, EntityType::Hatch(_)))
            .unwrap();
        let handle = hatch.common().handle.value();
        let area = session.entity_info(handle, &[]).unwrap().area.unwrap();
        assert!((area / (scale * scale) - 15.0).abs() < 1e-8);
        session.run(serde_json::from_value(json!({"type":"draw","kind":"line","a":[scale,scale],"b":[3.0*scale,3.0*scale]})).unwrap()).unwrap();
        let pick = |points: &[[f64; 2]]| {
            session
                .pick_polygon(points, true)
                .into_iter()
                .map(|p| p.top)
                .collect::<Vec<_>>()
        };
        assert!(!pick(&outer).is_empty());
        assert_eq!(pick(&outer), pick(&repeated));
    }
}
