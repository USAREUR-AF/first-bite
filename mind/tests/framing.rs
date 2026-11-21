//! A UDP packet holding several messages back to back, on an interface with
//! `framing: concatenated`, yields one record per message. Frames are built
//! by hand.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use bite::Dictionary;
use etherparse::PacketBuilder;
use mind::{Frame, LinkType, Pipeline};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

fn dictionary(framing: &str) -> Arc<Dictionary> {
    let metadata = format!(
        r#"{{"port": 47000,
            "message_type": {{"start": 0, "stop": 8, "values": {{"1": "Ping", "2": "Fix"}}}},
            "message_length": {{"start": 8, "stop": 16}}{framing}}}"#
    );
    Arc::new(
        Dictionary::from_files(vec![
            ("LINK/metadata.json".to_string(), metadata),
            (
                "LINK/messages/Ping.json".to_string(),
                r#"{"fields": {"label": {"start": 0, "stop": 8},
                    "length": {"start": 8, "stop": 16},
                    "sequence": {"start": 16, "stop": 32}}}"#
                    .to_string(),
            ),
            (
                "LINK/messages/Fix.json".to_string(),
                r#"{"fields": {"label": {"start": 0, "stop": 8},
                    "length": {"start": 8, "stop": 16},
                    "height": {"start": 16, "stop": 48, "type": "int"}}}"#
                    .to_string(),
            ),
        ])
        .unwrap(),
    )
}

fn udp_frame(payload: &[u8]) -> Vec<u8> {
    let builder = PacketBuilder::ethernet2([2, 0, 0, 0, 0, 1], [2, 0, 0, 0, 0, 2])
        .ipv4([192, 0, 2, 1], [192, 0, 2, 2], 64)
        .udp(47000, 47000);
    let mut frame = Vec::with_capacity(builder.size(payload.len()));
    builder.write(&mut frame, payload).unwrap();
    frame
}

const PING: [u8; 4] = [0x01, 0x04, 0x00, 0x2a];
const FIX: [u8; 6] = [0x02, 0x06, 0xff, 0xff, 0xff, 0x9c];

#[test]
fn one_record_per_concatenated_message_and_padding_is_dropped() {
    let pipeline = Pipeline::new(Some(dictionary(r#", "framing": "concatenated""#)));
    let payload = [&PING[..], &FIX[..], &[0, 0, 0, 0]].concat();
    let data = udp_frame(&payload);
    let frame = Frame {
        timestamp: UNIX_EPOCH,
        link: LinkType::Ethernet,
        data: &data,
        interface: None,
    };

    let records = pipeline.process_all(&frame).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].message_type.as_deref(), Some("Ping"));
    assert_eq!(records[1].message_type.as_deref(), Some("Fix"));
    assert_eq!(records[0].data_encoded_inner, STANDARD.encode(PING));
    assert_eq!(records[1].data_encoded_inner, STANDARD.encode(FIX));
    for record in &records {
        assert_eq!(record.data_encoded_full, STANDARD.encode(&data));
        assert_eq!(record.subsystem.as_deref(), Some("LINK"));
    }
    let fix = records[1].message.as_ref().unwrap();
    assert_eq!(fix["height"], -100);

    // process() keeps returning one record: the first message.
    let first = pipeline.process(&frame).unwrap();
    assert_eq!(first.message_type.as_deref(), Some("Ping"));
}

#[test]
fn single_framing_keeps_one_record_per_packet() {
    let pipeline = Pipeline::new(Some(dictionary("")));
    let payload = [&PING[..], &FIX[..]].concat();
    let data = udp_frame(&payload);
    let frame = Frame {
        timestamp: UNIX_EPOCH,
        link: LinkType::Ethernet,
        data: &data,
        interface: None,
    };
    let records = pipeline.process_all(&frame).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].data_encoded_inner, STANDARD.encode(&payload));
    assert_eq!(records[0].message_type.as_deref(), Some("Ping"));
}
