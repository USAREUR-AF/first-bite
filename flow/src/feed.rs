//! The reading side of the local ZeroMQ feed: a SUB socket and the topics it
//! lets through.

use crate::config::SubscriberConfig;
use crate::encoding::payload_to_json;
use crate::error::{Error, Problem};
use crate::publish::TOPIC_DELIMITER;
use log::{debug, error, warn};
use std::time::Duration;

/// Pause after a failed receive, so a broken socket cannot spin a core.
const RECEIVE_RETRY: Duration = Duration::from_millis(100);

/// Largest message accepted from a publisher socket, in bytes. A record
/// carries one frame twice in base64 and its decoded fields, well under
/// 1 MiB, so nothing first-bite publishes comes near this. libzmq ends the
/// session with a peer that sends more and does not reconnect to it: whatever
/// is at the configured address cannot make this process hold a message of
/// any size, and a subscriber that met one forwards nothing more until the
/// process restarts.
pub(crate) const MAX_MESSAGE_BYTES: i64 = 16 * 1024 * 1024;

/// The topics a subscriber forwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TopicFilter {
    all: bool,
    topics: Vec<String>,
}

impl TopicFilter {
    /// `*` or `all` anywhere in the list forwards every topic.
    pub(crate) fn new(topics: &[String]) -> Self {
        TopicFilter {
            all: topics.iter().any(|topic| topic == "*" || topic == "all"),
            topics: topics.to_vec(),
        }
    }

    pub(crate) fn matches(&self, topic: &str) -> bool {
        self.all || self.topics.iter().any(|wanted| wanted == topic)
    }
}

/// Where a subscriber reads from and what it lets through; checked at startup,
/// connected when the subscriber runs.
#[derive(Debug, Clone)]
pub(crate) struct FeedConfig {
    pub(crate) name: String,
    pub(crate) endpoint: String,
    pub(crate) port: String,
    pub(crate) filter: TopicFilter,
}

impl FeedConfig {
    pub(crate) fn from_config(config: &SubscriberConfig) -> Result<Self, Problem> {
        let host = config
            .zmq_url
            .as_deref()
            .ok_or(Problem::MissingKey("zmq_url"))?;
        let port = config
            .zmq_port
            .as_deref()
            .ok_or(Problem::MissingKey("zmq_port"))?;
        if config.topics.is_empty() {
            return Err(Problem::MissingKey("topics"));
        }
        Ok(FeedConfig {
            name: config.name.clone(),
            endpoint: format!("tcp://{host}:{port}"),
            port: port.to_string(),
            filter: TopicFilter::new(&config.topics),
        })
    }

    /// Open the SUB socket and connect it.
    pub(crate) fn connect(self, context: &zmq::Context) -> Result<Feed, Error> {
        let describe = |action: &str| format!("subscriber {}: cannot {action}", self.name);
        let socket = context
            .socket(zmq::SUB)
            .map_err(Error::zmq(describe("create a socket")))?;
        socket
            .set_maxmsgsize(MAX_MESSAGE_BYTES)
            .map_err(Error::zmq(describe("limit the message size")))?;
        if self.filter.all {
            socket
                .set_subscribe(b"")
                .map_err(Error::zmq(describe("subscribe to every topic")))?;
        } else {
            for topic in &self.filter.topics {
                socket
                    .set_subscribe(topic.as_bytes())
                    .map_err(Error::zmq(describe(&format!("subscribe to {topic:?}"))))?;
            }
        }
        socket
            .connect(&self.endpoint)
            .map_err(Error::zmq(describe(&format!(
                "connect to {}",
                self.endpoint
            ))))?;
        debug!("subscriber {} connected to {}", self.name, self.endpoint);
        Ok(Feed {
            name: self.name,
            socket,
            filter: self.filter,
        })
    }
}

/// A connected SUB socket.
pub(crate) struct Feed {
    name: String,
    socket: zmq::Socket,
    filter: TopicFilter,
}

