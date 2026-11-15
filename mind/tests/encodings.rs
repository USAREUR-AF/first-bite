//! Every vector of every shipped dictionary, as a version 2 record: the typed
//! values agree with what the vector expects, the record validates against
//! the schema `bite schema` prints, and the CBOR form reads back as the JSON
//! record.

use bite::vectors::load_vectors;
use bite::{Decoded, Dictionary};
use datamodels::packets::{PacketInfoWithParsedMessage, Port, RecordVersion};
use flow::encoding::{cbor_to_json, json_to_cbor};
use serde_json::Value;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

const DICTIONARIES: [&str; 4] = ["weather-station", "ntp", "vehicle-can", "glasshouse"];

fn dictionary(name: &str) -> Dictionary {
    Dictionary::load(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../dictionaries")
            .join(name),
    )
    .unwrap()
}

/// The record `mind` publishes for a decoded payload, with a fixed envelope.
fn record(dictionary: &Dictionary, decoded: &Decoded<'_>, payload: &[u8]) -> String {
    let version = RecordVersion::Typed;
    let record = PacketInfoWithParsedMessage {
        timestamp: UNIX_EPOCH + Duration::new(1_700_000_000, 250_000_000),
        src_port: Port::new(50000, version),
        dst_port: Port::new(40100, version),
        src_ip: vec![192, 0, 2, 1],
        dst_ip: vec![192, 0, 2, 2],
        data_encoded_inner: hex::encode(payload),
        subsystem: Some(decoded.interface.name.clone()),
        message_type: Some(decoded.message_type()),
        message: Some(serde_json::to_value(decoded).unwrap()),
        data_encoded_full: hex::encode(payload),
        data_source_geo_location: Some("hilltop".into()),
        data_source_network_name: None,
        data_source_system_type: None,
        data_source_owner: None,
        data_packet_classification: None,
        data_packet_accm: None,
        data_model_version: Some(datamodels::TYPED_RECORD_VERSION.into()),
        application_version: Some("0.0.0".into()),
        capture_interface: Some("eth0".into()),
        tunnel: None,
        dictionary: dictionary.spec.name.clone(),
        dictionary_version: dictionary.spec.version.clone(),
    };
    serde_json::to_string(&record).unwrap()
}

/// Call `check` with every vector of every shipped dictionary, decoded.
fn for_every_vector(
    mut check: impl FnMut(&str, &Dictionary, &Decoded<'_>, &[u8], &bite::schema::VectorSpec),
) {
    let mut vectors = 0;
    for name in DICTIONARIES {
        let dictionary = dictionary(name);
        for (vector_name, vector) in load_vectors(&dictionary).unwrap() {
            let vector = vector.unwrap();
            let interface = dictionary.interface(&vector.interface).unwrap();
            let compact: String = vector
                .payload_hex
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            let payload = hex::decode(compact).unwrap();
            let decoded = interface.decode_frame(&payload, vector.frame_id).unwrap();
            check(
                &format!("{name}/{vector_name}"),
                &dictionary,
                &decoded,
                &payload,
                &vector,
            );
            vectors += 1;
        }
    }
    assert!(vectors >= 27, "only {vectors} vectors were found");
}

#[test]
fn the_cbor_record_reads_back_as_the_json_record() {
    for_every_vector(|name, dictionary, decoded, payload, _| {
        let json = record(dictionary, decoded, payload);
        let cbor = json_to_cbor(&json).unwrap();
        let back: Value = serde_json::from_str(&cbor_to_json(&cbor).unwrap()).unwrap();
        let sent: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sent, "{name}");
        // The same record always gives the same bytes.
        assert_eq!(json_to_cbor(&back.to_string()).unwrap(), cbor, "{name}");
        assert!(cbor.len() < json.len(), "{name}");
    });
}

/// A typed value against the text a vector expects for it.
fn agrees(typed: &Value, expected: Option<&str>) -> bool {
    match (typed, expected) {
        (Value::Null, None) => true,
        (Value::String(text), Some(expected)) => text == expected,
        (Value::Bool(value), Some(expected)) => value.to_string() == expected,
        (Value::Number(number), Some(expected)) => {
            let Ok(expected) = expected.trim().parse::<f64>() else {
                return false;
            };
            if number.is_f64() {
                let actual = number.as_f64().unwrap_or(f64::NAN);
                (actual - expected).abs() <= 1e-9 * actual.abs().max(expected.abs()).max(1e-300)
            } else {
                // An integer is compared as written, so one too large for a
                // double is still compared exactly.
                false
            }
        }
        _ => false,
    }
}

