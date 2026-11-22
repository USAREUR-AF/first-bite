//! `mind discover` on capture files built in code.

use etherparse::{PacketBuilder, VlanId};
use mind::discover::classify;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SRC_MAC: [u8; 6] = [0x02, 0x11, 0x22, 0x33, 0x44, 0x55];
const DST_MAC: [u8; 6] = [0xff; 6];

fn udp(source: [u8; 4], sport: u16, destination: [u8; 4], dport: u16, payload: &[u8]) -> Vec<u8> {
    let builder = PacketBuilder::ethernet2(SRC_MAC, DST_MAC)
        .ipv4(source, destination, 64)
        .udp(sport, dport);
    let mut frame = Vec::new();
    builder.write(&mut frame, payload).unwrap();
    frame
}

fn arp_request(sender: [u8; 4], target: [u8; 4]) -> Vec<u8> {
    let mut frame = Vec::new();
    frame.extend_from_slice(&DST_MAC);
    frame.extend_from_slice(&SRC_MAC);
    frame.extend_from_slice(&[0x08, 0x06]);
    frame.extend_from_slice(&[0, 1, 0x08, 0, 6, 4, 0, 1]);
    frame.extend_from_slice(&SRC_MAC);
    frame.extend_from_slice(&sender);
    frame.extend_from_slice(&[0; 6]);
    frame.extend_from_slice(&target);
    frame
}

/// Write `frames` (seconds, bytes) as a little-endian microsecond pcap.
fn write_pcap(path: &Path, frames: &[(u32, Vec<u8>)]) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0xa1b2_c3d4u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&65535u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    for (seconds, frame) in frames {
        bytes.extend_from_slice(&seconds.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        bytes.extend_from_slice(frame);
    }
    fs::write(path, bytes).unwrap();
}

fn cat062() -> Vec<u8> {
    vec![62, 0, 8, 1, 2, 3, 4, 5]
}

