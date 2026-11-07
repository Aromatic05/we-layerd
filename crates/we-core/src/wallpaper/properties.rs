use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct UserPropertySchema {
    pub entries: Vec<UserProperty>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UserProperty {
    pub key: String,
    pub label: String,
    pub kind: UserPropertyKind,
    pub default: Value,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub precision: Option<u32>,
    pub options: Vec<UserPropertyOption>,
    pub order: f64,
    pub condition: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserPropertyKind {
    Boolean,
    Slider,
    Combo,
    Color,
    Text,
    File,
    Directory,
    Html,
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct UserPropertyOption {
    pub label: String,
    pub value: Value,
}

impl UserPropertySchema {
    pub fn from_project_file(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        Self::from_project_json(&raw).with_context(|| format!("invalid JSON: {}", path.display()))
    }

    pub fn from_project_json(raw: &str) -> Result<Self> {
        let project: Value = serde_json::from_str(raw)?;
        let properties = project
            .get("general")
            .and_then(Value::as_object)
            .and_then(|general| general.get("properties"))
            .and_then(Value::as_object);

        let mut entries = properties
            .map(|properties| {
                properties
                    .iter()
                    .filter_map(|(key, descriptor)| parse_entry(key, descriptor))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        entries.sort_by(|left, right| {
            left.order.total_cmp(&right.order).then(left.key.cmp(&right.key))
        });
        Ok(Self { entries })
    }

    pub fn values_json(&self, overrides: &BTreeMap<String, Value>) -> Value {
        Value::Object(
            self.entries
                .iter()
                .map(|entry| {
                    let value =
                        overrides.get(&entry.key).cloned().unwrap_or_else(|| entry.default.clone());
                    (entry.key.clone(), value)
                })
                .collect(),
        )
    }
}

fn parse_entry(key: &str, descriptor: &Value) -> Option<UserProperty> {
    let descriptor = descriptor.as_object()?;
    let raw_kind = descriptor.get("type")?.as_str()?.to_ascii_lowercase();
    let kind = match raw_kind.as_str() {
        "bool" => UserPropertyKind::Boolean,
        "slider" => UserPropertyKind::Slider,
        "combo" => UserPropertyKind::Combo,
        "color" => UserPropertyKind::Color,
        "textinput" => UserPropertyKind::Text,
        "file" | "replacetexture" | "texture" | "scenetexture" => UserPropertyKind::File,
        "directory" => UserPropertyKind::Directory,
        "html" | "text" => UserPropertyKind::Html,
        _ => UserPropertyKind::Unsupported(raw_kind),
    };
    let options = descriptor
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            let option = option.as_object()?;
            let value = option.get("value")?.clone();
            let label = option
                .get("label")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| value.to_string());
            Some(UserPropertyOption { label, value })
        })
        .collect();
    Some(UserProperty {
        key: key.to_owned(),
        label: descriptor
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .unwrap_or(key)
            .to_owned(),
        kind,
        default: descriptor.get("value").cloned().unwrap_or(Value::Null),
        minimum: descriptor.get("min").and_then(Value::as_f64),
        maximum: descriptor.get("max").and_then(Value::as_f64),
        precision: descriptor.get("precision").and_then(Value::as_u64).map(|value| value as u32),
        options,
        order: descriptor.get("order").and_then(Value::as_f64).unwrap_or_default(),
        condition: descriptor
            .get("condition")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|condition| !condition.is_empty())
            .map(ToOwned::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::{UserPropertyKind, UserPropertySchema};

    #[test]
    fn parses_full_project_property_schema_in_display_order() {
        let schema = UserPropertySchema::from_project_json(
            r#"{"general":{"properties":{"wind":{"type":"bool","text":"Wind","value":true,"order":2},"speed":{"type":"slider","text":"Speed","value":1.5,"min":0,"max":3,"precision":2,"order":1},"theme":{"type":"combo","value":"dark","options":[{"label":"Dark","value":"dark"}]},"image":{"type":"file","value":""},"unknown":{"type":"vector","value":"0 0"}}}}"#,
        )
        .expect("schema should parse");

        assert_eq!(schema.entries[0].key, "image");
        assert_eq!(schema.entries[1].key, "theme");
        assert_eq!(schema.entries[2].key, "unknown");
        assert_eq!(schema.entries[3].key, "speed");
        assert_eq!(schema.entries[3].kind, UserPropertyKind::Slider);
        assert_eq!(schema.entries[3].precision, Some(2));
        assert_eq!(schema.entries[4].kind, UserPropertyKind::Boolean);
    }

    #[test]
    fn merges_overrides_with_schema_defaults_as_renderer_value_object() {
        let schema = UserPropertySchema::from_project_json(
            r#"{"general":{"properties":{"enabled":{"type":"bool","value":true},"speed":{"type":"slider","value":1.0}}}}"#,
        )
        .expect("schema should parse");
        let values = schema.values_json(&BTreeMap::from([("speed".to_owned(), json!(2.0))]));
        assert_eq!(values, json!({"enabled":true,"speed":2.0}));
    }
    #[test]
    fn static_text_is_read_only_while_textinput_remains_editable() {
        let schema = UserPropertySchema::from_project_json(
            r#"{"general":{"properties":{
            "label":{"type":"text","text":"<b>Instructions</b>","value":"Read me"},
            "caption":{"type":"textinput","text":"Caption","value":"Hello"}
        }}}"#,
        )
        .unwrap();
        assert_eq!(
            schema.entries.iter().find(|p| p.key == "label").unwrap().kind,
            UserPropertyKind::Html
        );
        assert_eq!(
            schema.entries.iter().find(|p| p.key == "caption").unwrap().kind,
            UserPropertyKind::Text
        );
    }

    #[test]
    fn retains_display_conditions_and_distinguishes_static_labels_from_text_inputs() {
        let schema = UserPropertySchema::from_project_json(r#"{"general":{"properties":{
            "enabled":{"type":"bool","value":false},
            "label":{"type":"text","text":"<b>Instructions</b>","value":""},
            "caption":{"type":"textinput","text":"Caption","value":"Hello","condition":"enabled.value == true"}
        }}}"#).unwrap();
        let caption = schema.entries.iter().find(|p| p.key == "caption").unwrap();
        assert_eq!(caption.condition.as_deref(), Some("enabled.value == true"));
        assert_eq!(caption.kind, UserPropertyKind::Text);
        assert_eq!(
            schema.entries.iter().find(|p| p.key == "label").unwrap().kind,
            UserPropertyKind::Html
        );
    }
}
