//! Deterministic projects built through the same command bus as the window.
//! The arithmetic here is independent of Qiyas's measurement implementation.

mod pdf;

use std::fs;
use std::path::Path;

use opencadcodec::DxfWriter;
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

type Result<T> = std::result::Result<T, String>;
const TYPES: [&str; 6] = [
    "office",
    "warehouse",
    "apartment",
    "mep",
    "external",
    "part",
];
const VARIANTS: [&str; 12] = [
    "exploded",
    "wrong-layers",
    "merged-layers",
    "overlaps",
    "gaps",
    "line-text",
    "inches",
    "missing-schedule",
    "seeded-errors",
    "revision-b",
    "vector-pdf",
    "scanned-pdf",
];

struct Project {
    session: Session,
    truth: Value,
    scale: f64,
    region: Vec<[f64; 2]>,
    region_handle: u64,
    line_handle: u64,
    inserts: Vec<u64>,
    texts: Vec<(u64, [f64; 2], String)>,
    table: u64,
    block: String,
}

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn safe_output(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(format!(
                    "benchmark output cannot be a symlink: {}",
                    ancestor.display()
                ))
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(error(e)),
        }
    }
    Ok(())
}
fn make_directory(path: &Path) -> Result<()> {
    safe_output(path)?;
    fs::create_dir_all(path).map_err(error)
}
fn write_json(path: &Path, value: &Value) -> Result<()> {
    safe_output(path)?;
    fs::write(path, serde_json::to_vec_pretty(value).map_err(error)?).map_err(error)
}
fn run(session: &mut Session, value: Value) -> Result<Vec<u64>> {
    let outcome = session
        .run_input(&value, Source::User)
        .map_err(error)?
        .outcome;
    Ok(match outcome {
        Outcome::Added { handles } | Outcome::Exploded { handles, .. } => handles,
        Outcome::Batch { added, .. } => added,
        _ => Vec::new(),
    })
}
fn draw(session: &mut Session, spec: Value) -> Result<u64> {
    let mut spec = spec;
    spec["type"] = json!("draw");
    run(session, spec)?
        .first()
        .copied()
        .ok_or("Draw returned no handle".into())
}
fn layer(session: &mut Session, name: &str) -> Result<()> {
    if !session.layers().iter().any(|l| l.name == name) {
        run(session, json!({"type":"new_layer","name":name}))?;
    }
    run(
        session,
        json!({"type":"set_current","current":{"layer":name}}),
    )?;
    Ok(())
}
fn handles(values: &[u64]) -> Vec<String> {
    values.iter().map(|h| format!("{h:X}")).collect()
}
fn targets(values: &[u64]) -> Vec<Value> {
    values.iter().map(|h| json!({"handle":h})).collect()
}
fn polygon_area(points: &[[f64; 2]]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum::<f64>()
        .abs()
        / 2.0
}
fn length(points: &[[f64; 2]]) -> f64 {
    points
        .windows(2)
        .map(|p| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]))
        .sum()
}
fn item(
    id: &str,
    description: &str,
    kind: &str,
    quantity: f64,
    source: &[u64],
    position: [f64; 2],
) -> Value {
    let unit = match kind {
        "count" => "pcs",
        "length" => "m",
        "area" => "m2",
        _ => "m3",
    };
    json!({"id":id,"description":description,"kind":kind,"unit":unit,"quantity":quantity,
        "source_objects":handles(source),"basis":"geometric",
        "tolerance_class":if kind=="count" {"count"} else {"geometry"},"position":position})
}
fn rectangle(x: f64, y: f64, w: f64, h: f64) -> Vec<[f64; 2]> {
    vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
}

