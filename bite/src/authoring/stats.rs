//! What a set of payloads says about a dictionary: how often each message
//! occurs, what values each field takes, and where the two disagree.
//!
//! A capture is evidence, not truth. A field that is zero in every payload
//! may be transcribed at the wrong bits, or the capture may never have
//! exercised it; a value with no label may be missing from the dictionary, or
//! the sender may be wrong. Each flag is a place to look in the document.

use super::coverage::gaps;
use super::model::is_filler_name;
use crate::bits::extract;
use crate::dictionary::{Interface, Message};
use crate::schema::{DiscriminatorSource, FieldKind};
use serde_json::json;
use std::collections::BTreeMap;

/// One payload, with the frame identifier it arrived with when it has one.
#[derive(Debug, Clone, PartialEq)]
pub struct Payload {
    pub frame_id: Option<u64>,
    pub bytes: Vec<u8>,
}

/// Parse payloads, one per line: hex bytes (spaces allowed), optionally
/// preceded by a frame identifier in hex and `#`, as `can-utils` writes
/// (`100#E02E28B4250200`). Blank lines and lines starting with `;` are
/// skipped.
pub fn parse_payloads(text: &str) -> Result<Vec<Payload>, String> {
    let mut payloads = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let at = index + 1;
        let (frame_id, hex_part) = match line.split_once('#') {
            Some((id, rest)) => (
                Some(
                    u64::from_str_radix(id.trim(), 16)
                        .map_err(|_| format!("line {at}: frame identifier {id:?} is not hex"))?,
                ),
                rest,
            ),
            None => (None, line),
        };
        let compact: String = hex_part.chars().filter(|c| !c.is_whitespace()).collect();
        let bytes = hex::decode(&compact).map_err(|e| format!("line {at}: {e}"))?;
        payloads.push(Payload { frame_id, bytes });
    }
    Ok(payloads)
}

/// `1 payload`, `2 payloads`.
fn n(count: usize) -> String {
    if count == 1 {
        "1 payload".to_string()
    } else {
        format!("{count} payloads")
    }
}

