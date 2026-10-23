//! Text commands: single-line text and MText placed on the current layer with a text style, text
//! edits (texts, MTexts and attribute values), and find and replace across the sheet or the
//! drawing, each one undo step. New text is stored in reading order and drawn right whatever
//! the "Arabic text is reversed" setting says.

use std::f64::consts::FRAC_PI_2;

use opencadcodec::entities::{AttachmentPoint, Dimension};
use opencadcodec::{CadDocument, Color, EntityType, Handle, TextStyle, Vector3};
use qiyas_core::fixtures::{arabic_text_doc, schedule_doc, write_drawing};
use qiyas_core::{Command, CoreError, Current, Outcome, Session, Target, TextScope};

const MODEL: &str = "*Model_Space";

fn open(doc: &CadDocument, name: &str) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_drawing(doc, &dir.path().join(name));
    let s = Session::open(&path).unwrap();
    (dir, s)
}

/// The schedule drawing with a second text style NOTES made the drawing's current one.
fn styled_doc() -> CadDocument {
    let mut doc = schedule_doc();
    let mut style = TextStyle::new("NOTES");
    style.handle = doc.allocate_handle();
    let handle = style.handle;
    doc.text_styles.add(style).unwrap();
    doc.header.current_text_style_name = "NOTES".to_string();
    doc.header.current_text_style_handle = handle;
    doc
}

fn t(handle: u64) -> Target {
    Target {
        handle,
        chain: Vec::new(),
    }
}

fn added(outcome: Outcome) -> u64 {
    match outcome {
        Outcome::Added { handles } => {
            assert_eq!(handles.len(), 1, "{handles:?}");
            handles[0]
        }
        other => panic!("a text adds one entity, got {other:?}"),
    }
}

fn entity(s: &Session, h: u64) -> EntityType {
    s.drawing.doc.get_entity(Handle::new(h)).cloned().unwrap()
}

fn refused(s: &mut Session, cmd: Command) -> String {
    let before = s.drawing.doc.entities_in_block(MODEL).count();
    let undo = s.can_undo();
    let err = s.run(cmd).unwrap_err();
    assert_eq!(s.drawing.doc.entities_in_block(MODEL).count(), before);
    assert_eq!(s.can_undo(), undo, "a refused command is no step");
    match err {
        CoreError::InvalidCommand(m) => m,
        other => panic!("{other:?}"),
    }
}

fn text_cmd(value: &str, style: Option<&str>) -> Command {
    Command::Text {
        at: [1.0, 2.0],
        height: 2.5,
        rotation: 90.0,
        value: value.to_string(),
        style: style.map(str::to_string),
    }
}

/// The text value of a Text or MText.
fn value_of(s: &Session, h: u64) -> String {
    match entity(s, h) {
        EntityType::Text(t) => t.value,
        EntityType::MText(t) => t.value,
        other => panic!("not a text: {other:?}"),
    }
}

/// The handle of the first Text (or MText) in model space whose value is `value`.
fn find_text(s: &Session, value: &str) -> u64 {
    s.drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| match e {
            EntityType::Text(t) => t.value == value,
            EntityType::MText(t) => t.value == value,
            _ => false,
        })
        .unwrap_or_else(|| panic!("no text {value}"))
        .common()
        .handle
        .value()
}

fn insert_handle(s: &Session) -> u64 {
    s.drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Insert(_)))
        .unwrap()
        .common()
        .handle
        .value()
}

fn attribute(s: &Session, insert: u64, tag: &str) -> String {
    let EntityType::Insert(i) = entity(s, insert) else {
        panic!("an insert");
    };
    i.attributes
        .iter()
        .find(|a| a.tag == tag)
        .unwrap()
        .value
        .clone()
}

