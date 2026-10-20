//! Paper-space viewports: MVIEW makes one on a layout, the frame draws as a line, a click inside
//! picks the viewport, and the window draws model space inside each one with its own view and
//! without the layers it freezes.

use opencadcodec::{EntityType, Handle};
use qiyas_core::fixtures::write_sample;
use qiyas_core::{Command, EntityProp, Outcome, PaperView, Session, Target};
use serde_json::json;

const MODEL: &str = "*Model_Space";

fn t(handle: u64) -> Target {
    Target {
        handle,
        chain: Vec::new(),
    }
}

/// The sample (DWG, metres) on its first layout.
fn on_layout() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dwg");
    let mut s = Session::open(&path).unwrap();
    let paper = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sh| !sh.is_model)
        .unwrap();
    s.set_sheet(&paper.block).unwrap();
    (dir, s)
}

fn mview(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> u64 {
    match s
        .run(Command::MView {
            scale: None,
            centre: None,
            corner1: a,
            corner2: b,
        })
        .unwrap()
    {
        Outcome::Added { handles } => handles[0],
        other => panic!("{other:?}"),
    }
}

fn on_layer(s: &Session, layer: &str) -> Vec<u64> {
    s.drawing
        .doc
        .entities_in_block(MODEL)
        .filter(|e| e.common().layer == layer)
        .map(|e| e.common().handle.value())
        .collect()
}

/// The model batch's segment ranges of `view` (all when it has no spans) that `handles` draw.
fn drawn_in(s: &Session, view: &PaperView, handles: &[u64]) -> usize {
    let batch = s.model_batch();
    batch
        .ranges
        .iter()
        .filter(|r| handles.contains(&r.top))
        .filter(|r| match &view.lines {
            None => true,
            Some(spans) => spans
                .iter()
                .any(|[first, count]| r.first >= *first && r.first + r.count <= first + count),
        })
        .count()
}

#[test]
fn mview_creates_a_viewport_on_the_layout_that_saves() {
    let (dir, mut s) = on_layout();
    let model = s.model_batch().bounds;
    let vp = mview(&mut s, [110.0, 90.0], [10.0, 10.0]);
    let Some(EntityType::Viewport(v)) = s.drawing.doc.get_entity(Handle::new(vp)) else {
        panic!("a viewport");
    };
    assert_eq!((v.center.x, v.center.y), (60.0, 50.0));
    assert_eq!((v.width, v.height), (100.0, 80.0));
    assert!(v.status.is_on);
    assert!(v.id >= 2, "{}", v.id);
    // A view of the model's extents, the whole of them inside the frame.
    let centre = [
        (model[0][0] + model[1][0]) / 2.0,
        (model[0][1] + model[1][1]) / 2.0,
    ];
    assert!((v.view_center.x - centre[0]).abs() < 1e-9);
    assert!((v.view_center.y - centre[1]).abs() < 1e-9);
    let scale = v.height / v.view_height;
    assert!((model[1][0] - model[0][0]) * scale <= 100.0 + 1e-9);
    assert!((model[1][1] - model[0][1]) * scale <= 80.0 + 1e-9);
    assert!((v.custom_scale - scale).abs() < 1e-12);
    assert_eq!(s.info().viewports.len(), 1);
    // The frame draws on the layout.
    assert!(s.batch().ranges.iter().any(|r| r.top == vp));

    // One undo step.
    assert!(s.undo());
    assert!(s.drawing.doc.get_entity(Handle::new(vp)).is_none());
    assert!(s.redo());

    s.save().unwrap();
    let again = Session::open(&dir.path().join("sample.dwg")).unwrap();
    match again.drawing.doc.get_entity(Handle::new(vp)) {
        Some(EntityType::Viewport(v)) => {
            assert_eq!((v.center.x, v.center.y), (60.0, 50.0));
            assert_eq!((v.width, v.height), (100.0, 80.0));
        }
        other => panic!("{other:?}"),
    }

    // Not on model space, and not a flat box.
    s.set_sheet(MODEL).unwrap();
    assert!(s
        .run(Command::MView {
            scale: None,
            centre: None,
            corner1: [0.0, 0.0],
            corner2: [1.0, 1.0],
        })
        .is_err());
}

#[test]
fn a_viewport_draws_model_space_without_the_layers_it_freezes() {
    let (_dir, mut s) = on_layout();
    let vp = mview(&mut s, [10.0, 10.0], [110.0, 90.0]);
    let walls = on_layer(&s, "A-WALL");
    let doors = on_layer(&s, "A-DOOR");

    let views = s.paper_views();
    assert_eq!(views.len(), 1);
    let view = &views[0];
    assert_eq!(view.handle, vp);
    assert_eq!(view.rect, [[10.0, 10.0], [110.0, 90.0]]);
    assert!(view.scale > 0.0);
    // Nothing frozen: every range of the model batch.
    assert!(view.lines.is_none());
    assert!(drawn_in(&s, view, &walls) > 0);

    s.run(Command::SetViewportFrozen {
        viewport: t(vp),
        layer: "A-WALL".into(),
        frozen: true,
    })
    .unwrap();
    let view = &s.paper_views()[0];
    assert!(view.lines.is_some());
    // The model batch still holds the walls (another viewport may show them); this one skips them.
    assert!(s
        .model_batch()
        .ranges
        .iter()
        .any(|r| walls.contains(&r.top)));
    assert_eq!(drawn_in(&s, view, &walls), 0);
    assert!(drawn_in(&s, view, &doors) > 0);

    // The layers the user hides leave every viewport.
    s.set_layer_hidden("A-DOOR", true);
    assert!(!s
        .model_batch()
        .ranges
        .iter()
        .any(|r| doors.contains(&r.top)));

    // A model sheet has no viewports to draw.
    s.set_sheet(MODEL).unwrap();
    assert!(s.paper_views().is_empty());
}

#[test]
fn a_click_inside_a_viewport_picks_the_viewport() {
    let (_dir, mut s) = on_layout();
    let vp = mview(&mut s, [10.0, 10.0], [110.0, 90.0]);
    // Well inside the frame, where the model is drawn but the layout has nothing.
    let picked = s.pick([60.0, 50.0], 0.5).expect("the viewport");
    assert_eq!(picked.top, vp);
    assert_eq!(picked.entity, vp);
    assert!(picked.chain.is_empty());
    assert_eq!(picked.kind.as_deref(), Some("Viewport"));
    // Outside it, nothing.
    assert!(s.pick([200.0, 200.0], 0.5).is_none());
}

#[test]
fn the_palette_sets_a_viewports_scale_and_centre() {
    let (_dir, mut s) = on_layout();
    let vp = mview(&mut s, [10.0, 10.0], [110.0, 90.0]);
    let info = s.entity_info(vp, &[]).unwrap();
    assert!(info.viewport_scale.unwrap() > 0.0);
    s.run(Command::SetProperty {
        target: t(vp),
        prop: EntityProp::ViewportScale,
        value: json!(0.5),
    })
    .unwrap();
    s.run(Command::SetProperty {
        target: t(vp),
        prop: EntityProp::ViewportCentre,
        value: json!([3.0, 4.0]),
    })
    .unwrap();
    let info = s.entity_info(vp, &[]).unwrap();
    assert_eq!(info.viewport_scale, Some(0.5));
    assert_eq!(info.viewport_centre, Some([3.0, 4.0]));
    let view = &s.paper_views()[0];
    assert_eq!((view.scale, view.centre), (0.5, [3.0, 4.0]));
    // Two steps.
    assert!(s.undo());
    assert!(s.undo());
    assert_ne!(s.entity_info(vp, &[]).unwrap().viewport_scale, Some(0.5));
    // A line has no viewport scale.
    let line = on_layer(&s, "A-WALL")[0];
    s.set_sheet(MODEL).unwrap();
    assert!(s
        .run(Command::SetProperty {
            target: t(line),
            prop: EntityProp::ViewportScale,
            value: json!(2.0),
        })
        .is_err());
}

#[test]
fn another_sheet_draws_without_leaving_the_one_on_screen() {
    let (_dir, mut s) = on_layout();
    let layout = s.sheet.clone();
    mview(&mut s, [10.0, 10.0], [110.0, 90.0]);
    let version = s.sheet_batch_version(MODEL);
    let model = s.sheet_batch_bytes_if(MODEL, None, None).unwrap();
    assert_eq!(s.sheet, layout);
    assert_eq!(model, s.model_batch().to_bytes());
    // Unchanged: nothing sent.
    assert!(s.sheet_batch_bytes_if(MODEL, Some(version), None).is_none());
    // The layout's own viewports, seen from model space (a split window's second view).
    s.set_sheet(MODEL).unwrap();
    assert_eq!(s.paper_views_of(&layout).len(), 1);
    assert!(s.paper_views().is_empty());
    // A drawing change draws it again.
    s.run(Command::Draw(qiyas_core::DrawSpec::Line {
        a: [0.0, 0.0],
        b: [1.0, 1.0],
    }))
    .unwrap();
    assert_ne!(s.sheet_batch_version(MODEL), version);
}
