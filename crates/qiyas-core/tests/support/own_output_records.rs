//! Existing records named by the registry examples, created through the command bus.

use opencadcodec::{EntityType, Handle};
use qiyas_core::{Command, Session};
use serde_json::{json, Value};

fn run(session: &mut Session, input: Value) {
    let command: Command = serde_json::from_value(input).unwrap();
    session.run(command).unwrap();
}

fn layer(session: &mut Session, name: &str) {
    if session.drawing.doc.layers.get(name).is_none() {
        run(session, json!({"type":"new_layer","name":name}));
    }
}

fn viewport(session: &mut Session) -> Vec<u64> {
    run(
        session,
        json!({"type":"new_layout","name":"Guard paper","paper":"ISO A1","orientation":"landscape"}),
    );
    let sheet = session
        .drawing
        .sheets()
        .into_iter()
        .find(|sheet| sheet.name == "Guard paper")
        .unwrap();
    session.set_sheet(&sheet.block).unwrap();
    // Viewport corners are physical paper millimetres in every model unit.
    let before: std::collections::BTreeSet<_> = session
        .drawing
        .doc
        .entities()
        .map(|entity| entity.common().handle.value())
        .collect();
    run(
        session,
        json!({"type":"mview","corner1":[20,20],"corner2":[200,150],"scale":"1:100","centre":[0,0]}),
    );
    session
        .drawing
        .doc
        .entities()
        .filter(|entity| matches!(entity, EntityType::Viewport(_)))
        .map(|entity| entity.common().handle.value())
        .filter(|handle| !before.contains(handle))
        .collect()
}

fn attributes(session: &mut Session, block: &str) -> Vec<u64> {
    let definitions = super::added_by(
        session,
        json!({
            "type":"att_def","tag":"ROOM_NO","prompt":"Room number","default":"G00",
            "at":[0,0],"height":0.25
        }),
    );
    super::added_by(
        session,
        json!({
            "type":"create_block","name":block,"base":[0,0],"convert_to_insert":false,
            "targets":definitions.iter().map(|handle| json!({"handle":handle})).collect::<Vec<_>>()
        }),
    );
    super::added_by(session, json!({"type":"insert_block","name":block,"at":[10,0],"attribute_values":{"ROOM_NO":"G00"}}))
        .into_iter().filter(|handle| matches!(
            session.drawing.doc.get_entity(Handle::new(*handle)), Some(EntityType::Insert(_))
        )).collect()
}

pub fn prepare(session: &mut Session, name: &str, example: &mut Value) -> Option<Vec<u64>> {
    let mut handles = Vec::new();
    match name {
        "set_layer" | "delete_layer" => layer(session, example["name"].as_str().unwrap()),
        "rename_layer" => layer(session, example["from"].as_str().unwrap()),
        "translate_layers" => {
            for mapping in example["mappings"].as_array().unwrap() {
                layer(session, mapping["from"].as_str().unwrap());
            }
        }
        "restore_layer_state" | "delete_layer_state" | "rename_layer_state" => {
            layer(session, "A-WALL");
            let state = example[if name == "rename_layer_state" {
                "from"
            } else {
                "name"
            }]
            .as_str()
            .unwrap();
            run(session, json!({"type":"save_layer_state","name":state}));
        }
        "set_viewport_frozen" => {
            layer(session, example["layer"].as_str().unwrap());
            handles = viewport(session);
        }
        "add_to_group" | "remove_from_group" | "delete_group" => {
            for y in [0, 2, 4] {
                handles.extend(super::added_by(
                    session,
                    json!({"type":"draw","kind":"line","a":[0,y],"b":[5,y]}),
                ));
            }
            let members = if name == "add_to_group" {
                &handles[..2]
            } else {
                &handles[..]
            };
            run(
                session,
                json!({"type":"create_group","name":example["name"],
                "targets":members.iter().map(|handle| json!({"handle":handle})).collect::<Vec<_>>() }),
            );
        }
        "edit_attributes" | "edit_att_def" | "sync_attributes" => {
            let block = example["block"].as_str().unwrap_or("ROOM-TAG");
            handles = attributes(session, block);
        }
        "set_property" => {
            if example["prop"] == "viewport_scale" {
                handles = viewport(session);
            } else {
                layer(session, example["value"].as_str().unwrap());
                handles = super::added_by(
                    session,
                    json!({"type":"draw","kind":"line","a":[0,0],"b":[5,0]}),
                );
            }
        }
        "set_records" => match example["collection"].as_str().unwrap() {
            "entities" => {
                handles = super::added_by(
                    session,
                    json!({"type":"draw","kind":"circle","centre":[0,0],"radius":1}),
                );
                // Record values do not carry a geometric field name for the outer scaler.
                for update in example["updates"].as_array_mut().unwrap() {
                    if update["path"] == "/radius" {
                        update["value"] = json!(
                            update["value"].as_f64().unwrap() / session.drawing.units.to_metres()
                        );
                    }
                }
            }
            "layers" => {
                for update in example["updates"].as_array().unwrap() {
                    layer(session, update["key"].as_str().unwrap());
                }
            }
            "viewports" => {
                handles = viewport(session);
                for update in example["updates"].as_array_mut().unwrap() {
                    if update["path"] == "/view_height" {
                        update["value"] = json!(
                            update["value"].as_f64().unwrap() / session.drawing.units.to_metres()
                        );
                    }
                }
            }
            collection => panic!("Add a fixture for set_records {collection}"),
        },
        _ => return None,
    }
    Some(handles)
}
