//! The packs in `flow/xf` against the engine and against `bite`.
//!
//! * Every vector `tools/bite2pack` wrote passes on the engine: the tool's
//!   own codec and the engine agree on each of them.
//! * `bite` and the engine decode first-bite's fixtures (every dictionary
//!   vector, the records of the example capture, the packets of the NTP
//!   capture) to the same values, field by field. The fields that differ are
//!   counted by cause and compared with the numbers recorded here.
#![cfg(feature = "goat-transformers")]

use base64::Engine as _;
use bite::{Dictionary, FieldKind, Value as BiteValue};
use flow::xf::goat_transformers::{Engine, Pack, Value};
use flow::xf::{Xf, XfConfig};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn xf() -> Xf {
    let config: XfConfig = serde_json::from_str(r#"{"to": "cot-xml"}"#).unwrap();
    Xf::from_config(&config).unwrap()
}

/// `index.json`: one entry per generated pack.
fn index() -> Vec<serde_json::Value> {
    let text = std::fs::read_to_string(root().join("flow/xf/index.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn every_generated_vector_passes_on_the_engine() {
    let xf = xf();
    let engine = xf.engine();
    let mut run = 0;
    for entry in index() {
        let format = entry["format"].as_str().unwrap();
        let vectors = root().join("flow/xf/packs").join(format).join("vectors");
        let mut files: Vec<_> = std::fs::read_dir(&vectors)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        files.sort();
        assert!(!files.is_empty(), "{format}: no vectors");
        for file in files {
            let name = file.display();
            let vector: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            let input = vector["input"].as_str().unwrap();
            assert_eq!(vector["from"], format, "{name}");
            assert_eq!(vector["to"], format, "{name}");
            run += 1;
            if let Some(kind) = vector["expect_error"].as_str() {
                let error = engine.convert(format, format, input).unwrap_err();
                assert!(error.msg.starts_with(kind), "{name}: {error}");
                continue;
            }
            let entity = engine
                .decode(format, input)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let mut decoded: BTreeMap<String, String> = entity
                .fields
                .iter()
                .map(|(key, value)| (key.clone(), value.as_display()))
                .collect();
            let kind = entity.kind.as_ref().map(|(_, kind)| kind.clone());
            decoded.insert("@kind".to_string(), kind.unwrap_or_default());
            let expected: BTreeMap<String, String> = vector["model"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(key, value)| (key.clone(), value.as_str().unwrap().to_string()))
                .collect();
            assert_eq!(decoded, expected, "{name}");
            assert_eq!(
                engine.convert(format, format, input).unwrap(),
                vector["expect"].as_str().unwrap(),
                "{name}"
            );
        }
    }
    assert_eq!(run, 54, "vectors run");
}

/// The UDP payloads to or from `port` in a capture file: classic pcap,
/// Ethernet, IPv4.
fn udp_payloads(path: &Path, port: u16) -> Vec<Vec<u8>> {
    let file = std::fs::read(path).unwrap();
    assert_eq!(file[..4], [0xd4, 0xc3, 0xb2, 0xa1], "little-endian pcap");
    let mut payloads = Vec::new();
    let mut at = 24;
    while at + 16 <= file.len() {
        let length = u32::from_le_bytes(file[at + 8..at + 12].try_into().unwrap()) as usize;
        let frame = &file[at + 16..at + 16 + length];
        at += 16 + length;
        let ip = &frame[14..];
        let header = usize::from(ip[0] & 0x0f) * 4;
        let udp = &ip[header..];
        let ports = [
            u16::from_be_bytes([udp[0], udp[1]]),
            u16::from_be_bytes([udp[2], udp[3]]),
        ];
        if frame[12..14] == [0x08, 0x00] && ip[9] == 17 && ports.contains(&port) {
            payloads.push(udp[8..].to_vec());
        }
    }
    payloads
}

/// One fixture: an interface, a payload and, for a CAN frame, its identifier.
struct Fixture {
    interface: String,
    payload: Vec<u8>,
    frame_id: Option<u64>,
}

/// first-bite's own inputs for one dictionary.
fn fixtures(name: &str, dictionary: &Dictionary) -> Vec<Fixture> {
    let mut fixtures: Vec<Fixture> = bite::vectors::load_vectors(dictionary)
        .unwrap()
        .into_iter()
        .map(|(file, vector)| {
            let vector = vector.unwrap_or_else(|e| panic!("{name} vector {file}: {e}"));
            let compact: String = vector
                .payload_hex
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            Fixture {
                interface: vector.interface,
                payload: hex::decode(compact).unwrap(),
                frame_id: vector.frame_id,
            }
        })
        .collect();
    if name == "weather-station" {
        let records =
            std::fs::read_to_string(root().join("examples/weather-station.records.jsonl")).unwrap();
        for line in records.lines() {
            let record: serde_json::Value = serde_json::from_str(line).unwrap();
            let payload = base64::engine::general_purpose::STANDARD
                .decode(record["data_encoded_inner"].as_str().unwrap())
                .unwrap();
            fixtures.push(Fixture {
                interface: record["subsystem"].as_str().unwrap().to_string(),
                payload,
                frame_id: None,
            });
        }
    }
    if name == "ntp" {
        for payload in udp_payloads(&root().join("examples/ntp.pcap"), 123) {
            fixtures.push(Fixture {
                interface: "NTP".to_string(),
                payload,
                frame_id: None,
            });
        }
    }
    fixtures
}

/// How one field compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Outcome {
    /// The same value in the same form; a field beyond the payload is absent
    /// in both.
    Equal,
    /// A `bytes` field of two bytes or more: hexadecimal text in the record,
    /// the same bytes as an integer in the engine. The pack language has no
    /// hexadecimal transform; a one-byte field goes through a value table.
    BytesAsInteger,
    /// A scaled field: equal to nine significant digits, the rule of
    /// `bite check`, and not the same double.
    LastDigits,
    Different,
}

/// The model key the pack binds to the bits of a dictionary field.
fn key_of(pack: &Pack, kind: &str, bit: usize, width: usize, base: usize) -> Option<String> {
    let locator = format!("0:{}:{width}", base + bit);
    let kind = pack.kinds.iter().find(|candidate| candidate.name == kind)?;
    kind.fields
        .iter()
        .find(|field| field.locator == locator && field.model_key.contains('.'))
        .map(|field| field.model_key.clone())
}

fn compare(field: &bite::DecodedField<'_>, got: Option<&Value>) -> Outcome {
    let rendered = field.render();
    match (&field.value, got) {
        (None, None) => Outcome::Equal,
        (Some(BiteValue::Bytes(bytes)), Some(Value::I64(integer))) => {
            let spelled = bytes
                .iter()
                .fold(0u64, |acc, byte| (acc << 8) | u64::from(*byte));
            if spelled == *integer as u64 {
                Outcome::BytesAsInteger
            } else {
                Outcome::Different
            }
        }
        (Some(BiteValue::Float(value)), Some(Value::F64(float))) if field.label.is_none() => {
            let close = (value - float).abs() <= 1e-9 * value.abs().max(float.abs()).max(1e-300);
            match (value.to_bits() == float.to_bits(), close) {
                (true, _) => Outcome::Equal,
                (false, true) => Outcome::LastDigits,
                (false, false) => Outcome::Different,
            }
        }
        (Some(_), Some(got)) if rendered.as_deref() == Some(got.as_display().as_str()) => {
            Outcome::Equal
        }
        _ => Outcome::Different,
    }
}

/// Fields compared and how, for one dictionary's fixtures.
fn measure(name: &str, engine: &Engine) -> (usize, BTreeMap<Outcome, usize>) {
    let directory = root().join("dictionaries").join(name);
    let dictionary = Dictionary::load(&directory).unwrap();
    let index = index();
    let mut outcomes = BTreeMap::new();
    let fixtures = fixtures(name, &dictionary);
    for fixture in &fixtures {
        let entry = index
            .iter()
            .find(|entry| entry["dictionary"] == name && entry["interface"] == *fixture.interface)
            .unwrap();
        let format = entry["format"].as_str().unwrap();
        let prefix = entry["frame_id_bytes"].as_u64().unwrap() as usize;
        let pack_text = root().join("flow/xf/packs").join(format).join("pack.def");
        let pack = Pack::parse(&std::fs::read_to_string(pack_text).unwrap()).unwrap();
        let interface = dictionary.interface(&fixture.interface).unwrap();
        let decoded = interface
            .decode_frame(&fixture.payload, fixture.frame_id)
            .unwrap();
        // A frame identifier goes in front of the payload, in the byte order
        // of the interface's bit numbering.
        let mut message = match (prefix, fixture.frame_id) {
            (0, _) => Vec::new(),
            (_, Some(id)) if interface.bit_order == bite::BitOrder::Lsb0 => {
                (id as u32).to_le_bytes().to_vec()
            }
            (_, Some(id)) => (id as u32).to_be_bytes().to_vec(),
            (_, None) => panic!("{format}: a fixture without its frame identifier"),
        };
        message.extend_from_slice(&fixture.payload);
        let entity = engine
            .decode(format, &hex::encode(&message))
            .unwrap_or_else(|e| panic!("{format} {}: {e}", hex::encode(&message)));
        let kind = entity.kind.as_ref().map(|(_, kind)| kind.as_str()).unwrap();
        assert_eq!(
            decoded.message.is_some(),
            kind != "unidentified" && !decoded.fields.is_empty(),
            "{format}: bite says {}, the engine {kind}",
            decoded.message_type()
        );
        for field in &decoded.fields {
            let definition = field.field;
            let key = key_of(
                &pack,
                kind,
                definition.start,
                definition.width(),
                prefix * 8,
            )
            .unwrap_or_else(|| panic!("{format} {kind}: no line for {}", definition.name));
            let outcome = compare(field, entity.fields.get(&key));
            assert!(
                outcome != Outcome::BytesAsInteger || definition.kind == FieldKind::Bytes,
                "{format} {}",
                definition.name
            );
            *outcomes.entry(outcome).or_insert(0) += 1;
        }
    }
    (fixtures.len(), outcomes)
}

/// A dictionary, its fixtures, and how many of their fields compare each way.
type Recorded = (&'static str, usize, &'static [(Outcome, usize)]);

/// The recorded comparison. A change to a dictionary, to `tools/bite2pack`
/// or to the pinned engine that moves a number fails here; record the new
/// number with its cause, in this table and in `flow/README.md`.
#[test]
fn bite_and_the_engine_decode_the_fixtures_to_the_same_values() {
    use Outcome::{BytesAsInteger, Equal, LastDigits};
    let recorded: [Recorded; 4] = [
        ("glasshouse", 10, &[(Equal, 115), (LastDigits, 1)]),
        ("ntp", 20, &[(Equal, 256), (BytesAsInteger, 16)]),
        ("vehicle-can", 2, &[(Equal, 10)]),
        ("weather-station", 10, &[(Equal, 62), (BytesAsInteger, 2)]),
    ];
    let xf = xf();
    let (mut messages, mut fields, mut equal) = (0, 0, 0);
    for (name, fixtures, outcomes) in recorded {
        let (count, measured) = measure(name, xf.engine());
        let expected: BTreeMap<Outcome, usize> = outcomes.iter().copied().collect();
        assert_eq!((count, &measured), (fixtures, &expected), "{name}");
        messages += count;
        fields += measured.values().sum::<usize>();
        equal += measured.get(&Equal).copied().unwrap_or(0);
    }
    assert_eq!((messages, fields, equal), (42, 462, 443));
}
