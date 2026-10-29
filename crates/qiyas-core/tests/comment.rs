//! Comment clouds: a revision cloud and its label on the COMMENTS layer as one undo step, listed
//! with their text; the label's edits win; they survive a save in DWG and DXF.

use opencadcodec::{EntityType, Handle};
use qiyas_core::fixtures::write_sample;
use qiyas_core::{Command, CoreError, Outcome, Session, Target, COMMENT_LAYER};

const MODEL: &str = "*Model_Space";
const SQUARE: [[f64; 2]; 4] = [[0.0, 0.0], [4000.0, 0.0], [4000.0, 3000.0], [0.0, 3000.0]];

fn session(ext: &str) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let session = Session::open(&write_sample(dir.path(), ext)).unwrap();
    (dir, session)
}

fn comment(s: &mut Session, text: &str) -> Vec<u64> {
    let cmd = Command::Comment {
        points: SQUARE.to_vec().into(),
        text: text.to_string(),
        arc_len: None,
    };
    match s.run(cmd).unwrap() {
        Outcome::Added { handles } => handles,
        other => panic!("{other:?}"),
    }
}

fn list(s: &mut Session) -> Vec<qiyas_core::CommentInfo> {
    match s.run(Command::ListComments).unwrap() {
        Outcome::Comments { comments } => comments,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_comment_is_a_cloud_and_a_label_on_the_comments_layer() {
    let (_dir, mut s) = session("dxf");
    let handles = comment(&mut s, "Check the door width");
    assert_eq!(handles.len(), 2);
    let cloud = s.drawing.doc.get_entity(Handle::new(handles[0])).unwrap();
    let EntityType::LwPolyline(p) = cloud else {
        panic!("{cloud:?}")
    };
    assert!(p.is_closed && p.vertices.len() > 8, "arcs all round");
    assert!(p.vertices.iter().all(|v| v.bulge > 0.0));
    assert_eq!(p.common.layer, COMMENT_LAYER);
    let label = s.drawing.doc.get_entity(Handle::new(handles[1])).unwrap();
    let EntityType::MText(t) = label else {
        panic!("{label:?}")
    };
    assert_eq!(t.common.layer, COMMENT_LAYER);
    assert!(t.insertion_point.x > 4000.0, "beside the cloud");
    let layer = s.drawing.doc.layers.get(COMMENT_LAYER).unwrap();
    assert_eq!(layer.color, opencadcodec::Color::Index(1));

    let comments = list(&mut s);
    assert_eq!(comments.len(), 1);
    let c = &comments[0];
    assert_eq!(c.text, "Check the door width");
    assert_eq!((c.handle, c.label), (handles[0], Some(handles[1])));
    assert_eq!(c.sheet, MODEL);
    assert!(
        c.bounds[0][0] <= 0.0 && c.bounds[1][0] >= 4000.0,
        "{:?}",
        c.bounds
    );

    // One step: the cloud, its label and the new layer all go.
    assert!(s.undo());
    for h in &handles {
        assert!(s.drawing.doc.get_entity(Handle::new(*h)).is_none());
    }
    let names: Vec<_> = s
        .drawing
        .doc
        .layers
        .iter()
        .map(|l| l.name.clone())
        .collect();
    assert!(!s.drawing.doc.layers.contains(COMMENT_LAYER), "{names:?}");
    assert!(list(&mut s).is_empty());
    assert!(s.redo());
    assert_eq!(list(&mut s).len(), 1);
}

#[test]
fn the_current_layer_is_kept_and_a_blank_comment_refused() {
    let (_dir, mut s) = session("dxf");
    let current = s.current.clone();
    comment(&mut s, "One");
    assert_eq!(s.current, current);
    let err = s
        .run(Command::Comment {
            points: SQUARE.to_vec().into(),
            text: "   ".to_string(),
            arc_len: None,
        })
        .unwrap_err();
    assert!(matches!(err, CoreError::InvalidCommand(_)), "{err:?}");
    let err = s
        .run(Command::Comment {
            points: SQUARE[..2].to_vec().into(),
            text: "Two points".to_string(),
            arc_len: None,
        })
        .unwrap_err();
    assert!(matches!(err, CoreError::InvalidCommand(_)), "{err:?}");
    assert_eq!(list(&mut s).len(), 1);
}

#[test]
fn an_edited_label_is_the_comment_and_a_deleted_one_falls_back() {
    let (_dir, mut s) = session("dxf");
    let handles = comment(&mut s, "Old");
    s.run(Command::EditText {
        target: Target {
            handle: handles[1],
            chain: Vec::new(),
        },
        value: "New text\nsecond line".to_string(),
        tag: None,
    })
    .unwrap();
    assert_eq!(list(&mut s)[0].text, "New text\nsecond line");
    s.run(Command::DeleteEntities {
        targets: vec![Target {
            handle: handles[1],
            chain: Vec::new(),
        }],
    })
    .unwrap();
    let c = &list(&mut s)[0];
    assert_eq!((c.label, c.text.as_str()), (None, "Old"));
}

#[test]
fn comments_survive_a_save_in_dwg_and_dxf() {
    for ext in ["dwg", "dxf"] {
        let (dir, mut s) = session(ext);
        comment(&mut s, "تحقق من عرض الباب");
        let path = dir.path().join(format!("saved.{ext}"));
        s.save_as(&path).unwrap();
        let mut again = Session::open(&path).unwrap();
        let comments = list(&mut again);
        assert_eq!(comments.len(), 1, "{ext}");
        assert_eq!(comments[0].text, "تحقق من عرض الباب", "{ext}");
        assert!(comments[0].label.is_some(), "{ext}");
    }
}

#[test]
fn a_copied_comment_keeps_its_own_label() {
    let (_dir, mut s) = session("dxf");
    let handles = comment(&mut s, "Original");
    let targets = handles
        .iter()
        .map(|&handle| Target {
            handle,
            chain: Vec::new(),
        })
        .collect();
    let text = match s
        .run(Command::CopyToClipboard {
            targets,
            base: Some([0.0, 0.0]),
        })
        .unwrap()
    {
        Outcome::Clipboard { text, .. } => text,
        other => panic!("{other:?}"),
    };
    let copy = match s
        .run(Command::PasteFromClipboard {
            at: [10000.0, 0.0],
            text: Some(text),
            as_block: false,
            height: None,
        })
        .unwrap()
    {
        Outcome::Added { handles } => handles,
        other => panic!("{other:?}"),
    };
    let copy_label = copy
        .iter()
        .copied()
        .find(|h| {
            matches!(
                s.drawing.doc.get_entity(Handle::new(*h)),
                Some(EntityType::MText(_))
            )
        })
        .unwrap();
    s.run(Command::EditText {
        target: Target {
            handle: copy_label,
            chain: Vec::new(),
        },
        value: "The copy".to_string(),
        tag: None,
    })
    .unwrap();
    let comments = list(&mut s);
    assert_eq!(comments.len(), 2);
    let original = comments.iter().find(|c| c.handle == handles[0]).unwrap();
    let copied = comments.iter().find(|c| c.handle != handles[0]).unwrap();
    assert_eq!(
        (original.label, original.text.as_str()),
        (Some(handles[1]), "Original")
    );
    assert_eq!(
        (copied.label, copied.text.as_str()),
        (Some(copy_label), "The copy")
    );
}

#[test]
fn comment_accepts_a_caller_layer() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("layer.dxf"), 6);
    let report = s.run_input(&serde_json::json!({"type":"comment","points":[[0,0],[4,0],[4,3],[0,3]],"text":"Review / مراجعة","layer":"Review notes"}),qiyas_core::Source::User).unwrap();
    let Outcome::Added { handles } = report.outcome else {
        panic!("comment")
    };
    assert!(handles.iter().all(|h| {
        s.drawing
            .doc
            .get_entity(opencadcodec::Handle::new(*h))
            .unwrap()
            .common()
            .layer
            == "Review notes"
    }));
    assert_eq!(s.current.layer, "0");
}
