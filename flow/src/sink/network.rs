//! Send each payload to network destinations: `subscriber_type: udp` (one
//! datagram per payload, unicast or multicast) and `subscriber_type: tcp` (a
//! stream, optionally inside TLS).
//!
//! The payload is what the subscriber makes of a record. With a `cot:`
//! mapping it is the Cursor-on-Target event built from the record, which is
//! what a TAK server or client reads. With an `xf:` block it is the text the
//! goat-transformers engine writes for the record's payload in the block's
//! target format (cargo feature `goat-transformers`). Without either it is
//! the record itself, in the subscriber's `encoding`.
//!
//! Destinations are the ones the configuration lists; there is no default.
//!
//! UDP sends and forgets. TCP keeps one connection per destination: payloads
//! wait in a bounded queue while the connection is down, the connection is
//! retried with a growing pause, and a payload that finds the queue full is
//! counted and dropped, so a destination that is away never holds the
//! process up.

use super::{Built, Sink, SinkError};
use crate::config::SubscriberConfig;
use crate::encoding::Encoding;
use crate::error::Problem;
use crate::security::http::{read_certificates, tls_config};
use crate::security::{
    is_loopback_host, provider, Fips, PathPosture, Policy, ProbeTarget, Security, Transport,
};
use async_trait::async_trait;
use chrono::Utc;
use log::{debug, info, warn};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use serde::Deserialize;
use socket2::{Domain, Protocol as SocketProtocol, SockAddr, Socket, Type};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpSocket, UdpSocket};
use tokio::sync::{mpsc, OnceCell};
use tokio_rustls::TlsConnector;

/// Payloads a TCP destination holds while its connection is down, when the
/// subscriber does not set `queue`.
pub const DEFAULT_QUEUE: usize = 1000;

/// Largest payload one UDP datagram carries over IPv4.
const MAX_DATAGRAM: usize = 65_507;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// Pause before the first reconnection attempt; doubled after each failure.
const FIRST_RETRY: Duration = Duration::from_secs(1);
const LONGEST_RETRY: Duration = Duration::from_secs(30);
/// How long a `udp` destination keeps the address its name resolved to
/// before the name is resolved again.
const RESOLVE_INTERVAL: Duration = Duration::from_secs(300);
/// How often the counters are logged, when they have changed.
const STATS_INTERVAL: Duration = Duration::from_secs(60);

/// One place payloads are sent, as written in configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DestinationConfig {
    /// `host:port`; an IPv6 address is written `[address]:port`.
    pub address: String,
    /// Local port to send from. Unset: the system chooses one.
    pub source_port: Option<u16>,
    /// UDP multicast only: how many routers a datagram may cross. Unset: 1,
    /// which keeps it on the local network.
    pub ttl: Option<u32>,
    /// UDP multicast only: the interface datagrams leave on, as that
    /// interface's IPv4 address, or its index for an IPv6 group.
    pub interface: Option<String>,
}

/// TLS for a `tcp` subscriber, as written in configuration. A file left out
/// here is taken from the security block: `ca_bundle`, `client_cert` and
/// `client_key`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsConfig {
    /// PEM file of the certificate authorities the server is checked against.
    pub ca_file: Option<PathBuf>,
    /// PEM files of the certificate and key presented to the server.
    pub client_cert_file: Option<PathBuf>,
    pub client_key_file: Option<PathBuf>,
    /// The name the server's certificate must carry. Unset: the host of each
    /// destination.
    pub server_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protocol {
    Udp,
    Tcp,
}

impl Protocol {
    fn name(self) -> &'static str {
        match self {
            Protocol::Udp => "udp",
            Protocol::Tcp => "tcp",
        }
    }
}

/// What is sent for each record.
enum Payload {
    /// The record, in the subscriber's encoding.
    Record(Encoding),
    /// The Cursor-on-Target event a mapping builds from the record.
    Cot(Box<cotify::Mapping>),
    /// What the goat-transformers engine makes of the record's payload.
    #[cfg(feature = "goat-transformers")]
    Xf(Box<crate::xf::Xf>),
}

impl Payload {
    /// The bytes for one record. On a stream a JSON record ends in a newline,
    /// so the stream is JSON lines; a CoT event and a CBOR item say where
    /// they end themselves.
    fn bytes(&self, record: &str, stream: bool) -> Result<Vec<u8>, SinkError> {
        match self {
            Payload::Record(encoding) => {
                let mut bytes = encoding.encode(record.trim())?.into_owned();
                if stream && *encoding == Encoding::Json {
                    bytes.push(b'\n');
                }
                Ok(bytes)
            }
            Payload::Cot(mapping) => {
                let record: serde_json::Value = serde_json::from_str(record)?;
                let message = record
                    .get("message")
                    .and_then(serde_json::Value::as_object)
                    .ok_or(cotify::CotError::NoMessage)?;
                Ok(mapping
                    .event(message, Utc::now())?
                    .cot_xml_string
                    .into_bytes())
            }
            #[cfg(feature = "goat-transformers")]
            Payload::Xf(xf) => Ok(xf.convert_at(record, Utc::now())?.text.into_bytes()),
        }
    }
}

