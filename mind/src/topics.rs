//! Check, at start-up, that each topic a subscriber lists is one the loaded
//! dictionary can produce.
//!
//! A topic entry is matched exactly, or as a prefix when it ends in `/`. An
//! entry that matches nothing delivers nothing and says nothing, so it is
//! reported here with the nearest topics that do exist.

use bite::Dictionary;
use std::collections::BTreeSet;

/// A topic entry that matches no topic the publisher will use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmatched {
    pub subscriber: String,
    pub entry: String,
    /// Up to three topics that do exist, nearest first.
    pub nearest: Vec<String>,
}

impl std::fmt::Display for Unmatched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "subscriber {}: topic {:?} matches no message of the dictionary, so nothing will \
             arrive on it",
            self.subscriber, self.entry
        )?;
        if !self.nearest.is_empty() {
            write!(f, "; nearest: {}", self.nearest.join(", "))?;
        }
        f.write_str(". An entry matches a topic exactly, or as a prefix when it ends in `/`")
    }
}

/// The topics a publisher with `rule` uses for the messages of `dictionary`;
/// `None` when they cannot be listed (`location`).
fn topics(dictionary: &Dictionary, rule: &str) -> Option<BTreeSet<String>> {
    let mut topics = BTreeSet::new();
    match rule {
        "*" => {
            topics.insert("catchall".to_string());
        }
        "message_type" | "subsystem/message_type" => {
            for interface in dictionary.interfaces() {
                let names = interface
                    .messages()
                    .iter()
                    .map(|message| message.name.as_str())
                    .chain(interface.discriminator.names.values().map(String::as_str));
                for name in names {
                    topics.insert(if rule == "message_type" {
                        name.to_string()
                    } else {
                        format!("{}/{name}", interface.name)
                    });
                }
            }
        }
        _ => return None,
    }
    Some(topics)
}

/// Whether `entry` can match a topic outside `topics`: a message type the
/// interface does not name is published under its number.
fn is_numbered(dictionary: &Dictionary, rule: &str, entry: &str) -> bool {
    let number = match rule {
        "message_type" => entry,
        _ => match entry.split_once('/') {
            Some((interface, number)) if dictionary.interface(interface).is_some() => number,
            _ => return false,
        },
    };
    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
}

/// Edit distance between two strings, compared without case.
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (
        a.to_lowercase().chars().collect(),
        b.to_lowercase().chars().collect(),
    );
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let substitution = previous + usize::from(x != y);
            previous = row[j + 1];
            row[j + 1] = substitution.min(previous + 1).min(row[j] + 1);
        }
    }
    row[b.len()]
}

/// The entries that match nothing.
///
/// `publishers` is each publisher's port and topic rule; `subscribers` is
/// each subscriber's name, the local port it reads and its entries. A
/// subscriber that reads another machine, or a port no publisher of this
/// process binds, is not checked.
pub fn unmatched(
    dictionary: &Dictionary,
    publishers: &[(String, &'static str)],
    subscribers: &[(String, Option<String>, Vec<String>)],
) -> Vec<Unmatched> {
    let mut found = Vec::new();
    for (subscriber, port, entries) in subscribers {
        let Some((_, rule)) = publishers
            .iter()
            .find(|(bound, _)| Some(bound) == port.as_ref())
        else {
            continue;
        };
        let Some(topics) = topics(dictionary, rule) else {
            continue;
        };
        for entry in entries {
            let matches = topics.iter().any(|topic| {
                topic == entry || (entry.ends_with('/') && topic.starts_with(entry.as_str()))
            });
            if matches || is_numbered(dictionary, rule, entry) {
                continue;
            }
            let mut nearest: Vec<(usize, &String)> = topics
                .iter()
                .map(|topic| {
                    // A topic the entry begins, or that begins the entry,
                    // is nearer than its length suggests.
                    let related = topic.starts_with(entry.trim_end_matches('*'))
                        || entry.starts_with(topic.as_str());
                    (if related { 0 } else { distance(entry, topic) }, topic)
                })
                .collect();
            nearest.sort();
            found.push(Unmatched {
                subscriber: subscriber.clone(),
                entry: entry.clone(),
                nearest: nearest
                    .into_iter()
                    .take(3)
                    .map(|(_, topic)| topic.clone())
                    .collect(),
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn dictionary() -> Dictionary {
        Dictionary::load(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../dictionaries/weather-station"),
        )
        .unwrap()
    }

    fn check(rule: &'static str, entries: &[&str]) -> Vec<Unmatched> {
        unmatched(
            &dictionary(),
            &[("12000".to_string(), rule)],
            &[(
                "ToFile".to_string(),
                Some("12000".to_string()),
                entries.iter().map(ToString::to_string).collect(),
            )],
        )
    }

    #[test]
    fn exact_topics_and_interface_prefixes_match() {
        assert!(check(
            "subsystem/message_type",
            &["WX/Position", "WX/Heartbeat", "WX/", "WX/42"]
        )
        .is_empty());
        assert!(check("message_type", &["Observation", "7"]).is_empty());
        assert!(check("*", &["catchall"]).is_empty());
        // Topics of a `location` publisher are not known from the dictionary.
        assert!(check("location", &["anywhere"]).is_empty());
    }

    #[test]
    fn an_entry_that_matches_nothing_is_named_with_the_nearest_topics() {
        let found = check(
            "subsystem/message_type",
            &["WX", "WX/Possition", "wx/position", "BUOY/", "WX/Pos"],
        );
        let entries: Vec<&str> = found.iter().map(|u| u.entry.as_str()).collect();
        assert_eq!(
            entries,
            ["WX", "WX/Possition", "wx/position", "BUOY/", "WX/Pos"]
        );
        // `WX` alone is the start of every topic of the interface.
        assert_eq!(
            found[0].nearest,
            ["WX/Heartbeat", "WX/Observation", "WX/Position"]
        );
        assert_eq!(found[1].nearest[0], "WX/Position");
        assert_eq!(found[2].nearest[0], "WX/Position");
        assert_eq!(found[4].nearest[0], "WX/Position");
        assert_eq!(
            found[1].to_string(),
            "subscriber ToFile: topic \"WX/Possition\" matches no message of the dictionary, so \
             nothing will arrive on it; nearest: WX/Position, WX/Observation, WX/Heartbeat. An \
             entry matches a topic exactly, or as a prefix when it ends in `/`"
        );
    }

    #[test]
    fn a_subscriber_of_another_publisher_is_not_checked() {
        let dictionary = dictionary();
        let publishers = [("12000".to_string(), "subsystem/message_type")];
        for port in [None, Some("13000".to_string())] {
            let subscribers = [("Far".to_string(), port, vec!["nothing".to_string()])];
            assert!(unmatched(&dictionary, &publishers, &subscribers).is_empty());
        }
    }
}
