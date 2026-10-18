//! Make missing-reference fixtures in a new directory. Never accepts an existing directory.
use qiyas_core::{fixtures, Session};
use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("usage: corpus_fixtures <new-directory>"),
    );
    std::fs::create_dir(&dir).expect("use a new directory for generated fixtures");
    fixtures::write_xref_pair(&dir);
    std::fs::remove_file(dir.join("plan-arch.dxf")).unwrap();
    let mut session = Session::open(&fixtures::write_sample(&dir, "dxf")).unwrap();
    let image = fixtures::write_png(&dir, "pic.png");
    session.attach_image(&image, [5.0, 5.0], 20.0).unwrap();
    session.save_as(&dir.join("missing-image.dxf")).unwrap();
    std::fs::remove_file(image).unwrap();
}
