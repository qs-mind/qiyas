//! Defaults that depend on existing drawing geometry. These share the read-only calculations
//! used by execution; an unresolved target leaves the default for the caller's not_checked list.

use serde_json::{json, Value};

use crate::validation::DefaultUsed;
use crate::{Command, ConstraintKind, Session, Units};

fn unit_name(units: Units) -> &'static str {
    match units {
        Units::Inches => "in",
        Units::Feet => "ft",
        Units::Centimetres => "cm",
        Units::Metres => "m",
        Units::Millimetres | Units::Unknown => "mm",
    }
}

impl Session {
    pub(crate) fn contextual_defaults(&self, input: &Value) -> Vec<DefaultUsed> {
        let mut defaults = Vec::new();
        let Ok(command) = serde_json::from_value::<Command>(input.clone()) else {
            return defaults;
        };
        let model_unit = unit_name(self.drawing.units);
        let sheet_unit = if self.sheet.eq_ignore_ascii_case("*Model_Space") {
            model_unit
        } else {
            "mm"
        };
        let mut set = |field: &str, value: Value, unit: Option<&str>| {
            defaults.push(DefaultUsed {
                field: format!("/{field}"),
                value,
                unit: unit.map(str::to_string),
            });
        };
        match &command {
            Command::Symbol(crate::draft_symbols::SymbolSpec::Door {
                a, b, leaves: None, ..
            }) => {
                let k = if self.sheet.eq_ignore_ascii_case("*Model_Space") {
                    self.drawing.units.to_metres()
                } else {
                    0.001
                };
                let threshold = crate::drafting_standards::standards()["symbols"]
                    ["double_leaf_from_m"]["value"]
                    .as_f64()
                    .expect("bundled leaf threshold");
                let leaves = if (b[0] - a[0]).hypot(b[1] - a[1]) * k >= threshold {
                    2
                } else {
                    1
                };
                set("leaves", json!(leaves), None);
            }
            Command::AddWallOpenings {
                centreline,
                openings,
            } => {
                if let Ok(groups) = self.wall_groups(None) {
                    if let Some(run) = groups
                        .iter()
                        .flat_map(|g| &g.runs)
                        .find(|r| r.centreline == *centreline)
                    {
                        for (index, opening) in openings.iter().enumerate() {
                            if opening.sill.is_none() {
                                let sill = match opening.kind {
                                    crate::walls::OpeningKind::Door => 0.0,
                                    crate::walls::OpeningKind::Window => 0.9,
                                };
                                set(&format!("openings/{index}/sill"), json!(sill), Some("m"));
                            }
                            if opening.head.is_none() {
                                set(
                                    &format!("openings/{index}/head"),
                                    json!(run.height.min(2.1)),
                                    Some("m"),
                                );
                            }
                        }
                    }
                }
            }
            Command::EditWallOpening { insert, edit } => {
                if let Ok(groups) = self.wall_groups(None) {
                    if let Some(opening) = groups
                        .iter()
                        .flat_map(|g| &g.runs)
                        .flat_map(|r| &r.openings)
                        .find(|o| o.insert == *insert)
                    {
                        for (field, omitted, value, unit) in [
                            (
                                "at",
                                edit.at.is_none(),
                                json!(opening.opening.at),
                                model_unit,
                            ),
                            (
                                "width",
                                edit.width.is_none(),
                                json!(opening.opening.width),
                                model_unit,
                            ),
                            (
                                "sill",
                                edit.sill.is_none(),
                                json!(opening.opening.sill),
                                "m",
                            ),
                            (
                                "head",
                                edit.head.is_none(),
                                json!(opening.opening.head),
                                "m",
                            ),
                        ] {
                            if omitted {
                                set(field, value, Some(unit));
                            }
                        }
                        if edit.kind.is_none() {
                            set("kind", json!(opening.opening.kind), None);
                        }
                        if edit.swing.is_none() {
                            set("swing", json!(opening.opening.swing), None);
                        }
                    }
                }
            }
            Command::Grid {
                x, y, extent: None, ..
            } if !x.is_empty() || !y.is_empty() => {
                set(
                    "extent",
                    json!(self.grid_extent_default(x, y)),
                    Some(sheet_unit),
                );
            }
            Command::LabelSpaces {
                query,
                layer,
                height,
                language,
                ..
            } => {
                if layer.as_deref().is_none_or(|layer| layer.trim().is_empty()) {
                    set("layer", json!(self.current.layer), None);
                }
                let rooms = if height.is_none() || language.is_none() {
                    self.room_label_defaults(query).ok()
                } else {
                    None
                };
                if let Some(rooms) = rooms.filter(|rooms| !rooms.is_empty()) {
                    if height.is_none() {
                        let values: Vec<_> = rooms.iter().enumerate().map(|(index, room)| {
                            json!({"index":index, "name":room.name, "value":room.height_metres})
                        }).collect();
                        set("height", json!(values), Some("m"));
                    }
                    if language.is_none() {
                        let values: Vec<_> = rooms.iter().enumerate().map(|(index, room)| {
                            json!({"index":index, "name":room.name, "value":room.language})
                        }).collect();
                        set("language", json!(values), None);
                    }
                }
            }
            Command::MView {
                corner1,
                corner2,
                scale,
                centre,
                model_bounds,
                paper_centre,
                ..
            } if !self.sheet.eq_ignore_ascii_case("*Model_Space") => {
                if let Ok(frame) = self.viewport_frame(
                    *corner1,
                    *corner2,
                    scale.as_ref(),
                    *centre,
                    *model_bounds,
                    *paper_centre,
                ) {
                    if centre.is_none() {
                        set("centre", json!(frame.centre), Some(model_unit));
                    }
                    if scale.is_none() {
                        set(
                            "scale",
                            json!(frame.scale),
                            Some(&format!("mm/{model_unit}")),
                        );
                    }
                    if model_bounds.is_some() {
                        set("corner1", json!(frame.a), Some("mm"));
                        set("corner2", json!(frame.b), Some("mm"));
                    }
                }
            }
            Command::CopyToClipboard {
                targets,
                base: None,
            } => {
                if let Ok(base) = self.clipboard_base_default(targets) {
                    set("base", json!(base), Some(sheet_unit));
                }
            }
            Command::AddDimConstraint {
                kind,
                targets,
                points,
                line_point,
                expression,
            } if expression
                .as_deref()
                .is_none_or(|value| value.trim().is_empty()) =>
            {
                if let Ok(expression) =
                    self.dimension_expression_default(*kind, targets, points, *line_point)
                {
                    let unit = if *kind == ConstraintKind::Angular {
                        "degrees"
                    } else {
                        sheet_unit
                    };
                    set("expression", json!(expression), Some(unit));
                }
            }
            _ => {}
        }
        defaults
    }
}
