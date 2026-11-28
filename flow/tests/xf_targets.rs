//! Every dictionary of this repository to every target of the `xf`
//! transform: Cursor-on-Target (`cot-xml`), GeoJSON (`geojson`), the shared
//! model as a track (`canonical-track-v2`), ASTERIX category 062
//! (`asterix-cat062`) and SAPIENT (`sapient`).
//!
//! * Each vector `tools/bite2pack` wrote for a dictionary's pack is
//!   converted to each target with the context a subscriber gives for that
//!   target ([`context`]). A vector converts, or it is refused and the
//!   refusal names the keys the target needs that neither the message nor the
//!   context gives. The counts are recorded here and in `flow/README.md`.
//!   Nothing is filled in to make a conversion pass.
//! * A Position converts to every target and decodes from it to the same
//!   position.
//! * A binary target's output is the bytes its hexadecimal spells.
#![cfg(feature = "goat-transformers")]

use base64::Engine as _;
use chrono::{DateTime, TimeZone, Utc};
use flow::xf::{Carriage, Xf, XfConfig};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

const TARGETS: [&str; 5] = [
    "cot-xml",
    "geojson",
    "canonical-track-v2",
    "asterix-cat062",
    "sapient",
];

/// What a subscriber configures for a target, beyond `to`: the values no
/// dictionary message carries. A track of the shared model names its source
/// and generation; a CAT062 record its SAC and SIC and whether the track is
/// confirmed; SAPIENT is asked for a detection report, CoT for a track's
/// type and how it was made. `require` makes a message without an
/// identifier and a position a refusal, for every target.
fn context(to: &str) -> serde_json::Value {
    let (kind, context) = match to {
        "cot-xml" => (None, serde_json::json!({"type": "a-f-G", "how": "m-g"})),
        "canonical-track-v2" => (
            Some("Track"),
            serde_json::json!({"track.source_id": "first-bite", "track.generation": 1}),
        ),
        "asterix-cat062" => (
            Some("track"),
            serde_json::json!({"cat062.sac": 25, "cat062.sic": 4, "track.confirmed": 1}),
        ),
        "sapient" => (Some("detection_report"), serde_json::json!({})),
        _ => (None, serde_json::json!({})),
    };
    serde_json::json!({
        "to": to,
        "kind": kind,
        "context": context,
        "require": ["uid", "lat", "lon"],
    })
}

fn xf(mut config: serde_json::Value, from: &str) -> Xf {
    config["from"] = from.into();
    let config: XfConfig = serde_json::from_value(config).unwrap();
    Xf::from_config(&config).unwrap()
}

fn noon() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 4, 12, 0, 0).unwrap()
}

/// A published record whose payload is `bytes`.
fn record(bytes: &[u8]) -> String {
    let payload = base64::engine::general_purpose::STANDARD.encode(bytes);
    serde_json::json!({ "data_encoded_inner": payload }).to_string()
}

/// (dictionary, format) of each generated pack.
fn generated() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(root().join("flow/xf/index.json")).unwrap();
    let index: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap();
    index
        .iter()
        .map(|entry| {
            let text = |key: &str| entry[key].as_str().unwrap().to_string();
            (text("dictionary"), text("format"))
        })
        .collect()
}

/// The inputs of the vectors of a pack that decode: every vector but those
/// of a refused message.
fn inputs(format: &str) -> Vec<Vec<u8>> {
    let dir = root().join("flow/xf/packs").join(format).join("vectors");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.sort();
    files
        .iter()
        .map(|file| {
            let text = std::fs::read_to_string(file).unwrap();
            serde_json::from_str::<serde_json::Value>(&text).unwrap()
        })
        .filter(|vector| vector.get("expect_error").is_none())
        .map(|vector| hex::decode(vector["input"].as_str().unwrap()).unwrap())
        .collect()
}

/// For one target and one generated pack: how many vectors converted, and each
/// refusal with how many vectors gave it.
type Recorded = (
    &'static str,
    &'static str,
    usize,
    &'static [(&'static str, usize)],
);

