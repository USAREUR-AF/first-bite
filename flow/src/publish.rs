//! The local ZeroMQ sockets that records are published on.
//!
//! A message on the wire is `<topic>|<payload>`. The delimiter is a pipe
//! because topics contain spaces and slashes. The payload is the record as
//! JSON text or, for a publisher with `encoding: cbor`, as one CBOR item.

use crate::config::{load_publisher_configs, PublisherConfig};
use crate::encoding::{Encoding, EncodingError};
use crate::error::{ConfigError, Error, Problem};
use log::{debug, info, warn};
use serde::Deserialize;

pub(crate) const TOPIC_DELIMITER: char = '|';

/// Why one record was not published.
#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("record is not valid JSON: {0}")]
    Record(#[from] serde_json::Error),
    #[error("{0}")]
    Encoding(#[from] EncodingError),
    #[error("ZeroMQ send failed: {0}")]
    Zmq(#[from] zmq::Error),
}

/// How a publisher forms the topic of each record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TopicRule {
    /// Every record goes out on the topic `catchall`.
    Catchall,
    /// The record's `data_source_geo_location`.
    Location,
    /// The record's `message_type`.
    MessageType,
    /// `<subsystem>/<message_type>`.
    SubsystemMessageType,
}

impl TopicRule {
    fn parse(topic: &str) -> Option<Self> {
        match topic {
            "*" | "catchall" => Some(TopicRule::Catchall),
            "location" => Some(TopicRule::Location),
            "message_type" => Some(TopicRule::MessageType),
            "subsystem/message_type" => Some(TopicRule::SubsystemMessageType),
            _ => None,
        }
    }
}

/// The parts of a record that topics are formed from.
#[derive(Deserialize)]
struct TopicFields {
    #[serde(default)]
    subsystem: Option<String>,
    #[serde(default)]
    message_type: Option<String>,
    #[serde(default)]
    data_source_geo_location: Option<String>,
}

/// One bound ZeroMQ socket.
pub struct Publisher {
    name: String,
    address: String,
    socket: zmq::Socket,
    rule: TopicRule,
    topic_suffix: Option<String>,
    encoding: Encoding,
}

/// `<topic>|<payload>`.
fn wire_message(topic: &str, payload: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(topic.len() + 1 + payload.len());
    message.extend_from_slice(topic.as_bytes());
    message.push(TOPIC_DELIMITER as u8);
    message.extend_from_slice(payload);
    message
}

impl Publisher {
    /// Create the socket and bind it.
    pub fn bind(context: &zmq::Context, config: &PublisherConfig) -> Result<Self, Error> {
        let invalid = |key, reason: String| ConfigError::Publisher {
            publisher: config.name.clone(),
            problem: Problem::InvalidValue { key, reason },
        };
        let socket_type = match config.publisher_type.as_str() {
            "PUB" => zmq::PUB,
            "PUSH" => zmq::PUSH,
            other => {
                return Err(invalid(
                    "publisher_type",
                    format!("{other:?} is neither PUB nor PUSH"),
                )
                .into())
            }
        };
        let encoding = Encoding::from_config(config.encoding.as_deref()).map_err(|problem| {
            ConfigError::Publisher {
                publisher: config.name.clone(),
                problem,
            }
        })?;
        let rule = TopicRule::parse(&config.topic).ok_or_else(|| {
            invalid(
                "topic",
                format!(
                    "{:?} is not one of *, catchall, location, message_type, subsystem/message_type",
                    config.topic
                ),
            )
        })?;

        // A socket any host on a network could reach is refused before it binds.
        crate::security::permit_bind(config, &crate::security::current()).map_err(|problem| {
            ConfigError::Publisher {
                publisher: config.name.clone(),
                problem,
            }
        })?;

        let describe = |action: &str| {
            format!(
                "publisher {}: cannot {action} {}",
                config.name, config.address
            )
        };
        let socket = context
            .socket(socket_type)
            .map_err(Error::zmq(describe("create a socket for")))?;
        socket
            .set_linger(0)
            .map_err(Error::zmq(describe("configure the socket for")))?;
        socket
            .bind(&config.address)
            .map_err(Error::zmq(describe("bind")))?;
        info!("publisher {} bound to {}", config.name, config.address);
        // Under a security block the warning that names the override was
        // logged when the address was permitted.
        if !binds_this_machine_only(&config.address) && !crate::security::current().stated {
            warn!(
                "publisher {}: {} is reachable from other machines; the socket has no \
                 authentication and no encryption, so any host that can connect reads every \
                 record",
                config.name, config.address
            );
        }

        Ok(Publisher {
            name: config.name.clone(),
            address: config.address.clone(),
            socket,
            rule,
            topic_suffix: config.topic_suffix.clone(),
            encoding,
        })
    }

    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Publish one record, given as JSON text, on the topic this publisher
    /// derives from it.
    pub fn publish(&self, record: &str) -> Result<(), PublishError> {
        self.publish_encoded(record, &self.encoding.encode(record)?)
    }

    /// Publish a record whose bytes in this publisher's encoding are already
    /// at hand. `record` is the same record as JSON text; the topic is read
    /// from it.
    fn publish_encoded(&self, record: &str, payload: &[u8]) -> Result<(), PublishError> {
        let topic = match self.rule {
            TopicRule::Catchall => "catchall".to_string(),
            TopicRule::Location => {
                let fields: TopicFields = serde_json::from_str(record)?;
                fields
                    .data_source_geo_location
                    .unwrap_or_else(|| "unknown_location".to_string())
            }
            TopicRule::MessageType => {
                let fields: TopicFields = serde_json::from_str(record)?;
                fields
                    .message_type
                    .unwrap_or_else(|| "unknown_message_type".to_string())
            }
            TopicRule::SubsystemMessageType => {
                let fields: TopicFields = serde_json::from_str(record)?;
                let subsystem = fields.subsystem.as_deref().unwrap_or("unknown_subsystem");
                let message_type = fields
                    .message_type
                    .as_deref()
                    .unwrap_or("unknown_message_type");
                format!("{subsystem}/{message_type}")
            }
        };
        let mut message = Vec::with_capacity(topic.len() + 2 + payload.len());
        message.extend_from_slice(topic.as_bytes());
        message.push(TOPIC_DELIMITER as u8);
        // JSON on a subsystem/message_type topic has always followed the
        // delimiter after one space. A CBOR item begins at the delimiter.
        if self.rule == TopicRule::SubsystemMessageType && self.encoding == Encoding::Json {
            message.push(b' ');
        }
        message.extend_from_slice(payload);
        debug!("publisher {} sending on {}", self.name, self.address);
        Ok(self.socket.send(message, 0)?)
    }

    /// Publish `payload`, a JSON document, on `topic`, extended by this
    /// publisher's `topic_suffix` when it has one.
    pub fn publish_on(&self, topic: &str, payload: &str) -> Result<(), PublishError> {
        let payload = self.encoding.encode(payload)?;
        let message = match &self.topic_suffix {
            Some(suffix) => wire_message(&format!("{topic}/{suffix}"), &payload),
            None => wire_message(topic, &payload),
        };
        Ok(self.socket.send(message, 0)?)
    }
}

/// Whether a socket bound to `address` can be reached only from this
/// machine: a TCP address on the loopback network or `localhost`, or a
/// transport that never leaves the host.
fn binds_this_machine_only(address: &str) -> bool {
    let Some(rest) = address.strip_prefix("tcp://") else {
        return address.starts_with("ipc://") || address.starts_with("inproc://");
    };
    let host = rest.rsplit_once(':').map_or(rest, |(host, _)| host);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Every configured publisher. The owner publishes records through it; the
/// sockets close when it is dropped.
pub struct Publishers {
    publishers: Vec<Publisher>,
}

impl Publishers {
    /// Bind the publishers of the configuration file, or the built-in
    /// publisher when there is no file.
    pub fn load() -> Result<Self, Error> {
        crate::security::activate()?;
        Self::bind(&load_publisher_configs()?)
    }

    /// Bind one socket per configuration entry.
    pub fn bind(configs: &[PublisherConfig]) -> Result<Self, Error> {
        let context = zmq::Context::new();
        let publishers = configs
            .iter()
            .map(|config| Publisher::bind(&context, config))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Publishers { publishers })
    }

    pub fn len(&self) -> usize {
        self.publishers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.publishers.is_empty()
    }

    /// Publish one record, given as JSON text, on every publisher. A
    /// publisher that fails is reported and the others still send. The record
    /// is transcoded once, however many publishers want CBOR.
    pub fn publish(&self, record: &str) {
        let mut cbor: Option<Result<Vec<u8>, String>> = None;
        for publisher in &self.publishers {
            let sent = match publisher.encoding {
                Encoding::Json => publisher.publish_encoded(record, record.as_bytes()),
                Encoding::Cbor => {
                    let cbor = cbor.get_or_insert_with(|| {
                        Encoding::Cbor
                            .encode(record)
                            .map(|bytes| bytes.into_owned())
                            .map_err(|e| e.to_string())
                    });
                    match cbor {
                        Ok(bytes) => publisher.publish_encoded(record, bytes),
                        Err(e) => {
                            warn!("publisher {}: {e}", publisher.name);
                            continue;
                        }
                    }
                }
            };
            if let Err(e) = sent {
                warn!("publisher {}: {e}", publisher.name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{free_port, receive, receive_bytes};

    fn config(topic: &str, port: u16) -> PublisherConfig {
        PublisherConfig {
            name: "test".into(),
            publisher_type: "PUB".into(),
            address: format!("tcp://127.0.0.1:{port}"),
            topic: topic.into(),
            topic_suffix: None,
            encoding: None,
        }
    }

    const RECORD: &str = r#"{"subsystem":"WX","message_type":"Observation","data_source_geo_location":"hilltop","message":{}}"#;

    /// Bind a publisher with `topic`, connect a SUB socket, publish `record`
    /// and return what arrived.
    fn round_trip(topic: &str, record: &str) -> String {
        let port = free_port();
        let publishers = Publishers::bind(&[config(topic, port)]).unwrap();
        let context = zmq::Context::new();
        let subscriber = context.socket(zmq::SUB).unwrap();
        subscriber.set_subscribe(b"").unwrap();
        subscriber
            .connect(&format!("tcp://127.0.0.1:{port}"))
            .unwrap();
        receive(&subscriber, || publishers.publish(record))
    }

    #[test]
    fn topic_rules_form_the_wire_message() {
        assert_eq!(round_trip("*", RECORD), format!("catchall|{RECORD}"));
        assert_eq!(round_trip("location", RECORD), format!("hilltop|{RECORD}"));
        assert_eq!(
            round_trip("message_type", RECORD),
            format!("Observation|{RECORD}")
        );
        assert_eq!(
            round_trip("subsystem/message_type", RECORD),
            format!("WX/Observation| {RECORD}")
        );
    }

    #[test]
    fn a_cbor_publisher_sends_the_record_as_one_cbor_item() {
        use crate::encoding::{cbor_to_json, json_to_cbor};
        for (topic, prefix) in [
            ("*", "catchall|"),
            ("message_type", "Observation|"),
            ("subsystem/message_type", "WX/Observation|"),
        ] {
            let port = free_port();
            let mut config = config(topic, port);
            config.encoding = Some("cbor".into());
            let publishers = Publishers::bind(&[config]).unwrap();
            let context = zmq::Context::new();
            let subscriber = context.socket(zmq::SUB).unwrap();
            subscriber.set_subscribe(b"").unwrap();
            subscriber
                .connect(&format!("tcp://127.0.0.1:{port}"))
                .unwrap();
            let message = receive_bytes(&subscriber, || publishers.publish(RECORD));
            let payload = message.strip_prefix(prefix.as_bytes()).unwrap();
            assert_eq!(payload, json_to_cbor(RECORD).unwrap());
            let back: serde_json::Value =
                serde_json::from_str(&cbor_to_json(payload).unwrap()).unwrap();
            let sent: serde_json::Value = serde_json::from_str(RECORD).unwrap();
            assert_eq!(back, sent);
        }
    }

    #[test]
    fn a_transform_output_is_published_in_the_publishers_encoding() {
        let port = free_port();
        let context = zmq::Context::new();
        let mut config = config("subsystem/message_type", port);
        config.topic_suffix = Some("CoT".into());
        config.encoding = Some("cbor".into());
        let publisher = Publisher::bind(&context, &config).unwrap();
        let subscriber = context.socket(zmq::SUB).unwrap();
        subscriber.set_subscribe(b"").unwrap();
        subscriber
            .connect(&format!("tcp://127.0.0.1:{port}"))
            .unwrap();
        let output = r#"{"cot_datetime":"t"}"#;
        let message = receive_bytes(&subscriber, || {
            publisher.publish_on("WX/Position", output).unwrap()
        });
        let payload = message.strip_prefix(b"WX/Position/CoT|").unwrap();
        assert_eq!(payload, crate::encoding::json_to_cbor(output).unwrap());
    }

    #[test]
    fn an_unknown_encoding_stops_startup() {
        let mut config = config("*", free_port());
        config.encoding = Some("protobuf".into());
        let error = Publishers::bind(&[config]).err().unwrap();
        assert_eq!(
            error.to_string(),
            "publisher test: `encoding`: \"protobuf\" is not one of json, cbor"
        );
    }

    #[test]
    fn undecoded_records_go_out_on_placeholder_topics() {
        let record = r#"{"subsystem":null,"message_type":null}"#;
        assert_eq!(
            round_trip("subsystem/message_type", record),
            format!("unknown_subsystem/unknown_message_type| {record}")
        );
        assert_eq!(
            round_trip("location", record),
            format!("unknown_location|{record}")
        );
    }

    #[test]
    fn unknown_topic_rule_and_socket_type_stop_startup() {
        let error = Publishers::bind(&[config("by_colour", free_port())])
            .err()
            .unwrap();
        assert!(
            error.to_string().starts_with("publisher test: `topic`: "),
            "{error}"
        );
        let mut wrong_type = config("*", free_port());
        wrong_type.publisher_type = "ROUTER".into();
        let error = Publishers::bind(&[wrong_type]).err().unwrap();
        assert!(error.to_string().contains("`publisher_type`"), "{error}");
    }

    #[test]
    fn only_loopback_and_local_transports_stay_on_this_machine() {
        for local in [
            "tcp://127.0.0.1:12000",
            "tcp://127.8.9.10:12000",
            "tcp://localhost:12000",
            "tcp://[::1]:12000",
            "ipc:///run/first-bite/records",
            "inproc://records",
        ] {
            assert!(binds_this_machine_only(local), "{local}");
        }
        for exposed in [
            "tcp://*:12000",
            "tcp://0.0.0.0:12000",
            "tcp://[::]:12000",
            "tcp://192.0.2.7:12000",
            "tcp://eth0:12000",
            "tcp://records.example:12000",
            "pgm://192.0.2.7;239.192.1.1:5555",
        ] {
            assert!(!binds_this_machine_only(exposed), "{exposed}");
        }
    }

    #[test]
    fn a_port_in_use_stops_startup() {
        let port = free_port();
        let _first = Publishers::bind(&[config("*", port)]).unwrap();
        let error = Publishers::bind(&[config("*", port)]).err().unwrap();
        assert!(
            error
                .to_string()
                .starts_with("publisher test: cannot bind tcp://127.0.0.1:"),
            "{error}"
        );
    }
}
