//! The conventions of `docs/icd-to-dictionary/02-conventions.md` as checks.

use super::json::Node;
use super::model::{fields, interface_source, is_filler_name, InterfaceTree, SourceRef, Tree};
use super::rules::{Finding, LintConfig, UNITS};
use crate::schema::parse_int;

/// Every finding of the naming, unit, label, provenance, description, spelling,
/// header and vector rules. Allowed findings are still in the result;
/// [`super::rules::sort`] separates them.
pub fn lint(tree: &Tree, config: &LintConfig) -> Vec<Finding> {
    let mut findings = Vec::new();
    dictionary(tree, &mut findings);
    for interface in &tree.interfaces {
        self::interface(tree, interface, config, &mut findings);
    }
    for (name, vector) in &tree.vectors {
        let at = format!("vectors/{name}");
        let expect = vector.get("expect");
        let has_type = expect.and_then(|e| e.str_at("message_type")).is_some();
        let has_fields = expect
            .and_then(|e| e.get("fields"))
            .is_some_and(|f| !f.entries().is_empty());
        if !has_type && !has_fields {
            findings.push(Finding::new("V001", at.clone(), "expects nothing"));
        }
        let noted = vector.str_at("note").is_some_and(|n| !n.trim().is_empty());
        if !noted && !vector.get("source").is_some_and(Node::is_object) {
            findings.push(Finding::new(
                "V002",
                at,
                "does not say where its expected values come from",
            ));
        }
    }
    findings
}

fn is_semver(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.chars().all(|c| c.is_ascii_digit())
                && (*part == "0" || !part.starts_with('0'))
        })
}

fn sentence(text: &str, at: &str, what: &str, findings: &mut Vec<Finding>) {
    let ok = text.trim() == text
        && text.ends_with('.')
        && text
            .chars()
            .next()
            .is_some_and(|c| c.is_uppercase() || c.is_ascii_digit());
    if !ok {
        findings.push(Finding::new(
            "D002",
            at,
            format!("{what} {text:?} is not a sentence"),
        ));
    }
}

fn dictionary(tree: &Tree, findings: &mut Vec<Finding>) {
    let at = "dictionary.json";
    let Some(node) = &tree.dictionary else {
        findings.push(Finding::new("S003", at, "there is no dictionary.json"));
        return;
    };
    if node
        .str_at("name")
        .is_none_or(|name| name.trim().is_empty())
    {
        findings.push(Finding::new("S003", at, "no name"));
    }
    match node.str_at("version") {
        None => findings.push(Finding::new("S003", at, "no version")),
        Some(version) if !is_semver(version) => findings.push(Finding::new(
            "S003",
            at,
            format!("version {version:?} is not MAJOR.MINOR.PATCH"),
        )),
        Some(_) => {}
    }
    if let Some(text) = node.str_at("description") {
        sentence(text, at, "description", findings);
    }
}

