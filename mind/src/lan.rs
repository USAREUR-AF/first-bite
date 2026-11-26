//! `mind lan`: three read-only checks of the link a capture interface sits on.
//!
//! * `hub`: does this port receive unicast frames between other hosts? On a hub
//!   or a mirror port it does; on a switch port it does not, and passive capture
//!   of other hosts' traffic is not possible there.
//! * `clients`: which hosts hold TCP connections to one server port, and how
//!   often each connects and is cut off. A sensor that serves one client at a
//!   time and drops the rest shows up as several clients reconnecting.
//! * `follow`: copy one server's TCP stream to its client off the wire,
//!   reassembled in order, without ever connecting to the server, and serve it
//!   on a local port as a stand-in for the server. A program that would have
//!   connected to the server connects to the stand-in instead, so the server
//!   keeps exactly the clients it had.
//!
//! Nothing here sends on the capture interface.

use crate::source::{Frame, FrameSource, LinkType, SourceError};
use etherparse::{NetSlice, SlicedPacket, TransportSlice};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// One TCP segment, as the checks need it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub source: SocketAddrV4,
    pub destination: SocketAddrV4,
    pub sequence: u32,
    pub syn: bool,
    pub fin: bool,
    pub rst: bool,
    pub payload: Vec<u8>,
}

/// The Ethernet part of a frame: (destination MAC, source MAC, the bytes after
/// the Ethernet header and any VLAN tags). `None` on a raw-IP link.
fn ethernet(frame: &Frame<'_>) -> Option<([u8; 6], [u8; 6], usize)> {
    if frame.link != LinkType::Ethernet || frame.data.len() < 14 {
        return None;
    }
    let mut destination = [0u8; 6];
    let mut source = [0u8; 6];
    destination.copy_from_slice(&frame.data[0..6]);
    source.copy_from_slice(&frame.data[6..12]);
    Some((destination, source, 14))
}

/// The TCP segment in `frame`, if it carries IPv4 TCP.
pub fn segment(frame: &Frame<'_>) -> Option<Segment> {
    let sliced = match frame.link {
        LinkType::Ethernet => SlicedPacket::from_ethernet(frame.data),
        LinkType::RawIp => SlicedPacket::from_ip(frame.data),
    }
    .ok()?;
    let Some(NetSlice::Ipv4(ip)) = &sliced.net else {
        return None;
    };
    let Some(TransportSlice::Tcp(tcp)) = &sliced.transport else {
        return None;
    };
    Some(Segment {
        source: SocketAddrV4::new(ip.header().source_addr(), tcp.source_port()),
        destination: SocketAddrV4::new(ip.header().destination_addr(), tcp.destination_port()),
        sequence: tcp.sequence_number(),
        syn: tcp.syn(),
        fin: tcp.fin(),
        rst: tcp.rst(),
        payload: tcp.payload().to_vec(),
    })
}

fn mac_text(mac: &[u8; 6]) -> String {
    mac.iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Parse `aa:bb:cc:dd:ee:ff`.
pub fn parse_mac(text: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut mac = [0u8; 6];
    for (slot, part) in mac.iter_mut().zip(parts) {
        *slot = u8::from_str_radix(part, 16).ok()?;
    }
    Some(mac)
}

/// The MAC address of a local interface, from sysfs (Linux).
pub fn interface_mac(iface: &str) -> Option<[u8; 6]> {
    let text = std::fs::read_to_string(format!("/sys/class/net/{iface}/address")).ok()?;
    parse_mac(&text)
}

// ---------------------------------------------------------------- hub

/// What `mind lan hub` concludes.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct HubReport {
    pub source: String,
    pub local_mac: String,
    pub frames: u64,
    /// Unicast frames addressed to this port's MAC, or sent from it.
    pub own_unicast: u64,
    pub broadcast_or_multicast: u64,
    /// Unicast frames neither to nor from this port: only a hub or a mirror
    /// delivers these.
    pub foreign_unicast: u64,
    /// The distinct (source MAC, destination MAC) pairs of the foreign
    /// unicast frames, at most 20.
    pub foreign_pairs: Vec<(String, String)>,
    /// `hub-or-mirror`, `switch`, or `undetermined` (too little traffic).
    pub verdict: String,
}

