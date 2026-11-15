//! End to end without a network: write a capture file, replay it through the
//! file source and the pipeline, and check the records.

use bite::Dictionary;
use datamodels::packets::{PacketInfoWithParsedMessage, Port, RecordVersion};
use etherparse::PacketBuilder;
use mind::{FileCapture, Pipeline, Stats};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

const HEARTBEAT: &str = "01000c4b58370000015180600000";

fn dictionary() -> Arc<Dictionary> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dictionaries/weather-station");
    Arc::new(Dictionary::load(path).unwrap())
}

fn udp_frame(source_port: u16, destination_port: u16, payload: &[u8]) -> Vec<u8> {
    let builder = PacketBuilder::ethernet2([2, 0, 0, 0, 0, 1], [2, 0, 0, 0, 0, 2])
        .ipv4([192, 0, 2, 1], [192, 0, 2, 2], 64)
        .udp(source_port, destination_port);
    let mut frame = Vec::with_capacity(builder.size(payload.len()));
    builder.write(&mut frame, payload).unwrap();
    frame
}

fn tcp_frame(source_port: u16, destination_port: u16, payload: &[u8]) -> Vec<u8> {
    let builder = PacketBuilder::ethernet2([2, 0, 0, 0, 0, 1], [2, 0, 0, 0, 0, 2])
        .ipv6(
            [0x20, 1, 0xd, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
            [0xff; 16],
            64,
        )
        .tcp(source_port, destination_port, 1, 4096);
    let mut frame = Vec::with_capacity(builder.size(payload.len()));
    builder.write(&mut frame, payload).unwrap();
    frame
}

fn vlan_udp_frame(source_port: u16, destination_port: u16, payload: &[u8]) -> Vec<u8> {
    let builder = PacketBuilder::ethernet2([2, 0, 0, 0, 0, 1], [2, 0, 0, 0, 0, 2])
        .single_vlan(etherparse::VlanId::try_new(100).unwrap())
        .ipv4([192, 0, 2, 1], [192, 0, 2, 2], 64)
        .udp(source_port, destination_port);
    let mut frame = Vec::with_capacity(builder.size(payload.len()));
    builder.write(&mut frame, payload).unwrap();
    frame
}

/// Wrap `inner` (an Ethernet frame) in an outer Ethernet/IPv4 packet with the
/// given IP protocol number and the given bytes between the IP header and it.
fn tunnelled(protocol: u8, tunnel_header: &[u8], inner: &[u8]) -> Vec<u8> {
    let mut payload = tunnel_header.to_vec();
    payload.extend_from_slice(inner);
    let builder = PacketBuilder::ethernet2([2, 0, 0, 0, 0, 9], [2, 0, 0, 0, 0, 8]).ipv4(
        [198, 51, 100, 1],
        [198, 51, 100, 2],
        64,
    );
    let mut frame = Vec::with_capacity(builder.size(payload.len()));
    builder
        .write(&mut frame, etherparse::IpNumber(protocol), &payload)
        .unwrap();
    frame
}

fn vxlan_frame(inner: &[u8]) -> Vec<u8> {
    let mut payload = vec![0x08, 0, 0, 0, 0, 0, 0x2A, 0];
    payload.extend_from_slice(inner);
    udp_frame(49152, 4789, &payload)
}

fn arp_frame() -> Vec<u8> {
    let mut frame = vec![0xff; 6];
    frame.extend_from_slice(&[2, 0, 0, 0, 0, 1]);
    frame.extend_from_slice(&[0x08, 0x06]);
    frame.extend_from_slice(&[0u8; 28]);
    frame
}

struct CaptureFile(PathBuf);

impl CaptureFile {
    fn write(name: &str, frames: &[Vec<u8>]) -> Self {
        let path =
            std::env::temp_dir().join(format!("mind-test-{}-{name}.pcap", std::process::id()));
        let capture = pcap::Capture::dead(pcap::Linktype::ETHERNET).unwrap();
        let mut savefile = capture.savefile(&path).unwrap();
        for (index, frame) in frames.iter().enumerate() {
            let header = pcap::PacketHeader {
                ts: libc::timeval {
                    tv_sec: 1_700_000_000 + index as libc::time_t,
                    tv_usec: 250_000,
                },
                caplen: frame.len() as u32,
                len: frame.len() as u32,
            };
            savefile.write(&pcap::Packet::new(&header, frame));
        }
        savefile.flush().unwrap();
        CaptureFile(path)
    }
}

impl Drop for CaptureFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn replay(file: &CaptureFile, filter: Option<&str>) -> (Stats, Vec<PacketInfoWithParsedMessage>) {
    let mut source = FileCapture::open(&file.0, filter).unwrap();
    let pipeline = Pipeline::new(Some(dictionary()));
    let mut records = Vec::new();
    let mut sink = |_: &PacketInfoWithParsedMessage, json: &str| {
        records.push(serde_json::from_str(json).unwrap());
    };
    let stats = mind::run(
        &mut source,
        &pipeline,
        &mut sink,
        None,
        None,
        &AtomicBool::new(false),
    )
    .unwrap();
    (stats, records)
}

#[test]
fn replays_a_capture_file() {
    let heartbeat = hex::decode(HEARTBEAT).unwrap();
    let file = CaptureFile::write(
        "mixed",
        &[
            udp_frame(40100, 50000, &heartbeat),
            udp_frame(50001, 50002, b"not in the dictionary"),
            arp_frame(),
            tcp_frame(50003, 40100, &heartbeat),
            vec![0x45, 0x00],
        ],
    );
    let (stats, records) = replay(&file, None);
    assert_eq!(
        stats,
        Stats {
            frames: 5,
            records: 3,
            decoded: 2,
            skipped: 1,
            malformed: 1,
        }
    );

    let first = &records[0];
    assert_eq!(first.subsystem.as_deref(), Some("WX"));
    assert_eq!(first.message_type.as_deref(), Some("Heartbeat"));
    assert_eq!(first.src_port, Port::Number(40100));
    assert_eq!(first.dst_port, Port::Number(50000));
    assert_eq!(first.src_ip, [192, 0, 2, 1]);
    let message = first.message.as_ref().unwrap();
    assert_eq!(message["station_id"], "KX7");
    assert_eq!(message["uptime"], 86400);
    assert_eq!(message["battery_low"], false);
    assert_eq!(message["mode"], "Calibrating");
    // Version 2 names the dictionary on the records it decoded, and nothing
    // was unwrapped or captured live.
    assert_eq!(first.data_model_version.as_deref(), Some("2.0.0"));
    assert_eq!(
        first.application_version.as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(first.dictionary.as_deref(), Some("weather-station"));
    assert!(first.dictionary_version.is_some());
    assert_eq!(first.tunnel, None);
    assert_eq!(first.capture_interface, None);
    assert_eq!(
        first.timestamp,
        UNIX_EPOCH + Duration::new(1_700_000_000, 250_000_000)
    );

    // A port no interface claims still yields a record, undecoded.
    let second = &records[1];
    assert_eq!(second.subsystem, None);
    assert_eq!(second.message_type, None);
    assert!(second.message.is_none());
    assert_eq!(second.dictionary, None);

    // TCP over IPv6, matched on the destination port.
    let third = &records[2];
    assert_eq!(third.subsystem.as_deref(), Some("WX"));
    assert_eq!(third.src_ip.len(), 16);
    assert_eq!(third.message.as_ref().unwrap()["sensor_fault"], true);
}

/// Version 1 is the record as it was before values were typed: text values,
/// text ports, and none of the capture context.
#[test]
fn version_one_records_carry_every_value_as_text() {
    let heartbeat = hex::decode(HEARTBEAT).unwrap();
    let file = CaptureFile::write("v1", &[udp_frame(40100, 50000, &heartbeat)]);
    let mut source = FileCapture::open(&file.0, None).unwrap();
    let pipeline = Pipeline::new(Some(dictionary())).with_record_version(RecordVersion::Text);
    let mut published = Vec::new();
    let mut sink = |_: &PacketInfoWithParsedMessage, json: &str| published.push(json.to_string());
    mind::run(
        &mut source,
        &pipeline,
        &mut sink,
        None,
        None,
        &AtomicBool::new(false),
    )
    .unwrap();
    let record: serde_json::Value = serde_json::from_str(&published[0]).unwrap();
    assert_eq!(record["src_port"], "40100");
    assert_eq!(record["dst_port"], "50000");
    assert_eq!(record["message"]["uptime"], "86400");
    assert_eq!(record["message"]["battery_low"], "false");
    assert_eq!(record["message"]["mode"], "Calibrating");
    let keys: Vec<&str> = record
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "application_version",
            "data_encoded_full",
            "data_encoded_inner",
            "data_model_version",
            "data_packet_accm",
            "data_packet_classification",
            "data_source_geo_location",
            "data_source_network_name",
            "data_source_owner",
            "data_source_system_type",
            "dst_ip",
            "dst_port",
            "message",
            "message_type",
            "src_ip",
            "src_port",
            "subsystem",
            "timestamp",
        ]
    );
}