/// How many distinct values of one field are kept before counting stops.
const MAX_DISTINCT: usize = 4096;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FieldStats {
    pub name: String,
    /// Payloads long enough to hold the field.
    pub present: usize,
    /// Payloads that end before the field does.
    pub absent: usize,
    /// Rendered value to the number of payloads with it.
    pub values: BTreeMap<String, usize>,
    /// True when more than [`MAX_DISTINCT`] values were seen and counting
    /// of new ones stopped.
    pub truncated: bool,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Payloads in which every bit of the field is zero.
    pub zero: usize,
    /// For a field with labels: values that have none, and how often.
    pub unlabelled: BTreeMap<String, usize>,
    /// What to look at, in words.
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MessageStats {
    pub name: String,
    pub count: usize,
    /// Payload length in bytes to the number of payloads of that length.
    pub lengths: BTreeMap<usize, usize>,
    /// Bytes the definition needs: to the end of its last field.
    pub definition_bytes: usize,
    pub fields: Vec<FieldStats>,
    /// Bit ranges no field covers, with the number of payloads that carry a
    /// one bit there.
    pub gaps_set: Vec<((u64, u64), usize)>,
    /// Payloads with a non-zero byte after the definition ends.
    pub trailing_set: usize,
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stats {
    pub interface: String,
    pub payloads: usize,
    /// Payloads too short to hold the message type, or with no frame
    /// identifier where one is needed.
    pub unreadable: usize,
    /// Identifiers the interface has no name for.
    pub unknown_types: BTreeMap<u64, usize>,
    /// Messages the interface names and does not define.
    pub undefined: BTreeMap<String, usize>,
    pub messages: Vec<MessageStats>,
}

fn all_zero(field: &crate::Field, payload: &[u8]) -> bool {
    match field.kind {
        FieldKind::Bytes | FieldKind::Ascii => payload[field.start / 8..field.stop / 8]
            .iter()
            .all(|byte| *byte == 0),
        _ => extract(payload, field.start, field.stop, field.bit_order) == Some(0),
    }
}

/// Decode every payload against `interface` and count what is seen.
/// `declared_bits` gives the declared length of a message, by name.
pub fn stats(
    interface: &Interface,
    declared_bits: &dyn Fn(&str) -> Option<u64>,
    payloads: &[Payload],
) -> Stats {
    let mut out = Stats {
        interface: interface.name.clone(),
        payloads: payloads.len(),
        ..Stats::default()
    };
    // A message's definition, its counts and its unassigned bit ranges.
    type Entry<'a> = (&'a Message, MessageStats, Vec<(u64, u64)>);
    let mut by_message: BTreeMap<&str, Entry<'_>> = BTreeMap::new();
    for payload in payloads {
        let Ok(decoded) = interface.decode_frame(&payload.bytes, payload.frame_id) else {
            out.unreadable += 1;
            continue;
        };
        let Some(name) = decoded.type_name else {
            *out.unknown_types.entry(decoded.type_code).or_default() += 1;
            continue;
        };
        let Some(message) = decoded.message else {
            *out.undefined.entry(name.to_string()).or_default() += 1;
            continue;
        };
        let entry = by_message.entry(name).or_insert_with(|| {
            let spans: Vec<(u64, u64)> = message
                .fields
                .iter()
                .map(|field| (field.start as u64, field.stop as u64))
                .collect();
            let end = declared_bits(name).unwrap_or(message.bit_length() as u64);
            let stats = MessageStats {
                name: name.to_string(),
                definition_bytes: message.bit_length().div_ceil(8),
                fields: message
                    .fields
                    .iter()
                    .map(|field| FieldStats {
                        name: field.name.clone(),
                        ..FieldStats::default()
                    })
                    .collect(),
                ..MessageStats::default()
            };
            let gaps = gaps(&spans, end);
            (message, stats, gaps)
        });
        let (_, stats, gap_list) = entry;
        stats.count += 1;
        *stats.lengths.entry(payload.bytes.len()).or_default() += 1;
        if stats.gaps_set.is_empty() {
            stats.gaps_set = gap_list.iter().map(|gap| (*gap, 0)).collect();
        }
        for (gap, count) in &mut stats.gaps_set {
            let set = (gap.0..gap.1).any(|bit| {
                extract(
                    &payload.bytes,
                    bit as usize,
                    bit as usize + 1,
                    interface.bit_order,
                ) == Some(1)
            });
            if set {
                *count += 1;
            }
        }
        if payload.bytes.len() > stats.definition_bytes
            && payload.bytes[stats.definition_bytes..]
                .iter()
                .any(|byte| *byte != 0)
        {
            stats.trailing_set += 1;
        }
        for (field_stats, value) in stats.fields.iter_mut().zip(&decoded.fields) {
            let Some(rendered) = value.render() else {
                field_stats.absent += 1;
                continue;
            };
            field_stats.present += 1;
            if all_zero(value.field, &payload.bytes) {
                field_stats.zero += 1;
            }
            let number = match &value.value {
                Some(crate::Value::UInt(v)) => Some(*v as f64),
                Some(crate::Value::Int(v)) => Some(*v as f64),
                Some(crate::Value::Float(v)) => Some(*v),
                _ => None,
            };
            if let Some(number) = number.filter(|n| n.is_finite()) {
                field_stats.min = Some(field_stats.min.map_or(number, |m| m.min(number)));
                field_stats.max = Some(field_stats.max.map_or(number, |m| m.max(number)));
            }
            if value.field.has_labels() && value.label.is_none() {
                *field_stats.unlabelled.entry(rendered.clone()).or_default() += 1;
            }
            if field_stats.values.len() < MAX_DISTINCT || field_stats.values.contains_key(&rendered)
            {
                *field_stats.values.entry(rendered).or_default() += 1;
            } else {
                field_stats.truncated = true;
            }
        }
    }

    for (name, (message, mut stats, _)) in by_message {
        let declared = declared_bits(name);
        if let Some(bits) = declared {
            let bytes = (bits as usize).div_ceil(8);
            let other: usize = stats
                .lengths
                .iter()
                .filter(|(length, _)| **length != bytes)
                .map(|(_, count)| count)
                .sum();
            if other > 0 {
                stats.flags.push(format!(
                    "length differs from the declared {bytes} bytes in {}",
                    n(other)
                ));
            }
        }
        if stats.trailing_set > 0 {
            stats.flags.push(format!(
                "{} with non-zero bytes after the last field (byte {})",
                n(stats.trailing_set),
                stats.definition_bytes
            ));
        }
        for ((start, stop), count) in &stats.gaps_set {
            if *count > 0 {
                stats.flags.push(format!(
                    "bits [{start}, {stop}) belong to no field and are non-zero in {}",
                    n(*count)
                ));
            }
        }
        for (field_stats, field) in stats.fields.iter_mut().zip(&message.fields) {
            let filler = is_filler_name(&field.name);
            let disc = &interface.discriminator;
            let is_type = disc.source == DiscriminatorSource::Payload
                && (field.start, field.stop) == (disc.start, disc.stop);
            if field_stats.present == 0 {
                field_stats
                    .flags
                    .push("never present: every payload ends before it".to_string());
                continue;
            }
            if filler {
                let set = field_stats.present - field_stats.zero;
                if set > 0 {
                    field_stats.flags.push(format!(
                        "named as having no meaning and non-zero in {}",
                        n(set)
                    ));
                }
                continue;
            }
            if !field_stats.unlabelled.is_empty() {
                let total: usize = field_stats.unlabelled.values().sum();
                let mut examples: Vec<&str> = field_stats
                    .unlabelled
                    .keys()
                    .map(String::as_str)
                    .take(5)
                    .collect();
                examples.sort_by_key(|text| text.parse::<i128>().unwrap_or(i128::MAX));
                field_stats.flags.push(format!(
                    "{} with a value that has no label: {}{}",
                    n(total),
                    examples.join(", "),
                    if field_stats.unlabelled.len() > 5 {
                        ", ..."
                    } else {
                        ""
                    }
                ));
            }
            if is_type || stats.count < 2 {
                continue;
            }
            if field_stats.zero == field_stats.present {
                field_stats.flags.push("zero in every payload".to_string());
            }
        }
        out.messages.push(stats);
    }
    out
}

impl Stats {
    /// Every flag, as `interface/message[/field]: flag`.
    pub fn flags(&self) -> Vec<String> {
        let mut flags = Vec::new();
        for message in &self.messages {
            for flag in &message.flags {
                flags.push(format!("{}/{}: {flag}", self.interface, message.name));
            }
            for field in &message.fields {
                for flag in &field.flags {
                    flags.push(format!(
                        "{}/{}/{}: {flag}",
                        self.interface, message.name, field.name
                    ));
                }
            }
        }
        flags
    }

    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "interface": self.interface,
            "payloads": self.payloads,
            "unreadable": self.unreadable,
            "unknown_types": self.unknown_types.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(),
            "undefined": self.undefined,
            "messages": self.messages.iter().map(|m| json!({
                "message": m.name,
                "count": m.count,
                "lengths": m.lengths.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(),
                "flags": m.flags,
                "fields": m.fields.iter().map(|f| json!({
                    "field": f.name,
                    "present": f.present,
                    "absent": f.absent,
                    "distinct": f.values.len(),
                    "truncated": f.truncated,
                    "min": f.min,
                    "max": f.max,
                    "zero": f.zero,
                    "values": top(&f.values, 10).into_iter().map(|(value, count)| json!([value, count])).collect::<Vec<_>>(),
                    "unlabelled": f.unlabelled,
                    "flags": f.flags,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }

    pub fn to_text(&self) -> String {
        let mut out = format!("{}: {}\n", self.interface, n(self.payloads));
        if self.unreadable > 0 {
            out.push_str(&format!(
                "  {} could not be identified (too short, or no frame identifier)\n",
                self.unreadable
            ));
        }
        for (code, count) in &self.unknown_types {
            out.push_str(&format!(
                "  type {code}: {}, not named by the interface
",
                n(*count)
            ));
        }
        for (name, count) in &self.undefined {
            out.push_str(&format!(
                "  {name}: {}, named and not defined
",
                n(*count)
            ));
        }
        for message in &self.messages {
            let lengths: Vec<String> = message
                .lengths
                .iter()
                .map(|(length, count)| format!("{count} of {length} bytes"))
                .collect();
            out.push_str(&format!(
                "{}/{}: {} ({})\n",
                self.interface,
                message.name,
                n(message.count),
                lengths.join(", ")
            ));
            let width = message
                .fields
                .iter()
                .map(|f| f.name.len())
                .max()
                .unwrap_or(0);
            for field in &message.fields {
                let shown: Vec<String> = top(&field.values, 4)
                    .into_iter()
                    .map(|(value, count)| format!("{value} x{count}"))
                    .collect();
                let range = match (field.min, field.max) {
                    (Some(min), Some(max)) if field.values.len() > 4 => {
                        format!("  min {min} max {max}")
                    }
                    _ => String::new(),
                };
                out.push_str(&format!(
                    "  {:<width$}  {:>5} distinct  {}{}{range}\n",
                    field.name,
                    if field.truncated {
                        format!(">{MAX_DISTINCT}")
                    } else {
                        field.values.len().to_string()
                    },
                    shown.join(", "),
                    if field.values.len() > 4 { ", ..." } else { "" },
                ));
            }
        }
        let flags = self.flags();
        out.push_str(&format!("{} to look at\n", flags.len()));
        for flag in flags {
            out.push_str(&format!("  {flag}\n"));
        }
        out
    }
}

/// The `limit` most frequent values, most frequent first, ties by value.
fn top(values: &BTreeMap<String, usize>, limit: usize) -> Vec<(&str, usize)> {
    let mut all: Vec<(&str, usize)> = values.iter().map(|(v, c)| (v.as_str(), *c)).collect();
    all.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    all.truncate(limit);
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Dictionary;

    #[test]
    fn payload_lines() {
        let payloads = parse_payloads("; comment\n01 02\n\n100#E02E\n").unwrap();
        assert_eq!(
            payloads,
            [
                Payload {
                    frame_id: None,
                    bytes: vec![1, 2]
                },
                Payload {
                    frame_id: Some(0x100),
                    bytes: vec![0xE0, 0x2E]
                },
            ]
        );
        assert!(parse_payloads("0g").unwrap_err().starts_with("line 1"));
        assert!(parse_payloads("zz#00")
            .unwrap_err()
            .contains("frame identifier"));
    }

    #[test]
    fn suspicious_fields_are_flagged() {
        let dictionary = Dictionary::from_files([
            (
                "A/metadata.json",
                r#"{"message_type": {"start": 0, "stop": 8, "values": {"1": "M", "2": "Named"}}}"#,
            ),
            (
                "A/messages/M.json",
                r#"{"fields": {
                    "kind": {"start": 0, "stop": 8},
                    "mode": {"start": 8, "stop": 16, "values": {"0": "Off", "1": "On"}},
                    "level": {"start": 16, "stop": 24},
                    "dead": {"start": 24, "stop": 32},
                    "spare_32": {"start": 32, "stop": 36},
                    "tail": {"start": 40, "stop": 48}
                }}"#,
            ),
        ])
        .unwrap();
        let payloads = parse_payloads(
            "01 00 05 00 00 07\n01 01 09 00 10 07\n01 07 0a 00 08 07 ff\n01 00 05\n02 00\n09\n",
        )
        .unwrap();
        let stats = stats(dictionary.interface("A").unwrap(), &|_| Some(48), &payloads);
        assert_eq!(stats.payloads, 6);
        assert_eq!(stats.unknown_types[&9], 1);
        assert_eq!(stats.undefined["Named"], 1);
        let flags = stats.flags();
        let expect = [
            "A/M: length differs from the declared 6 bytes in 2 payloads",
            "A/M: 1 payload with non-zero bytes after the last field (byte 6)",
            "A/M: bits [36, 40) belong to no field and are non-zero in 1 payload",
            "A/M/mode: 1 payload with a value that has no label: 7",
            "A/M/dead: zero in every payload",
            "A/M/spare_32: named as having no meaning and non-zero in 1 payload",
        ];
        assert_eq!(flags, expect, "{flags:#?}");
        let m = &stats.messages[0];
        assert_eq!((m.fields[2].min, m.fields[2].max), (Some(5.0), Some(10.0)));
        assert_eq!(m.fields[5].absent, 1);
        assert!(stats.to_text().contains("6 to look at"));
    }
}
