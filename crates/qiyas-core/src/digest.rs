//! Compact drawing evidence and take-off checks. Interpretation remains drawing-owned data.
use std::collections::{BTreeMap, BTreeSet};

use opencadcodec::{EntityType, Handle};
use serde_json::{json, Value};

use crate::objects::ObjectFilter;
use crate::recognition::ReviewState;
use crate::{CoreError, EntityFilter, Session};

pub const MAX_BYTES: usize = 6144;

fn short(s: &str) -> String {
    let mut text: String = s.chars().take(96).collect();
    if s.chars().count() > 96 {
        text.push('…');
    }
    text
}

/// Page complete entries, retaining the same overview on every page. Large source text is
/// previewed, with its handle available for a full records read.
fn page(mut overview: Value, entries: Vec<Value>, offset: usize) -> Value {
    overview["total_entries"] = json!(entries.len());
    overview["offset"] = json!(offset);
    overview["entries"] = json!([]);
    overview["next_offset"] = Value::Null;
    for (i, mut entry) in entries.into_iter().enumerate().skip(offset) {
        if serde_json::to_vec(&entry).unwrap().len() > 3000 {
            entry = json!({"section":entry["section"],"handle":entry["handle"],"row":entry["row"],"preview":short(&entry.to_string()),"detail":"Read the source record by handle"});
        }
        overview["entries"].as_array_mut().unwrap().push(entry);
        overview["next_offset"] = json!(i + 1);
        if serde_json::to_vec(&overview).unwrap().len() > MAX_BYTES {
            overview["entries"].as_array_mut().unwrap().pop();
            overview["next_offset"] = json!(i);
            break;
        }
    }
    if overview["next_offset"] == overview["total_entries"] {
        overview["next_offset"] = Value::Null;
    }
    overview
}

