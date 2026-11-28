//! The `xf` transform against the `cot` transform: the same records through
//! `flow/config/subpubs.example.yml` (`subtype: "cot"`) and
//! `flow/config/subpubs.xf.example.yml` (`subtype: "xf"`), compared attribute
//! by attribute.
//!
//! The records are the Position records of the example capture
//! (`examples/weather-station.records.jsonl`), the dictionary's Position
//! vector, and Position payloads generated here. What differs is recorded in
//! [`KNOWN`]; anything else that differs fails the test.
#![cfg(feature = "goat-transformers")]

use base64::Engine as _;
use bite::Dictionary;
use chrono::{DateTime, TimeZone, Utc};
use flow::config::{parse, SubPubConfigs};
use flow::xf::Xf;
use flow::Transform;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// What the two transforms write differently, and on how many of the 502
/// records.
const KNOWN: [(&str, usize, &str); 6] = [
    (
        "event/@time",
        502,
        "the same instant: `cot` writes +00:00, `xf` milliseconds and Z",
    ),
    ("event/@start", 502, "as event/@time"),
    ("event/@stale", 502, "as event/@time"),
    // `cot` reads the number from the record's JSON, and serde_json reads a
    // 17-digit decimal to within one unit in the last place; `xf` reads the
    // payload. Where the two differ they are neighbouring doubles.
    ("point/@lat", 29, "neighbouring doubles"),
    ("point/@lon", 53, "neighbouring doubles"),
    ("point/@hae", 2, "neighbouring doubles"),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The transforms of the two example files.
fn transforms() -> (cotify::Mapping, Xf) {
    let subscriber = |name: &str| {
        let text = std::fs::read_to_string(root().join("flow/config").join(name)).unwrap();
        let mut subpubs = parse::<SubPubConfigs>(&text, name).unwrap().subpubs;
        assert_eq!(subpubs.len(), 1, "{name}");
        assert_eq!(subpubs[0].subscriber.topics, ["WX/Position"], "{name}");
        subpubs.remove(0).subscriber
    };
    let cot = match Transform::from_config(&subscriber("subpubs.example.yml")).unwrap() {
        Transform::Cot(mapping) => mapping,
        other => panic!("subpubs.example.yml: {other:?}"),
    };
    let xf = match Transform::from_config(&subscriber("subpubs.xf.example.yml")).unwrap() {
        Transform::Xf(xf) => xf,
        other => panic!("subpubs.xf.example.yml: {other:?}"),
    };
    (cot, xf)
}

/// Every attribute of an event as `element/@name`, and each child element of
/// the event by its name.
fn parts(xml: &str) -> BTreeMap<String, String> {
    let mut reader = Reader::from_str(xml);
    let mut parts = BTreeMap::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(element) | Event::Empty(element) => {
                let name = element.name().as_ref().to_string();
                if name != "event" {
                    parts.insert(name.clone(), String::new());
                }
                for attribute in element.attributes() {
                    let attribute = attribute.unwrap();
                    parts.insert(
                        format!("{name}/@{}", attribute.key.as_ref()),
                        attribute
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .unwrap()
                            .into_owned(),
                    );
                }
            }
            Event::Eof => return parts,
            _ => {}
        }
    }
}

