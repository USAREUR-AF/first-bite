//! The local ZeroMQ sockets that records are published on.
//!
//! A message on the wire is `<topic>|<payload>`. The delimiter is a pipe
//! because topics contain spaces and slashes.

use crate::config::{load_publisher_configs, PublisherConfig};
use crate::error::{ConfigError, Error, Problem};
use log::{debug, info, warn};
use serde::Deserialize;

pub(crate) const TOPIC_DELIMITER: char = '|';

/// Why one record was not published.
#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("record is not valid JSON: {0}")]
    Record(#[from] serde_json::Error),
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
        let rule = TopicRule::parse(&config.topic).ok_or_else(|| {
            invalid(
                "topic",
                format!(
                    "{:?} is not one of *, catchall, location, message_type, subsystem/message_type",
                    config.topic
                ),
            )
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
        if !binds_this_machine_only(&config.address) {
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
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Publish one record on the topic this publisher derives from it.
    pub fn publish(&self, record: &str) -> Result<(), PublishError> {
        let message = match self.rule {
            TopicRule::Catchall => format!("catchall{TOPIC_DELIMITER}{record}"),
            TopicRule::Location => {
                let fields: TopicFields = serde_json::from_str(record)?;
                let location = fields
                    .data_source_geo_location
                    .as_deref()
                    .unwrap_or("unknown_location");
                format!("{location}{TOPIC_DELIMITER}{record}")
            }
            TopicRule::MessageType => {
                let fields: TopicFields = serde_json::from_str(record)?;
                let message_type = fields
                    .message_type
                    .as_deref()
                    .unwrap_or("unknown_message_type");
                format!("{message_type}{TOPIC_DELIMITER}{record}")
            }
            TopicRule::SubsystemMessageType => {
                let fields: TopicFields = serde_json::from_str(record)?;
                let subsystem = fields.subsystem.as_deref().unwrap_or("unknown_subsystem");
                let message_type = fields
                    .message_type
                    .as_deref()
                    .unwrap_or("unknown_message_type");
                format!("{subsystem}/{message_type}{TOPIC_DELIMITER} {record}")
            }
        };
        debug!("publisher {} sending on {}", self.name, self.address);
        Ok(self.socket.send(&message, 0)?)
    }

    /// Publish `payload` on `topic`, extended by this publisher's
    /// `topic_suffix` when it has one.
    pub fn publish_on(&self, topic: &str, payload: &str) -> Result<(), PublishError> {
        let message = match &self.topic_suffix {
            Some(suffix) => format!("{topic}/{suffix}{TOPIC_DELIMITER}{payload}"),
            None => format!("{topic}{TOPIC_DELIMITER}{payload}"),
        };
        Ok(self.socket.send(&message, 0)?)
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

    /// Publish one record on every publisher. A publisher that fails is
    /// reported and the others still send.
    pub fn publish(&self, record: &str) {
        for publisher in &self.publishers {
            if let Err(e) = publisher.publish(record) {
                warn!("publisher {}: {e}", publisher.name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{free_port, receive};

    fn config(topic: &str, port: u16) -> PublisherConfig {
        PublisherConfig {
            name: "test".into(),
            publisher_type: "PUB".into(),
            address: format!("tcp://127.0.0.1:{port}"),
            topic: topic.into(),
            topic_suffix: None,
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