/// Count frames by whether they were meant for this port.
pub struct Hub {
    local: [u8; 6],
    frames: u64,
    own: u64,
    group: u64,
    foreign: u64,
    pairs: BTreeSet<([u8; 6], [u8; 6])>,
}

impl Hub {
    pub fn new(local: [u8; 6]) -> Self {
        Hub {
            local,
            frames: 0,
            own: 0,
            group: 0,
            foreign: 0,
            pairs: BTreeSet::new(),
        }
    }

    pub fn add(&mut self, frame: &Frame<'_>) {
        let Some((destination, source, _)) = ethernet(frame) else {
            return;
        };
        self.frames += 1;
        if destination[0] & 1 == 1 {
            self.group += 1;
        } else if destination == self.local || source == self.local {
            self.own += 1;
        } else {
            self.foreign += 1;
            if self.pairs.len() < 20 {
                self.pairs.insert((source, destination));
            }
        }
    }

    /// `min_frames`: below this many frames the verdict is `undetermined`.
    pub fn report(&self, source: String, min_frames: u64) -> HubReport {
        let verdict = if self.foreign > 0 {
            "hub-or-mirror"
        } else if self.frames < min_frames {
            "undetermined"
        } else {
            "switch"
        };
        HubReport {
            source,
            local_mac: mac_text(&self.local),
            frames: self.frames,
            own_unicast: self.own,
            broadcast_or_multicast: self.group,
            foreign_unicast: self.foreign,
            foreign_pairs: self
                .pairs
                .iter()
                .map(|(s, d)| (mac_text(s), mac_text(d)))
                .collect(),
            verdict: verdict.to_string(),
        }
    }
}

// ---------------------------------------------------------------- clients

/// One client host of the server, over the capture.
#[derive(Debug, Serialize, Default, PartialEq, Clone)]
pub struct ClientReport {
    pub address: String,
    /// Connection attempts (SYN from the client).
    pub connects: u64,
    /// Connections the server ended (FIN or RST from the server).
    pub ended_by_server: u64,
    /// Connections the client ended.
    pub ended_by_client: u64,
    /// Payload bytes from the server to this client.
    pub bytes_from_server: u64,
    /// Distinct client ports seen, a count of separate connections.
    pub ports: u64,
    pub first_seen: f64,
    pub last_seen: f64,
}

/// What `mind lan clients` concludes.
#[derive(Debug, Serialize, PartialEq)]
pub struct ClientsReport {
    pub source: String,
    pub server: String,
    pub seconds: f64,
    pub clients: Vec<ClientReport>,
    /// True when more than one host connected to the server and the server
    /// ended connections: they are taking turns on a server that keeps few.
    pub contention: bool,
}

/// Track the TCP clients of one server endpoint.
pub struct Clients {
    server: SocketAddrV4,
    clients: BTreeMap<Ipv4Addr, (ClientReport, BTreeSet<u16>)>,
    first: Option<SystemTime>,
    last: Option<SystemTime>,
}

fn epoch(t: SystemTime) -> f64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

impl Clients {
    pub fn new(server: SocketAddrV4) -> Self {
        Clients {
            server,
            clients: BTreeMap::new(),
            first: None,
            last: None,
        }
    }

