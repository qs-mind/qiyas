//! Offline clinic replay. Fixtures stay in the local audit folder, never in a provider request.
use crate::{ContentBlock, Message, ResultPart, ToolOutput};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};

#[test]
#[ignore = "requires the local clinic audit; QIYAS_CACHE_REPLAY points to its copied fixtures"]
fn clinic_policy_replay() {
    let folder = std::env::var("QIYAS_CACHE_REPLAY").expect("local fixture folder");
    let folder = Path::new(&folder);
    let read =
        |name| serde_json::from_slice::<Value>(&std::fs::read(folder.join(name)).unwrap()).unwrap();
    let saved = read("saved-history.json");
    let tools = read("live-tools.json");
    let requests = read("requests.json");
    let old = read("cache-simulation.json");
    let mut original: Vec<Message> = serde_json::from_value(saved["history"].clone()).unwrap();
    let mut known = BTreeMap::new();
    let mut padded = Vec::new();
    for message in &mut original {
        for block in &mut message.content {
            let ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } = block
            else {
                continue;
            };
            let call = tools
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == *tool_use_id)
                .unwrap();
            let saved_text = content
                .iter()
                .filter_map(|p| match p {
                    ResultPart::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            let text = if let Some(error) = call["error"].as_str() {
                error.to_owned()
            } else if call["result_truncated"] == false {
                call["result_prefix"].as_str().unwrap().to_owned()
            } else if !saved_text.starts_with("[tool result summary]") {
                saved_text
            } else {
                // The original audit also pads these lost tails. They cannot prove exact evidence.
                padded.push(call["index"].clone());
                json!({"archived_excerpt":call["result_prefix"],"unavailable_tail":"~".repeat(call["result_chars"].as_u64().unwrap() as usize)}).to_string()
            };
            let Ok(mut value) = serde_json::from_str::<Value>(&text) else {
                *content = vec![ResultPart::Text(text)];
                continue;
            };
            let name = call["tool"].as_str().unwrap();
            let op = call["input"]["op"].as_str().unwrap_or_default();
            let args = crate::surface::args_of(&call["input"]).unwrap();
            if value.get("unavailable_tail").is_some()
                && name == crate::surface::READ
                && matches!(op, "commands" | "takeoff_kit" | "record_schema")
            {
                if let Some(Ok(ToolOutput::Json(registry))) =
                    crate::surface::run_without_drawing(name, op, &args, None)
                {
                    value = registry;
                }
            }
            let mut value = if *is_error {
                crate::result::error(value)
            } else {
                let ToolOutput::Json(value) =
                    crate::result::format(name, op, &args, ToolOutput::Json(value)).unwrap()
                else {
                    unreachable!()
                };
                value
            };
            if !*is_error {
                crate::result::deduplicate(&mut value, &mut known);
            }
            if let Some(map) = value.as_object_mut() {
                map.remove("ui");
            }
            *content = vec![ResultPart::Text(
                crate::artifact::canonical(&value).to_string(),
            )];
        }
    }
    let stream = |history: &[Message]| {
        format!(
            "{}\n{}",
            crate::artifact::canonical(
                &json!({"system":saved["system"],"tools":"[three stable tool schemas]"})
            ),
            history
                .iter()
                .map(|m| crate::artifact::canonical(&serde_json::to_value(m).unwrap()).to_string())
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    let (mut history, mut previous, mut next) = (Vec::new(), String::new(), 0);
    let (mut total, mut hits, mut trims) = (0.0, 0.0, 0);
    let mut rows = Vec::new();
    for (index, request) in requests.as_array().unwrap().iter().enumerate() {
        let end = request["messages"].as_u64().unwrap() as usize;
        history.extend_from_slice(&original[next..end]);
        next = end;
        let before = history.clone();
        let trimmed = crate::history::trim(&mut history);
        if trimmed.is_some() {
            trims += 1;
        }
        for (a, b) in before.iter().zip(&history) {
            for (a, b) in a.content.iter().zip(&b.content) {
                match (a, b) {
                    (
                        ContentBlock::ToolResult {
                            tool_use_id: a,
                            is_error: x,
                            ..
                        },
                        ContentBlock::ToolResult {
                            tool_use_id: b,
                            is_error: y,
                            ..
                        },
                    ) => assert_eq!((a, x), (b, y)),
                    _ => assert_eq!(a, b, "user words, calls and reasoning must survive"),
                }
            }
        }
        let current = stream(&history);
        let prefix = previous
            .chars()
            .zip(current.chars())
            .take_while(|(a, b)| a == b)
            .count();
        let chars = current.chars().count();
        let ratio = request["total_input"].as_f64().unwrap()
            / old["requests"]["current"][index]["chars"].as_f64().unwrap();
        total += chars as f64 * ratio;
        hits += prefix as f64 * ratio;
        rows.push(json!({"request":index+1,"estimated_input":chars as f64*ratio,"ideal_reusable":prefix as f64*ratio,"trim":trimmed.is_some()}));
        previous = current;
    }
    let baseline = requests
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["total_input"].as_f64().unwrap())
        .sum::<f64>();
    let summary = json!({"baseline_input":baseline,"estimated_input":total,"reduction":1.0-total/baseline,"ideal_reusable_prefix":hits/total,"trim_requests":trims,
        "padded_calls":padded,"limitations":"Character-weighted estimate calibrated to the original logged inputs, not provider tokens or cache residency. Missing discovery bodies are regenerated from this registry; other lost tails use padded structured evidence. No live calls.","requests":rows});
    std::fs::write(
        folder.join("production-replay.json"),
        serde_json::to_vec_pretty(&summary).unwrap(),
    )
    .unwrap();
    println!(
        "estimated input={total:.0}; reduction={:.2}%; ideal reusable prefix={:.2}%; trims={trims}",
        100.0 * (1.0 - total / baseline),
        100.0 * hits / total
    );
    assert!(total <= baseline * 0.4, "at least 60% less total input");
    assert!(hits / total >= 0.90, "at least 90% ideal reusable prefix");
}