impl Session {
    /// All sheets, including hidden layers; no trade names or language-specific label lists.
    pub fn drawing_digest(&self, offset: usize) -> Result<Value, CoreError> {
        let info = self.info();
        let types = self.object_types()?;
        let rules = self.recognition_rules()?;
        let report = self.measurement_report()?;
        let selection = self.measurement_selection()?;
        let objects = self.objects(&ObjectFilter::default())?;
        let mut entries = Vec::new();
        let mut layers = BTreeMap::<String, BTreeMap<String, usize>>::new();
        for layer in &info.layers {
            layers.entry(layer.name.clone()).or_default();
        }
        let mut blocks = BTreeMap::<String, (usize, BTreeSet<String>)>::new();
        let mut hatches = BTreeMap::<String, usize>::new();
        let mut regions = 0;
        let mut area = 0.0;
        let mut text_entries = Vec::new();
        for sheet in &info.sheets {
            entries.push(json!({"section":"sheets","name":short(&sheet.name),"block":sheet.block,"model":sheet.is_model}));
            let query = EntityFilter {
                sheet: Some(sheet.block.clone()),
                ..Default::default()
            };
            let xrefs = self.xref_docs();
            let rows: Vec<_> = self
                .matching(&query, &xrefs)
                .into_iter()
                .map(|c| self.entity_row(&xrefs, c))
                .collect();
            let closed: Vec<_> = rows
                .iter()
                .filter(|r| r.area.is_some_and(|a| a > 0.0))
                .collect();
            regions += closed.len();
            area += closed.iter().filter_map(|r| r.area).sum::<f64>();
            for row in &rows {
                *layers
                    .entry(row.layer.clone())
                    .or_default()
                    .entry(row.kind.clone())
                    .or_default() += 1;
                // Xref rows are counted, but their handles belong to a different document.
                let entity = row
                    .xref
                    .is_none()
                    .then(|| self.drawing.doc.get_entity(Handle::new(row.handle)))
                    .flatten();
                if let Some(EntityType::Underlay(underlay)) = entity {
                    if let Some(opencadcodec::objects::ObjectType::UnderlayDefinition(def)) =
                        self.drawing.doc.objects.get(&underlay.definition_handle)
                    {
                        let path = std::path::Path::new(&def.file_path);
                        let path = if path.is_absolute() {
                            path.to_path_buf()
                        } else {
                            self.drawing
                                .path
                                .parent()
                                .unwrap_or(std::path::Path::new("."))
                                .join(path)
                        };
                        let path = std::path::absolute(path)?;
                        entries.push(json!({"section":"documents","handle":row.handle,"path":path,
                            "page":def.page_name.trim().parse::<u32>().unwrap_or(1),
                            "capture":{"source":"document","args":{"path":path,"page":def.page_name.trim().parse::<u32>().unwrap_or(1)}},
                            "note":"Scans need capture, visible calibration and recognition.vision; review raster hints."}));
                    }
                }
                if let Some(name) = &row.block_name {
                    let (count, tags) = blocks.entry(name.clone()).or_default();
                    *count += 1;
                    if let Some(EntityType::Insert(insert)) = entity {
                        tags.extend(insert.attributes.iter().map(|a| a.tag.clone()));
                    }
                }
                if let Some(EntityType::Hatch(h)) = entity {
                    *hatches.entry(h.pattern.name.clone()).or_default() += 1;
                }
                if let Some(EntityType::Table(table)) = entity {
                    let grid = crate::table::table_info(table);
                    for (index, cells) in grid.cells.iter().enumerate() {
                        for (column, cell) in cells.iter().enumerate() {
                            text_entries.push(json!({"section":"text_clusters","kind":"table_or_schedule","sheet":sheet.block,"handle":row.handle,"row":index,"column":column,"text":cell}));
                        }
                    }
                }
            }
            for schedule in self.read_tables(Some(&sheet.block)) {
                for (column, heading) in schedule.columns.iter().enumerate() {
                    text_entries.push(json!({"section":"schedule_columns","sheet":sheet.block,"handle":schedule.source,"schedule":schedule.name,"column":column,"heading":heading}));
                }
            }
            let texts = self.read_text(Some(&sheet.block), None);
            // Nearby baselines form possible legend/schedule rows. These are evidence, never
            // a semantic assertion that every aligned annotation is a schedule.
            let mut ordered: Vec<_> = texts.iter().collect();
            ordered.sort_by(|a, b| {
                b.at[1]
                    .total_cmp(&a.at[1])
                    .then(a.at[0].total_cmp(&b.at[0]))
            });
            let mut groups: Vec<Vec<&crate::query::TextRow>> = Vec::new();
            for text in ordered {
                if let Some(group) = groups.last_mut().filter(|g| {
                    (g[0].at[1] - text.at[1]).abs()
                        <= g[0].height.max(text.height).abs().max(f64::EPSILON)
                }) {
                    group.push(text);
                } else {
                    groups.push(vec![text]);
                }
            }
            for (cluster, group) in groups.into_iter().enumerate() {
                let group_size = group.len();
                let grid = group_size > 1;
                for text in group {
                    let enclosed = closed.iter().any(|r| {
                        text.at[0] >= r.bounds[0][0]
                            && text.at[0] <= r.bounds[1][0]
                            && text.at[1] >= r.bounds[0][1]
                            && text.at[1] <= r.bounds[1][1]
                    });
                    let mut nearest: Vec<_> = rows
                        .iter()
                        .filter(|r| {
                            r.handle != text.handle
                                && (r.length.is_some()
                                    || r.area.is_some()
                                    || r.block_name.is_some())
                        })
                        .map(|r| {
                            let dx = (r.bounds[0][0] - text.at[0])
                                .max(0.0)
                                .max(text.at[0] - r.bounds[1][0]);
                            let dy = (r.bounds[0][1] - text.at[1])
                                .max(0.0)
                                .max(text.at[1] - r.bounds[1][1]);
                            (dx.hypot(dy), r)
                        })
                        .collect();
                    nearest
                        .sort_by(|(a, ar), (b, br)| a.total_cmp(b).then(ar.handle.cmp(&br.handle)));
                    let closest: Vec<_> = nearest.iter().take(3).map(|(distance, r)|
                        json!({"handle":r.handle,"layer":r.layer,"distance_m":distance*self.drawing.units.to_metres(),"xref":r.xref})).collect();
                    // Preserve complete note text across entries instead of hiding its scope
                    // behind a 96-character preview. Proximity is evidence, not applicability.
                    let chars: Vec<_> = text.text.chars().collect();
                    for (part, chunk) in chars.chunks(600).enumerate() {
                        text_entries.push(json!({"section":"text_clusters","cluster":cluster,"group_size":group_size,"kind":if grid {"possible_legend_or_schedule"} else if enclosed {"possible_room_label"} else {"text"},"sheet":sheet.block,"handle":text.handle,"at":text.at,"text":chunk.iter().collect::<String>(),"part":part,"closest":closest,"xref":text.xref}));
                    }
                }
            }
        }
        for (name, kinds) in &layers {
            entries.push(json!({"section":"layers","name":short(name),"kinds":kinds}));
        }
        for (name, (inserts, tags)) in &blocks {
            entries.push(json!({"section":"blocks","name":short(name),"inserts":inserts,"attribute_tags":tags.iter().take(6).map(|t|short(t)).collect::<Vec<_>>(),"tag_count":tags.len()}));
        }
        for (pattern, count) in &hatches {
            entries.push(json!({"section":"hatches","pattern":short(pattern),"count":count}));
        }
        for t in &types {
            entries.push(json!({"section":"element_types","name":short(&t.name),"geometry":t.geometry,"instances":objects.iter().filter(|o|o.instance.type_name==t.name).count()}));
        }
        for rule in &rules {
            entries.push(json!({"section":"recognition_rules","name":short(&rule.name),"element_type":short(&rule.object_type),"confidence":rule.confidence}));
        }
        if let Some(method) = selection.effective() {
            for rule in &method.rules {
                entries.push(json!({"section":"measurement_rules","id":short(&rule.id),"element_type":rule.element_type.as_deref().map(short),"unit":rule.unit}));
            }
        }
        for item in &self.takeoff.items {
            entries.push(json!({"section":"existing_takeoff","id":item.id,"description":short(&item.description),"quantity":item.shown,"unit":item.unit,"source_count":item.handles.len()}));
        }
        for item in &report.items {
            entries.push(json!({"section":"generated_boq","code":short(&item.code),"description":short(&item.description_en),"quantity":item.quantity,"unit":item.unit}));
        }
        let mut text_kinds = BTreeMap::<String, usize>::new();
        for entry in &text_entries {
            if let Some(kind) = entry["kind"].as_str() {
                *text_kinds.entry(kind.into()).or_default() += 1;
            }
        }
        entries.extend(text_entries);
        // Use an actual insert's paired names, without assigning it a trade or meaning.
        let example_names = self
            .matching(&EntityFilter::default(), &self.xref_docs())
            .into_iter()
            .map(|c| self.entity_row(&self.xref_docs(), c))
            .find(|row| row.xref.is_none() && row.block_name.is_some())
            .map(|row| json!({"layer":row.layer,"block":row.block_name}));
        let overview = json!({"takeoff_kit":{"op":"takeoff_kit","args":{}},"example_names":example_names,"units":info.units,"metres_per_unit":self.drawing.units.to_metres(),"annotation_scale":info.annotation_scale,"scale_note":"Annotation scale is not PDF calibration. Verify source dimensions before measuring imported pages.","text_clusters":text_kinds,"counts":{"sheets":info.sheets.len(),"layers":layers.len(),"blocks":blocks.len(),"element_types":types.len(),"recognition_rules":rules.len(),"elements":objects.len(),"manual_takeoff":self.takeoff.items.len(),"generated_boq":report.items.len()},"closed_regions":{"count":regions,"summed_area_m2":area*self.drawing.units.to_metres().powi(2),"note":"May overlap; candidates only"},"measurement_method":{"name":selection.effective().map(|m|short(&m.name)),"scope":if selection.project.is_some(){"project"}else{"settings"},"rules_loaded":selection.effective().is_some_and(|m|!m.rules.is_empty())},"picture":{"tool":"qiyas_capture","source":"picture","args":{"sheet":self.sheet,"width":1200,"marks":true}},"detail":"takeoff_kit for complete examples and batch; records by handle for missing evidence; next_offset pages remaining evidence"});
        Ok(page(overview, entries, offset))
    }

