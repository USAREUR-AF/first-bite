//! Subpubs: read records from the local feed, transform each one, and publish
//! the result on a second socket.
//!
//! The output goes out on the topic the input arrived on, extended by the
//! publisher's `topic_suffix`. The original stream is left as it was; a
//! transform adds a stream, it never replaces one.

use crate::config::{load_subpub_configs, SubPubConfig, SubscriberConfig};
use crate::error::{ConfigError, Error, Problem};
use crate::feed::{Feed, FeedConfig};
use crate::publish::Publisher;
use log::{error, info, warn};
use std::thread;

/// What a subpub does to each record.
#[derive(Debug, Clone)]
pub enum Transform {
    /// Build a Cursor-on-Target event from the fields a mapping names.
    Cot(cotify::Mapping),
}

/// Why one record was not transformed.
#[derive(Debug, thiserror::Error)]
pub enum TransformError {
    #[error("Cursor-on-Target: {0}")]
    Cot(#[from] cotify::CotError),
}

impl Transform {
    /// The transform a subpub's subscriber asks for: `subscriber_type:
    /// "transformer"` with `subtype: "cot"` and a `cot:` mapping.
    pub fn from_config(config: &SubscriberConfig) -> Result<Self, Problem> {
        match (config.subscriber_type.as_str(), config.subtype.as_deref()) {
            ("transformer", Some("cot")) => config
                .cot
                .clone()
                .map(Transform::Cot)
                .ok_or(Problem::MissingKey("cot")),
            _ => Err(Problem::UnknownTransform {
                subscriber_type: config.subscriber_type.clone(),
                subtype: config.subtype.clone(),
            }),
        }
    }

    pub fn apply(&self, record: &str) -> Result<String, TransformError> {
        match self {
            Transform::Cot(mapping) => Ok(cotify::record_to_cot(mapping, record)?),
        }
    }
}

struct SubPub {
    feed: FeedConfig,
    transform: Transform,
    publisher: Publisher,
}

/// Every configured subpub, with its transform checked and its output socket
/// bound.
pub struct SubPubs {
    subpubs: Vec<SubPub>,
}

impl SubPubs {
    /// Build the subpubs of the configuration file. With no file there are
    /// none.
    pub fn load() -> Result<Self, Error> {
        Self::from_configs(&load_subpub_configs()?)
    }

