//! `mind discover`: passive survey of an unknown sensor's traffic.
//!
//! Reads frames from a capture file or, for a set time, from an interface,
//! and sends nothing. It reports who talks (IPv4 address and MAC), ARP, every
//! UDP and TCP flow with its packet count, sizes and rate, multicast and
//! broadcast destinations, other EtherTypes, and the shape of each flow's
//! payloads: ASTERIX (category and blocks), Cursor-on-Target XML or protobuf,
//! other XML, JSON, NMEA 0183, MAVLink 2, DDS/RTPS, text, or the leading
//! bytes. Where a shape matches a goat-transformers pack, the pack is named.
//! With a dictionary, flows on a port an interface claims are decoded and
//! their message types counted.

use crate::source::{Frame, FrameSource, LinkType, SourceError};
use bite::Dictionary;
use etherparse::{NetSlice, SlicedPacket, TransportSlice};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;
use std::time::{Duration, Instant, SystemTime};

/// ASTERIX categories in use, as the first byte of a data block.
const ASTERIX_CATEGORIES: [u8; 19] = [
    1, 2, 8, 10, 11, 19, 20, 21, 23, 25, 30, 32, 34, 48, 62, 63, 65, 240, 247,
];

/// The shape of one payload, and the goat-transformers pack that reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    pub name: String,
    pub pack: Option<&'static str>,
}

fn shape(name: impl Into<String>, pack: Option<&'static str>) -> Shape {
    Shape {
        name: name.into(),
        pack,
    }
}

/// ASTERIX data blocks that fill the payload exactly: (category, blocks).
fn asterix(payload: &[u8]) -> Option<(u8, usize)> {
    let category = *payload.first()?;
    if !ASTERIX_CATEGORIES.contains(&category) {
        return None;
    }
    let mut rest = payload;
    let mut blocks = 0;
    while !rest.is_empty() {
        if rest.len() < 3 || rest[0] != category {
            return None;
        }
        let length = u16::from_be_bytes([rest[1], rest[2]]) as usize;
        if length < 3 || length > rest.len() {
            return None;
        }
        rest = &rest[length..];
        blocks += 1;
    }
    Some((category, blocks))
}

fn printable(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes
            .iter()
            .all(|&c| (32..127).contains(&c) || matches!(c, b'\t' | b'\n' | b'\r'))
}

/// Classify one payload.
pub fn classify(payload: &[u8]) -> Shape {
    if let Some((category, blocks)) = asterix(payload) {
        let pack = (category == 62).then_some("asterix-cat062");
        let name = if blocks == 1 {
            format!("ASTERIX CAT{category:03}")
        } else {
            format!("ASTERIX CAT{category:03} ({blocks} blocks)")
        };
        return shape(name, pack);
    }
    // TAK protocol version 1: 0xbf, a version varint, 0xbf, then protobuf.
    if payload.len() > 3 && payload[0] == 0xbf && payload[2] == 0xbf {
        return shape("CoT protobuf (TAK protocol)", Some("cot-protobuf"));
    }
    // MAVLink 2: 0xfd, payload length; header 10 bytes, checksum 2,
    // optional signature 13.
    if payload.len() >= 12 && payload[0] == 0xfd {
        let total = payload[1] as usize + 12;
        if payload.len() == total || payload.len() == total + 13 {
            return shape("MAVLink 2", Some("mavlink2"));
        }
    }
    if payload.starts_with(b"RTPS") {
        return shape("DDS/RTPS", None);
    }
    let head = &payload[..payload.len().min(512)];
    let trimmed: &[u8] = {
        let start = head
            .iter()
            .position(|c| !c.is_ascii_whitespace())
            .unwrap_or(head.len());
        &head[start..]
    };
    // The text shapes need text: a binary message whose first byte happens
    // to be a newline and whose second is `<` is not XML.
    if !printable(&trimmed[..trimmed.len().min(40)]) {
        let lead: String = payload.iter().take(2).map(|b| format!("{b:02x}")).collect();
        return shape(format!("binary, leading bytes {lead}"), None);
    }
    if trimmed.starts_with(b"<") {
        let text = String::from_utf8_lossy(trimmed);
        if text.contains("<event") {
            return shape("CoT XML", Some("cot-xml"));
        }
        if text.contains("<nvg") {
            return shape("NVG XML", Some("nvg"));
        }
        return shape("XML", None);
    }
    if trimmed.starts_with(b"{") || trimmed.starts_with(b"[") {
        return shape("JSON", None);
    }
    if (trimmed.starts_with(b"$") || trimmed.starts_with(b"!"))
        && trimmed.len() > 6
        && trimmed[..trimmed.len().min(12)].contains(&b',')
        && printable(&trimmed[..trimmed.len().min(40)])
    {
        return shape("NMEA 0183", None);
    }
    if printable(&payload[..payload.len().min(40)]) {
        return shape("text", None);
    }
    let lead: String = payload.iter().take(2).map(|b| format!("{b:02x}")).collect();
    shape(format!("binary, leading bytes {lead}"), None)
}

