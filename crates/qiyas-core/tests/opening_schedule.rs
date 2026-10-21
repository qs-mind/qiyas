use opencadcodec::{EntityType, Handle};
use qiyas_core::{Command, Outcome, Session};
use serde_json::{json, Value};

fn run(s: &mut Session, value: Value) -> Outcome {
    s.run(serde_json::from_value::<Command>(value).unwrap())
        .unwrap()
}

fn cells(s: &Session, handle: u64) -> Vec<Vec<String>> {
    let EntityType::Table(t) = s.drawing.doc.get_entity(Handle::new(handle)).unwrap() else {
        panic!()
    };
    (0..t.rows.len())
        .map(|r| {
            (0..t.columns.len())
                .map(|c| t.cell_text(r, c).unwrap_or("").to_string())
                .collect()
        })
        .collect()
}

#[test]
fn opening_schedule_measures_all_openings_tracks_sizes_counts_and_undo_in_every_unit() {
    for units in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("openings.dxf"), units);
        let k = 1.0 / s.drawing.units.to_metres();
        let openings: Vec<_> = (0..27).map(|i| json!({"run":0,"at":(1.0+i as f64*3.0)*k,"width":0.8*k,"kind":if i%2==0 {"door"} else {"window"}})).collect();
        run(
            &mut s,
            json!({"type":"walls","runs":[{"points":[[0,0],[100.0*k,0]]}],"thickness":0.2*k,"openings":openings}),
        );
        run(&mut s, json!({"type":"set_sheet","block":"*Paper_Space"}));
        let before = s.history_steps().0.len();
        let Outcome::Added { handles } = run(
            &mut s,
            json!({"type":"opening_schedule","at":[10,100],"source_sheet":"*Model_Space"}),
        ) else {
            panic!()
        };
        let table = handles[0];
        let initial = cells(&s, table);
        assert_eq!(s.history_steps().0.len(), before + 1);
        assert_eq!(initial.len(), 4);
        assert_eq!(initial[1], ["D-800x2100", "Door", "800 x 2100", "14"]);
        assert_eq!(initial[2], ["W-800x1200", "Window", "800 x 1200", "13"]);
        assert_eq!(initial[3][3], "27");
        let EntityType::Table(t) = s.drawing.doc.get_entity(Handle::new(table)).unwrap() else {
            panic!()
        };
        assert_eq!(t.common.layer, "A-ANNO-TABL");
        assert_eq!(s.current.layer, "0");
        assert!(t.columns.iter().map(|c| c.width).sum::<f64>() < 400.0);
        assert!(t.common.extended_data.get_record("QIYAS_FIELD").is_some());
        assert!(s.undo());
        assert!(s.drawing.doc.get_entity(Handle::new(table)).is_none());
        assert!(!s.drawing.doc.layers.contains("A-ANNO-TABL"));
        assert!(s.redo());
        assert_eq!(cells(&s, table), initial);
        run(&mut s, json!({"type":"set_sheet","block":"*Model_Space"}));
        let groups = s.wall_groups(None).unwrap();
        let opening = &groups[0].runs[0].openings[0];
        assert_eq!(opening.mark.as_deref(), Some("D-800x2100"));
        let insert = opening.insert;
        run(
            &mut s,
            json!({"type":"edit_wall_opening","insert":insert,"width":k,"head":2.0}),
        );
        let resized = cells(&s, table);
        assert_eq!(resized.len(), 5);
        assert!(resized.iter().any(|r| r[0] == "D-1000x2000" && r[3] == "1"));
        assert_eq!(resized.last().unwrap()[3], "27");
        s.undo();
        assert_eq!(cells(&s, table), initial);
        s.redo();
        assert_eq!(cells(&s, table), resized);
        let groups = s.wall_groups(None).unwrap();
        let window = groups[0].runs[0].openings[1].insert;
        run(
            &mut s,
            json!({"type":"remove_wall_openings","inserts":[window]}),
        );
        assert_eq!(cells(&s, table).last().unwrap()[3], "26");
        s.undo();
        assert_eq!(cells(&s, table), resized);
        let groups = s.wall_groups(None).unwrap();
        let centreline = groups[0].runs[0].centreline;
        run(
            &mut s,
            json!({"type":"add_wall_openings","centreline":centreline,"openings":[{"at":90.0*k,"width":1.5*k,"kind":"window"}]}),
        );
        assert_eq!(cells(&s, table).last().unwrap()[3], "28");
        assert!(cells(&s, table)
            .iter()
            .any(|r| r[0] == "W-1500x1200" && r[3] == "1"));
        for extension in ["dxf", "dwg"] {
            let path = dir.path().join(format!("saved.{extension}"));
            s.save_as_headless(&path).unwrap();
            let mut reopened = Session::open(&path).unwrap();
            assert_eq!(cells(&reopened, table), cells(&s, table));
            let groups = reopened.wall_groups(None).unwrap();
            assert!(groups[0].runs[0].openings.iter().all(|o| o.mark.is_some()));
            let insert = groups[0].runs[0].openings[0].insert;
            run(
                &mut reopened,
                json!({"type":"remove_wall_openings","inserts":[insert]}),
            );
            assert_eq!(cells(&reopened, table).last().unwrap()[3], "27");
            reopened.save_as_headless(&path).unwrap();
            let mut again = Session::open(&path).unwrap();
            run(&mut again, json!({"type":"update_fields"}));
            assert_eq!(cells(&again, table), cells(&reopened, table));
        }
    }
}
