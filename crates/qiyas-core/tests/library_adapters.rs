//! Focused compatibility and Unicode cases for the library adapters.
use opencadcodec::{CadDocument, EntityType, MText, Vector3};
use qiyas_core::validation::{validate_batch, validate_schema};
use qiyas_render::{build_batch_with_text, TextEngine, TextOptions, Visibility, DEFAULT_CHAIN};
use serde_json::json;

#[test]
fn schema_unions_and_intersections_keep_their_actual_semantics() {
    let ambiguous = json!({"oneOf":[{"type":"number"},{"minimum":0}]});
    assert!(!validate_schema(&ambiguous, &json!(2)).valid);
    assert!(validate_schema(&json!({"anyOf":ambiguous["oneOf"]}), &json!(2)).valid);
    let both = json!({"allOf":[
        {"properties":{"n":{"minimum":2}}},
        {"properties":{"n":{"maximum":4}}}
    ]});
    assert!(!validate_schema(&both, &json!({"n":1})).valid);
    assert!(!validate_schema(&both, &json!({"n":5})).valid);
    assert!(validate_schema(&both, &json!({"n":3})).valid);
}

#[test]
fn schema_messages_and_pointers_remain_registry_messages() {
    let schema = json!({"type":"object", "required":["a/b~c"], "properties":{
        "a/b~c":{"type":"number"}, "n":{"minimum":2},
        "choice":{"anyOf":[{"const":"a"},{"const":"b"}]}
    }});
    let report = validate_schema(&schema, &json!({"n":1,"choice":"c"}));
    assert!(report.errors.iter().any(|e| e.pointer == "/a~1b~0c"
        && e.code == "missing_field"
        && e.message == "Missing required field a/b~c"));
    assert!(report
        .errors
        .iter()
        .any(|e| e.pointer == "/n" && e.message == "Number violates minimum 2"));
    assert!(report
        .errors
        .iter()
        .any(|e| e.pointer == "/choice" && e.message == "Choose one of [\"a\",\"b\"]"));
    let report = validate_schema(&schema, &json!({"a/b~c":"x"}));
    assert!(report
        .errors
        .iter()
        .any(|e| e.pointer == "/a~1b~0c" && e.message == "Expected \"number\""));
}

#[test]
fn schema_patterns_and_local_references_are_enforced_without_retrieval() {
    let schema =
        json!({"$defs":{"code":{"type":"string","pattern":"^[A-Z]{2}$"}}, "$ref":"#/$defs/code"});
    assert!(validate_schema(&schema, &json!("AB")).valid);
    assert!(!validate_schema(&schema, &json!("ab")).valid);
    assert!(
        !validate_schema(
            &json!({"$ref":"https://invalid.example/schema"}),
            &json!({})
        )
        .valid
    );
}

#[test]
fn step_values_are_deferred_but_reference_errors_are_not() {
    let commands = vec![
        json!({"type":"draw","kind":"line","a":[0,0],"b":[1,0]}),
        json!({"type":"delete_entities","targets":[{"handle":{"$step":0,"pointer":"/handles/0"}}]}),
    ];
    assert!(validate_batch(&commands, None).valid);
    let mut invalid = commands.clone();
    invalid[1]["targets"][0]["handle"]["$step"] = json!(1);
    let report = validate_batch(&invalid, None);
    assert!(report
        .errors
        .iter()
        .any(|e| e.pointer == "/commands/1/targets/0/handle/$step"));
}

fn wrapped(value: &str, width: f64) -> String {
    let mut doc = CadDocument::new();
    let mut text = MText::with_value(value, Vector3::ZERO).with_height(1.0);
    text.rectangle_width = width;
    doc.add_entity(EntityType::MText(text)).unwrap();
    let mut engine = TextEngine::bundled_only();
    let batch = build_batch_with_text(
        &doc,
        "*Model_Space",
        &Visibility::default(),
        1e-3,
        &mut engine,
        TextOptions {
            arabic_reversed: false,
            px_per_unit: None,
        },
    );
    batch.texts[0].text.clone()
}

#[test]
fn wrapping_preserves_ascii_advances_and_nbsp_glue() {
    let mut engine = TextEngine::bundled_only();
    let width = f64::from(engine.shape_line(&DEFAULT_CHAIN, "door", false).width) + 0.01;
    assert_eq!(wrapped("door door", width), "door\ndoor");
    assert_eq!(wrapped("door\u{00a0}door", width), "door\u{00a0}door");
    assert_eq!(wrapped("door  door", 100.0), "door  door");
    assert_eq!(wrapped("  door ", 100.0), "door ");
}

#[test]
fn visual_reversal_keeps_extended_marks_and_byte_offsets() {
    let pieces = qiyas_render::visual_pieces("X خبطم\u{08f0}");
    assert_eq!(pieces[1].at, 2);
    assert_eq!(pieces[1].text, "م\u{08f0}طبخ");
    let mut engine = TextEngine::bundled_only();
    let text = "a\u{0301} ب\u{064e}";
    let line = engine.shape_line(&DEFAULT_CHAIN, text, false);
    assert!(line
        .glyphs
        .iter()
        .all(|g| text.is_char_boundary(g.cluster as usize)));
}

#[test]
fn literal_search_folds_sigma_and_keeps_combining_marks_inside_words() {
    use qiyas_core::{Command, Outcome, Session, Source, TextScope};
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("search.dxf"), 4);
    session
        .run_input(
            &json!({"type":"text", "at":[0,0], "height":1,
        "value":"Σ σ ς a a\u{0301} aa ."}),
            Source::User,
        )
        .unwrap();
    for (find, whole_word, count) in [("σ", true, 3), ("a", true, 1), (".", true, 1)] {
        let found = session
            .run(Command::FindText {
                find: find.into(),
                scope: TextScope::Sheet,
                match_case: false,
                whole_word,
            })
            .unwrap();
        let Outcome::Found { count: actual, .. } = found else {
            panic!("{found:?}")
        };
        assert_eq!(actual, count);
    }
}