    /// Coverage describes explanation by valid elements, not benchmark truth or completeness
    /// of the real project. Zero denominators remain null, and unresolved evidence is retained.
    pub fn takeoff_check(
        &self,
        offset: usize,
        expectations: &[crate::verify::Expectation],
    ) -> Result<Value, CoreError> {
        let objects = self.objects(&ObjectFilter::default())?;
        let evidence = self.recognition_evidence()?;
        let valid: BTreeSet<_> = objects
            .iter()
            .filter(|o| o.valid)
            .map(|o| o.instance.id)
            .collect();
        let mut explained = valid.clone();
        for c in &evidence {
            if c.state == ReviewState::Accepted && c.object.is_some_and(|h| valid.contains(&h)) {
                explained.insert(c.handle);
            }
        }
        let mut layer_counts = BTreeMap::<String, (usize, usize)>::new();
        let mut block_counts = BTreeMap::<String, (usize, usize)>::new();
        let mut closed = (0, 0);
        let mut entries = Vec::new();
        let rules = self.recognition_rules()?;
        let mut unparsed_rows = 0;
        for sheet in self.info().sheets {
            let mut query = EntityFilter {
                sheet: Some(sheet.block.clone()),
                limit: 500,
                ..Default::default()
            };
            loop {
                let found = self.find_entities(&query);
                for row in found.rows {
                    if row.xref.is_none()
                        && !rules
                            .iter()
                            .any(|rule| rule.sources.iter().any(|e| e.handle == Some(row.handle)))
                    {
                        if let Some(EntityType::Table(t)) =
                            self.drawing.doc.get_entity(Handle::new(row.handle))
                        {
                            for (index, cells) in
                                crate::table::table_info(t).cells.iter().enumerate()
                            {
                                unparsed_rows += 1;
                                entries.push(json!({"section":"unmapped_table_rows","handle":row.handle,"row":index,"cells":cells.iter().take(4).map(|s|short(s)).collect::<Vec<_>>() }));
                            }
                        }
                    }
                    let yes = usize::from(row.xref.is_none() && explained.contains(&row.handle));
                    let counts = layer_counts.entry(row.layer.clone()).or_default();
                    counts.0 += 1;
                    counts.1 += yes;
                    if let Some(block) = row.block_name {
                        let c = block_counts.entry(block).or_default();
                        c.0 += 1;
                        c.1 += yes;
                    }
                    if row.area.is_some_and(|a| a > 0.0) {
                        closed.0 += 1;
                        closed.1 += yes;
                    }
                    if yes == 0 {
                        entries.push(json!({"section":"unexplained","sheet":sheet.block,"handle":row.handle,"kind":row.kind,"layer":short(&row.layer),"xref":row.xref}));
                    }
                }
                let Some(next) = found.next_offset else { break };
                query.offset = next;
            }
        }
        let fraction = |n: usize, d: usize| {
            if d == 0 {
                Value::Null
            } else {
                json!(n as f64 / d as f64)
            }
        };
        let counts =
            |c: (usize, usize)| json!({"total":c.0,"explained":c.1,"share":fraction(c.1,c.0)});
        let mut detail = Vec::new();
        for (name, c) in &layer_counts {
            detail.push(json!({"section":"layers","name":short(name),"coverage":counts(*c)}));
        }
        for (name, c) in &block_counts {
            detail.push(json!({"section":"blocks","name":short(name),"coverage":counts(*c)}));
        }
        let mut unmatched = unparsed_rows;
        for rule in rules {
            if rule.sources.is_empty() {
                continue;
            }
            let found = evidence.iter().any(|c| {
                c.rule == rule.name
                    && c.state == ReviewState::Accepted
                    && c.object.is_some_and(|h| valid.contains(&h))
            });
            if !found {
                unmatched += 1;
                detail.push(json!({"section":"unmatched_schedule_rows","rule":short(&rule.name),"sources":rule.sources.iter().take(4).collect::<Vec<_>>()}));
            }
        }
        let low: Vec<_> = evidence
            .iter()
            .filter(|c| {
                c.state == ReviewState::Pending || !c.issues.is_empty() || c.confidence < 0.9
            })
            .collect();
        for c in &low {
            detail.push(json!({"section":"low_confidence","handle":c.handle,"confidence":c.confidence,"state":c.state,"rule":short(&c.rule),"issues":c.issues.iter().map(|s|short(s)).take(6).collect::<Vec<_>>() }));
        }
        let report = self.measurement_report()?;
        for warning in &report.warnings {
            detail.push(json!({"section":"measurement_warnings","message":short(warning)}));
        }
        let mut basis_warnings = Vec::new();
        if let Some(method) = report.selection.effective() {
            let notes: Vec<_> = self
                .info()
                .sheets
                .iter()
                .flat_map(|sheet| self.read_text(Some(&sheet.block), None))
                .collect();
            for rule in &method.rules {
                for term in rule
                    .terms
                    .iter()
                    .chain(rule.bands.iter().flat_map(|b| &b.terms))
                {
                    for note in &notes {
                        let citation = format!(
                            "{} {} {} {}",
                            rule.citation.source,
                            rule.citation.clause,
                            term.citation.source,
                            term.citation.clause
                        );
                        let cited =
                            citation
                                .split(|c: char| !c.is_ascii_alphanumeric())
                                .any(|word| {
                                    word == note.handle.to_string()
                                        || word
                                            .eq_ignore_ascii_case(&format!("0x{:X}", note.handle))
                                })
                                || citation.contains(&note.text);
                        if cited && !note.text.is_empty() {
                            let unit = rule
                                .bands
                                .iter()
                                .find(|b| b.terms.contains(term))
                                .map_or(rule.unit.as_str(), |b| b.unit.as_str());
                            for reason in note_basis_warnings(term, unit, &note.text) {
                                basis_warnings.push(json!({"section":"quantity_basis_warnings","rule":rule.id,"handle":note.handle,"term":term.label,"message":reason}));
                            }
                        }
                    }
                }
            }
        }
        let basis_warning_count = basis_warnings.len();
        detail.extend(basis_warnings);
        let verification = self.verify(expectations)?;
        let verification_ok = !verification.is_empty() && verification.iter().all(|v| v.passed);
        let unexplained = entries
            .iter()
            .filter(|v| v["section"] == "unexplained")
            .count();
        for result in &verification {
            detail.push(json!({"section":"expectations","result":result}));
        }
        detail.extend(entries);
        let overview = json!({"quantity_basis_warnings":basis_warning_count,"passed":basis_warning_count==0 && unexplained==0 && unmatched==0 && low.is_empty() && !report.items.is_empty() && verification_ok,"layers":counts((layer_counts.len(),layer_counts.values().filter(|c|c.1>0).count())),"blocks":counts((block_counts.len(),block_counts.values().filter(|c|c.1>0).count())),"closed_regions":counts(closed),"unexplained_entities":unexplained,"unmatched_schedule_rows":unmatched,"low_confidence":low.len(),"block_inserts":counts(block_counts.values().fold((0,0),|(a,b),(c,d)|(a+c,b+d))),"generated_boq":report.items.len(),"expectation_count":verification.len(),"expectations_checked":verification_ok,"note":"Explanation shares are evidence coverage, not completeness. Unmapped tables/text grids require interpretation via recognition.schedule. Fix by handle and repeat, or report unresolved evidence at the budget limit; human review is optional."});
        Ok(page(overview, detail, offset))
    }
}

