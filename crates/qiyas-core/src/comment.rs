//! Comment clouds: a revision cloud with a written comment beside it (`Command::Comment`), and
//! the list of them on every sheet (`Command::ListComments`).
//!
//! Both parts go on the layer `COMMENT_LAYER` (red, made when missing) ByLayer, so the comments
//! can be shown, hidden or plotted together. Each part carries `COMMENT_APP` xdata: the cloud
//! `Integer16(1)`, its id and the comment as typed; the label `Integer16(2)` and the same id.
//! The label is an ordinary MText (a double click edits it), and the list reads the label's text
//! when it is still there, else the text the cloud keeps. An id is a string, never a handle, so a
//! copied comment still finds its own copied label (the copy's id is the same).

use opencadcodec::entities::EntityCommon;
use opencadcodec::xdata::{ExtendedDataRecord, XDataValue};
use opencadcodec::{AppId, Color, EntityType};
use serde::Serialize;

use crate::draw::{Current, DrawSpec};
use crate::field::chunks;
use crate::query::plain_text;
use crate::{CoreError, Session};

/// The application name of comment xdata.
pub const COMMENT_APP: &str = "QIYAS_COMMENT";
/// The layer comment clouds and their labels go on.
pub const COMMENT_LAYER: &str = "COMMENTS";

const CLOUD: i16 = 1;
const LABEL: i16 = 2;

/// A comment cloud on a sheet (`Outcome::Comments`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommentInfo {
    /// Stays the same for the comment's life (copies share it).
    pub id: String,
    /// The cloud.
    pub handle: u64,
    /// The label beside it, when it is still there.
    pub label: Option<u64>,
    /// The comment: the label's text, else the text the cloud keeps.
    pub text: String,
    /// The sheet's block name.
    pub sheet: String,
    /// The cloud's corners, [[min_x, min_y], [max_x, max_y]], in drawing coordinates.
    pub bounds: [[f64; 2]; 2],
}

