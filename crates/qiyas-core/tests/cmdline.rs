use qiyas_core::cmdline::{Machine, Step};
use qiyas_core::{Command, DrawSpec, Ucs};
use serde_json::{json, Value};

fn command(machine: &mut Machine, line: &str) -> Value {
    match machine
        .step(line)
        .unwrap_or_else(|error| panic!("{line}: {error}"))
    {
        Step::Command(command) => serde_json::to_value(command).unwrap(),
        other => panic!("{line}: expected a command, got {other:?}"),
    }
}

fn prompt(machine: &mut Machine, line: &str) -> String {
    match machine
        .step(line)
        .unwrap_or_else(|error| panic!("{line}: {error}"))
    {
        Step::Prompt(prompt) => prompt,
        other => panic!("{line}: expected a prompt, got {other:?}"),
    }
}

fn near(actual: [f64; 2], expected: [f64; 2]) {
    assert!(
        (actual[0] - expected[0]).abs() < 1e-9,
        "{actual:?} != {expected:?}"
    );
    assert!(
        (actual[1] - expected[1]).abs() < 1e-9,
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn fixed_points_work_inline_or_one_prompt_at_a_time() {
    let expected = json!({"type": "draw", "kind": "line", "a": [1., 2.], "b": [4., 6.]});
    let mut machine = Machine::default();
    assert_eq!(command(&mut machine, "l 1,2 @3,4"), expected);
    assert!(!machine.is_active());
    assert_eq!(machine.last(), Some([4., 6.]));

    assert!(!prompt(&mut machine, "LINE").is_empty());
    assert!(machine.is_active());
    assert!(!prompt(&mut machine, "1,2").is_empty());
    assert_eq!(command(&mut machine, "@3,4"), expected);
}

#[test]
fn completed_input_keeps_omitted_defaults_for_execution() {
    let mut machine = Machine::default();
    let Step::Command(Command::Text { height, .. }) = machine.step("TEXT 0,0 Room").unwrap() else {
        panic!("expected a plain text command");
    };
    assert_eq!(height, 0.0);
    let input = machine.take_input().expect("completed wire input");
    assert_eq!(input, json!({"type":"text","at":[0.0,0.0],"value":"Room"}));
    assert!(machine.take_input().is_none());
    let mut session = qiyas_core::Session::new_blank(std::path::Path::new("typed.dxf"), 6);
    let report = session.run_input(&input, qiyas_core::Source::User).unwrap();
    assert!(matches!(report.outcome, qiyas_core::Outcome::Added { .. }));
    assert!(report
        .defaults_used
        .iter()
        .any(|default| default.field == "/height" && default.value == json!(0.25)));
    assert!(report
        .defaults_used
        .iter()
        .any(|default| default.field == "/rotation"));
}

#[test]
fn arabic_digits_and_both_relative_forms_use_the_ucs() {
    let mut machine = Machine::default();
    machine.set_ucs(Ucs::new([100., 200.], [0., 1.]).unwrap());
    let Step::Command(Command::Draw(DrawSpec::Line { a, b })) =
        machine.step("LINE ٢٫٥،۳ @٤<٩٠").unwrap()
    else {
        panic!("expected a line");
    };
    near(a, [97., 202.5]);
    near(b, [93., 202.5]);
    let Step::Command(Command::Draw(DrawSpec::Line { a, b })) =
        machine.step("L @١،٢ @-۳،۴").unwrap()
    else {
        panic!("expected a line");
    };
    near(a, [91., 203.5]);
    near(b, [87., 200.5]);
}

#[test]
fn polyline_close_and_enter_work_on_inline_and_prompt_lines() {
    let mut machine = Machine::default();
    let closed = command(&mut machine, "PLINE 0,0 10,0 C");
    assert_eq!(closed["points"], json!([[0., 0.], [10., 0.]]));
    assert_eq!(closed["closed"], true);
    assert_eq!(closed["bulges"], json!([]));

    prompt(&mut machine, "PL");
    prompt(&mut machine, "0,0");
    prompt(&mut machine, "10,0");
    let open = command(&mut machine, "");
    assert_eq!(open["points"], closed["points"]);
    assert_eq!(open["closed"], false);
    assert!(!machine.is_active());

    prompt(&mut machine, "PL");
    prompt(&mut machine, "0,0");
    assert!(machine.step("C").unwrap_err().contains("at least 2"));
    assert!(machine.is_active());
    prompt(&mut machine, "5,5");
    assert_eq!(command(&mut machine, "Close")["closed"], true);
}

#[test]
fn registry_prompt_order_preserves_the_existing_typed_draws() {
    let mut machine = Machine::default();
    assert_eq!(
        command(&mut machine, "POLYGON 6 10,10 2"),
        json!({"type": "draw", "kind": "polygon", "centre": [10., 10.], "sides": 6, "radius": 2., "inscribed": true}),
    );
    assert_eq!(command(&mut machine, "POL 6 10,10 2 C")["inscribed"], false);
    assert_eq!(
        command(&mut machine, "DONUT 1 2 5,5"),
        json!({"type": "draw", "kind": "donut", "centre": [5., 5.], "inner": 1., "outer": 2.}),
    );
    assert_eq!(
        command(&mut machine, "RAY 1,1 4,5"),
        json!({"type": "draw", "kind": "ray", "origin": [1., 1.], "dir": [3., 4.]}),
    );
    assert_eq!(
        command(&mut machine, "TRACE 0.5 0,0 4,0"),
        json!({"type": "draw", "kind": "trace", "points": [[0., 0.], [4., 0.]], "width": 0.5}),
    );
    let solid = command(&mut machine, "SOLID 0,0 1,0 0,1");
    assert_eq!(solid["points"].as_array().unwrap().len(), 3);
    assert!(machine.step("SOLID 0,0 1,0 0,1 1,1 2,2").is_err());
}

#[test]
fn new_words_need_only_the_registry_and_support_names_and_options() {
    let mut machine = Machine::default();
    assert_eq!(
        command(&mut machine, "newlayer \"Ground floor walls\""),
        json!({"type": "new_layer", "name": "Ground floor walls"}),
    );
    prompt(&mut machine, "NEWLAYER");
    assert_eq!(
        command(&mut machine, "\"Another layer\""),
        json!({"type": "new_layer", "name": "Another layer"}),
    );
    assert_eq!(
        command(
            &mut machine,
            "NEWLAYOUT \"Ground floor\" \"ISO A1\" portrait"
        ),
        json!({"type": "new_layout", "name": "Ground floor", "paper": "ISO A1", "orientation": "portrait"}),
    );
    assert_eq!(
        command(
            &mut machine,
            "RENAMELAYOUT \"Ground floor\" \"Upper floor\""
        ),
        json!({"type": "rename_layout", "from": "Ground floor", "to": "Upper floor"}),
    );
}

#[test]
fn viewport_corner_prompts_remain_required_in_the_typed_command_form() {
    let mut machine = Machine::default();
    prompt(&mut machine, "MVIEW");
    prompt(&mut machine, "0,0");
    let view = command(&mut machine, "200,160");
    assert_eq!(view["type"], "mview");
    assert_eq!(view["corner1"], json!([0.0, 0.0]));
    assert_eq!(view["corner2"], json!([200.0, 160.0]));
    let view = command(&mut machine, "MVIEW 10,20 210,180");
    assert_eq!(view["corner1"], json!([10.0, 20.0]));
    assert_eq!(view["corner2"], json!([210.0, 180.0]));
}

#[test]
fn errors_preserve_pending_points_and_cancel_keeps_the_last_point() {
    let mut machine = Machine::default();
    assert!(machine
        .step("LINE @1,2 4,4")
        .unwrap_err()
        .contains("no point"));
    assert!(!machine.is_active());
    assert_eq!(machine.last(), None);
    for malformed in ["1e3,0", "NaN,0", "1,2,3", "@2<90<1"] {
        assert!(machine.step(&format!("LINE 0,0 {malformed}")).is_err());
        assert!(!machine.is_active());
        assert_eq!(machine.last(), None);
    }
    prompt(&mut machine, "LINE 1,2");
    assert!(machine.step("4,5,6").is_err());
    assert_eq!(machine.last(), Some([1., 2.]));
    assert_eq!(command(&mut machine, "@3,4")["b"], json!([4., 6.]));
    prompt(&mut machine, "LINE");
    assert!(matches!(machine.step("ESC").unwrap(), Step::Cancelled));
    assert!(!machine.is_active());
    assert_eq!(machine.last(), Some([4., 6.]));
    assert!(!prompt(&mut machine, "").is_empty());
    assert!(machine
        .step("NO_SUCH_WORD")
        .unwrap_err()
        .contains("unknown command"));
}

#[test]
fn undo_a_pending_point_restores_the_relative_reference() {
    let mut machine = Machine::default();
    machine.set_last(Some([20., 20.]));
    prompt(&mut machine, "PLINE");
    prompt(&mut machine, "1,2");
    prompt(&mut machine, "3,4");
    prompt(&mut machine, "U");
    assert_eq!(machine.last(), Some([1., 2.]));
    prompt(&mut machine, "@4,0");
    assert_eq!(
        command(&mut machine, "")["points"],
        json!([[1., 2.], [5., 2.]])
    );
}

#[test]
fn words_for_the_same_command_keep_their_distinct_options() {
    let mut machine = Machine::default();
    assert_eq!(
        command(&mut machine, "LAYOFF \"Furniture plan\""),
        json!({"type": "set_layer_hidden", "name": "Furniture plan", "hidden": true}),
    );
    assert_eq!(command(&mut machine, "LAYON furniture")["hidden"], false);
    assert_eq!(command(&mut machine, "PASTECLIP 1,2")["as_block"], false);
    assert_eq!(command(&mut machine, "PASTEBLOCK 1,2")["as_block"], true);
}

#[test]
fn direction_fields_rotate_without_taking_the_ucs_origin() {
    let mut machine = Machine::default();
    machine.set_ucs(Ucs::new([100., 200.], [0., 1.]).unwrap());
    let ellipse = command(&mut machine, "ELLIPSE 1,2 3,0 0.5");
    assert_eq!(ellipse["centre"], json!([98., 201.]));
    assert_eq!(ellipse["major"], json!([0., 3.]));
    assert_eq!(machine.last(), Some([98., 201.]));
    let ucs = command(&mut machine, "UCS 0,0 1,0");
    assert_eq!(ucs["origin"], json!([100., 200.]));
    assert_eq!(ucs["x_axis"], json!([0., 1.]));
}

#[test]
fn integer_handles_keep_every_digit_and_reject_fractional_or_overflowing_values() {
    let mut machine = Machine::default();
    for text in [
        "9007199254740993",
        "+009007199254740993.000",
        "٩٠٠٧١٩٩٢٥٤٧٤٠٩٩٣",
    ] {
        assert_eq!(
            command(&mut machine, &format!("GEOMETRY {text}"))["handle"],
            json!(9_007_199_254_740_993_u64),
        );
    }
    assert_eq!(
        command(&mut machine, "GEOMETRY 18446744073709551615")["handle"],
        json!(u64::MAX),
    );
    for text in ["9007199254740993.1", "18446744073709551616", "-1", "1e3"] {
        assert!(machine.step(&format!("GEOMETRY {text}")).is_err());
        assert!(!machine.is_active());
    }
}