const NONE: &str = "the payload gives no `uid`, `lat`, `lon`, which `xf.require` names";
const NO_TRACK_NUMBER: &str =
    "format asterix-cat062 needs cat062.track_number for its kind track, \
                               which neither the payload nor `xf.context` gives";

/// The recorded outcome. Only a weather-station Position carries an
/// identifier and a position (`flow/xf/bindings.json`); every other message
/// of these dictionaries is refused for every target. CAT062 numbers a track
/// from its identifier, and the station identifier is text. A change to a
/// dictionary, to `tools/bite2pack`, to [`context`] or to the pinned engine
/// that moves a number fails here; record the new number and its cause here
/// and in `flow/README.md`.
const RECORDED: &[Recorded] = &[
    ("cot-xml", "bite-glasshouse-supv", 0, &[(NONE, 9)]),
    ("cot-xml", "bite-glasshouse-zone", 0, &[(NONE, 14)]),
    ("cot-xml", "bite-ntp-ntp", 0, &[(NONE, 17)]),
    ("cot-xml", "bite-vehicle-can-powertrain", 0, &[(NONE, 5)]),
    ("cot-xml", "bite-weather-station-wx", 2, &[(NONE, 7)]),
    ("geojson", "bite-glasshouse-supv", 0, &[(NONE, 9)]),
    ("geojson", "bite-glasshouse-zone", 0, &[(NONE, 14)]),
    ("geojson", "bite-ntp-ntp", 0, &[(NONE, 17)]),
    ("geojson", "bite-vehicle-can-powertrain", 0, &[(NONE, 5)]),
    ("geojson", "bite-weather-station-wx", 2, &[(NONE, 7)]),
    (
        "canonical-track-v2",
        "bite-glasshouse-supv",
        0,
        &[(NONE, 9)],
    ),
    (
        "canonical-track-v2",
        "bite-glasshouse-zone",
        0,
        &[(NONE, 14)],
    ),
    ("canonical-track-v2", "bite-ntp-ntp", 0, &[(NONE, 17)]),
    (
        "canonical-track-v2",
        "bite-vehicle-can-powertrain",
        0,
        &[(NONE, 5)],
    ),
    (
        "canonical-track-v2",
        "bite-weather-station-wx",
        2,
        &[(NONE, 7)],
    ),
    ("asterix-cat062", "bite-glasshouse-supv", 0, &[(NONE, 9)]),
    ("asterix-cat062", "bite-glasshouse-zone", 0, &[(NONE, 14)]),
    ("asterix-cat062", "bite-ntp-ntp", 0, &[(NONE, 17)]),
    (
        "asterix-cat062",
        "bite-vehicle-can-powertrain",
        0,
        &[(NONE, 5)],
    ),
    (
        "asterix-cat062",
        "bite-weather-station-wx",
        0,
        &[(NO_TRACK_NUMBER, 2), (NONE, 7)],
    ),
    ("sapient", "bite-glasshouse-supv", 0, &[(NONE, 9)]),
    ("sapient", "bite-glasshouse-zone", 0, &[(NONE, 14)]),
    ("sapient", "bite-ntp-ntp", 0, &[(NONE, 17)]),
    ("sapient", "bite-vehicle-can-powertrain", 0, &[(NONE, 5)]),
    ("sapient", "bite-weather-station-wx", 2, &[(NONE, 7)]),
];

#[test]
fn every_dictionary_vector_converts_to_every_target_or_names_what_it_lacks() {
    let mut measured: Vec<(String, String, usize, BTreeMap<String, usize>)> = Vec::new();
    for to in TARGETS {
        for (_, format) in generated() {
            let xf = xf(context(to), &format);
            let (mut converted, mut refused) = (0, BTreeMap::new());
            for input in inputs(&format) {
                match xf.convert_at(&record(&input), noon()) {
                    Ok(output) => {
                        assert!(!output.bytes.is_empty(), "{to} {format}");
                        converted += 1;
                    }
                    Err(e) => *refused.entry(e.to_string()).or_insert(0) += 1,
                }
            }
            measured.push((to.to_string(), format, converted, refused));
        }
    }
    let recorded: Vec<(String, String, usize, BTreeMap<String, usize>)> = RECORDED
        .iter()
        .map(|(to, format, converted, refused)| {
            let refused = refused.iter().map(|(e, n)| (e.to_string(), *n)).collect();
            (to.to_string(), format.to_string(), *converted, refused)
        })
        .collect();
    assert_eq!(measured, recorded);
}

