//! The JSON schema for `boxset.toml`, built from the field table.
//!
//! Editors that understand the `#:schema` directive read this for completion
//! and validation. Even Better TOML is one, as is anything built on taplo.

use serde_json::{Map, Value, json};

use crate::fields::{Field, QUALITY_TIERS, Shape, TARGET_FIELDS, TOP_LEVEL_FIELDS};

/// Pinned per release, so a config keeps validating against the schema of the
/// boxset that wrote it.
pub const SCHEMA_URL_BASE: &str = "https://raw.githubusercontent.com/edgfoo/boxset";

/// If the version looks like a prerelease, just point to whatever schema's on the
/// main branch.
pub fn schema_url(version: &str) -> String {
    let git_ref = match version.contains('-') {
        true => "main".to_string(),
        false => format!("v{version}"),
    };
    format!("{SCHEMA_URL_BASE}/{git_ref}/schema.json")
}

pub fn generate() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://github.com/edgfoo/boxset/schema.json",
        "title": "boxset.toml",
        "description": "Configuration for boxset, which prepares video for the web.",
        "type": "object",
        "properties": root_properties(),
        "additionalProperties": false,
    })
}

fn root_properties() -> Value {
    let mut props = Map::new();
    for field in TOP_LEVEL_FIELDS {
        props.insert(field.toml_key.to_string(), field_schema(field));
    }

    props.insert(
        "defaults".to_string(),
        with_description(
            target_object(),
            "Target settings shared by every target. A target's own value always wins.",
        ),
    );
    props.insert(
        "target".to_string(),
        json!({
            "type": "array",
            "description": "One entry per output, not per video. Several targets may share a source.",
            "items": target_object(),
        }),
    );

    Value::Object(props)
}

fn target_object() -> Value {
    let mut props = Map::new();
    for field in TARGET_FIELDS {
        props.insert(field.toml_key.to_string(), field_schema(field));
    }
    json!({
        "type": "object",
        "properties": Value::Object(props),
        "additionalProperties": false,
    })
}

fn field_schema(field: &Field) -> Value {
    with_description(
        schema_for_shape(field.toml_key, &field.shape),
        field.schema_doc,
    )
}

fn with_description(mut schema: Value, doc: &str) -> Value {
    if let Some(object) = schema.as_object_mut() {
        object.insert("description".to_string(), json!(doc));
    }
    schema
}

fn schema_for_shape(key: &str, shape: &Shape) -> Value {
    match *shape {
        Shape::Path | Shape::Text => json!({ "type": "string", "minLength": 1 }),
        Shape::Choice(options) => json!({ "type": "string", "enum": options }),
        Shape::ChoiceList(options) => json!({
            "type": "array",
            "items": { "type": "string", "enum": options },
            "minItems": 1,
            "uniqueItems": true,
        }),
        Shape::Integer { min, max } => json!({
            "type": "integer",
            "minimum": min,
            "maximum": max,
        }),
        Shape::IntegerList { min, max } => json!({
            "type": "array",
            "items": { "type": "integer", "minimum": min, "maximum": max },
            "minItems": 1,
            "uniqueItems": true,
        }),
        Shape::Boolean => json!({ "type": "boolean" }),
        Shape::Pattern { regex, hint } => json!({
            "type": "string",
            "pattern": regex,
            "$comment": hint,
        }),
        Shape::NumberOrRatio => json!({
            "anyOf": [
                { "type": "number", "exclusiveMinimum": 0 },
                { "type": "string", "pattern": r"^\d+\s*/\s*\d+$" },
            ],
        }),
        Shape::Table(fields) | Shape::CodecTable(fields) => table(key, fields),
        // `false` switches the feature off. `true` parses today but means
        // nothing, so the schema admits only the boolean that does something.
        Shape::Toggle(fields) => json!({
            "anyOf": [
                { "const": false, "description": "Switch this off." },
                table(key, fields),
            ],
        }),
        Shape::TextList => json!({
            "type": "array",
            "items": { "type": "string" },
        }),
    }
}

fn table(key: &str, fields: &'static [Field]) -> Value {
    let mut props = Map::new();
    for field in fields {
        props.insert(field.toml_key.to_string(), field_schema(field));
    }
    let object = json!({
        "type": "object",
        "properties": Value::Object(props),
        "additionalProperties": false,
    });

    match key {
        // `crop` also accepts a bare `"16:9"`, which is how it is usually
        // written.
        "crop" => json!({
            "anyOf": [
                { "type": "string", "pattern": r"^\d+:\d+$" },
                object,
            ],
        }),
        // A bare tier sets video and audio both.
        "quality" => json!({
            "anyOf": [
                { "type": "string", "enum": QUALITY_TIERS },
                object,
            ],
        }),
        _ => object,
    }
}

pub fn to_json_text() -> String {
    let mut text = serde_json::to_string_pretty(&generate()).expect("schema serialises");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_properties() -> Value {
        generate()["properties"]["target"]["items"]["properties"].clone()
    }

    #[test]
    fn the_committed_schema_matches_the_table() {
        let committed = include_str!("../../schema.json");
        assert_eq!(
            committed,
            to_json_text(),
            "schema.json is stale; regenerate with `cargo run -- --print-schema > schema.json`"
        );
    }

    #[test]
    fn only_a_release_pins_its_own_tag() {
        assert!(schema_url("0.4.0").ends_with("/v0.4.0/schema.json"));
        assert!(schema_url("0.4.0-rc.1").ends_with("/main/schema.json"));
    }

    #[test]
    fn a_target_rejects_unknown_keys() {
        assert_eq!(
            generate()["properties"]["target"]["items"]["additionalProperties"],
            json!(false)
        );
    }

    #[test]
    fn a_toggle_admits_false_but_not_true() {
        let audio = target_properties()["audio"].clone();
        let options = audio["anyOf"].as_array().expect("anyOf").clone();
        assert!(options.iter().any(|o| o["const"] == json!(false)));
        assert!(!options.iter().any(|o| o["const"] == json!(true)));
    }

    #[test]
    fn quality_accepts_a_bare_tier_and_a_table() {
        let quality = target_properties()["quality"].clone();
        let options = quality["anyOf"].as_array().expect("anyOf").clone();
        assert!(options.iter().any(|o| o["type"] == json!("string")));
        assert!(options.iter().any(|o| o["type"] == json!("object")));
    }

    #[test]
    fn crop_accepts_a_bare_ratio_and_a_table() {
        let crop = target_properties()["crop"].clone();
        let options = crop["anyOf"].as_array().expect("anyOf").clone();
        assert!(options.iter().any(|o| o["type"] == json!("string")));
        assert!(options.iter().any(|o| o["type"] == json!("object")));
    }
}