    /// Check every entry, then bind the output sockets.
    pub fn from_configs(configs: &[SubPubConfig]) -> Result<Self, Error> {
        let checked = configs
            .iter()
            .map(|config| {
                check(config).map_err(|problem| ConfigError::SubPub {
                    subpub: config.name.clone(),
                    problem,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let context = zmq::Context::new();
        let subpubs = configs
            .iter()
            .zip(checked)
            .map(|(config, (feed, transform))| {
                Ok(SubPub {
                    feed,
                    transform,
                    publisher: Publisher::bind(&context, &config.publisher)?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(SubPubs { subpubs })
    }

    pub fn len(&self) -> usize {
        self.subpubs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.subpubs.is_empty()
    }

    /// Run every subpub on its own thread. Returns at once when there are
    /// none; otherwise blocks for the life of the process.
    pub fn run(self) -> Result<(), Error> {
        if self.subpubs.is_empty() {
            return Ok(());
        }
        let context = zmq::Context::new();
        let mut threads = Vec::new();
        for subpub in self.subpubs {
            let feed = subpub.feed.connect(&context)?;
            let name = format!("subpub-{}", feed.name());
            let (transform, publisher) = (subpub.transform, subpub.publisher);
            let thread = thread::Builder::new()
                .name(name.clone())
                .spawn(move || republish(&feed, &transform, &publisher))
                .map_err(|source| Error::Thread { name, source })?;
            threads.push(thread);
        }
        info!("running {} subpub(s)", threads.len());
        for thread in threads {
            if thread.join().is_err() {
                error!("a subpub thread panicked");
            }
        }
        Ok(())
    }
}

/// The feed and transform of one entry. A publisher on the port the
/// subscriber reads would feed its own output back in, and is refused.
fn check(config: &SubPubConfig) -> Result<(FeedConfig, Transform), Problem> {
    let transform = Transform::from_config(&config.subscriber)?;
    let mut feed = FeedConfig::from_config(&config.subscriber)?;
    feed.name = config.name.clone();
    let publisher_port = config.publisher.address.rsplit(':').next().unwrap_or("");
    if publisher_port == feed.port {
        return Err(Problem::Loopback(feed.port));
    }
    Ok((feed, transform))
}

fn republish(feed: &Feed, transform: &Transform, publisher: &Publisher) {
    let name = feed.name();
    while let Some((topic, record)) = feed.next() {
        match transform.apply(&record) {
            Ok(output) => {
                if let Err(e) = publisher.publish_on(&topic, &output) {
                    warn!("subpub {name}: {e}");
                }
            }
            Err(e) => warn!("subpub {name}: record on {topic} not transformed: {e}"),
        }
    }
    info!("subpub {name} stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{parse, PublisherConfig, SubPubConfigs};
    use crate::publish::Publishers;
    use crate::test_support::{free_port, receive};

    fn subpubs(input_port: u16, output_port: u16, subscriber: &str) -> Vec<SubPubConfig> {
        let yaml = format!(
            "subpubs:\n  - name: Position2CoT\n    subscriber:\n      name: positions\n      \
             zmq_url: \"127.0.0.1\"\n      zmq_port: {input_port}\n      topics: [\"WX/Position\"]\n\
             {subscriber}    publisher:\n      name: cot\n      publisher_type: PUB\n      \
             address: \"tcp://127.0.0.1:{output_port}\"\n      topic: \"subsystem/message_type\"\n      \
             topic_suffix: CoT\n"
        );
        parse::<SubPubConfigs>(&yaml, "test").unwrap().subpubs
    }

    const COT: &str =
        "      subscriber_type: transformer\n      subtype: cot\n      cot:\n        \
        uid: [station_id]\n        type: { value: a-f-G }\n        \
        position: { geodetic: [latitude, longitude, altitude] }\n        stale_minutes: 60\n";

    fn refusal(configs: &[SubPubConfig]) -> String {
        SubPubs::from_configs(configs).err().unwrap().to_string()
    }

    #[test]
    fn no_subpubs_runs_nothing_and_returns() {
        let subpubs = SubPubs::from_configs(&[]).unwrap();
        assert!(subpubs.is_empty());
        subpubs.run().unwrap();
    }

    #[test]
    fn unknown_transforms_name_the_subpub() {
        let error = refusal(&subpubs(
            free_port(),
            free_port(),
            "      subscriber_type: transformer\n      subtype: morse\n",
        ));
        assert!(
            error.starts_with(
                "subpub Position2CoT: unknown transform: subscriber_type \"transformer\" with subtype \"morse\""
            ),
            "{error}"
        );
    }

    #[test]
    fn the_cot_transform_needs_a_mapping() {
        assert_eq!(
            refusal(&subpubs(
                free_port(),
                free_port(),
                "      subscriber_type: transformer\n      subtype: cot\n",
            )),
            "subpub Position2CoT: `cot` is required"
        );
    }

    #[test]
    fn output_on_the_input_port_is_refused() {
        let port = free_port();
        let error = refusal(&subpubs(port, port, COT));
        assert!(
            error.starts_with(&format!(
                "subpub Position2CoT: the publisher binds port {port}"
            )),
            "{error}"
        );
    }

    /// Records published on the input socket come out of the output socket as
    /// CoT, on the input topic with the suffix.
    #[test]
    fn position_records_are_republished_as_cot() {
        let (input_port, output_port) = (free_port(), free_port());
        let publishers = Publishers::bind(&[PublisherConfig {
            name: "records".into(),
            publisher_type: "PUB".into(),
            address: format!("tcp://127.0.0.1:{input_port}"),
            topic: "subsystem/message_type".into(),
            topic_suffix: None,
            encoding: None,
        }])
        .unwrap();
        let subpubs = SubPubs::from_configs(&subpubs(input_port, output_port, COT)).unwrap();
        assert_eq!(subpubs.len(), 1);
        thread::spawn(move || subpubs.run());

        let context = zmq::Context::new();
        let output = context.socket(zmq::SUB).unwrap();
        output.set_subscribe(b"").unwrap();
        output
            .connect(&format!("tcp://127.0.0.1:{output_port}"))
            .unwrap();

        let record = r#"{"subsystem":"WX","message_type":"Position","message":{"station_id":"KX7","latitude":"64.8378","longitude":"-147.7164","altitude":"136.5"}}"#;
        let message = receive(&output, || publishers.publish(record));
        let (topic, payload) = message.split_once('|').unwrap();
        assert_eq!(topic, "WX/Position/CoT");
        let event: serde_json::Value = serde_json::from_str(payload).unwrap();
        let xml = event["cot_xml_string"].as_str().unwrap();
        assert!(
            xml.starts_with(r#"<event version="2.0" uid="KX7" type="a-f-G" time=""#),
            "{xml}"
        );
        assert!(
            xml.ends_with(
                r#" how="m-g"><point lat="64.8378" lon="-147.7164" hae="136.5" ce="9999999" le="9999999"/><detail/></event>"#
            ),
            "{xml}"
        );
    }
}
