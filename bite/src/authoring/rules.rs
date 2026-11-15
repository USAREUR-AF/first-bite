//! The rules `bite lint` and `bite coverage` report against, and how a
//! dictionary allows a finding with a reason.

use super::json::Node;

/// One rule: its identifier and what it requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    pub id: &'static str,
    pub requires: &'static str,
}

/// Every rule, in the order `bite lint --rules` prints them. The same table is
/// in `docs/icd-to-dictionary/02-conventions.md`; a test keeps them equal.
pub const RULES: &[Rule] = &[
    Rule { id: "N001", requires: "An interface name is upper-case letters, digits and underscores, starting with a letter." },
    Rule { id: "N002", requires: "A message name is the document's name for the message: words of letters, digits and hyphens with single spaces between them." },
    Rule { id: "N003", requires: "A field name is snake_case: lower-case letters and digits, words joined by single underscores, starting with a letter." },
    Rule { id: "N004", requires: "Bits with no meaning are named spare_<start bit> or reserved_<start bit>." },
    Rule { id: "N005", requires: "Two message names in one interface differ by more than letter case." },
    Rule { id: "U001", requires: "A unit is one of the unit vocabulary, or is listed under lint.units in dictionary.json." },
    Rule { id: "U002", requires: "A field with a scale, an offset or type float states its unit." },
    Rule { id: "L001", requires: "A label is not empty and has no space at either end." },
    Rule { id: "L002", requires: "Within one field, a label names one value." },
    Rule { id: "L003", requires: "Values the document leaves reserved, spare or undefined are not given a label." },
    Rule { id: "L004", requires: "The keys of one values object are all decimal or all hexadecimal." },
    Rule { id: "P001", requires: "Every interface resolves a source document and revision, its own or the dictionary's." },
    Rule { id: "P002", requires: "Every message has a source with a section, a table or a page." },
    Rule { id: "D001", requires: "Every interface and every message has a description." },
    Rule { id: "D002", requires: "A description is a sentence: it starts with a capital letter or a digit, ends with a full stop and has no space at either end." },
    Rule { id: "S001", requires: "A signed integer is written \"type\": \"int\"; the signed key is not used." },
    Rule { id: "S002", requires: "A key that only repeats a default is left out: \"type\": \"uint\", a byte_order equal to the interface's, \"scale\": 1, \"offset\": 0." },
    Rule { id: "S003", requires: "dictionary.json gives a name and a version of the form MAJOR.MINOR.PATCH." },
    Rule { id: "H001", requires: "Every message of an interface has a field on exactly the message_type bits and one on exactly the message_length bits, with the same names throughout the interface." },
    Rule { id: "V001", requires: "A vector expects something: a message type or at least one field." },
    Rule { id: "V002", requires: "A vector says where its expected values come from, in a source or a note." },
    Rule { id: "X001", requires: "An entry of lint.allow names a rule that exists and gives a reason." },
    Rule { id: "X002", requires: "An entry of lint.allow matches at least one finding." },
    Rule { id: "C001", requires: "Every bit of a message up to its length belongs to a field." },
    Rule { id: "C002", requires: "No two fields of a message share a bit." },
    Rule { id: "C003", requires: "No field reaches past the message's declared_bits." },
    Rule { id: "C004", requires: "Every message named in message_type has a definition." },
    Rule { id: "C005", requires: "Every message definition is named in message_type." },
    Rule { id: "C006", requires: "Every message states its length in declared_bits." },
    Rule { id: "C007", requires: "Every field resolves a complete source: document, revision and a section, table or page." },
    Rule { id: "C008", requires: "Every field that carries meaning has a description." },
    Rule { id: "C009", requires: "Every numeric field without labels states its unit." },
    Rule { id: "C010", requires: "Every message definition is exercised by at least one vector." },
    Rule { id: "C011", requires: "Every field has a start and a stop with stop greater than start." },
];

pub fn rule(id: &str) -> Option<&'static Rule> {
    RULES.iter().find(|rule| rule.id == id)
}

