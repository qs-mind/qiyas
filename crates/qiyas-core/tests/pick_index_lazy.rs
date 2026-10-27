//! The click index is 85% of the cost of a batch on a big drawing (0.69 s of 0.82 s on 2.5 million
//! segments), so a drawing that is drawn again after an edit, an undo or a layer toggle must not
//! build it until something is picked: the window draws first and the first snap or pick pays.
//! Picks must answer as before (`pick`, `snap`, `pick_window`).

use qiyas_core::{Session, Source};
use serde_json::json;

fn line(session: &mut Session, y: f64) {
    session
        .run_input(
            &json!({"type":"draw","kind":"line","a":[0.0, y],"b":[10.0, y]}),
            Source::User,
        )
        .unwrap();
}

#[test]
fn the_batch_does_not_build_the_click_index_until_something_is_picked() {
    let mut session = Session::new_blank(std::path::Path::new("lazy.dxf"), 6);
    line(&mut session, 0.0);
    line(&mut session, 5.0);
    assert!(!session.pick_index_built());

    // Drawing it (the batch the window asks for) builds no index.
    assert!(!session.batch().segments.is_empty());
    assert!(session.batch_bytes_if(None, None).is_some());
    assert!(!session.pick_index_built());

    // The first pick builds it, and finds the line.
    let hit = session.pick([5.0, 0.01], 0.1).expect("the line is picked");
    assert!(session.pick_index_built());
    assert!(session.pick([5.0, 2.5], 0.1).is_none());
    let window = session.pick_window([[-1.0, -1.0], [11.0, 6.0]], false);
    assert_eq!(window.len(), 2);
    assert!(window.iter().any(|p| p.top == hit.top));

    // An edit draws the batch again without it; the picks see the new line.
    line(&mut session, 8.0);
    assert!(!session.pick_index_built());
    let _ = session.batch_bytes_if(None, None);
    assert!(!session.pick_index_built());
    assert!(session.pick([5.0, 8.0], 0.1).is_some());
    assert!(session.pick_index_built());

    // So does an undo.
    assert!(session.undo());
    let _ = session.batch_bytes_if(None, None);
    assert!(!session.pick_index_built());
    assert!(session.pick([5.0, 8.0], 0.1).is_none());
}