/// The payload of a subscriber with an `xf:` block.
#[cfg(feature = "goat-transformers")]
fn xf_payload(config: &crate::xf::XfConfig) -> Result<Payload, Problem> {
    Ok(Payload::Xf(Box::new(crate::xf::Xf::from_config(config)?)))
}

#[cfg(not(feature = "goat-transformers"))]
fn xf_payload(_: &crate::xf::XfConfig) -> Result<Payload, Problem> {
    Err(Problem::XfFeatureOff)
}

/// What a network sink puts on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wire {
    /// `udp` or `tcp`.
    pub protocol: &'static str,
    /// Whether the stream runs inside TLS.
    pub tls: bool,
    /// Whether a client certificate is configured.
    pub client_certificate: bool,
    /// Each destination as written, and whether it is on this machine.
    pub destinations: Vec<(String, bool)>,
}

/// Whether a sink that puts `wire` on the network may run under `level`, a
/// transport level of the security block. Plaintext is admitted under `tls`
/// and `mtls` only when it stays on this machine; a subscriber that must send
/// it further says so with `security: { transport: unenforced, reason: ... }`.
/// The error says what the level needs and what the configuration gives.
pub fn admit(level: Transport, wire: &Wire) -> Result<(), String> {
    let off_host = || {
        wire.destinations
            .iter()
            .find(|(_, loopback)| !loopback)
            .map(|(address, _)| address.as_str())
    };
    match level {
        Transport::Unenforced => Ok(()),
        Transport::LoopbackPlaintext => match off_host() {
            Some(address) => Err(format!(
                "level `loopback-plaintext` refuses a sink that leaves this host; \
                 `destinations` gives {address}"
            )),
            None => Ok(()),
        },
        Transport::Tls | Transport::Mtls => {
            let name = level.as_str();
            if !wire.tls {
                // Plaintext that stays on this machine crosses no network.
                return match off_host() {
                    None => Ok(()),
                    Some(address) if wire.protocol == "udp" => Err(format!(
                        "level `{name}` needs TLS, and a `udp` subscriber sends plaintext \
                         datagrams; `destinations` gives {address}. Use a `tcp` subscriber \
                         with a `tls` block, or give this subscriber `security: \
                         {{ transport: unenforced, reason: ... }}`"
                    )),
                    Some(address) => Err(format!(
                        "level `{name}` needs TLS; `destinations` gives {address} and the \
                         subscriber has no `tls` block. Add one, or give this subscriber \
                         `security: {{ transport: unenforced, reason: ... }}`"
                    )),
                };
            }
            if level == Transport::Mtls && !wire.client_certificate {
                return Err(
                    "level `mtls` needs a client certificate: set `tls.client_cert_file` and \
                     `tls.client_key_file`, or `client_cert` and `client_key` in the security \
                     block"
                        .to_string(),
                );
            }
            Ok(())
        }
    }
}

/// Counters of one destination.
#[derive(Debug, Default)]
struct Counters {
    /// Payloads put on the queue of a `tcp` destination.
    queued: AtomicU64,
    sent: AtomicU64,
    /// Payloads that found the queue full.
    dropped: AtomicU64,
    /// Payloads a send or a write failed on.
    failed: AtomicU64,
    /// Connections made after the first.
    reconnects: AtomicU64,
}

/// The counters of one destination, as read at one moment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DestinationStats {
    pub sent: u64,
    pub dropped: u64,
    pub failed: u64,
    pub reconnects: u64,
}

impl Counters {
    fn read(&self) -> DestinationStats {
        DestinationStats {
            sent: self.sent.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            reconnects: self.reconnects.load(Ordering::Relaxed),
        }
    }
}

/// Turns a destination's host and port into addresses, in the order to try
/// them.
pub type Resolver = Arc<dyn Fn(&str, u16) -> std::io::Result<Vec<SocketAddr>> + Send + Sync>;

/// A `udp` destination's socket and the address it sends to.
struct Datagrams {
    socket: UdpSocket,
    target: SocketAddr,
    resolved: tokio::time::Instant,
}

/// A destination, checked.
struct Destination {
    /// As written: `host:port`.
    shown: String,
    host: String,
    port: u16,
    source_port: Option<u16>,
    ttl: Option<u32>,
    interface: Option<String>,
    counters: Arc<Counters>,
    /// UDP: the socket and the address in use.
    datagrams: tokio::sync::Mutex<Option<Datagrams>>,
    /// The address in use, for [`NetworkSink::addresses`].
    address: std::sync::Mutex<Option<SocketAddr>>,
    /// Stands in for the system's name resolution, when set.
    resolver: Option<Resolver>,
    /// TCP: the queue its connection task reads.
    queue: OnceCell<mpsc::Sender<Vec<u8>>>,
}

