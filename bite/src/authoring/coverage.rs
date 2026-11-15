//! What a dictionary covers of the messages it describes: which bits belong
//! to a field, which belong to none or to two, which messages are named and
//! not defined, and which fields lack a source, a description or a unit.

use super::json::Node;
use super::model::{fields, interface_source, is_filler_name, SourceRef, Tree};
use super::rules::Finding;
use serde_json::json;

/// Coverage of one message definition.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageCoverage {
    pub interface: String,
    pub message: String,
    /// The length the message file states in `declared_bits`.
    pub declared_bits: Option<u64>,
    /// The length coverage is measured over: `declared_bits`, or the end of
    /// the last field without it.
    pub length_bits: u64,
    pub covered_bits: u64,
    pub fields: usize,
    /// Bit ranges `[start, stop)` inside the length that no field covers.
    pub gaps: Vec<(u64, u64)>,
    /// Pairs of fields that share bits, and the shared range.
    pub overlaps: Vec<(String, String, u64, u64)>,
    /// Fields that reach past `declared_bits`.
    pub beyond: Vec<String>,
    /// Fields with no usable range.
    pub invalid: Vec<String>,
    /// Vectors that expect this message.
    pub vectors: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceCoverage {
    pub name: String,
    pub messages: Vec<MessageCoverage>,
    /// Named in `message_type.values` with no file in `messages/`.
    pub named_undefined: Vec<String>,
    /// A file in `messages/` that no `message_type` value names.
    pub defined_unnamed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Report {
    pub interfaces: Vec<InterfaceCoverage>,
    /// Fields that carry meaning: every field not named as spare or reserved.
    pub fields: usize,
    pub without_source: usize,
    pub without_description: usize,
    /// Numeric fields without labels, and how many of them state no unit.
    pub numeric_fields: usize,
    pub without_unit: usize,
    /// Every finding, each under its rule (C001 to C011).
    pub findings: Vec<Finding>,
}

/// The ranges inside `[0, end)` that none of `ranges` covers.
pub fn gaps(ranges: &[(u64, u64)], end: u64) -> Vec<(u64, u64)> {
    let mut sorted: Vec<(u64, u64)> = ranges.to_vec();
    sorted.sort_unstable();
    let mut out = Vec::new();
    let mut reached = 0;
    for (start, stop) in sorted {
        if start > reached && reached < end {
            out.push((reached, start.min(end)));
        }
        reached = reached.max(stop);
    }
    if reached < end {
        out.push((reached, end));
    }
    out
}

fn plural(count: u64, word: &str) -> String {
    if count == 1 {
        format!("1 {word}")
    } else {
        format!("{count} {word}s")
    }
}

/// Measure a dictionary.
pub fn coverage(tree: &Tree) -> Report {
    let mut report = Report::default();
    for interface in &tree.interfaces {
        let source = interface_source(tree, interface);
        let mut named: Vec<&str> = interface
            .type_values()
            .into_iter()
            .map(|(_, name)| name)
            .collect();
        named.sort_unstable();
        named.dedup();
        let mut entry = InterfaceCoverage {
            name: interface.name.clone(),
            messages: Vec::new(),
            named_undefined: Vec::new(),
            defined_unnamed: Vec::new(),
        };
        for name in &named {
            if interface.message(name).is_none() {
                entry.named_undefined.push(name.to_string());
                report.findings.push(Finding::new(
                    "C004",
                    format!("{}/{name}", interface.name),
                    "named in message_type and not defined",
                ));
            }
        }
        for (name, message) in &interface.messages {
            let at = format!("{}/{name}", interface.name);
            if !named.contains(&name.as_str()) {
                entry.defined_unnamed.push(name.clone());
                report.findings.push(Finding::new(
                    "C005",
                    at.clone(),
                    "defined and not named in message_type",
                ));
            }
            let vectors = tree
                .vectors
                .iter()
                .filter(|(_, vector)| {
                    vector.str_at("interface") == Some(interface.name.as_str())
                        && vector
                            .get("expect")
                            .and_then(|expect| expect.str_at("message_type"))
                            == Some(name.as_str())
                })
                .count();
            entry.messages.push(message_coverage(
                &interface.name,
                name,
                message,
                &source,
                vectors,
                &mut report,
            ));
        }
        report.interfaces.push(entry);
    }
    report
}

fn message_coverage(
    interface: &str,
    name: &str,
    message: &Node,
    interface_source: &SourceRef,
    vectors: usize,
    report: &mut Report,
) -> MessageCoverage {
    let at = format!("{interface}/{name}");
    let declared_bits = message.u64_at("declared_bits");
    let message_source = SourceRef::read(message).or(interface_source);
    let views = fields(message);
    let mut ranges: Vec<(&str, u64, u64)> = Vec::new();
    let mut invalid = Vec::new();
    let mut beyond = Vec::new();
    for view in &views {
        let field_at = format!("{at}/{}", view.name);
        match view.range() {
            Some((start, stop)) => {
                ranges.push((view.name, start, stop));
                if let Some(declared) = declared_bits.filter(|declared| stop > *declared) {
                    beyond.push(view.name.to_string());
                    report.findings.push(Finding::new(
                        "C003",
                        field_at.clone(),
                        format!(
                            "bits [{start}, {stop}) reach past the declared length of {declared} bits"
                        ),
                    ));
                }
            }
            None => {
                invalid.push(view.name.to_string());
                let show = |value: Option<u64>| value.map_or("?".to_string(), |v| v.to_string());
                report.findings.push(Finding::new(
                    "C011",
                    field_at.clone(),
                    format!(
                        "bits [{}, {}) are missing, empty or inverted",
                        show(view.start),
                        show(view.stop)
                    ),
                ));
            }
        }
        if is_filler_name(view.name) {
            continue;
        }
        report.fields += 1;
        if !SourceRef::read(view.node).or(&message_source).is_complete() {
            report.without_source += 1;
            report.findings.push(Finding::new(
                "C007",
                field_at.clone(),
                "no complete source: document, revision and a section, table or page",
            ));
        }
        if view
            .node
            .str_at("description")
            .is_none_or(|text| text.trim().is_empty())
        {
            report.without_description += 1;
            report
                .findings
                .push(Finding::new("C008", field_at.clone(), "no description"));
        }
        if view.is_numeric() && !view.has_values() {
            report.numeric_fields += 1;
            if view
                .node
                .str_at("unit")
                .is_none_or(|text| text.trim().is_empty())
            {
                report.without_unit += 1;
                report.findings.push(Finding::new(
                    "C009",
                    field_at,
                    "a numeric field with no unit (\"1\" for a count, an index or an identifier)",
                ));
            }
        }
    }

    let mut overlaps = Vec::new();
    for (index, a) in ranges.iter().enumerate() {
        for b in &ranges[index + 1..] {
            let (start, stop) = (a.1.max(b.1), a.2.min(b.2));
            if start < stop {
                overlaps.push((a.0.to_string(), b.0.to_string(), start, stop));
                report.findings.push(Finding::new(
                    "C002",
                    format!("{at}/{}", b.0),
                    format!(
                        "bits [{}, {}) overlap {} [{}, {}) in [{start}, {stop})",
                        b.1, b.2, a.0, a.1, a.2
                    ),
                ));
            }
        }
    }

    let spans: Vec<(u64, u64)> = ranges.iter().map(|range| (range.1, range.2)).collect();
    let last = spans.iter().map(|span| span.1).max().unwrap_or(0);
    let length_bits = declared_bits.unwrap_or(last);
    let gaps = gaps(&spans, length_bits);
    let missing: u64 = gaps.iter().map(|(start, stop)| stop - start).sum();
    for (start, stop) in &gaps {
        report.findings.push(Finding::new(
            "C001",
            at.clone(),
            format!(
                "bits [{start}, {stop}) belong to no field ({})",
                plural(stop - start, "bit")
            ),
        ));
    }
    if declared_bits.is_none() {
        report.findings.push(Finding::new(
            "C006",
            at.clone(),
            "no declared_bits: bits after the last field cannot be checked",
        ));
    }
    if vectors == 0 {
        report
            .findings
            .push(Finding::new("C010", at, "no vector expects this message"));
    }
    MessageCoverage {
        interface: interface.to_string(),
        message: name.to_string(),
        declared_bits,
        length_bits,
        covered_bits: length_bits - missing,
        fields: views.len(),
        gaps,
        overlaps,
        beyond,
        invalid,
        vectors,
    }
}

impl Report {
    pub fn messages(&self) -> impl Iterator<Item = &MessageCoverage> {
        self.interfaces
            .iter()
            .flat_map(|interface| interface.messages.iter())
    }

    /// How many findings there are under a rule.
    pub fn count(&self, rule: &str) -> usize {
        self.findings.iter().filter(|f| f.rule == rule).count()
    }

    pub fn to_json(&self) -> serde_json::Value {
        let interfaces: Vec<serde_json::Value> = self
            .interfaces
            .iter()
            .map(|interface| {
                let messages: Vec<serde_json::Value> = interface
                    .messages
                    .iter()
                    .map(|m| {
                        json!({
                            "message": m.message,
                            "declared_bits": m.declared_bits,
                            "length_bits": m.length_bits,
                            "covered_bits": m.covered_bits,
                            "fields": m.fields,
                            "gaps": m.gaps.iter().map(|(a, b)| json!([a, b])).collect::<Vec<_>>(),
                            "overlaps": m.overlaps.iter().map(|(a, b, start, stop)| json!({
                                "fields": [a, b], "bits": [start, stop],
                            })).collect::<Vec<_>>(),
                            "beyond_declared": m.beyond,
                            "invalid": m.invalid,
                            "vectors": m.vectors,
                        })
                    })
                    .collect();
                json!({
                    "interface": interface.name,
                    "messages": messages,
                    "named_undefined": interface.named_undefined,
                    "defined_unnamed": interface.defined_unnamed,
                })
            })
            .collect();
        json!({
            "interfaces": interfaces,
            "fields": self.fields,
            "without_source": self.without_source,
            "without_description": self.without_description,
            "numeric_fields": self.numeric_fields,
            "without_unit": self.without_unit,
        })
    }

    /// The report as text: one line per message, its gaps and overlaps under
    /// it, then the interface and dictionary totals.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for interface in &self.interfaces {
            for m in &interface.messages {
                let length = match m.declared_bits {
                    Some(bits) => format!("{bits} bits declared"),
                    None => format!("{} bits to the last field, none declared", m.length_bits),
                };
                out.push_str(&format!(
                    "{}/{}: {length}, {} covered by {}, {}, {}\n",
                    m.interface,
                    m.message,
                    m.covered_bits,
                    plural(m.fields as u64, "field"),
                    plural(m.gaps.len() as u64, "gap"),
                    plural(m.vectors as u64, "vector"),
                ));
                for (start, stop) in &m.gaps {
                    out.push_str(&format!(
                        "  gap [{start}, {stop}) {}\n",
                        plural(stop - start, "bit")
                    ));
                }
                for (a, b, start, stop) in &m.overlaps {
                    out.push_str(&format!("  overlap [{start}, {stop}) {a} and {b}\n"));
                }
                for name in &m.beyond {
                    out.push_str(&format!("  beyond the declared length: {name}\n"));
                }
                for name in &m.invalid {
                    out.push_str(&format!("  no usable bit range: {name}\n"));
                }
            }
            for name in &interface.named_undefined {
                out.push_str(&format!(
                    "{}/{name}: named in message_type, not defined\n",
                    interface.name
                ));
            }
            for name in &interface.defined_unnamed {
                out.push_str(&format!(
                    "{}/{name}: defined, not named in message_type\n",
                    interface.name
                ));
            }
        }
        let messages = self.messages().count();
        let full = self.messages().filter(|m| m.gaps.is_empty()).count();
        out.push_str(&format!(
            "{} of {} fully covered; {} named and not defined; {} defined and not named\n",
            full,
            plural(messages as u64, "message"),
            self.count("C004"),
            self.count("C005"),
        ));
        out.push_str(&format!(
            "{}: {} without a source, {} without a description; {} of {} without a unit\n",
            plural(self.fields as u64, "field"),
            self.without_source,
            self.without_description,
            self.without_unit,
            plural(self.numeric_fields as u64, "numeric field"),
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::model::FileSet;

    #[test]
    fn gaps_between_and_after_ranges() {
        assert_eq!(gaps(&[(0, 8), (8, 16)], 16), []);
        assert_eq!(gaps(&[(8, 16), (0, 4)], 24), [(4, 8), (16, 24)]);
        assert_eq!(gaps(&[(0, 12), (4, 8)], 12), []);
        assert_eq!(gaps(&[], 8), [(0, 8)]);
        assert_eq!(gaps(&[(0, 32)], 16), []);
    }

    /// A message with a gap of four bits at [12, 16), an overlap of four bits
    /// at [20, 24), a field past the declared length and an inverted field.
    #[test]
    fn a_known_gap_and_overlap_are_found() {
        let mut files = FileSet::new();
        files.insert(
            "A/metadata.json".into(),
            r#"{"message_type": {"start": 0, "stop": 8, "values": {"1": "M", "2": "Absent"}}}"#
                .into(),
        );
        files.insert(
            "A/messages/M.json".into(),
            r#"{"declared_bits": 40, "fields": {
                "kind": {"start": 0, "stop": 8},
                "a": {"start": 8, "stop": 12},
                "b": {"start": 16, "stop": 24},
                "c": {"start": 20, "stop": 32},
                "d": {"start": 32, "stop": 48},
                "e": {"start": 9, "stop": 3}
            }}"#
            .into(),
        );
        files.insert("A/messages/Orphan.json".into(), r#"{"fields": {}}"#.into());
        let report = coverage(&Tree::parse(&files).unwrap());
        let m = &report.interfaces[0].messages[0];
        assert_eq!(m.gaps, [(12, 16)]);
        assert_eq!(m.overlaps, [("b".to_string(), "c".to_string(), 20, 24)]);
        assert_eq!(m.beyond, ["d"]);
        assert_eq!(m.invalid, ["e"]);
        assert_eq!((m.length_bits, m.covered_bits), (40, 36));
        assert_eq!(report.interfaces[0].named_undefined, ["Absent"]);
        assert_eq!(report.interfaces[0].defined_unnamed, ["Orphan"]);
        for (rule, count) in [
            ("C001", 1),
            ("C002", 1),
            ("C003", 1),
            ("C004", 1),
            ("C005", 1),
            ("C006", 1),
            ("C010", 2),
            ("C011", 1),
        ] {
            assert_eq!(report.count(rule), count, "{rule}");
        }
        let text = report.to_text();
        assert!(text.contains("A/M: 40 bits declared, 36 covered by 6 fields, 1 gap, 0 vectors\n  gap [12, 16) 4 bits\n  overlap [20, 24) b and c\n"), "{text}");
    }
}
