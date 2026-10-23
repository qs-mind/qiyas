//! Column mappings are supplied by Mizan; the drawing supplies every cell value.
use super::*;
use crate::objects::Property;

type TextCell = ([f64; 2], String);

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GridSource {
    Table {
        handle: u64,
        #[serde(default)]
        skip_rows: usize,
    },
    Text {
        handles: Vec<u64>,
        row_tolerance_m: f64,
        #[serde(default)]
        delimiter: Option<String>,
        #[serde(default)]
        skip_rows: usize,
    },
    Attributes {
        handles: Vec<u64>,
        tags: Vec<String>,
    },
}
#[derive(Debug, Serialize)]
pub struct Grid {
    pub rows: Vec<Vec<String>>,
    pub sources: Vec<Evidence>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchColumn {
    Block,
    Layer,
    Text,
    Attribute(String),
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Column {
    pub column: usize,
    #[serde(default = "one")]
    pub factor: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduleSpec {
    pub source: GridSource,
    pub template: ObjectType,
    pub type_column: usize,
    pub match_column: usize,
    pub match_on: MatchColumn,
    #[serde(default)]
    pub properties: BTreeMap<String, Column>,
    #[serde(default)]
    pub selector: Selector,
    pub confidence: f64,
}
impl Session {
    pub fn read_recognition_grid(&self, source: &GridSource) -> Result<Grid, CoreError> {
        let get = |h: u64| {
            self.drawing
                .doc
                .get_entity(Handle::new(h))
                .ok_or_else(|| invalid(format!("Missing schedule source {h}")))
        };
        match source {
            GridSource::Table { handle, skip_rows } => {
                let e = get(*handle)?;
                let EntityType::Table(table) = e else {
                    return Err(invalid("Schedule source must be a table"));
                };
                Ok(Grid {
                    rows: crate::table::table_info(table)
                        .cells
                        .into_iter()
                        .skip(*skip_rows)
                        .collect(),
                    sources: vec![source_of(e)],
                })
            }
            GridSource::Attributes { handles, tags } => {
                let mut rows = Vec::new();
                let mut sources = Vec::new();
                for h in handles {
                    let e = get(*h)?;
                    rows.push(
                        tags.iter()
                            .map(|t| {
                                attribute(e, t).map(str::to_owned).ok_or_else(|| {
                                    invalid(format!("Missing schedule attribute {t}"))
                                })
                            })
                            .collect::<Result<_, _>>()?,
                    );
                    sources.push(source_of(e));
                }
                Ok(Grid { rows, sources })
            }
            GridSource::Text {
                handles,
                row_tolerance_m,
                delimiter,
                skip_rows,
            } => {
                if !row_tolerance_m.is_finite()
                    || *row_tolerance_m <= 0.0
                    || delimiter.as_ref().is_some_and(|s| s.is_empty())
                {
                    return Err(invalid(
                        "Text grid needs positive row_tolerance_m and a nonempty delimiter",
                    ));
                }
                let mut texts = Vec::new();
                let mut sources = Vec::new();
                for h in handles {
                    let e = get(*h)?;
                    let (at, text) = match e {
                        EntityType::Text(t) => {
                            ([t.insertion_point.x, t.insertion_point.y], t.value.clone())
                        }
                        EntityType::MText(t) => (
                            [t.insertion_point.x, t.insertion_point.y],
                            crate::query::plain_text(&t.value),
                        ),
                        _ => return Err(invalid("Text grid sources must be TEXT or MTEXT")),
                    };
                    texts.push((at, text));
                    sources.push(source_of(e));
                }
                texts.sort_by(|a, b| b.0[1].total_cmp(&a.0[1]).then(a.0[0].total_cmp(&b.0[0])));
                let tol = row_tolerance_m / self.drawing.units.to_metres();
                let mut grouped: Vec<(f64, Vec<TextCell>)> = Vec::new();
                for (at, text) in texts {
                    if let Some((_, cells)) = grouped
                        .last_mut()
                        .filter(|(y, _)| (at[1] - *y).abs() <= tol)
                    {
                        cells.push((at, text));
                    } else {
                        grouped.push((at[1], vec![(at, text)]));
                    }
                }
                let mut rows = Vec::new();
                for (_, mut cells) in grouped {
                    cells.sort_by(|a, b| a.0[0].total_cmp(&b.0[0]));
                    // A delimited MTEXT can contain complete rows separated by paragraph breaks.
                    if cells.len() == 1 && delimiter.is_some() {
                        for line in cells[0].1.lines() {
                            rows.push(
                                line.split(delimiter.as_deref().unwrap())
                                    .map(|s| s.trim().to_owned())
                                    .collect(),
                            );
                        }
                    } else {
                        rows.push(cells.into_iter().map(|(_, s)| s).collect());
                    }
                }
                Ok(Grid {
                    rows: rows.into_iter().skip(*skip_rows).collect(),
                    sources,
                })
            }
        }
    }
    pub(super) fn recognise_schedule(&mut self, spec: ScheduleSpec) -> Result<Value, CoreError> {
        let grid = self.read_recognition_grid(&spec.source)?;
        if grid.rows.is_empty() {
            return Err(invalid("The schedule has no data rows"));
        }
        let mut names = Vec::new();
        for (index, row) in grid.rows.iter().enumerate() {
            let cell = |column: usize| {
                row.get(column)
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        invalid(format!(
                            "Schedule row {} has no value in column {column}",
                            index + 1
                        ))
                    })
            };
            let mut definition = spec.template.clone();
            definition.name = format!("{}:{}", spec.template.name, cell(spec.type_column)?);
            let mut extract = BTreeMap::new();
            for (name, column) in &spec.properties {
                if !column.factor.is_finite() {
                    return Err(invalid("Column factor must be finite"));
                }
                let property = definition
                    .properties
                    .get_mut(name)
                    .ok_or_else(|| invalid(format!("Unknown schedule property {name}")))?;
                let value = cell(column.column)?;
                let value = match property {
                    Property::Number { default, .. } => {
                        *default = number(value)? * column.factor;
                        json!(*default)
                    }
                    Property::Text { default } | Property::Choice { default, .. } => {
                        *default = value.into();
                        json!(value)
                    }
                    Property::Bool { default } => {
                        *default = value
                            .parse()
                            .map_err(|_| invalid("Boolean cells must be true or false"))?;
                        json!(*default)
                    }
                };
                extract.insert(
                    name.clone(),
                    Extraction::Constant {
                        value,
                        source: format!("schedule row {}, column {}", index + 1, column.column),
                    },
                );
            }
            self.define_object_type(definition.clone(), false)?;
            let mut selector = spec.selector.clone();
            let pattern = Pattern::Regex {
                regex: format!("^{}$", regex::escape(cell(spec.match_column)?)),
            };
            match &spec.match_on {
                MatchColumn::Block => selector.block = Some(pattern),
                MatchColumn::Layer => selector.layer = Some(pattern),
                MatchColumn::Text => selector.text = Some(pattern),
                MatchColumn::Attribute(tag) => {
                    selector.attributes.insert(tag.clone(), pattern);
                }
            }
            let rule = Rule {
                name: definition.name.clone(),
                object_type: definition.name.clone(),
                selector,
                extract,
                confidence: spec.confidence,
                review_below: threshold(),
                priority: 0,
                reject: false,
                sources: grid.sources.clone(),
            };
            self.define_recognition(&rule)?;
            names.push(definition.name);
        }
        Ok(json!({"types":names,"sources":grid.sources}))
    }
}
