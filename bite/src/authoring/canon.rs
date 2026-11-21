//! The canonical form of a dictionary: what `bite fmt` writes.
//!
//! Two people who transcribe the same definitions get the same bytes: keys in
//! one order, fields by start bit, enumerated values in numeric order, numbers
//! in one spelling. Nothing here changes what a payload decodes to.

use super::json::Node;
use super::model::{fields, FileSet, InterfaceTree, Tree, SOURCE_KEYS};
use crate::schema::parse_int;

const DICTIONARY_KEYS: [&str; 5] = ["name", "version", "description", "source", "lint"];
const METADATA_KEYS: [&str; 12] = [
    "description",
    "icd",
    "source",
    "note",
    "protocol",
    "port",
    "bit_order",
    "byte_order",
    "icd_numbering",
    "message_type",
    "message_length",
    "framing",
];
const MESSAGE_KEYS: [&str; 5] = ["description", "source", "note", "declared_bits", "fields"];
pub const FIELD_KEYS: [&str; 12] = [
    "start",
    "stop",
    "type",
    "signed",
    "byte_order",
    "scale",
    "offset",
    "unit",
    "values",
    "description",
    "source",
    "note",
];
const VECTOR_KEYS: [&str; 6] = [
    "interface",
    "frame_id",
    "payload_hex",
    "expect",
    "source",
    "note",
];
pub const NUMBERING_KEYS: [&str; 5] =
    ["word_bits", "bit0", "first_word", "first_bit", "word_order"];

/// `files` with every dictionary file rewritten in canonical form. Files that
/// are not part of the dictionary layout are left out of the result.
pub fn format(files: &FileSet) -> Result<FileSet, Vec<String>> {
    let tree = Tree::parse(files)?;
    Ok(canonical(&tree)?.to_files())
}

/// The canonical form of a tree.
pub fn canonical(tree: &Tree) -> Result<Tree, Vec<String>> {
    let mut errors = Vec::new();
    let dictionary = tree.dictionary.as_ref().map(dictionary_file);
    let interfaces: Vec<InterfaceTree> = tree
        .interfaces
        .iter()
        .map(|interface| {
            let mut messages: Vec<(String, Node)> = interface
                .messages
                .iter()
                .map(|(name, message)| {
                    let at = format!("{}/messages/{name}.json", interface.name);
                    (name.clone(), message_file(message, &at, &mut errors))
                })
                .collect();
            messages.sort_by(|a, b| a.0.cmp(&b.0));
            InterfaceTree {
                name: interface.name.clone(),
                metadata: metadata_file(
                    &interface.metadata,
                    &format!("{}/metadata.json", interface.name),
                    &mut errors,
                ),
                messages,
            }
        })
        .collect();
    let mut vectors: Vec<(String, Node)> = tree
        .vectors
        .iter()
        .map(|(name, vector)| (name.clone(), vector_file(vector, tree)))
        .collect();
    vectors.sort_by(|a, b| a.0.cmp(&b.0));
    if errors.is_empty() {
        Ok(Tree {
            dictionary,
            interfaces,
            vectors,
        })
    } else {
        Err(errors)
    }
}

/// The members of an object with the known keys first, in the order given,
/// then every other key in alphabetical order. `each` rewrites a member.
fn ordered(node: &Node, known: &[&str], mut each: impl FnMut(&str, &Node) -> Option<Node>) -> Node {
    let Node::Object(entries) = node else {
        return node.clone();
    };
    let mut out: Vec<(String, Node)> = Vec::new();
    for key in known {
        if let Some(value) = node.get(key) {
            if let Some(value) = each(key, value) {
                out.push((key.to_string(), value));
            }
        }
    }
    let mut rest: Vec<&(String, Node)> = entries
        .iter()
        .filter(|(key, _)| !known.contains(&key.as_str()))
        .collect();
    rest.sort_by(|a, b| a.0.cmp(&b.0));
    for (key, value) in rest {
        if let Some(value) = each(key, value) {
            out.push((key.clone(), value));
        }
    }
    Node::Object(out)
}

fn plain(node: &Node, known: &[&str]) -> Node {
    ordered(node, known, |_, value| Some(value.clone()))
}

/// A `source` object: members in order, each as text.
fn source(node: &Node) -> Node {
    ordered(node, &SOURCE_KEYS, |key, value| {
        Some(match value {
            Node::Number(number) if SOURCE_KEYS.contains(&key) => {
                Node::String(super::json::number_text(number))
            }
            other => other.clone(),
        })
    })
}