/// One UDP or TCP flow.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Flow {
    pub protocol: String,
    pub source: String,
    pub destination: String,
    /// `multicast`, `broadcast` or empty.
    pub kind: String,
    pub packets: u64,
    pub bytes: u64,
    pub min_payload: usize,
    pub max_payload: usize,
    /// Payload shape to packets.
    pub shapes: BTreeMap<String, u64>,
    /// goat-transformers packs whose shape was seen.
    pub packs: BTreeSet<String>,
    /// Dictionary interface claiming a port of the flow, and the message
    /// types decoded, when a dictionary was given.
    pub interface: Option<String>,
    pub messages: BTreeMap<String, u64>,
    /// The first payload's leading bytes, hex.
    pub first_payload: String,
    #[serde(skip)]
    first_seen: Option<SystemTime>,
    #[serde(skip)]
    last_seen: Option<SystemTime>,
    /// Packets per second over the time the flow was seen.
    pub rate: f64,
}

/// One IPv4 address seen as a source.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Talker {
    pub address: String,
    pub macs: BTreeSet<String>,
    pub packets: u64,
    pub bytes: u64,
}

/// The whole survey.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Report {
    pub source: String,
    pub frames: u64,
    pub seconds: f64,
    pub talkers: Vec<Talker>,
    /// (sender IPv4, sender MAC, target IPv4) to count.
    pub arp: Vec<ArpEntry>,
    pub flows: Vec<Flow>,
    /// EtherType (hex) to frames, for frames that are neither IPv4 nor ARP.
    pub other_ethertypes: BTreeMap<String, u64>,
    /// IP protocol number to frames, for IPv4 that is neither UDP nor TCP.
    pub other_ip_protocols: BTreeMap<u8, u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArpEntry {
    pub sender: String,
    pub sender_mac: String,
    pub target: String,
    pub count: u64,
}

fn mac(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn destination_kind(address: Ipv4Addr) -> &'static str {
    if address.is_multicast() {
        "multicast"
    } else if address.is_broadcast() || address.octets()[3] == 255 {
        "broadcast"
    } else {
        ""
    }
}

/// Accumulates frames into a [`Report`].
pub struct Survey<'d> {
    dictionary: Option<&'d Dictionary>,
    frames: u64,
    talkers: BTreeMap<Ipv4Addr, Talker>,
    arp: BTreeMap<(String, String, String), u64>,
    flows: BTreeMap<(String, String, String), Flow>,
    other_ethertypes: BTreeMap<String, u64>,
    other_ip_protocols: BTreeMap<u8, u64>,
    first: Option<SystemTime>,
    last: Option<SystemTime>,
}

impl<'d> Survey<'d> {
    pub fn new(dictionary: Option<&'d Dictionary>) -> Self {
        Survey {
            dictionary,
            frames: 0,
            talkers: BTreeMap::new(),
            arp: BTreeMap::new(),
            flows: BTreeMap::new(),
            other_ethertypes: BTreeMap::new(),
            other_ip_protocols: BTreeMap::new(),
            first: None,
            last: None,
        }
    }

