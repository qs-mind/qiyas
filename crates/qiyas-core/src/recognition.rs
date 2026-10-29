//! Recognition interprets evidence using drawing-owned rules. No trade selects an algorithm.
mod pdf_text;
mod schedule;
mod vision;
pub(crate) use pdf_text::text_runs;
pub use pdf_text::{calibrate_pdf_text, PdfTextReading, PdfTextRules, TextCalibration};

use std::collections::BTreeMap;

use opencadcodec::{Color, EntityType, Handle};
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::knowledge::{self, Kind};
use crate::objects::{GeometryKind, ObjectPlacement, ObjectType};
use crate::{CoreError, EntityFilter, EntityRow, Session, Target};

pub use schedule::{GridSource, ScheduleSpec};
mod raster_calibration;
pub use raster_calibration::{calibrate_raster, RasterCalibration, VisionDimension};
pub use vision::{Calibration, VisionSpec};
pub const APP: &str = "QIYAS_RECOGNITION";
pub const SOURCE_APP: &str = "QIYAS_SOURCE";

fn invalid(s: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(s.into())
}
fn one() -> f64 {
    1.0
}
fn threshold() -> f64 {
    0.9
}
fn group_one() -> usize {
    1
}

/// Strings are case-insensitive globs (* and ?); regexes use their explicit flags.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Pattern {
    Glob(String),
    Regex { regex: String },
}
impl Pattern {
    fn compile(&self) -> Result<Regex, CoreError> {
        let pattern = match self {
            Self::Regex { regex } => regex.clone(),
            Self::Glob(glob) => {
                let mut out = String::from("(?i)^");
                for c in glob.chars() {
                    match c {
                        '*' => out.push_str(".*"),
                        '?' => out.push('.'),
                        c => out.push_str(&regex::escape(&c.to_string())),
                    }
                }
                out.push('$');
                out
            }
        };
        Regex::new(&pattern).map_err(|e| invalid(format!("Invalid recognition pattern: {e}")))
    }
    fn matches(&self, text: &str) -> Result<bool, CoreError> {
        Ok(self.compile()?.is_match(text))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Selector {
    pub layer: Option<Pattern>,
    pub block: Option<Pattern>,
    pub entity_type: Option<Pattern>,
    pub attributes: BTreeMap<String, Pattern>,
    pub hatch: Option<Pattern>,
    pub text: Option<Pattern>,
    /// Effective RGB, including ByLayer colours. 0xRRGGBB.
    pub colour: Option<u32>,
    /// Positive closed area on the entity, or a region enclosing its text insertion.
    pub closed_region: Option<bool>,
    /// Enclosure sources selected from drawing evidence, independent of text layers.
    pub boundary_layers: Vec<String>,
    pub exclude_layers: Vec<String>,
    /// A correction can deliberately be scoped to specific evidence, without generalising it.
    pub handles: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Extraction {
    /// The source insert name, for data-defined grouping and descriptions.
    BlockName,
    /// An explicitly cited value, for example a schedule or specification property.
    Constant {
        value: Value,
        source: String,
        /// Where it was read: drawing_note, drawing_table, document (default), engineer or
        /// mizan (an assumption). Shown beside the value in the working.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_kind: Option<crate::measurement::SourceKind>,
        /// The note or table it was read from, when it is on the drawing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        handle: Option<u64>,
    },
    Attribute {
        tag: String,
        #[serde(default = "one")]
        factor: f64,
        #[serde(default)]
        text: bool,
    },
    /// Capture a number from this entity's text, or a nearby note/section label.
    Text {
        regex: String,
        #[serde(default = "group_one")]
        group: usize,
        #[serde(default = "one")]
        factor: f64,
        #[serde(default)]
        nearby: Option<Selector>,
        #[serde(default)]
        max_distance_m: Option<f64>,
    },
    /// Nearest overlapping parallel line, measured in metres; the rule supplies the search span.
    ParallelDistance {
        other: Selector,
        max_distance_m: f64,
    },
    /// Geometry facts are computed by Qiyas, in physical units.
    Geometry { field: GeometryField },
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GeometryField {
    Length,
    Area,
    Count,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub name: String,
    pub object_type: String,
    #[serde(rename = "match")]
    pub selector: Selector,
    #[serde(default)]
    pub extract: BTreeMap<String, Extraction>,
    pub confidence: f64,
    #[serde(default = "threshold")]
    pub review_below: f64,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub sources: Vec<Evidence>,
    #[serde(default)]
    pub reject: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Evidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<crate::measurement::SourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    /// Displayed PDF points, when the evidence is read from a scan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<[[f64; 2]; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(default)]
    pub detail: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Pending,
    Accepted,
    Rejected,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub handle: u64,
    pub rule: String,
    pub confidence: f64,
    pub state: ReviewState,
    pub placement: ObjectPlacement,
    pub evidence: Vec<Evidence>,
    pub issues: Vec<String>,
    pub object: Option<u64>,
    #[serde(default)]
    pub unresolved: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecognitionCommand {
    /// Define or replace a validated recognition rule in QIYAS_KNOWLEDGE/recognition.
    /// Define object_type first in the same batch, including types used by rejection rules.
    /// For a path use {name:"Run",category:"observed",geometry:"path"} with no plan recipe.
    /// Quantity aliases must not refer to themselves: measured_length:"geometry.length",
    /// measured_count:"geometry.count". Nearby text extraction requires max_distance_m.
    #[schemars(example = json!({"kind":"define","rule":{"name":"runs","object_type":"Run","match":{"entity_type":"Line"},"confidence":0.95}}))]
    #[schemars(extend("x-takeoff-step" = "2 define recognition"))]
    #[schemars(example = json!({"kind":"define","rule":{"name":"observed","object_type":"Observed","match":{"layer":"$layer","block":"$block"},"confidence":0.95}}))]
    #[schemars(extend("x-takeoff-kit" = {"order":20,"example":1,"batch":20,"phase":"recognition"}))]
    Define { rule: Box<Rule> },
    /// List drawing-owned recognition rules without changing the drawing.
    #[schemars(example = json!({"kind":"list"}))]
    List,
    /// Dry run: return proposed typed elements, confidence, evidence and unresolved properties.
    #[schemars(example = json!({"kind":"test"}))]
    Test {
        #[serde(default)]
        rules: Vec<String>,
    },
    /// Recognise the current sheet. Accept confident matches and retain the rest for review.
    /// All instances and review entries belong to one undo step. Repeated apply is idempotent.
    #[schemars(example = json!({"kind":"apply"}))]
    #[schemars(extend("x-takeoff-step" = "3 recognise"))]
    #[schemars(extend("x-takeoff-kit" = {"order":40,"example":0,"batch":30,"phase":"recognition"}))]
    Apply {
        #[serde(default)]
        rules: Vec<String>,
    },
    /// Read persistent review entries on the current sheet; omit state to include all decisions.
    #[schemars(example = json!({"kind":"queue","state":"pending"}))]
    Queue {
        #[serde(default)]
        state: Option<ReviewState>,
    },
    /// Accept, fix properties/type, or reject a pending candidate. A named correction saves a
    /// rule scoped to this source; an explicit rule can express a reviewed generalisation.
    #[schemars(example = json!({"kind":"review","handle":1234,"decision":"accepted"}))]
    #[schemars(extend("x-takeoff-step" = "7 fix by handle"))]
    Review {
        handle: u64,
        decision: ReviewState,
        #[serde(default)]
        props: BTreeMap<String, Value>,
        #[serde(default)]
        object_type: Option<String>,
        #[serde(default)]
        save_as: Option<String>,
        #[serde(default)]
        rule: Option<Box<Rule>>,
    },
    /// Read cells from drawing tables, text grids or attribute blocks. No domain interpretation.
    #[schemars(example = json!({"kind":"read_schedule","source":{"kind":"table","handle":1234}}))]
    ReadSchedule { source: GridSource },
    /// Convert source cells into types and recognition rules using the supplied column mapping.
    /// Each created type/rule is named template.name:cell (for example Item:D1).
    /// Use these returned names in later rules and method selectors in the same batch.
    #[schemars(example = json!({"kind":"schedule","spec":{"source":{"kind":"table","handle":1234},"template":{"name":"Item","category":"example","geometry":"point","quantities":{"measured_count":"count"}},"type_column":0,"match_column":0,"match_on":"block","confidence":0.95}}))]
    #[schemars(extend("x-takeoff-step" = "2 interpret schedules"))]
    #[schemars(extend("x-takeoff-kit" = {"order":30,"example":0}))]
    Schedule { spec: Box<ScheduleSpec> },
    /// Import vectors with a scale computed from two page points and a known distance in metres.
    /// Every imported entity retains its PDF path and page. One undo step.
    #[schemars(extend("x-needs" = "pdf"))]
    #[schemars(example = json!({"kind":"pdf","path":"drawing.pdf","page":1,"calibration":{"a":[0,0],"b":[100,0],"distance_m":1}}))]
    Pdf {
        path: String,
        page: u32,
        calibration: Calibration,
        #[serde(default)]
        insert_at: [f64; 2],
    },
    /// Import a vector PDF calibrated from its own text and ticked scale bar.
    /// The regex, physical unit and search radius are supplied as general recognition data.
    #[schemars(extend("x-needs" = "pdf"))]
    #[schemars(example = json!({"kind":"pdf_text","path":"drawing.pdf","page":1,"rule":{"label_regex":"Reference (?P<distance>[0-9.]+) m","metres_per_label_unit":1,"search_heights":8}}))]
    PdfText {
        path: String,
        page: u32,
        rule: TextCalibration,
    },
    /// Snap vision hints from a calibrated PDF page to existing geometry or raster ink.
    /// Points use displayed page points, bottom-left origin. Supply all visible calibrated spans,
    /// dimension strings with endpoints, and verbatim spatial texts from vision/OCR. The longest
    /// verified span sets scale; residual uncertainty above 0.5% lowers confidence.
    #[schemars(extend("x-needs" = "pdf"))]
    #[schemars(example = json!({"kind":"vision","spec":{"path":"drawing.pdf","page":1,"calibration":{"a":[0,0],"b":[100,0],"distance_m":1},"snap_radius_m":0.01,"hints":[{"object_type":"Run","points":[[20,20],[120,20]],"confidence":0.8}]}}))]
    Vision { spec: Box<VisionSpec> },
}
impl RecognitionCommand {
    pub fn is_read(&self) -> bool {
        matches!(
            self,
            Self::List | Self::Test { .. } | Self::Queue { .. } | Self::ReadSchedule { .. }
        )
    }
}

impl Session {
    pub fn recognition_rules(&self) -> Result<Vec<Rule>, CoreError> {
        knowledge::list(&self.drawing.doc, Kind::Recognition)?
            .into_iter()
            .map(|e| {
                let rule: Rule =
                    serde_json::from_value(e.data).map_err(|e| invalid(e.to_string()))?;
                self.validate_recognition_rule(&rule)?;
                Ok(rule)
            })
            .collect()
    }
    fn recognition_type(&self, name: &str) -> Result<ObjectType, CoreError> {
        let definition = self.object_type(name)?;
        // Recognition does not redraw source geometry or manufacture hosted cuts.
        if !definition.plan.symbols.is_empty()
            || definition.plan.outline
            || definition.plan.hatch
            || definition.plan.centreline
            || definition.plan.band_faces.is_some()
            || definition.host_cut.is_some()
        {
            return Err(invalid("Recognition types must retain source geometry; use a type with an empty plan recipe and no host cut"));
        }
        Ok(definition)
    }
    pub(crate) fn validate_recognition_rule(&self, rule: &Rule) -> Result<(), CoreError> {
        if rule.name.trim().is_empty()
            || !valid_confidence(rule.confidence)
            || !valid_confidence(rule.review_below)
        {
            return Err(invalid(
                "Recognition needs a name and confidence/threshold in 0..1",
            ));
        }
        validate_selector(&rule.selector)?;
        let definition = self.recognition_type(&rule.object_type)?;
        for (name, extraction) in &rule.extract {
            if !definition.properties.contains_key(name)
                || definition.property_bindings.contains_key(name)
            {
                return Err(invalid(format!(
                    "Unknown or bound extraction property {name}"
                )));
            }
            match extraction {
                Extraction::Constant { source, .. } if source.trim().is_empty() => {
                    return Err(invalid("Constant extraction needs a source citation"))
                }
                Extraction::Attribute { factor, .. } if !factor.is_finite() => {
                    return Err(invalid("Attribute factor must be finite"))
                }
                Extraction::Text {
                    regex,
                    group,
                    factor,
                    nearby,
                    max_distance_m,
                } => {
                    let regex = Regex::new(regex).map_err(|e| invalid(e.to_string()))?;
                    if *group >= regex.captures_len() || !factor.is_finite() {
                        return Err(invalid("Invalid text capture or factor"));
                    }
                    if let Some(selector) = nearby {
                        validate_selector(selector)?;
                        if !max_distance_m.is_some_and(|n| n.is_finite() && n > 0.0) {
                            return Err(invalid("Nearby text needs a positive max_distance_m"));
                        }
                    }
                }
                Extraction::ParallelDistance {
                    other,
                    max_distance_m,
                } => {
                    validate_selector(other)?;
                    if !max_distance_m.is_finite() || *max_distance_m <= 0.0 {
                        return Err(invalid("Parallel search needs a positive max_distance_m"));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub(crate) fn define_recognition(&mut self, rule: &Rule) -> Result<(), CoreError> {
        self.validate_recognition_rule(rule)?;
        knowledge::put(
            &mut self.drawing.doc,
            Kind::Recognition,
            &rule.name,
            &json!(rule),
        )
    }
    fn recognition_rows(&self) -> Vec<EntityRow> {
        let mut query = EntityFilter {
            limit: 500,
            ..Default::default()
        };
        let mut rows = Vec::new();
        loop {
            let page = self.find_entities(&query);
            rows.extend(
                page.rows
                    .into_iter()
                    .filter(|r| r.chain.is_empty() && r.xref.is_none()),
            );
            let Some(next) = page.next_offset else { break };
            query.offset = next;
        }
        rows
    }
    fn selector_matches(&self, selector: &Selector, row: &EntityRow) -> Result<bool, CoreError> {
        let Some(e) = self.drawing.doc.get_entity(Handle::new(row.handle)) else {
            return Ok(false);
        };
        for (pattern, text) in [
            (&selector.layer, Some(row.layer.as_str())),
            (&selector.block, row.block_name.as_deref()),
            (&selector.entity_type, Some(row.kind.as_str())),
            (&selector.text, row.text.as_deref()),
            (
                &selector.hatch,
                match e {
                    EntityType::Hatch(h) => Some(h.pattern.name.as_str()),
                    _ => None,
                },
            ),
        ] {
            if let Some(p) = pattern {
                if !text.map(|s| p.matches(s)).transpose()?.unwrap_or(false) {
                    return Ok(false);
                }
            }
        }
        if !selector.handles.is_empty() && !selector.handles.contains(&row.handle) {
            return Ok(false);
        }
        for (tag, pattern) in &selector.attributes {
            if !attribute(e, tag)
                .map(|s| pattern.matches(s))
                .transpose()?
                .unwrap_or(false)
            {
                return Ok(false);
            }
        }
        if let Some(rgb) = selector.colour {
            let colour = match e.common().color {
                Color::ByLayer => self
                    .drawing
                    .doc
                    .layers
                    .get(&row.layer)
                    .map(|l| l.color)
                    .unwrap_or(Color::Index(7)),
                c => c,
            };
            let (r, g, b) = colour.rgb().unwrap_or((255, 255, 255));
            if rgb != ((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)) {
                return Ok(false);
            }
        }
        if let Some(closed) = selector.closed_region {
            let enclosed = if matches!(e, EntityType::Text(_) | EntityType::MText(_)) {
                self.recognition_region(
                    row,
                    &crate::object_regions::BoundaryLayers {
                        boundary_layers: selector.boundary_layers.clone(),
                        exclude_layers: selector.exclude_layers.clone(),
                    },
                )
                .is_some()
            } else {
                row.area.is_some_and(|a| a > 0.0)
            };
            if closed != enclosed {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn recognition_region(
        &self,
        row: &EntityRow,
        policy: &crate::object_regions::BoundaryLayers,
    ) -> Option<(Vec<[f64; 2]>, [f64; 2])> {
        if !matches!(row.kind.as_str(), "Text" | "MText") {
            return None;
        }
        let entity = self.drawing.doc.get_entity(Handle::new(row.handle))?;
        let seed = position(entity, row);
        match self.room_boundary_in(&self.sheet, seed, policy) {
            crate::Boundary::Ring { ring, .. } => Some((ring, seed)),
            _ => None,
        }
    }
    fn fit_recognition_geometry(
        &self,
        candidate: &mut Candidate,
        row: &EntityRow,
        definition: &ObjectType,
        policy: &crate::object_regions::BoundaryLayers,
    ) -> Result<(), CoreError> {
        let e = self
            .drawing
            .doc
            .get_entity(Handle::new(row.handle))
            .ok_or_else(|| invalid("Recognition source is missing"))?;
        candidate.placement.driver = Some(row.handle);
        candidate.placement.points.clear();
        candidate.placement.region = None;
        match definition.geometry {
            GeometryKind::Point if !matches!(e, EntityType::Point(_) | EntityType::Insert(_)) => {
                candidate.placement.driver = None;
                candidate.placement.points = vec![position(e, row)];
            }
            GeometryKind::Region
                if matches!(e, EntityType::Text(_) | EntityType::MText(_))
                    || !row.area.is_some_and(|a| a > 0.0) =>
            {
                if let Some((ring, seed)) = self.recognition_region(row, policy) {
                    candidate.placement.driver = None;
                    candidate.placement.points = ring;
                    candidate.placement.region = Some(crate::object_regions::RegionSource {
                        boundary_layers: policy.clone(),
                        seed,
                        subtract_islands: true,
                    });
                } else {
                    candidate
                        .issues
                        .push("A positive closed region is required".into());
                }
            }
            GeometryKind::Path | GeometryKind::Band if !row.length.is_some_and(|l| l > 0.0) => {
                candidate.issues.push("A nonempty path is required".into())
            }
            GeometryKind::Hosted => candidate
                .issues
                .push("A host must be assigned before recognition".into()),
            _ => {}
        }
        Ok(())
    }
    pub fn test_recognition(&self, names: &[String]) -> Result<Vec<Candidate>, CoreError> {
        self.validate_object_metadata()?;
        let rules = self.recognition_rules()?;
        for name in names {
            if !rules.iter().any(|r| r.name == *name) {
                return Err(invalid(format!("Unknown recognition rule {name}")));
            }
        }
        let rows = self.recognition_rows();
        let mut result = Vec::new();
        for row in &rows {
            let e = self
                .drawing
                .doc
                .get_entity(Handle::new(row.handle))
                .expect("queried entity");
            if crate::objects::instance(e).is_some()
                || e.common().extended_data.get_record(APP).is_some()
                || e.common()
                    .extended_data
                    .get_record(crate::objects::PART_APP)
                    .is_some()
            {
                continue;
            }
            let mut matches = Vec::new();
            for rule in rules
                .iter()
                .filter(|r| names.is_empty() || names.contains(&r.name))
            {
                if self.selector_matches(&rule.selector, row)? {
                    matches.push(rule);
                }
            }
            matches.sort_by(|a, b| {
                b.priority
                    .cmp(&a.priority)
                    .then(b.confidence.total_cmp(&a.confidence))
                    .then(a.name.cmp(&b.name))
            });
            let Some(rule) = matches.first() else {
                continue;
            };
            let definition = self.object_type(&rule.object_type)?;
            let mut candidate = Candidate {
                handle: row.handle,
                rule: rule.name.clone(),
                confidence: rule.confidence,
                state: ReviewState::Pending,
                placement: serde_json::from_value(
                    json!({"type":rule.object_type,"driver":row.handle}),
                )
                .expect("placement"),
                evidence: rule.sources.clone(),
                issues: Vec::new(),
                object: None,
                unresolved: Vec::new(),
            };
            candidate.evidence.push(source_of(e));
            if matches
                .get(1)
                .is_some_and(|other| other.priority == rule.priority)
            {
                candidate
                    .issues
                    .push("Conflicting recognition rules require a type decision".into());
            }
            for (name, extraction) in &rule.extract {
                match self.extract_recognition(extraction, row, &rows) {
                    Ok((value, evidence)) => {
                        candidate.placement.props.insert(name.clone(), value);
                        candidate
                            .placement
                            .sources
                            .insert(name.clone(), extraction_source(extraction, &evidence));
                        candidate.evidence.extend(evidence);
                    }
                    Err(error) => {
                        candidate.issues.push(format!("{name}: {error}"));
                        candidate.unresolved.push(name.clone());
                    }
                }
            }
            if let Err(error) = definition.properties(&candidate.placement.props) {
                candidate.issues.push(error.to_string());
            }
            self.fit_recognition_geometry(
                &mut candidate,
                row,
                &definition,
                &crate::object_regions::BoundaryLayers {
                    boundary_layers: rule.selector.boundary_layers.clone(),
                    exclude_layers: rule.selector.exclude_layers.clone(),
                },
            )?;
            if candidate.issues.is_empty() && candidate.confidence >= rule.review_below {
                candidate.state = ReviewState::Accepted;
            }
            if rule.reject {
                candidate.state = ReviewState::Rejected;
            }
            result.push(candidate);
        }
        Ok(result)
    }
    fn extract_recognition(
        &self,
        extraction: &Extraction,
        row: &EntityRow,
        rows: &[EntityRow],
    ) -> Result<(Value, Vec<Evidence>), CoreError> {
        let e = self
            .drawing
            .doc
            .get_entity(Handle::new(row.handle))
            .expect("queried entity");
        let factor = self.drawing.units.to_metres();
        match extraction {
            Extraction::BlockName => Ok((json!(row.block_name.as_ref().ok_or_else(||invalid("Block name requires an insert"))?), vec![source_of(e)])),
            Extraction::Constant { value, source, handle, .. } => Ok((
                value.clone(),
                vec![Evidence {
                    handle: *handle,
                    detail: source.clone(),
                    ..Default::default()
                }],
            )),
            Extraction::Attribute { tag, factor, text } => {
                let value =
                    attribute(e, tag).ok_or_else(|| invalid(format!("Missing attribute {tag}")))?;
                Ok((
                    if *text {
                        json!(value)
                    } else {
                        json!(number(value)? * factor)
                    },
                    vec![Evidence {
                        detail: format!("attribute {tag}"),
                        ..source_of(e)
                    }],
                ))
            }
            Extraction::Geometry { field }
                if matches!(e, EntityType::Text(_) | EntityType::MText(_))
                    && !matches!(field, GeometryField::Count) => {
                Err(invalid("Text bounds are not measurable geometry; use the enclosing region's derived quantities"))
            }
            Extraction::Geometry { field } => Ok((
                json!(match field {
                    GeometryField::Length =>
                        row.length.ok_or_else(|| invalid("No measurable length"))? * factor,
                    GeometryField::Area =>
                        row.area.ok_or_else(|| invalid("No closed area"))? * factor * factor,
                    GeometryField::Count => 1.0,
                }),
                vec![source_of(e)],
            )),
            Extraction::Text {
                regex,
                group,
                factor: multiplier,
                nearby,
                max_distance_m,
            } => {
                let regex = Regex::new(regex).map_err(|e| invalid(e.to_string()))?;
                let mut values = Vec::new();
                for other in rows {
                    if let Some(selector) = nearby {
                        if !self.selector_matches(selector, other)?
                            || distance(centre(row), centre(other)) * factor
                                > max_distance_m.unwrap_or(0.0)
                        {
                            continue;
                        }
                    } else if other.handle != row.handle {
                        continue;
                    }
                    if let Some(text) = &other.text {
                        for capture in regex.captures_iter(text) {
                            if let Some(value) = capture.get(*group) {
                                values.push((number(value.as_str())? * multiplier, other));
                            }
                        }
                    }
                }
                if values.len() != 1 {
                    return Err(invalid(format!(
                        "Expected one text value, found {}",
                        values.len()
                    )));
                }
                let (value, source) = values[0];
                Ok((
                    json!(value),
                    vec![Evidence {
                        handle: Some(source.handle),
                        detail: source.text.clone().unwrap_or_default(),
                        ..Default::default()
                    }],
                ))
            }
            Extraction::ParallelDistance {
                other,
                max_distance_m,
            } => {
                let EntityType::Line(line) = e else {
                    return Err(invalid("Parallel distance requires a line"));
                };
                let a = [line.start.x, line.start.y];
                let b = [line.end.x, line.end.y];
                let len = distance(a, b);
                if len <= self.measure_tol() {
                    return Err(invalid("Zero length line"));
                }
                let u = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
                let mut found = Vec::new();
                for r in rows {
                    if r.handle == row.handle || !self.selector_matches(other, r)? {
                        continue;
                    }
                    let Some(EntityType::Line(l)) =
                        self.drawing.doc.get_entity(Handle::new(r.handle))
                    else {
                        continue;
                    };
                    let c = [l.start.x - a[0], l.start.y - a[1]];
                    let d = [l.end.x - a[0], l.end.y - a[1]];
                    let cross = |p: [f64; 2]| u[0] * p[1] - u[1] * p[0];
                    let dot = |p: [f64; 2]| u[0] * p[0] + u[1] * p[1];
                    if (cross(c) - cross(d)).abs() > self.measure_tol() {
                        continue;
                    }
                    if dot(c).max(dot(d)).min(len) - dot(c).min(dot(d)).max(0.0)
                        <= self.measure_tol()
                    {
                        continue;
                    }
                    let width = cross(c).abs() * factor;
                    if width > self.measure_tol() * factor && width <= *max_distance_m {
                        found.push((width, r.handle));
                    }
                }
                found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                let (width, handle) = found.first().copied().ok_or_else(|| {
                    invalid("No overlapping parallel line within the search span")
                })?;
                Ok((
                    json!(width),
                    vec![Evidence {
                        handle: Some(handle),
                        detail: "parallel line distance".into(),
                        ..Default::default()
                    }],
                ))
            }
        }
    }
    fn stamp_recognition(&mut self, candidate: &Candidate) -> Result<(), CoreError> {
        self.sheet_targets(&[Target {
            handle: candidate.handle,
            chain: vec![],
        }])?;
        let app = self.object_app(APP)?;
        let entity = self
            .drawing
            .doc
            .get_entity_mut(Handle::new(candidate.handle))
            .ok_or_else(|| invalid("Recognition source is missing"))?;
        crate::xdata::write_json(entity.common_mut(), APP, app, candidate)
    }
    fn accept_recognition(&mut self, candidate: &mut Candidate) -> Result<(), CoreError> {
        if candidate.object.is_some() {
            return Err(invalid("This candidate already has an instance"));
        }
        candidate.object =
            Some(self.place_objects(vec![candidate.placement.clone()], crate::Source::Ai)?[0]);
        candidate.state = ReviewState::Accepted;
        candidate.issues.clear();
        Ok(())
    }
    /// Recognition evidence on every sheet, used by generated BOQ and submission checks.
    pub fn recognition_evidence(&self) -> Result<Vec<Candidate>, CoreError> {
        self.drawing
            .doc
            .entities()
            .filter_map(|e| crate::xdata::read_text(e.common(), APP))
            .map(|text| {
                serde_json::from_str(&text)
                    .map_err(|e| invalid(format!("Invalid recognition metadata: {e}")))
            })
            .collect()
    }
    pub fn recognition_queue(
        &self,
        state: Option<ReviewState>,
    ) -> Result<Vec<Candidate>, CoreError> {
        self.drawing
            .entities_in(&self.sheet)
            .filter_map(|e| crate::xdata::read_text(e.common(), APP))
            .map(|text| {
                serde_json::from_str::<Candidate>(&text)
                    .map_err(|e| invalid(format!("Invalid recognition metadata: {e}")))
            })
            .filter(|r| {
                r.as_ref()
                    .map(|c| state.is_none_or(|s| c.state == s))
                    .unwrap_or(true)
            })
            .collect()
    }
    pub(crate) fn run_recognition(
        &mut self,
        command: RecognitionCommand,
    ) -> Result<Value, CoreError> {
        match command {
            RecognitionCommand::Define { rule } => {
                self.define_recognition(&rule)?;
                Ok(json!({"rule":rule.name}))
            }
            RecognitionCommand::List => Ok(json!({"rules":self.recognition_rules()?})),
            RecognitionCommand::Test { rules } => {
                Ok(json!({"candidates":self.test_recognition(&rules)?}))
            }
            RecognitionCommand::Queue { state } => {
                Ok(json!({"candidates":self.recognition_queue(state)?}))
            }
            RecognitionCommand::Apply { rules } => {
                let mut candidates = self.test_recognition(&rules)?;
                for c in &mut candidates {
                    if c.state == ReviewState::Accepted {
                        self.accept_recognition(c)?;
                    }
                    self.stamp_recognition(c)?;
                }
                Ok(json!({"candidates":candidates}))
            }
            RecognitionCommand::Review {
                handle,
                decision,
                mut props,
                object_type,
                save_as,
                rule,
            } => {
                self.sheet_targets(&[Target {
                    handle,
                    chain: vec![],
                }])?;
                let mut candidate = self
                    .recognition_queue(None)?
                    .into_iter()
                    .find(|c| c.handle == handle)
                    .ok_or_else(|| invalid("No recognition candidate on this source"))?;
                if candidate.state != ReviewState::Pending || decision == ReviewState::Pending {
                    return Err(invalid("Choose accept or reject for a pending candidate"));
                }
                if let Some(name) = object_type {
                    let old = self.object_type(&candidate.placement.type_name)?;
                    let definition = self.recognition_type(&name)?;
                    let applicable = |key: &String| {
                        definition.properties.contains_key(key)
                            && !definition.property_bindings.contains_key(key)
                    };
                    candidate.placement.props.retain(|key, _| applicable(key));
                    candidate.unresolved.retain(applicable);
                    // Changing a type discards its inapplicable old fields. Unrecognised input
                    // names still reach property validation, rather than being silently ignored.
                    props.retain(|key, _| applicable(key) || !old.properties.contains_key(key));
                    candidate.placement.type_name = name;
                    let row = self
                        .recognition_rows()
                        .into_iter()
                        .find(|r| r.handle == handle)
                        .ok_or_else(|| invalid("Recognition source is not on the current sheet"))?;
                    candidate.issues.clear();
                    let policy = candidate
                        .placement
                        .region
                        .as_ref()
                        .map(|r| r.boundary_layers.clone())
                        .unwrap_or_default();
                    self.fit_recognition_geometry(&mut candidate, &row, &definition, &policy)?;
                    if decision == ReviewState::Accepted && !candidate.issues.is_empty() {
                        return Err(invalid(candidate.issues.join("; ")));
                    }
                }
                if decision == ReviewState::Accepted
                    && candidate
                        .unresolved
                        .iter()
                        .any(|name| !props.contains_key(name))
                {
                    return Err(invalid(
                        "Fix every unresolved property before accepting this candidate",
                    ));
                }
                candidate.unresolved.clear();
                candidate.placement.props.extend(props.clone());
                if decision == ReviewState::Accepted {
                    self.accept_recognition(&mut candidate)?;
                } else {
                    candidate.state = decision;
                }
                if let Some(rule) = rule {
                    self.define_recognition(&rule)?;
                }
                if let Some(name) = save_as {
                    let mut learned = self
                        .recognition_rules()?
                        .into_iter()
                        .find(|r| r.name == candidate.rule)
                        .unwrap_or_else(|| Rule {
                            name: String::new(),
                            object_type: candidate.placement.type_name.clone(),
                            selector: Selector::default(),
                            extract: BTreeMap::new(),
                            confidence: 1.0,
                            review_below: threshold(),
                            priority: 0,
                            sources: candidate.evidence.clone(),
                            reject: false,
                        });
                    learned.name = name;
                    learned.object_type = candidate.placement.type_name.clone();
                    let definition = self.object_type(&learned.object_type)?;
                    learned.extract.retain(|key, _| {
                        definition.properties.contains_key(key)
                            && !definition.property_bindings.contains_key(key)
                    });
                    learned.priority = learned.priority.saturating_add(1);
                    learned.selector.handles = vec![handle];
                    learned.confidence = 1.0;
                    learned.reject = decision == ReviewState::Rejected;
                    for (key, value) in props {
                        learned.extract.insert(
                            key,
                            Extraction::Constant {
                                value,
                                source: format!("Reviewed correction of source {handle}"),
                                source_kind: Some(crate::measurement::SourceKind::Engineer),
                                handle: None,
                            },
                        );
                    }
                    self.define_recognition(&learned)?;
                }
                self.stamp_recognition(&candidate)?;
                Ok(json!({"candidate":candidate}))
            }
            RecognitionCommand::ReadSchedule { source } => {
                Ok(json!(self.read_recognition_grid(&source)?))
            }
            RecognitionCommand::Schedule { spec } => self.recognise_schedule(*spec),
            RecognitionCommand::Pdf {
                path,
                page,
                calibration,
                insert_at,
            } => {
                let scale = calibration.drawing_scale(self.drawing.units.to_metres())?;
                let prepared = crate::PreparedPdf::read(
                    std::path::Path::new(&path),
                    page,
                    scale,
                    crate::AttachMode::Import,
                )?;
                let handles = self.apply_prepared(prepared, insert_at)?;
                Ok(json!({"handles":handles,"drawing_units_per_point":scale}))
            }
            RecognitionCommand::PdfText { path, page, rule } => {
                let calibration = calibrate_pdf_text(std::path::Path::new(&path), page, &rule)?;
                self.run_recognition(RecognitionCommand::Pdf {
                    path,
                    page,
                    calibration,
                    insert_at: [0.0, 0.0],
                })
            }
            RecognitionCommand::Vision { spec } => self.recognise_vision(*spec),
        }
    }
}
fn validate_selector(selector: &Selector) -> Result<(), CoreError> {
    for p in [
        &selector.layer,
        &selector.block,
        &selector.entity_type,
        &selector.hatch,
        &selector.text,
    ]
    .into_iter()
    .flatten()
    .chain(selector.attributes.values())
    {
        p.compile()?;
    }
    if selector.colour.is_some_and(|n| n > 0xffffff) {
        return Err(invalid("Colour must be RGB 0x000000..0xFFFFFF"));
    }
    Ok(())
}
fn valid_confidence(n: f64) -> bool {
    n.is_finite() && (0.0..=1.0).contains(&n)
}
fn attribute<'a>(entity: &'a EntityType, tag: &str) -> Option<&'a str> {
    if let EntityType::Insert(insert) = entity {
        insert
            .attributes
            .iter()
            .find(|a| a.tag.eq_ignore_ascii_case(tag))
            .map(|a| a.value.as_str())
    } else {
        None
    }
}
fn number(text: &str) -> Result<f64, CoreError> {
    let normalized: String = text
        .trim()
        .chars()
        .map(|c| match c {
            '٠'..='٩' => char::from_u32('0' as u32 + c as u32 - '٠' as u32).unwrap(),
            '۰'..='۹' => char::from_u32('0' as u32 + c as u32 - '۰' as u32).unwrap(),
            '٫' => '.',
            c => c,
        })
        .collect();
    normalized
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| invalid(format!("Expected a number, found {text:?}")))
}
fn centre(row: &EntityRow) -> [f64; 2] {
    [
        (row.bounds[0][0] + row.bounds[1][0]) * 0.5,
        (row.bounds[0][1] + row.bounds[1][1]) * 0.5,
    ]
}
fn position(e: &EntityType, row: &EntityRow) -> [f64; 2] {
    match e {
        EntityType::Text(t) => [t.insertion_point.x, t.insertion_point.y],
        EntityType::MText(t) => [t.insertion_point.x, t.insertion_point.y],
        _ => centre(row),
    }
}
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
pub(crate) fn source_of(e: &EntityType) -> Evidence {
    let mut source: Evidence = crate::xdata::read_json(e.common(), SOURCE_APP).unwrap_or_default();
    source.handle = Some(e.common().handle.value());
    source
}

/// Where an extracted property value came from, for the working an engineer reads.
fn extraction_source(
    extraction: &Extraction,
    evidence: &[Evidence],
) -> crate::measurement::PropSource {
    use crate::measurement::{PropSource, SourceKind};
    let first = evidence.first();
    let handle = first.and_then(|e| e.handle);
    let detail = first.map(|e| e.detail.clone()).unwrap_or_default();
    let (kind, text) = match extraction {
        Extraction::BlockName => (SourceKind::DrawingBlock, detail),
        Extraction::Constant {
            source,
            source_kind: kind,
            ..
        } => {
            // A cited constant: a schedule, a note, a specification or the engineer's word.
            (kind.unwrap_or(SourceKind::Document), source.clone())
        }
        Extraction::Attribute { tag, .. } => {
            (SourceKind::DrawingAttribute, format!("attribute {tag}"))
        }
        Extraction::Text { .. } => (SourceKind::DrawingNote, detail),
        Extraction::ParallelDistance { .. } | Extraction::Geometry { .. } => {
            (SourceKind::Measured, String::new())
        }
    };
    PropSource { kind, handle, text }
}
