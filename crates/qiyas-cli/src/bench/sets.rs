//! Frozen development cohorts selected by metadata, never by scores or gold quantities.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Serialize, Deserialize)]
struct CaseSet {
    version: u32,
    rule: String,
    cases: Vec<String>,
}

pub fn select(root: &Path, name: &str) -> Result<Vec<String>, String> {
    if !matches!(name, "quick" | "live-dev") {
        return Err("set must be quick or live-dev".into());
    }
    let relative = format!("sets/{name}.json");
    if root.join(&relative).exists() {
        let path = super::safe_file(root, Path::new(&relative))?;
        let set: CaseSet = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if set.version != 1 || set.cases.is_empty() {
            return Err("Invalid development set".into());
        }
        let unique: std::collections::BTreeSet<_> = set.cases.iter().collect();
        if unique.len() != set.cases.len() {
            return Err("Duplicate cases in development set".into());
        }
        for id in &set.cases {
            super::load_case(root, id)?;
        }
        return Ok(set.cases);
    }
    let ids = super::dev_ids(root, false)?;
    let mut strata = BTreeMap::<String, Vec<String>>::new();
    for id in ids {
        let (truth, _) = super::load_case(root, &id)?;
        let key = if name == "quick" {
            format!(
                "{}/{}",
                truth.project_type,
                id.split("/variants/").nth(1).unwrap_or("clean")
            )
        } else if truth.drawing.to_lowercase().ends_with(".pdf") {
            "pdf".into()
        } else if !id.contains("/variants/")
            && matches!(
                truth.project_type.as_str(),
                "apartment" | "office" | "warehouse" | "mep" | "external"
            )
        {
            truth.project_type
        } else {
            continue;
        };
        strata.entry(key).or_default().push(id);
    }
    let rule = if name == "quick" {
        "Round-robin project type × variant strata in lexical order, SHA-256(case ID) order within each, first 120; one repetition"
    } else {
        "First lexical clean case per apartment, office, warehouse, mep, external, plus first lexical PDF case; six cases"
    };
    for ids in strata.values_mut() {
        if name == "quick" {
            ids.sort_by_key(|id| format!("{:x}", Sha256::digest(id.as_bytes())));
        } else {
            ids.sort();
        }
    }
    let cases = if name == "quick" {
        let mut cases = Vec::new();
        let mut index = 0;
        while cases.len() < 120 {
            let before = cases.len();
            for ids in strata.values() {
                if let Some(id) = ids.get(index) {
                    cases.push(id.clone());
                }
                if cases.len() == 120 {
                    break;
                }
            }
            if cases.len() == before {
                break;
            }
            index += 1;
        }
        cases
    } else {
        ["apartment", "office", "warehouse", "mep", "external", "pdf"]
            .into_iter()
            .map(|key| {
                strata
                    .get(key)
                    .and_then(|ids| ids.first())
                    .cloned()
                    .ok_or_else(|| format!("live-dev needs a {key} development case"))
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    if root.join("sets").exists() {
        super::safe_file(root, Path::new("sets"))?;
    } else {
        std::fs::create_dir(root.join("sets")).map_err(|e| e.to_string())?;
    }
    super::write_json(
        &root.join(relative),
        &CaseSet {
            version: 1,
            rule: rule.into(),
            cases: cases.clone(),
        },
    )?;
    Ok(cases)
}