    pub fn add(&mut self, frame: &Frame<'_>) {
        self.frames += 1;
        self.first.get_or_insert(frame.timestamp);
        self.last = Some(frame.timestamp);
        let (source_mac, ethertype, after) = match frame.link {
            LinkType::Ethernet => {
                if frame.data.len() < 14 {
                    return;
                }
                let mut ethertype = u16::from_be_bytes([frame.data[12], frame.data[13]]);
                let mut at = 14;
                // VLAN tags.
                while matches!(ethertype, 0x8100 | 0x88a8) && frame.data.len() >= at + 4 {
                    ethertype = u16::from_be_bytes([frame.data[at + 2], frame.data[at + 3]]);
                    at += 4;
                }
                (Some(mac(&frame.data[6..12])), ethertype, &frame.data[at..])
            }
            LinkType::RawIp => (None, 0x0800, frame.data),
        };
        if ethertype == 0x0806 {
            if after.len() >= 28 {
                let sender = Ipv4Addr::new(after[14], after[15], after[16], after[17]).to_string();
                let target = Ipv4Addr::new(after[24], after[25], after[26], after[27]).to_string();
                *self
                    .arp
                    .entry((sender, mac(&after[8..14]), target))
                    .or_default() += 1;
            }
            return;
        }
        if ethertype != 0x0800 {
            *self
                .other_ethertypes
                .entry(format!("0x{ethertype:04x}"))
                .or_default() += 1;
            return;
        }
        let sliced = match frame.link {
            LinkType::Ethernet => SlicedPacket::from_ethernet(frame.data),
            LinkType::RawIp => SlicedPacket::from_ip(frame.data),
        };
        let Ok(sliced) = sliced else {
            return;
        };
        let Some(NetSlice::Ipv4(ip)) = &sliced.net else {
            return;
        };
        let source = ip.header().source_addr();
        let destination = ip.header().destination_addr();
        let length = frame.data.len() as u64;
        let talker = self.talkers.entry(source).or_insert_with(|| Talker {
            address: source.to_string(),
            ..Talker::default()
        });
        talker.packets += 1;
        talker.bytes += length;
        if let Some(source_mac) = &source_mac {
            talker.macs.insert(source_mac.clone());
        }
        let (protocol, source_port, destination_port, payload) = match &sliced.transport {
            Some(TransportSlice::Udp(udp)) => (
                "UDP",
                udp.source_port(),
                udp.destination_port(),
                udp.payload(),
            ),
            Some(TransportSlice::Tcp(tcp)) => (
                "TCP",
                tcp.source_port(),
                tcp.destination_port(),
                tcp.payload(),
            ),
            _ => {
                *self
                    .other_ip_protocols
                    .entry(ip.header().protocol().0)
                    .or_default() += 1;
                return;
            }
        };
        let key = (
            protocol.to_string(),
            format!("{source}:{source_port}"),
            format!("{destination}:{destination_port}"),
        );
        let interface = self
            .dictionary
            .and_then(|dictionary| dictionary.interface_for_ports(source_port, destination_port));
        let flow = self.flows.entry(key.clone()).or_insert_with(|| Flow {
            protocol: key.0.clone(),
            source: key.1.clone(),
            destination: key.2.clone(),
            kind: destination_kind(destination).to_string(),
            min_payload: usize::MAX,
            first_payload: payload
                .iter()
                .take(24)
                .map(|b| format!("{b:02x}"))
                .collect(),
            interface: interface.map(|interface| interface.name.clone()),
            ..Flow::default()
        });
        flow.packets += 1;
        flow.bytes += length;
        flow.first_seen.get_or_insert(frame.timestamp);
        flow.last_seen = Some(frame.timestamp);
        if payload.is_empty() {
            return;
        }
        flow.min_payload = flow.min_payload.min(payload.len());
        flow.max_payload = flow.max_payload.max(payload.len());
        let found = classify(payload);
        *flow.shapes.entry(found.name).or_default() += 1;
        if let Some(pack) = found.pack {
            flow.packs.insert(pack.to_string());
        }
        // An interface with framing concatenated holds several messages per
        // payload; each is counted.
        if let Some(interface) = interface {
            for decoded in interface.decode_all(payload).into_iter().flatten() {
                *flow.messages.entry(decoded.message_type()).or_default() += 1;
            }
        }
    }

    pub fn report(self, source: String) -> Report {
        let seconds = match (self.first, self.last) {
            (Some(first), Some(last)) => {
                last.duration_since(first).unwrap_or_default().as_secs_f64()
            }
            _ => 0.0,
        };
        let mut flows: Vec<Flow> = self
            .flows
            .into_values()
            .map(|mut flow| {
                if flow.min_payload == usize::MAX {
                    flow.min_payload = 0;
                }
                let span = match (flow.first_seen, flow.last_seen) {
                    (Some(first), Some(last)) => {
                        last.duration_since(first).unwrap_or_default().as_secs_f64()
                    }
                    _ => 0.0,
                };
                flow.rate = if span > 0.0 {
                    ((flow.packets - 1) as f64 / span * 100.0).round() / 100.0
                } else {
                    0.0
                };
                flow
            })
            .collect();
        flows.sort_by(|a, b| b.packets.cmp(&a.packets).then(a.source.cmp(&b.source)));
        let mut arp: Vec<ArpEntry> = self
            .arp
            .into_iter()
            .map(|((sender, sender_mac, target), count)| ArpEntry {
                sender,
                sender_mac,
                target,
                count,
            })
            .collect();
        arp.sort_by(|a, b| b.count.cmp(&a.count).then(a.sender.cmp(&b.sender)));
        Report {
            source,
            frames: self.frames,
            seconds: (seconds * 1000.0).round() / 1000.0,
            talkers: self.talkers.into_values().collect(),
            arp,
            flows,
            other_ethertypes: self.other_ethertypes,
            other_ip_protocols: self.other_ip_protocols,
        }
    }
}

/// Read `source` until it ends or `limit` has passed, and survey every frame.
pub fn survey(
    source: &mut dyn FrameSource,
    dictionary: Option<&Dictionary>,
    limit: Option<Duration>,
) -> Result<Report, SourceError> {
    let started = Instant::now();
    let mut survey = Survey::new(dictionary);
    loop {
        if limit.is_some_and(|limit| started.elapsed() >= limit) {
            break;
        }
        let got = match source.next_frame()? {
            Some(frame) => {
                survey.add(&frame);
                true
            }
            None => false,
        };
        if !got && source.finished() {
            break;
        }
    }
    Ok(survey.report(source.describe()))
}

