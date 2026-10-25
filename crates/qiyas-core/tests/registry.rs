//! The command registry's guards: every command describes itself (a doc comment, a category, an
//! example that reads back), names are unique, and a command that says it only reads leaves the
//! session as it was. A command added without these fails here, so the AI, MCP and the command
//! line never see a command they cannot use.

use std::collections::BTreeSet;
use std::path::PathBuf;

use qiyas_core::registry::{effect_of, registry, Effect};
use qiyas_core::{Command, Session};
use serde_json::Value;

#[test]
fn takeoff_kit_is_stable_bounded_and_generated_from_registry_metadata() {
    let kit = qiyas_core::takeoff_kit::kit();
    assert_eq!(kit, qiyas_core::takeoff_kit::kit());
    let rule = &kit["commands"]["measurement.set_method"]["example"]["method"]["rules"][0];
    assert_eq!(rule["kind"], "count");
    assert_eq!(rule["basis"], "geometric");
    assert!(kit["measurement_rules"]
        .as_str()
        .unwrap()
        .contains("rule:<cited note name>"));
    let bytes = serde_json::to_vec(&kit).unwrap().len();
    assert!(
        bytes < qiyas_core::takeoff_kit::MAX_BYTES - 256,
        "kit is {bytes} bytes"
    );
    for info in registry()
        .commands()
        .iter()
        .filter(|c| c.takeoff_kit.is_some())
    {
        assert_eq!(
            kit["commands"][&info.name],
            qiyas_core::takeoff_kit::example_contract(info)
        );
        let example = &kit["commands"][&info.name]["example"];
        assert!(info.examples.contains(example));
        let report = qiyas_core::validation::validate_command(example, None);
        assert!(report.valid, "{}: {}", info.name, report.message());
    }
    assert_eq!(kit["batch"]["commands"].as_array().unwrap().len(), 6);
    let mut session = sample();
    let mut batch = kit["batch"].clone();
    batch["type"] = serde_json::json!("batch");
    session.run_input(&batch, qiyas_core::Source::Ai).unwrap();
}

#[test]
fn missing_fields_return_complete_registry_repairs_for_nested_stubs() {
    for input in [
        serde_json::json!({"type":"define_object_type"}),
        serde_json::json!({"type":"recognition","kind":"define"}),
        serde_json::json!({"type":"measurement","kind":"set_mappings","mappings":[{}]}),
    ] {
        let report = qiyas_core::validation::validate_command(&input, None);
        assert!(!report.valid);
        let repair = &report.repairs[0];
        assert!(repair["required"].as_object().unwrap().len() > 1);
        assert!(qiyas_core::validation::validate_command(&repair["example"], None).valid);
    }
}

const CATEGORIES: [&str; 20] = [
    "attach",
    "blocks",
    "boq",
    "clipboard",
    "dimensions",
    "draw",
    "groups",
    "inquiry",
    "layers",
    "layouts",
    "manage",
    "markup",
    "modify",
    "objects",
    "parametric",
    "records",
    "tables",
    "takeoff",
    "text",
    "view",
];

fn sample() -> Session {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/test-plan.dwg");
    Session::open(&path).expect("sample plan")
}

#[test]
fn every_command_has_a_doc_a_category_and_an_example() {
    let reg = registry();
    let top: Vec<_> = reg
        .commands()
        .iter()
        .filter(|c| !c.name.contains('.'))
        .collect();
    assert!(top.len() >= 100, "{} commands", top.len());
    let mut missing = Vec::new();
    for c in reg.commands() {
        if c.doc.trim().is_empty() {
            missing.push(format!("{}: no doc comment", c.name));
        }
        if !CATEGORIES.contains(&c.category.as_str()) {
            missing.push(format!("{}: category {:?}", c.name, c.category));
        }
        if c.examples.is_empty() {
            missing.push(format!("{}: no example", c.name));
        }
        if !c.kinds.is_empty() && c.examples.is_empty() {
            missing.push(format!("{}: no example of the whole command", c.name));
        }
    }
    assert!(missing.is_empty(), "\n{}", missing.join("\n"));
}

#[test]
fn names_are_unique_and_found_in_any_case() {
    let reg = registry();
    let mut seen = BTreeSet::new();
    for c in reg.commands() {
        assert!(seen.insert(c.name.clone()), "{} twice", c.name);
    }
    assert_eq!(reg.get("Draw.Line").unwrap().name, "draw.line");
    assert_eq!(reg.get("new-layer").unwrap().name, "new_layer");
    assert!(reg
        .get("draw")
        .unwrap()
        .kinds
        .contains(&"hatch".to_string()));
    assert!(reg.get("no_such_command").is_none());
}