impl Feed {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Block until a message on a wanted topic arrives and return its topic
    /// and its payload as JSON text, whether the publisher sent JSON or
    /// CBOR. `None` means the ZeroMQ context was shut down.
    pub(crate) fn next(&self) -> Option<(String, String)> {
        loop {
            let message = match self.socket.recv_bytes(0) {
                Ok(message) => message,
                Err(zmq::Error::ETERM) => return None,
                Err(e) => {
                    error!("subscriber {}: receive failed: {e}", self.name);
                    std::thread::sleep(RECEIVE_RETRY);
                    continue;
                }
            };
            let Some((topic, payload)) = split_message(&message) else {
                warn!(
                    "subscriber {}: dropped a message with no topic delimiter or a topic that is not UTF-8",
                    self.name
                );
                continue;
            };
            // The socket filters by prefix; forward exact matches only.
            if !self.filter.matches(topic) {
                continue;
            }
            match payload_to_json(payload) {
                Ok(record) => return Some((topic.to_string(), record.into_owned())),
                Err(e) => warn!(
                    "subscriber {}: dropped a message on {topic}: {e}",
                    self.name
                ),
            }
        }
    }
}

/// The topic and the payload of a wire message.
fn split_message(message: &[u8]) -> Option<(&str, &[u8])> {
    let delimiter = message
        .iter()
        .position(|byte| *byte == TOPIC_DELIMITER as u8)?;
    let topic = std::str::from_utf8(&message[..delimiter]).ok()?;
    Some((topic, &message[delimiter + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn topics(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn listed_topics_match_exactly() {
        let filter = TopicFilter::new(&topics(&["WX/Observation", "WX/Heartbeat"]));
        assert!(filter.matches("WX/Observation"));
        assert!(!filter.matches("WX/Observation/extra"));
        assert!(!filter.matches("WX/Position"));
    }

    #[test]
    fn a_message_over_the_size_limit_is_not_received() {
        use crate::test_support::free_port;
        let port = free_port();
        let context = zmq::Context::new();
        let publisher = context.socket(zmq::PUB).unwrap();
        publisher.set_linger(0).unwrap();
        publisher.bind(&format!("tcp://127.0.0.1:{port}")).unwrap();
        let feed = FeedConfig {
            name: "test".to_string(),
            endpoint: format!("tcp://127.0.0.1:{port}"),
            port: port.to_string(),
            filter: TopicFilter::new(&topics(&["*"])),
        }
        .connect(&context)
        .unwrap();
        feed.socket.set_rcvtimeo(100).unwrap();
        let receive = |feed: &Feed| match feed.socket.recv_string(0) {
            Ok(Ok(message)) => Some(message.len()),
            _ => None,
        };

        // Wait for the subscription to take effect.
        let mut connected = false;
        for _ in 0..100 {
            publisher.send("t|small", 0).unwrap();
            if receive(&feed).is_some() {
                connected = true;
                break;
            }
        }
        assert!(connected, "no message arrived within ten seconds");

        let huge = format!("t|{}", "x".repeat(MAX_MESSAGE_BYTES as usize + 1));
        publisher.send(huge.as_str(), 0).unwrap();
        // libzmq ends the session with a peer that sends an oversized
        // message; what must hold is that the message itself never arrives.
        for _ in 0..20 {
            publisher.send("t|small", 0).unwrap();
            if let Some(length) = receive(&feed) {
                assert_eq!(length, "t|small".len(), "the oversized message arrived");
            }
        }
    }

    #[test]
    fn a_message_splits_at_the_first_delimiter() {
        assert_eq!(
            split_message(b"WX/Observation| {\"a\":\"x|y\"}"),
            Some(("WX/Observation", &b" {\"a\":\"x|y\"}"[..]))
        );
        // The payload need not be text.
        assert_eq!(
            split_message(&[b't', b'|', 0xa0, 0xff]),
            Some(("t", &[0xa0, 0xff][..]))
        );
        assert_eq!(split_message(b"no delimiter"), None);
        assert_eq!(split_message(&[0xff, b'|', b'{', b'}']), None);
    }

    #[test]
    fn star_or_all_matches_everything() {
        assert!(TopicFilter::new(&topics(&["*"])).matches("anything"));
        assert!(TopicFilter::new(&topics(&["WX/Heartbeat", "all"])).matches("anything"));
    }
}
