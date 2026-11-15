//! Object names for the object-storage sinks.
//!
//! Each record is stored twice, so that a listing by time and a listing by
//! topic are both prefix scans. The name ends in the SHA-256 of the record,
//! which makes a repeated upload of the same record overwrite itself.

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

/// `all-time-topic/<Y>/<m>/<d>/<H>/<M>/<S>/<topic>/<sha256>.json`
pub fn time_then_topic(topic: &str, record: &str, now: DateTime<Utc>) -> String {
    format!(
        "all-time-topic/{}/{topic}/{}.json",
        now.format("%Y/%m/%d/%H/%M/%S"),
        sha256_hex(record.as_bytes())
    )
}

/// `all-topic-time/<topic>/<Y>/<m>/<d>/<H>/<M>/<S>/<sha256>.json`
pub fn topic_then_time(topic: &str, record: &str, now: DateTime<Utc>) -> String {
    format!(
        "all-topic-time/{topic}/{}/{}.json",
        now.format("%Y/%m/%d/%H/%M/%S"),
        sha256_hex(record.as_bytes())
    )
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const EMPTY_OBJECT_SHA256: &str =
        "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a";

    #[test]
    fn names_are_ordered_by_time_or_by_topic() {
        let now = Utc.with_ymd_and_hms(2026, 3, 4, 5, 6, 7).unwrap();
        assert_eq!(
            time_then_topic("WX/Observation", "{}", now),
            format!("all-time-topic/2026/03/04/05/06/07/WX/Observation/{EMPTY_OBJECT_SHA256}.json")
        );
        assert_eq!(
            topic_then_time("WX/Observation", "{}", now),
            format!("all-topic-time/WX/Observation/2026/03/04/05/06/07/{EMPTY_OBJECT_SHA256}.json")
        );
    }
}
