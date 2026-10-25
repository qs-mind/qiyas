//! Annotation checks use the same text boxes and visible shapes as placement.
use crate::check::{Finding, Severity, PAPER_MAX_MM, PAPER_MIN_MM};
use crate::ink::{self, SheetInk};
use crate::{Session, Target};
use opencadcodec::{EntityType, Handle};
use std::collections::HashSet;

impl Session {
    pub(crate) fn check_layout_ink(
        &self,
        ink: &SheetInk,
        _sheet: &str,
        to_m: f64,
        scale: f64,
        active: &impl Fn(u64) -> bool,
        push: &mut impl FnMut(Finding),
    ) {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("check_layout.check_layout_ink");
        let mut seen = HashSet::new();
        let mut report = |rule, a: u64, b: u64, at, message: String| {
            let key = (rule, a.min(b), a.max(b));
            if seen.insert(key) {
                push(Finding {
                    rule,
                    pack: "general",
                    severity: Severity::Warning,
                    handles: if b == 0 || a == b {
                        vec![a]
                    } else {
                        vec![a, b]
                    },
                    at: Some(at),
                    message,
                });
            }
        };
        for a in &ink.pieces {
            if !a.annotation {
                continue;
            }
            if !active(a.owner)
                && !ink.near(a.bounds).any(|b| active(b.owner))
                && !self
                    .room_from_label(&Target {
                        handle: a.owner,
                        chain: vec![],
                    })
                    .is_ok_and(active)
            {
                continue;
            }
            let at = [
                (a.bounds[0][0] + a.bounds[1][0]) / 2.0,
                (a.bounds[0][1] + a.bounds[1][1]) / 2.0,
            ];
            if a.text.is_some() {
                let mm = a.height * to_m * 1000.0 / scale;
                if mm > 0.0 && !(PAPER_MIN_MM - 1e-8..=PAPER_MAX_MM + 1e-8).contains(&mm) {
                    report(
                        "text_height_vs_scale",
                        a.owner,
                        0,
                        at,
                        format!(
                            "Text is {mm:.1} mm on paper; use at least {PAPER_MIN_MM:.1} mm for readable labels"
                        ),
                    );
                }
                if let Ok(boundary) = self.room_from_label(&Target {
                    handle: a.owner,
                    chain: vec![],
                }) {
                    if let Some(e) = self.drawing.doc.get_entity(Handle::new(boundary)) {
                        let paths = qiyas_geom::flatten(
                            &self.drawing.doc,
                            e,
                            &qiyas_geom::Xform::identity(),
                            0.0002 / to_m,
                        )
                        .into_paths();
                        if let Some(ring) = paths.first() {
                            let holes = crate::object_regions::retained_region(e)
                                .map(|r| {
                                    r.holes
                                        .iter()
                                        .enumerate()
                                        .map(|(i, h)| {
                                            self.layout_ring(
                                                h,
                                                r.hole_bulges.get(i).map_or(&[], Vec::as_slice),
                                            )
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            if !polygon_inside(&a.points, &ring.points, &holes) {
                                report(
                                    "label_outside_room",
                                    a.owner,
                                    boundary,
                                    at,
                                    "The area label extends outside its room; move or wrap it"
                                        .into(),
                                );
                            }
                        }
                    }
                }
            }
            for b in ink.near(a.bounds) {
                if (a.owner == b.owner
                    && (std::ptr::eq(a, b)
                        || (a.text.is_none() && b.text.is_none())
                        || a.table
                        || b.table))
                    || (a.grid && b.grid)
                {
                    continue;
                }
                // A grid label belongs inside its bubble, including imported unmarked grids.
                let bubble = grid_bubble(self, ink, b);
                if a.text.is_some() && bubble && a.points.iter().all(|p| ink::inside(*p, &b.points))
                {
                    continue;
                }
                let hit = if a.filled {
                    b.hits_polygon(&a.points)
                } else if b.filled {
                    a.hits_polygon(&b.points)
                } else {
                    a.edges()
                        .any(|(p, q)| b.edges().any(|(r, s)| ink::crosses(p, q, r, s)))
                };
                if !hit {
                    continue;
                }
                let rule = if bubble {
                    "annotation_over_grid"
                } else if a.text.is_some() && b.text.is_some() {
                    "text_overlaps_text"
                } else if a.text.is_some() {
                    "text_over_lines"
                } else if a.table || b.table {
                    "annotation_overlap"
                } else {
                    continue;
                };
                report(
                    rule,
                    a.owner,
                    b.owner,
                    at,
                    match rule {
                        "annotation_over_grid" => "Annotation crosses a grid bubble",
                        "text_overlaps_text" => "Two labels overlap",
                        "text_over_lines" => "Drawing geometry crosses a label",
                        _ => "Annotations overlap",
                    }
                    .into(),
                );
            }
        }
    }
}
pub(crate) fn polygon_inside(p: &[[f64; 2]], ring: &[[f64; 2]], holes: &[Vec<[f64; 2]>]) -> bool {
    p.iter()
        .all(|v| ink::inside(*v, ring) && !holes.iter().any(|h| ink::inside(*v, h)))
        && !ink::edges(p, true).any(|(a, b)| {
            ink::edges(ring, true)
                .chain(holes.iter().flat_map(|h| ink::edges(h, true)))
                .any(|(c, d)| ink::crosses(a, b, c, d))
        })
        && !holes
            .iter()
            .any(|h| h.first().is_some_and(|v| ink::inside(*v, p)))
}

pub(crate) fn grid_bubble(session: &Session, ink: &SheetInk, piece: &ink::Ink) -> bool {
    session
        .drawing
        .doc
        .get_entity(Handle::new(piece.owner))
        .is_some_and(|e| matches!(e, EntityType::Circle(_)))
        && (piece.grid
            || ink.near(piece.bounds).any(|t| {
                t.text.is_some()
                    && session
                        .drawing
                        .doc
                        .get_entity(Handle::new(t.owner))
                        .is_some_and(|e| matches!(e, EntityType::Text(_) | EntityType::MText(_)))
                    && ink::inside(
                        [
                            (t.bounds[0][0] + t.bounds[1][0]) / 2.0,
                            (t.bounds[0][1] + t.bounds[1][1]) / 2.0,
                        ],
                        &piece.points,
                    )
            }))
}