fn dictionary_file(node: &Node) -> Node {
    ordered(node, &DICTIONARY_KEYS, |key, value| {
        Some(match key {
            "source" => source(value),
            "lint" => ordered(value, &["units", "allow"], |key, value| {
                Some(match (key, value) {
                    ("units", Node::Array(items)) => {
                        let mut items = items.clone();
                        if items.iter().all(|item| item.as_str().is_some()) {
                            items.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
                            items.dedup();
                        }
                        Node::Array(items)
                    }
                    ("allow", Node::Array(items)) => {
                        let mut items: Vec<Node> = items
                            .iter()
                            .map(|item| plain(item, &["rule", "at", "reason"]))
                            .collect();
                        items.sort_by_key(|item| {
                            (
                                item.str_at("rule").unwrap_or_default().to_string(),
                                item.str_at("at").unwrap_or_default().to_string(),
                                item.str_at("reason").unwrap_or_default().to_string(),
                            )
                        });
                        Node::Array(items)
                    }
                    _ => value.clone(),
                })
            }),
            _ => value.clone(),
        })
    })
}

/// The canonical spelling of a key of a `values` object: decimal without
/// leading zeros, or `0x` and upper-case digits where it was written in hex.
pub fn value_key(key: &str) -> String {
    let trimmed = key.trim();
    let Some(value) = parse_int(trimmed) else {
        return key.to_string();
    };
    let digits = trimmed.strip_prefix('-').unwrap_or(trimmed);
    match digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        Some(hex) => format!(
            "{}0x{}",
            if value < 0 { "-" } else { "" },
            hex.to_ascii_uppercase()
        ),
        None => value.to_string(),
    }
}

/// A `values` object with canonical keys in numeric order. `labels` turns the
/// boolean and numeric labels the decoder accepts into the text it shows.
fn values(node: &Node, labels: bool, at: &str, errors: &mut Vec<String>) -> Node {
    let Node::Object(entries) = node else {
        return node.clone();
    };
    let mut out: Vec<(Option<i128>, String, Node)> = Vec::new();
    for (key, value) in entries {
        let canonical = value_key(key);
        let number = parse_int(&canonical);
        if let Some(number) = number {
            if out.iter().any(|(known, _, _)| *known == Some(number)) {
                errors.push(format!("{at}: values has two keys for {number}"));
                continue;
            }
        }
        let value = match value {
            Node::Bool(flag) if labels => Node::String(flag.to_string()),
            Node::Number(number) if labels => Node::String(super::json::number_text(number)),
            other => other.clone(),
        };
        out.push((number, canonical, value));
    }
    out.sort_by(|a, b| match (a.0, b.0) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.1.cmp(&b.1),
    });
    Node::Object(
        out.into_iter()
            .map(|(_, key, value)| (key, value))
            .collect(),
    )
}

fn metadata_file(node: &Node, at: &str, errors: &mut Vec<String>) -> Node {
    // `mission` is the older name of `description`.
    let renamed;
    let node = match (node.get("mission"), node.get("description")) {
        (Some(_), None) => {
            renamed = Node::Object(
                node.entries()
                    .iter()
                    .map(|(key, value)| {
                        let key = if key == "mission" { "description" } else { key };
                        (key.to_string(), value.clone())
                    })
                    .collect(),
            );
            &renamed
        }
        _ => node,
    };
    ordered(node, &METADATA_KEYS, |key, value| {
        Some(match key {
            "icd" => plain(value, &["version", "date"]),
            "source" => source(value),
            "protocol" => plain(value, &["network", "transport"]),
            "icd_numbering" => plain(value, &NUMBERING_KEYS),
            "message_length" => plain(value, &["start", "stop"]),
            "message_type" => ordered(
                value,
                &["source", "start", "stop", "mask", "values"],
                |key, value| {
                    Some(match key {
                        "values" => values(value, false, &format!("{at}: message_type"), errors),
                        _ => value.clone(),
                    })
                },
            ),
            _ => value.clone(),
        })
    })
}

fn message_file(node: &Node, at: &str, errors: &mut Vec<String>) -> Node {
    ordered(node, &MESSAGE_KEYS, |key, value| {
        Some(match key {
            "source" => source(value),
            "fields" if value.is_object() => Node::Object(
                fields(node)
                    .into_iter()
                    .map(|view| {
                        let at = format!("{at}: {}", view.name);
                        (view.name.to_string(), field(view.node, &at, errors))
                    })
                    .collect(),
            ),
            _ => value.clone(),
        })
    })
}