fn unit_power(unit: &str) -> Option<i32> {
    match unit
        .trim()
        .to_lowercase()
        .replace('²', "2")
        .replace('³', "3")
        .as_str()
    {
        "m" | "mm" | "cm" | "ft" | "in" => Some(1),
        "m2" | "mm2" | "cm2" | "ft2" | "sqm" => Some(2),
        "m3" | "mm3" | "cm3" | "ft3" | "cum" => Some(3),
        "pcs" => Some(0),
        _ => None,
    }
}

/// Check only dimensions explicitly evidenced by a cited note. Unitless text cannot prove
/// applicability; the model must still choose the correct source geometry and interpretation.
fn note_basis_warnings(
    term: &crate::measurement::Term,
    output_unit: &str,
    note: &str,
) -> Vec<String> {
    use std::sync::LazyLock;
    static UNITS: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)([0-9]+(?:\.[0-9]+)?)\s*(mm|cm|ft|in|m)([²³]|[23]\b|\b)").unwrap()
    });
    static UNIT_NAMES: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?i)\b(?:mm|cm|ft|in|m)(?:[²³]|[23]\b|\b)").unwrap());
    static PERCENT: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"([0-9]+(?:\.[0-9]+)?)\s*%").unwrap());
    let Some(output) = unit_power(output_unit) else {
        return vec![];
    };
    let inputs: Vec<_> = term
        .factors
        .iter()
        .filter_map(|f| match f.trim() {
            "geometry.length" => Some(1),
            "geometry.area" => Some(2),
            "geometry.volume" => Some(3),
            "geometry.count" => Some(0),
            _ => None,
        })
        .collect();
    let values: Vec<f64> = term
        .factors
        .iter()
        .chain((term.timesing.trim() != "1").then_some(&term.timesing))
        .filter_map(|f| f.parse().ok())
        .collect();
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
    let mut warnings = BTreeSet::new();
    for clause in note.split([';', '\n']) {
        let units: Vec<_> = UNITS.captures_iter(clause).collect();
        for capture in &units {
            let power = capture
                .get(3)
                .filter(|p| !p.as_str().is_empty())
                .map_or(1, |p| match p.as_str() {
                    "2" | "²" => 2,
                    _ => 3,
                });
            let scale: f64 = match capture[2].to_lowercase().as_str() {
                "mm" => 0.001,
                "cm" => 0.01,
                "ft" => 0.3048,
                "in" => 0.0254,
                _ => 1.0,
            };
            let value = capture[1].parse::<f64>().unwrap() * scale.powi(power);
            if inputs.len() == 1
                && values.iter().any(|v| close(*v, value))
                && inputs[0] + power != output
            {
                warnings.insert(format!("Cited factor {} has dimension {}; input dimension {} does not produce {}. Check the quantity basis and source handles.", &capture[0], power, inputs[0], output_unit));
            }
        }
        for percent in PERCENT.captures_iter(clause) {
            let factor = percent[1].parse::<f64>().unwrap() / 100.0;
            let powers: BTreeSet<_> = UNIT_NAMES
                .find_iter(clause)
                .filter_map(|unit| unit_power(unit.as_str()))
                .collect();
            if powers.len() == 1
                && !powers.contains(&output)
                && values
                    .iter()
                    .any(|v| close(*v, factor) || close(*v, 1.0 + factor))
            {
                warnings.insert(format!("Cited percentage {} applies to dimension {}, but this rule produces {}. Check the quantity basis.", &percent[0], powers.first().unwrap(), output_unit));
            }
        }
    }
    warnings.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn note_units_check_dimensions_without_interpreting_domain_words() {
        let mut term: crate::measurement::Term = serde_json::from_value(json!({
            "label":"Derived", "operation":"add", "factors":["geometry.area","2"],
            "citation":{"source":"drawing","clause":"note 24"}
        }))
        .unwrap();
        assert_eq!(note_basis_warnings(&term, "m2", "Factor 2000 mm").len(), 1);
        term.factors[0] = "geometry.length".into();
        assert!(note_basis_warnings(&term, "m2", "Factor 2 m").is_empty());
        term.factors = vec!["geometry.area".into()];
        assert!(note_basis_warnings(&term, "m2", "Reference 1 m").is_empty());
        term.factors = vec!["geometry.area".into(), "0.1".into()];
        assert_eq!(note_basis_warnings(&term, "m2", "10% (m3)").len(), 1);
        assert_eq!(note_basis_warnings(&term, "m2", "10% of 20 m³").len(), 1);
        assert!(note_basis_warnings(&term, "m3", "10% of 20 m3").is_empty());
        assert!(note_basis_warnings(&term, "m2", "10% working factor").is_empty());
        assert!(note_basis_warnings(&term, "m2", "Value 20 m3, factor 5%").is_empty());
    }
}