#[test]
fn a_text_goes_on_the_current_layer_by_layer_with_the_drawing_s_text_style() {
    let (_dir, mut s) = open(&styled_doc(), "text.dxf");
    s.run(Command::SetCurrent {
        current: Current {
            layer: "A-WALL".to_string(),
            ..Current::default()
        },
    })
    .unwrap();
    let h = added(s.run(text_cmd("D1 door", None)).unwrap());
    let EntityType::Text(text) = entity(&s, h) else {
        panic!("a text");
    };
    assert_eq!(text.value, "D1 door");
    assert_eq!(text.common.layer, "A-WALL");
    assert_eq!(text.common.color, Color::ByLayer);
    assert_eq!(text.common.linetype, "");
    assert_eq!(text.style, "NOTES", "the drawing's current text style");
    assert_eq!(text.height, 2.5);
    assert!((text.rotation - FRAC_PI_2).abs() < 1e-12);
    assert_eq!([text.insertion_point.x, text.insertion_point.y], [1.0, 2.0]);

    // A style chosen by name, as the drawing spells it.
    let h2 = added(s.run(text_cmd("D2", Some("standard"))).unwrap());
    let EntityType::Text(text) = entity(&s, h2) else {
        panic!("a text");
    };
    assert_eq!(text.style, "Standard");

    // One undo step each.
    assert!(s.undo());
    assert!(s.drawing.doc.get_entity(Handle::new(h2)).is_none());
    assert!(s.undo());
    assert!(s.drawing.doc.get_entity(Handle::new(h)).is_none());
    assert!(s.redo());
    assert_eq!(value_of(&s, h), "D1 door", "redo keeps the handle");
}

#[test]
fn without_a_current_style_a_text_takes_standard() {
    let mut doc = schedule_doc();
    doc.header.current_text_style_name = "GONE".to_string();
    let (_dir, mut s) = open(&doc, "text.dxf");
    let h = added(s.run(text_cmd("D1", None)).unwrap());
    let EntityType::Text(text) = entity(&s, h) else {
        panic!("a text");
    };
    assert_eq!(text.style, "Standard");
}

#[test]
fn bad_texts_are_refused() {
    let (_dir, mut s) = open(&styled_doc(), "text.dxf");
    let why = refused(&mut s, text_cmd("D1", Some("NOPE")));
    assert!(why.contains("NOPE"), "{why}");
    refused(&mut s, text_cmd("  ", None));
    let mut flat = text_cmd("D1", None);
    if let Command::Text { height, .. } = &mut flat {
        *height = -1.0;
    }
    refused(&mut s, flat);
    refused(
        &mut s,
        Command::MText {
            at: [0.0, 0.0],
            width: -1.0,
            height: 2.5,
            value: "x".to_string(),
            style: None,
        },
    );
}

#[test]
fn an_mtext_keeps_its_lines_and_braces_as_typed() {
    let (_dir, mut s) = open(&styled_doc(), "mtext.dxf");
    let h = added(
        s.run(Command::MText {
            at: [5.0, 10.0],
            width: 40.0,
            height: 3.5,
            value: "Line 1\nA {b} \\c".to_string(),
            style: None,
        })
        .unwrap(),
    );
    let EntityType::MText(m) = entity(&s, h) else {
        panic!("an mtext");
    };
    assert_eq!(m.value, "Line 1\\PA \\{b\\} \\\\c");
    assert_eq!(m.rectangle_width, 40.0);
    assert_eq!(m.height, 3.5);
    assert_eq!(m.style, "NOTES");
    assert_eq!(m.attachment_point, AttachmentPoint::TopLeft);
    assert_eq!([m.insertion_point.x, m.insertion_point.y], [5.0, 10.0]);
    assert_eq!(m.common.layer, "0");
    assert!(s.undo());
    assert!(s.drawing.doc.get_entity(Handle::new(h)).is_none());
}

/// The glyphs (font, glyph, pen x) drawn for the entity `h`.
fn glyphs(s: &Session, h: u64) -> Vec<(u16, u16, i64)> {
    let batch = s.batch();
    let laid = batch
        .texts
        .iter()
        .find(|t| t.entity == h)
        .unwrap_or_else(|| panic!("text {h:X} is drawn"));
    laid.glyphs
        .iter()
        .map(|g| (g.font, g.glyph, (g.x * 1000.0).round() as i64))
        .collect()
}

