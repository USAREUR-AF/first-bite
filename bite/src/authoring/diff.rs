//! What changed between two versions of a dictionary, in terms of what a
//! consumer of the decoded records sees.

use super::json::{number_text, Node};
use super::model::{fields, is_filler_name, FieldView, InterfaceTree, Tree};
use crate::schema::parse_int;
use serde_json::json;
use std::collections::BTreeMap;

/// How much a change asks of a consumer of the decoded records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Impact {
    /// Descriptions, sources, notes: the records are the same.
    Documentation,
    /// Something new that an existing consumer can ignore.
    Additive,
    /// The same names and types, different values for the same bytes: bits
    /// moved, a scale corrected.
    Value,
    /// A name, a type, a unit or a label an existing consumer may rely on is
    /// gone or means something else.
    Breaking,
}

impl Impact {
    pub fn word(self) -> &'static str {
        match self {
            Impact::Documentation => "documentation",
            Impact::Additive => "additive",
            Impact::Value => "value",
            Impact::Breaking => "breaking",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub impact: Impact,
    /// `added`, `removed`, `moved`, `resized`, `retyped`, `rescaled`,
    /// `relabelled`, `renamed`, `changed` or `documented`.
    pub kind: &'static str,
    pub location: String,
    pub detail: String,
}

/// The version part a set of changes asks to be raised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bump {
    None,
    Patch,
    Minor,
    Major,
}

impl Bump {
    pub fn word(self) -> &'static str {
        match self {
            Bump::None => "none",
            Bump::Patch => "patch",
            Bump::Minor => "minor",
            Bump::Major => "major",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diff {
    pub changes: Vec<Change>,
    pub old_version: Option<String>,
    pub new_version: Option<String>,
}

fn version(text: &str) -> Option<[u64; 3]> {
    let parts: Vec<u64> = text
        .split('.')
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    <[u64; 3]>::try_from(parts).ok()
}

impl Diff {
    /// The greatest impact among the changes.
    pub fn impact(&self) -> Option<Impact> {
        self.changes.iter().map(|change| change.impact).max()
    }

    pub fn is_breaking(&self) -> bool {
        self.impact() == Some(Impact::Breaking)
    }

    /// The version part these changes ask for: major for a breaking change,
    /// minor for an additive or value change, patch for documentation.
    pub fn required_bump(&self) -> Bump {
        match self.impact() {
            None => Bump::None,
            Some(Impact::Documentation) => Bump::Patch,
            Some(Impact::Additive | Impact::Value) => Bump::Minor,
            Some(Impact::Breaking) => Bump::Major,
        }
    }

    /// The version part that was raised between the two dictionaries, when
    /// both state a `MAJOR.MINOR.PATCH` version.
    pub fn actual_bump(&self) -> Option<Bump> {
        let old = version(self.old_version.as_deref()?)?;
        let new = version(self.new_version.as_deref()?)?;
        Some(if new[0] > old[0] {
            Bump::Major
        } else if new[0] == old[0] && new[1] > old[1] {
            Bump::Minor
        } else if new[..2] == old[..2] && new[2] > old[2] {
            Bump::Patch
        } else {
            Bump::None
        })
    }

    /// Whether the new version was raised by at least what the changes ask.
    /// `None` when a version is missing or not `MAJOR.MINOR.PATCH`.
    pub fn version_is_sufficient(&self) -> Option<bool> {
        Some(self.actual_bump()? >= self.required_bump())
    }

    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "old_version": self.old_version,
            "new_version": self.new_version,
            "impact": self.impact().map(Impact::word),
            "breaking": self.is_breaking(),
            "required_bump": self.required_bump().word(),
            "actual_bump": self.actual_bump().map(Bump::word),
            "version_is_sufficient": self.version_is_sufficient(),
            "changes": self.changes.iter().map(|change| json!({
                "impact": change.impact.word(),
                "kind": change.kind,
                "location": change.location,
                "detail": change.detail,
            })).collect::<Vec<_>>(),
        })
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for impact in [
            Impact::Breaking,
            Impact::Value,
            Impact::Additive,
            Impact::Documentation,
        ] {
            let changes: Vec<&Change> = self
                .changes
                .iter()
                .filter(|change| change.impact == impact)
                .collect();
            if changes.is_empty() {
                continue;
            }
            out.push_str(&format!("{} ({})\n", impact.word(), changes.len()));
            for change in changes {
                out.push_str(&format!(
                    "  {:<10} {}: {}\n",
                    change.kind, change.location, change.detail
                ));
            }
        }
        if self.changes.is_empty() {
            out.push_str("no changes\n");
        }
        let show = |version: &Option<String>| version.clone().unwrap_or_else(|| "none".to_string());
        out.push_str(&format!(
            "version {} -> {}; the changes ask for a {} version",
            show(&self.old_version),
            show(&self.new_version),
            match self.required_bump() {
                Bump::None => "unchanged".to_string(),
                bump => format!("new {}", bump.word()),
            },
        ));
        out.push_str(match self.version_is_sufficient() {
            Some(true) => ": sufficient\n",
            Some(false) => ": NOT sufficient\n",
            None => "\n",
        });
        out
    }
}