fn capture(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("mind-discover-{}-{name}.pcap", std::process::id()));
    let sensor = [192, 0, 2, 5];
    let mut frames = vec![];
    for second in 0..3 {
        frames.push((
            1_700_000_000 + second,
            udp(sensor, 5000, [239, 1, 2, 3], 8600, &cat062()),
        ));
    }
    // ASTERIX CAT048, two blocks in one datagram.
    let two_blocks = [&[48, 0, 5, 1, 2][..], &[48, 0, 4, 9][..]].concat();
    frames.push((
        1_700_000_001,
        udp(sensor, 5001, [192, 0, 2, 9], 8601, &two_blocks),
    ));
    let cot = br#"<?xml version="1.0"?><event version="2.0" uid="x" type="a-f-G"/>"#;
    frames.push((
        1_700_000_001,
        udp(sensor, 6969, [192, 0, 2, 255], 6969, cot),
    ));
    frames.push((
        1_700_000_002,
        udp(sensor, 7000, [192, 0, 2, 9], 7000, br#"{"id": 1}"#),
    ));
    frames.push((
        1_700_000_002,
        udp(
            sensor,
            10110,
            [192, 0, 2, 9],
            10110,
            b"$GPGGA,123519,4807.038,N,01131.000,E\r\n",
        ),
    ));
    let mut mavlink = vec![0xfd, 4, 0, 0, 1, 1, 1, 0, 0, 0];
    mavlink.extend_from_slice(&[1, 2, 3, 4, 0xaa, 0xbb]);
    frames.push((
        1_700_000_002,
        udp(sensor, 14550, [192, 0, 2, 9], 14550, &mavlink),
    ));
    frames.push((
        1_700_000_002,
        udp(
            sensor,
            4242,
            [192, 0, 2, 9],
            4242,
            &[0xbf, 1, 0xbf, 0x0a, 0x02],
        ),
    ));
    frames.push((
        1_700_000_002,
        udp(sensor, 7400, [239, 255, 0, 1], 7400, b"RTPS\x02\x03"),
    ));
    frames.push((
        1_700_000_002,
        udp(
            sensor,
            9000,
            [192, 0, 2, 9],
            9000,
            &[0x00, 0xff, 0x10, 0x80],
        ),
    ));
    // The weather-station dictionary's Heartbeat on its port.
    let heartbeat = hex("01000c4b58370000015180600000");
    frames.push((
        1_700_000_002,
        udp([192, 0, 2, 7], 40100, [192, 0, 2, 9], 40100, &heartbeat),
    ));
    // TCP, VLAN-tagged UDP, ICMP, ARP and LLDP.
    let mut tcp = Vec::new();
    PacketBuilder::ethernet2(SRC_MAC, DST_MAC)
        .ipv4(sensor, [192, 0, 2, 9], 64)
        .tcp(41000, 8080, 1, 1024)
        .write(&mut tcp, b"hello sensor")
        .unwrap();
    frames.push((1_700_000_002, tcp));
    let mut vlan = Vec::new();
    PacketBuilder::ethernet2(SRC_MAC, DST_MAC)
        .single_vlan(VlanId::try_new(5).unwrap())
        .ipv4([192, 0, 2, 6], [192, 0, 2, 9], 64)
        .udp(5555, 5555)
        .write(&mut vlan, &cat062())
        .unwrap();
    frames.push((1_700_000_002, vlan));
    let mut icmp = Vec::new();
    PacketBuilder::ethernet2(SRC_MAC, DST_MAC)
        .ipv4(sensor, [192, 0, 2, 9], 64)
        .icmpv4_echo_request(1, 1)
        .write(&mut icmp, b"ping")
        .unwrap();
    frames.push((1_700_000_002, icmp));
    frames.push((1_700_000_002, arp_request(sensor, [192, 0, 2, 1])));
    let mut lldp = vec![0x01, 0x80, 0xc2, 0, 0, 0x0e];
    lldp.extend_from_slice(&SRC_MAC);
    lldp.extend_from_slice(&[0x88, 0xcc, 0, 0]);
    frames.push((1_700_000_002, lldp));
    write_pcap(&path, &frames);
    path
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn discover(args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_mind"))
        .arg("discover")
        .args(args)
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8(output.stdout).unwrap(),
    )
}

fn flow<'a>(report: &'a Value, destination: &str) -> &'a Value {
    report["flows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|flow| flow["destination"] == destination)
        .unwrap_or_else(|| panic!("no flow to {destination}"))
}

#[test]
fn shapes_are_recognised() {
    assert_eq!(classify(&cat062()).name, "ASTERIX CAT062");
    assert_eq!(classify(&cat062()).pack, Some("asterix-cat062"));
    assert_eq!(
        classify(&[48, 0, 5, 1, 2, 48, 0, 4, 9]).name,
        "ASTERIX CAT048 (2 blocks)"
    );
    assert_eq!(classify(&[48, 0, 5, 1, 2]).pack, None);
    // A category byte with a length that does not fit is not ASTERIX.
    assert_ne!(classify(&[62, 0, 40, 1]).name, "ASTERIX CAT062");
    assert_eq!(classify(b"  <event uid='a'/>").pack, Some("cot-xml"));
    assert_eq!(classify(b"<nvg version='2'/>").pack, Some("nvg"));
    assert_eq!(classify(b"<root/>").name, "XML");
    assert_eq!(classify(b"[1,2]").name, "JSON");
    assert_eq!(
        classify(b"!AIVDM,1,1,,A,15M67FC000G?ufbE`FepT@3n00Sa,0*5C").name,
        "NMEA 0183"
    );
    assert_eq!(classify(&[0xbf, 1, 0xbf, 0x0a]).pack, Some("cot-protobuf"));
    assert_eq!(classify(b"RTPS\x02\x03").name, "DDS/RTPS");
    assert_eq!(classify(b"status ok").name, "text");
    // A binary message that starts with a newline byte and then `<`.
    assert_eq!(
        classify(&[0x0a, 0x3c, 0x00, 0x01, 0x02]).name,
        "binary, leading bytes 0a3c"
    );
    assert_eq!(
        classify(&[0x00, 0xff, 0x10]).name,
        "binary, leading bytes 00ff"
    );
}

