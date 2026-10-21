use opencadcodec::{DxfWriter, EntityType};
use qiyas_core::{Drawing, Session};

#[test]
fn standalone_open_does_not_read_or_back_up_a_project_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("benchmark.dxf");
    let mut drawing = Drawing::blank(&path, 6);
    drawing
        .doc
        .add_entity(EntityType::Line(Default::default()))
        .unwrap();
    std::fs::write(&path, DxfWriter::new(&drawing.doc).write_to_vec().unwrap()).unwrap();
    let sidecar = qiyas_core::project_path(&path);
    std::fs::write(&sidecar, b"not a project; do not open").unwrap();
    let session = Session::open_standalone(&path).unwrap();
    assert!(session.takeoff.items.is_empty());
    assert!(session.boq.is_none());
    assert!(!std::path::PathBuf::from(format!("{}.bak", sidecar.display())).exists());
    assert_eq!(
        std::fs::read(sidecar).unwrap(),
        b"not a project; do not open"
    );
}

#[test]
fn standalone_open_refuses_external_picture_definitions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("benchmark.dxf");
    let mut drawing = Drawing::blank(&path, 6);
    drawing
        .doc
        .add_entity(EntityType::Line(Default::default()))
        .unwrap();
    drawing
        .doc
        .add_entity(EntityType::RasterImage(Default::default()))
        .unwrap();
    std::fs::write(&path, DxfWriter::new(&drawing.doc).write_to_vec().unwrap()).unwrap();
    assert!(Session::open_standalone(&path)
        .err()
        .unwrap()
        .to_string()
        .contains("external"));
}
