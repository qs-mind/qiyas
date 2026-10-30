//! Recorded handles identify fixture runs, not allocator positions in a new drawing.
use serde_json::{json, Value};

pub fn calls() -> Vec<Value> {
    serde_json::from_str(include_str!("../fixtures/villa-openings.json")).unwrap()
}

pub fn bind_openings(input: &mut Value, groups: &Value, factor: f64) {
    let fixture = calls();
    let walls = &fixture[3]["input"]["args"]["commands"];
    for command in input["args"]["commands"].as_array_mut().unwrap() {
        if command["type"] != "add_wall_openings" {
            continue;
        }
        let recorded = command["centreline"].as_u64().unwrap();
        let (system, index) = match recorded {
            143..=163 => (0, (recorded - 143) as usize),
            190 => (1, 0),
            _ => panic!("unknown recorded wall {recorded}"),
        };
        let points: Vec<[f64; 2]> =
            serde_json::from_value(walls[system]["runs"][index]["points"].clone()).unwrap();
        let expected = json!(points
            .into_iter()
            .map(|[x, y]| [x / factor, y / factor])
            .collect::<Vec<_>>());
        let matches: Vec<_> = groups
            .as_array()
            .expect("wall_groups readback")
            .iter()
            .flat_map(|group| group["runs"].as_array().unwrap())
            .filter(|run| run["points"] == expected)
            .collect();
        assert_eq!(matches.len(), 1, "unique fixture wall {recorded}");
        command["centreline"] = matches[0]["centreline"].clone();
    }
}