#[test]
fn a_capture_is_surveyed_as_json() {
    let path = capture("json");
    let dictionary = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dictionaries/weather-station");
    let (ok, out) = discover(&[
        path.to_str().unwrap(),
        "--json",
        "--dictionary",
        dictionary.to_str().unwrap(),
    ]);
    assert!(ok);
    let report: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(report["frames"], 17);
    assert_eq!(report["seconds"], 2.0);

    let asterix = flow(&report, "239.1.2.3:8600");
    assert_eq!(asterix["packets"], 3);
    assert_eq!(asterix["kind"], "multicast");
    assert_eq!(asterix["rate"], 1.0);
    assert_eq!(asterix["shapes"]["ASTERIX CAT062"], 3);
    assert_eq!(asterix["packs"][0], "asterix-cat062");
    assert_eq!(
        asterix["first_payload"],
        "3e000801020304 05".replace(' ', "")
    );

    assert_eq!(
        flow(&report, "192.0.2.9:8601")["shapes"]["ASTERIX CAT048 (2 blocks)"],
        1
    );
    let cot = flow(&report, "192.0.2.255:6969");
    assert_eq!(cot["kind"], "broadcast");
    assert_eq!(cot["packs"][0], "cot-xml");
    assert_eq!(flow(&report, "192.0.2.9:7000")["shapes"]["JSON"], 1);
    assert_eq!(flow(&report, "192.0.2.9:10110")["shapes"]["NMEA 0183"], 1);
    assert_eq!(flow(&report, "192.0.2.9:14550")["packs"][0], "mavlink2");
    assert_eq!(flow(&report, "192.0.2.9:4242")["packs"][0], "cot-protobuf");
    assert_eq!(flow(&report, "239.255.0.1:7400")["shapes"]["DDS/RTPS"], 1);
    assert_eq!(
        flow(&report, "192.0.2.9:9000")["shapes"]["binary, leading bytes 00ff"],
        1
    );
    let tcp = flow(&report, "192.0.2.9:8080");
    assert_eq!(tcp["protocol"], "TCP");
    assert_eq!(tcp["shapes"]["text"], 1);
    assert_eq!(
        flow(&report, "192.0.2.9:5555")["shapes"]["ASTERIX CAT062"],
        1
    );

    let weather = flow(&report, "192.0.2.9:40100");
    assert_eq!(weather["interface"], "WX");
    assert_eq!(weather["messages"]["Heartbeat"], 1);

    let talkers: Vec<&str> = report["talkers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|talker| talker["address"].as_str().unwrap())
        .collect();
    assert_eq!(talkers, vec!["192.0.2.5", "192.0.2.6", "192.0.2.7"]);
    let mac: Vec<String> = SRC_MAC.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(report["talkers"][0]["macs"][0], mac.join(":").as_str());
    assert_eq!(report["arp"][0]["sender"], "192.0.2.5");
    assert_eq!(report["arp"][0]["target"], "192.0.2.1");
    assert_eq!(report["other_ethertypes"]["0x88cc"], 1);
    assert_eq!(report["other_ip_protocols"]["1"], 1);
    fs::remove_file(path).unwrap();
}

#[test]
fn the_text_report_names_packs_and_says_nothing_was_sent() {
    let path = capture("text");
    let (ok, out) = discover(&[path.to_str().unwrap()]);
    assert!(ok);
    assert!(
        out.contains("17 frames over 2.0 s. Nothing was sent."),
        "{out}"
    );
    assert!(out.contains("goat-transformers pack: asterix-cat062"));
    assert!(out.contains("[multicast]"));
    assert!(out.contains("ARP (sender"));
    fs::remove_file(path).unwrap();
}

#[test]
fn a_missing_file_fails_cleanly() {
    let (ok, _) = discover(&["/nonexistent/capture.pcap"]);
    assert!(!ok);
}
