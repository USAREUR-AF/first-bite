//! Subscribers: each reads the local ZeroMQ feed and hands the records on its
//! topics to one sink.

use crate::config::{load_subscriber_configs, SubscriberConfig};
use crate::error::{ConfigError, Error};
use crate::feed::{Feed, FeedConfig};
use crate::sink::{build_sink, Sink, SinkError};
use log::{error, info, warn};
use std::thread;
use tokio::runtime::{Builder, Handle};

/// Worker threads for sink I/O. Sinks spend their time waiting on the
/// network, so a few threads serve any number of subscribers.
const MAX_SINK_WORKERS: usize = 4;

/// After the first failure in a row, every this-many-th is logged.
const FAILURE_LOG_INTERVAL: u64 = 100;

struct Subscriber {
    feed: FeedConfig,
    sink: Box<dyn Sink>,
}

/// Every configured subscriber, with its sink built and checked.
pub struct Subscribers {
    subscribers: Vec<Subscriber>,
}

impl Subscribers {
    /// Build the subscribers of the configuration file. With no file there
    /// are none.
    pub fn load() -> Result<Self, Error> {
        Ok(Self::from_configs(&load_subscriber_configs()?)?)
    }

    /// Build one subscriber per entry. The first entry that cannot be built
    /// is the error.
    pub fn from_configs(configs: &[SubscriberConfig]) -> Result<Self, ConfigError> {
        let subscribers = configs
            .iter()
            .map(|config| {
                let feed =
                    FeedConfig::from_config(config).map_err(|problem| ConfigError::Subscriber {
                        subscriber: config.name.clone(),
                        problem,
                    })?;
                Ok(Subscriber {
                    feed,
                    sink: build_sink(config)?,
                })
            })
            .collect::<Result<Vec<_>, ConfigError>>()?;
        Ok(Subscribers { subscribers })
    }

    pub fn len(&self) -> usize {
        self.subscribers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.subscribers.is_empty()
    }

    /// Run every subscriber on its own thread. Returns at once when there are
    /// none; otherwise blocks for the life of the process.
    pub fn run(self) -> Result<(), Error> {
        if self.subscribers.is_empty() {
            return Ok(());
        }
        let workers = thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(MAX_SINK_WORKERS);
        let runtime = Builder::new_multi_thread()
            .worker_threads(workers)
            .thread_name("flow-sink")
            .enable_all()
            .build()
            .map_err(Error::Runtime)?;
        let context = zmq::Context::new();

        let mut threads = Vec::new();
        for Subscriber { feed, sink } in self.subscribers {
            let feed = feed.connect(&context)?;
            let handle = runtime.handle().clone();
            let name = format!("sub-{}", feed.name());
            let thread = thread::Builder::new()
                .name(name.clone())
                .spawn(move || forward(&feed, sink.as_ref(), &handle))
                .map_err(|source| Error::Thread { name, source })?;
            threads.push(thread);
        }
        info!("running {} subscriber(s)", threads.len());
        for thread in threads {
            if thread.join().is_err() {
                error!("a subscriber thread panicked");
            }
        }
        Ok(())
    }
}

/// Prepare the sink, then pass it every record the feed delivers. The socket
/// is already connected, so records that arrive during preparation wait in
/// its queue.
fn forward(feed: &Feed, sink: &dyn Sink, runtime: &Handle) {
    let name = feed.name();
    if let Err(e) = runtime.block_on(sink.prepare()) {
        error!("subscriber {name}: cannot prepare the sink: {e}");
    }
    let mut failures = Failures::default();
    while let Some((topic, record)) = feed.next() {
        match runtime.block_on(sink.send(&topic, &record)) {
            Ok(()) => failures.recovered(name),
            Err(e) => failures.failed(name, &topic, &e),
        }
    }
    info!("subscriber {name} stopped");
}

/// Keeps a sink that is down from writing one log line per record.
#[derive(Default)]
struct Failures {
    in_a_row: u64,
}

impl Failures {
    fn failed(&mut self, name: &str, topic: &str, error: &SinkError) {
        self.in_a_row += 1;
        if self.in_a_row == 1 {
            warn!("subscriber {name}: record on {topic} not delivered: {error}");
        } else if self.in_a_row.is_multiple_of(FAILURE_LOG_INTERVAL) {
            warn!(
                "subscriber {name}: {} records in a row not delivered; latest on {topic}: {error}",
                self.in_a_row
            );
        }
    }

