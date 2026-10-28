//! Even-odd polygon topology. Clipper owns all outer/hole/island relationships.
use crate::predicates::canonical_start;
use crate::walls::Region;
use clipper2_rust::{ClipType, Clipper64, FillRule, Path64, Paths64, Point64, PolyTree64};

pub(crate) fn boolean(
    subject: &[Vec<[f64; 2]>],
    clip: &[Vec<[f64; 2]>],
    operation: ClipType,
) -> Result<Vec<Region>, &'static str> {
    let points = || subject.iter().chain(clip).flatten();
    let Some(&origin) = points().next() else {
        return Ok(vec![]);
    };
    let mut extent = 0.0_f64;
    for p in points() {
        for axis in 0..2 {
            let v = p[axis] - origin[axis];
            if !v.is_finite() {
                return Err("Polygon coordinates must be finite");
            }
            extent = extent.max(v.abs());
        }
    }
    if extent == 0.0 {
        return Ok(vec![]);
    }
    // Power-of-two scaling avoids decimal rescaling error. The grid is at most 2^-40
    // drawing units for ordinary extents, finer for small polygons. Keep integer magnitudes
    // below 2^50, where Clipper's f64 intersection arithmetic still has guard bits.
    // Walls arrive already translated and scaled by their thickness.
    let scale = 2.0_f64.powf(
        (2.0_f64.powi(44) / extent)
            .max(2.0_f64.powi(40))
            .min(2.0_f64.powi(50) / extent)
            .log2()
            .floor(),
    );
    if !scale.is_finite() || scale <= 0.0 {
        return Err("Polygon extent exceeds clipping precision");
    }
    let mut originals = std::collections::BTreeMap::new();
    let mut convert = |rings: &[Vec<[f64; 2]>]| -> Paths64 {
        rings
            .iter()
            .filter(|r| r.len() >= 3)
            .map(|ring| {
                ring.iter()
                    .map(|p| {
                        let point = Point64::new(
                            ((p[0] - origin[0]) * scale).round() as i64,
                            ((p[1] - origin[1]) * scale).round() as i64,
                        );
                        originals.entry((point.x, point.y)).or_insert(*p);
                        point
                    })
                    .collect()
            })
            .collect()
    };
    let mut engine = Clipper64::new();
    engine.add_subject(&convert(subject));
    engine.add_clip(&convert(clip));
    let mut tree = PolyTree64::new();
    if !engine.execute_tree(operation, FillRule::EvenOdd, &mut tree, &mut Paths64::new()) {
        return Err("Polygon clipping failed");
    }
    let ring = |path: &Path64| {
        let mut out: Vec<_> = path
            .iter()
            .map(|p| {
                *originals.get(&(p.x, p.y)).unwrap_or(&[
                    p.x as f64 / scale + origin[0],
                    p.y as f64 / scale + origin[1],
                ])
            })
            .collect();
        canonical_start(&mut out);
        out
    };
    let mut regions = Vec::new();
    for (i, node) in tree.nodes.iter().enumerate().skip(1) {
        if tree.is_hole(i) {
            continue;
        }
        let mut holes: Vec<_> = node
            .children()
            .iter()
            .map(|&child| ring(tree.nodes[child].polygon()))
            .collect();
        holes.sort_by(|a, b| compare_rings(a, b));
        regions.push(Region {
            outer: ring(node.polygon()),
            holes,
        });
    }
    regions.sort_by(|a, b| compare_rings(&a.outer, &b.outer));
    Ok(regions)
}

fn compare_rings(a: &[[f64; 2]], b: &[[f64; 2]]) -> std::cmp::Ordering {
    a.iter()
        .flatten()
        .zip(b.iter().flatten())
        .map(|(a, b)| a.total_cmp(b))
        .find(|c| !c.is_eq())
        .unwrap_or(a.len().cmp(&b.len()))
}

pub(crate) fn rings(regions: &[Region]) -> Vec<Vec<[f64; 2]>> {
    regions
        .iter()
        .flat_map(|r| std::iter::once(r.outer.clone()).chain(r.holes.iter().cloned()))
        .collect()
}

pub(crate) fn resolve(points: &[[f64; 2]]) -> Result<Vec<Region>, &'static str> {
    boolean(&[points.to_vec()], &[], ClipType::Union)
}