fn build(path: &Path, typ: &str, seed: u64, inches: bool) -> Result<Project> {
    let scale = if inches { 1.0 / 0.0254 } else { 1.0 };
    let mut session = Session::new_blank(path, if inches { 1 } else { 6 });
    // SplitMix64 fixes the sequence across Rust versions and platforms.
    let mut state = seed;
    let mut sample = |lo: f64, step: f64, n: u64| {
        state = state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        lo + ((z ^ (z >> 31)) % n) as f64 * step
    };
    let w = sample(4.0, 0.25, 17);
    let h = sample(3.0, 0.25, 13);
    let n = sample(3.0, 1.0, 5) as usize;
    let depth = sample(0.1, 0.025, 9) * if typ == "part" { 0.1 } else { 1.0 };
    let surface_factor = match typ {
        "part" => depth,
        "mep" => 0.8,
        "external" => 1.2,
        _ => 2.4,
    };
    let surface_description = match typ {
        "part" => "Coated cut-face area",
        "mep" => "Pipe insulation surface",
        "external" => "Drain trench side lining",
        _ => "Declared wall lining area",
    };
    let allowance_description = match typ {
        "part" => "Provisional fabrication stock with 10% allowance",
        "mep" => "Provisional plant base with 10% allowance",
        _ => "Provisional excavation with 10% working allowance",
    };
    let (names, shapes, route, block) = match typ {
        "office" => (
            vec!["Reception", "Meeting room", "Workroom"],
            vec![
                rectangle(0.0, 0.0, w, h),
                rectangle(w + 0.3, 0.0, w * 0.8, h),
                rectangle(0.0, h + 0.3, w * 1.8 + 0.3, h),
            ],
            vec![[0.0, -1.0], [w, -1.0], [w, -h]],
            "DESK",
        ),
        "warehouse" => (
            vec!["Storage hall", "Loading bay", "Service room"],
            vec![
                rectangle(0.0, 0.0, w * 5.0, h * 4.0),
                rectangle(w * 5.0 + 0.4, 0.0, w * 2.0, h * 2.0),
                rectangle(w * 5.0 + 0.4, h * 2.0 + 0.4, w, h),
            ],
            vec![[0.0, -2.0], [w * 5.0, -2.0], [w * 5.0, -h * 2.0]],
            "COLUMN",
        ),
        "apartment" => (
            vec!["Living room", "Bedroom", "Kitchen", "Bathroom"],
            vec![
                vec![
                    [0.0, 0.0],
                    [w * 1.5, 0.0],
                    [w * 1.5, h * 0.6],
                    [w, h * 0.6],
                    [w, h],
                    [0.0, h],
                ],
                rectangle(w * 1.5 + 0.2, 0.0, w, h),
                rectangle(0.0, h + 0.2, w * 0.7, h * 0.7),
                rectangle(w * 0.7 + 0.2, h + 0.2, w * 0.7, h * 0.7),
            ],
            vec![[0.0, -0.5], [w * 2.5, -0.5], [w * 2.5, -h]],
            "DOOR",
        ),
        "mep" => (
            vec!["Duct fabrication", "Plant plinth", "Insulation panel"],
            vec![
                vec![
                    [0.0, 0.0],
                    [w, 0.0],
                    [w, 0.6],
                    [1.0, 0.6],
                    [1.0, h],
                    [0.0, h],
                ],
                rectangle(w + 1.0, 0.0, 2.0, 1.5),
                rectangle(w + 1.0, 2.0, w * 0.5, h * 0.5),
            ],
            vec![[0.0, -2.0], [w, -2.0], [w, -h - 2.0], [w * 2.0, -h - 2.0]],
            "VALVE",
        ),
        "external" => (
            vec!["Road paving", "Footpath", "Landscape bed"],
            vec![
                vec![[0.0, 0.0], [w * 5.0, 0.0], [w * 5.0 + 2.0, h], [2.0, h]],
                rectangle(0.0, h + 0.3, w * 5.0, 1.5),
                rectangle(0.0, h + 2.1, w * 2.0, h),
            ],
            vec![[0.0, -1.0], [w * 2.0, -1.0], [w * 3.0, -h], [w * 5.0, -h]],
            "MANHOLE",
        ),
        "part" => (
            vec!["Bracket blank", "Gusset plate", "Cover plate"],
            vec![
                vec![
                    [0.0, 0.0],
                    [w * 0.1, 0.0],
                    [w * 0.1, 0.04],
                    [0.04, 0.04],
                    [0.04, h * 0.1],
                    [0.0, h * 0.1],
                ],
                vec![[1.0, 0.0], [1.0 + w * 0.1, 0.0], [1.0, h * 0.1]],
                rectangle(2.0, 0.0, w * 0.1, h * 0.1),
            ],
            vec![[0.0, -0.1], [w * 0.1, -0.1], [w * 0.1, -h * 0.1]],
            "FASTENER",
        ),
        _ => return Err(format!("Unknown project type {typ}")),
    };
    let to_drawing = |p: [f64; 2]| [p[0] * scale, p[1] * scale];
    let mut items = Vec::new();
    let mut texts = Vec::new();
    let mut areas = Vec::new();
    for (i, (shape, name)) in shapes.iter().zip(&names).enumerate() {
        layer(&mut session, "A-ROOM")?;
        let points: Vec<_> = shape.iter().copied().map(to_drawing).collect();
        let handle = draw(
            &mut session,
            json!({"kind":"polyline","points":points,"closed":true}),
        )?;
        let pos = [shape[0][0] + 0.03, shape[0][1] + 0.03];
        items.push(item(
            &format!("area-{}", i + 1),
            name,
            "area",
            polygon_area(shape),
            &[handle],
            pos,
        ));
        areas.push(handle);
        layer(&mut session, "A-ROOM-NAME")?;
        let title = format!("{} {}", i + 1, name);
        let text_height = if typ == "part" { 0.014 } else { 0.16 };
        let text = run(
            &mut session,
            json!({"type":"text","at":to_drawing(pos),"height":text_height*scale,"value":title}),
        )?[0];
        texts.push((text, pos, title));
    }
    layer(&mut session, if typ == "mep" { "M-PIPE" } else { "A-LINE" })?;
    let line_handle = draw(
        &mut session,
        json!({"kind":"polyline","points":route.iter().copied().map(to_drawing).collect::<Vec<_>>(),"closed":false}),
    )?;
    items.push(item(
        "length-1",
        match typ {
            "mep" => "Pipe centreline",
            "external" => "Drain centreline",
            "part" => "Cutting path",
            _ => "Partition centreline",
        },
        "length",
        length(&route),
        &[line_handle],
        route[0],
    ));
    layer(&mut session, "A-EQUIPMENT")?;
    let symbol_size = if typ == "part" { 0.012 } else { 0.25 };
    let a = draw(
        &mut session,
        json!({"kind":"line","a":[0.0,0.0],"b":[symbol_size*scale,symbol_size*scale]}),
    )?;
    let b = draw(
        &mut session,
        json!({"kind":"line","a":[0.0,symbol_size*scale],"b":[symbol_size*scale,0.0]}),
    )?;
    run(
        &mut session,
        json!({"type":"create_block","name":block,"base":[0,0],"targets":targets(&[a,b]),"convert_to_insert":false}),
    )?;
    let mut inserts = Vec::new();
    for i in 0..n {
        let pos = if typ == "part" {
            [0.03 + i as f64 * 0.035, 0.015]
        } else {
            [0.6 + i as f64 * 0.7, 0.6]
        };
        inserts.push(
            run(
                &mut session,
                json!({"type":"insert_block","name":block,"at":to_drawing(pos)}),
            )?[0],
        );
    }
    items.push(item(
        "count-1",
        block,
        "count",
        n as f64,
        &inserts,
        if typ == "part" {
            [0.03, 0.015]
        } else {
            [0.6, 0.6]
        },
    ));
    let mut volume = item(
        "volume-1",
        if typ == "part" {
            "Plate volume"
        } else {
            "Slab volume"
        },
        "volume",
        polygon_area(&shapes[0]) * depth,
        &[areas[0]],
        [0.03, 0.03],
    );
    volume["height_m"] = json!(depth);
    items.push(volume);
    let mut rule = item(
        "rule-1",
        surface_description,
        "area",
        length(&route) * surface_factor,
        &[line_handle],
        route[0],
    );
    rule["basis"] = json!("rule:lining-height");
    rule["tolerance_class"] = json!("rule");
    rule["tolerance_percent"] = json!(2.0);
    items.push(rule);
    let mut judgement = item(
        "judgement-1",
        allowance_description,
        "volume",
        polygon_area(&shapes[0]) * depth * 1.1,
        &[areas[0]],
        [0.03, 0.03],
    );
    judgement["basis"] = json!("rule:working-allowance");
    judgement["height_m"] = json!(depth * 1.1);
    judgement["tolerance_class"] = json!("judgement");
    items.push(judgement);
    let annotation_factor = if typ == "part" { 0.1 } else { 1.0 };
    layer(&mut session, "A-SCHEDULE")?;
    let table = run(
        &mut session,
        json!({"type":"table","at":to_drawing([0.0,(-h*2.0-4.0)*annotation_factor]),"rows":2,"cols":2,"col_width":2.5*scale*annotation_factor,"row_height":0.5*scale*annotation_factor,"data":[["Item","Count"],[block,n.to_string()]]}),
    )?[0];
    layer(&mut session, "A-NOTES")?;
    run(
        &mut session,
        json!({"type":"text","at":to_drawing([0.0,(-h*2.0-6.0)*annotation_factor]),"height":0.16*scale*annotation_factor,"value":format!("{}; material thickness {:.4} m; surface factor {:.4} m; provisional allowance 10%; units {}",typ,depth,surface_factor,if inches {"inches"} else {"metres"})}),
    )?;
    // A visible reference keeps fitted vector PDFs and scans physically measurable.
    let bar_length = if typ == "part" { 0.1 } else { 1.0 };
    let bar_y = (-h * 2.0 - 7.0) * annotation_factor;
    draw(
        &mut session,
        json!({"kind":"line","a":to_drawing([0.0,bar_y]),"b":to_drawing([bar_length,bar_y])}),
    )?;
    for x in [0.0, bar_length] {
        draw(
            &mut session,
            json!({"kind":"line","a":to_drawing([x,bar_y-0.08*annotation_factor]),"b":to_drawing([x,bar_y+0.08*annotation_factor])}),
        )?;
    }
    run(
        &mut session,
        json!({"type":"text","at":to_drawing([0.0,bar_y-0.3*annotation_factor]),"height":0.16*scale*annotation_factor,"value":format!("Scale bar: {bar_length} m")}),
    )?;
    let truth = json!({"schema_version":1,"id":"","project_type":typ,"seed":seed,"drawing":"drawing.dxf",
        "source":"qiyas-generator-v1","licence":"Apache-2.0","items":items,"seeded_errors":[],
        "rules":{"lining-height":{"operation":"length_times_factor","factor":surface_factor,"clause":format!("project preamble: {surface_description}; centreline length times {surface_factor} m")},
        "working-allowance":{"operation":"area_times_height","height_m":depth*1.1,"clause":"project preamble: material depth plus 10% provisional allowance"}},
        "generator":{"version":1,"commands":"Session::run_input","units":if inches {"inches"} else {"metres"}},"mutations":[]});
    Ok(Project {
        session,
        truth,
        scale,
        region: shapes[0].clone(),
        region_handle: areas[0],
        line_handle,
        inserts,
        texts,
        table,
        block: block.into(),
    })
}