/// The kind of comment part `common` is (`CLOUD` or `LABEL`), its id, and the text it keeps.
fn comment_part(common: &EntityCommon) -> Option<(i16, String, String)> {
    let record = common.extended_data.get_record(COMMENT_APP)?;
    let mut values = record.values.iter();
    let kind = match values.next()? {
        XDataValue::Integer16(k) => *k,
        _ => return None,
    };
    let id = match values.next()? {
        XDataValue::String(s) => s.clone(),
        _ => return None,
    };
    let text = values
        .filter_map(|v| match v {
            XDataValue::String(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    Some((kind, id, text))
}

fn mark(common: &mut EntityCommon, kind: i16, id: &str, text: Option<&str>) {
    let mut record = ExtendedDataRecord::new(COMMENT_APP);
    record.add_value(XDataValue::Integer16(kind));
    record.add_value(XDataValue::String(id.to_string()));
    for v in text.map(chunks).unwrap_or_default() {
        record.add_value(v);
    }
    common.extended_data.upsert_record(record);
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

impl Session {
    /// `Command::Comment`: a revision cloud around the polygon `points` (arcs about `arc_len`
    /// long; `None`: a 32nd of its outline) and the comment `text` beside its top right corner,
    /// 2.5 mm tall at the annotation scale (at least an 80th of the sheet), both
    /// on `COMMENT_LAYER`. The cloud's and the label's handles, in that order.
    pub(crate) fn add_comment(
        &mut self,
        points: Vec<[f64; 2]>,
        text: &str,
        arc_len: Option<f64>,
    ) -> Result<Vec<u64>, CoreError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(invalid("Type the comment"));
        }
        let arc_len = arc_len.unwrap_or_else(|| crate::draw::default_cloud_arc_len(&points));
        let mut cloud = self
            .draw_entities(DrawSpec::RevCloud {
                points: points.clone().into(),
                arc_len: Some(arc_len),
            })?
            .remove(0);
        let [_, [max_x, max_y]] = crate::query::bounds(&points);
        // 2.5 mm on the sheet at the annotation scale, but never lost on the sheet (a plan at
        // 1:1 in metres would give 2.5 mm in the model): an 80th of what it draws, at least.
        let paper = self.default_text_height();
        let [[x0, y0], [x1, y1]] = self.sheet_bounds();
        let height = if self.sheet.eq_ignore_ascii_case("*Model_Space") {
            paper.max((x1 - x0).max(y1 - y0) / 80.0)
        } else {
            paper
        };
        // Beside the cloud's corner, clear of its arcs (they bulge about a quarter of their
        // length out).
        let at = [max_x + arc_len * 0.4 + height, max_y];

        self.register_comment_app();
        let id = format!("C{:X}", self.drawing.doc.next_handle());
        mark(cloud.common_mut(), CLOUD, &id, Some(text));

        let current = std::mem::replace(
            &mut self.current,
            Current {
                layer: COMMENT_LAYER.to_string(),
                ..Current::default()
            },
        );
        self.ensure_layer(COMMENT_LAYER, Color::Index(1));
        let added = self
            .mtext_entity(
                opencadcodec::Vector3::new(at[0], at[1], 0.0),
                height * 30.0,
                height,
                text,
                None,
            )
            .and_then(|mut label| {
                mark(label.common_mut(), LABEL, &id, None);
                self.add_styled(vec![cloud, label])
            });
        self.current = current;
        added
    }

    fn register_comment_app(&mut self) {
        let doc = &mut self.drawing.doc;
        if !doc.app_ids.contains(COMMENT_APP) {
            let mut app = AppId::new(COMMENT_APP);
            app.handle = doc.allocate_handle();
            doc.app_ids.add(app).expect("checked it is missing");
        }
    }

    /// `Command::ListComments`: the comment clouds of every sheet, model first, each sheet's in
    /// the order they were drawn.
    pub fn list_comments(&self) -> Vec<CommentInfo> {
        let mut out = Vec::new();
        for sheet in self.drawing.sheets() {
            let mut labels: Vec<Option<Label>> = Vec::new();
            let mut clouds = Vec::new();
            for e in self.drawing.entities_in(&sheet.block) {
                let Some((kind, id, kept)) = comment_part(e.common()) else {
                    continue;
                };
                let h = e.common().handle.value();
                match (kind, e) {
                    (LABEL, EntityType::MText(t)) => {
                        let at = [t.insertion_point.x, t.insertion_point.y];
                        labels.push(Some((id, h, plain_text(&t.value), at)));
                    }
                    (CLOUD, EntityType::LwPolyline(p)) => {
                        let corners: Vec<[f64; 2]> = p
                            .vertices
                            .iter()
                            .map(|v| [v.location.x, v.location.y])
                            .collect();
                        clouds.push((id, h, kept, crate::query::bounds(&corners)));
                    }
                    _ => {}
                }
            }
            for (id, handle, kept, bounds) in clouds {
                // A copy keeps the id, so each cloud takes the nearest unused label with its id
                // to where its label goes (its top right corner).
                let corner = [bounds[1][0], bounds[1][1]];
                let near = |at: &[f64; 2]| (at[0] - corner[0]).hypot(at[1] - corner[1]);
                let label = labels
                    .iter_mut()
                    .filter(|l| l.as_ref().is_some_and(|l| l.0 == id))
                    .min_by(|a, b| {
                        let (a, b) = (a.as_ref().unwrap(), b.as_ref().unwrap());
                        near(&a.3).total_cmp(&near(&b.3))
                    })
                    .and_then(Option::take);
                out.push(CommentInfo {
                    label: label.as_ref().map(|l| l.1),
                    text: label.map_or(kept, |l| l.2),
                    id,
                    handle,
                    sheet: sheet.block.clone(),
                    bounds,
                });
            }
        }
        out
    }
}

/// A comment's label as `list_comments` reads it: its id, handle, plain text and position.
type Label = (String, u64, String, [f64; 2]);
