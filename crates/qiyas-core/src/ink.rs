//! Plotted geometry shared by layout checks and placement. Text uses the viewport shaper.
use crate::Session;
use opencadcodec::{EntityType, Handle};
use qiyas_geom::{Paint, Xform};
use std::{collections::BTreeMap, sync::Arc};

pub(crate) type Bounds = [[f64; 2]; 2];
#[derive(Clone)]
pub(crate) struct Ink {
    pub owner: u64,
    pub bounds: Bounds,
    pub points: Vec<[f64; 2]>,
    pub closed: bool,
    pub filled: bool,
    pub holes: Vec<Vec<[f64; 2]>>,
    pub text: Option<String>,
    pub height: f64,
    pub annotation: bool,
    pub grid: bool,
    pub table: bool,
}
#[derive(Default)]
pub(crate) struct SheetInk {
    pub pieces: Vec<Ink>,
    pub incomplete: Vec<u64>,
    tree: rstar::RTree<
        rstar::primitives::GeomWithData<rstar::primitives::Rectangle<[f64; 2]>, usize>,
    >,
}
/// Individual primitive shapes survive unrelated edits. References use the normal path,
/// since their dependencies include block and external drawing contents.
#[derive(Default)]
pub(crate) struct InkCache {
    context: String,
    entities: BTreeMap<u64, CachedInk>,
}
type CachedInk = (Arc<EntityType>, Vec<Ink>, Vec<u64>);
fn cacheable(entity: &EntityType) -> bool {
    matches!(
        entity,
        EntityType::Line(_)
            | EntityType::Arc(_)
            | EntityType::Circle(_)
            | EntityType::Ellipse(_)
            | EntityType::LwPolyline(_)
            | EntityType::Polyline2D(_)
            | EntityType::Spline(_)
            | EntityType::Text(_)
            | EntityType::MText(_)
    )
}
impl Session {
    pub(crate) fn layout_ring(&self, points: &[[f64; 2]], bulges: &[f64]) -> Vec<[f64; 2]> {
        let entity = EntityType::LwPolyline(crate::object_regions::region_polyline(points, bulges));
        qiyas_geom::flatten(
            &self.drawing.doc,
            &entity,
            &Xform::identity(),
            0.0002 / self.check_space(&self.sheet).0,
        )
        .into_paths()
        .into_iter()
        .next()
        .map(|p| p.points)
        .unwrap_or_default()
    }
    pub(crate) fn sheet_ink(&self, sheet: &str) -> SheetInk {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("ink.sheet_ink");
        let mut out = SheetInk::default();
        let doc = &self.drawing.doc;
        let xrefs = self.xref_docs();
        let mut engine = self.text.borrow_mut();
        let plotted = |name: &str| {
            qiyas_geom::find_layer(doc, &xrefs, name)
                .is_none_or(|l| l.is_plottable && !l.flags.off && !l.flags.frozen)
        };
        let tol = 0.0002 / self.check_space(sheet).0;
        let mut header = doc.header.clone();
        // Allocation is not a plotting input. Every other header setting stays in the key.
        header.handle_seed = 0;
        let context = serde_json::json!([
            header,
            doc.layers.iter().collect::<Vec<_>>(),
            doc.text_styles.iter().collect::<Vec<_>>(),
            doc.line_types.iter().collect::<Vec<_>>(),
            doc.objects,
            doc.context_scales,
            self.arabic_reversed,
            tol
        ])
        .to_string();
        let mut cache = self.ink_cache.borrow_mut();
        if cache.context != context {
            *cache = InkCache {
                context,
                ..Default::default()
            };
        }
        cache
            .entities
            .retain(|id, _| doc.get_entity(Handle::new(*id)).is_some());
        for entity in doc.entities_in_block(sheet) {
            let common = entity.common();
            if common.invisible || !plotted(&common.layer) {
                continue;
            }
            let owner = common.handle.value();
            if let Some((previous, pieces, incomplete)) = cache.entities.get(&owner) {
                if std::ptr::eq(Arc::as_ptr(previous), entity) {
                    out.pieces.extend(pieces.iter().cloned());
                    out.incomplete.extend(incomplete);
                    continue;
                }
            }
            let cache_start = (out.pieces.len(), out.incomplete.len());
            let grid = common
                .extended_data
                .get_record(crate::conventions::GRID_APP)
                .is_some();
            let annotation = matches!(
                entity,
                EntityType::Text(_)
                    | EntityType::MText(_)
                    | EntityType::Table(_)
                    | EntityType::Dimension(_)
                    | EntityType::Leader(_)
                    | EntityType::MultiLeader(_)
            );
            // A per-entity budget prevents one nested block from hiding later sheet entities.
            let mut budget = qiyas_render::MAX_SHEET_WORK;
            let flat = qiyas_geom::flatten_within(
                doc,
                &xrefs,
                entity,
                &Xform::identity(),
                tol,
                &mut budget,
            );
            // A layout's own viewport (number 1) and a switched-off one draw nothing: they are
            // not geometry this check missed.
            let silent_viewport =
                matches!(entity, EntityType::Viewport(v) if v.id == 1 || !v.status.is_on);
            if flat.truncated
                || (flat.paths.is_empty()
                    && matches!(entity, EntityType::MultiLeader(_) | EntityType::Viewport(_))
                    && !silent_viewport)
            {
                out.incomplete.push(owner);
            }
            let start = out.pieces.len();
            for f in flat.paths {
                if !plotted(&f.layer)
                    || !f.xref_insert_layers.iter().all(|l| plotted(l))
                    || f.chain.iter().any(|h| {
                        doc.get_entity(Handle::new(*h))
                            .is_some_and(|e| !plotted(&e.common().layer))
                    })
                {
                    continue;
                }
                let mut piece = Ink {
                    owner,
                    bounds: [[0.0; 2]; 2],
                    points: f.path.points,
                    closed: f.path.closed,
                    filled: false,
                    holes: vec![],
                    text: None,
                    height: 0.0,
                    annotation,
                    grid,
                    table: false,
                };
                if let Some(text) = f.text {
                    if let Some(laid) =
                        engine.lay_out_text(doc, &xrefs, &text, self.arabic_reversed)
                    {
                        piece.points = laid.corners.to_vec();
                        piece.closed = true;
                        piece.filled = true;
                        piece.height = laid.xf.m[0][1].hypot(laid.xf.m[1][1]);
                        piece.text = Some(laid.text);
                        piece.annotation = true;
                    } else {
                        continue;
                    }
                } else if let Paint::Fill(fill) = f.paint {
                    if fill.background {
                        continue;
                    }
                    let mut rings = fill.world_rings().into_iter();
                    piece.points = rings.next().unwrap_or_default();
                    piece.holes = rings.collect();
                    piece.closed = true;
                    piece.filled = true;
                } else if matches!(f.paint, Paint::Infinite { .. }) {
                    out.incomplete.push(owner);
                    continue;
                }
                if piece.points.is_empty() {
                    continue;
                }
                piece.bounds = bounds(
                    &piece
                        .points
                        .iter()
                        .chain(piece.holes.iter().flatten())
                        .copied()
                        .collect::<Vec<_>>(),
                );
                out.pieces.push(piece);
            }
            // A table reserves its cells as well as their borders and text.
            if matches!(entity, EntityType::Table(_)) && out.pieces.len() > start {
                let b = union(out.pieces[start..].iter().map(|p| p.bounds)).unwrap();
                out.pieces.push(Ink {
                    owner,
                    bounds: b,
                    points: corners(b).to_vec(),
                    closed: true,
                    filled: true,
                    holes: vec![],
                    text: None,
                    height: 0.0,
                    annotation: true,
                    grid: false,
                    table: true,
                });
            }
            if cacheable(entity) {
                cache.entities.insert(
                    owner,
                    (
                        doc.get_entity_arc(common.handle).expect("listed entity"),
                        out.pieces[cache_start.0..].to_vec(),
                        out.incomplete[cache_start.1..].to_vec(),
                    ),
                );
            }
        }
        out.pieces
            .sort_by(|a, b| a.bounds[0][0].total_cmp(&b.bounds[0][0]));
        out.index();
        out.incomplete.sort_unstable();
        out.incomplete.dedup();
        out
    }
}
impl SheetInk {
    fn index(&mut self) {
        use rstar::primitives::{GeomWithData, Rectangle};
        self.tree = rstar::RTree::bulk_load(
            self.pieces
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    GeomWithData::new(Rectangle::from_corners(p.bounds[0], p.bounds[1]), i)
                })
                .collect(),
        );
    }
    pub fn near(&self, b: Bounds) -> impl Iterator<Item = &Ink> {
        // Callers supply their clearance in `b`; roundoff padding only broadens candidates.
        // The original bounds and the exact shape tests still decide the result.
        let size = b.iter().flatten().map(|v| v.abs()).fold(0.0, f64::max);
        let query = expand(b, qiyas_geom::tol_near(size, qiyas_geom::SAME_POINT));
        let mut ids: Vec<_> = self
            .tree
            .locate_in_envelope_intersecting(&rstar::AABB::from_corners(query[0], query[1]))
            .map(|entry| entry.data)
            .collect();
        ids.sort_unstable();
        ids.into_iter()
            .map(|i| &self.pieces[i])
            .filter(move |p| intersects(p.bounds, b))
    }
    pub fn collisions(&self, b: Bounds, exclude: &[u64]) -> Vec<u64> {
        let rect = corners(b);
        let mut owners: Vec<_> = self
            .near(b)
            .filter(|p| !exclude.contains(&p.owner) && p.hits_polygon(&rect))
            .map(|p| p.owner)
            .collect();
        owners.sort_unstable();
        owners.dedup();
        owners
    }
}
impl Ink {
    pub fn edges(&self) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
        edges(&self.points, self.closed).chain(self.holes.iter().flat_map(|r| edges(r, true)))
    }
    pub fn contains(&self, p: [f64; 2]) -> bool {
        self.filled
            && (usize::from(inside(p, &self.points))
                + self.holes.iter().filter(|r| inside(p, r)).count())
                % 2
                == 1
    }
    pub fn hits_polygon(&self, p: &[[f64; 2]]) -> bool {
        self.points
            .iter()
            .chain(self.holes.iter().flatten())
            .any(|&v| inside(v, p))
            || p.iter().any(|&v| self.contains(v))
            || self
                .edges()
                .any(|(a, b)| edges(p, true).any(|(c, d)| crosses(a, b, c, d)))
    }
}
pub(crate) fn edges(
    p: &[[f64; 2]],
    closed: bool,
) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
    let n = if closed && p.len() > 2 {
        p.len()
    } else {
        p.len().saturating_sub(1)
    };
    (0..n).map(|i| (p[i], p[(i + 1) % p.len()]))
}
pub(crate) fn bounds(p: &[[f64; 2]]) -> Bounds {
    let mut b = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    for v in p {
        for i in 0..2 {
            b[0][i] = b[0][i].min(v[i]);
            b[1][i] = b[1][i].max(v[i]);
        }
    }
    b
}
pub(crate) fn union(b: impl Iterator<Item = Bounds>) -> Option<Bounds> {
    b.reduce(|a, b| {
        [
            [a[0][0].min(b[0][0]), a[0][1].min(b[0][1])],
            [a[1][0].max(b[1][0]), a[1][1].max(b[1][1])],
        ]
    })
}
pub(crate) fn corners(b: Bounds) -> [[f64; 2]; 4] {
    [b[0], [b[1][0], b[0][1]], b[1], [b[0][0], b[1][1]]]
}
pub(crate) fn expand(b: Bounds, d: f64) -> Bounds {
    [[b[0][0] - d, b[0][1] - d], [b[1][0] + d, b[1][1] + d]]
}
pub(crate) fn intersects(a: Bounds, b: Bounds) -> bool {
    a[0][0] <= b[1][0] && b[0][0] <= a[1][0] && a[0][1] <= b[1][1] && b[0][1] <= a[1][1]
}
pub(crate) use qiyas_geom::predicates::inside;
pub(crate) use qiyas_geom::predicates::intersects as crosses;
