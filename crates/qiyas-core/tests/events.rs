use qiyas_core::{Session, Source};
use serde_json::json;

#[test]
fn events_are_committed_atomic_and_session_bound_in_every_unit() {
    for units in [1, 4, 5, 6] {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new_blank(&dir.path().join("events.dxf"), units);
        let initial = session.events_since(0, None);
        let cursor = initial["cursor"].as_u64().unwrap();
        let id = initial["session"].as_str().unwrap();
        session
            .run_input(
                &json!({"type":"batch","commands":[
                    {"type":"draw","kind":"line","a":[0,0],"b":[1,0]},
                    {"type":"draw","kind":"line","a":[0,1],"b":[1,1]}
                ]}),
                Source::User,
            )
            .unwrap();
        let committed = session.events_since(cursor, Some(id));
        assert_eq!(committed["resync"], false);
        assert_eq!(committed["events"].as_array().unwrap().len(), 1);
        assert_eq!(committed["events"][0]["added"].as_array().unwrap().len(), 2);
        for handle in committed["events"][0]["added"].as_array().unwrap() {
            assert!(session.entity_info(handle.as_u64().unwrap(), &[]).is_ok());
        }
        let cursor = committed["cursor"].as_u64().unwrap();
        assert!(session
            .run_input(
                &json!({"type":"batch","commands":[
                    {"type":"draw","kind":"line","a":[0,2],"b":[1,2]},
                    {"type":"draw","kind":"circle","centre":[0,0],"radius":-1}
                ]}),
                Source::User
            )
            .is_err());
        assert_eq!(session.events_since(cursor, Some(id))["events"], json!([]));
        assert!(session.undo());
        let undo = session.events_since(cursor, Some(id));
        assert_eq!(undo["events"].as_array().unwrap().len(), 1);
        assert_eq!(undo["events"][0]["removed"].as_array().unwrap().len(), 2);
        assert_eq!(
            session.events_since(cursor, Some("other-drawing"))["resync"],
            true
        );
    }
}

#[test]
fn expired_event_cursors_explicitly_require_resync() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("events.dxf"), 6);
    for index in 0..140 {
        session
            .run_input(
                &json!({"type":"draw","kind":"line","a":[0,index],"b":[1,index]}),
                Source::User,
            )
            .unwrap();
    }
    let events = session.events_since(0, None);
    assert_eq!(events["resync"], true);
    assert_eq!(events["events"].as_array().unwrap().len(), 128);
    assert_eq!(
        session.events_since(events["cursor"].as_u64().unwrap(), None)["resync"],
        false
    );
}