struct Collector {
    changes: Vec<Change>,
}

impl Collector {
    fn push(&mut self, impact: Impact, kind: &'static str, location: &str, detail: String) {
        self.changes.push(Change {
            impact,
            kind,
            location: location.to_string(),
            detail,
        });
    }
}

fn show(node: Option<&Node>) -> String {
    node.map_or("none".to_string(), Node::to_line)
}

/// Whether two optional nodes are equal, with numbers compared by value.
fn same(a: Option<&Node>, b: Option<&Node>) -> bool {
    match (a, b) {
        (Some(Node::Number(x)), Some(Node::Number(y))) => number_text(x) == number_text(y),
        (Some(Node::Object(x)), Some(Node::Object(y))) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(key, value)| same(Some(value), b.and_then(|node| node.get(key))))
        }
        _ => a == b,
    }
}

/// Compare two dictionaries.
pub fn diff(old: &Tree, new: &Tree) -> Diff {
    let mut c = Collector {
        changes: Vec::new(),
    };
    let version_of = |tree: &Tree| {
        tree.dictionary
            .as_ref()
            .and_then(|node| node.str_at("version"))
            .map(str::to_string)
    };
    documentation(
        old.dictionary.as_ref(),
        new.dictionary.as_ref(),
        &["name", "description", "source"],
        "dictionary.json",
        &mut c,
    );
    for interface in &old.interfaces {
        match new.interface(&interface.name) {
            None => c.push(
                Impact::Breaking,
                "removed",
                &interface.name,
                "interface removed".to_string(),
            ),
            Some(other) => interfaces(interface, other, &mut c),
        }
    }
    for interface in &new.interfaces {
        if old.interface(&interface.name).is_none() {
            c.push(
                Impact::Additive,
                "added",
                &interface.name,
                format!("interface added with {} messages", interface.messages.len()),
            );
        }
    }
    Diff {
        changes: c.changes,
        old_version: version_of(old),
        new_version: version_of(new),
    }
}

/// Report keys that only document, when they differ.
fn documentation(
    old: Option<&Node>,
    new: Option<&Node>,
    keys: &[&str],
    at: &str,
    c: &mut Collector,
) {
    for key in keys {
        let (a, b) = (old.and_then(|n| n.get(key)), new.and_then(|n| n.get(key)));
        if !same(a, b) {
            c.push(
                Impact::Documentation,
                "documented",
                at,
                format!("{key}: {} -> {}", show(a), show(b)),
            );
        }
    }
}

/// `message_type.values` by number.
fn type_ids(interface: &InterfaceTree) -> BTreeMap<i128, (&str, &str)> {
    interface
        .type_values()
        .into_iter()
        .filter_map(|(key, name)| Some((parse_int(key)?, (key, name))))
        .collect()
}