    fn recovered(&mut self, name: &str) {
        if self.in_a_row > 0 {
            info!(
                "subscriber {name}: delivering again after {} failed record(s)",
                self.in_a_row
            );
            self.in_a_row = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{parse, PublisherConfig, SubscriberConfigs};
    use crate::publish::Publishers;
    use crate::test_support::{free_port, temp_path};
    use std::time::{Duration, Instant};

    fn configs(yaml: &str) -> Vec<SubscriberConfig> {
        parse::<SubscriberConfigs>(yaml, "test")
            .unwrap()
            .subscribers
    }

    #[test]
    fn no_subscribers_runs_nothing_and_returns() {
        let subscribers = Subscribers::from_configs(&[]).unwrap();
        assert!(subscribers.is_empty());
        subscribers.run().unwrap();
    }

    #[test]
    fn one_bad_entry_stops_startup_and_is_named() {
        let error = Subscribers::from_configs(&configs(
            "subscribers:\n  \
             - { name: Good, subscriber_type: stateful, zmq_url: \"127.0.0.1\", zmq_port: 12000, topics: [\"*\"] }\n  \
             - { name: Bad, subscriber_type: telegraph, zmq_url: \"127.0.0.1\", zmq_port: 12000, topics: [\"*\"] }\n",
        ))
        .err()
        .unwrap();
        assert_eq!(
            error.to_string(),
            "subscriber Bad: unknown subscriber_type \"telegraph\" with subtype unset"
        );
    }

    #[test]
    fn a_subscriber_needs_an_address_and_topics() {
        let error = |yaml: &str| {
            Subscribers::from_configs(&configs(yaml))
                .err()
                .unwrap()
                .to_string()
        };
        assert_eq!(
            error("subscribers:\n  - { name: A, subscriber_type: stateful, zmq_url: \"127.0.0.1\", topics: [\"*\"] }\n"),
            "subscriber A: `zmq_port` is required"
        );
        assert_eq!(
            error("subscribers:\n  - { name: A, subscriber_type: stateful, zmq_url: \"127.0.0.1\", zmq_port: 1, topics: [] }\n"),
            "subscriber A: `topics` is required"
        );
    }

    /// A publisher, and a subscriber that writes one topic to a local file:
    /// records on that topic reach the file, records on another do not.
    #[test]
    fn records_flow_from_publisher_to_sink() {
        let port = free_port();
        let log = temp_path("subscribe.log");
        let publishers = Publishers::bind(&[PublisherConfig {
            name: "records".into(),
            publisher_type: "PUB".into(),
            address: format!("tcp://127.0.0.1:{port}"),
            topic: "subsystem/message_type".into(),
            topic_suffix: None,
            encoding: None,
        }])
        .unwrap();
        let subscribers = Subscribers::from_configs(&configs(&format!(
            "subscribers:\n  - name: ToFile\n    subscriber_type: local\n    subtype: locallog\n    \
             zmq_url: \"127.0.0.1\"\n    zmq_port: {port}\n    local_logfile: \"{}\"\n    \
             topics: [\"WX/Heartbeat\"]\n",
            log.display()
        )))
        .unwrap();
        assert_eq!(subscribers.len(), 1);
        thread::spawn(move || subscribers.run());

        let wanted = r#"{"subsystem":"WX","message_type":"Heartbeat","message":{"uptime":"1"}}"#;
        let unwanted = r#"{"subsystem":"WX","message_type":"Observation","message":{}}"#;
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut written = String::new();
        while written.is_empty() && Instant::now() < deadline {
            publishers.publish(unwanted);
            publishers.publish(wanted);
            thread::sleep(Duration::from_millis(50));
            written = std::fs::read_to_string(&log).unwrap_or_default();
        }
        let lines: Vec<&str> = written.lines().collect();
        assert!(!lines.is_empty(), "nothing reached the file");
        assert!(lines.iter().all(|line| *line == wanted), "{written}");
        let _ = std::fs::remove_file(&log);
    }

    /// A publisher that sends CBOR, read by two subscribers: one stores JSON
    /// lines, one stores a CBOR sequence. Neither is told what the publisher
    /// sends.
    #[test]
    fn subscribers_read_a_cbor_publisher_and_write_their_own_encoding() {
        let port = free_port();
        let (lines, sequence) = (temp_path("from-cbor.log"), temp_path("from-cbor.cborseq"));
        let publishers = Publishers::bind(&[PublisherConfig {
            name: "records".into(),
            publisher_type: "PUB".into(),
            address: format!("tcp://127.0.0.1:{port}"),
            topic: "subsystem/message_type".into(),
            topic_suffix: None,
            encoding: Some("cbor".into()),
        }])
        .unwrap();
        let subscribers = Subscribers::from_configs(&configs(&format!(
            "subscribers:\n  \
             - {{ name: Lines, subscriber_type: local, zmq_url: \"127.0.0.1\", zmq_port: {port}, \
             local_logfile: \"{}\", topics: [\"WX/Heartbeat\"] }}\n  \
             - {{ name: Sequence, subscriber_type: local, zmq_url: \"127.0.0.1\", zmq_port: {port}, \
             local_logfile: \"{}\", topics: [\"*\"], encoding: cbor }}\n",
            lines.display(),
            sequence.display()
        )))
        .unwrap();
        thread::spawn(move || subscribers.run());

        let record = r#"{"subsystem":"WX","message_type":"Heartbeat","message":{"uptime":86400,"battery_low":false}}"#;
        let sent: serde_json::Value = serde_json::from_str(record).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let read = |path: &std::path::Path| std::fs::read(path).unwrap_or_default();
        while (read(&lines).is_empty() || read(&sequence).is_empty()) && Instant::now() < deadline {
            publishers.publish(record);
            thread::sleep(Duration::from_millis(50));
        }
        let written = String::from_utf8(read(&lines)).unwrap();
        let first: serde_json::Value =
            serde_json::from_str(written.lines().next().expect("no JSON line arrived")).unwrap();
        assert_eq!(first, sent);

        let stored = read(&sequence);
        let expected = crate::encoding::json_to_cbor(record).unwrap();
        assert!(!stored.is_empty(), "no CBOR item arrived");
        assert_eq!(stored.len() % expected.len(), 0);
        assert!(stored.chunks(expected.len()).all(|item| item == expected));
        let _ = std::fs::remove_file(&lines);
        let _ = std::fs::remove_file(&sequence);
    }
}
