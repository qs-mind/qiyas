//! A repeated shape at a different scale may be an enlarged detail. Report it as advice.
use opencadcodec::EntityType;
use qiyas_geom::{Path, Xform};
use serde_json::{json, Value};

use crate::{CoreError, Session};

type Bounds = [[f64; 2]; 2];
type Signature = Vec<[[i64; 2]; 2]>;
struct Candidate {
    handle: u64,
    sheet: String,
    bounds: Bounds,
    signature: Signature,
}

pub(super) fn findings(session: &Session) -> Result<Vec<Value>, CoreError> {
    let exclusions = session.measurement_exclusions()?;
    let mut candidates = Vec::new();
    for sheet in session.drawing.sheets() {
        let mut paths = Vec::<(u64, Path)>::new();
        for entity in session.drawing.doc.entities_in_block(&sheet.block) {
            let handle = entity.common().handle.value();
            if exclusions.entity(session, handle)
                || matches!(
                    entity,
                    EntityType::Text(_)
                        | EntityType::MText(_)
                        | EntityType::Dimension(_)
                        | EntityType::Hatch(_)
                        | EntityType::Table(_)
                )
            {
                continue;
            }
            paths.extend(
                qiyas_geom::flatten(
                    &session.drawing.doc,
                    entity,
                    &Xform::identity(),
                    session.measure_tol(),
                )
                .into_paths()
                .into_iter()
                .map(|p| (handle, p)),
            );
        }
        for (handle, outline) in paths
            .iter()
            .filter(|(_, p)| p.closed && p.points.len() >= 3)
        {
            let bounds = crate::ink::bounds(&outline.points);
            let width = bounds[1][0] - bounds[0][0];
            let height = bounds[1][1] - bounds[0][1];
            if width <= session.measure_tol() || height <= session.measure_tol() {
                continue;
            }
            let mut signature = Vec::new();
            let mut sources = std::collections::BTreeSet::new();
            for (source, path) in &paths {
                if path.points.is_empty()
                    || !path.points.iter().all(|p| {
                        (0..2).all(|i| {
                            p[i] >= bounds[0][i] - session.measure_tol()
                                && p[i] <= bounds[1][i] + session.measure_tol()
                        })
                    })
                {
                    continue;
                }
                sources.insert(*source);
                let point = |p: [f64; 2]| {
                    [
                        ((p[0] - bounds[0][0]) / width * 100_000.0).round() as i64,
                        ((p[1] - bounds[0][1]) / height * 100_000.0).round() as i64,
                    ]
                };
                for (a, b) in path
                    .points
                    .iter()
                    .zip(path.points.iter().cycle().skip(1))
                    .take(path.points.len() - usize::from(!path.closed))
                {
                    let mut segment = [point(*a), point(*b)];
                    segment.sort();
                    signature.push(segment);
                }
            }
            // A plain repeated rectangle is common in a plan and provides too little evidence.
            if sources.len() < 3 || signature.len() < 6 {
                continue;
            }
            signature.sort();
            candidates.push(Candidate {
                handle: *handle,
                sheet: sheet.name.clone(),
                bounds,
                signature,
            });
        }
    }
    let mut result = Vec::new();
    let mut reported = std::collections::BTreeSet::new();
    for (i, a) in candidates.iter().enumerate() {
        for b in &candidates[i + 1..] {
            if a.signature != b.signature {
                continue;
            }
            let scale_x = (b.bounds[1][0] - b.bounds[0][0]) / (a.bounds[1][0] - a.bounds[0][0]);
            let scale_y = (b.bounds[1][1] - b.bounds[0][1]) / (a.bounds[1][1] - a.bounds[0][1]);
            if (scale_x / scale_y - 1.0).abs() > 0.01 || (2.0 / 3.0..1.5).contains(&scale_x) {
                continue;
            }
            if a.sheet == b.sheet && crate::ink::intersects(a.bounds, b.bounds) {
                continue;
            }
            let (small, large) = if scale_x > 1.0 { (a, b) } else { (b, a) };
            if reported.insert(large.handle) {
                result.push(json!({"section":"possible_detail", "handles":[small.handle,large.handle],"sheets":[small.sheet,large.sheet],"scale":scale_x.max(1.0/scale_x),"message":"Repeated geometry at a larger scale may be a detail. Mark its region or group as view only if it should not be measured.","message_ar":"قد تكون الأشكال المتكررة بمقياس أكبر تفصيلا. اجعل منطقتها أو مجموعتها للعرض فقط إذا لم تكن ضمن الحصر.","command":{"type":"set_measurement_scope","region":{"handle":large.handle},"measured":false}}));
            }
        }
    }
    Ok(result)
}
