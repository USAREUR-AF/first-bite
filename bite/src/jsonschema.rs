//! JSON Schema (draft 2020-12) for the typed JSON form of a decoded message.
//!
//! The schema describes what [`crate::Decoded`] serialises to: one property
//! per field, holding the dictionary's label where the raw value has one and
//! the typed value otherwise. Consumers of decoded records can validate
//! against it, and catalogues that index data by schema can ingest it.

use crate::dictionary::{Dictionary, Field, Interface, Message};
use crate::schema::FieldKind;
use serde_json::{json, Map, Value};

/// Schema for one message of one interface.
pub fn message_schema(dictionary: &Dictionary, interface: &Interface, message: &Message) -> Value {
    let mut properties = Map::new();
    for field in &message.fields {
        properties.insert(field.name.clone(), field_schema(field));
    }
    let prefix = dictionary
        .spec
        .name
        .as_deref()
        .map(|name| format!("{name}/"))
        .unwrap_or_default();
    let mut schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": format!("{prefix}{}/{}", interface.name, message.name),
        "title": message.name,
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
    });
    if let Some(description) = &message.description {
        schema["description"] = json!(description);
    }
    if let Some(version) = &dictionary.spec.version {
        schema["x-dictionary-version"] = json!(version);
    }
    schema
}

/// Schemas for every message in the dictionary, keyed `interface/message`.
pub fn dictionary_schemas(dictionary: &Dictionary) -> Map<String, Value> {
    let mut schemas = Map::new();
    for interface in dictionary.interfaces() {
        for message in interface.messages() {
            schemas.insert(
                format!("{}/{}", interface.name, message.name),
                message_schema(dictionary, interface, message),
            );
        }
    }
    schemas
}

fn field_schema(field: &Field) -> Value {
    let width = field.width();
    let scaled = field.scale.is_some() || field.offset.is_some();
    let mut value = match field.kind {
        FieldKind::Float => json!({"type": "number"}),
        FieldKind::Bool => json!({"type": "boolean"}),
        FieldKind::Bytes => json!({
            "type": "string",
            "pattern": "^[0-9a-f]*$",
            "maxLength": width / 4,
        }),
        FieldKind::Ascii => json!({"type": "string", "maxLength": width / 8}),
        FieldKind::Uint | FieldKind::Int if scaled => json!({"type": "number"}),
        FieldKind::Uint => {
            let mut schema = json!({"type": "integer", "minimum": 0});
            // Larger bounds are not exactly representable as JSON numbers.
            if width <= 53 {
                schema["maximum"] = json!((1u64 << width) - 1);
            }
            schema
        }
        FieldKind::Int => {
            let mut schema = json!({"type": "integer"});
            if width <= 53 {
                schema["minimum"] = json!(-(1i64 << (width - 1)));
                schema["maximum"] = json!((1i64 << (width - 1)) - 1);
            }
            schema
        }
    };

    let mut labels: Vec<&str> = field.labels().collect();
    labels.sort_unstable();
    labels.dedup();
    let mut alternatives = Vec::new();
    if !labels.is_empty() {
        alternatives.push(json!({"enum": labels}));
    }
    alternatives.push(value.take());
    // A field beyond the end of a short payload is null.
    alternatives.push(json!({"type": "null"}));

    let mut schema = json!({"anyOf": alternatives});
    if let Some(description) = &field.description {
        schema["description"] = json!(description);
    }
    if let Some(unit) = &field.unit {
        schema["x-unit"] = json!(unit);
    }
    schema["x-bits"] = json!([field.start, field.stop]);
    schema
}
