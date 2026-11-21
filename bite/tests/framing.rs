//! `framing: concatenated`: one payload holding several messages back to back,
//! each stating its own length. Every payload here is built by hand.

use bite::authoring::canon::canonical;
use bite::authoring::table::{export, import, ImportOptions};
use bite::authoring::{FileSet, Tree};
use bite::{Dictionary, Framing, Severity};

const PING: &str = r#"{"fields": {
    "label": {"start": 0, "stop": 8},
    "length": {"start": 8, "stop": 16},
    "sequence": {"start": 16, "stop": 32}}}"#;
const FIX: &str = r#"{"fields": {
    "label": {"start": 0, "stop": 8},
    "length": {"start": 8, "stop": 16},
    "height": {"start": 16, "stop": 48, "type": "int"},
    "checksum": {"start": 48, "stop": 56}}}"#;

fn metadata(framing: &str) -> String {
    format!(
        r#"{{"port": 47000,
            "message_type": {{"start": 0, "stop": 8, "values": {{"1": "Ping", "2": "Fix"}}}},
            "message_length": {{"start": 8, "stop": 16}}{framing}}}"#
    )
}

fn files(framing: &str) -> Vec<(String, String)> {
    vec![
        ("LINK/metadata.json".to_string(), metadata(framing)),
        ("LINK/messages/Ping.json".to_string(), PING.to_string()),
        ("LINK/messages/Fix.json".to_string(), FIX.to_string()),
    ]
}

