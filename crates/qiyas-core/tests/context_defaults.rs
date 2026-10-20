use std::path::Path;

use qiyas_core::validation::{DefaultUsed, ValidationReport};
use qiyas_core::{Session, Source};
use serde_json::{json, Value};

fn run(session: &mut Session, input: Value) -> Value {
    serde_json::to_value(session.run_input(&input, Source::User).unwrap()).unwrap()
}

fn default<'a>(report: &'a ValidationReport, field: &str) -> &'a DefaultUsed {
    assert!(report.valid, "{report:?}");
    report
        .defaults_used
        .iter()
        .find(|d| d.field == field)
        .unwrap_or_else(|| panic!("Missing {field}: {report:?}"))
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
}

fn boundary(session: &mut Session, metres: f64) {
    run(
        session,
        json!({
            "type":"draw", "kind":"polyline", "closed":true,
            "points":[[0,0],[8.0/metres,0],[8.0/metres,6.0/metres],[0,6.0/metres]]
        }),
    );
}

#[test]
fn geometry_defaults_are_effective_and_read_only_in_every_unit() {
    for (code, metres, unit) in [
        (4, 0.001, "mm"),
        (5, 0.01, "cm"),
        (6, 1.0, "m"),
        (1, 0.0254, "in"),
    ] {
        let mut session = Session::new_blank(Path::new("context.dxf"), code);
        boundary(&mut session, metres);
        let line = run(&mut session, json!({
            "type":"draw", "kind":"line", "a":[2.0/metres,3.0/metres], "b":[6.0/metres,3.0/metres]
        }))["handles"][0].as_u64().unwrap();
        let before =
            serde_json::to_value(session.drawing.doc.entities().collect::<Vec<_>>()).unwrap();
        let clipboard = session.validate_input(&json!({
            "type":"copy_to_clipboard", "targets":[{"handle":line}]
        }));
        let base = default(&clipboard, "/base");
        near(base.value[0].as_f64().unwrap() * metres, 2.0);
        near(base.value[1].as_f64().unwrap() * metres, 3.0);
        assert_eq!(base.unit.as_deref(), Some(unit));

        let grid = session.validate_input(&json!({
            "type":"grid", "x":[{"label":"A","at":0}], "y":[{"label":"1","at":0}]
        }));
        let extent = &default(&grid, "/extent").value;
        near(extent["max"][0].as_f64().unwrap() * metres, 8.0);
        near(extent["max"][1].as_f64().unwrap() * metres, 6.0);

        let labels = session.validate_input(&json!({
            "type":"label_spaces", "query":{"seeds":[{"name":"Room","point":[1.0/metres,1.0/metres]}]}
        }));
        let height = default(&labels, "/height");
        near(height.value[0]["value"].as_f64().unwrap(), 0.25);
        assert_eq!(height.unit.as_deref(), Some("m"));
        assert_eq!(default(&labels, "/language").value[0]["value"], "en");
        assert_eq!(default(&labels, "/layer").value, session.current.layer);

        let input = json!({
            "type":"add_dim_constraint", "kind":"linear", "targets":[{"handle":line}],
            "points":[[4.0/metres,3.0/metres]],
            "line_point":[4.0/metres,2.0/metres]
        });
        let dimensional = session.validate_input(&input);
        let expression = default(&dimensional, "/expression");
        near(
            expression.value.as_str().unwrap().parse::<f64>().unwrap() * metres,
            4.0,
        );
        assert_eq!(expression.unit.as_deref(), Some(unit));
        assert_eq!(
            before,
            serde_json::to_value(session.drawing.doc.entities().collect::<Vec<_>>()).unwrap()
        );
        assert!(session.list_parameters().is_empty());

        run(&mut session, input);
        let parameter = &session.list_parameters()[0];
        assert_eq!(parameter.expression, expression.value.as_str().unwrap());
        near(parameter.value.unwrap() * metres, 4.0);
    }
}

#[test]
fn viewport_defaults_report_the_model_centre_and_paper_scale_used() {
    for (code, metres, unit) in [
        (4, 0.001, "mm"),
        (5, 0.01, "cm"),
        (6, 1.0, "m"),
        (1, 0.0254, "in"),
    ] {
        let mut session = Session::new_blank(Path::new("viewport-context.dxf"), code);
        boundary(&mut session, metres);
        run(
            &mut session,
            json!({"type":"new_layout","name":"Plan","paper":"ISO A3"}),
        );
        let sheet = session
            .drawing
            .sheets()
            .into_iter()
            .find(|s| s.name == "Plan")
            .unwrap();
        session.set_sheet(&sheet.block).unwrap();
        let input = json!({"type":"mview","corner1":[10,10],"corner2":[170,130]});
        let before = session.drawing.doc.entities().count();
        let report = session.validate_input(&input);
        let centre = default(&report, "/centre");
        near(centre.value[0].as_f64().unwrap() * metres, 4.0);
        near(centre.value[1].as_f64().unwrap() * metres, 3.0);
        assert_eq!(centre.unit.as_deref(), Some(unit));
        let scale = default(&report, "/scale");
        near(scale.value.as_f64().unwrap() / metres, 20.0);
        assert_eq!(scale.unit.as_deref(), Some(format!("mm/{unit}").as_str()));
        assert_eq!(session.drawing.doc.entities().count(), before);
        run(&mut session, input);
        let views = session.paper_views();
        let view = views.last().unwrap();
        near(view.scale, scale.value.as_f64().unwrap());
        assert_eq!(json!(view.centre), centre.value);
    }
}
