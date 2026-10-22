#[path = "../../qiyas-core/tests/support/measurement_fixture.rs"]
mod fixture;
use qiyas_core::{Boq, BoqItem, Command, ConflictKind, Session, Source};
use serde_json::json;
#[test]
fn generated_boq_compares_to_import_and_changes_after_edit_without_overwriting_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("test.dxf"), 6);
    let (host, _) = fixture::fixture(&mut s);
    fixture::apply(&mut s, fixture::method("compare-test", false));
    let boq = Boq::new(
        "client.xlsx".into(),
        vec![BoqItem {
            code: "S-m2".into(),
            description: "Surface 0.2 wide".into(),
            unit: "m2".into(),
            quantity: 32.68,
            rate: None,
            section: "Test".into(),
            tolerance: None,
            row: 2,
        }],
        0,
    );
    s.run(Command::SetBoq { boq: boq.clone() }).unwrap();
    assert!(qiyas_takeoff::compare_session(&mut s, 2.0, Source::User)
        .unwrap()
        .is_empty());
    fixture::run(
        &mut s,
        json!({"type":"set_object_props","targets":[host],"props":{"copies":2}}),
    );
    let conflicts = qiyas_takeoff::compare_session(&mut s, 2.0, Source::User).unwrap();
    assert!(conflicts
        .iter()
        .any(|c| c.kind == ConflictKind::QuantityMismatch));
    assert_eq!(s.boq, Some(boq));
    assert!(s.takeoff.items.is_empty());
}
