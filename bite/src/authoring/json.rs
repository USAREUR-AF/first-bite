//! A JSON tree that keeps the order of object keys, refuses duplicate keys and
//! writes one canonical layout.
//!
//! `serde_json::Value` sorts keys and silently keeps the last of two equal
//! keys. A dictionary file with the same field name twice has lost a field,
//! and that must be an error; and a canonical file needs its keys in a chosen
//! order, not the alphabetical one.

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Number;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Node>),
    /// Keys in file order.
    Object(Vec<(String, Node)>),
}

impl Node {
    /// Parse JSON text. A key that appears twice in one object is an error.
    pub fn parse(text: &str) -> Result<Node, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }

    pub fn object() -> Node {
        Node::Object(Vec::new())
    }

    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn entries(&self) -> &[(String, Node)] {
        match self {
            Node::Object(entries) => entries,
            _ => &[],
        }
    }

    pub fn items(&self) -> &[Node] {
        match self {
            Node::Array(items) => items,
            _ => &[],
        }
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Node::Object(_))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Node::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Node::Number(number) => number.as_u64().or_else(|| {
                let float = number.as_f64()?;
                (float >= 0.0 && float.fract() == 0.0 && float < 9.0e15).then_some(float as u64)
            }),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Node::Number(number) => number.as_f64(),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Node::Bool(flag) => Some(*flag),
            _ => None,
        }
    }

    /// The text of a key that holds a string.
    pub fn str_at(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Node::as_str)
    }

    pub fn u64_at(&self, key: &str) -> Option<u64> {
        self.get(key).and_then(Node::as_u64)
    }

    pub fn f64_at(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Node::as_f64)
    }

    /// Set a key of an object, replacing it or appending it.
    pub fn set(&mut self, key: &str, value: Node) {
        if let Node::Object(entries) = self {
            match entries.iter_mut().find(|(k, _)| k == key) {
                Some(entry) => entry.1 = value,
                None => entries.push((key.to_string(), value)),
            }
        }
    }

    /// A number node; integral values are held as integers.
    pub fn number(value: f64) -> Node {
        if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
            Node::Number(Number::from(value as i64))
        } else {
            Number::from_f64(value).map_or(Node::Null, Node::Number)
        }
    }

    /// The canonical text of this tree: four-space indentation, one member
    /// per line, a single trailing newline.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        write_node(self, 0, &mut out);
        out.push('\n');
        out
    }

    /// The tree on one line, for messages.
    pub fn to_line(&self) -> String {
        match self {
            Node::Null => "null".to_string(),
            Node::Bool(flag) => flag.to_string(),
            Node::Number(number) => number_text(number),
            Node::String(text) => quoted(text),
            Node::Array(items) => {
                let inner: Vec<String> = items.iter().map(Node::to_line).collect();
                format!("[{}]", inner.join(", "))
            }
            Node::Object(entries) => {
                let inner: Vec<String> = entries
                    .iter()
                    .map(|(key, value)| format!("{}: {}", quoted(key), value.to_line()))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
        }
    }
}

impl From<&str> for Node {
    fn from(text: &str) -> Node {
        Node::String(text.to_string())
    }
}

impl From<String> for Node {
    fn from(text: String) -> Node {
        Node::String(text)
    }
}

impl From<u64> for Node {
    fn from(value: u64) -> Node {
        Node::Number(Number::from(value))
    }
}

impl From<bool> for Node {
    fn from(flag: bool) -> Node {
        Node::Bool(flag)
    }
}

fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

/// A number in its canonical spelling: an integral value has no fraction and
/// no exponent, anything else is the shortest text that reads back equal.
pub fn number_text(number: &Number) -> String {
    if number.is_f64() {
        if let Some(float) = number.as_f64() {
            if float.fract() == 0.0 && float.abs() < 9_007_199_254_740_992.0 {
                return format!("{}", float as i64);
            }
        }
    }
    number.to_string()
}

fn indent(depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push_str("    ");
    }
}

fn write_node(node: &Node, depth: usize, out: &mut String) {
    match node {
        Node::Array(items) if !items.is_empty() => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                indent(depth + 1, out);
                write_node(item, depth + 1, out);
                out.push_str(if index + 1 < items.len() { ",\n" } else { "\n" });
            }
            indent(depth, out);
            out.push(']');
        }
        Node::Object(entries) if !entries.is_empty() => {
            out.push_str("{\n");
            for (index, (key, value)) in entries.iter().enumerate() {
                indent(depth + 1, out);
                out.push_str(&quoted(key));
                out.push_str(": ");
                write_node(value, depth + 1, out);
                out.push_str(if index + 1 < entries.len() {
                    ",\n"
                } else {
                    "\n"
                });
            }
            indent(depth, out);
            out.push('}');
        }
        other => out.push_str(&other.to_line()),
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Node, D::Error> {
        struct NodeVisitor;

        impl<'de> Visitor<'de> for NodeVisitor {
            type Value = Node;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON value")
            }

            fn visit_bool<E>(self, value: bool) -> Result<Node, E> {
                Ok(Node::Bool(value))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Node, E> {
                Ok(Node::Number(Number::from(value)))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Node, E> {
                Ok(Node::Number(Number::from(value)))
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Node, E> {
                Number::from_f64(value)
                    .map(Node::Number)
                    .ok_or_else(|| E::custom("a number that is not finite"))
            }

            fn visit_str<E>(self, value: &str) -> Result<Node, E> {
                Ok(Node::String(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> Result<Node, E> {
                Ok(Node::String(value))
            }

            fn visit_unit<E>(self) -> Result<Node, E> {
                Ok(Node::Null)
            }

            fn visit_none<E>(self) -> Result<Node, E> {
                Ok(Node::Null)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(Node::Array(items))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
                let mut entries: Vec<(String, Node)> = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    if entries.iter().any(|(known, _)| known == &key) {
                        return Err(de::Error::custom(format!(
                            "key {key:?} appears twice in one object"
                        )));
                    }
                    entries.push((key, map.next_value()?));
                }
                Ok(Node::Object(entries))
            }
        }

        deserializer.deserialize_any(NodeVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_key_order_and_writes_four_spaces() {
        let node = Node::parse(r#"{"b": 1, "a": {"z": [1, 2], "y": []}, "c": {}}"#).unwrap();
        assert_eq!(
            node.to_text(),
            "{\n    \"b\": 1,\n    \"a\": {\n        \"z\": [\n            1,\n            2\n        ],\n        \"y\": []\n    },\n    \"c\": {}\n}\n"
        );
    }

    #[test]
    fn a_repeated_key_is_an_error() {
        let error = Node::parse(r#"{"fields": {"speed": 1, "speed": 2}}"#).unwrap_err();
        assert!(error.contains("\"speed\" appears twice"), "{error}");
    }

    #[test]
    fn numbers_have_one_spelling() {
        let node = Node::parse(r#"[2.0, 2, 1.0E-2, 0.010, -40.0, 1e3, 0.1, 1e-7]"#).unwrap();
        assert_eq!(node.to_line(), "[2, 2, 0.01, 0.01, -40, 1000, 0.1, 1e-7]");
    }

    #[test]
    fn text_outside_ascii_is_written_as_itself() {
        let node = Node::parse(r#"{"unit": "µs", "label": "a\"b\n"}"#).unwrap();
        assert_eq!(
            node.to_line(),
            "{\"unit\": \"µs\", \"label\": \"a\\\"b\\n\"}"
        );
    }
}