fn replace_sources(truth: &mut Value, old: u64, new: &[u64]) {
    let old = format!("{old:X}");
    for item in truth["items"].as_array_mut().unwrap() {
        let sources = item["source_objects"].as_array_mut().unwrap();
        if let Some(index) = sources.iter().position(|h| h.as_str() == Some(&old)) {
            sources.remove(index);
            sources.extend(handles(new).into_iter().map(Value::String));
        }
    }
}

fn variant(project: &mut Project, name: &str, directory: &Path) -> Result<()> {
    let s = &mut project.session;
    match name {
        "exploded" => {
            for h in &project.inserts {
                let parts = run(
                    s,
                    json!({"type":"modify","kind":"explode","targets":targets(&[*h])}),
                )?;
                replace_sources(&mut project.truth, *h, &parts);
            }
        }
        "wrong-layers" | "merged-layers" => {
            let mappings: Vec<_> = s.layers().iter().filter(|l| l.name.starts_with("A-") || l.name.starts_with("M-"))
                .enumerate().map(|(i,l)| json!({"from":l.name,"to":if name=="merged-layers" {"0".to_string()} else {format!("LEGACY-{}",(i+3)%7)}})).collect();
            run(s, json!({"type":"translate_layers","mappings":mappings}))?;
        }
        "overlaps" => {
            run(
                s,
                json!({"type":"modify","kind":"copy","targets":targets(&[project.line_handle,project.region_handle]),"delta":[0,0],"count":1}),
            )?;
        }
        "gaps" => {
            run(
                s,
                json!({"type":"delete_entities","targets":targets(&[project.region_handle])}),
            )?;
            layer(s, "A-ROOM")?;
            let mut parts = Vec::new();
            for (i, (a, b)) in project
                .region
                .iter()
                .zip(project.region.iter().cycle().skip(1))
                .take(project.region.len())
                .enumerate()
            {
                let mut a = *a;
                if i == 0 {
                    a[0] += 0.01;
                }
                parts.push(draw(s,json!({"kind":"line","a":[a[0]*project.scale,a[1]*project.scale],"b":[b[0]*project.scale,b[1]*project.scale]}))?);
            }
            replace_sources(&mut project.truth, project.region_handle, &parts);
        }
        "line-text" => {
            for (h, p, label) in &project.texts {
                run(
                    s,
                    json!({"type":"delete_entities","targets":targets(&[*h])}),
                )?;
                layer(s, "A-ROOM-NAME")?;
                // A seven-segment room number retains a human-readable location label, but no text entity.
                let digit = label.as_bytes()[0] - b'0';
                for (a, b) in digit_segments(digit) {
                    let map = |q: [f64; 2]| {
                        [
                            (p[0] + q[0] * 0.1) * project.scale,
                            (p[1] + q[1] * 0.1) * project.scale,
                        ]
                    };
                    draw(s, json!({"kind":"line","a":map(a),"b":map(b)}))?;
                }
            }
        }
        "missing-schedule" => {
            run(
                s,
                json!({"type":"delete_entities","targets":targets(&[project.table])}),
            )?;
        }
        "seeded-errors" => {
            let mut rows = project.truth["items"].as_array().unwrap().clone();
            let missing = rows.remove(0)["id"].clone();
            rows[0]["quantity"] = json!(rows[0]["quantity"].as_f64().unwrap() * 1.25);
            rows[1]["unit"] = json!("m");
            let quantity_id = rows[0]["id"].clone();
            let unit_id = rows[1]["id"].clone();
            write_json(
                &directory.join("input-boq.json"),
                &json!({"schema_version":1,"items":rows}),
            )?;
            let count = project.inserts.len() + 2;
            run(
                s,
                json!({"type":"edit_cell","target":{"handle":project.table},"row":1,"col":1,"text":count.to_string()}),
            )?;
            write_json(
                &directory.join("schedule.json"),
                &json!({"items":[{"item_id":"count-1","source_objects":handles(&project.inserts),"quantity":count,"block_name":project.block}]}),
            )?;
            project.truth["seeded_errors"] = json!([
                {"item_id":missing,"kind":"missing"}, {"item_id":quantity_id,"kind":"wrong_quantity","magnitude_percent":25},
                {"item_id":unit_id,"kind":"wrong_unit"},{"item_id":"count-1","kind":"schedule_mismatch"}]);
        }
        "revision-b" => {
            let old = project.inserts.len();
            layer(s, "A-EQUIPMENT")?;
            let insert = run(
                s,
                json!({"type":"insert_block","name":project.block,"at":[-project.scale,project.scale]}),
            )?[0];
            let item = project.truth["items"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|i| i["id"] == "count-1")
                .unwrap();
            item["quantity"] = json!(old + 1);
            item["source_objects"]
                .as_array_mut()
                .unwrap()
                .push(json!(format!("{insert:X}")));
            run(
                s,
                json!({"type":"edit_cell","target":{"handle":project.table},"row":1,"col":1,"text":(old+1).to_string()}),
            )?;
            project.truth["revision"] = json!({"from":"A","to":"B","deltas":[{"item_id":"count-1","before":old,"after":old+1,"added_handles":handles(&[insert])}]});
        }
        "inches" | "vector-pdf" | "scanned-pdf" => {}
        _ => return Err(format!("Unknown variant {name}")),
    }
    project.truth["mutations"] = json!([{"kind":name,"truth_policy":if name=="revision-b" {"known revision delta"} else {"physical design quantities retained"}}]);
    if name.ends_with("pdf") {
        project.truth["drawing"] = json!("drawing.pdf");
        safe_output(&directory.join("drawing.pdf"))?;
        project.truth["pdf"] = pdf::write(
            &project.session,
            &directory.join("drawing.pdf"),
            name == "scanned-pdf",
        )?;
        // PDF citations are page regions, never handles that falsely appear to exist in a PDF.
        for item in project.truth["items"].as_array_mut().unwrap() {
            item["model_source_objects"] = item["source_objects"].clone();
            item["source_objects"] =
                json!([format!("page:1:item:{}", item["id"].as_str().unwrap())]);
        }
    }
    Ok(())
}