    pub fn add(&mut self, frame: &Frame<'_>) {
        let Some(s) = segment(frame) else {
            return;
        };
        let (client, from_server) = if s.destination == self.server {
            (s.source, false)
        } else if s.source == self.server {
            (s.destination, true)
        } else {
            return;
        };
        self.first.get_or_insert(frame.timestamp);
        self.last = Some(frame.timestamp);
        let t = epoch(frame.timestamp);
        let (report, ports) = self.clients.entry(*client.ip()).or_insert_with(|| {
            (
                ClientReport {
                    address: client.ip().to_string(),
                    first_seen: t,
                    ..ClientReport::default()
                },
                BTreeSet::new(),
            )
        });
        report.last_seen = t;
        ports.insert(client.port());
        if s.syn && !from_server {
            report.connects += 1;
        }
        if s.fin || s.rst {
            if from_server {
                report.ended_by_server += 1;
            } else {
                report.ended_by_client += 1;
            }
        }
        if from_server {
            report.bytes_from_server += s.payload.len() as u64;
        }
    }

    pub fn report(&self, source: String) -> ClientsReport {
        let clients: Vec<ClientReport> = self
            .clients
            .values()
            .map(|(r, ports)| ClientReport {
                ports: ports.len() as u64,
                ..r.clone()
            })
            .collect();
        let contention = clients.len() > 1
            && clients
                .iter()
                .any(|c| c.ended_by_server > 0 || c.connects > 1);
        let seconds = match (self.first, self.last) {
            (Some(a), Some(b)) => b.duration_since(a).map(|d| d.as_secs_f64()).unwrap_or(0.0),
            _ => 0.0,
        };
        ClientsReport {
            source,
            server: self.server.to_string(),
            seconds,
            clients,
            contention,
        }
    }
}

// ---------------------------------------------------------------- follow

/// What the follower hands on.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// The next bytes of the followed stream, in order.
    Data(Vec<u8>),
    /// The stream broke or changed; a consumer should reset its framing, as it
    /// would after a disconnect. The text says why.
    Reset(String),
}

/// Counters for `mind lan follow`.
#[derive(Debug, Serialize, Default, Clone, PartialEq, Eq)]
pub struct FollowStats {
    pub segments: u64,
    pub delivered_bytes: u64,
    pub retransmissions: u64,
    pub gaps_skipped: u64,
    pub stream_switches: u64,
    pub following: Option<String>,
}

struct Stream {
    client: SocketAddrV4,
    next: u32,
    pending: BTreeMap<u32, Vec<u8>>,
    last: Instant,
    gap_since: Option<Instant>,
}

/// `a` comes before `b` in TCP sequence space.
fn before(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}

/// Follow one server's stream to one client at a time, reassembled.
pub struct Follower {
    server: SocketAddrV4,
    ignore: BTreeSet<Ipv4Addr>,
    gap_wait: Duration,
    switch_idle: Duration,
    max_pending: usize,
    current: Option<Stream>,
    last_seen: HashMap<SocketAddrV4, Instant>,
    pub stats: FollowStats,
}

impl Follower {
    /// `ignore`: client addresses never followed (this host's own).
    pub fn new(server: SocketAddrV4, ignore: BTreeSet<Ipv4Addr>) -> Self {
        Follower {
            server,
            ignore,
            gap_wait: Duration::from_secs(1),
            switch_idle: Duration::from_secs(5),
            max_pending: 4 * 1024 * 1024,
            current: None,
            last_seen: HashMap::new(),
            stats: FollowStats::default(),
        }
    }

    pub fn with_timing(mut self, gap_wait: Duration, switch_idle: Duration) -> Self {
        self.gap_wait = gap_wait;
        self.switch_idle = switch_idle;
        self
    }

