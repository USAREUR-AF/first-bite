//! Turn a captured frame into the record that is published.

use crate::source::{Frame, LinkType};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use bite::Dictionary;
use datamodels::packets::{get_packet_metadata, PacketInfoWithParsedMessage, PacketMetadata};
use etherparse::{NetSlice, SlicedPacket, TransportSlice};
use std::sync::Arc;

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
}

impl Pipeline {
    /// `dictionary` of `None` publishes every packet undecoded. The metadata is
    /// read from the environment once, here.
    pub fn new(dictionary: Option<Arc<Dictionary>>) -> Self {
        Pipeline {
            dictionary,
            metadata: get_packet_metadata(),
        }
    }

    /// Build the record for one frame.
    ///
    /// Every UDP and TCP packet yields a record. One whose port belongs to an
    /// interface in the dictionary also carries the interface name, the message
    /// type and the decoded fields.
    pub fn process(&self, frame: &Frame<'_>) -> Result<PacketInfoWithParsedMessage, Skip> {
        let sliced = match frame.link {
            LinkType::Ethernet => SlicedPacket::from_ethernet(frame.data),
            LinkType::RawIp => SlicedPacket::from_ip(frame.data),
        }
        .map_err(|e| Skip::Malformed(e.to_string()))?;

        let (source_port, destination_port, payload) = match &sliced.transport {
            Some(TransportSlice::Udp(udp)) => {
                (udp.source_port(), udp.destination_port(), udp.payload())
            }
            Some(TransportSlice::Tcp(tcp)) => {
                (tcp.source_port(), tcp.destination_port(), tcp.payload())
            }
            _ => return Err(Skip::NotTransport),
        };
        let (source_ip, destination_ip) = match &sliced.net {
            Some(NetSlice::Ipv4(ip)) => (
                ip.header().source().to_vec(),
                ip.header().destination().to_vec(),
            ),
            Some(NetSlice::Ipv6(ip)) => (
                ip.header().source().to_vec(),
                ip.header().destination().to_vec(),
            ),
            _ => return Err(Skip::NotTransport),
        };

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
