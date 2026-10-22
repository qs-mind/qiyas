use super::invalid;
use crate::{CoreError, EntityFilter, Session};
use opencadcodec::{EntityType, Handle};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum RegisterField {
    Number,
    Title,
    Revision,
    Date,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterRule {
    pub field: RegisterField,
    /// Exact attribute tag; no vendor or trade tags are built in.
    #[serde(default)]
    pub attribute_tag: Option<String>,
    /// Regular expression with a first capture containing the value.
    #[serde(default)]
    pub text_pattern: Option<String>,
    #[serde(default)]
    pub layer: Option<String>,
    #[serde(default)]
    pub block: Option<String>,
    #[serde(default)]
    pub sheet: Option<String>,
}
impl RegisterRule {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.attribute_tag.is_some() == self.text_pattern.is_some() {
            return Err(invalid(
                "Register rules need exactly one attribute tag or text pattern",
            ));
        }
        if self
            .attribute_tag
            .as_ref()
            .is_some_and(|t| t.trim().is_empty())
        {
            return Err(invalid("Empty attribute tag"));
        }
        if let Some(pattern) = &self.text_pattern {
            let regex = regex::Regex::new(pattern).map_err(|e| invalid(e.to_string()))?;
            if regex.captures_len() < 2 {
                return Err(invalid("Register text patterns need a value capture"));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegisterEvidence {
    pub field: RegisterField,
    pub handle: u64,
    pub value: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegisterRow {
    pub drawing: String,
    pub sheet: String,
    pub number: Option<String>,
    pub title: Option<String>,
    pub revision: Option<String>,
    pub date: Option<String>,
    pub flags: Vec<String>,
    pub evidence: Vec<RegisterEvidence>,
}
pub(super) fn read(
    session: &Session,
    drawing: &str,
    layouts: &[String],
    rules: &[RegisterRule],
) -> Result<Vec<RegisterRow>, CoreError> {
    for rule in rules {
        rule.validate()?;
    }
    let mut result = vec![];
    for sheet in session.drawing.sheets() {
        if !layouts.is_empty() && !layouts.contains(&sheet.name) && !layouts.contains(&sheet.block)
        {
            continue;
        }
        let filter = EntityFilter {
            sheet: Some(sheet.block.clone()),
            ..Default::default()
        };
        let xrefs = session.xref_docs();
        let entities: Vec<_> = session
            .matching(&filter, &xrefs)
            .into_iter()
            .map(|c| session.entity_row(&xrefs, c))
            .filter(|e| e.xref.is_none())
            .collect();
        if entities.is_empty() {
            continue;
        }
        let mut values: BTreeMap<RegisterField, Vec<String>> = BTreeMap::new();
        let mut evidence = vec![];
        let mut add = |field, handle, value: String| {
            let value = value.trim().to_string();
            if !value.is_empty() {
                values.entry(field).or_default().push(value.clone());
                evidence.push(RegisterEvidence {
                    field,
                    handle,
                    value,
                });
            }
        };
        for rule in rules.iter().filter(|r| {
            r.sheet
                .as_ref()
                .is_none_or(|s| s == &sheet.name || s == &sheet.block)
        }) {
            if let Some(tag) = &rule.attribute_tag {
                for entity in &entities {
                    if rule.layer.as_ref().is_some_and(|l| l != &entity.layer)
                        || rule
                            .block
                            .as_ref()
                            .is_some_and(|b| entity.block_name.as_ref() != Some(b))
                    {
                        continue;
                    }
                    if let Some(EntityType::Insert(insert)) =
                        session.drawing.doc.get_entity(Handle::new(entity.handle))
                    {
                        for attr in insert
                            .attributes
                            .iter()
                            .filter(|a| a.tag.eq_ignore_ascii_case(tag))
                        {
                            add(rule.field, entity.handle, attr.value.clone());
                        }
                    }
                }
            }
            if let Some(pattern) = &rule.text_pattern {
                let regex = regex::Regex::new(pattern).map_err(|e| invalid(e.to_string()))?;
                for text in session
                    .read_text(Some(&sheet.block), None)
                    .into_iter()
                    .filter(|t| t.xref.is_none())
                {
                    if rule
                        .block
                        .as_ref()
                        .is_some_and(|b| text.inside_block.as_ref() != Some(b))
                    {
                        continue;
                    }
                    if rule.layer.as_ref().is_some_and(|l| {
                        session
                            .drawing
                            .doc
                            .get_entity(Handle::new(text.handle))
                            .is_none_or(|e| &e.common().layer != l)
                    }) {
                        continue;
                    }
                    for capture in regex.captures_iter(&text.text) {
                        if let Some(value) = capture.get(1) {
                            add(rule.field, text.handle, value.as_str().into());
                        }
                    }
                }
            }
        }
        let mut flags = vec![];
        let mut field = |field: RegisterField| {
            let mut candidates = values.remove(&field).unwrap_or_default();
            candidates.sort();
            candidates.dedup();
            if candidates.len() == 1 {
                candidates.pop()
            } else {
                flags.push(format!(
                    "{} {field:?}",
                    if candidates.is_empty() {
                        "Missing"
                    } else {
                        "Ambiguous"
                    }
                ));
                None
            }
        };
        let (number, title, revision, date) = (
            field(RegisterField::Number),
            field(RegisterField::Title),
            field(RegisterField::Revision),
            field(RegisterField::Date),
        );
        result.push(RegisterRow {
            drawing: drawing.into(),
            sheet: sheet.block,
            number,
            title,
            revision,
            date,
            flags,
            evidence,
        });
    }
    if result.is_empty() {
        result.push(RegisterRow {
            drawing: drawing.into(),
            sheet: String::new(),
            number: None,
            title: None,
            revision: None,
            date: None,
            flags: vec!["No populated sheets; register metadata missing".into()],
            evidence: vec![],
        });
    }
    Ok(result)
}