    /// Take one segment seen at `now`; return what to hand on.
    pub fn feed(&mut self, s: &Segment, now: Instant) -> Vec<Event> {
        let mut out = Vec::new();
        if s.source != self.server || self.ignore.contains(s.destination.ip()) {
            return out;
        }
        let client = s.destination;
        if s.fin || s.rst {
            if self.current.as_ref().is_some_and(|c| c.client == client) {
                self.current = None;
                self.stats.following = None;
                out.push(Event::Reset("the server ended the stream".to_string()));
            }
            self.last_seen.remove(&client);
            return out;
        }
        if s.payload.is_empty() {
            return out;
        }
        self.stats.segments += 1;
        self.last_seen.insert(client, now);
        let switch = match &self.current {
            None => true,
            Some(c) => c.client != client && now.duration_since(c.last) > self.switch_idle,
        };
        if switch {
            if self.current.is_some() {
                self.stats.stream_switches += 1;
                out.push(Event::Reset(format!("following the stream to {client}")));
            }
            self.current = Some(Stream {
                client,
                next: s.sequence,
                pending: BTreeMap::new(),
                last: now,
                gap_since: None,
            });
            self.stats.following = Some(client.to_string());
        }
        let max_pending = self.max_pending;
        let Some(cur) = self.current.as_mut() else {
            return out;
        };
        if cur.client != client {
            return out;
        }
        cur.last = now;
        let (mut sequence, mut payload) = (s.sequence, s.payload.as_slice());
        if before(sequence, cur.next) {
            let skip = cur.next.wrapping_sub(sequence) as usize;
            if skip >= payload.len() {
                self.stats.retransmissions += 1;
                return out;
            }
            payload = &payload[skip..];
            sequence = cur.next;
        }
        cur.pending
            .entry(sequence)
            .or_insert_with(|| payload.to_vec());
        if cur.pending.values().map(Vec::len).sum::<usize>() > max_pending {
            cur.pending.clear();
            out.push(Event::Reset("reassembly buffer overflow".to_string()));
        }
        out.extend(self.deliver(now));
        out
    }

    /// Hand on what is in order; skip a gap that has lasted longer than the
    /// gap wait. Call this also when no segment arrives, so a gap is noticed.
    pub fn deliver(&mut self, now: Instant) -> Vec<Event> {
        let mut out = Vec::new();
        let gap_wait = self.gap_wait;
        let Some(cur) = self.current.as_mut() else {
            return out;
        };
        let mut data = Vec::new();
        while let Some(p) = cur.pending.remove(&cur.next) {
            cur.next = cur.next.wrapping_add(p.len() as u32);
            data.extend_from_slice(&p);
            cur.gap_since = None;
        }
        if !data.is_empty() {
            self.stats.delivered_bytes += data.len() as u64;
            out.push(Event::Data(data));
        } else if !cur.pending.is_empty() {
            let since = *cur.gap_since.get_or_insert(now);
            if now.duration_since(since) > gap_wait {
                let next = cur.next;
                let first = *cur
                    .pending
                    .keys()
                    .min_by_key(|q| q.wrapping_sub(next))
                    .expect("pending is not empty");
                cur.next = first;
                cur.gap_since = None;
                self.stats.gaps_skipped += 1;
                out.push(Event::Reset("skipped a gap in the stream".to_string()));
                out.extend(self.deliver(now));
            }
        }
        out
    }
}

/// Read `source` until it ends or `limit` has passed, feeding each frame to `f`.
pub fn each_frame(
    source: &mut dyn FrameSource,
    limit: Option<Duration>,
    mut f: impl FnMut(&Frame<'_>),
) -> Result<(), SourceError> {
    let started = Instant::now();
    loop {
        if limit.is_some_and(|limit| started.elapsed() >= limit) {
            return Ok(());
        }
        match source.next_frame()? {
            Some(frame) => f(&frame),
            None => {
                if source.finished() {
                    return Ok(());
                }
            }
        }
    }
}

// ---------------------------------------------------------------- commands

use crate::cli::{LanAction, LanOpt, LanSource};
use std::io::Write;
use std::process::ExitCode;

/// An opened source, how long to read it, and its name.
type Opened = (Box<dyn FrameSource>, Option<Duration>, String);

fn open(source: &LanSource) -> Result<Opened, String> {
    use crate::source::{CaptureOptions, FileCapture, LiveCapture};
    match (&source.pcap, &source.iface) {
        (Some(path), _) => FileCapture::open(path, source.filter.as_deref())
            .map(|s| {
                (
                    Box::new(s) as Box<dyn FrameSource>,
                    None,
                    path.display().to_string(),
                )
            })
            .map_err(|e| e.to_string()),
        (None, Some(iface)) => {
            let options = CaptureOptions {
                promiscuous: true,
                filter: source.filter.clone(),
                buffer_bytes: 8 * 1024 * 1024,
            };
            let limit = (source.seconds > 0).then(|| Duration::from_secs(source.seconds));
            LiveCapture::open(iface, &options)
                .map(|s| (Box::new(s) as Box<dyn FrameSource>, limit, iface.clone()))
                .map_err(|e| e.to_string())
        }
        (None, None) => Err("give a capture file or --iface".to_string()),
    }
}

/// IPv4 addresses of a local interface (from libpcap's device list).
fn interface_addresses(iface: &str) -> BTreeSet<Ipv4Addr> {
    pcap::Device::list()
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d.name == iface)
        .flat_map(|d| d.addresses)
        .filter_map(|a| match a.addr {
            std::net::IpAddr::V4(v4) => Some(v4),
            _ => None,
        })
        .collect()
}

