//! Mizan's eyes: `view_image` draws what the engineer sees on any sheet (pictures and PDF
//! underlays under the lines, model space inside a layout's viewports), marks entities with
//! numbers the caption ties to handles, highlights entities, and says how readable the text is.

use qiyas_ai::{surface, ToolOutput};
use qiyas_core::{Command, EntityFilter, Outcome, Session, Source};
use serde_json::{json, Value};

fn sample(ext: &str) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), ext);
    let session = Session::open(&path).unwrap();
    (dir, session)
}

fn picture(session: &mut Session, input: Value) -> (tiny_skia::Pixmap, String) {
    let out =
        surface::run_session(session, surface::CAPTURE, "picture", &input, Source::Ai).unwrap();
    let ToolOutput::Image { png, caption } = out else {
        panic!("expected an image");
    };
    (tiny_skia::Pixmap::decode_png(&png).expect("a PNG"), caption)
}

fn handles_of(session: &Session, kind: &str) -> Vec<u64> {
    session
        .find_entities(&EntityFilter {
            kind: Some(kind.to_string()),
            ..Default::default()
        })
        .rows
        .iter()
        .map(|r| r.handle)
        .collect()
}

/// Pixels close to `rgb`.
fn pixels_like(pixmap: &tiny_skia::Pixmap, rgb: [u8; 3]) -> usize {
    pixmap
        .pixels()
        .iter()
        .filter(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue()]
                .iter()
                .zip(rgb)
                .all(|(a, b)| a.abs_diff(b) < 24)
        })
        .count()
}

fn dark_pixels(pixmap: &tiny_skia::Pixmap) -> usize {
    pixmap
        .pixels()
        .iter()
        .filter(|p| p.red() < 128 || p.green() < 128 || p.blue() < 128)
        .count()
}

/// A raster image (a scanned plan, a site photo) is part of what Mizan sees: it was left out, so
/// a drawing traced over a scan looked empty.
#[test]
fn a_picture_shows_an_attached_image() {
    let (dir, mut session) = sample("dxf");
    let mut red = tiny_skia::Pixmap::new(20, 20).unwrap();
    red.fill(tiny_skia::Color::from_rgba8(220, 30, 30, 255));
    let file = dir.path().join("scan.png");
    std::fs::write(&file, red.encode_png().unwrap()).unwrap();
    session.attach_image(&file, [200.0, 200.0], 10.0).unwrap();

    let (pixmap, caption) = picture(
        &mut session,
        json!({ "bounds": [[198, 198], [212, 212]], "width": 400 }),
    );
    let red = pixels_like(&pixmap, [220, 30, 30]);
    // 10 of 14 units each way: about half the picture.
    assert!(red > 50_000, "{red} red pixels\n{caption}");
    assert!(caption.contains("scan.png"), "{caption}");
}

/// A layout's viewports show model space, as on screen and on paper.
#[test]
fn a_layout_shows_model_space_inside_its_viewports() {
    let (_dir, mut session) = sample("dwg");
    // A solid square in model space, filling much of its extents.
    session
        .run(Command::Draw(qiyas_core::DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            holes: Vec::new(),
            bulges: Vec::new(),
            pattern: qiyas_core::HatchPattern::Solid,
        }))
        .unwrap();
    let paper = session
        .drawing
        .sheets()
        .into_iter()
        .find(|s| !s.is_model)
        .unwrap();
    session.set_sheet(&paper.block).unwrap();
    let Outcome::Added { .. } = session
        .run(Command::MView {
            scale: None,
            centre: None,
            corner1: [10.0, 10.0],
            corner2: [200.0, 150.0],
        })
        .unwrap()
    else {
        panic!("a viewport");
    };
    let (pixmap, caption) = picture(
        &mut session,
        json!({ "bounds": [[0, 0], [210, 160]], "width": 800 }),
    );
    let dark = dark_pixels(&pixmap);
    assert!(
        dark > 20_000,
        "the model's fill in the viewport: {dark}\n{caption}"
    );
}

/// Another sheet is drawn without changing the one on screen.
#[test]
fn a_picture_of_another_sheet_leaves_the_screen_alone() {
    let (_dir, mut session) = sample("dwg");
    let paper = session
        .drawing
        .sheets()
        .into_iter()
        .find(|s| !s.is_model)
        .unwrap();
    let model = session.sheet.clone();
    session.set_sheet(&paper.block).unwrap();
    session
        .run(Command::MView {
            scale: None,
            centre: None,
            corner1: [10.0, 10.0],
            corner2: [200.0, 150.0],
        })
        .unwrap();
    session.set_sheet(&model).unwrap();

    let (pixmap, caption) = picture(&mut session, json!({ "sheet": paper.name, "width": 300 }));
    assert_eq!(session.sheet, model);
    assert!(caption.contains(&paper.name), "{caption}");
    assert!(dark_pixels(&pixmap) > 0, "the viewport and what it shows");
    let empty = surface::run_session(
        &mut session,
        surface::CAPTURE,
        "picture",
        &json!({ "sheet": "No such sheet" }),
        Source::Ai,
    );
    assert!(empty.is_err());
}

/// Numbered marks tie what Mizan sees to handles it can measure or edit; a highlighted entity
/// is marked first and drawn in violet.
#[test]
fn marks_name_the_entities_and_highlights_come_first() {
    let (_dir, mut session) = sample("dxf");
    let rectangle = handles_of(&session, "LwPolyline")[0];
    let (plain, caption) = picture(&mut session, json!({ "width": 800, "marks": true }));
    assert!(caption.contains("#1 handle "), "{caption}");
    let (lit, caption) = picture(
        &mut session,
        json!({ "width": 800, "marks": true, "highlight": [rectangle] }),
    );
    assert!(
        caption.contains(&format!("#1 handle {rectangle} LwPolyline")),
        "{caption}"
    );
    let violet = |p: &tiny_skia::Pixmap| pixels_like(p, [0x6E, 0x4A, 0x9E]);
    assert!(
        violet(&lit) > violet(&plain) + 100,
        "the rectangle in violet"
    );
    let (none, caption) = picture(&mut session, json!({ "width": 800 }));
    assert!(
        !caption.contains("#1 handle"),
        "no marks unless asked: {caption}"
    );
    let _ = none;
}

/// The caption says how tall the text is in pixels, so Mizan knows when to zoom in to read it.
#[test]
fn the_caption_says_how_tall_the_text_is() {
    let (_dir, mut session) = sample("dxf");
    // "GROUND FLOOR PLAN", 0.5 high at (0, -2).
    let px = |session: &mut Session, width: u32| -> f64 {
        let (_, caption) = picture(
            session,
            json!({ "bounds": [[-1, -3], [11, -1]], "width": width }),
        );
        let at = caption
            .find("text is about ")
            .unwrap_or_else(|| panic!("{caption}"));
        caption[at + "text is about ".len()..]
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap()
    };
    let big = px(&mut session, 1200);
    let small = px(&mut session, 300);
    assert!(big > 3.0 * small, "{big} px at 1200, {small} px at 300");
}