fn is_interface_name(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn is_message_name(name: &str) -> bool {
    !name.is_empty()
        && name.split(' ').all(|word| {
            !word.is_empty()
                && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !word.starts_with('-')
                && !word.ends_with('-')
        })
}

fn is_field_name(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && name.split('_').all(|word| {
            !word.is_empty()
                && word
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// Whether a label stands for "no meaning assigned".
fn is_placeholder_label(label: &str) -> bool {
    const WORDS: [&str; 11] = [
        "reserved",
        "spare",
        "unused",
        "undefined",
        "not used",
        "not defined",
        "not assigned",
        "unassigned",
        "n/a",
        "tbd",
        "rsvd",
    ];
    let lower = label.trim().to_ascii_lowercase();
    WORDS.contains(&lower.as_str())
}

fn interface(
    tree: &Tree,
    interface: &InterfaceTree,
    config: &LintConfig,
    findings: &mut Vec<Finding>,
) {
    let at = interface.name.as_str();
    if !is_interface_name(at) {
        findings.push(Finding::new(
            "N001",
            at,
            "the interface name is not UPPER_CASE",
        ));
    }
    let source = interface_source(tree, interface);
    if !source.has_document() {
        findings.push(Finding::new(
            "P001",
            at,
            "no source document and revision, here or in dictionary.json",
        ));
    }
    match interface.metadata.str_at("description") {
        Some(text) => sentence(text, at, "description", findings),
        None => match interface.metadata.str_at("mission") {
            Some(text) => sentence(text, at, "description", findings),
            None => findings.push(Finding::new("D001", at, "the interface has no description")),
        },
    }
    radix(
        interface
            .metadata
            .get("message_type")
            .and_then(|node| node.get("values")),
        at,
        findings,
    );

    let mut lower: Vec<(String, &str)> = interface
        .messages
        .iter()
        .map(|(name, _)| (name.to_lowercase(), name.as_str()))
        .collect();
    lower.sort();
    for pair in lower.windows(2) {
        if pair[0].0 == pair[1].0 {
            findings.push(Finding::new(
                "N005",
                format!("{at}/{}", pair[1].1),
                format!("differs from {:?} only by letter case", pair[0].1),
            ));
        }
    }

    // The header: the message_type bits when they are in the payload, and the
    // message_length bits.
    let range_of = |key: &str| -> Option<(u64, u64)> {
        let node = interface.metadata.get(key)?;
        if key == "message_type" && node.str_at("source") == Some("frame_id") {
            return None;
        }
        Some((node.u64_at("start")?, node.u64_at("stop")?)).filter(|(start, stop)| stop > start)
    };
    let header: Vec<(&str, (u64, u64))> = ["message_type", "message_length"]
        .into_iter()
        .filter_map(|key| Some((key, range_of(key)?)))
        .collect();
    let mut header_names: Vec<Option<&str>> = vec![None; header.len()];

    let interface_byte_order = interface.metadata.str_at("byte_order").unwrap_or("big");
    for (name, message) in &interface.messages {
        let at = format!("{}/{name}", interface.name);
        if !is_message_name(name) {
            findings.push(Finding::new(
                "N002",
                at.clone(),
                "the message name is not words of letters, digits and hyphens with single spaces",
            ));
        }
        match message.str_at("description") {
            Some(text) => sentence(text, &at, "description", findings),
            None => findings.push(Finding::new(
                "D001",
                at.clone(),
                "the message has no description",
            )),
        }
        if !SourceRef::read(message).has_locator() {
            findings.push(Finding::new(
                "P002",
                at.clone(),
                "no source with a section, a table or a page",
            ));
        }
        let views = fields(message);
        for (index, (key, range)) in header.iter().enumerate() {
            match views.iter().find(|view| view.range() == Some(*range)) {
                None => findings.push(Finding::new(
                    "H001",
                    at.clone(),
                    format!("no field on the {key} bits [{}, {})", range.0, range.1),
                )),
                Some(view) => match header_names[index] {
                    None => header_names[index] = Some(view.name),
                    Some(known) if known != view.name => findings.push(Finding::new(
                        "H001",
                        format!("{at}/{}", view.name),
                        format!(
                            "the field on the {key} bits is named {known} in another message of {}",
                            interface.name
                        ),
                    )),
                    Some(_) => {}
                },
            }
        }
        for view in views {
            let at = format!("{at}/{}", view.name);
            if !is_field_name(view.name) {
                findings.push(Finding::new(
                    "N003",
                    at.clone(),
                    "the field name is not snake_case",
                ));
            }
            if is_filler_name(view.name) {
                let start = view.start.unwrap_or(0);
                if view.name != format!("spare_{start}") && view.name != format!("reserved_{start}")
                {
                    findings.push(Finding::new(
                        "N004",
                        at.clone(),
                        format!("bits with no meaning are named spare_{start} or reserved_{start}"),
                    ));
                }
            }
            field(
                view.node,
                &at,
                interface_byte_order,
                view.kind(),
                config,
                findings,
            );
        }
    }
}

fn field(
    node: &Node,
    at: &str,
    interface_byte_order: &str,
    kind: &str,
    config: &LintConfig,
    findings: &mut Vec<Finding>,
) {
    if let Some(unit) = node.str_at("unit") {
        let known = UNITS.iter().any(|(known, _)| *known == unit)
            || config.units.iter().any(|known| known == unit);
        if !known {
            findings.push(Finding::new(
                "U001",
                at,
                format!("unit {unit:?} is not in the vocabulary"),
            ));
        }
    } else if node.get("scale").is_some() || node.get("offset").is_some() || kind == "float" {
        findings.push(Finding::new(
            "U002",
            at,
            "a scaled or float field with no unit",
        ));
    }
    if let Some(text) = node.str_at("description") {
        sentence(text, at, "description", findings);
    }
    if node.get("signed").is_some() {
        findings.push(Finding::new(
            "S001",
            at,
            "write \"type\": \"int\" for a signed integer and leave signed out",
        ));
    }
    let mut defaults = Vec::new();
    if node.str_at("type") == Some("uint") {
        defaults.push("\"type\": \"uint\"");
    }
    if node.str_at("byte_order") == Some(interface_byte_order) {
        defaults.push("byte_order");
    }
    if node.f64_at("scale") == Some(1.0) {
        defaults.push("\"scale\": 1");
    }
    if node.f64_at("offset") == Some(0.0) {
        defaults.push("\"offset\": 0");
    }
    for key in defaults {
        findings.push(Finding::new(
            "S002",
            at,
            format!("{key} repeats the default"),
        ));
    }

    let values = node.get("values");
    radix(values, at, findings);
    let mut seen: Vec<&str> = Vec::new();
    for (key, label) in values.map(Node::entries).unwrap_or_default() {
        let Some(label) = label.as_str() else {
            continue;
        };
        if label.is_empty() || label.trim() != label {
            findings.push(Finding::new(
                "L001",
                at,
                format!("the label of {key} is empty or has space at an end: {label:?}"),
            ));
        }
        if is_placeholder_label(label) {
            findings.push(Finding::new(
                "L003",
                at,
                format!("value {key} is labelled {label:?}; leave unassigned values out"),
            ));
        } else if seen.contains(&label) {
            findings.push(Finding::new(
                "L002",
                at,
                format!("the label {label:?} is on more than one value"),
            ));
        }
        seen.push(label);
    }
}

/// L004: the keys of one `values` object in one radix.
fn radix(values: Option<&Node>, at: &str, findings: &mut Vec<Finding>) {
    let keys: Vec<&str> = values
        .map(Node::entries)
        .unwrap_or_default()
        .iter()
        .map(|(key, _)| key.trim())
        .filter(|key| parse_int(key).is_some())
        .collect();
    let hex = keys
        .iter()
        .filter(|key| {
            key.trim_start_matches('-')
                .to_ascii_lowercase()
                .starts_with("0x")
        })
        .count();
    if hex != 0 && hex != keys.len() {
        findings.push(Finding::new(
            "L004",
            at,
            format!(
                "values has {hex} hexadecimal and {} decimal keys",
                keys.len() - hex
            ),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::model::FileSet;

    fn rules_for(files: &[(&str, &str)]) -> Vec<String> {
        let files: FileSet = files
            .iter()
            .map(|(path, text)| (path.to_string(), text.to_string()))
            .collect();
        let tree = Tree::parse(&files).unwrap();
        let config = LintConfig::read(tree.dictionary.as_ref());
        lint(&tree, &config)
            .into_iter()
            .map(|finding| format!("{} {}", finding.rule, finding.location))
            .collect()
    }

    const CLEAN_DICTIONARY: &str = r#"{"name": "x", "version": "1.0.0", "description": "An example.",
        "source": {"document": "DOC-1", "revision": "A"}}"#;
    const CLEAN_METADATA: &str = r#"{"description": "A link.",
        "message_type": {"start": 0, "stop": 8, "values": {"1": "Status Report"}},
        "message_length": {"start": 8, "stop": 16}}"#;

    #[test]
    fn a_dictionary_that_follows_the_conventions_has_no_findings() {
        let found = rules_for(&[
            ("dictionary.json", CLEAN_DICTIONARY),
            ("LINK_A/metadata.json", CLEAN_METADATA),
            (
                "LINK_A/messages/Status Report.json",
                r#"{"description": "Sent once a second.", "source": {"table": "4-1"}, "fields": {
                    "message_id": {"start": 0, "stop": 8},
                    "length": {"start": 8, "stop": 16, "unit": "words"},
                    "speed": {"start": 16, "stop": 32, "type": "int", "scale": 0.1, "unit": "m/s",
                              "description": "Speed over ground."},
                    "mode": {"start": 32, "stop": 36, "values": {"0": "Idle", "1": "Run"}},
                    "spare_36": {"start": 36, "stop": 40}
                }}"#,
            ),
            (
                "vectors/status.json",
                r#"{"interface": "LINK_A", "payload_hex": "01", "note": "From table 4-2.",
                    "expect": {"message_type": "Status Report"}}"#,
            ),
        ]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn each_rule_fires_on_its_defect() {
        let found = rules_for(&[
            (
                "dictionary.json",
                r#"{"version": "1.0", "description": "no stop"}"#,
            ),
            (
                "link/metadata.json",
                r#"{"message_type": {"start": 0, "stop": 8, "values": {"1": "Status", "0x2": "status"}},
                    "message_length": {"start": 8, "stop": 16}}"#,
            ),
            (
                "link/messages/Status.json",
                r#"{"fields": {
                    "message_id": {"start": 0, "stop": 8},
                    "Speed": {"start": 16, "stop": 32, "signed": true, "scale": 1, "offset": 0, "byte_order": "big"},
                    "range": {"start": 32, "stop": 48, "scale": 0.5},
                    "height": {"start": 48, "stop": 64, "unit": "furlongs", "type": "uint",
                               "description": "height above ground"},
                    "mode": {"start": 64, "stop": 68,
                             "values": {"0": "Idle", "1": "Idle", "2": " Run", "3": "Reserved", "0x4": "Go"}},
                    "reserved": {"start": 68, "stop": 72}
                }}"#,
            ),
            (
                "link/messages/status.json",
                r#"{"description": "Second.", "source": {"page": 4}, "fields": {
                    "msg_type": {"start": 0, "stop": 8}, "length": {"start": 8, "stop": 16}}}"#,
            ),
            (
                "link/messages/Bad  Name.json",
                r#"{"description": "X.", "source": {"page": 4}, "fields": {}}"#,
            ),
            (
                "vectors/empty.json",
                r#"{"interface": "link", "payload_hex": "", "expect": {}}"#,
            ),
        ]);
        let expect = [
            "S003 dictionary.json",
            "D002 dictionary.json",
            "N001 link",
            "P001 link",
            "D001 link",
            "L004 link",
            "N005 link/status",
            "N002 link/Bad  Name",
            "H001 link/Bad  Name",
            "D001 link/Status",
            "P002 link/Status",
            "H001 link/Status",
            "N003 link/Status/Speed",
            "S001 link/Status/Speed",
            "S002 link/Status/Speed",
            "U002 link/Status/range",
            "U001 link/Status/height",
            "D002 link/Status/height",
            "L002 link/Status/mode",
            "L001 link/Status/mode",
            "L003 link/Status/mode",
            "L004 link/Status/mode",
            "N004 link/Status/reserved",
            "H001 link/status/msg_type",
            "V001 vectors/empty",
            "V002 vectors/empty",
        ];
        for line in expect {
            assert!(
                found.iter().any(|f| f == line),
                "missing {line} in {found:#?}"
            );
        }
        // S002 fires once for each of the three defaults on Speed and once on height.
        assert_eq!(found.iter().filter(|f| f.starts_with("S002")).count(), 4);
        // S003 fires for the missing name and the two-part version.
        assert_eq!(found.iter().filter(|f| f.starts_with("S003")).count(), 2);
    }

    #[test]
    fn an_added_unit_is_accepted() {
        let found = rules_for(&[
            (
                "dictionary.json",
                r#"{"name": "x", "version": "1.0.0", "lint": {"units": ["furlongs"]},
                "source": {"document": "D", "revision": "A"}}"#,
            ),
            (
                "A/metadata.json",
                r#"{"description": "A.", "message_type": {"source": "frame_id"}}"#,
            ),
            (
                "A/messages/M.json",
                r#"{"description": "M.", "source": {"section": "2"},
                "fields": {"x": {"start": 0, "stop": 8, "unit": "furlongs"}}}"#,
            ),
        ]);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn names() {
        assert!(is_interface_name("WX") && is_interface_name("LINK_16A"));
        assert!(!is_interface_name("Wx") && !is_interface_name("1A") && !is_interface_name(""));
        assert!(is_message_name("Keep Alive") && is_message_name("Type-3 Report"));
        assert!(
            !is_message_name("Keep  Alive") && !is_message_name(" Keep") && !is_message_name("A/B")
        );
        assert!(is_field_name("wind_speed_2") && is_field_name("x"));
        assert!(
            !is_field_name("windSpeed")
                && !is_field_name("wind__speed")
                && !is_field_name("_x")
                && !is_field_name("2x")
        );
        assert!(is_semver("1.0.0") && is_semver("0.12.3"));
        assert!(!is_semver("1.0") && !is_semver("1.0.0-rc1") && !is_semver("01.0.0"));
    }
}