impl Destination {
    fn is_loopback(&self) -> bool {
        is_loopback_host(&self.host)
    }

    /// Every address the host resolves to, in the order the system (or the
    /// resolver that stands in for it) gives them. A name is resolved each
    /// time this is called.
    async fn resolve(&self) -> std::io::Result<Vec<SocketAddr>> {
        let addresses: Vec<SocketAddr> = match &self.resolver {
            Some(resolver) => resolver(&self.host, self.port)?,
            None => tokio::net::lookup_host((self.host.as_str(), self.port))
                .await?
                .collect(),
        };
        if addresses.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{} has no address", self.host),
            ));
        }
        Ok(addresses)
    }

    fn set_address(&self, address: SocketAddr) {
        *self
            .address
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(address);
    }

    /// `, which the name also resolves to ...` for a log line, when a name
    /// gave more than the address in use.
    fn others(&self, all: &[SocketAddr], used: SocketAddr) -> String {
        let others: Vec<String> = all
            .iter()
            .filter(|address| **address != used)
            .map(ToString::to_string)
            .collect();
        if others.is_empty() {
            String::new()
        } else {
            format!(" (the name also resolves to {})", others.join(", "))
        }
    }
}

/// `host` and `port` of `host:port` or `[address]:port`.
fn split_address(address: &str) -> Result<(String, u16), Problem> {
    let invalid = |reason: &str| Problem::InvalidValue {
        key: "destinations",
        reason: format!("{address:?} {reason}; write it as host:port or [ipv6]:port"),
    };
    let (host, port) = address
        .rsplit_once(':')
        .ok_or_else(|| invalid("has no port"))?;
    let port: u16 = port.parse().map_err(|_| invalid("has no port number"))?;
    if port == 0 {
        return Err(invalid("names port 0"));
    }
    // An IPv6 address has colons of its own, so it is written in brackets.
    let host = match host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
    {
        Some(address) if address.parse::<Ipv6Addr>().is_ok() => address,
        None if !host.is_empty() && !host.contains(':') => host,
        _ => return Err(invalid("has no host")),
    };
    Ok((host.to_string(), port))
}

/// A UDP socket for sending to `target`, with the destination's options.
fn udp_socket(destination: &Destination, target: SocketAddr) -> std::io::Result<Socket> {
    let invalid = |reason: String| std::io::Error::new(std::io::ErrorKind::InvalidInput, reason);
    let socket = Socket::new(
        Domain::for_address(target),
        Type::DGRAM,
        Some(SocketProtocol::UDP),
    )?;
    let source_port = destination.source_port.unwrap_or(0);
    if destination.source_port.is_some() {
        socket.set_reuse_address(true)?;
    }
    match target {
        SocketAddr::V4(_) => {
            socket.bind(&SockAddr::from(SocketAddr::from((
                Ipv4Addr::UNSPECIFIED,
                source_port,
            ))))?;
            if target.ip().is_multicast() {
                socket.set_multicast_ttl_v4(destination.ttl.unwrap_or(1))?;
                if let Some(interface) = &destination.interface {
                    let address: Ipv4Addr = interface.parse().map_err(|_| {
                        invalid(format!(
                            "interface {interface:?} is not the IPv4 address of an interface"
                        ))
                    })?;
                    socket.set_multicast_if_v4(&address)?;
                }
            }
        }
        SocketAddr::V6(_) => {
            socket.bind(&SockAddr::from(SocketAddr::from((
                Ipv6Addr::UNSPECIFIED,
                source_port,
            ))))?;
            if target.ip().is_multicast() {
                socket.set_multicast_hops_v6(destination.ttl.unwrap_or(1))?;
                if let Some(interface) = &destination.interface {
                    let index: u32 = interface.parse().map_err(|_| {
                        invalid(format!(
                            "interface {interface:?} is not an interface index, which an IPv6 \
                             group needs"
                        ))
                    })?;
                    socket.set_multicast_if_v6(index)?;
                }
            }
        }
    }
    socket.set_nonblocking(true)?;
    Ok(socket)
}

/// A stream a connection task writes payloads to.
trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

/// TLS for the connections of one sink.
#[derive(Clone)]
struct Tls {
    connector: TlsConnector,
    config: Arc<ClientConfig>,
    server_name: Option<String>,
    /// The certificate presented, when one is configured.
    client_certificate: Option<PathBuf>,
    /// What the server is checked against, for the posture report.
    trust: String,
}