/// `mind discover`.
pub fn run(opt: &crate::cli::DiscoverOpt) -> std::process::ExitCode {
    use crate::source::{CaptureOptions, FileCapture, LiveCapture};
    use std::process::ExitCode;
    let fail = |message: String| {
        eprintln!("mind discover: {message}");
        ExitCode::FAILURE
    };
    let dictionary = match &opt.dictionary {
        Some(path) => match Dictionary::load(path) {
            Ok(dictionary) => Some(dictionary),
            Err(e) => return fail(format!("dictionary {}: {e}", path.display())),
        },
        None => None,
    };
    let (mut source, limit): (Box<dyn FrameSource>, Option<Duration>) =
        match (&opt.pcap, &opt.iface) {
            (Some(path), _) => match FileCapture::open(path, opt.filter.as_deref()) {
                Ok(source) => (Box::new(source), None),
                Err(e) => return fail(e.to_string()),
            },
            (None, Some(iface)) => {
                let options = CaptureOptions {
                    promiscuous: true,
                    filter: opt.filter.clone(),
                    buffer_bytes: 8 * 1024 * 1024,
                };
                match LiveCapture::open(iface, &options) {
                    Ok(source) => (Box::new(source), Some(Duration::from_secs(opt.seconds))),
                    Err(e) => return fail(e.to_string()),
                }
            }
            (None, None) => return fail("give a capture file or --iface".to_string()),
        };
    if limit.is_some() {
        eprintln!(
            "mind discover: listening for {} s; nothing is sent",
            opt.seconds
        );
    }
    let report = match survey(source.as_mut(), dictionary.as_ref(), limit) {
        Ok(report) => report,
        Err(e) => return fail(e.to_string()),
    };
    if opt.json {
        match serde_json::to_string_pretty(&report) {
            Ok(text) => println!("{text}"),
            Err(e) => return fail(e.to_string()),
        }
    } else {
        print!("{}", render(&report));
    }
    ExitCode::SUCCESS
}

/// The report as text tables.
pub fn render(report: &Report) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{}: {} frames over {:.1} s. Nothing was sent.\n",
        report.source, report.frames, report.seconds
    ));
    out.push_str("\nTalkers (IPv4 source, MAC, packets, bytes)\n");
    for talker in &report.talkers {
        out.push_str(&format!(
            "  {:<16} {:<17} {:>8} {:>10}\n",
            talker.address,
            talker.macs.iter().cloned().collect::<Vec<_>>().join(","),
            talker.packets,
            talker.bytes
        ));
    }
    if !report.arp.is_empty() {
        out.push_str("\nARP (sender, sender MAC, asks for, count)\n");
        for entry in &report.arp {
            out.push_str(&format!(
                "  {:<16} {:<17} {:<16} {:>6}\n",
                entry.sender, entry.sender_mac, entry.target, entry.count
            ));
        }
    }
    out.push_str(
        "\nFlows (protocol, source -> destination, packets, packets/s, payload bytes min-max, shapes)\n",
    );
    for flow in &report.flows {
        let kind = if flow.kind.is_empty() {
            String::new()
        } else {
            format!(" [{}]", flow.kind)
        };
        let shapes = flow
            .shapes
            .iter()
            .map(|(name, count)| format!("{name} x{count}"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "  {} {:<21} -> {:<21}{kind} {:>7} {:>8.2}/s {:>5}-{:<5} {}\n",
            flow.protocol,
            flow.source,
            flow.destination,
            flow.packets,
            flow.rate,
            flow.min_payload,
            flow.max_payload,
            shapes
        ));
        if !flow.packs.is_empty() {
            out.push_str(&format!(
                "      goat-transformers pack: {}\n",
                flow.packs.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        if let Some(interface) = &flow.interface {
            let messages = flow
                .messages
                .iter()
                .map(|(name, count)| format!("{name} x{count}"))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(
                "      dictionary interface {interface}: {messages}\n"
            ));
        }
        if !flow.first_payload.is_empty() {
            out.push_str(&format!("      first payload: {}\n", flow.first_payload));
        }
    }
    if !report.other_ethertypes.is_empty() {
        out.push_str("\nOther EtherTypes (frames)\n");
        for (ethertype, count) in &report.other_ethertypes {
            out.push_str(&format!("  {ethertype} {count}\n"));
        }
    }
    if !report.other_ip_protocols.is_empty() {
        out.push_str("\nOther IP protocols (number, frames)\n");
        for (protocol, count) in &report.other_ip_protocols {
            out.push_str(&format!("  {protocol} {count}\n"));
        }
    }
    out
}
