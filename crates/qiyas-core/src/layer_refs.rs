//! Rename persisted layer references together with the drawing's layer table.
use serde_json::Value;

use crate::{CoreError, Session};

fn rename(value: &mut Value, from: &str, to: &str, reference: bool) {
    match value {
        Value::String(name) if reference && name.eq_ignore_ascii_case(from) => *name = to.into(),
        Value::Array(values) => values
            .iter_mut()
            .for_each(|v| rename(v, from, to, reference)),
        Value::Object(map) => {
            for (key, value) in map {
                let layer = reference
                    || matches!(key.as_str(), "layer" | "layers" | "role_layers")
                    || key.ends_with("_layer")
                    || key.ends_with("_layers");
                if layer && value.get("kind").is_some() {
                    // A property declaration contains type/description text too. Only its
                    // default and choices are layer references.
                    if let Some(default) = value.get_mut("default") {
                        rename(default, from, to, true);
                    }
                    if let Some(choices) = value.get_mut("choices") {
                        rename(choices, from, to, true);
                    }
                } else {
                    rename(value, from, to, layer);
                }
            }
        }
        _ => {}
    }
}

impl Session {
    pub(crate) fn rename_layer_references(
        &mut self,
        from: &str,
        to: &str,
    ) -> Result<(), CoreError> {
        let mut bindings = std::collections::BTreeMap::<String, Vec<String>>::new();
        let mut host_properties = Vec::new();
        for entry in crate::knowledge::list(&self.drawing.doc, crate::knowledge::Kind::Types)? {
            if let Ok(definition) = serde_json::from_value::<crate::objects::ObjectType>(entry.data)
            {
                bindings.insert(
                    entry.name,
                    definition.plan.layer_properties.values().cloned().collect(),
                );
                host_properties.extend(definition.plan.host_layer_property);
            }
        }
        for kind in crate::knowledge::Kind::ALL {
            for entry in crate::knowledge::list(&self.drawing.doc, kind)? {
                let mut data = entry.data.clone();
                rename(&mut data, from, to, false);
                if kind == crate::knowledge::Kind::Types {
                    for property in bindings
                        .get(&entry.name)
                        .into_iter()
                        .flatten()
                        .chain(&host_properties)
                    {
                        if let Some(definition) =
                            data.get_mut("properties").and_then(|p| p.get_mut(property))
                        {
                            for field in ["default", "choices"] {
                                if let Some(value) = definition.get_mut(field) {
                                    rename(value, from, to, true);
                                }
                            }
                        }
                    }
                }
                if data != entry.data {
                    crate::knowledge::put(&mut self.drawing.doc, kind, &entry.name, &data)?;
                }
            }
        }
        let handles: Vec<_> = self
            .drawing
            .doc
            .entities()
            .map(|e| e.common().handle)
            .collect();
        for handle in handles {
            // Wall records also carry legacy thickness/height values, retained by stamp_wall.
            if let Some(mut tag) = self
                .drawing
                .doc
                .get_entity(handle)
                .and_then(crate::walls::wall_tag)
            {
                let names = crate::walls::resolved_layers(&tag.spec.layers);
                let names = names.map(|name| {
                    if name.eq_ignore_ascii_case(from) {
                        to.into()
                    } else {
                        name
                    }
                });
                tag.spec.layers = crate::WallLayers {
                    outline: Some(names[0].clone()),
                    hatch: Some(names[1].clone()),
                    centreline: Some(names[2].clone()),
                    doors: Some(names[3].clone()),
                    windows: Some(names[4].clone()),
                };
                self.stamp_wall(handle.value(), &tag)?;
            }
            for app in [
                crate::objects::OBJECT_APP,
                crate::objects::SCHEDULE_APP,
                "QIYAS_ROOM",
                "QIYAS_ROOM_SCHEDULE",
            ] {
                let entity = self.drawing.doc.get_entity(handle).expect("listed entity");
                let Some(mut data) = crate::xdata::read_json::<Value>(entity.common(), app) else {
                    continue;
                };
                let before = data.clone();
                rename(&mut data, from, to, false);
                if app == crate::objects::OBJECT_APP {
                    let name = data["type"].as_str().unwrap_or_default().to_string();
                    for property in bindings
                        .get(&name)
                        .into_iter()
                        .flatten()
                        .chain(&host_properties)
                    {
                        if let Some(value) = data["props"].get_mut(property) {
                            rename(value, from, to, true);
                        }
                    }
                }
                if data != before {
                    let app_handle = crate::xdata::app_handle(&self.drawing.doc, app);
                    crate::xdata::write_json(
                        self.drawing
                            .doc
                            .get_entity_mut(handle)
                            .expect("listed entity")
                            .common_mut(),
                        app,
                        app_handle,
                        &data,
                    )?;
                }
            }
        }
        Ok(())
    }
}