fn print_json<T: Serialize>(value: &T) -> Result<(), String> {
    serde_json::to_string_pretty(value)
        .map(|text| println!("{text}"))
        .map_err(|e| e.to_string())
}

/// `mind lan`.
pub fn run(opt: &LanOpt) -> ExitCode {
    let result = match &opt.action {
        LanAction::Hub(o) => run_hub(o),
        LanAction::Clients(o) => run_clients(o),
        LanAction::Follow(o) => run_follow(o),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mind lan: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_hub(o: &crate::cli::LanHubOpt) -> Result<(), String> {
    let local = match (&o.local_mac, &o.source.iface) {
        (Some(text), _) => {
            parse_mac(text).ok_or_else(|| format!("--local-mac {text}: not a MAC address"))?
        }
        (None, Some(iface)) => interface_mac(iface)
            .ok_or_else(|| format!("cannot read the MAC address of {iface}; give --local-mac"))?,
        (None, None) => {
            return Err("a capture file needs --local-mac (the capturing port's MAC)".to_string())
        }
    };
    let (mut source, limit, name) = open(&o.source)?;
    if limit.is_some() {
        eprintln!(
            "mind lan hub: listening on {name} for {} s; nothing is sent",
            o.source.seconds
        );
    }
    let mut hub = Hub::new(local);
    each_frame(source.as_mut(), limit, |f| hub.add(f)).map_err(|e| e.to_string())?;
    let report = hub.report(name, o.min_frames);
    if o.json {
        print_json(&report)?;
    } else {
        println!(
            "{} frames on {} (this port {})",
            report.frames, report.source, report.local_mac
        );
        println!("  to or from this port: {}", report.own_unicast);
        println!(
            "  broadcast or multicast: {}",
            report.broadcast_or_multicast
        );
        println!("  unicast between other hosts: {}", report.foreign_unicast);
        for (s, d) in &report.foreign_pairs {
            println!("    {s} -> {d}");
        }
        println!(
            "verdict: {}",
            match report.verdict.as_str() {
                "hub-or-mirror" => "hub or mirror port: other hosts' traffic arrives here, passive capture works",
                "switch" => "switch port: no other hosts' unicast arrived; passive capture needs a mirror port or a tap",
                _ => "undetermined: too little traffic; listen longer",
            }
        );
    }
    Ok(())
}

fn run_clients(o: &crate::cli::LanClientsOpt) -> Result<(), String> {
    let (mut source, limit, name) = open(&o.source)?;
    if limit.is_some() {
        eprintln!(
            "mind lan clients: listening on {name} for {} s; nothing is sent",
            o.source.seconds
        );
    }
    let mut clients = Clients::new(o.server);
    each_frame(source.as_mut(), limit, |f| clients.add(f)).map_err(|e| e.to_string())?;
    let report = clients.report(name);
    if o.json {
        print_json(&report)?;
    } else {
        println!(
            "TCP clients of {} over {:.0} s:",
            report.server, report.seconds
        );
        println!(
            "  {:<16} {:>9} {:>14} {:>14} {:>6} {:>12}",
            "client", "connects", "ended:server", "ended:client", "ports", "bytes in"
        );
        for c in &report.clients {
            println!(
                "  {:<16} {:>9} {:>14} {:>14} {:>6} {:>12}",
                c.address,
                c.connects,
                c.ended_by_server,
                c.ended_by_client,
                c.ports,
                c.bytes_from_server
            );
        }
        if report.contention {
            println!("contention: several hosts connect and the server keeps ending connections; they are taking turns on it");
        }
    }
    Ok(())
}

fn run_follow(o: &crate::cli::LanFollowOpt) -> Result<(), String> {
    use std::net::{TcpListener, TcpStream};
    let mut ignore: BTreeSet<Ipv4Addr> = o.ignore.iter().copied().collect();
    if let Some(iface) = &o.source.iface {
        ignore.extend(interface_addresses(iface));
    }
    let (mut source, limit, name) = open(&o.source)?;
    let mut follower = Follower::new(o.server, ignore).with_timing(
        Duration::from_millis(o.gap_wait_ms),
        Duration::from_millis(o.switch_idle_ms),
    );
    let mut output = match &o.output {
        Some(path) => {
            Some(std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?)
        }
        None => None,
    };
    let listener = match &o.listen {
        Some(addr) => {
            let l = TcpListener::bind(addr).map_err(|e| format!("--listen {addr}: {e}"))?;
            l.set_nonblocking(true).map_err(|e| e.to_string())?;
            eprintln!(
                "mind lan follow: serving the stream of {} on {addr}; nothing is sent on {name}",
                o.server
            );
            Some(l)
        }
        None => None,
    };
    if listener.is_none() && output.is_none() {
        return Err("give --listen ADDR, --output FILE, or both".to_string());
    }
    let mut clients: Vec<TcpStream> = Vec::new();
    let mut handle = |events: Vec<Event>, clients: &mut Vec<TcpStream>| -> Result<(), String> {
        for event in events {
            match event {
                Event::Data(bytes) => {
                    if let Some(file) = output.as_mut() {
                        file.write_all(&bytes).map_err(|e| e.to_string())?;
                    }
                    clients.retain_mut(|c| c.write_all(&bytes).is_ok());
                }
                Event::Reset(why) => {
                    if !clients.is_empty() {
                        eprintln!(
                            "mind lan follow: {why}; dropping {} local client(s)",
                            clients.len()
                        );
                    }
                    clients.clear();
                }
            }
        }
        Ok(())
    };
    let started = Instant::now();
    let mut next_report = Instant::now() + Duration::from_secs(30);
    loop {
        if limit.is_some_and(|limit| started.elapsed() >= limit) {
            break;
        }
        if let Some(l) = &listener {
            while let Ok((stream, peer)) = l.accept() {
                let _ = stream.set_nodelay(true);
                eprintln!("mind lan follow: local client {peer} connected");
                clients.push(stream);
            }
        }
        let (got, seg) = match source.next_frame().map_err(|e| e.to_string())? {
            Some(frame) => (true, segment(&frame)),
            None => (false, None),
        };
        let now = Instant::now();
        let events = match seg {
            Some(s) => follower.feed(&s, now),
            None => follower.deliver(now),
        };
        handle(events, &mut clients)?;
        if !got && source.finished() {
            break;
        }
        if listener.is_some() && now >= next_report {
            next_report = now + Duration::from_secs(30);
            eprintln!(
                "mind lan follow: {}",
                serde_json::to_string(&follower.stats).unwrap_or_default()
            );
        }
    }
    // A file ends: hand on whatever is still waiting behind a gap.
    let flush = follower.deliver(Instant::now() + Duration::from_secs(3600));
    handle(flush, &mut clients)?;
    if o.json {
        print_json(&follower.stats)?;
    } else if listener.is_none() {
        eprintln!(
            "mind lan follow: {} bytes in order from {} segments ({} retransmissions, {} gaps skipped, {} stream switches)",
            follower.stats.delivered_bytes,
            follower.stats.segments,
            follower.stats.retransmissions,
            follower.stats.gaps_skipped,
            follower.stats.stream_switches
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(seq: u32, data: &[u8]) -> Segment {
        Segment {
            source: "192.0.2.1:7003".parse().unwrap(),
            destination: "192.0.2.10:50000".parse().unwrap(),
            sequence: seq,
            syn: false,
            fin: false,
            rst: false,
            payload: data.to_vec(),
        }
    }

    fn data(events: &[Event]) -> Vec<u8> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Data(d) => Some(d.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn reorders_and_drops_retransmissions() {
        let mut f = Follower::new("192.0.2.1:7003".parse().unwrap(), BTreeSet::new());
        let t = Instant::now();
        let mut ev = f.feed(&seg(1000, b"abc"), t);
        ev.extend(f.feed(&seg(1006, b"ghi"), t));
        ev.extend(f.feed(&seg(1003, b"def"), t));
        ev.extend(f.feed(&seg(1003, b"def"), t));
        ev.extend(f.feed(&seg(1001, b"bcdefghij"), t));
        assert_eq!(data(&ev), b"abcdefghij");
        assert_eq!(f.stats.retransmissions, 1);
    }

    #[test]
    fn skips_a_gap_after_the_wait() {
        let mut f = Follower::new("192.0.2.1:7003".parse().unwrap(), BTreeSet::new())
            .with_timing(Duration::from_millis(100), Duration::from_secs(5));
        let t = Instant::now();
        let mut ev = f.feed(&seg(1, b"ab"), t);
        ev.extend(f.feed(&seg(10, b"xy"), t));
        assert_eq!(data(&ev), b"ab");
        let later = f.deliver(t + Duration::from_millis(150));
        assert_eq!(data(&later), b"xy");
        assert!(matches!(later[0], Event::Reset(_)));
        assert_eq!(f.stats.gaps_skipped, 1);
    }

    #[test]
    fn sequence_wraps() {
        let mut f = Follower::new("192.0.2.1:7003".parse().unwrap(), BTreeSet::new());
        let t = Instant::now();
        let mut ev = f.feed(&seg(u32::MAX - 1, b"ab"), t);
        ev.extend(f.feed(&seg(0, b"cd"), t));
        assert_eq!(data(&ev), b"abcd");
    }

    #[test]
    fn ignores_own_address_and_other_servers() {
        let mut own = BTreeSet::new();
        own.insert("192.0.2.10".parse().unwrap());
        let mut f = Follower::new("192.0.2.1:7003".parse().unwrap(), own);
        assert!(f.feed(&seg(1, b"ab"), Instant::now()).is_empty());
        let mut other = seg(1, b"ab");
        other.source = "192.0.2.2:7003".parse().unwrap();
        let mut f = Follower::new("192.0.2.1:7003".parse().unwrap(), BTreeSet::new());
        assert!(f.feed(&other, Instant::now()).is_empty());
    }

    #[test]
    fn switches_streams_only_after_the_current_one_goes_quiet() {
        let mut f = Follower::new("192.0.2.1:7003".parse().unwrap(), BTreeSet::new())
            .with_timing(Duration::from_secs(1), Duration::from_millis(100));
        let t = Instant::now();
        f.feed(&seg(1, b"aa"), t);
        let mut other = seg(500, b"zz");
        other.destination = "192.0.2.11:40000".parse().unwrap();
        assert!(f.feed(&other, t + Duration::from_millis(50)).is_empty());
        let ev = f.feed(&other, t + Duration::from_millis(200));
        assert!(matches!(ev[0], Event::Reset(_)));
        assert_eq!(data(&ev), b"zz");
        assert_eq!(f.stats.stream_switches, 1);
    }

    #[test]
    fn parses_mac() {
        assert_eq!(
            parse_mac("02:11:22:33:44:55"),
            Some([2, 0x11, 0x22, 0x33, 0x44, 0x55])
        );
        assert_eq!(parse_mac("nope"), None);
    }
}