/// The Position record of `examples/weather-station.records.jsonl`: station
/// KX7 at 64.8378, -147.7164, 136.5 m.
const POSITION: &str = r#"{"data_encoded_inner":"AwAbS1g3AEBQNZ6D5CWvwGJ27L+xW1dDCIAA","subsystem":"WX","message_type":"Position","dictionary":"weather-station"}"#;

/// A Position converts to each target; the target's own pack decodes the
/// output to the station's position, within the target's resolution. The
/// station identifier `KX7` is no number, so a CAT062 track number is given
/// in the context.
#[test]
fn a_position_converts_to_every_target_and_decodes_to_the_same_position() {
    for to in TARGETS {
        let mut config = context(to);
        config.as_object_mut().unwrap().remove("require");
        if to == "asterix-cat062" {
            config["context"]["cat062.track_number"] = 7.into();
        }
        let config: XfConfig = serde_json::from_value(config).unwrap();
        let xf = Xf::from_config(&config).unwrap();
        let output = xf.convert_at(POSITION, noon()).unwrap();
        let entity = xf.engine().decode(to, &output.text).unwrap();
        let number = |key: &str| entity.fields[key].as_display().parse::<f64>().unwrap();
        // CAT062 states a WGS-84 position in units of 180/2^25 degrees.
        let within = if to == "asterix-cat062" { 6e-6 } else { 1e-9 };
        assert!((number("lat") - 64.8378).abs() < within, "{to} {entity:?}");
        assert!((number("lon") + 147.7164).abs() < within, "{to} {entity:?}");
        let binary = xf.carriage().is_binary();
        assert_eq!(binary, matches!(to, "asterix-cat062" | "sapient"), "{to}");
        if binary {
            assert_eq!(output.bytes, hex::decode(&output.text).unwrap(), "{to}");
        } else {
            assert_eq!(output.bytes, output.text.as_bytes(), "{to}");
        }
        match to {
            "asterix-cat062" => {
                assert_eq!(xf.carriage(), Carriage::Binary);
                assert_eq!(entity.fields["cat062.track_number"].as_display(), "7");
                // An ASTERIX data block: category 62, then its length.
                assert_eq!(output.bytes[0], 62);
                let length = u16::from_be_bytes([output.bytes[1], output.bytes[2]]);
                assert_eq!(usize::from(length), output.bytes.len());
            }
            "sapient" => {
                assert_eq!(xf.carriage(), Carriage::Protobuf);
                assert_eq!(entity.kind.as_ref().unwrap().1, "detection_report");
            }
            _ => assert_eq!(entity.fields["uid"].as_display(), "KX7", "{to}"),
        }
    }
}

/// A kind the target does not have stops startup; a record that lacks a key
/// of the configured kind is refused with the key named.
#[test]
fn the_configured_kind_is_checked() {
    let config: XfConfig =
        serde_json::from_value(serde_json::json!({"to": "sapient", "kind": "sighting"})).unwrap();
    let refusal = Xf::from_config(&config).unwrap_err().to_string();
    assert!(
        refusal.contains("`kind`: format sapient has no kind \"sighting\"; its kinds are"),
        "{refusal}"
    );
    let heartbeat = r#"{"data_encoded_inner":"AQAMS1g3AAABUYBg","subsystem":"WX","dictionary":"weather-station"}"#;
    let config: XfConfig =
        serde_json::from_value(serde_json::json!({"to": "sapient", "kind": "detection_report"}))
            .unwrap();
    let xf = Xf::from_config(&config).unwrap();
    assert_eq!(
        xf.convert_at(heartbeat, noon()).unwrap_err().to_string(),
        "format sapient needs lat, lon for its kind detection_report, which neither the payload \
         nor `xf.context` gives"
    );
}