/// The TLS client configuration for a `tls` block under `policy`, what it
/// trusts, and the certificate it presents. The provider, the protocol
/// versions and the key-exchange groups are the policy's. The server is
/// checked against `tls.ca_file`, or the block's `ca_bundle`, and nothing
/// else: a TAK server's certificate comes from its own authority.
fn tls_client_config(
    tls: &TlsConfig,
    policy: &Policy,
) -> Result<(ClientConfig, String, Option<PathBuf>), Problem> {
    let invalid = |key: &'static str, reason: String| Problem::InvalidValue { key, reason };
    let (ca_key, ca_file) = match (&tls.ca_file, &policy.ca_bundle) {
        (Some(path), _) => ("tls.ca_file", path),
        (None, Some(path)) => ("security.ca_bundle", path),
        (None, None) => {
            return Err(invalid(
                "tls",
                "needs `ca_file`, the certificate authorities the server is checked against \
                 (or `ca_bundle` in the security block)"
                    .to_string(),
            ))
        }
    };
    let mut roots = RootCertStore::empty();
    for certificate in read_certificates(ca_key, ca_file)? {
        roots
            .add(certificate)
            .map_err(|e| invalid(ca_key, format!("{}: {e}", ca_file.display())))?;
    }
    let identity = match (&tls.client_cert_file, &tls.client_key_file) {
        (Some(certificate), Some(key)) => Some((certificate.as_path(), key.as_path())),
        (None, None) => policy.identity(),
        _ => {
            return Err(invalid(
                "tls",
                "`client_cert_file` and `client_key_file` go together; one is set without the \
                 other"
                    .to_string(),
            ))
        }
    };
    let config = tls_config(
        policy.min_tls,
        policy.key_exchange,
        roots,
        identity,
        ("tls.client_cert_file", "tls.client_key_file"),
    )?;
    if policy.fips == Fips::Required && !config.fips() {
        return Err(Problem::Security(
            "`fips: required`: the TLS configuration built for this sink is not in FIPS mode"
                .to_string(),
        ));
    }
    Ok((
        config,
        format!("bundle {}", ca_file.display()),
        identity.map(|(certificate, _)| certificate.to_path_buf()),
    ))
}

/// The sink behind `subscriber_type: udp` and `subscriber_type: tcp`.
pub struct NetworkSink {
    name: String,
    protocol: Protocol,
    payload: Payload,
    destinations: Vec<Arc<Destination>>,
    queue: usize,
    tls: Option<Tls>,
    stats_started: OnceCell<()>,
}

impl fmt::Debug for NetworkSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetworkSink")
            .field("name", &self.name)
            .field("wire", &self.wire())
            .finish_non_exhaustive()
    }
}

impl NetworkSink {
    /// The sink with no security block in force.
    pub fn from_config(config: &SubscriberConfig) -> Result<Self, Problem> {
        Self::build(config, &Security::unstated().base)
    }

    /// The sink held to `policy`: its destinations are checked against the
    /// level, and a `tls` block uses the policy's provider, protocol versions
    /// and key exchange.
    pub(crate) fn secured(config: &SubscriberConfig, policy: &Policy) -> Result<Built, Problem> {
        let sink = Self::build(config, policy)?;
        let posture = sink.posture(policy);
        let targets = sink.probe_targets();
        Ok(Built::new(sink, posture, targets))
    }

    fn build(config: &SubscriberConfig, policy: &Policy) -> Result<Self, Problem> {
        let protocol = match config.subscriber_type.as_str() {
            "udp" => Protocol::Udp,
            _ => Protocol::Tcp,
        };
        let invalid = |key: &'static str, reason: String| Problem::InvalidValue { key, reason };
        let listed = config
            .destinations
            .as_deref()
            .filter(|listed| !listed.is_empty())
            .ok_or(Problem::MissingKey("destinations"))?;
        let mut destinations = Vec::new();
        for entry in listed {
            let (host, port) = split_address(&entry.address)?;
            let multicast = host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_multicast());
            if (entry.ttl.is_some() || entry.interface.is_some())
                && !(protocol == Protocol::Udp && multicast)
            {
                return Err(invalid(
                    "destinations",
                    format!(
                        "{}: `ttl` and `interface` apply to a UDP multicast group only",
                        entry.address
                    ),
                ));
            }
            if multicast && protocol == Protocol::Tcp {
                return Err(invalid(
                    "destinations",
                    format!(
                        "{} is a multicast group, which a `tcp` subscriber cannot connect to; \
                         use `subscriber_type: udp`",
                        entry.address
                    ),
                ));
            }
            if entry.ttl.is_some_and(|ttl| ttl > 255) {
                return Err(invalid(
                    "destinations",
                    format!("{}: `ttl` is at most 255", entry.address),
                ));
            }
            destinations.push(Arc::new(Destination {
                shown: entry.address.clone(),
                host,
                port,
                source_port: entry.source_port,
                ttl: entry.ttl,
                interface: entry.interface.clone(),
                counters: Arc::default(),
                datagrams: tokio::sync::Mutex::new(None),
                address: std::sync::Mutex::new(None),
                resolver: None,
                queue: OnceCell::new(),
            }));
        }