fn field(node: &Node, at: &str, errors: &mut Vec<String>) -> Node {
    ordered(node, &FIELD_KEYS, |key, value| match key {
        "source" => Some(source(value)),
        "values" if value.is_object() && value.entries().is_empty() => None,
        "values" => Some(values(value, true, at, errors)),
        _ => Some(value.clone()),
    })
}

fn vector_file(node: &Node, tree: &Tree) -> Node {
    // The order of the message's fields, when the vector names a message.
    let order: Vec<&str> = node
        .str_at("interface")
        .and_then(|name| tree.interface(name))
        .zip(
            node.get("expect")
                .and_then(|expect| expect.str_at("message_type")),
        )
        .and_then(|(interface, message)| interface.message(message))
        .map(|message| fields(message).into_iter().map(|view| view.name).collect())
        .unwrap_or_default();
    ordered(node, &VECTOR_KEYS, |key, value| {
        Some(match (key, value) {
            ("payload_hex", Node::String(text)) => Node::String(
                text.chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>()
                    .to_ascii_lowercase(),
            ),
            ("source", _) => source(value),
            ("expect", _) => ordered(value, &["message_type", "fields"], |key, value| {
                Some(match key {
                    "fields" => plain(value, &order),
                    _ => value.clone(),
                })
            }),
            _ => value.clone(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(path: &str, text: &str) -> String {
        let mut files = FileSet::new();
        files.insert("A/metadata.json".into(), r#"{"message_type": {}}"#.into());
        files.insert(path.into(), text.into());
        format(&files).unwrap().remove(path).unwrap()
    }

    #[test]
    fn fields_go_by_start_bit_and_keys_in_the_standard_order() {
        let text = one(
            "A/messages/M.json",
            r#"{"fields": {
                "b": {"unit": "m", "stop": 16, "zeta": 1, "start": 8, "scale": 2.0, "type": "int"},
                "a": {"values": {"10": "Ten", "0x2": true, "1": "One"}, "stop": 8, "start": 0}
            }, "description": "D."}"#,
        );
        assert_eq!(
            text,
            r#"{
    "description": "D.",
    "fields": {
        "a": {
            "start": 0,
            "stop": 8,
            "values": {
                "1": "One",
                "0x2": "true",
                "10": "Ten"
            }
        },
        "b": {
            "start": 8,
            "stop": 16,
            "type": "int",
            "scale": 2,
            "unit": "m",
            "zeta": 1
        }
    }
}
"#
        );
    }

    #[test]
    fn two_spellings_of_one_value_are_an_error() {
        let mut files = FileSet::new();
        files.insert("A/metadata.json".into(), r#"{"message_type": {}}"#.into());
        files.insert(
            "A/messages/M.json".into(),
            r#"{"fields": {"a": {"start": 0, "stop": 8, "values": {"1": "x", "0x01": "y"}}}}"#
                .into(),
        );
        let errors = format(&files).unwrap_err();
        assert_eq!(errors, ["A/messages/M.json: a: values has two keys for 1"]);
    }

    #[test]
    fn mission_becomes_description_and_type_values_sort_by_number() {
        let text = one(
            "A/metadata.json",
            r#"{"message_type": {"values": {"0x10": "B", "9": "A"}, "stop": 8, "start": 0}, "mission": "M."}"#,
        );
        assert_eq!(
            text,
            "{\n    \"description\": \"M.\",\n    \"message_type\": {\n        \"start\": 0,\n        \"stop\": 8,\n        \"values\": {\n            \"9\": \"A\",\n            \"0x10\": \"B\"\n        }\n    }\n}\n"
        );
    }

    #[test]
    fn formatting_twice_changes_nothing() {
        let mut files = FileSet::new();
        files.insert(
            "A/metadata.json".into(),
            r#"{"port": 1, "message_type": {"values": {"2": "M"}, "start": 0, "stop": 8}}"#.into(),
        );
        files.insert(
            "A/messages/M.json".into(),
            r#"{"fields": {"z": {"stop": 16, "start": 8}, "a": {"stop": 8, "start": 0}}}"#.into(),
        );
        files.insert(
            "vectors/v.json".into(),
            r#"{"expect": {"fields": {"z": "1", "a": "2"}, "message_type": "M"}, "payload_hex": "02 0A", "interface": "A"}"#.into(),
        );
        let once = format(&files).unwrap();
        assert_eq!(format(&once).unwrap(), once);
        assert!(once["vectors/v.json"].contains("\"payload_hex\": \"020a\""));
        let a = once["vectors/v.json"].find("\"a\"").unwrap();
        let z = once["vectors/v.json"].find("\"z\"").unwrap();
        assert!(a < z);
    }
}
