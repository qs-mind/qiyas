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
            } if (scale.is_none() || centre.is_none())
                && !self.sheet.eq_ignore_ascii_case("*Model_Space") =>
            {
                if let Ok((fit_centre, fit_scale)) = self.viewport_fit_default(*corner1, *corner2) {
                    if centre.is_none() {
                        set("centre", json!(fit_centre), Some(model_unit));
                    }
                    if scale.is_none() {
                        set("scale", json!(fit_scale), Some(&format!("mm/{model_unit}")));
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