#[test]
fn capture_filter_limits_what_is_read() {
    let heartbeat = hex::decode(HEARTBEAT).unwrap();
    let file = CaptureFile::write(
        "filtered",
        &[
            udp_frame(40100, 50000, &heartbeat),
            udp_frame(50001, 50002, b"other traffic"),
        ],
    );
    let (stats, records) = replay(&file, Some("udp port 40100"));
    assert_eq!(stats.frames, 1);
    assert_eq!(records.len(), 1);
}

#[test]
fn without_a_dictionary_packets_are_published_undecoded() {
    let heartbeat = hex::decode(HEARTBEAT).unwrap();
    let file = CaptureFile::write("undecoded", &[udp_frame(40100, 50000, &heartbeat)]);
    let mut source = FileCapture::open(&file.0, None).unwrap();
    let pipeline = Pipeline::new(None);
    let mut count = 0;
    let mut sink = |record: &PacketInfoWithParsedMessage, _: &str| {
        assert!(record.subsystem.is_none());
        assert!(!record.data_encoded_inner.is_empty());
        count += 1;
    };
    mind::run(
        &mut source,
        &pipeline,
        &mut sink,
        None,
        None,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn vlan_tagged_frames_are_decoded() {
    let heartbeat = hex::decode(HEARTBEAT).unwrap();
    let file = CaptureFile::write("vlan", &[vlan_udp_frame(40100, 50000, &heartbeat)]);
    let (stats, records) = replay(&file, None);
    assert_eq!(stats.decoded, 1);
    assert_eq!(records[0].message_type.as_deref(), Some("Heartbeat"));
    assert_eq!(records[0].src_port, Port::Number(40100));
}

#[test]
fn mirrored_traffic_in_gre_and_erspan_is_unwrapped() {
    let heartbeat = hex::decode(HEARTBEAT).unwrap();
    let inner = udp_frame(40100, 50000, &heartbeat);
    // ERSPAN type II: GRE with a sequence number, then an 8-byte ERSPAN header.
    let erspan_ii = [
        0x10, 0x00, 0x88, 0xBE, 0, 0, 0, 1, 0x10, 0x64, 0x00, 0x01, 0, 0, 0, 0,
    ];
    let file = CaptureFile::write(
        "gre",
        &[
            tunnelled(47, &[0x00, 0x00, 0x65, 0x58], &inner),
            tunnelled(47, &erspan_ii, &inner),
            tunnelled(47, &[0x00, 0x00, 0x88, 0xBE], &inner),
            // A GRE protocol the pipeline does not know stays a non-transport packet.
            tunnelled(47, &[0x00, 0x00, 0x12, 0x34], &inner),
        ],
    );
    let (stats, records) = replay(&file, None);
    assert_eq!(stats.frames, 4);
    assert_eq!(stats.decoded, 3);
    assert_eq!(stats.skipped, 1);
    for record in &records {
        // The record describes the mirrored packet, not the tunnel.
        assert_eq!(record.src_ip, [192, 0, 2, 1]);
        assert_eq!(record.src_port, Port::Number(40100));
        assert!(matches!(record.tunnel.as_deref(), Some("gre" | "erspan")));
        assert_eq!(record.message_type.as_deref(), Some("Heartbeat"));
    }
}

#[test]
fn vxlan_is_unwrapped_only_on_a_named_port() {
    let heartbeat = hex::decode(HEARTBEAT).unwrap();
    let file = CaptureFile::write(
        "vxlan",
        &[vxlan_frame(&udp_frame(40100, 50000, &heartbeat))],
    );

    let mut source = FileCapture::open(&file.0, None).unwrap();
    let pipeline = Pipeline::new(Some(dictionary())).with_decapsulation(mind::Decapsulation {
        keep_gre: false,
        vxlan_ports: vec![4789],
    });
    let mut ports = Vec::new();
    let mut sink = |record: &PacketInfoWithParsedMessage, _: &str| {
        assert_eq!(record.tunnel.as_deref(), Some("vxlan"));
        ports.push((record.src_port.number(), record.message_type.clone()));
    };
    mind::run(
        &mut source,
        &pipeline,
        &mut sink,
        None,
        None,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(ports, [(Some(40100), Some("Heartbeat".to_string()))]);
    assert_eq!(pipeline.unwrapped(), 1);

    // Without the option the outer UDP packet is published as it is.
    let (stats, records) = replay(&file, None);
    assert_eq!(stats.decoded, 0);
    assert_eq!(records[0].dst_port, Port::Number(4789));
}
