//! Every dictionary shipped in `dictionaries/` loads without errors and passes
//! its own vectors.

use bite::vectors::run_vectors;
use bite::Dictionary;
use std::path::PathBuf;

fn shipped(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../dictionaries")
        .join(name)
}

fn assert_clean(name: &str, expected_vectors: usize) -> Dictionary {
    let dictionary = Dictionary::load(shipped(name)).expect("loads");
    let errors: Vec<String> = dictionary
        .issues()
        .iter()
        .filter(|issue| issue.severity == bite::Severity::Error)
        .map(ToString::to_string)
        .collect();
    assert!(errors.is_empty(), "{name}: {errors:#?}");
    let results = run_vectors(&dictionary).expect("vectors readable");
    assert_eq!(results.len(), expected_vectors, "{name}: vector count");
    for result in &results {
        assert!(
            result.passed(),
            "{name}/{}: {:#?}",
            result.name,
            result.failures
        );
    }
    dictionary
}

#[test]
fn weather_station() {
    let dictionary = assert_clean("weather-station", 5);
    assert!(dictionary.issues().is_empty(), "{:#?}", dictionary.issues());
    assert_eq!(dictionary.interface_for_port(40100).unwrap().name, "WX");
    assert_eq!(
        dictionary.interface_for_ports(50000, 40100).unwrap().name,
        "WX"
    );
    assert!(dictionary.interface_for_port(40101).is_none());
}

#[test]
fn vehicle_can() {
    let dictionary = assert_clean("vehicle-can", 2);
    assert!(dictionary.issues().is_empty(), "{:#?}", dictionary.issues());
    let interface = dictionary.interface("POWERTRAIN").unwrap();
    // Messages on this interface are identified by the frame, not the payload.
    assert!(interface.decode(&[0u8; 8]).is_err());
    let decoded = interface.decode_frame(&[0u8; 8], Some(0x7FF)).unwrap();
    assert_eq!(decoded.message_type(), "2047");
    assert!(decoded.fields.is_empty());
}

#[test]
fn toot() {
    assert_clean("toot", 1);
}

#[test]
fn typed_json_keeps_numbers_as_numbers() {
    let dictionary = Dictionary::load(shipped("weather-station")).unwrap();
    let interface = dictionary.interface("WX").unwrap();
    let payload = hex::decode("01000c4b58370000015180600000").unwrap();
    let decoded = interface.decode(&payload).unwrap();
    let json = serde_json::to_value(&decoded).unwrap();
    assert_eq!(json["uptime"], 86400);
    assert_eq!(json["sensor_fault"], true);
    assert_eq!(json["mode"], "Calibrating");
    assert_eq!(json["station_id"], "KX7");
}

#[test]
fn json_schema_describes_the_typed_form() {
    let dictionary = Dictionary::load(shipped("weather-station")).unwrap();
    let schemas = bite::jsonschema::dictionary_schemas(&dictionary);
    assert_eq!(schemas.len(), 3);
    let observation = &schemas["WX/Observation"];
    assert_eq!(observation["$id"], "weather-station/WX/Observation");
    let properties = &observation["properties"];
    // Scaled integer: a number, in degrees Celsius.
    assert_eq!(properties["temperature"]["anyOf"][0]["type"], "number");
    assert_eq!(properties["temperature"]["x-unit"], "degC");
    // 20-bit unsigned: bounded integer.
    assert_eq!(properties["pressure"]["anyOf"][0]["maximum"], 1_048_575);
    assert_eq!(
        properties["pressure"]["x-bits"],
        serde_json::json!([72, 92])
    );
    // Labelled field: the labels, or the raw integer for an unlabelled value.
    let mode = &schemas["WX/Heartbeat"]["properties"]["mode"]["anyOf"];
    assert_eq!(
        mode[0]["enum"],
        serde_json::json!(["Calibrating", "Idle", "Sampling"])
    );
    assert_eq!(mode[1]["type"], "integer");
}