/// The unit vocabulary: the spelling to use and what it stands for.
pub const UNITS: &[(&str, &str)] = &[
    (
        "1",
        "dimensionless: a count, an index, an identifier, a ratio",
    ),
    ("%", "percent"),
    ("ppm", "parts per million"),
    ("dB", "decibel"),
    ("dBm", "decibel relative to one milliwatt"),
    ("s", "second"),
    ("ms", "millisecond"),
    ("us", "microsecond"),
    ("ns", "nanosecond"),
    ("min", "minute"),
    ("h", "hour"),
    ("d", "day"),
    ("m", "metre"),
    ("km", "kilometre"),
    ("cm", "centimetre"),
    ("mm", "millimetre"),
    ("ft", "foot"),
    ("nmi", "nautical mile"),
    ("m/s", "metre per second"),
    ("km/h", "kilometre per hour"),
    ("kn", "knot"),
    ("ft/min", "foot per minute"),
    ("mm/h", "millimetre per hour"),
    ("m/s2", "metre per second squared"),
    ("deg", "degree of angle"),
    ("rad", "radian"),
    ("mrad", "milliradian"),
    ("arcmin", "minute of arc"),
    ("arcsec", "second of arc"),
    ("deg/s", "degree per second"),
    ("rad/s", "radian per second"),
    ("rpm", "revolution per minute"),
    ("kg", "kilogram"),
    ("g", "gram"),
    ("N", "newton"),
    ("Nm", "newton metre"),
    ("Pa", "pascal"),
    ("hPa", "hectopascal"),
    ("kPa", "kilopascal"),
    ("bar", "bar"),
    ("J", "joule"),
    ("Wh", "watt hour"),
    ("kWh", "kilowatt hour"),
    ("W", "watt"),
    ("kW", "kilowatt"),
    ("V", "volt"),
    ("mV", "millivolt"),
    ("A", "ampere"),
    ("mA", "milliampere"),
    ("ohm", "ohm"),
    ("Hz", "hertz"),
    ("kHz", "kilohertz"),
    ("MHz", "megahertz"),
    ("GHz", "gigahertz"),
    ("degC", "degree Celsius"),
    ("K", "kelvin"),
    ("L", "litre"),
    ("L/min", "litre per minute"),
    ("m3", "cubic metre"),
    ("bits", "bits"),
    ("bytes", "bytes of eight bits"),
    ("words", "words of the interface's word size"),
    ("bit/s", "bit per second"),
    ("kbit/s", "kilobit per second"),
    ("Mbit/s", "megabit per second"),
];

/// Something a rule found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub rule: &'static str,
    /// `dictionary.json`, `INTERFACE`, `INTERFACE/Message`,
    /// `INTERFACE/Message/field` or `vectors/<name>`.
    pub location: String,
    pub message: String,
}

impl Finding {
    pub fn new(
        rule: &'static str,
        location: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Finding {
            rule,
            location: location.into(),
            message: message.into(),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "rule": self.rule,
            "location": self.location,
            "message": self.message,
        })
    }
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}: {}", self.rule, self.location, self.message)
    }
}

/// One entry of `lint.allow` in `dictionary.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allow {
    pub rule: String,
    /// The location the entry covers, with everything under it; everywhere
    /// when absent.
    pub at: Option<String>,
    pub reason: String,
}

impl Allow {
    pub fn covers(&self, finding: &Finding) -> bool {
        self.covers_at(finding.rule, &finding.location)
    }

    pub fn covers_at(&self, rule: &str, location: &str) -> bool {
        self.rule == rule
            && match &self.at {
                None => true,
                Some(at) => {
                    location == at
                        || location
                            .strip_prefix(at.as_str())
                            .is_some_and(|rest| rest.starts_with('/'))
                }
            }
    }
}

/// The `lint` object of `dictionary.json`: what is allowed and which units
/// are added to the vocabulary.
#[derive(Debug, Clone, Default)]
pub struct LintConfig {
    pub allows: Vec<Allow>,
    pub units: Vec<String>,
    /// Entries that could not be used, as X001 findings.
    pub malformed: Vec<Finding>,
}

impl LintConfig {
    pub fn read(dictionary: Option<&Node>) -> LintConfig {
        let mut config = LintConfig::default();
        let Some(lint) = dictionary.and_then(|node| node.get("lint")) else {
            return config;
        };
        if let Some(units) = lint.get("units") {
            for unit in units.items() {
                match unit.as_str() {
                    Some(unit) => config.units.push(unit.to_string()),
                    None => config.malformed.push(Finding::new(
                        "X001",
                        "dictionary.json",
                        format!("lint.units holds {}, which is not text", unit.to_line()),
                    )),
                }
            }
        }
        if let Some(allow) = lint.get("allow") {
            for (index, entry) in allow.items().iter().enumerate() {
                let bad = |why: &str| {
                    Finding::new(
                        "X001",
                        "dictionary.json",
                        format!("lint.allow entry {} {why}", index + 1),
                    )
                };
                let Some(id) = entry.str_at("rule") else {
                    config.malformed.push(bad("has no rule"));
                    continue;
                };
                if rule(id).is_none() {
                    config
                        .malformed
                        .push(bad(&format!("names {id:?}, which is not a rule")));
                    continue;
                }
                if id.starts_with('X') || id == "C011" {
                    config
                        .malformed
                        .push(bad(&format!("allows {id}, which cannot be allowed")));
                    continue;
                }
                let reason = entry.str_at("reason").unwrap_or_default().trim();
                if reason.is_empty() {
                    config.malformed.push(bad("gives no reason"));
                    continue;
                }
                config.allows.push(Allow {
                    rule: id.to_string(),
                    at: entry.str_at("at").map(str::to_string),
                    reason: reason.to_string(),
                });
            }
        }
        config
    }
}