fn interfaces(old: &InterfaceTree, new: &InterfaceTree, c: &mut Collector) {
    let at = old.name.as_str();
    // Keys that decide how every message of the interface is read.
    for key in ["bit_order", "byte_order", "message_length"] {
        let (a, b) = (old.metadata.get(key), new.metadata.get(key));
        if !same(a, b) {
            c.push(
                Impact::Value,
                "changed",
                at,
                format!("{key}: {} -> {}", show(a), show(b)),
            );
        }
    }
    let (a, b) = (old.metadata.get("port"), new.metadata.get("port"));
    if !same(a, b) {
        c.push(
            Impact::Value,
            "changed",
            at,
            format!("port: {} -> {}", show(a), show(b)),
        );
    }
    let (a, b) = (
        old.metadata.get("message_type"),
        new.metadata.get("message_type"),
    );
    for key in ["source", "start", "stop", "mask"] {
        let (a, b) = (a.and_then(|n| n.get(key)), b.and_then(|n| n.get(key)));
        if !same(a, b) {
            c.push(
                Impact::Value,
                "changed",
                at,
                format!("message_type.{key}: {} -> {}", show(a), show(b)),
            );
        }
    }
    let mut metadata_keys = vec![
        "description",
        "icd",
        "source",
        "note",
        "protocol",
        "icd_numbering",
    ];
    if old.metadata.get("description").is_none() || new.metadata.get("description").is_none() {
        metadata_keys.push("mission");
    }
    documentation(
        Some(&old.metadata),
        Some(&new.metadata),
        &metadata_keys,
        at,
        c,
    );

    let (old_ids, new_ids) = (type_ids(old), type_ids(new));
    for (number, (key, name)) in &old_ids {
        match new_ids.get(number) {
            None => c.push(
                Impact::Breaking,
                "removed",
                at,
                format!("message_type {key} no longer names {name:?}"),
            ),
            Some((_, other)) if other != name => c.push(
                Impact::Breaking,
                "renamed",
                at,
                format!("message_type {key}: {name:?} -> {other:?}"),
            ),
            Some(_) => {}
        }
    }
    for (number, (key, name)) in &new_ids {
        if !old_ids.contains_key(number) {
            c.push(
                Impact::Additive,
                "added",
                at,
                format!("message_type {key} names {name:?}"),
            );
        }
    }

    for (name, message) in &old.messages {
        let at = format!("{}/{name}", old.name);
        match new.message(name) {
            None => c.push(
                Impact::Breaking,
                "removed",
                &at,
                "message definition removed".to_string(),
            ),
            Some(other) => messages(message, other, &at, c),
        }
    }
    for (name, message) in &new.messages {
        if old.message(name).is_none() {
            c.push(
                Impact::Additive,
                "added",
                &format!("{}/{name}", new.name),
                format!("message defined with {} fields", fields(message).len()),
            );
        }
    }
}

fn range_text(view: &FieldView<'_>) -> String {
    let show = |value: Option<u64>| value.map_or("?".to_string(), |v| v.to_string());
    format!("[{}, {})", show(view.start), show(view.stop))
}

fn messages(old: &Node, new: &Node, at: &str, c: &mut Collector) {
    let (a, b) = (old.get("declared_bits"), new.get("declared_bits"));
    if !same(a, b) {
        c.push(
            Impact::Value,
            "resized",
            at,
            format!("declared_bits: {} -> {}", show(a), show(b)),
        );
    }
    documentation(
        Some(old),
        Some(new),
        &["description", "source", "note"],
        at,
        c,
    );
    let (old_fields, new_fields) = (fields(old), fields(new));
    for field in &old_fields {
        let at = format!("{at}/{}", field.name);
        let filler = is_filler_name(field.name);
        match new_fields.iter().find(|other| other.name == field.name) {
            None => {
                // A field at the same bits under a new name is a rename.
                let successor = new_fields.iter().find(|other| {
                    other.range().is_some()
                        && other.range() == field.range()
                        && !old_fields.iter().any(|known| known.name == other.name)
                });
                let impact = if filler {
                    Impact::Additive
                } else {
                    Impact::Breaking
                };
                match successor {
                    Some(other) => c.push(
                        impact,
                        "renamed",
                        &at,
                        format!("now {} at the same bits {}", other.name, range_text(field)),
                    ),
                    None => c.push(
                        impact,
                        "removed",
                        &at,
                        format!("field removed from {}", range_text(field)),
                    ),
                }
            }
            Some(other) => field_change(field, other, &at, filler, c),
        }
    }
    for field in &new_fields {
        if old_fields.iter().any(|other| other.name == field.name) {
            continue;
        }
        let renamed = old_fields.iter().any(|other| {
            other.range().is_some()
                && other.range() == field.range()
                && !new_fields.iter().any(|known| known.name == other.name)
        });
        if !renamed {
            c.push(
                Impact::Additive,
                "added",
                &format!("{at}/{}", field.name),
                format!("field added at {}", range_text(field)),
            );
        }
    }
}