#[test]
fn new_arabic_text_is_stored_in_reading_order_and_drawn_right_whatever_the_setting() {
    for ext in ["dxf", "dwg"] {
        let (dir, mut s) = open(&arabic_text_doc(), &format!("arabic.{ext}"));
        let h = added(s.run(text_cmd("قياس الكميات", None)).unwrap());
        let m = added(
            s.run(Command::MText {
                at: [0.0, 10.0],
                width: 0.0,
                height: 0.5,
                value: "غرفة\nمطبخ".to_string(),
                style: None,
            })
            .unwrap(),
        );
        assert_eq!(
            value_of(&s, h),
            "قياس الكميات",
            "logical order, not reversed"
        );
        let living = find_text(&s, "غرفة المعيشة");
        let (text_as_read, mtext_as_read, old_as_read) =
            (glyphs(&s, h), glyphs(&s, m), glyphs(&s, living));

        s.run(Command::SetArabicReversed { on: true }).unwrap();
        assert_ne!(
            glyphs(&s, living),
            old_as_read,
            "the setting still turns the drawing's own text"
        );
        assert_eq!(
            glyphs(&s, h),
            text_as_read,
            "{ext}: Qiyas's text is left alone"
        );
        assert_eq!(glyphs(&s, m), mtext_as_read, "{ext}: Qiyas's mtext too");

        // Saved and opened again (the setting comes back from the project file).
        s.save().unwrap();
        s.save_project().unwrap();
        let path = dir.path().join(format!("arabic.{ext}"));
        let again = Session::open(&path).unwrap();
        assert!(again.arabic_reversed);
        assert_eq!(value_of(&again, h), "قياس الكميات", "{ext}");
        assert_eq!(
            glyphs(&again, h),
            text_as_read,
            "{ext}: the mark survives a save"
        );
        assert_eq!(glyphs(&again, m), mtext_as_read, "{ext}");
    }
}

#[test]
fn edit_text_changes_texts_mtexts_and_attribute_values_one_step_each() {
    let (_dir, mut s) = open(&schedule_doc(), "edit.dxf");
    let d1 = find_text(&s, "D1");
    s.run(Command::EditText {
        target: t(d1),
        value: "D1a".to_string(),
        tag: None,
    })
    .unwrap();
    assert_eq!(value_of(&s, d1), "D1a");

    let note = find_text(&s, "{\\C1;FIRE} RATED\\PDOORS");
    s.run(Command::EditText {
        target: t(note),
        value: "SMOKE\nDOORS".to_string(),
        tag: None,
    })
    .unwrap();
    assert_eq!(value_of(&s, note), "SMOKE\\PDOORS");

    let tag = insert_handle(&s);
    s.run(Command::EditText {
        target: t(tag),
        value: "D10".to_string(),
        tag: Some("mark".to_string()),
    })
    .unwrap();
    assert_eq!(attribute(&s, tag, "MARK"), "D10");

    assert!(s.undo());
    assert_eq!(attribute(&s, tag, "MARK"), "D9");
    assert!(s.undo());
    assert_eq!(value_of(&s, note), "{\\C1;FIRE} RATED\\PDOORS");
    assert!(s.undo());
    assert_eq!(value_of(&s, d1), "D1");
    assert!(!s.can_undo());
}

#[test]
fn edit_text_refuses_what_it_cannot_change() {
    let (_dir, mut s) = open(&schedule_doc(), "edit.dxf");
    let tag = insert_handle(&s);
    let edit = |target: Target, tag: Option<&str>| Command::EditText {
        target,
        value: "X".to_string(),
        tag: tag.map(str::to_string),
    };
    // An insert needs the tag of one of its attributes.
    refused(&mut s, edit(t(tag), None));
    let why = refused(&mut s, edit(t(tag), Some("NOPE")));
    assert!(why.contains("NOPE"), "{why}");
    // A text inside a block is the block's: refused.
    let inside = Target {
        handle: 1,
        chain: vec![tag],
    };
    refused(&mut s, edit(inside, None));
    // Not a text.
    let table = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Table(_)))
        .unwrap()
        .common()
        .handle
        .value();
    refused(&mut s, edit(t(table), None));
}

/// The schedule drawing with a dimension text override "<> DOOR" and, on a paper sheet, a
/// text "DOOR NOTE".
fn find_doc() -> CadDocument {
    let mut doc = schedule_doc();
    let dimension = doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Dimension(_)))
        .unwrap()
        .common()
        .handle;
    if let Some(EntityType::Dimension(d)) = doc.get_entity_mut(dimension) {
        d.base_mut().set_text_override(Some("<> DOOR".to_string()));
    }
    doc.add_paper_space_entity(EntityType::Text(
        opencadcodec::Text::with_value("DOOR NOTE", Vector3::new(0.0, 0.0, 0.0)).with_height(0.2),
    ))
    .unwrap();
    doc
}

fn dim_override(s: &Session) -> String {
    let EntityType::Dimension(d) = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Dimension(_)))
        .unwrap()
        .clone()
    else {
        unreachable!()
    };
    let _: &Dimension = &d;
    d.base().text_override().unwrap_or_default().to_string()
}