#[test]
fn typed_values_are_the_values_the_vectors_expect() {
    let mut compared = 0;
    for_every_vector(|name, _, decoded, _, vector| {
        let message = serde_json::to_value(decoded).unwrap();
        for (field, expected) in &vector.expect.fields {
            let typed = &message[field];
            let exact =
                typed.is_number() && Some(typed.to_string().as_str()) == expected.as_deref();
            assert!(
                exact || agrees(typed, expected.as_deref()),
                "{name}: {field} is {typed}, the vector expects {expected:?}"
            );
            compared += 1;
        }
    });
    assert!(compared > 150, "only {compared} values were compared");
}

/// The part of JSON Schema that `bite schema` uses.
fn validates(schema: &Value, value: &Value) -> bool {
    if let Some(alternatives) = schema.get("anyOf").and_then(Value::as_array) {
        return alternatives.iter().any(|schema| validates(schema, value));
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        return allowed.contains(value);
    }
    let ok_type = match schema.get("type").and_then(Value::as_str) {
        Some("null") => value.is_null(),
        Some("boolean") => value.is_boolean(),
        Some("string") => value.is_string(),
        Some("integer") => value.is_u64() || value.is_i64(),
        Some("number") => value.is_number(),
        Some("object") => value.is_object(),
        Some(other) => panic!("the test does not know type {other:?}"),
        None => true,
    };
    if !ok_type {
        return false;
    }
    if let (Some(minimum), Some(number)) = (schema.get("minimum"), value.as_f64()) {
        if number < minimum.as_f64().unwrap() {
            return false;
        }
    }
    if let (Some(maximum), Some(number)) = (schema.get("maximum"), value.as_f64()) {
        if number > maximum.as_f64().unwrap() {
            return false;
        }
    }
    if let Some(text) = value.as_str() {
        if let Some(longest) = schema.get("maxLength").and_then(Value::as_u64) {
            if text.chars().count() as u64 > longest {
                return false;
            }
        }
        match schema.get("pattern").and_then(Value::as_str) {
            Some("^[0-9a-f]*$") => {
                if !text
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return false;
                }
            }
            Some(other) => panic!("the test does not know pattern {other:?}"),
            None => {}
        }
    }
    if let Some(object) = value.as_object() {
        let properties = schema.get("properties").and_then(Value::as_object);
        for (name, value) in object {
            match properties.and_then(|properties| properties.get(name)) {
                Some(schema) => {
                    if !validates(schema, value) {
                        return false;
                    }
                }
                None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => {
                    return false
                }
                None => {}
            }
        }
    }
    true
}

#[test]
fn the_message_of_a_record_validates_against_bite_schema() {
    let mut validated = 0;
    for_every_vector(|name, dictionary, decoded, payload, _| {
        let Some(message) = decoded.message else {
            return;
        };
        let schema = bite::jsonschema::message_schema(dictionary, decoded.interface, message);
        let record: Value = serde_json::from_str(&record(dictionary, decoded, payload)).unwrap();
        assert!(
            validates(&schema, &record["message"]),
            "{name}: {} does not validate",
            record["message"]
        );
        validated += 1;
    });
    assert!(validated >= 20, "only {validated} messages were validated");

    // The validator itself refuses what the schema does not allow.
    let dictionary = dictionary("weather-station");
    let interface = dictionary.interface("WX").unwrap();
    let message = interface.message("Heartbeat").unwrap();
    let schema = bite::jsonschema::message_schema(&dictionary, interface, message);
    let text_values = serde_json::json!({"uptime": "86400"});
    assert!(!validates(&schema, &text_values));
    assert!(!validates(
        &schema,
        &serde_json::json!({"no_such_field": 1})
    ));
    assert!(validates(
        &schema,
        &serde_json::json!({"uptime": 86400, "mode": "Idle"})
    ));
}
