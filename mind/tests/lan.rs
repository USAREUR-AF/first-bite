//! `mind lan hub`, `clients` and `follow` on capture files built in code.

use etherparse::PacketBuilder;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PROBE: [u8; 6] = [0x02, 0, 0, 0, 0, 0x01]; // the capturing port
const SERVER_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 0x10];
const NAV_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 0x20];
const OTHER_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 0x30];
const SERVER: [u8; 4] = [192, 0, 2, 1];
const NAV: [u8; 4] = [192, 0, 2, 20];
const OTHER: [u8; 4] = [192, 0, 2, 30];
const PORT: u16 = 7003;

#[allow(clippy::too_many_arguments)]
fn tcp(
    macs: ([u8; 6], [u8; 6]),
    source: [u8; 4],
    sport: u16,
    destination: [u8; 4],
    dport: u16,
    seq: u32,
    flags: &str,
    payload: &[u8],
) -> Vec<u8> {
    let mut b = PacketBuilder::ethernet2(macs.0, macs.1)
        .ipv4(source, destination, 64)
        .tcp(sport, dport, seq, 65535);
    if flags.contains('S') {
        b = b.syn();
    }
    if flags.contains('F') {
        b = b.fin();
    }
    if flags.contains('R') {
        b = b.rst();
    }
    if flags.contains('A') {
        b = b.ack(1);
    }
    let mut frame = Vec::new();
    b.write(&mut frame, payload).unwrap();
    frame
}

fn write_pcap(path: &Path, frames: &[Vec<u8>]) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0xa1b2_c3d4u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&65535u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    for (i, frame) in frames.iter().enumerate() {
        bytes.extend_from_slice(&(1_700_000_000u32 + i as u32 / 10).to_le_bytes());
        bytes.extend_from_slice(&((i as u32 % 10) * 100_000).to_le_bytes());
        bytes.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        bytes.extend_from_slice(frame);
    }
    fs::write(path, bytes).unwrap();
}

fn path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mind-lan-{}-{name}", std::process::id()))
}

fn mind(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_mind"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn json(args: &[&str]) -> Value {
    let (ok, stdout, stderr) = mind(args);
    assert!(ok, "mind {args:?} failed: {stderr}");
    serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("{e}: {stdout}"))
}

#[test]
fn hub_sees_other_hosts_unicast() {
    let file = path("hub.pcap");
    let mut frames = vec![];
    for i in 0..10u32 {
        frames.push(tcp(
            (NAV_MAC, SERVER_MAC),
            NAV,
            50000,
            SERVER,
            PORT,
            i,
            "A",
            b"",
        ));
        frames.push(tcp(
            (SERVER_MAC, NAV_MAC),
            SERVER,
            PORT,
            NAV,
            50000,
            i,
            "A",
            b"data",
        ));
    }
    write_pcap(&file, &frames);
    let r = json(&[
        "lan",
        "hub",
        file.to_str().unwrap(),
        "--local-mac",
        "02:00:00:00:00:01",
        "--json",
    ]);
    assert_eq!(r["verdict"], "hub-or-mirror");
    assert_eq!(r["foreign_unicast"], 20);
    assert_eq!(r["own_unicast"], 0);
}

#[test]
fn hub_on_a_switch_port_sees_only_its_own_and_broadcast() {
    let file = path("switch.pcap");
    let mut frames = vec![];
    for i in 0..60u32 {
        frames.push(tcp(
            (PROBE, SERVER_MAC),
            [192, 0, 2, 2],
            40000,
            SERVER,
            22,
            i,
            "A",
            b"",
        ));
        frames.push(tcp(
            (OTHER_MAC, [0xff; 6]),
            OTHER,
            1,
            [192, 0, 2, 255],
            2,
            i,
            "A",
            b"",
        ));
    }
    write_pcap(&file, &frames);
    let r = json(&[
        "lan",
        "hub",
        file.to_str().unwrap(),
        "--local-mac",
        "02:00:00:00:00:01",
        "--json",
    ]);
    assert_eq!(r["verdict"], "switch");
    assert_eq!(r["foreign_unicast"], 0);
    assert_eq!(r["own_unicast"], 60);
    assert_eq!(r["broadcast_or_multicast"], 60);
}

#[test]
fn hub_with_too_little_traffic_is_undetermined() {
    let file = path("quiet.pcap");
    let frames = vec![tcp(
        (PROBE, SERVER_MAC),
        [192, 0, 2, 2],
        40000,
        SERVER,
        22,
        1,
        "A",
        b"",
    )];
    write_pcap(&file, &frames);
    let r = json(&[
        "lan",
        "hub",
        file.to_str().unwrap(),
        "--local-mac",
        "02:00:00:00:00:01",
        "--json",
    ]);
    assert_eq!(r["verdict"], "undetermined");
}

#[test]
fn hub_from_a_file_needs_the_local_mac() {
    let file = path("nomac.pcap");
    write_pcap(&file, &[]);
    let (ok, _, stderr) = mind(&["lan", "hub", file.to_str().unwrap()]);
    assert!(!ok);
    assert!(stderr.contains("--local-mac"), "{stderr}");
}