/// Findings split by whether the dictionary allows them.
#[derive(Debug, Clone, Default)]
pub struct Sorted {
    pub kept: Vec<Finding>,
    /// The finding and the reason it is allowed.
    pub allowed: Vec<(Finding, String)>,
}

/// Split `findings` by the allow list, marking in `used` every entry that
/// covered something.
pub fn sort(findings: Vec<Finding>, allows: &[Allow], used: &mut [bool]) -> Sorted {
    let mut sorted = Sorted::default();
    for finding in findings {
        match allows.iter().position(|allow| allow.covers(&finding)) {
            Some(index) => {
                // Every entry that covers the finding counts as used.
                for (entry, allow) in allows.iter().enumerate() {
                    if allow.covers(&finding) {
                        used[entry] = true;
                    }
                }
                sorted.allowed.push((finding, allows[index].reason.clone()));
            }
            None => sorted.kept.push(finding),
        }
    }
    sorted
}

/// The rule a warning of the loader ([`crate::Issue`]) corresponds to, for the
/// three warnings a dictionary may allow with a reason.
pub fn rule_of_check_warning(message: &str) -> Option<&'static str> {
    if message.contains("overlap") {
        Some("C002")
    } else if message.starts_with("named by message_type but has no file") {
        Some("C004")
    } else if message.starts_with("no message_type value maps to this message") {
        Some("C005")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_identifiers_are_unique() {
        for (index, rule) in RULES.iter().enumerate() {
            assert!(
                RULES[..index].iter().all(|other| other.id != rule.id),
                "{}",
                rule.id
            );
        }
        for (index, unit) in UNITS.iter().enumerate() {
            assert!(
                UNITS[..index].iter().all(|other| other.0 != unit.0),
                "{}",
                unit.0
            );
        }
    }

    #[test]
    fn an_allow_covers_a_location_and_what_is_under_it() {
        let allow = Allow {
            rule: "N004".into(),
            at: Some("WX/Heartbeat".into()),
            reason: "r".into(),
        };
        assert!(allow.covers_at("N004", "WX/Heartbeat"));
        assert!(allow.covers_at("N004", "WX/Heartbeat/reserved"));
        assert!(!allow.covers_at("N004", "WX/Heartbeat Extended/reserved"));
        assert!(!allow.covers_at("N003", "WX/Heartbeat/reserved"));
    }

    #[test]
    fn allow_entries_need_a_known_rule_and_a_reason() {
        let node = Node::parse(
            r#"{"lint": {"units": ["furlong", 3], "allow": [
                {"rule": "N004", "reason": "kept"},
                {"rule": "N004"},
                {"rule": "Z999", "reason": "x"},
                {"reason": "x"},
                {"rule": "X002", "reason": "x"}
            ]}}"#,
        )
        .unwrap();
        let config = LintConfig::read(Some(&node));
        assert_eq!(config.allows.len(), 1);
        assert_eq!(config.units, ["furlong"]);
        assert_eq!(config.malformed.len(), 5);
    }

    #[test]
    fn the_loader_s_warnings_map_to_rules() {
        let dictionary = crate::Dictionary::from_files([
            (
                "A/metadata.json",
                r#"{"message_type": {"start": 0, "stop": 8, "values": {"1": "Named", "2": "Missing"}}}"#,
            ),
            (
                "A/messages/Named.json",
                r#"{"fields": {"a": {"start": 0, "stop": 8}, "b": {"start": 4, "stop": 12}}}"#,
            ),
            ("A/messages/Unnamed.json", r#"{"fields": {}}"#),
        ])
        .unwrap();
        let mut rules: Vec<&str> = dictionary
            .issues()
            .iter()
            .map(|issue| rule_of_check_warning(&issue.message).expect("mapped"))
            .collect();
        rules.sort_unstable();
        assert_eq!(rules, ["C002", "C004", "C005"]);
    }
}
