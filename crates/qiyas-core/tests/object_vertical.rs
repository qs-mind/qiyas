//! Plan unions must not deduct material from another level or vertical span.

use opencadcodec::EntityType;
use qiyas_core::objects::ObjectFilter;
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};

fn run(session: &mut Session, input: Value) -> Outcome {
    session.run_input(&input, Source::User).unwrap().outcome
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
}

fn totals(session: &Session) -> (f64, f64) {
    let objects = session.objects(&ObjectFilter::default()).unwrap();
    (
        objects
            .iter()
            .map(|object| object.quantities["plan_area"])
            .sum(),
        objects
            .iter()
            .map(|object| object.quantities["solid"])
            .sum(),
    )
}

fn band_definition() -> Value {
    json!({"type":"define_object_type","definition":{
        "name":"Band", "category":"Test geometry", "geometry":"band", "join":"union",
        "properties":{
            "width":{"kind":"number","unit":"length","default":0.2},
            "rise":{"kind":"number","unit":"length","default":3},
            "lift":{"kind":"number","unit":"length","default":0}
        },
        "vertical":{"base":"level_elevation + lift", "height":"rise"},
        "plan":{"band_faces":"width", "hatch":true},
        "quantities":{"plan_area":"net_area", "solid":"net_area * height"}
    }})
}

#[test]
fn union_preserves_separate_levels_bases_and_heights_but_accepts_level_name_case() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("vertical.dxf"), 6);
    for (level, elevation) in [("Ground", 0.0), ("First", 3.0)] {
        run(
            &mut session,
            json!({"type":"set_knowledge","kind":"levels","name":level,"data":{"elevation":elevation}}),
        );
    }
    run(&mut session, band_definition());
    let Outcome::Added { handles } = run(
        &mut session,
        json!({"type":"place_objects","objects":[
            {"type":"Band","points":[[0,0],[4,0]],"level":"Ground"},
            {"type":"Band","points":[[2,-2],[2,2]],"level":"ground"},
            {"type":"Band","points":[[0,0],[4,0]],"level":"First"},
            {"type":"Band","points":[[0,1],[4,1]],"level":"Ground","props":{"lift":1}},
            {"type":"Band","points":[[0,0],[4,0]],"level":"Ground","props":{"rise":4}}
        ]}),
    ) else {
        panic!("Expected object drivers")
    };
    let objects = session.objects(&ObjectFilter::default()).unwrap();
    let ground = session
        .objects(&ObjectFilter {
            level: Some("Ground".into()),
            ..ObjectFilter::default()
        })
        .unwrap();
    assert_eq!(ground.len(), 4);
    assert!(ground
        .iter()
        .any(|object| object.instance.level.as_deref() == Some("ground")));
    near(objects[0].quantities["plan_area"], 0.8);
    near(objects[1].quantities["plan_area"], 0.76);
    for object in &objects[2..] {
        near(object.quantities["plan_area"], 0.8);
    }
    near(totals(&session).0, 3.96);
    near(totals(&session).1, 12.68);
    assert_eq!(
        session
            .drawing
            .doc
            .entities()
            .filter(|entity| matches!(entity, EntityType::Hatch(_)))
            .count(),
        4
    );
    let Outcome::Added { handles: tables } = run(
        &mut session,
        json!({"type":"object_schedule","at":[0,10],"columns":["type","plan_area","solid"],"group_by":["type"]}),
    ) else {
        panic!("Expected a schedule")
    };
    let table_row = |session: &Session| {
        let EntityType::Table(table) = session
            .drawing
            .doc
            .get_entity(opencadcodec::Handle::new(tables[0]))
            .unwrap()
        else {
            panic!("Expected a table")
        };
        (0..3)
            .map(|column| table.cell_text(1, column).unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(table_row(&session), ["Band", "3.96 m²", "12.680"]);
    let steps = session.history_steps().0.len();
    run(
        &mut session,
        json!({"type":"set_object_props","targets":[handles[3]],"props":{"lift":0}}),
    );
    assert_eq!(session.history_steps().0.len(), steps + 1);
    near(totals(&session).0, 3.92);
    near(totals(&session).1, 12.56);
    assert_eq!(table_row(&session), ["Band", "3.92 m²", "12.560"]);
    assert!(session.undo());
    near(totals(&session).0, 3.96);
    near(totals(&session).1, 12.68);
    assert_eq!(table_row(&session), ["Band", "3.96 m²", "12.680"]);
}

#[test]
fn a_hosted_type_without_a_host_station_is_refused_before_it_can_be_used() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("hosted.dxf"), 6);
    let definition = json!({"name":"Hosted", "category":"Test geometry", "geometry":"hosted"});
    let before = session.history_steps().0.len();
    let error = session
        .run_input(
            &json!({"type":"define_object_type","definition":definition}),
            Source::User,
        )
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("host_cut") && error.contains("station"),
        "{error}"
    );
    assert_eq!(session.history_steps().0.len(), before);
    assert!(session.object_types().unwrap().is_empty());
    let mut anchored = definition;
    anchored["host_cut"] = json!({"width":"0.5", "at":"1"});
    run(
        &mut session,
        json!({"type":"define_object_type","definition":anchored}),
    );
    assert_eq!(session.object_types().unwrap().len(), 1);
}

#[test]
fn coincident_parallel_union_members_are_refused_without_partial_placement() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("parallel.dxf"), 6);
    run(&mut session, band_definition());
    run(
        &mut session,
        json!({"type":"place_objects","objects":[
            {"type":"Band","points":[[0,0],[4,0]]}
        ]}),
    );
    let steps = session.history_steps().0.len();
    let entities = session.drawing.doc.entities().count();
    let error = session
        .run_input(
            &json!({"type":"place_objects","objects":[
                {"type":"Band","points":[[0,0],[4,0]]}
            ]}),
            Source::User,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("overlapping parallel walls"), "{error}");
    assert_eq!(session.history_steps().0.len(), steps);
    assert_eq!(session.drawing.doc.entities().count(), entities);
    assert_eq!(session.objects(&ObjectFilter::default()).unwrap().len(), 1);
    near(totals(&session).0, 0.8);
    near(totals(&session).1, 2.4);
}