fn replace(
    s: &mut Session,
    find: &str,
    with: &str,
    scope: TextScope,
    match_case: bool,
    whole_word: bool,
) -> (usize, Vec<u64>) {
    match s
        .run(Command::FindReplace {
            find: find.to_string(),
            replace: with.to_string(),
            scope,
            match_case,
            whole_word,
        })
        .unwrap()
    {
        Outcome::Replaced { count, handles, .. } => (count, handles),
        other => panic!("{other:?}"),
    }
}

fn paper_text(s: &Session) -> String {
    let paper = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sh| !sh.is_model)
        .unwrap();
    s.drawing
        .doc
        .entities_in_block(&paper.block)
        .find_map(|e| match e {
            EntityType::Text(t) => Some(t.value.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn find_and_replace_counts_every_replacement_in_one_undo_step() {
    let (_dir, mut s) = open(&find_doc(), "find.dxf");
    let title = find_text(&s, "DOOR SCHEDULE");
    let note = find_text(&s, "{\\C1;FIRE} RATED\\PDOORS");

    // Any case, parts of words too: the title, the note's DOORS, the dimension override; the
    // sheet on screen only.
    let (count, handles) = replace(&mut s, "door", "Gate", TextScope::Sheet, false, false);
    assert_eq!(count, 3);
    assert_eq!(value_of(&s, title), "Gate SCHEDULE");
    assert_eq!(value_of(&s, note), "{\\C1;FIRE} RATED\\PGateS");
    assert_eq!(dim_override(&s), "<> Gate");
    assert!(handles.contains(&title) && handles.contains(&note));
    assert_eq!(handles.len(), 3);
    assert_eq!(paper_text(&s), "DOOR NOTE", "another sheet is left alone");

    // One step puts all of them back.
    assert!(s.undo());
    assert_eq!(value_of(&s, title), "DOOR SCHEDULE");
    assert_eq!(value_of(&s, note), "{\\C1;FIRE} RATED\\PDOORS");
    assert_eq!(dim_override(&s), "<> DOOR");
    assert!(!s.can_undo());
    assert!(s.redo());
    assert_eq!(value_of(&s, title), "Gate SCHEDULE");
    assert!(s.undo());

    // Whole words, matching case, the whole drawing: DOORS is no match, the paper text is.
    let (count, _) = replace(&mut s, "DOOR", "DR", TextScope::Drawing, true, true);
    assert_eq!(count, 3);
    assert_eq!(value_of(&s, title), "DR SCHEDULE");
    assert_eq!(value_of(&s, note), "{\\C1;FIRE} RATED\\PDOORS");
    assert_eq!(dim_override(&s), "<> DR");
    assert_eq!(paper_text(&s), "DR NOTE");
    assert!(s.undo());

    // Matching case: "door" is nowhere, and nothing found is no step.
    let (count, handles) = replace(&mut s, "door", "x", TextScope::Drawing, true, false);
    assert_eq!((count, handles.len()), (0, 0));
    assert!(!s.can_undo());
}

#[test]
fn find_and_replace_keeps_mtext_formatting_and_reaches_attributes() {
    let (_dir, mut s) = open(&find_doc(), "find.dxf");
    let note = find_text(&s, "{\\C1;FIRE} RATED\\PDOORS");
    let (count, _) = replace(&mut s, "FIRE", "SMOKE {1}", TextScope::Sheet, true, false);
    assert_eq!(count, 1);
    assert_eq!(value_of(&s, note), "{\\C1;SMOKE \\{1\\}} RATED\\PDOORS");
    // Formatting codes are not text: C1 is inside \C1; only.
    let (count, _) = replace(&mut s, "C1", "C2", TextScope::Sheet, false, false);
    assert_eq!(count, 0);

    let tag = insert_handle(&s);
    let (count, handles) = replace(&mut s, "D9", "D10", TextScope::Sheet, true, true);
    assert_eq!(count, 1);
    assert_eq!(handles, vec![tag], "an attribute names its insert");
    assert_eq!(attribute(&s, tag, "MARK"), "D10");
}

#[test]
fn find_text_lists_the_matches_and_their_sheets_without_changing_anything() {
    let (_dir, mut s) = open(&find_doc(), "find.dxf");
    let title = find_text(&s, "DOOR SCHEDULE");
    let outcome = s
        .run(Command::FindText {
            find: "door".to_string(),
            scope: TextScope::Drawing,
            match_case: false,
            whole_word: true,
        })
        .unwrap();
    let Outcome::Found { count, matches } = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(count, 3, "the title, the dimension and the paper note");
    assert_eq!(matches.len(), 3);
    assert_eq!(matches[0].handle, title);
    assert_eq!(matches[0].sheet, MODEL);
    assert!(matches.iter().any(|m| m.sheet != MODEL));
    assert!(!s.can_undo());
    assert_eq!(value_of(&s, title), "DOOR SCHEDULE");
    // An empty search is refused.
    assert!(s
        .run(Command::FindText {
            find: String::new(),
            scope: TextScope::Sheet,
            match_case: false,
            whole_word: false,
        })
        .is_err());
}

#[test]
fn the_drawing_lists_its_text_styles_and_the_current_one() {
    let (_dir, s) = open(&styled_doc(), "styles.dxf");
    let info = s.info();
    assert!(info.text_styles.contains(&"NOTES".to_string()));
    assert!(info.text_styles.contains(&"Standard".to_string()));
    assert_eq!(info.text_style, "NOTES");
}

#[test]
fn the_wire_forms() {
    let cmd: Command = serde_json::from_str(
        r#"{"type":"text","at":[1,2],"height":2.5,"rotation":0,"value":"A","style":null}"#,
    )
    .unwrap();
    assert!(matches!(cmd, Command::Text { .. }));
    let cmd: Command =
        serde_json::from_str(r#"{"type":"mtext","at":[1,2],"width":10,"height":2.5,"value":"A"}"#)
            .unwrap();
    assert!(matches!(cmd, Command::MText { style: None, .. }));
    let cmd: Command = serde_json::from_str(
        r#"{"type":"edit_text","target":{"handle":5,"chain":[]},"value":"B","tag":"MARK"}"#,
    )
    .unwrap();
    assert!(matches!(cmd, Command::EditText { .. }));
    let cmd: Command = serde_json::from_str(
        r#"{"type":"find_replace","find":"a","replace":"b","scope":"drawing","match_case":true,"whole_word":false}"#,
    )
    .unwrap();
    assert!(matches!(
        cmd,
        Command::FindReplace {
            scope: TextScope::Drawing,
            ..
        }
    ));
}

#[test]
fn an_attribute_may_be_cleared_but_a_text_may_not() {
    let (_dir, mut s) = open(&schedule_doc(), "edit.dxf");
    let tag = insert_handle(&s);
    s.run(Command::EditText {
        target: t(tag),
        value: String::new(),
        tag: Some("MARK".to_string()),
    })
    .unwrap();
    assert_eq!(attribute(&s, tag, "MARK"), "");
    let d1 = find_text(&s, "D1");
    refused(
        &mut s,
        Command::EditText {
            target: t(d1),
            value: " ".to_string(),
            tag: None,
        },
    );
}

#[test]
fn a_replacement_that_would_empty_a_text_or_break_its_line_is_skipped() {
    let (_dir, mut s) = open(&find_doc(), "find.dxf");
    let title = find_text(&s, "DOOR SCHEDULE");
    let note = find_text(&s, "{\\C1;FIRE} RATED\\PDOORS");
    let run = |s: &mut Session, find: &str, with: &str| match s
        .run(Command::FindReplace {
            find: find.to_string(),
            replace: with.to_string(),
            scope: TextScope::Sheet,
            match_case: true,
            whole_word: false,
        })
        .unwrap()
    {
        Outcome::Replaced {
            count,
            handles,
            skipped,
        } => (count, handles, skipped),
        other => panic!("{other:?}"),
    };
    // The whole title would go: left as it is, and nothing else matched, so no step.
    assert_eq!(run(&mut s, "DOOR SCHEDULE", ""), (0, vec![], 1));
    assert_eq!(value_of(&s, title), "DOOR SCHEDULE");
    assert!(!s.can_undo());
    // A line break: the TEXT is skipped, the MText takes it as a new line, the dimension too.
    let (count, handles, skipped) = run(&mut s, "DOOR", "DOOR\nWAY");
    assert_eq!((count, skipped), (2, 1));
    assert!(!handles.contains(&title));
    assert_eq!(value_of(&s, title), "DOOR SCHEDULE");
    assert_eq!(value_of(&s, note), "{\\C1;FIRE} RATED\\PDOOR\\PWAYS");
}