fn digit_segments(digit: u8) -> Vec<([f64; 2], [f64; 2])> {
    let segments = [
        ([0.0, 2.0], [1.0, 2.0]),
        ([1.0, 2.0], [1.0, 1.0]),
        ([1.0, 1.0], [1.0, 0.0]),
        ([0.0, 0.0], [1.0, 0.0]),
        ([0.0, 1.0], [0.0, 0.0]),
        ([0.0, 2.0], [0.0, 1.0]),
        ([0.0, 1.0], [1.0, 1.0]),
    ];
    let bits = [
        0b0111111, 0b0000110, 0b1011011, 0b1001111, 0b1100110, 0b1101101, 0b1111101, 0b0000111,
        0b1111111, 0b1101111,
    ];
    segments
        .into_iter()
        .enumerate()
        .filter_map(|(i, s)| (bits[digit as usize] & 1 << i != 0).then_some(s))
        .collect()
}

fn save(project: &Project, directory: &Path) -> Result<()> {
    if project.truth["drawing"] == "drawing.dxf" {
        safe_output(&directory.join("drawing.dxf"))?;
        DxfWriter::new(&project.session.drawing.doc)
            .write_to_file(directory.join("drawing.dxf"))
            .map_err(error)?;
    }
    write_json(&directory.join("truth.json"), &project.truth)?;
    if project.truth["seeded_errors"]
        .as_array()
        .is_some_and(Vec::is_empty)
    {
        write_json(
            &directory.join("clean-boq.json"),
            &json!({"schema_version":1,"items":project.truth["items"]}),
        )?;
    }
    Ok(())
}