        let tls = match (&config.tls, protocol) {
            (None, _) => None,
            (Some(_), Protocol::Udp) => {
                return Err(invalid(
                    "tls",
                    "a `udp` subscriber sends plaintext datagrams and takes no `tls` block; \
                     TLS needs `subscriber_type: tcp`"
                        .to_string(),
                ))
            }
            (Some(tls), Protocol::Tcp) => {
                if let Some(name) = &tls.server_name {
                    ServerName::try_from(name.clone()).map_err(|_| {
                        invalid(
                            "tls.server_name",
                            format!("{name:?} is neither a DNS name nor an IP address"),
                        )
                    })?;
                }
                let (client, trust, client_certificate) = tls_client_config(tls, policy)?;
                let client = Arc::new(client);
                Some(Tls {
                    connector: TlsConnector::from(Arc::clone(&client)),
                    config: client,
                    server_name: tls.server_name.clone(),
                    client_certificate,
                    trust,
                })
            }
        };
        let queue = match (config.queue, protocol) {
            (None, _) => DEFAULT_QUEUE,
            (Some(_), Protocol::Udp) => {
                return Err(invalid(
                    "queue",
                    "applies to a `tcp` subscriber; a `udp` subscriber holds nothing".to_string(),
                ))
            }
            (Some(0), _) => return Err(invalid("queue", "must be at least 1".to_string())),
            (Some(queue), _) => queue,
        };
        let payload = match (&config.cot, &config.xf) {
            (Some(_), Some(_)) => {
                return Err(invalid(
                    "xf",
                    "does not apply with `cot`: give one of the two".to_string(),
                ))
            }
            (Some(mapping), None) => Payload::Cot(Box::new(mapping.clone())),
            (None, Some(xf)) => xf_payload(xf)?,
            (None, None) => Payload::Record(Encoding::from_config(config.encoding.as_deref())?),
        };
        if config.xf.is_some() && config.encoding.is_some() {
            return Err(invalid(
                "encoding",
                "does not apply with `xf`: the target format is sent as it is written".to_string(),
            ));
        }
        if config.cot.is_some() && config.encoding.is_some() {
            return Err(invalid(
                "encoding",
                "does not apply with `cot`: a Cursor-on-Target event is XML".to_string(),
            ));
        }

