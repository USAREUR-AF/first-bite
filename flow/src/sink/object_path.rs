//! Object names for the object-storage sinks.
//!
//! Each record is stored twice, so that a listing by time and a listing by
//! topic are both prefix scans. The name ends in the SHA-256 of the record,
//! which makes a repeated upload of the same record overwrite itself.

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

/// Longest topic kept in an object name, in bytes. Object stores cap a name
/// at about 1,024 bytes; the rest of the name needs under 120.
const MAX_TOPIC_BYTES: usize = 512;

/// The topic as it appears in an object name.
///
/// A topic arrives on a socket and is built from dictionary and deployment
/// names, so it is not trusted to be a path. Empty segments are dropped; a
/// segment of `.` or `..`, which a store or a proxy in front of it may
/// resolve as a directory step out of the prefix or the bucket, becomes `_`
/// or `__`; control characters and `\` become `_`. A topic with nothing left
/// is `_`. Every other topic is kept as it is.
pub fn topic_path(topic: &str) -> String {
    let mut path = String::new();
    for segment in topic.split('/').filter(|segment| !segment.is_empty()) {
        let segment: String = match segment {
            "." => "_".to_string(),
            ".." => "__".to_string(),
            other => other
                .chars()
                .map(|c| if c.is_control() || c == '\\' { '_' } else { c })
                .collect(),
        };
        if path.len() + 1 + segment.len() > MAX_TOPIC_BYTES {
            break;
        }
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(&segment);
    }
    if path.is_empty() {
        path.push('_');
    }
    path
}

/// `all-time-topic/<Y>/<m>/<d>/<H>/<M>/<S>/<topic>/<sha256>.json`
pub fn time_then_topic(topic: &str, record: &str, now: DateTime<Utc>) -> String {
    let topic = topic_path(topic);
    format!(
        "all-time-topic/{}/{topic}/{}.json",
        now.format("%Y/%m/%d/%H/%M/%S"),
        sha256_hex(record.as_bytes())
    )
}

/// `all-topic-time/<topic>/<Y>/<m>/<d>/<H>/<M>/<S>/<sha256>.json`
pub fn topic_then_time(topic: &str, record: &str, now: DateTime<Utc>) -> String {
    let topic = topic_path(topic);
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
    fn a_topic_cannot_step_out_of_its_prefix() {
        assert_eq!(topic_path("WX/Observation"), "WX/Observation");
        assert_eq!(
            topic_path("FAE/FOMO Report Message"),
            "FAE/FOMO Report Message"
        );
        assert_eq!(topic_path("../../other-bucket/x"), "__/__/other-bucket/x");
        assert_eq!(topic_path("/a//./b/"), "a/_/b");
        assert_eq!(topic_path("a\\..\\b\n\u{7}"), "a_.._b__");
        assert_eq!(topic_path(""), "_");
        assert_eq!(topic_path("///"), "_");
        assert_eq!(topic_path("..."), "...");

        let now = Utc.with_ymd_and_hms(2026, 3, 4, 5, 6, 7).unwrap();
        for name in [
            time_then_topic("../../../etc", "{}", now),
            topic_then_time("../../../etc", "{}", now),
        ] {
            assert!(name.split('/').all(|segment| segment != ".."), "{name}");
        }
        assert!(topic_then_time("..", "{}", now).starts_with("all-topic-time/__/2026/"));
    }

    #[test]
    fn a_long_topic_is_cut_at_a_segment() {
        let long = vec!["segment"; 200].join("/");
        let path = topic_path(&long);
        assert!(path.len() <= MAX_TOPIC_BYTES, "{}", path.len());
        assert!(path.ends_with("/segment"));
        assert_eq!(topic_path(&"x".repeat(MAX_TOPIC_BYTES + 1)), "_");
    }

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