/// Rebuild existing scans from their original seeds. No other case or holdout is touched.
pub fn regenerate_scans(root: &Path) -> Result<Value> {
    let mut refreshed = Vec::new();
    for directory in super::directories(&root.join("cases"))? {
        let scan = root
            .join("cases")
            .join(directory)
            .join("variants/scanned-pdf");
        if !scan.is_dir() {
            continue;
        }
        super::reject_symlink(&scan)?;
        let truth_path = scan.join("truth.json");
        super::reject_symlink(&truth_path)?;
        let original = fs::read_to_string(&truth_path).map_err(error)?;
        let truth: Value = serde_json::from_str(&original).map_err(error)?;
        if truth["source"] != "qiyas-generator-v1" {
            continue;
        }
        let seed = truth["seed"].as_u64().ok_or("Missing scan seed")?;
        let typ = truth["project_type"]
            .as_str()
            .ok_or("Missing scan family")?;
        let project = build(&scan.join("drawing.dxf"), typ, seed, false)?;
        let drawing = scan.join("drawing.pdf");
        safe_output(&drawing)?;
        let metadata = pdf::write(&project.session, &drawing, true)?;
        let recipe_path = scan.join("recognition.json");
        if recipe_path.exists() {
            super::reject_symlink(&recipe_path)?;
            let mut recipe: Value =
                serde_json::from_slice(&fs::read(&recipe_path).map_err(error)?).map_err(error)?;
            reframe_scan_recipe(&mut recipe, &truth["pdf"], &metadata)?;
            write_json(&recipe_path, &recipe)?;
        }
        let updated = replace_scan_metadata(&original, &metadata)?;
        safe_output(&truth_path)?;
        fs::write(&truth_path, updated).map_err(error)?;
        refreshed.push(truth["id"].clone());
    }
    Ok(json!({"regenerated_scans":refreshed.len(),"cases":refreshed,"seeds":"unchanged"}))
}