        let sink = NetworkSink {
            name: config.name.clone(),
            protocol,
            payload,
            destinations,
            queue,
            tls,
            stats_started: OnceCell::new(),
        };
        // The one place this sink is held to the security block's level.
        admit(policy.transport, &sink.wire()).map_err(Problem::Security)?;
        Ok(sink)
    }

    /// Resolve destination names with `resolver` in place of the system. For
    /// a caller that has its own source of addresses, and for tests.
    pub fn with_resolver(mut self, resolver: Resolver) -> Self {
        for destination in &mut self.destinations {
            if let Some(destination) = Arc::get_mut(destination) {
                destination.resolver = Some(Arc::clone(&resolver));
            }
        }
        self
    }

    /// The address each destination is using, in configuration order: the
    /// one a `udp` destination sends to, the one a `tcp` destination is, or
    /// was last, connected to. `None` before the first resolution.
    pub fn addresses(&self) -> Vec<Option<SocketAddr>> {
        self.destinations
            .iter()
            .map(|destination| {
                *destination
                    .address
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
            })
            .collect()
    }

    /// What this sink puts on the wire, for [`admit`].
    pub fn wire(&self) -> Wire {
        Wire {
            protocol: self.protocol.name(),
            tls: self.tls.is_some(),
            client_certificate: self
                .tls
                .as_ref()
                .is_some_and(|tls| tls.client_certificate.is_some()),
            destinations: self
                .destinations
                .iter()
                .map(|destination| (destination.shown.clone(), destination.is_loopback()))
                .collect(),
        }
    }

    /// What this sink will use, for the posture report.
    fn posture(&self, policy: &Policy) -> PathPosture {
        let endpoints: Vec<&str> = self
            .destinations
            .iter()
            .map(|destination| destination.shown.as_str())
            .collect();
        let mut posture = PathPosture::local(
            &self.name,
            self.protocol.name(),
            &endpoints.join(", "),
            policy.transport,
        );
        posture.off_host = self
            .destinations
            .iter()
            .any(|destination| !destination.is_loopback());
        posture.protocol = match self.protocol {
            Protocol::Udp => "UDP datagrams",
            Protocol::Tcp => "TCP stream",
        }
        .to_string();
        posture.overrides = policy.overrides.iter().map(ToString::to_string).collect();
        posture.override_reason = policy.reason.clone();
        match &self.tls {
            None => posture.transport = "plaintext".to_string(),
            Some(tls) => {
                posture.transport = match tls.client_certificate {
                    Some(_) => "mutual TLS",
                    None => "TLS",
                }
                .to_string();
                posture.min_tls = Some(policy.min_tls.to_string());
                posture.key_exchange = Some(policy.key_exchange);
                posture.groups = provider::group_names(policy.key_exchange);
                posture.client_auth = match &tls.client_certificate {
                    Some(certificate) => format!(
                        "certificate {} (presented when the server asks)",
                        certificate.display()
                    ),
                    None => "none".to_string(),
                };
                posture.trust = Some(tls.trust.clone());
                posture.fips = Some(tls.config.fips());
            }
        }
        posture
    }

    /// The destinations a probe can handshake with: those of a `tls` block.
    fn probe_targets(&self) -> Vec<ProbeTarget> {
        let Some(tls) = &self.tls else {
            return Vec::new();
        };
        self.destinations
            .iter()
            .map(|destination| ProbeTarget {
                sink: self.name.clone(),
                endpoint: destination.shown.clone(),
                host: tls
                    .server_name
                    .clone()
                    .unwrap_or_else(|| destination.host.clone()),
                port: destination.port,
                config: Arc::clone(&tls.config),
            })
            .collect()
    }

    /// The counters of each destination, in configuration order.
    pub fn stats(&self) -> Vec<DestinationStats> {
        self.destinations
            .iter()
            .map(|destination| destination.counters.read())
            .collect()
    }

    /// Start the task that logs the counters, once.
    async fn start_stats(&self) {
        self.stats_started
            .get_or_init(|| async {
                let name = self.name.clone();
                let protocol = self.protocol;
                let destinations = self.destinations.clone();
                tokio::spawn(async move {
                    let mut before = vec![DestinationStats::default(); destinations.len()];
                    let mut ticks = tokio::time::interval(STATS_INTERVAL);
                    ticks.tick().await;
                    loop {
                        ticks.tick().await;
                        for (destination, before) in destinations.iter().zip(&mut before) {
                            let now = destination.counters.read();
                            if now != *before {
                                log_stats(&name, protocol, &destination.shown, now, *before);
                                *before = now;
                            }
                        }
                    }
                });
            })
            .await;
    }

    async fn send_datagram(
        &self,
        destination: &Destination,
        payload: &[u8],
    ) -> Result<(), SinkError> {
        if payload.len() > MAX_DATAGRAM {
            return Err(SinkError::Network(format!(
                "a payload of {} bytes does not fit one UDP datagram ({MAX_DATAGRAM} bytes)",
                payload.len()
            )));
        }
        let mut datagrams = destination.datagrams.lock().await;
        let stale = datagrams
            .as_ref()
            .is_none_or(|current| current.resolved.elapsed() >= RESOLVE_INTERVAL);
        if stale {
            self.resolve_datagrams(destination, &mut datagrams).await?;
        }
        let Some(current) = datagrams.as_ref() else {
            return Err(SinkError::Network(format!(
                "{} is not resolved",
                destination.shown
            )));
        };
        if let Err(e) = current.socket.send_to(payload, current.target).await {
            // The address may no longer be the destination's: the name is
            // resolved again before the next payload.
            *datagrams = None;
            return Err(e.into());
        }
        Ok(())
    }

    /// Resolve a `udp` destination and make its socket. The first address the
    /// name resolves to is used. A socket is kept while the address stays the
    /// same.
    async fn resolve_datagrams(
        &self,
        destination: &Destination,
        datagrams: &mut Option<Datagrams>,
    ) -> std::io::Result<()> {
        let resolved = match destination.resolve().await {
            Ok(resolved) => resolved,
            // An address that worked is kept while the name cannot be looked up.
            Err(e) => {
                return match datagrams.as_mut() {
                    Some(current) => {
                        warn!(
                            "subscriber {}: cannot resolve {} again ({e}); still sending to {}",
                            self.name, destination.shown, current.target
                        );
                        current.resolved = tokio::time::Instant::now();
                        Ok(())
                    }
                    None => Err(e),
                }
            }
        };
        let target = resolved[0];
        if let Some(current) = datagrams.as_mut() {
            if current.target == target {
                current.resolved = tokio::time::Instant::now();
                return Ok(());
            }
            info!(
                "subscriber {}: {} now resolves to {target}, was {}",
                self.name, destination.shown, current.target
            );
        }
        // The socket in use is closed first: a fixed source port can be
        // bound once.
        *datagrams = None;
        let socket = UdpSocket::from_std(udp_socket(destination, target)?.into())?;
        info!(
            "subscriber {}: sending UDP to {} at {target} from port {}{}",
            self.name,
            destination.shown,
            socket.local_addr().map_or(0, |local| local.port()),
            destination.others(&resolved, target)
        );
        destination.set_address(target);
        *datagrams = Some(Datagrams {
            socket,
            target,
            resolved: tokio::time::Instant::now(),
        });
        Ok(())
    }

    /// A destination's queue; its connection task is started on first use.
    async fn queue_of<'a>(&self, destination: &'a Arc<Destination>) -> &'a mpsc::Sender<Vec<u8>> {
        destination
            .queue
            .get_or_init(|| async {
                let (sender, receiver) = mpsc::channel(self.queue);
                tokio::spawn(connection(
                    self.name.clone(),
                    Arc::clone(destination),
                    self.tls.clone(),
                    receiver,
                ));
                sender
            })
            .await
    }

    /// Put a payload on a destination's queue. A full queue drops the
    /// payload and counts it.
    async fn enqueue(&self, destination: &Arc<Destination>, payload: Vec<u8>) {
        if self.queue_of(destination).await.try_send(payload).is_ok() {
            destination.counters.queued.fetch_add(1, Ordering::Relaxed);
        } else {
            let dropped = destination.counters.dropped.fetch_add(1, Ordering::Relaxed) + 1;
            if dropped == 1 {
                warn!(
                    "subscriber {}: the queue for {} is full ({} payloads); dropping until it \
                     drains",
                    self.name, destination.shown, self.queue
                );
            }
        }
    }
}

