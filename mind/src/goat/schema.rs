//! The JSON Schema of the record published for each message of a dictionary.
//!
//! `bite::jsonschema` describes a decoded message with typed values. The
//! record carries the same fields rendered as text, inside the envelope of
//! `datamodels::packets::PacketInfoWithParsedMessage`; the schema here
//! describes that record, and keeps the typed form of each field under
//! `x-typed`.

use bite::dictionary::{Interface, Message};
use bite::Dictionary;
use flow::goat::{Declarations, MessageDeclaration};
use serde_json::{json, Map, Value};

/// The messages of `dictionary` and the schema of each one's record.
pub fn declarations(dictionary: Option<&Dictionary>) -> Declarations {
    let Some(dictionary) = dictionary else {
        return Declarations::default();
    };
    let mut messages = Vec::new();
    for interface in dictionary.interfaces() {
        for message in interface.messages() {
            messages.push(MessageDeclaration {
                interface: interface.name.clone(),
                message: message.name.clone(),
                schema: record_schema(dictionary, interface, message),
            });
        }
    }
    Declarations {
        dictionary: dictionary_label(dictionary),
        messages,
    }
}

/// `name version`, as far as the dictionary states them.
pub fn dictionary_label(dictionary: &Dictionary) -> Option<String> {
    let spec = &dictionary.spec;
    match (&spec.name, &spec.version) {
        (Some(name), Some(version)) => Some(format!("{name} {version}")),
        (Some(name), None) => Some(name.clone()),
        (None, Some(version)) => Some(format!("version {version}")),
        (None, None) => None,
    }
}

/// Schema of the record published for one message.
pub fn record_schema(dictionary: &Dictionary, interface: &Interface, message: &Message) -> Value {
    let typed = bite::jsonschema::message_schema(dictionary, interface, message);
    let mut fields = Map::new();
    if let Some(properties) = typed["properties"].as_object() {
        for (name, typed_field) in properties {
            let mut field = json!({"type": ["string", "null"]});
            for key in ["description", "x-unit", "x-bits"] {
                if let Some(value) = typed_field.get(key) {
                    field[key] = value.clone();
                }
            }
            // goat's data dictionary reads the unit from `unit`.
            if let Some(unit) = typed_field.get("x-unit") {
                field["unit"] = unit.clone();
            }
            if let Some(alternatives) = typed_field.get("anyOf") {
                field["x-typed"] = alternatives.clone();
            }
            fields.insert(name.clone(), field);
        }
    }
    let text = json!({"type": ["string", "null"]});
    let address = json!({
        "type": "array",
        "items": {"type": "integer", "minimum": 0, "maximum": 255},
        "description": "IPv4 (4 bytes) or IPv6 (16 bytes) address",
    });
    let port = json!({"type": "string", "pattern": "^[0-9]+$"});
    let encoded = |description: &str| json!({"type": "string", "contentEncoding": "base64", "description": description});
    let mut schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": format!("{} {} record", interface.name, message.name),
        "description": "One captured packet whose payload decoded as this message. \
            Every value of `message` is text: the dictionary's label where the raw \
            value has one, the value in decimal otherwise, null for a field beyond \
            the end of a short payload.",
        "type": "object",
        "required": ["timestamp", "subsystem", "message_type", "message", "data_encoded_inner"],
        "properties": {
            "timestamp": {
                "type": "object",
                "description": "capture time since 1970-01-01T00:00:00Z",
                "required": ["secs_since_epoch", "nanos_since_epoch"],
                "properties": {
                    "secs_since_epoch": {"type": "integer", "unit": "s"},
                    "nanos_since_epoch": {"type": "integer", "unit": "ns"},
                },
            },
            "src_ip": address,
            "dst_ip": address,
            "src_port": port,
            "dst_port": port,
            "subsystem": {"const": interface.name},
            "message_type": {"const": message.name},
            "message": {
                "type": "object",
                "properties": fields,
                "additionalProperties": false,
            },
            "data_encoded_inner": encoded("the transport payload"),
            "data_encoded_full": encoded("the whole captured frame"),
            "data_source_geo_location": text,
            "data_source_network_name": text,
            "data_source_system_type": text,
            "data_source_owner": text,
            "data_packet_classification": text,
            "data_packet_accm": text,
            "data_model_version": text,
            "application_version": text,
        },
    });
    if let Some(description) = &message.description {
        schema["x-message-description"] = json!(description);
    }
    if let Some(label) = dictionary_label(dictionary) {
        schema["x-dictionary"] = json!(label);
    }
    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use datamodels::packets::PacketInfoWithParsedMessage;
    use std::collections::BTreeSet;
    use std::path::Path;

    fn dictionary() -> Dictionary {
        Dictionary::load(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../dictionaries/weather-station"),
        )
        .unwrap()
    }

    #[test]
    fn every_message_of_the_dictionary_is_declared() {
        let dictionary = dictionary();
        let declared = declarations(Some(&dictionary));
        let expected: usize = dictionary
            .interfaces()
            .iter()
            .map(|interface| interface.messages().len())
            .sum();
        assert!(expected > 0);
        assert_eq!(declared.messages.len(), expected);
        assert!(declarations(None).messages.is_empty());
    }

    #[test]
    fn the_schema_names_every_field_of_the_published_record() {
        let dictionary = dictionary();
        let interface = &dictionary.interfaces()[0];
        let message = &interface.messages()[0];
        let schema = record_schema(&dictionary, interface, message);

        // The record type itself is the reference for the envelope.
        let record = PacketInfoWithParsedMessage {
            timestamp: std::time::UNIX_EPOCH,
            src_port: "1".into(),
            dst_port: "2".into(),
            src_ip: vec![192, 0, 2, 1],
            dst_ip: vec![192, 0, 2, 2],
            data_encoded_inner: String::new(),
            subsystem: None,
            message_type: None,
            message: None,
            data_encoded_full: String::new(),
            data_source_geo_location: None,
            data_source_network_name: None,
            data_source_system_type: None,
            data_source_owner: None,
            data_packet_classification: None,
            data_packet_accm: None,
            data_model_version: None,
            application_version: None,
        };
        let published = serde_json::to_value(&record).unwrap();
        let in_record: BTreeSet<&String> = published.as_object().unwrap().keys().collect();
        let in_schema: BTreeSet<&String> =
            schema["properties"].as_object().unwrap().keys().collect();
        assert_eq!(in_schema, in_record);
        let time: BTreeSet<&String> = published["timestamp"].as_object().unwrap().keys().collect();
        let time_schema: BTreeSet<&String> = schema["properties"]["timestamp"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(time_schema, time);

        assert_eq!(schema["properties"]["subsystem"]["const"], interface.name);
        assert_eq!(schema["properties"]["message_type"]["const"], message.name);
        let fields = schema["properties"]["message"]["properties"]
            .as_object()
            .unwrap();
        assert_eq!(fields.len(), message.fields.len());
        for field in &message.fields {
            let declared = &fields[&field.name];
            assert_eq!(
                declared["type"],
                json!(["string", "null"]),
                "{}",
                field.name
            );
            assert!(declared["x-typed"].is_array(), "{}", field.name);
            assert_eq!(
                declared.get("unit"),
                declared.get("x-unit"),
                "{}",
                field.name
            );
        }
    }
}