fn replace_scan_metadata(original: &str, metadata: &Value) -> Result<String> {
    // Generator truth uses this top-level indentation. Keep every other byte, including
    // floating-point quantity literals that a JSON parse/write may otherwise round.
    let marker = "\n  \"pdf\": ";
    let begin = original
        .find(marker)
        .ok_or("Missing generated PDF metadata")?
        + marker.len();
    let mut stream = serde_json::Deserializer::from_str(&original[begin..]).into_iter::<Value>();
    stream
        .next()
        .ok_or("Missing PDF metadata value")?
        .map_err(error)?;
    let end = begin + stream.byte_offset();
    let replacement = serde_json::to_string_pretty(metadata)
        .map_err(error)?
        .replace('\n', "\n  ");
    Ok(format!(
        "{}{}{}",
        &original[..begin],
        replacement,
        &original[end..]
    ))
}

fn reframe_scan_recipe(recipe: &mut Value, old: &Value, new: &Value) -> Result<()> {
    let bounds = |v: &Value| {
        serde_json::from_value::<[[f64; 2]; 2]>(v["drawing_bounds"].clone()).map_err(error)
    };
    let old_bounds = bounds(old)?;
    let new_bounds = bounds(new)?;
    let size = |v: &Value| -> Result<[f64; 2]> {
        Ok([
            v["width_px"].as_f64().ok_or("Missing scan width")?,
            v["height_px"].as_f64().ok_or("Missing scan height")?,
        ])
    };
    let old_size = size(old)?;
    let new_size = size(new)?;
    let transform = |point: &mut Value| -> Result<()> {
        let mut p: [f64; 2] = serde_json::from_value(point.clone()).map_err(error)?;
        for axis in 0..2 {
            let world = old_bounds[0][axis]
                + p[axis] / old_size[axis] * (old_bounds[1][axis] - old_bounds[0][axis]);
            p[axis] = (world - new_bounds[0][axis]) / (new_bounds[1][axis] - new_bounds[0][axis])
                * new_size[axis];
        }
        *point = json!(p);
        Ok(())
    };
    if let Some(commands) = recipe["commands"].as_array_mut() {
        for command in commands {
            if command["type"] != "recognition" || command["kind"] != "vision" {
                continue;
            }
            let spec = &mut command["spec"];
            transform(&mut spec["calibration"]["a"])?;
            transform(&mut spec["calibration"]["b"])?;
            if let Some(hints) = spec["hints"].as_array_mut() {
                for hint in hints {
                    if let Some(points) = hint["points"].as_array_mut() {
                        for point in points {
                            transform(point)?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Builds six project families, each seed's variants, and a separate hash-only holdout seal.
/// An existing holdout is never opened, listed, regenerated, or used by the developer suite.
pub fn generate(root: &Path, seeds: usize) -> Result<Value> {
    if !(1..=100).contains(&seeds) {
        return Err("Seeds per family must be 1 to 100".into());
    }
    make_directory(&root.join("cases"))?;
    make_directory(&root.join("sources/qiyas-generator"))?;
    write_json(
        &root.join("sources/qiyas-generator/provenance.json"),
        &json!({"source":"Qiyas benchmark generator v1","licence":"Apache-2.0","author":"QS Mind","origin":"crates/qiyas-cli/src/bench/generate.rs","seed_algorithm":"SplitMix64","downloaded":false}),
    )?;
    safe_output(&root.join("sources/qiyas-generator/LICENSE"))?;
    fs::write(
        root.join("sources/qiyas-generator/LICENSE"),
        include_str!("../../../../LICENSE"),
    )
    .map_err(error)?;
    for (family, typ) in TYPES.iter().enumerate() {
        for index in 0..seeds {
            let seed = 15000 + family as u64 * 1000 + index as u64;
            let id = format!("{typ}-{seed}");
            let directory = root.join("cases").join(&id);
            make_directory(&directory)?;
            let mut clean = build(&directory.join("drawing.dxf"), typ, seed, false)?;
            clean.truth["id"] = json!(id);
            save(&clean, &directory)?;
            for name in VARIANTS {
                let variant_dir = directory.join("variants").join(name);
                make_directory(&variant_dir)?;
                let mut project = build(
                    &variant_dir.join("drawing.dxf"),
                    typ,
                    seed,
                    name == "inches",
                )?;
                project.truth["id"] = json!(format!("{id}/variants/{name}"));
                variant(&mut project, name, &variant_dir)?;
                save(&project, &variant_dir)?;
            }
        }
    }
    safe_output(&root.join("holdout"))?;
    let mut sealed = false;
    if !root.join("holdout").exists() {
        fs::create_dir(root.join("holdout")).map_err(error)?;
        let mut hashes = Vec::new();
        for (index, typ) in TYPES.iter().enumerate() {
            let seed = 0x5048415345313500 + index as u64;
            let mut project = build(&root.join("holdout/drawing.dxf"), typ, seed, false)?;
            project.truth["id"] = json!("sealed");
            let drawing = DxfWriter::new(&project.session.drawing.doc)
                .write_to_vec()
                .map_err(error)?;
            let truth = serde_json::to_vec_pretty(&project.truth).map_err(error)?;
            let mut digest = Sha256::new();
            digest.update(&drawing);
            digest.update(&truth);
            let hash = format!("{:x}", digest.finalize());
            let dir = root.join("holdout").join(&hash);
            fs::create_dir(&dir).map_err(error)?;
            fs::write(dir.join("drawing.dxf"), drawing).map_err(error)?;
            fs::write(dir.join("truth.json"), truth).map_err(error)?;
            hashes.push(json!({"sha256":hash}));
        }
        write_json(
            &root.join("holdout-manifest.json"),
            &json!({"schema_version":1,"hash_algorithm":"sha256(drawing.dxf || truth.json)","entries":hashes}),
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root.join("holdout"), fs::Permissions::from_mode(0o000))
                .map_err(error)?;
        }
        sealed = true;
    }
    Ok(
        json!({"clean_cases":seeds*TYPES.len(),"variants":seeds*TYPES.len()*VARIANTS.len(),"project_types":TYPES,"holdout_created":sealed,"holdout":"hash-only; existing contents never read"}),
    )
}

#[cfg(test)]
pub(crate) fn offline_case(path: &Path, typ: &str, seed: u64) -> Result<(Session, Value)> {
    let project = build(path, typ, seed, false)?;
    Ok((project.session, project.truth))
}

#[cfg(test)]
mod tests {
    use super::*;
    use qiyas_core::Command;
    #[cfg(unix)]
    #[test]
    fn scan_refresh_preserves_seed_truth_and_other_variants() {
        let dir = tempfile::tempdir().unwrap();
        let case = dir.path().join("cases/office-55");
        let scan = case.join("variants/scanned-pdf");
        fs::create_dir_all(&scan).unwrap();
        let mut project = build(&case.join("drawing.dxf"), "office", 55, false).unwrap();
        project.truth["id"] = json!("office-55/variants/scanned-pdf");
        variant(&mut project, "scanned-pdf", &scan).unwrap();
        save(&project, &scan).unwrap();
        fs::write(case.join("drawing.dxf"), b"untouched clean drawing").unwrap();
        let vector = case.join("variants/vector-pdf");
        fs::create_dir_all(&vector).unwrap();
        fs::write(vector.join("drawing.pdf"), b"untouched vector").unwrap();
        let before = fs::read(scan.join("truth.json")).unwrap();
        let result = regenerate_scans(dir.path()).unwrap();
        assert_eq!(result["regenerated_scans"], 1);
        assert_eq!(fs::read(scan.join("truth.json")).unwrap(), before);
        assert_eq!(
            fs::read(case.join("drawing.dxf")).unwrap(),
            b"untouched clean drawing"
        );
        assert_eq!(
            fs::read(vector.join("drawing.pdf")).unwrap(),
            b"untouched vector"
        );
        assert!(!dir.path().join("holdout").exists());
        let mut recipe = json!({"commands":[{"type":"recognition","kind":"vision","spec":{
            "calibration":{"a":[0,0],"b":[100,0]},"hints":[{"points":[[50,40]]}]}}]});
        let old = json!({"drawing_bounds":[[0,0],[10,10]],"width_px":100,"height_px":100});
        let new = json!({"drawing_bounds":[[-1,-1],[11,11]],"width_px":120,"height_px":120});
        reframe_scan_recipe(&mut recipe, &old, &new).unwrap();
        assert_eq!(
            recipe["commands"][0]["spec"]["calibration"]["a"],
            json!([10.0, 10.0])
        );
        assert_eq!(
            recipe["commands"][0]["spec"]["hints"][0]["points"][0],
            json!([60.0, 50.0])
        );
    }

    #[test]
    fn generator_refuses_symlink_outputs() {
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), directory.path().join("cases")).unwrap();
        assert!(generate(directory.path(), 1)
            .unwrap_err()
            .contains("symlink"));
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    }
    #[test]
    fn generator_truth_remeasures_and_seed_repeats() {
        let temp = tempfile::tempdir().unwrap();
        for typ in TYPES {
            let mut a = build(&temp.path().join("a.dxf"), typ, 42, false).unwrap();
            let b = build(&temp.path().join("b.dxf"), typ, 42, false).unwrap();
            assert_eq!(a.truth, b.truth);
            for item in a.truth["items"].as_array().unwrap().clone() {
                if item["basis"] != "geometric" {
                    continue;
                }
                let hs: Vec<u64> = item["source_objects"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|h| u64::from_str_radix(h.as_str().unwrap(), 16).unwrap())
                    .collect();
                let cmd:Command=serde_json::from_value(json!({"type":"add_measurement","kind":item["kind"],"handles":hs,"height":item.get("height_m"),"description":"independent generator test"})).unwrap();
                let Outcome::Measured { measurement } = a.session.run(cmd).unwrap() else {
                    panic!("no measurement")
                };
                assert!(
                    (measurement.shown - item["quantity"].as_f64().unwrap()).abs() < 1e-7,
                    "{typ} {}",
                    item["id"]
                );
            }
        }
    }
    #[test]
    fn variants_retain_geometry_and_declare_changes() {
        let temp = tempfile::tempdir().unwrap();
        for name in VARIANTS.into_iter().filter(|name| !name.ends_with("pdf")) {
            let dir = temp.path().join(name);
            fs::create_dir(&dir).unwrap();
            let mut p = build(&dir.join("drawing.dxf"), "office", 41, name == "inches").unwrap();
            let before = p.truth["items"].clone();
            variant(&mut p, name, &dir).unwrap();
            if name == "revision-b" {
                let a = before
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|i| i["id"] == "count-1")
                    .unwrap()["quantity"]
                    .as_f64()
                    .unwrap();
                let b = p.truth["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|i| i["id"] == "count-1")
                    .unwrap()["quantity"]
                    .as_f64()
                    .unwrap();
                assert_eq!(b, a + 1.0);
            } else {
                for (a, b) in before
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(p.truth["items"].as_array().unwrap())
                {
                    assert_eq!(a["quantity"], b["quantity"]);
                }
            }
            if name == "seeded-errors" {
                assert_eq!(p.truth["seeded_errors"].as_array().unwrap().len(), 4);
            }
        }
    }
}
