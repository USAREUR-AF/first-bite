//! Count records per topic and write the counts to a local file at a fixed
//! interval.

use super::{Sink, SinkError};
use crate::config::SubscriberConfig;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use log::warn;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

const DEFAULT_LOG: &str = "messagerates.log";
const INTERVAL: Duration = Duration::from_secs(10);
/// The file is cut back to its last `KEPT_LINES` lines once it passes
/// `TRIM_ABOVE_BYTES`.
const KEPT_LINES: usize = 100;
const TRIM_ABOVE_BYTES: u64 = 10_000;

type Counts = Arc<Mutex<BTreeMap<String, usize>>>;

pub struct MessageRatesSink {
    path: PathBuf,
    counts: Counts,
}

impl MessageRatesSink {
    /// Counts go to `local_logfile`, or `messagerates.log` in the working
    /// directory.
    pub fn from_config(config: &SubscriberConfig) -> Self {
        MessageRatesSink {
            path: PathBuf::from(config.local_logfile.as_deref().unwrap_or(DEFAULT_LOG)),
            counts: Counts::default(),
        }
    }
}

/// Take the counts gathered since the last call.
fn drain(counts: &Counts) -> BTreeMap<String, usize> {
    std::mem::take(&mut *counts.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Append one line: `"<from>::<to>": {"<topic>": <count>, ...}`.
fn append_counts(
    path: &Path,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    counts: &BTreeMap<String, usize>,
) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "\"{from:?}::{to:?}\": {counts:?}")?;
    if file.metadata()?.len() > TRIM_ABOVE_BYTES {
        let text = fs::read_to_string(path)?;
        let lines: Vec<&str> = text.lines().collect();
        if lines.len() > KEPT_LINES {
            let mut kept = lines[lines.len() - KEPT_LINES..].join("\n");
            kept.push('\n');
            fs::write(path, kept)?;
        }
    }
    Ok(())
}

#[async_trait]
impl Sink for MessageRatesSink {
    /// Start the file afresh and begin writing counts every interval.
    async fn prepare(&self) -> Result<(), SinkError> {
        File::create(&self.path)?;
        let path = self.path.clone();
        let counts = Arc::clone(&self.counts);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(INTERVAL);
            interval.tick().await;
            let mut from = Utc::now();
            loop {
                interval.tick().await;
                let to = Utc::now();
                if let Err(e) = append_counts(&path, from, to, &drain(&counts)) {
                    warn!("cannot write message rates to {}: {e}", path.display());
                }
                from = to;
            }
        });
        Ok(())
    }

    async fn send(&self, topic: &str, _record: &str) -> Result<(), SinkError> {
        *self
            .counts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(topic.to_string())
            .or_insert(0) += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{subscriber, temp_path};
    use chrono::TimeZone;

    #[tokio::test]
    async fn records_are_counted_per_topic_and_counts_reset_when_taken() {
        let sink = MessageRatesSink::from_config(&subscriber("    subscriber_type: stateful\n"));
        assert_eq!(sink.path, PathBuf::from(DEFAULT_LOG));
        for topic in ["WX/Heartbeat", "WX/Observation", "WX/Heartbeat"] {
            sink.send(topic, "{}").await.unwrap();
        }
        let counts = drain(&sink.counts);
        assert_eq!(counts["WX/Heartbeat"], 2);
        assert_eq!(counts["WX/Observation"], 1);
        assert!(drain(&sink.counts).is_empty());
    }

    #[test]
    fn counts_are_appended_and_the_file_stays_bounded() {
        let path = temp_path("message-rates.log");
        let _ = fs::remove_file(&path);
        let from = Utc.with_ymd_and_hms(2026, 3, 4, 12, 0, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2026, 3, 4, 12, 0, 10).unwrap();
        let counts = BTreeMap::from([("WX/Heartbeat".to_string(), 2)]);
        append_counts(&path, from, to, &counts).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "\"2026-03-04T12:00:00Z::2026-03-04T12:00:10Z\": {\"WX/Heartbeat\": 2}\n"
        );
        for _ in 0..300 {
            append_counts(&path, from, to, &counts).unwrap();
        }
        let lines = fs::read_to_string(&path).unwrap().lines().count();
        assert!((KEPT_LINES..=KEPT_LINES + 60).contains(&lines), "{lines}");
        let _ = fs::remove_file(&path);
    }
}
