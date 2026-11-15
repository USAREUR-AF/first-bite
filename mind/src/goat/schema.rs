//! The JSON Schema of the record published for each message of a dictionary.
//!
//! `bite::jsonschema` describes a decoded message with typed values. A
//! version 2 record carries exactly that as its `message`, inside the envelope
//! of `datamodels::packets::PacketInfoWithParsedMessage`. A version 1 record
//! carries the same fields rendered as text; its schema says so and keeps the
//! typed form of each field under `x-typed`.

use bite::dictionary::{Interface, Message};
use bite::Dictionary;
use datamodels::packets::{RecordVersion, TYPED_RECORD_VERSION};
use flow::goat::{Declarations, MessageDeclaration};
use serde_json::{json, Map, Value};

/// The messages of `dictionary` and the schema of each one's record.
pub fn declarations(dictionary: Option<&Dictionary>, version: RecordVersion) -> Declarations {
    // What `mind --version` prints.
    let application_version = Some(env!("CARGO_PKG_VERSION").to_string());
    let Some(dictionary) = dictionary else {
        return Declarations {
            application_version,
            ..Declarations::default()
        };
    };
    let mut messages = Vec::new();
    for interface in dictionary.interfaces() {
        for message in interface.messages() {
            messages.push(MessageDeclaration {
                interface: interface.name.clone(),
                message: message.name.clone(),
                schema: record_schema(dictionary, interface, message, version),
            });
        }
    }
    Declarations {
        application_version,
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

/// The schema of one field of `message`, from its typed schema.
fn field_schema(typed_field: &Value, version: RecordVersion) -> Value {
    let mut field = match version {
        RecordVersion::Typed => typed_field.clone(),
        RecordVersion::Text => {
            let mut field = json!({"type": ["string", "null"]});
            for key in ["description", "x-unit", "x-bits"] {
                if let Some(value) = typed_field.get(key) {
                    field[key] = value.clone();
                }
            }
            if let Some(alternatives) = typed_field.get("anyOf") {
                field["x-typed"] = alternatives.clone();
            }
            field
        }
    };
    // goat's data dictionary reads the unit from `unit`.
    if let Some(unit) = typed_field.get("x-unit") {
        field["unit"] = unit.clone();
    }
    field
}

/// Schema of the record published for one message.
pub fn record_schema(
    dictionary: &Dictionary,
    interface: &Interface,
    message: &Message,
    version: RecordVersion,
) -> Value {
    let typed = bite::jsonschema::message_schema(dictionary, interface, message);
    let mut fields = Map::new();
    if let Some(properties) = typed["properties"].as_object() {
        for (name, typed_field) in properties {
            fields.insert(name.clone(), field_schema(typed_field, version));
        }
    }
    let text = json!({"type": ["string", "null"]});
    let address = json!({
        "type": "array",
        "items": {"type": "integer", "minimum": 0, "maximum": 255},
        "description": "IPv4 (4 bytes) or IPv6 (16 bytes) address",
    });
    let (port, values, model) = match version {
        RecordVersion::Typed => (
            json!({"type": "integer", "minimum": 0, "maximum": 65535}),
            "Each value of `message` has the type its field declares: a number, a boolean, \
             text, bytes as hexadecimal text, or the dictionary's label where the raw value \
             has one; null for a field beyond the end of a short payload.",
            json!({"const": TYPED_RECORD_VERSION}),
        ),
        RecordVersion::Text => (
            json!({"type": "string", "pattern": "^[0-9]+$"}),
            "Every value of `message` is text: the dictionary's label where the raw \
             value has one, the value in decimal otherwise, null for a field beyond \
             the end of a short payload.",
            text.clone(),
        ),
    };
    let encoded = |description: &str| json!({"type": "string", "contentEncoding": "base64", "description": description});
    let mut schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": format!("{} {} record", interface.name, message.name),
        "description": format!("One captured packet whose payload decoded as this message. {values}"),
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
            "data_model_version": model,
            "application_version": text,
        },
    });
    if version == RecordVersion::Typed {
        let context = [
            (
                "capture_interface",
                json!({"type": "string", "description": "network interface the frame was captured on; absent for a frame read from a file"}),
            ),
            (
                "tunnel",
                json!({"enum": ["gre", "erspan", "vxlan"], "description": "tunnel the packet was unwrapped from; absent when there was none"}),
            ),
            (
                "dictionary",
                json!({"type": "string", "description": "name of the dictionary that decoded the message"}),
            ),
            ("dictionary_version", json!({"type": "string"})),
        ];
        for (name, property) in context {
            schema["properties"][name] = property;
        }
    }
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
    use datamodels::packets::{PacketInfoWithParsedMessage, Port};
    use std::collections::BTreeSet;
    use std::path::Path;

    fn dictionary() -> Dictionary {
        Dictionary::load(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../dictionaries/weather-station"),
        )
        .unwrap()
    }

    /// A record with every field present; the record type itself is the
    /// reference for the envelope.
    fn record(version: RecordVersion) -> Value {
        let typed = version == RecordVersion::Typed;
        let context = |value: &str| typed.then(|| value.to_string());
        let record = PacketInfoWithParsedMessage {
            timestamp: std::time::UNIX_EPOCH,
            src_port: Port::new(1, version),
            dst_port: Port::new(2, version),
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
            capture_interface: context("eth0"),
            tunnel: context("gre"),
            dictionary: context("weather-station"),
            dictionary_version: context("1.0.0"),
        };
        serde_json::to_value(&record).unwrap()
    }

    #[test]
    fn every_message_of_the_dictionary_is_declared() {
        let dictionary = dictionary();
        let declared = declarations(Some(&dictionary), RecordVersion::Typed);
        let expected: usize = dictionary
            .interfaces()
            .iter()
            .map(|interface| interface.messages().len())
            .sum();
        assert!(expected > 0);
        assert_eq!(declared.messages.len(), expected);
        let none = declarations(None, RecordVersion::Typed);
        assert!(none.messages.is_empty());
        // The goat sink reports the version `mind --version` prints.
        for declared in [&declared, &none] {
            assert_eq!(
                declared.application_version.as_deref(),
                Some(env!("CARGO_PKG_VERSION"))
            );
        }
    }

    #[test]
    fn the_schema_names_every_field_of_the_published_record() {
        let dictionary = dictionary();
        let interface = &dictionary.interfaces()[0];
        let message = &interface.messages()[0];
        for version in [RecordVersion::Typed, RecordVersion::Text] {
            let schema = record_schema(&dictionary, interface, message, version);
            let published = record(version);
            let in_record: BTreeSet<&String> = published.as_object().unwrap().keys().collect();
            let in_schema: BTreeSet<&String> =
                schema["properties"].as_object().unwrap().keys().collect();
            assert_eq!(in_schema, in_record, "{version:?}");
            let time: BTreeSet<&String> =
                published["timestamp"].as_object().unwrap().keys().collect();
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
        }
    }

    #[test]
    fn a_version_two_schema_declares_typed_values_and_numeric_ports() {
        let dictionary = dictionary();
        let interface = &dictionary.interfaces()[0];
        let message = interface.message("Observation").unwrap();
        let schema = record_schema(&dictionary, interface, message, RecordVersion::Typed);
        let typed = bite::jsonschema::message_schema(&dictionary, interface, message);
        assert_eq!(schema["properties"]["src_port"]["type"], "integer");
        assert_eq!(schema["properties"]["src_port"]["maximum"], 65535);
        assert_eq!(schema["properties"]["data_model_version"]["const"], "2.0.0");
        let fields = &schema["properties"]["message"]["properties"];
        for field in &message.fields {
            let declared = &fields[&field.name];
            // The field is declared as `bite schema` declares it, plus `unit`.
            assert_eq!(
                declared["anyOf"], typed["properties"][&field.name]["anyOf"],
                "{}",
                field.name
            );
            assert_eq!(
                declared.get("unit"),
                declared.get("x-unit"),
                "{}",
                field.name
            );
        }
        assert_eq!(fields["temperature"]["anyOf"][0]["type"], "number");
        assert_eq!(fields["temperature"]["unit"], "degC");
    }

    #[test]
    fn a_version_one_schema_declares_text_and_keeps_the_typed_form_beside_it() {
        let dictionary = dictionary();
        let interface = &dictionary.interfaces()[0];
        let message = &interface.messages()[0];
        let schema = record_schema(&dictionary, interface, message, RecordVersion::Text);
        assert_eq!(schema["properties"]["src_port"]["type"], "string");
        let fields = schema["properties"]["message"]["properties"]
            .as_object()
            .unwrap();
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