fn instant(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

/// A Position payload of the weather-station dictionary.
fn position(station: &[u8; 4], latitude: f64, longitude: f64, altitude: f32) -> Vec<u8> {
    let mut payload = vec![3, 0, 27];
    payload.extend_from_slice(station);
    payload.extend_from_slice(&latitude.to_be_bytes());
    payload.extend_from_slice(&longitude.to_be_bytes());
    payload.extend_from_slice(&altitude.to_be_bytes());
    payload
}

/// The record `mind` publishes for a payload, cut to the keys the two
/// transforms read.
fn record(dictionary: &Dictionary, payload: &[u8]) -> String {
    let decoded = dictionary.interface("WX").unwrap().decode(payload).unwrap();
    serde_json::json!({
        "data_encoded_inner": base64::engine::general_purpose::STANDARD.encode(payload),
        "subsystem": "WX",
        "message_type": decoded.message_type(),
        "message": decoded,
        "dictionary": "weather-station",
    })
    .to_string()
}

/// The Position records to compare: the capture's, the vector's, and 500
/// generated ones.
fn records() -> Vec<String> {
    let dictionary = Dictionary::load(root().join("dictionaries/weather-station")).unwrap();
    let sample =
        std::fs::read_to_string(root().join("examples/weather-station.records.jsonl")).unwrap();
    let mut records: Vec<String> = sample
        .lines()
        .filter(|line| line.contains(r#""message_type":"Position""#))
        .map(str::to_string)
        .collect();
    assert_eq!(records.len(), 1, "Position records in the example capture");
    let vector: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("dictionaries/weather-station/vectors/position.json"))
            .unwrap(),
    )
    .unwrap();
    let payload = hex::decode(vector["payload_hex"].as_str().unwrap().replace(' ', "")).unwrap();
    records.push(record(&dictionary, &payload));
    // A fixed sequence, so every run compares the same payloads.
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    for number in 0..500 {
        let station = match number % 4 {
            0 => *b"KX7\0",
            1 => *b"AB  ",
            2 => *b"Z9zz",
            _ => [b'A' + (number % 26) as u8, b'-', 0, 0],
        };
        let latitude = next() * 180.0 - 90.0;
        let longitude = next() * 360.0 - 180.0;
        let altitude = (next() * 9000.0 - 400.0) as f32;
        records.push(record(
            &dictionary,
            &position(&station, latitude, longitude, altitude),
        ));
    }
    records
}

#[test]
fn xf_writes_the_event_cot_writes_but_for_the_recorded_differences() {
    let (cot, xf) = transforms();
    assert_eq!(xf.target(), flow::xf::COT_XML);
    let now = Utc.with_ymd_and_hms(2026, 3, 4, 12, 0, 0).unwrap();
    let mut differing: BTreeMap<String, usize> = BTreeMap::new();
    let records = records();
    for record in &records {
        let envelope = |json: String| -> serde_json::Value { serde_json::from_str(&json).unwrap() };
        let by_cot = envelope(cotify::record_to_cot_at(&cot, record, now).unwrap());
        let by_xf = envelope(xf.apply_at(record, now).unwrap());
        // The same envelope, with the creation time in each transform's form.
        let keys = |value: &serde_json::Value| -> Vec<String> {
            value.as_object().unwrap().keys().cloned().collect()
        };
        assert_eq!(keys(&by_cot), keys(&by_xf));
        assert_eq!(
            instant(by_cot["cot_datetime"].as_str().unwrap()),
            instant(by_xf["cot_datetime"].as_str().unwrap())
        );
        let cot_parts = parts(by_cot["cot_xml_string"].as_str().unwrap());
        let xf_parts = parts(by_xf["cot_xml_string"].as_str().unwrap());
        let names: BTreeSet<&String> = cot_parts.keys().chain(xf_parts.keys()).collect();
        for name in names {
            let (left, right) = (cot_parts.get(name), xf_parts.get(name));
            if left == right {
                continue;
            }
            assert!(
                KNOWN.iter().any(|(known, _, _)| known == name),
                "{name}: cot {left:?}, xf {right:?}\n{record}"
            );
            match (name.starts_with("point/@"), left, right) {
                (true, Some(left), Some(right)) => {
                    let (left, right): (f64, f64) = (left.parse().unwrap(), right.parse().unwrap());
                    let step = (left.to_bits() as i64 - right.to_bits() as i64).abs();
                    assert_eq!(step, 1, "{name}: cot {left}, xf {right}");
                }
                // A time that differs in text is the same instant.
                (false, Some(left), Some(right)) => {
                    assert_eq!(instant(left), instant(right), "{name}")
                }
                _ => panic!("{name}: written by one transform only"),
            }
            *differing.entry(name.clone()).or_insert(0) += 1;
        }
        // 12 attributes, <point> and <detail> in both: the cot-xml pack writes an
        // empty <detail/> as `cot` does (goat-transformers spec 0.18, `element detail`).
        assert_eq!((cot_parts.len(), xf_parts.len()), (14, 14), "{record}");
    }
    let recorded: BTreeMap<String, usize> = KNOWN
        .iter()
        .map(|(name, count, _)| (name.to_string(), *count))
        .collect();
    assert_eq!(differing, recorded);
    assert_eq!(records.len(), 502);
}

/// A message with no position: both transforms refuse the record.
#[test]
fn a_message_with_no_position_is_refused_by_both() {
    let (cot, xf) = transforms();
    let sample =
        std::fs::read_to_string(root().join("examples/weather-station.records.jsonl")).unwrap();
    let heartbeat = sample.lines().next().unwrap();
    assert!(cotify::record_to_cot(&cot, heartbeat).is_err());
    assert_eq!(
        xf.apply_at(heartbeat, Utc::now()).unwrap_err().to_string(),
        "the payload gives no `uid`, `lat`, `lon`, which `xf.require` names"
    );
}