fn log_stats(
    name: &str,
    protocol: Protocol,
    shown: &str,
    now: DestinationStats,
    before: DestinationStats,
) {
    let line = format!(
        "subscriber {name}: {} {shown}: {} sent, {} dropped with the queue full, {} failed, \
         {} reconnections",
        protocol.name(),
        now.sent,
        now.dropped,
        now.failed,
        now.reconnects
    );
    if now.dropped > before.dropped || now.failed > before.failed {
        warn!("{line}");
    } else {
        info!("{line}");
    }
}

/// Open a TCP connection to one address, from the destination's source port
/// when it has one.
async fn connect_to(
    destination: &Destination,
    target: SocketAddr,
) -> std::io::Result<tokio::net::TcpStream> {
    let socket = match target {
        SocketAddr::V4(_) => TcpSocket::new_v4()?,
        SocketAddr::V6(_) => TcpSocket::new_v6()?,
    };
    if let Some(port) = destination.source_port {
        // A connection that just closed leaves the port in a waiting state;
        // this lets the next one use it.
        socket.set_reuseaddr(true)?;
        let any: IpAddr = match target {
            SocketAddr::V4(_) => Ipv4Addr::UNSPECIFIED.into(),
            SocketAddr::V6(_) => Ipv6Addr::UNSPECIFIED.into(),
        };
        socket.bind(SocketAddr::new(any, port))?;
    }
    match tokio::time::timeout(CONNECT_TIMEOUT, socket.connect(target)).await {
        Ok(connected) => connected,
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("no answer within {CONNECT_TIMEOUT:?}"),
        )),
    }
}

/// Connect to a destination, inside TLS when the sink has it. A name that
/// resolves to several addresses is tried address by address, in the order
/// resolved, until one connects. Returns the stream and a description of the
/// address it reached.
async fn connect(
    destination: &Destination,
    tls: Option<&Tls>,
) -> std::io::Result<(Box<dyn Stream>, String)> {
    let resolved = destination.resolve().await?;
    let mut refusals = Vec::new();
    let mut connected = None;
    for target in &resolved {
        match connect_to(destination, *target).await {
            Ok(stream) => {
                connected = Some((stream, *target));
                break;
            }
            Err(e) => refusals.push(format!("{target}: {e}")),
        }
    }
    let Some((stream, target)) = connected else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            refusals.join("; "),
        ));
    };
    destination.set_address(target);
    let mut reached = format!("{target}{}", destination.others(&resolved, target));
    if !refusals.is_empty() {
        reached.push_str(&format!("; before it, {}", refusals.join("; ")));
    }
    stream.set_nodelay(true)?;
    let Some(tls) = tls else {
        return Ok((Box::new(stream), reached));
    };
    let name = tls
        .server_name
        .clone()
        .unwrap_or_else(|| destination.host.clone());
    let name = ServerName::try_from(name)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let handshake = tokio::time::timeout(CONNECT_TIMEOUT, tls.connector.connect(name, stream));
    match handshake.await {
        Ok(stream) => Ok((Box::new(stream?), reached)),
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("{target}: the TLS handshake did not finish within {CONNECT_TIMEOUT:?}"),
        )),
    }
}