#[test]
fn command_line_words_are_unique_upper_case_and_find_their_command() {
    let reg = registry();
    let mut seen = BTreeSet::new();
    for command in reg.commands() {
        for word in &command.words {
            assert!(
                !word.is_empty()
                    && word == &word.to_ascii_uppercase()
                    && !word.chars().any(char::is_whitespace),
                "{}: invalid command word {word:?}",
                command.name
            );
            assert!(seen.insert(word), "{word} belongs to more than one command");
            assert_eq!(
                reg.by_word(&word.to_ascii_lowercase()).unwrap().name,
                command.name
            );
        }
    }
    for (word, command) in [
        ("LINE", "draw.line"),
        ("L", "draw.line"),
        ("PL", "draw.polyline"),
        ("DI", "measure_distance"),
        ("DISTANCE", "measure_distance"),
        ("AA", "measure_area"),
        ("CO", "modify.copy"),
        ("CP", "modify.copy"),
        ("ED", "edit_text"),
        ("DDEDIT", "edit_text"),
        ("NEWLAYER", "new_layer"),
        ("NEWLAYOUT", "new_layout"),
        ("RENAMELAYOUT", "rename_layout"),
    ] {
        assert_eq!(reg.by_word(word).unwrap().name, command);
    }
    assert!(reg.by_word("no_such_word").is_none());
    for word in reg.words() {
        let command = reg.get(&word.command).unwrap();
        assert!(command.words.contains(&word.word));
        assert_eq!(word.summary, command.summary);
    }
}

#[test]
fn prompt_overrides_name_fields_of_their_command() {
    for command in registry().commands() {
        for prompt in &command.prompts {
            let field = prompt
                .as_str()
                .or_else(|| prompt["field"].as_str())
                .unwrap_or_else(|| panic!("{}: prompt needs a field", command.name));
            assert!(
                command.schema["properties"].get(field).is_some(),
                "{}: prompt names unknown field {field:?}",
                command.name
            );
            if let Some(values) = prompt.get("by_word").and_then(Value::as_object) {
                for word in values.keys() {
                    assert!(
                        command.words.contains(word),
                        "{}: prompt names unknown word {word:?}",
                        command.name
                    );
                }
            }
        }
    }
}

