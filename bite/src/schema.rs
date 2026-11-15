//! On-disk dictionary format.
//!
//! A dictionary is a directory. Each sub-directory that contains a
//! `metadata.json` is one *interface*: a set of messages that share a
//! transport and a way of telling the messages apart. Each file in the
//! interface's `messages/` directory describes one message; the file stem is
//! the message name.
//!
//! ```text
//! my-dictionary/
//!   dictionary.json            optional: name, version, description
//!   RADAR/
//!     metadata.json            InterfaceSpec
//!     messages/
//!       Heartbeat.json  MessageSpec
//!   vectors/                   optional: test vectors, see `bite check`
//! ```
//!
//! Bit positions are half-open ranges `[start, stop)`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How bit positions are numbered. See [`crate::bits::extract`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BitOrder {
    #[default]
    Msb0,
    Lsb0,
}

/// Byte order of a byte-aligned multi-byte field under [`BitOrder::Msb0`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ByteOrder {
    #[default]
    Big,
    Little,
}

/// What the bits of a field mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    /// Unsigned integer, 1 to 64 bits.
    Uint,
    /// Two's-complement integer, 1 to 64 bits.
    Int,
    /// IEEE 754 binary32 or binary64.
    Float,
    /// One bit or more; zero is false.
    Bool,
    /// Opaque bytes; byte-aligned, any length.
    Bytes,
    /// Text; byte-aligned, trailing NUL bytes and spaces removed.
    Ascii,
}

/// `dictionary.json`: optional description of the dictionary as a whole.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DictionarySpec {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// The document an interface was transcribed from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Icd {
    pub version: String,
    #[serde(default)]
    pub date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Protocol {
    pub network: String,
    pub transport: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BitRange {
    pub start: usize,
    pub stop: usize,
}

/// Where the number that identifies a message comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscriminatorSource {
    /// Bits `[start, stop)` of the payload.
    #[default]
    Payload,
    /// The identifier that travels with the frame, outside the payload (a CAN
    /// identifier, a 1553 sub-address), optionally masked.
    FrameId,
}

/// How to tell the messages of an interface apart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscriminatorSpec {
    #[serde(default)]
    pub source: DiscriminatorSource,
    #[serde(default)]
    pub start: usize,
    #[serde(default)]
    pub stop: usize,
    /// Applied to the frame identifier before lookup. Decimal or `0x` hex.
    #[serde(default)]
    pub mask: Option<String>,
    /// Identifier (decimal or `0x` hex) to message name.
    #[serde(default)]
    pub values: BTreeMap<String, String>,
}

/// `metadata.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceSpec {
    #[serde(default)]
    pub icd: Option<Icd>,
    /// Free-text description. `mission` is the name older dictionaries use.
    #[serde(default, alias = "description")]
    pub mission: Option<String>,
    #[serde(default)]
    pub protocol: Option<Protocol>,
    /// UDP or TCP port that carries this interface. A packet whose source or
    /// destination port equals it belongs to the interface.
    #[serde(default)]
    pub port: Option<u16>,
    pub message_type: DiscriminatorSpec,
    /// Where a message states its own length, if it does.
    #[serde(default)]
    pub message_length: Option<BitRange>,
    #[serde(default)]
    pub bit_order: BitOrder,
    #[serde(default)]
    pub byte_order: ByteOrder,
}

/// One field of a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldSpec {
    pub start: usize,
    pub stop: usize,
    /// Shorthand for `"type": "int"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed: Option<bool>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<FieldKind>,
    /// Overrides the interface's byte order for this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_order: Option<ByteOrder>,
    /// Engineering value is `raw * scale + offset`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Raw integer value (decimal or `0x` hex) to label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<BTreeMap<String, serde_json::Value>>,
}

/// `messages/<name>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSpec {
    #[serde(default)]
    pub description: Option<String>,
    pub fields: BTreeMap<String, FieldSpec>,
}

/// `vectors/<name>.json`: a payload and what it must decode to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorSpec {
    pub interface: String,
    /// Payload bytes as hex; whitespace is ignored.
    pub payload_hex: String,
    #[serde(default)]
    pub frame_id: Option<u64>,
    pub expect: VectorExpectation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorExpectation {
    #[serde(default)]
    pub message_type: Option<String>,
    /// Field name to rendered value. Fields not listed are not checked; a
    /// `null` expects the field to lie beyond the end of the payload.
    #[serde(default)]
    pub fields: BTreeMap<String, Option<String>>,
}

/// Parse a decimal or `0x`-prefixed hexadecimal integer with at most one
/// leading sign. `None` for anything else, including a value outside `i128`.
pub fn parse_int(text: &str) -> Option<i128> {
    let text = text.trim();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    // The integer parsers below accept a sign of their own; a second sign is
    // not a number.
    if digits.starts_with(['+', '-']) {
        return None;
    }
    let value = match digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        Some(hex) if hex.starts_with(['+', '-']) => return None,
        Some(hex) => i128::from_str_radix(hex, 16).ok()?,
        None => digits.parse::<i128>().ok()?,
    };
    if negative {
        value.checked_neg()
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::parse_int;

    #[test]
    fn decimal_and_hexadecimal_with_one_sign() {
        assert_eq!(parse_int("42"), Some(42));
        assert_eq!(parse_int(" -42 "), Some(-42));
        assert_eq!(parse_int("+42"), Some(42));
        assert_eq!(parse_int("0x2A"), Some(42));
        assert_eq!(parse_int("-0X2a"), Some(-42));
        assert_eq!(parse_int(""), None);
        assert_eq!(parse_int("0x"), None);
        assert_eq!(parse_int("four"), None);
    }

    #[test]
    fn a_second_sign_is_not_a_number() {
        // Each of these used to reach the negation with i128::MIN, or to
        // parse as the opposite sign.
        assert_eq!(parse_int("--170141183460469231731687303715884105728"), None);
        assert_eq!(parse_int("-0x-80000000000000000000000000000000"), None);
        assert_eq!(parse_int("--5"), None);
        assert_eq!(parse_int("-+5"), None);
        assert_eq!(parse_int("0x-5"), None);
        assert_eq!(parse_int("+-5"), None);
    }

    #[test]
    fn the_ends_of_the_range_parse() {
        assert_eq!(
            parse_int("170141183460469231731687303715884105727"),
            Some(i128::MAX)
        );
        assert_eq!(
            parse_int("-170141183460469231731687303715884105727"),
            Some(-i128::MAX)
        );
        assert_eq!(parse_int("170141183460469231731687303715884105728"), None);
    }
}