#[test]
fn clients_shows_two_hosts_taking_turns() {
    let file = path("clients.pcap");
    let mut frames = vec![];
    // The NAV holds one connection; the other host connects five times and is
    // cut off by the server each time.
    frames.push(tcp(
        (NAV_MAC, SERVER_MAC),
        NAV,
        50000,
        SERVER,
        PORT,
        1,
        "S",
        b"",
    ));
    frames.push(tcp(
        (SERVER_MAC, NAV_MAC),
        SERVER,
        PORT,
        NAV,
        50000,
        100,
        "A",
        b"0123456789",
    ));
    for n in 0..5u16 {
        frames.push(tcp(
            (OTHER_MAC, SERVER_MAC),
            OTHER,
            41000 + n,
            SERVER,
            PORT,
            1,
            "S",
            b"",
        ));
        frames.push(tcp(
            (SERVER_MAC, OTHER_MAC),
            SERVER,
            PORT,
            OTHER,
            41000 + n,
            1,
            "RA",
            b"",
        ));
    }
    frames.push(tcp((NAV_MAC, SERVER_MAC), NAV, 9, SERVER, 22, 1, "S", b"")); // another service: ignored
    write_pcap(&file, &frames);
    let r = json(&[
        "lan",
        "clients",
        file.to_str().unwrap(),
        "--server",
        "192.0.2.1:7003",
        "--json",
    ]);
    let clients = r["clients"].as_array().unwrap();
    assert_eq!(clients.len(), 2);
    let other = clients
        .iter()
        .find(|c| c["address"] == "192.0.2.30")
        .unwrap();
    assert_eq!(other["connects"], 5);
    assert_eq!(other["ended_by_server"], 5);
    assert_eq!(other["ports"], 5);
    let nav = clients
        .iter()
        .find(|c| c["address"] == "192.0.2.20")
        .unwrap();
    assert_eq!(nav["bytes_from_server"], 10);
    assert_eq!(r["contention"], true);
}

#[test]
fn follow_reassembles_the_servers_stream_without_connecting() {
    let file = path("follow.pcap");
    let out = path("follow.bin");
    let stream: Vec<u8> = (0..3000u32).map(|i| (i % 251) as u8).collect();
    let chunks: Vec<(u32, &[u8])> = stream
        .chunks(500)
        .enumerate()
        .map(|(i, c)| (10_000 + 500 * i as u32, c))
        .collect();
    let to_nav = |seq: u32, data: &[u8]| {
        tcp(
            (SERVER_MAC, NAV_MAC),
            SERVER,
            PORT,
            NAV,
            50000,
            seq,
            "A",
            data,
        )
    };
    // Out of order, with a retransmission and a partial overlap, plus the NAV's
    // acknowledgements and another host's unrelated traffic.
    let mut frames = vec![
        to_nav(chunks[0].0, chunks[0].1),
        to_nav(chunks[2].0, chunks[2].1),
        to_nav(chunks[1].0, chunks[1].1),
        to_nav(chunks[1].0, chunks[1].1),
        tcp((NAV_MAC, SERVER_MAC), NAV, 50000, SERVER, PORT, 1, "A", b""),
        tcp(
            (OTHER_MAC, NAV_MAC),
            OTHER,
            80,
            NAV,
            3000,
            5,
            "A",
            b"not this",
        ),
        to_nav(chunks[3].0 - 100, &stream[1400..2000]),
        to_nav(chunks[4].0, chunks[4].1),
        to_nav(chunks[5].0, chunks[5].1),
    ];
    frames.push(tcp(
        (SERVER_MAC, NAV_MAC),
        SERVER,
        PORT,
        NAV,
        50000,
        13_000,
        "FA",
        b"",
    ));
    write_pcap(&file, &frames);
    let r = json(&[
        "lan",
        "follow",
        file.to_str().unwrap(),
        "--server",
        "192.0.2.1:7003",
        "--output",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(fs::read(&out).unwrap(), stream);
    assert_eq!(r["delivered_bytes"], 3000);
    assert_eq!(r["retransmissions"], 1);
    assert_eq!(r["gaps_skipped"], 0);
}

#[test]
fn follow_skips_a_missing_segment_at_the_end_of_a_file() {
    let file = path("gap.pcap");
    let out = path("gap.bin");
    let to_nav = |seq: u32, data: &[u8]| {
        tcp(
            (SERVER_MAC, NAV_MAC),
            SERVER,
            PORT,
            NAV,
            50000,
            seq,
            "A",
            data,
        )
    };
    write_pcap(&file, &[to_nav(1, b"aaaa"), to_nav(9, b"cccc")]);
    let r = json(&[
        "lan",
        "follow",
        file.to_str().unwrap(),
        "--server",
        "192.0.2.1:7003",
        "--output",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(fs::read(&out).unwrap(), b"aaaacccc");
    assert_eq!(r["gaps_skipped"], 1);
}

#[test]
fn follow_never_follows_an_ignored_client() {
    let file = path("ignore.pcap");
    let out = path("ignore.bin");
    let to_nav = |seq: u32, data: &[u8]| {
        tcp(
            (SERVER_MAC, NAV_MAC),
            SERVER,
            PORT,
            NAV,
            50000,
            seq,
            "A",
            data,
        )
    };
    write_pcap(&file, &[to_nav(1, b"secret")]);
    let r = json(&[
        "lan",
        "follow",
        file.to_str().unwrap(),
        "--server",
        "192.0.2.1:7003",
        "--ignore",
        "192.0.2.20",
        "--output",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(fs::read(&out).unwrap(), b"");
    assert_eq!(r["delivered_bytes"], 0);
}

#[test]
fn follow_needs_somewhere_to_put_the_stream() {
    let file = path("nowhere.pcap");
    write_pcap(&file, &[]);
    let (ok, _, stderr) = mind(&[
        "lan",
        "follow",
        file.to_str().unwrap(),
        "--server",
        "192.0.2.1:7003",
    ]);
    assert!(!ok);
    assert!(stderr.contains("--listen"), "{stderr}");
}