/// Read a window tool's exported string union or constant array, without keeping another
/// list of tool names in the core test.
fn window_names(source: &'static str, declaration: &str) -> BTreeSet<&'static str> {
    let body = source
        .split_once(declaration)
        .unwrap_or_else(|| panic!("missing {declaration}"))
        .1
        .split_once(";\n")
        .unwrap()
        .0;
    body.split('"').skip(1).step_by(2).collect()
}

#[test]
fn every_ribbon_button_names_a_command_or_a_window_tool() {
    let ribbon: Value =
        serde_json::from_str(include_str!("../../../src/shell/ribbon.default.json")).unwrap();
    let mut window = window_names(
        include_str!("../../../src/viewport/Viewport.tsx"),
        "export type Tool =",
    );
    for (source, declaration) in [
        (
            include_str!("../../../src/tools/drawTools.ts"),
            "export const DRAW_TOOLS =",
        ),
        (
            include_str!("../../../src/tools/modifyTools.ts"),
            "export const MODIFY_TOOLS =",
        ),
        (
            include_str!("../../../src/tools/editTools.ts"),
            "export const EDIT_TOOLS =",
        ),
        (
            include_str!("../../../src/tools/textTools.ts"),
            "export type TextKind =",
        ),
        (
            include_str!("../../../src/tools/dimTools.ts"),
            "export const DIM_TOOLS =",
        ),
        (
            include_str!("../../../src/tools/constraintTools.ts"),
            "export const CONSTRAINT_TOOLS =",
        ),
    ] {
        window.extend(window_names(source, declaration));
    }
    let shell = include_str!("../../../src/shell/Ribbon.tsx");
    let actions = shell
        .split_once("function actions(")
        .unwrap()
        .1
        .split_once("\n  };")
        .unwrap()
        .0;
    window.extend(actions.lines().filter_map(|line| {
        let (name, handler) = line.trim().split_once(':')?;
        handler.trim().starts_with("props.").then_some(name)
    }));
    // These controls have their own rendering and handlers instead of the action table.
    window.extend(["views", "groupSelect", "ucsIcon", "arabicReversed"]);
    let reg = registry();
    for tab in ribbon["tabs"].as_array().unwrap() {
        for group in tab["groups"].as_array().unwrap() {
            for button in group["buttons"].as_array().unwrap() {
                let name = button["command"].as_str().unwrap();
                assert!(
                    reg.by_word(name).is_some() || reg.get(name).is_some() || window.contains(name),
                    "ribbon command {name:?} is neither a registry command nor a window tool"
                );
            }
        }
    }
}

#[test]
fn every_example_reads_back_as_its_command() {
    for c in registry().commands() {
        for e in &c.examples {
            let parsed: Command = serde_json::from_value(e.clone())
                .unwrap_or_else(|err| panic!("{}: example {e} does not read: {err}", c.name));
            let wire = serde_json::to_value(&parsed).unwrap();
            let base = c.name.split('.').next().unwrap();
            assert_eq!(wire["type"], base, "{}", c.name);
            if let Some(kind) = c.name.split('.').nth(1) {
                assert_eq!(wire["kind"], kind, "{}", c.name);
            }
        }
    }
}

#[test]
fn schemas_are_self_contained_and_name_their_type() {
    for c in registry().commands() {
        let text = c.schema.to_string();
        assert!(!text.contains("\"$ref\""), "{} keeps a $ref", c.name);
        assert!(!text.contains("\"x-"), "{} keeps an extension", c.name);
        let base = c.name.split('.').next().unwrap();
        let tag = &c.schema["properties"]["type"];
        let named = tag["const"] == base || tag["enum"][0] == base;
        assert!(named, "{}: {}", c.name, c.schema);
    }
}

#[test]
fn effects_and_undo_parts_follow_the_command() {
    let reg = registry();
    let get = |n: &str| reg.get(n).unwrap();
    assert_eq!(get("draw.line").effect, Effect::Edit);
    assert_eq!(get("draw.line").undo.as_deref(), Some("drawing"));
    assert_eq!(get("new_layer").undo.as_deref(), Some("drawing+tables"));
    assert_eq!(get("measure_distance").undo.as_deref(), Some("takeoff"));
    assert_eq!(get("set_layer_hidden").effect, Effect::View);
    assert_eq!(get("write_block").effect, Effect::File);
    assert_eq!(get("edit_block_in_place").effect, Effect::Session);
    assert_eq!(get("id_point").effect, Effect::Read);
    assert!(get("delete_entities").destructive);
    assert!(!get("batch").batchable);
    assert!(get("set_conflicts").internal);
    let dry: Command = serde_json::from_str(r#"{"type":"purge","dry_run":true}"#).unwrap();
    assert_eq!(effect_of(&dry), Effect::Read);
    let real: Command = serde_json::from_str(r#"{"type":"purge","dry_run":false}"#).unwrap();
    assert_eq!(effect_of(&real), Effect::Edit);
}

/// What a read must not change: the drawing, the take-off, the history and the view.
fn fingerprint(s: &Session) -> Value {
    let info = serde_json::to_value(s.info()).unwrap();
    serde_json::json!({
        "batch": s.batch_version(),
        "dirty": s.is_dirty(),
        "takeoff": serde_json::to_value(&s.takeoff).unwrap(),
        "can_undo": s.can_undo(),
        "can_redo": s.can_redo(),
        "info": info,
    })
}

#[test]
fn a_command_that_reads_leaves_the_session_as_it_was() {
    let mut session = sample();
    let before = fingerprint(&session);
    let mut ran = 0;
    for c in registry().commands() {
        for e in &c.examples {
            let command: Command = serde_json::from_value(e.clone()).unwrap();
            if effect_of(&command) != Effect::Read || c.needs.is_some() {
                continue;
            }
            // Handles in examples may not exist here: an error is fine, a change is not.
            let _ = session.run(command);
            ran += 1;
            assert_eq!(
                fingerprint(&session),
                before,
                "{} changed the session: {e}",
                c.name
            );
        }
    }
    assert!(ran >= 8, "only {ran} reads ran");
}

#[test]
fn the_index_lists_every_public_command_once_and_stays_small() {
    let reg = registry();
    let index = reg.index_text();
    for c in reg
        .commands()
        .iter()
        .filter(|c| !c.internal && !c.name.contains('.'))
    {
        let word = format!(" {}", c.name);
        let found = index.contains(&format!("{word},"))
            || index.contains(&format!("{word} ("))
            || index.contains(&format!("{word}\n"))
            || index.ends_with(&word);
        assert!(found, "{} missing from the index", c.name);
    }
    assert!(
        !index.contains("set_conflicts"),
        "internal commands stay out"
    );
    // The index is part of the cached system prompt; new commands grow it slowly.
    assert!(index.len() < 4_500, "index is {} characters", index.len());
    assert_eq!(index, reg.index_text(), "deterministic");
}

#[test]
fn search_finds_commands_by_what_they_do() {
    let reg = registry();
    let names = |q: &str| -> Vec<String> {
        reg.search(q, 5)
            .into_iter()
            .map(|c| c.name.clone())
            .collect()
    };
    assert_eq!(
        names("hatch").first().map(String::as_str),
        Some("draw.hatch")
    );
    assert!(names("layer lineweight").iter().any(|n| n == "set_layer"));
    assert!(names("viewport").iter().any(|n| n == "mview"));
    assert!(names("set_conflicts").is_empty(), "internal");
}