fn field_change(
    old: &FieldView<'_>,
    new: &FieldView<'_>,
    at: &str,
    filler: bool,
    c: &mut Collector,
) {
    // A spare that moves or changes carries nothing a consumer relies on.
    let cap = |impact: Impact| {
        if filler {
            Impact::Additive.min(impact)
        } else {
            impact
        }
    };
    if (old.start, old.stop) != (new.start, new.stop) {
        let width = |view: &FieldView<'_>| view.range().map(|(start, stop)| stop - start);
        let kind = if width(old) == width(new) {
            "moved"
        } else {
            "resized"
        };
        c.push(
            cap(Impact::Value),
            kind,
            at,
            format!("{} -> {}", range_text(old), range_text(new)),
        );
    }
    if old.kind() != new.kind() {
        c.push(
            cap(Impact::Breaking),
            "retyped",
            at,
            format!("{} -> {}", old.kind(), new.kind()),
        );
    }
    let (a, b) = (old.node.get("byte_order"), new.node.get("byte_order"));
    if !same(a, b) {
        c.push(
            cap(Impact::Value),
            "changed",
            at,
            format!("byte_order: {} -> {}", show(a), show(b)),
        );
    }
    let scaling = |view: &FieldView<'_>| {
        (
            view.node.f64_at("scale").unwrap_or(1.0),
            view.node.f64_at("offset").unwrap_or(0.0),
        )
    };
    let (a, b) = (scaling(old), scaling(new));
    if a != b {
        // Gaining or losing a scale turns an integer into a number.
        let has = |view: &FieldView<'_>| {
            view.node.get("scale").is_some() || view.node.get("offset").is_some()
        };
        let impact = if has(old) != has(new) && old.kind() != "float" {
            Impact::Breaking
        } else {
            Impact::Value
        };
        c.push(
            cap(impact),
            "rescaled",
            at,
            format!(
                "scale {} offset {} -> scale {} offset {}",
                a.0, a.1, b.0, b.1
            ),
        );
    }
    let (a, b) = (old.node.get("unit"), new.node.get("unit"));
    if !same(a, b) {
        // Stating a unit where there was none takes nothing away.
        let impact = if a.is_none() {
            Impact::Documentation
        } else {
            Impact::Breaking
        };
        c.push(
            cap(impact),
            "changed",
            at,
            format!("unit: {} -> {}", show(a), show(b)),
        );
    }

    let labels = |view: &FieldView<'_>| -> BTreeMap<i128, String> {
        view.node
            .get("values")
            .map(Node::entries)
            .unwrap_or_default()
            .iter()
            .filter_map(|(key, label)| {
                let label = match label {
                    Node::String(text) => text.clone(),
                    Node::Bool(flag) => flag.to_string(),
                    Node::Number(number) => number_text(number),
                    _ => return None,
                };
                Some((parse_int(key)?, label))
            })
            .collect()
    };
    let (a, b) = (labels(old), labels(new));
    for (value, label) in &a {
        match b.get(value) {
            None => c.push(
                cap(Impact::Breaking),
                "relabelled",
                at,
                format!("value {value}: label {label:?} removed"),
            ),
            Some(other) if other != label => c.push(
                cap(Impact::Breaking),
                "relabelled",
                at,
                format!("value {value}: {label:?} -> {other:?}"),
            ),
            Some(_) => {}
        }
    }
    for (value, label) in &b {
        if !a.contains_key(value) {
            // A value that decoded as a number now decodes as a label.
            c.push(
                cap(Impact::Additive),
                "relabelled",
                at,
                format!("value {value}: label {label:?} added"),
            );
        }
    }
    documentation(
        Some(old.node),
        Some(new.node),
        &["description", "source", "note"],
        at,
        c,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::model::FileSet;

    fn tree(version: &str, metadata: &str, message: &str) -> Tree {
        let mut files = FileSet::new();
        files.insert(
            "dictionary.json".into(),
            format!(r#"{{"name": "x", "version": "{version}"}}"#),
        );
        files.insert("A/metadata.json".into(), metadata.into());
        files.insert("A/messages/M.json".into(), message.into());
        Tree::parse(&files).unwrap()
    }

    const METADATA: &str =
        r#"{"port": 1, "message_type": {"start": 0, "stop": 8, "values": {"1": "M"}}}"#;
    const MESSAGE: &str = r#"{"description": "M.", "fields": {
        "kind": {"start": 0, "stop": 8},
        "speed": {"start": 8, "stop": 24, "scale": 0.1, "unit": "m/s"},
        "mode": {"start": 24, "stop": 28, "values": {"0": "Idle", "1": "Run"}},
        "spare_28": {"start": 28, "stop": 32},
        "count": {"start": 32, "stop": 40}
    }}"#;

    fn kinds(diff: &Diff) -> Vec<String> {
        diff.changes
            .iter()
            .map(|c| format!("{} {} {}", c.impact.word(), c.kind, c.location))
            .collect()
    }

    #[test]
    fn identical_dictionaries_have_no_changes() {
        let d = diff(
            &tree("1.0.0", METADATA, MESSAGE),
            &tree("1.0.0", METADATA, MESSAGE),
        );
        assert!(d.changes.is_empty());
        assert_eq!(d.required_bump(), Bump::None);
        assert_eq!(d.version_is_sufficient(), Some(true));
        assert!(d.to_text().starts_with("no changes\n"));
    }

    #[test]
    fn every_kind_of_change_is_classified() {
        let new_metadata = r#"{"port": 2, "message_type": {"start": 0, "stop": 8, "values": {"1": "M", "2": "N"}}}"#;
        let new_message = r#"{"description": "M, revised.", "fields": {
            "kind": {"start": 0, "stop": 8},
            "speed": {"start": 8, "stop": 24, "scale": 0.01, "unit": "km/h"},
            "mode": {"start": 24, "stop": 27, "values": {"0": "Idle", "1": "Running", "2": "Fault"}},
            "armed": {"start": 27, "stop": 28, "type": "bool"},
            "spare_28": {"start": 28, "stop": 30},
            "total": {"start": 32, "stop": 40},
            "extra": {"start": 40, "stop": 48, "type": "int"}
        }}"#;
        let d = diff(
            &tree("1.0.0", METADATA, MESSAGE),
            &tree("1.1.0", new_metadata, new_message),
        );
        let found = kinds(&d);
        for line in [
            "value changed A",
            "additive added A",
            "documentation documented A/M",
            "value rescaled A/M/speed",
            "breaking changed A/M/speed",
            "value resized A/M/mode",
            "breaking relabelled A/M/mode",
            "additive relabelled A/M/mode",
            "additive resized A/M/spare_28",
            "breaking renamed A/M/count",
            "additive added A/M/armed",
            "additive added A/M/extra",
        ] {
            assert!(
                found.iter().any(|f| f == line),
                "missing {line} in {found:#?}"
            );
        }
        assert!(
            !found.iter().any(|f| f.ends_with("A/M/total")),
            "{found:#?}"
        );
        assert!(d.is_breaking());
        assert_eq!(d.required_bump(), Bump::Major);
        assert_eq!(d.actual_bump(), Some(Bump::Minor));
        assert_eq!(d.version_is_sufficient(), Some(false));
        assert!(d.to_text().ends_with(
            "version 1.0.0 -> 1.1.0; the changes ask for a new major version: NOT sufficient\n"
        ));
    }

    #[test]
    fn additions_and_documentation_are_not_breaking() {
        let more = MESSAGE.replace(
            r#""count": {"start": 32, "stop": 40}"#,
            r#""count": {"start": 32, "stop": 40, "description": "Counts.", "unit": "1"}, "new": {"start": 40, "stop": 48}"#,
        );
        let d = diff(
            &tree("1.0.0", METADATA, MESSAGE),
            &tree("1.1.0", METADATA, &more),
        );
        assert_eq!(d.impact(), Some(Impact::Additive));
        assert!(!d.is_breaking());
        assert_eq!(d.version_is_sufficient(), Some(true));
        let d = diff(
            &tree("1.0.0", METADATA, MESSAGE),
            &tree("1.0.0", METADATA, &more),
        );
        assert_eq!(d.version_is_sufficient(), Some(false));
    }

    #[test]
    fn a_number_written_two_ways_is_one_number() {
        let a = MESSAGE.replace("0.1", "1.0E-1");
        let d = diff(
            &tree("1.0.0", METADATA, MESSAGE),
            &tree("1.0.0", METADATA, &a),
        );
        assert!(d.changes.is_empty(), "{:#?}", d.changes);
    }

    #[test]
    fn bumps() {
        let bump = |old: &str, new: &str| {
            Diff {
                changes: vec![],
                old_version: Some(old.into()),
                new_version: Some(new.into()),
            }
            .actual_bump()
        };
        assert_eq!(bump("1.2.3", "2.0.0"), Some(Bump::Major));
        assert_eq!(bump("1.2.3", "1.3.0"), Some(Bump::Minor));
        assert_eq!(bump("1.2.3", "1.2.4"), Some(Bump::Patch));
        assert_eq!(bump("1.2.3", "1.2.3"), Some(Bump::None));
        assert_eq!(bump("1.2.3", "1.1.9"), Some(Bump::None));
        assert_eq!(bump("1.2", "1.3.0"), None);
    }
}