/// Keep one connection to a destination and write its queue to it. Ends when
/// the sink is dropped.
async fn connection(
    name: String,
    destination: Arc<Destination>,
    tls: Option<Tls>,
    mut queue: mpsc::Receiver<Vec<u8>>,
) {
    let counters = &destination.counters;
    let shown = &destination.shown;
    let mut retry = FIRST_RETRY;
    let mut connected_before = false;
    let mut failures = 0u64;
    loop {
        let (mut stream, reached) = match connect(&destination, tls.as_ref()).await {
            Ok(connected) => connected,
            Err(e) => {
                let reason = e.to_string();
                failures += 1;
                // The first failure, then one line in ten.
                if failures == 1 || failures.is_multiple_of(10) {
                    warn!(
                        "subscriber {name}: cannot connect to {shown}: {reason}; attempt \
                         {failures}, next in {retry:?}"
                    );
                }
                if queue.is_closed() {
                    return;
                }
                tokio::time::sleep(retry).await;
                retry = (retry * 2).min(LONGEST_RETRY);
                continue;
            }
        };
        if connected_before {
            counters.reconnects.fetch_add(1, Ordering::Relaxed);
        }
        connected_before = true;
        failures = 0;
        retry = FIRST_RETRY;
        info!(
            "subscriber {name}: connected to {shown} at {reached}{}",
            if tls.is_some() { ", with TLS" } else { "" }
        );

        // What the far end sends is read and discarded: a TAK server streams
        // events to every client, and a full receive buffer would stall it.
        let mut incoming = [0u8; 4096];
        let lost = loop {
            tokio::select! {
                payload = queue.recv() => {
                    let Some(payload) = payload else {
                        let _ = stream.shutdown().await;
                        return;
                    };
                    let written = tokio::time::timeout(WRITE_TIMEOUT, async {
                        stream.write_all(&payload).await?;
                        stream.flush().await
                    });
                    match written.await {
                        Ok(Ok(())) => {
                            counters.sent.fetch_add(1, Ordering::Relaxed);
                        }
                        failed => {
                            counters.failed.fetch_add(1, Ordering::Relaxed);
                            break match failed {
                                Ok(Err(e)) => e.to_string(),
                                _ => format!("a write did not finish within {WRITE_TIMEOUT:?}"),
                            };
                        }
                    }
                }
                read = stream.read(&mut incoming) => {
                    match read {
                        Ok(0) => break "the far end closed the connection".to_string(),
                        Ok(_) => {}
                        Err(e) => break e.to_string(),
                    }
                }
            }
        };
        warn!("subscriber {name}: connection to {shown} lost: {lost}; reconnecting");
    }
}

#[async_trait]
impl Sink for NetworkSink {
    /// Start the connections of a `tcp` subscriber, so the first payload
    /// finds one open, and resolve the destinations of a `udp` subscriber.
    async fn prepare(&self) -> Result<(), SinkError> {
        self.start_stats().await;
        for destination in &self.destinations {
            match self.protocol {
                Protocol::Tcp => {
                    self.queue_of(destination).await;
                }
                // Resolved here so the address in use is in the log from the
                // start; a name that cannot be resolved yet is tried again
                // with the first payload.
                Protocol::Udp => {
                    let mut datagrams = destination.datagrams.lock().await;
                    if let Err(e) = self.resolve_datagrams(destination, &mut datagrams).await {
                        warn!(
                            "subscriber {}: cannot set up UDP to {}: {e}",
                            self.name, destination.shown
                        );
                    }
                }
            }
        }
        Ok(())
    }

    async fn send(&self, topic: &str, record: &str) -> Result<(), SinkError> {
        let payload = self.payload.bytes(record, self.protocol == Protocol::Tcp)?;
        let mut first_error = None;
        for destination in &self.destinations {
            match self.protocol {
                Protocol::Tcp => self.enqueue(destination, payload.clone()).await,
                Protocol::Udp => match self.send_datagram(destination, &payload).await {
                    Ok(()) => {
                        destination.counters.sent.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        destination.counters.failed.fetch_add(1, Ordering::Relaxed);
                        debug!(
                            "subscriber {}: payload on {topic} not sent to {}: {e}",
                            self.name, destination.shown
                        );
                        first_error.get_or_insert(SinkError::Network(format!(
                            "{}: {e}",
                            destination.shown
                        )));
                    }
                },
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Wait for the queues of a `tcp` subscriber to empty. What is left is
    /// the most any one destination still held.
    async fn finish(&self, within: Duration) -> u64 {
        let pending = || {
            self.destinations
                .iter()
                .map(|destination| {
                    let counters = &destination.counters;
                    counters.queued.load(Ordering::Relaxed).saturating_sub(
                        counters.sent.load(Ordering::Relaxed)
                            + counters.failed.load(Ordering::Relaxed),
                    )
                })
                .max()
                .unwrap_or(0)
        };
        if self.protocol == Protocol::Udp {
            return 0;
        }
        let deadline = tokio::time::Instant::now() + within;
        while pending() > 0 && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        pending()
    }

    /// Payloads a `tcp` destination dropped with its queue full or lost on a
    /// failed write; the most at any one destination. A `udp` failure is
    /// reported by `send` itself.
    fn lost(&self) -> u64 {
        if self.protocol == Protocol::Udp {
            return 0;
        }
        self.destinations
            .iter()
            .map(|destination| {
                let counters = &destination.counters;
                counters.dropped.load(Ordering::Relaxed) + counters.failed.load(Ordering::Relaxed)
            })
            .max()
            .unwrap_or(0)
    }
}
