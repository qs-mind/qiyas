//! Measures a spread of a drawing's entities the way a click does and prints one JSON line each,
//! for `scripts/accuracy/check_accuracy.py` to compare with an independent calculation.
//!
//! `cargo run --release -p qiyas-core --example accuracy_dump -- <drawing> [per-group] [inspection.dxf]`
//! Read only: it never writes to the input drawing. An optional new DXF is an inspection copy. Entities are grouped by type and by whether they sit
//! inside a block; `per-group` (default 25) of each group are taken, evenly spaced. Blocks are
//! followed to depth 5, so a nested block's entities carry their whole chain.

use std::collections::BTreeMap;
use std::fmt::{self, Write};
use std::path::PathBuf;

use opencadcodec::{CadDocument, DxfWriter, EntityType, Handle};
use qiyas_core::{Command, Outcome, Session, XrefSet};

struct Candidate {
    handle: u64,
    chain: Vec<u64>,
    kind: String,
}

fn variant_name(entity: &EntityType) -> String {
    struct UpToParen(String);
    impl Write for UpToParen {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            match s.find('(') {
                Some(i) => {
                    self.0.push_str(&s[..i]);
                    Err(fmt::Error)
                }
                None => {
                    self.0.push_str(s);
                    Ok(())
                }
            }
        }
    }
    let mut out = UpToParen(String::new());
    let _ = write!(out, "{entity:?}");
    out.0
}

fn walk(
    doc: &CadDocument,
    xrefs: &XrefSet,
    block: &str,
    chain: &[u64],
    seen: &mut Vec<String>,
    out: &mut Vec<Candidate>,
) {
    let Some(record) = doc.block_records.get(block) else {
        return;
    };
    for h in record.entity_handles.iter() {
        let Some(e) = doc.get_entity(Handle::new(h.value())) else {
            continue;
        };
        let handle = e.common().handle.value();
        out.push(Candidate {
            handle,
            chain: chain.to_vec(),
            kind: variant_name(e),
        });
        if let EntityType::Insert(insert) = e {
            // An xref's entities are the xref drawing's own, at the top of its model space.
            if let Some(x) = xrefs
                .get(&insert.block_name)
                .and_then(|x| x.drawing.as_ref())
            {
                let mut next = chain.to_vec();
                next.push(handle);
                walk(&x.doc, xrefs, "*Model_Space", &next, &mut Vec::new(), out);
                continue;
            }
            if chain.len() < 5 && !seen.contains(&insert.block_name) {
                seen.push(insert.block_name.clone());
                let mut next = chain.to_vec();
                next.push(handle);
                walk(doc, xrefs, &insert.block_name, &next, seen, out);
                seen.pop();
            }
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = PathBuf::from(
        args.next()
            .expect("usage: accuracy_dump <drawing> [per-group] [inspection.dxf]"),
    );
    let per_group: usize = args.next().map_or(25, |s| s.parse().expect("per-group"));

    let mut session = Session::open(&path).expect("open");
    // Optional inspection copy for an independent Python geometry calculation. This shares
    // Qiyas' DWG reader, so it checks measurement arithmetic, not independent DWG decoding.
    if let Some(export) = args.next() {
        let bytes = DxfWriter::new(&session.drawing.doc)
            .write_to_vec()
            .expect("export DXF");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(export)
            .expect("export path must not exist");
        std::io::Write::write_all(&mut file, &bytes).expect("write DXF");
    }
    if let Some(block_name) = std::env::var_os("QIYAS_COUNT_BLOCK") {
        let block_name = block_name.to_string_lossy().into_owned();
        let layer = std::env::var("QIYAS_COUNT_LAYER").ok();
        if let Ok(name) = std::env::var("QIYAS_HIDE_LAYER") {
            session
                .run(Command::SetLayerHidden { name, hidden: true })
                .expect("hide");
        }
        let result = session
            .run(Command::CountBlock { block_name, layer })
            .expect("count");
        if let Outcome::Measured { measurement: m } = result {
            println!(
                "{}",
                serde_json::json!({"count": m.shown, "handles": m.handles.iter().map(|h| format!("{h:X}")).collect::<Vec<_>>(), "xref_points": m.points.len()})
            );
        }
        return;
    }
    let mut all = Vec::new();
    walk(
        &session.drawing.doc,
        &session.xrefs,
        "*Model_Space",
        &[],
        &mut Vec::new(),
        &mut all,
    );

    let mut groups: BTreeMap<(String, bool), Vec<Candidate>> = BTreeMap::new();
    for c in all {
        if matches!(
            c.kind.as_str(),
            "Text" | "MText" | "AttributeDefinition" | "AttributeEntity" | "Viewport"
        ) {
            continue;
        }
        groups
            .entry((c.kind.clone(), !c.chain.is_empty()))
            .or_default()
            .push(c);
    }

    let units = format!("{:?}", session.drawing.units);
    println!(
        "{}",
        serde_json::json!({"row": "header", "file": path.file_name().unwrap().to_string_lossy(),
            "units": units, "problems": session.drawing.problems.len()})
    );
    for ((kind, nested), list) in groups {
        let n = list.len();
        let take = per_group.min(n);
        eprintln!("{kind:<24} nested={nested:<5} total={n:<8} taken={take}");
        for i in 0..take {
            let c = &list[i * n / take];
            let result = session.run(Command::MeasureEntity {
                handle: c.handle,
                chain: c.chain.clone(),
            });
            let row = match result {
                Ok(Outcome::Measured { measurement: m }) => serde_json::json!({
                    "row": "m", "handle": format!("{:X}", c.handle),
                    "chain": c.chain.iter().map(|h| format!("{h:X}")).collect::<Vec<_>>(),
                    "type": kind, "layer": m.layer, "kind": m.kind,
                    "value": m.value, "unit": m.unit, "shown": m.shown,
                    "self_intersecting": m.self_intersecting,
                }),
                Ok(other) => {
                    serde_json::json!({"row": "other", "handle": format!("{:X}", c.handle),
                    "type": kind, "outcome": format!("{other:?}").chars().take(60).collect::<String>()})
                }
                Err(e) => serde_json::json!({"row": "error", "handle": format!("{:X}", c.handle),
                    "chain": c.chain.iter().map(|h| format!("{h:X}")).collect::<Vec<_>>(),
                    "type": kind, "error": e.to_string()}),
            };
            println!("{row}");
        }
    }
}