fn concatenated() -> Dictionary {
    Dictionary::from_files(files(r#", "framing": "concatenated""#)).unwrap()
}

/// Ping, sequence 42: 4 bytes.
const PING_BYTES: [u8; 4] = [0x01, 0x04, 0x00, 0x2a];
/// Fix, height -100, checksum 0x5a: 7 bytes.
const FIX_BYTES: [u8; 7] = [0x02, 0x07, 0xff, 0xff, 0xff, 0x9c, 0x5a];

fn payload(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

#[test]
fn a_payload_of_two_messages_and_padding_splits_into_the_two_messages() {
    let dictionary = concatenated();
    let interface = dictionary.interface("LINK").unwrap();
    assert_eq!(interface.framing, Framing::Concatenated);
    let bytes = payload(&[&PING_BYTES, &FIX_BYTES, &[0, 0, 0, 0]]);

    let messages = interface.split(&bytes);
    assert_eq!(messages, vec![&PING_BYTES[..], &FIX_BYTES[..]]);

    let decoded: Vec<_> = interface
        .decode_all(&bytes)
        .into_iter()
        .map(Result::unwrap)
        .collect();
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[0].message_type(), "Ping");
    assert_eq!(decoded[0].rendered()["sequence"].as_deref(), Some("42"));
    assert_eq!(decoded[1].message_type(), "Fix");
    assert_eq!(decoded[1].rendered()["height"].as_deref(), Some("-100"));
    assert_eq!(decoded[1].rendered()["checksum"].as_deref(), Some("90"));
}

#[test]
fn messages_of_one_type_repeat_and_no_padding_is_needed() {
    let dictionary = concatenated();
    let interface = dictionary.interface("LINK").unwrap();
    let bytes = payload(&[&FIX_BYTES, &PING_BYTES, &PING_BYTES]);
    assert_eq!(
        interface.split(&bytes),
        vec![&FIX_BYTES[..], &PING_BYTES[..], &PING_BYTES[..]]
    );
}

#[test]
fn single_framing_is_the_default_and_reads_the_payload_as_one_message() {
    let dictionary = Dictionary::from_files(files("")).unwrap();
    let interface = dictionary.interface("LINK").unwrap();
    assert_eq!(interface.framing, Framing::Single);
    let bytes = payload(&[&PING_BYTES, &FIX_BYTES]);
    assert_eq!(interface.split(&bytes), vec![&bytes[..]]);
    assert_eq!(interface.decode_all(&bytes).len(), 1);

    let explicit = Dictionary::from_files(files(r#", "framing": "single""#)).unwrap();
    assert_eq!(explicit.interface("LINK").unwrap().framing, Framing::Single);
}

#[test]
fn a_length_that_cannot_be_followed_leaves_the_rest_as_one_message() {
    let dictionary = concatenated();
    let interface = dictionary.interface("LINK").unwrap();

    // A second message stating length 0.
    let zero = payload(&[&PING_BYTES, &[0x02, 0x00, 0x11, 0x22]]);
    assert_eq!(interface.split(&zero), vec![&zero[..4], &zero[4..]]);

    // A second message stating more bytes than are left.
    let long = payload(&[&PING_BYTES, &[0x02, 0x40, 0x11]]);
    assert_eq!(interface.split(&long), vec![&long[..4], &long[4..]]);

    // A length shorter than the two header bytes.
    let short = payload(&[&[0x01, 0x01, 0x05], &PING_BYTES]);
    assert_eq!(interface.split(&short), vec![&short[..]]);

    // One byte left after a message: too short for a header, not all zero.
    let stub = payload(&[&PING_BYTES, &[0x07]]);
    assert_eq!(interface.split(&stub), vec![&stub[..4], &stub[4..]]);
}

#[test]
fn empty_and_all_zero_payloads_are_one_message() {
    let dictionary = concatenated();
    let interface = dictionary.interface("LINK").unwrap();
    let empty: &[u8] = &[];
    assert_eq!(interface.split(empty), vec![empty]);
    assert!(interface.decode_all(&[])[0].is_err());
    let zeros = [0u8; 16];
    assert_eq!(interface.split(&zeros), vec![&zeros[..]]);
}

#[test]
fn a_hostile_payload_of_minimal_messages_splits_without_trouble() {
    let dictionary = concatenated();
    let interface = dictionary.interface("LINK").unwrap();
    let bytes: Vec<u8> = std::iter::repeat_n([0x09u8, 0x02], 32_768)
        .flatten()
        .collect();
    let messages = interface.split(&bytes);
    assert_eq!(messages.len(), 32_768);
    assert!(messages.iter().all(|message| message == &[0x09, 0x02]));
}

#[test]
fn concatenated_framing_without_message_length_is_an_error_and_falls_back() {
    let text = r#"{"port": 47000, "framing": "concatenated",
        "message_type": {"start": 0, "stop": 8, "values": {"1": "Ping"}}}"#;
    let dictionary = Dictionary::from_files(vec![
        ("LINK/metadata.json", text.to_string()),
        ("LINK/messages/Ping.json", PING.to_string()),
    ])
    .unwrap();
    let interface = dictionary.interface("LINK").unwrap();
    assert_eq!(interface.framing, Framing::Single);
    assert!(dictionary.issues().iter().any(|issue| {
        issue.severity == Severity::Error
            && issue.location == "LINK"
            && issue
                .message
                .contains("framing concatenated needs message_length")
    }));
}

#[test]
fn framing_survives_fmt_and_the_tables() {
    let set: FileSet = files(r#", "framing": "concatenated""#)
        .into_iter()
        .collect();
    let tree = Tree::parse(&set).unwrap();
    let formatted = canonical(&tree).unwrap().to_files();
    let metadata = &formatted["LINK/metadata.json"];
    assert!(
        metadata.contains(r#""framing": "concatenated""#),
        "{metadata}"
    );
    // message_length comes before framing in the canonical key order.
    assert!(metadata.find("message_length") < metadata.find("framing"));

    let tables = export(&Tree::parse(&formatted).unwrap()).unwrap();
    assert!(tables
        .interfaces
        .lines()
        .next()
        .unwrap()
        .contains("framing"));
    let interfaces = import(&tables, ImportOptions::default()).unwrap();
    let again = canonical(&Tree {
        dictionary: None,
        interfaces,
        vectors: Vec::new(),
    })
    .unwrap()
    .to_files();
    assert_eq!(again["LINK/metadata.json"], formatted["LINK/metadata.json"]);
}
