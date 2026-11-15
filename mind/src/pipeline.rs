//! Turn a captured frame into the record that is published.

use crate::decap::{gre_inner, vxlan_inner};
use crate::source::{Frame, LinkType};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use bite::Dictionary;
use datamodels::packets::{get_packet_metadata, PacketInfoWithParsedMessage, PacketMetadata};
use etherparse::{IpNumber, NetSlice, SlicedPacket, TransportSlice};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A mirrored frame can itself carry a tunnel; unwrapping stops at this depth.
const MAX_TUNNEL_DEPTH: usize = 3;

/// Which tunnels the pipeline unwraps before it looks for a message.
#[derive(Debug, Clone, Default)]
pub struct Decapsulation {
    /// Leave GRE and ERSPAN packets as they are.
    pub keep_gre: bool,
    /// UDP destination ports that carry VXLAN. Empty: VXLAN is not unwrapped.
    pub vxlan_ports: Vec<u16>,
}

/// The transport packet found in a frame, after any tunnel was unwrapped.
struct Located<'a> {
    source_ip: Vec<u8>,
    destination_ip: Vec<u8>,
    source_port: u16,
    destination_port: u16,
    payload: &'a [u8],
}

/// Why a frame produced no record.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Skip {
    #[error("not a UDP or TCP packet")]
    NotTransport,
    #[error("malformed packet: {0}")]
    Malformed(String),
}

/// Decodes frames against one dictionary and stamps records with the
/// deployment's metadata.
pub struct Pipeline {
    dictionary: Option<Arc<Dictionary>>,
    metadata: PacketMetadata,
    decapsulation: Decapsulation,
    unwrapped: AtomicU64,
}

impl Pipeline {
    /// `dictionary` of `None` publishes every packet undecoded. The metadata is
    /// read from the environment once, here.
    pub fn new(dictionary: Option<Arc<Dictionary>>) -> Self {
        Pipeline {
            dictionary,
            metadata: get_packet_metadata(),
            decapsulation: Decapsulation::default(),
            unwrapped: AtomicU64::new(0),
        }
    }

    /// Choose which tunnels are unwrapped. The default unwraps GRE and ERSPAN
    /// and leaves VXLAN alone.
    pub fn with_decapsulation(mut self, decapsulation: Decapsulation) -> Self {
        self.decapsulation = decapsulation;
        self
    }

    /// Frames in which a tunnel was unwrapped since the pipeline was built.
    pub fn unwrapped(&self) -> u64 {
        self.unwrapped.load(Ordering::Relaxed)
    }

    /// Find the UDP or TCP packet a frame carries. A packet mirrored through
    /// GRE, ERSPAN or VXLAN is unwrapped first, so the addresses, ports and
    /// payload are those of the mirrored packet, not of the tunnel.
    fn locate<'a>(&self, link: LinkType, data: &'a [u8]) -> Result<Located<'a>, Skip> {
        let (mut link, mut data) = (link, data);
        for depth in 0..=MAX_TUNNEL_DEPTH {
            let sliced = match link {
                LinkType::Ethernet => SlicedPacket::from_ethernet(data),
                LinkType::RawIp => SlicedPacket::from_ip(data),
            }
            .map_err(|e| Skip::Malformed(e.to_string()))?;

            let (source_ip, destination_ip, ip_payload) = match &sliced.net {
                Some(NetSlice::Ipv4(ip)) => (
                    ip.header().source().to_vec(),
                    ip.header().destination().to_vec(),
                    ip.payload(),
                ),
                Some(NetSlice::Ipv6(ip)) => (
                    ip.header().source().to_vec(),
                    ip.header().destination().to_vec(),
                    ip.payload(),
                ),
                _ => return Err(Skip::NotTransport),
            };

            let inner = match &sliced.transport {
                Some(TransportSlice::Udp(udp)) => {
                    let tunnelled = self
                        .decapsulation
                        .vxlan_ports
                        .contains(&udp.destination_port())
                        .then(|| vxlan_inner(udp.payload()))
                        .flatten();
                    match tunnelled {
                        Some(inner) => inner,
                        None => {
                            return Ok(Located {
                                source_ip,
                                destination_ip,
                                source_port: udp.source_port(),
                                destination_port: udp.destination_port(),
                                payload: udp.payload(),
                            })
                        }
                    }
                }
                Some(TransportSlice::Tcp(tcp)) => {
                    return Ok(Located {
                        source_ip,
                        destination_ip,
                        source_port: tcp.source_port(),
                        destination_port: tcp.destination_port(),
                        payload: tcp.payload(),
                    })
                }
                _ if ip_payload.ip_number == IpNumber::GRE
                    && !ip_payload.fragmented
                    && !self.decapsulation.keep_gre =>
                {
                    gre_inner(ip_payload.payload).ok_or(Skip::NotTransport)?
                }
                _ => return Err(Skip::NotTransport),
            };
            if depth == MAX_TUNNEL_DEPTH {
                break;
            }
            if depth == 0 {
                self.unwrapped.fetch_add(1, Ordering::Relaxed);
            }
            (link, data) = (inner.1, inner.2);
        }
        Err(Skip::NotTransport)
    }

    /// Build the record for one frame.
    ///
    /// Every UDP and TCP packet yields a record. One whose port belongs to an
    /// interface in the dictionary also carries the interface name, the message
    /// type and the decoded fields. For a packet mirrored through a tunnel the
    /// record describes the mirrored packet; `data_encoded_full` keeps the
    /// frame as captured, tunnel included.
    pub fn process(&self, frame: &Frame<'_>) -> Result<PacketInfoWithParsedMessage, Skip> {
        let Located {
            source_ip,
            destination_ip,
            source_port,
            destination_port,
            payload,
        } = self.locate(frame.link, frame.data)?;

        let interface = self
            .dictionary
            .as_deref()
            .and_then(|dictionary| dictionary.interface_for_ports(source_port, destination_port));
        let decoded = interface.and_then(|interface| interface.decode(payload).ok());

        Ok(PacketInfoWithParsedMessage {
            timestamp: frame.timestamp,
            src_port: source_port.to_string(),
            dst_port: destination_port.to_string(),
            src_ip: source_ip,
            dst_ip: destination_ip,
            data_encoded_inner: STANDARD.encode(payload),
            subsystem: interface.map(|interface| interface.name.clone()),
            message_type: decoded.as_ref().map(|decoded| decoded.message_type()),
            message: decoded
                .as_ref()
                .and_then(|decoded| serde_json::to_value(decoded.rendered()).ok()),
            data_encoded_full: STANDARD.encode(frame.data),
            data_source_geo_location: self.metadata.data_source_geo_location.clone(),
            data_source_network_name: self.metadata.data_source_network_name.clone(),
            data_source_system_type: self.metadata.data_source_system_type.clone(),
            data_source_owner: self.metadata.data_source_owner.clone(),
            data_packet_classification: self.metadata.data_packet_classification.clone(),
            data_packet_accm: self.metadata.data_packet_accm.clone(),
            data_model_version: self.metadata.data_model_version.clone(),
            application_version: self.metadata.application_version.clone(),
        })
    }
}
