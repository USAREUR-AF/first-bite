//! How a record is encoded where it leaves the process.
//!
//! Inside the process a record is JSON text. Each publisher and each sink
//! writes it in the encoding its configuration names:
//!
//! | `encoding` | Bytes | Media type | File extension |
//! |---|---|---|---|
//! | `json` (default) | the record as JSON text | `application/json` | `json` |
//! | `cbor` | the same record as one CBOR item (RFC 8949) | `application/cbor` | `cbor` |
//!
//! The CBOR item has the JSON record's data model, value for value: an object
//! is a map with text keys, an array an array, a string a text string, an
//! integer an integer, any other number a float, and `true`, `false` and
//! `null` themselves. Nothing is tagged and nothing is a byte string, so
//! reading the item back gives the JSON record, value for value.
//!
//! The item is written in the core deterministic encoding of RFC 8949
//! section 4.2.1: integers and lengths in their shortest form, floats in the
//! shortest form that keeps the value, definite lengths, and the keys of each
//! map in bytewise order of their encodings. The same record always gives the
//! same bytes.

use crate::error::Problem;
use serde::de::{DeserializeSeed, Deserializer, Error as _, MapAccess, SeqAccess, Visitor};
use std::borrow::Cow;
use std::fmt::{self, Write as _};

/// The encodings a publisher or sink can be given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encoding {
    #[default]
    Json,
    Cbor,
}

/// Why a record could not be written in, or read from, an encoding.
#[derive(Debug, thiserror::Error)]
pub enum EncodingError {
    #[error("record is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("record is not valid CBOR: {0}")]
    Cbor(String),
    #[error("CBOR record holds {0}, which a record never does")]
    Unsupported(&'static str),
    #[error("{0} bytes follow the CBOR record")]
    Trailing(usize),
    #[error("message is neither a JSON object nor a CBOR map")]
    Unrecognised,
}

impl Encoding {
    /// The value of an `encoding` key; unset means JSON.
    pub fn from_config(value: Option<&str>) -> Result<Self, Problem> {
        match value {
            None | Some("json") => Ok(Encoding::Json),
            Some("cbor") => Ok(Encoding::Cbor),
            Some(other) => Err(Problem::InvalidValue {
                key: "encoding",
                reason: format!("{other:?} is not one of json, cbor"),
            }),
        }
    }

    /// The name used in configuration.
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Json => "json",
            Encoding::Cbor => "cbor",
        }
    }

    /// The media type, for an HTTP `Content-Type` and for object metadata.
    pub fn media_type(self) -> &'static str {
        match self {
            Encoding::Json => "application/json",
            Encoding::Cbor => "application/cbor",
        }
    }

    /// The extension of a stored object's name.
    pub fn extension(self) -> &'static str {
        match self {
            Encoding::Json => "json",
            Encoding::Cbor => "cbor",
        }
    }

    /// The bytes of a record given as JSON text. JSON is passed through as it
    /// is; CBOR is transcoded.
    pub fn encode<'a>(self, record: &'a str) -> Result<Cow<'a, [u8]>, EncodingError> {
        match self {
            Encoding::Json => Ok(Cow::Borrowed(record.as_bytes())),
            Encoding::Cbor => json_to_cbor(record).map(Cow::Owned),
        }
    }
}

// CBOR major types (RFC 8949, section 3.1).
const UNSIGNED: u8 = 0;
const NEGATIVE: u8 = 1;
const BYTES: u8 = 2;
const TEXT: u8 = 3;
const ARRAY: u8 = 4;
const MAP: u8 = 5;
const TAG: u8 = 6;
const SIMPLE: u8 = 7;

/// The "break" that ends an indefinite-length item.
const BREAK: u8 = 0xff;

/// Deepest nesting read from CBOR. A record nests three deep.
const MAX_DEPTH: usize = 64;

/// Append the head of an item: its major type and its argument, in the
/// shortest form.
fn push_head(out: &mut Vec<u8>, major: u8, argument: u64) {
    let major = major << 5;
    match argument {
        0..=23 => out.push(major | argument as u8),
        24..=0xff => out.extend_from_slice(&[major | 24, argument as u8]),
        0x100..=0xffff => {
            out.push(major | 25);
            out.extend_from_slice(&(argument as u16).to_be_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            out.push(major | 26);
            out.extend_from_slice(&(argument as u32).to_be_bytes());
        }
        _ => {
            out.push(major | 27);
            out.extend_from_slice(&argument.to_be_bytes());
        }
    }
}

/// The value of an IEEE 754 half-precision float.
fn half_to_f64(bits: u16) -> f64 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = i32::from((bits >> 10) & 0x1f);
    let fraction = f64::from(bits & 0x3ff);
    sign * match exponent {
        0 => fraction * 2f64.powi(-24),
        31 if fraction == 0.0 => f64::INFINITY,
        31 => f64::NAN,
        _ => (fraction + 1024.0) * 2f64.powi(exponent - 25),
    }
}

/// The half-precision float equal to `value`, when there is one.
fn f64_to_half(value: f64) -> Option<u16> {
    let single = value as f32;
    if f64::from(single) != value {
        return None;
    }
    let bits = single.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127;
    let fraction = bits & 0x007f_ffff;
    let half = if bits << 1 == 0 {
        sign
    } else if (-14..=15).contains(&exponent) && fraction & 0x1fff == 0 {
        sign | (((exponent + 15) as u16) << 10) | (fraction >> 13) as u16
    } else if (-24..-14).contains(&exponent) {
        // Subnormal in half precision: the leading one becomes explicit.
        let shift = (13 - 14 - exponent) as u32;
        let mantissa = fraction | 0x0080_0000;
        if mantissa & ((1 << shift) - 1) != 0 {
            return None;
        }
        sign | (mantissa >> shift) as u16
    } else {
        return None;
    };
    (half_to_f64(half) == value).then_some(half)
}

/// Append a float in the shortest of the three widths that keeps its value.
fn push_float(out: &mut Vec<u8>, value: f64) {
    if let Some(half) = f64_to_half(value) {
        out.push(0xf9);
        out.extend_from_slice(&half.to_be_bytes());
    } else if f64::from(value as f32) == value {
        out.push(0xfa);
        out.extend_from_slice(&(value as f32).to_be_bytes());
    } else {
        out.push(0xfb);
        out.extend_from_slice(&value.to_be_bytes());
    }
}

fn push_text(out: &mut Vec<u8>, text: &str) {
    push_head(out, TEXT, text.len() as u64);
    out.extend_from_slice(text.as_bytes());
}

/// Writes one JSON value, as the parser reads it, to `out` as CBOR. No tree
/// of the document is built.
struct Item<'a>(&'a mut Vec<u8>);

impl<'de> DeserializeSeed<'de> for Item<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

/// A map key, written to the output as a CBOR text string.
struct Key<'a>(&'a mut Vec<u8>);

impl<'de> DeserializeSeed<'de> for Key<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_str(Item(self.0))
    }
}

impl<'de> Visitor<'de> for Item<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<(), E> {
        self.0.push(if value { 0xf5 } else { 0xf4 });
        Ok(())
    }

    fn visit_u64<E>(self, value: u64) -> Result<(), E> {
        push_head(self.0, UNSIGNED, value);
        Ok(())
    }

    fn visit_i64<E>(self, value: i64) -> Result<(), E> {
        if value >= 0 {
            push_head(self.0, UNSIGNED, value as u64);
        } else {
            // -1 - n is encoded as n.
            push_head(self.0, NEGATIVE, !(value as u64));
        }
        Ok(())
    }

    fn visit_f64<E>(self, value: f64) -> Result<(), E> {
        push_float(self.0, value);
        Ok(())
    }

    fn visit_str<E>(self, value: &str) -> Result<(), E> {
        push_text(self.0, value);
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<(), E> {
        self.0.push(0xf6);
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<(), A::Error> {
        // The count comes first in CBOR and last in JSON: the items are
        // written, then the head is put in front of them.
        let start = self.0.len();
        let mut count = 0u64;
        while items.next_element_seed(Item(self.0))?.is_some() {
            count += 1;
        }
        let items = self.0.split_off(start);
        push_head(self.0, ARRAY, count);
        self.0.extend_from_slice(&items);
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<(), A::Error> {
        // Each entry is written in the order it is read; `spans` holds where
        // each one starts, where its key ends and where it ends.
        let start = self.0.len();
        let mut spans: Vec<(usize, usize, usize)> = Vec::new();
        loop {
            let entry = self.0.len();
            if entries.next_key_seed(Key(self.0))?.is_none() {
                break;
            }
            let key_end = self.0.len();
            entries.next_value_seed(Item(self.0))?;
            spans.push((entry, key_end, self.0.len()));
        }
        // Deterministic encoding orders a map by the bytes of its encoded
        // keys.
        let written = self.0.split_off(start);
        let key = |span: &(usize, usize, usize)| &written[span.0 - start..span.1 - start];
        spans.sort_by(|a, b| key(a).cmp(key(b)));
        if spans.windows(2).any(|pair| key(&pair[0]) == key(&pair[1])) {
            return Err(A::Error::custom("an object has the same key twice"));
        }
        push_head(self.0, MAP, spans.len() as u64);
        for span in &spans {
            self.0
                .extend_from_slice(&written[span.0 - start..span.2 - start]);
        }
        Ok(())
    }
}

/// Transcode a JSON document to one deterministic CBOR item.
pub fn json_to_cbor(json: &str) -> Result<Vec<u8>, EncodingError> {
    let mut out = Vec::with_capacity(json.len());
    let mut deserializer = serde_json::Deserializer::from_str(json);
    Item(&mut out).deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(out)
}

/// Reads CBOR items from `input` and writes them to `json`.
struct Reader<'a> {
    input: &'a [u8],
    json: String,
}

impl<'a> Reader<'a> {
    fn truncated() -> EncodingError {
        EncodingError::Cbor("the input ends inside an item".to_string())
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], EncodingError> {
        if count > self.input.len() {
            return Err(Self::truncated());
        }
        let (taken, rest) = self.input.split_at(count);
        self.input = rest;
        Ok(taken)
    }

    fn byte(&mut self) -> Result<u8, EncodingError> {
        Ok(self.take(1)?[0])
    }

    /// The argument that follows an initial byte; `None` for the
    /// indefinite-length marker.
    fn argument(&mut self, info: u8) -> Result<Option<u64>, EncodingError> {
        Ok(Some(match info {
            0..=23 => u64::from(info),
            24 => u64::from(self.byte()?),
            25 => {
                let bytes = self.take(2)?;
                u64::from(u16::from_be_bytes([bytes[0], bytes[1]]))
            }
            26 => {
                let bytes = self.take(4)?;
                u64::from(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            }
            27 => {
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(self.take(8)?);
                u64::from_be_bytes(bytes)
            }
            31 => return Ok(None),
            _ => {
                return Err(EncodingError::Cbor(format!(
                    "additional information {info} is reserved"
                )))
            }
        }))
    }

    /// A definite-length text string whose head has been read.
    fn text(&mut self, length: u64) -> Result<&'a str, EncodingError> {
        let length = usize::try_from(length).map_err(|_| Self::truncated())?;
        std::str::from_utf8(self.take(length)?)
            .map_err(|_| EncodingError::Cbor("a text string is not UTF-8".to_string()))
    }

    /// A text string of either length form, written as a JSON string.
    fn string(&mut self, argument: Option<u64>) -> Result<(), EncodingError> {
        let text: Cow<'a, str> = match argument {
            Some(length) => Cow::Borrowed(self.text(length)?),
            None => {
                let mut whole = String::new();
                loop {
                    let initial = self.byte()?;
                    if initial == BREAK {
                        break;
                    }
                    let length = match (initial >> 5, self.argument(initial & 0x1f)?) {
                        (TEXT, Some(length)) => length,
                        _ => {
                            return Err(EncodingError::Cbor(
                                "a chunk of a text string is not a text string".to_string(),
                            ))
                        }
                    };
                    whole.push_str(self.text(length)?);
                }
                Cow::Owned(whole)
            }
        };
        write_json_string(&mut self.json, &text);
        Ok(())
    }

    /// Whether another item of an array or map follows; `remaining` counts
    /// down a definite length.
    fn more(&mut self, remaining: &mut Option<u64>) -> Result<bool, EncodingError> {
        match remaining {
            Some(0) => Ok(false),
            Some(count) => {
                *count -= 1;
                Ok(true)
            }
            None if self.input.first() == Some(&BREAK) => {
                self.input = &self.input[1..];
                Ok(false)
            }
            None => Ok(true),
        }
    }

    fn item(&mut self, depth: usize) -> Result<(), EncodingError> {
        if depth > MAX_DEPTH {
            return Err(EncodingError::Cbor(format!(
                "items are nested more than {MAX_DEPTH} deep"
            )));
        }
        let initial = self.byte()?;
        let (major, info) = (initial >> 5, initial & 0x1f);
        if major == SIMPLE {
            return self.simple(info);
        }
        let argument = self.argument(info)?;
        match (major, argument) {
            (UNSIGNED, Some(value)) => {
                let _ = write!(self.json, "{value}");
            }
            (NEGATIVE, Some(value)) => {
                // The item is -1 - value.
                let value = i64::try_from(value)
                    .map_err(|_| EncodingError::Unsupported("an integer below -2^63"))?;
                let _ = write!(self.json, "{}", -1 - value);
            }
            (TEXT, argument) => self.string(argument)?,
            (ARRAY, mut remaining) => {
                self.json.push('[');
                let mut first = true;
                while self.more(&mut remaining)? {
                    if !std::mem::take(&mut first) {
                        self.json.push(',');
                    }
                    self.item(depth + 1)?;
                }
                self.json.push(']');
            }
            (MAP, mut remaining) => {
                self.json.push('{');
                let mut first = true;
                while self.more(&mut remaining)? {
                    if !std::mem::take(&mut first) {
                        self.json.push(',');
                    }
                    let initial = self.byte()?;
                    if initial >> 5 != TEXT {
                        return Err(EncodingError::Unsupported("a map key that is not text"));
                    }
                    let argument = self.argument(initial & 0x1f)?;
                    self.string(argument)?;
                    self.json.push(':');
                    self.item(depth + 1)?;
                }
                self.json.push('}');
            }
            (BYTES, _) => return Err(EncodingError::Unsupported("a byte string")),
            (TAG, _) => return Err(EncodingError::Unsupported("a tagged item")),
            _ => {
                return Err(EncodingError::Cbor(
                    "an integer cannot have an indefinite length".to_string(),
                ))
            }
        }
        Ok(())
    }

    /// An item of major type 7: a float or a simple value.
    fn simple(&mut self, info: u8) -> Result<(), EncodingError> {
        let float = match info {
            20..=22 => {
                self.json
                    .push_str(["false", "true", "null"][usize::from(info - 20)]);
                return Ok(());
            }
            25 => {
                let bytes = self.take(2)?;
                half_to_f64(u16::from_be_bytes([bytes[0], bytes[1]]))
            }
            26 => {
                let bytes = self.take(4)?;
                f64::from(f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            }
            27 => {
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(self.take(8)?);
                f64::from_be_bytes(bytes)
            }
            31 => return Err(EncodingError::Cbor("a break outside an item".to_string())),
            _ => {
                return Err(EncodingError::Unsupported(
                    "a simple value other than true, false and null",
                ))
            }
        };
        // JSON has no NaN and no infinity; a record never carries one.
        match serde_json::Number::from_f64(float) {
            Some(number) => {
                let _ = write!(self.json, "{number}");
            }
            None => self.json.push_str("null"),
        }
        Ok(())
    }
}

/// Append `text` as a JSON string.
fn write_json_string(json: &mut String, text: &str) {
    json.push('"');
    let mut plain = 0;
    for (index, byte) in text.bytes().enumerate() {
        let escape = match byte {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            0x00..=0x1f => "",
            _ => continue,
        };
        json.push_str(&text[plain..index]);
        if escape.is_empty() {
            let _ = write!(json, "\\u{byte:04x}");
        } else {
            json.push_str(escape);
        }
        plain = index + 1;
    }
    json.push_str(&text[plain..]);
    json.push('"');
}

/// Transcode one CBOR item to JSON text. Bytes after the item are an error.
pub fn cbor_to_json(bytes: &[u8]) -> Result<String, EncodingError> {
    let mut reader = Reader {
        input: bytes,
        json: String::with_capacity(bytes.len() + bytes.len() / 2),
    };
    reader.item(0)?;
    if !reader.input.is_empty() {
        return Err(EncodingError::Trailing(reader.input.len()));
    }
    Ok(reader.json)
}

/// The JSON text of a message payload read from a publisher socket, whichever
/// encoding the publisher uses. A JSON object begins with `{`; a CBOR map
/// begins with a byte from 0xA0 to 0xBF, which no JSON text does.
pub fn payload_to_json(payload: &[u8]) -> Result<Cow<'_, str>, EncodingError> {
    let start = payload
        .iter()
        .position(|byte| *byte != b' ')
        .unwrap_or(payload.len());
    let payload = &payload[start..];
    match payload.first() {
        Some(b'{') => std::str::from_utf8(payload)
            .map(Cow::Borrowed)
            .map_err(|_| EncodingError::Unrecognised),
        Some(0xA0..=0xBF) => cbor_to_json(payload).map(Cow::Owned),
        _ => Err(EncodingError::Unrecognised),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value as Json};

    fn cbor_hex(json: &str) -> String {
        json_to_cbor(json)
            .unwrap()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// Encodings listed in RFC 8949, Appendix A.
    #[test]
    fn items_are_encoded_as_the_rfc_lists_them() {
        for (json, expected) in [
            ("0", "00"),
            ("10", "0a"),
            ("23", "17"),
            ("24", "1818"),
            ("1000", "1903e8"),
            ("1000000", "1a000f4240"),
            ("1000000000000", "1b000000e8d4a51000"),
            ("18446744073709551615", "1bffffffffffffffff"),
            ("-1", "20"),
            ("-1000", "3903e7"),
            ("0.0", "f90000"),
            ("1.0", "f93c00"),
            ("1.5", "f93e00"),
            ("1.1", "fb3ff199999999999a"),
            ("100000.0", "fa47c35000"),
            ("-4.1", "fbc010666666666666"),
            ("false", "f4"),
            ("true", "f5"),
            ("null", "f6"),
            (r#""""#, "60"),
            (r#""IETF""#, "6449455446"),
            ("[]", "80"),
            ("[1, 2, 3]", "83010203"),
            ("{}", "a0"),
            (r#"{"a": 1, "b": [2, 3]}"#, "a26161016162820203"),
        ] {
            assert_eq!(cbor_hex(json), expected, "{json}");
        }
    }

    #[test]
    fn map_keys_are_ordered_by_length_and_then_by_bytes() {
        // {"b": 1, "aa": 2, "a": 3}: a, b, aa.
        assert_eq!(
            cbor_hex(r#"{"b": 1, "aa": 2, "a": 3}"#),
            "a361610361620162616102"
        );
        // The same object written in another order gives the same bytes.
        assert_eq!(
            cbor_hex(r#"{"aa": 2, "a": 3, "b": 1}"#),
            cbor_hex(r#"{"b": 1, "aa": 2, "a": 3}"#)
        );
    }

    #[test]
    fn every_json_type_survives_the_round_trip() {
        let record = json!({
            "timestamp": {"secs_since_epoch": 1_700_000_000u64, "nanos_since_epoch": 250_000_000},
            "src_port": 40100,
            "src_ip": [192, 0, 2, 1],
            "subsystem": "WX",
            "data_source_owner": null,
            "message": {
                "uptime": 86400,
                "battery_low": false,
                "temperature": -12.34,
                "whole": 21.0,
                "offset": -7,
                "serial": u64::MAX,
                "lowest": i64::MIN,
                "mode": "Calibrating",
                "checksum": "beef",
                "missing": null,
            },
        });
        let cbor = json_to_cbor(&record.to_string()).unwrap();
        let back: Json = serde_json::from_str(&cbor_to_json(&cbor).unwrap()).unwrap();
        assert_eq!(back, record);
        // A float that holds a whole number stays a float.
        assert!(back["message"]["whole"].is_f64());
        assert!(back["message"]["uptime"].is_u64());
        assert!(cbor.len() < record.to_string().len());
    }

    #[test]
    fn what_a_record_never_holds_is_refused() {
        let refusal = |bytes: &[u8]| cbor_to_json(bytes).unwrap_err().to_string();
        // h'0102', a tag, an integer key, 2^64 as a negative, trailing bytes.
        assert!(refusal(&[0x42, 1, 2]).contains("a byte string"));
        assert!(refusal(&[0xc1, 0x00]).contains("a tagged item"));
        assert!(refusal(&[0xa1, 0x01, 0x02]).contains("a map key that is not text"));
        assert!(
            refusal(&[0x3b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff])
                .contains("an integer below -2^63")
        );
        assert_eq!(refusal(&[0xa0, 0x00]), "1 bytes follow the CBOR record");
        assert!(refusal(&[0xa1, 0x61]).starts_with("record is not valid CBOR"));
        assert!(refusal(&[0xf7]).contains("a simple value"));
        assert!(refusal(&[0x1c]).contains("reserved"));
        assert!(refusal(&[0x62, 0xff, 0xfe]).contains("not UTF-8"));
        assert!(refusal(&[0xff]).contains("a break outside an item"));
        assert!(matches!(
            json_to_cbor(r#"{"a": 1, "a": 2}"#),
            Err(EncodingError::Json(_))
        ));
        assert!(matches!(json_to_cbor("{} {}"), Err(EncodingError::Json(_))));
        assert!(matches!(
            json_to_cbor("not json"),
            Err(EncodingError::Json(_))
        ));
    }

    #[test]
    fn floats_take_the_shortest_width_that_keeps_the_value() {
        // RFC 8949, Appendix A, and the edges of half precision.
        for (value, expected) in [
            (5.960464477539063e-8, "f90001"),
            (0.00006103515625, "f90400"),
            (65504.0, "f97bff"),
            (-0.0, "f98000"),
            (3.4028234663852886e38, "fa7f7fffff"),
            (1.0e300, "fb7e37e43c8800759c"),
            (65536.0, "fa47800000"),
            (2.9802322387695312e-8, "fa33000000"),
        ] {
            let mut out = Vec::new();
            push_float(&mut out, value);
            let hex: String = out.iter().map(|byte| format!("{byte:02x}")).collect();
            assert_eq!(hex, expected, "{value}");
        }
        // Every half-precision value reads back as itself.
        for bits in 0..=u16::MAX {
            let value = half_to_f64(bits);
            if value.is_finite() {
                assert_eq!(f64_to_half(value), Some(bits), "{bits:#06x}");
            }
        }
    }

    #[test]
    fn indefinite_lengths_from_another_encoder_are_read() {
        // {_ "a": [_ 1, 2], "b": (_ "x", "y")}
        let bytes = [
            0xbf, 0x61, b'a', 0x9f, 0x01, 0x02, 0xff, 0x61, b'b', 0x7f, 0x61, b'x', 0x61, b'y',
            0xff, 0xff,
        ];
        assert_eq!(cbor_to_json(&bytes).unwrap(), r#"{"a":[1,2],"b":"xy"}"#);
    }

    #[test]
    fn strings_are_escaped_as_json() {
        let text = "quote \" slash \\ newline \n tab \t bell \u{7} é 雨";
        let cbor = json_to_cbor(&serde_json::to_string(text).unwrap()).unwrap();
        let back: String = serde_json::from_str(&cbor_to_json(&cbor).unwrap()).unwrap();
        assert_eq!(back, text);
    }

    /// The same documents through an independent CBOR implementation.
    #[test]
    fn another_implementation_reads_and_writes_the_same_bytes() {
        let documents = [
            json!({"timestamp": {"secs_since_epoch": 1_700_000_000u64, "nanos_since_epoch": 250_000_000}, "src_ip": [192, 0, 2, 1], "message": {"t": -12.34, "u": 86400, "ok": true, "none": null, "mode": "Idle"}}),
            json!([
                0,
                23,
                24,
                255,
                256,
                65535,
                65536,
                u32::MAX,
                u64::MAX,
                -1,
                -24,
                -25,
                i64::MIN
            ]),
            json!([0.5, 1.5, -2.25, 0.1, 1e10, 1e-10, 3.0e38, 1.0e39]),
            json!({"": "", "k": "v".repeat(300), "nested": [[[]], {}]}),
        ];
        for document in documents {
            let ours = json_to_cbor(&document.to_string()).unwrap();
            let read: ciborium::Value = ciborium::from_reader(&ours[..]).unwrap();
            assert_eq!(serde_json::to_value(&read).unwrap(), document);
            // Written by the other implementation, read by this one.
            let mut theirs = Vec::new();
            ciborium::into_writer(&document, &mut theirs).unwrap();
            let back: Json = serde_json::from_str(&cbor_to_json(&theirs).unwrap()).unwrap();
            assert_eq!(back, document);
        }
    }

    #[test]
    fn no_input_makes_the_reader_panic() {
        // Every one- and two-byte input, and every truncation and single-byte
        // corruption of a record.
        for first in 0..=255u8 {
            let _ = cbor_to_json(&[first]);
            for second in 0..=255u8 {
                let _ = cbor_to_json(&[first, second]);
            }
        }
        let record = json_to_cbor(
            r#"{"src_ip":[192,0,2,1],"message":{"t":-12.34,"mode":"Idle","n":null},"x":1e300}"#,
        )
        .unwrap();
        for cut in 0..record.len() {
            assert!(cbor_to_json(&record[..cut]).is_err(), "{cut}");
            for value in [0x00, 0x1f, 0x5f, 0x7f, 0x9f, 0xbf, 0xdf, 0xff] {
                let mut corrupt = record.clone();
                corrupt[cut] = value;
                let _ = cbor_to_json(&corrupt);
            }
        }
    }

    #[test]
    fn deeply_nested_input_is_an_error_not_a_crash() {
        let mut nested = vec![0x81; 100_000];
        nested.push(0x00);
        assert!(cbor_to_json(&nested).is_err());
    }

    #[test]
    fn a_payload_is_recognised_by_its_first_byte() {
        let json = r#"{"subsystem":"WX"}"#;
        assert_eq!(payload_to_json(json.as_bytes()).unwrap(), json);
        assert_eq!(
            payload_to_json(format!(" {json}").as_bytes()).unwrap(),
            json
        );
        let cbor = json_to_cbor(json).unwrap();
        assert_eq!(payload_to_json(&cbor).unwrap(), json);
        for other in [&b""[..], b"  ", b"[1]", b"small", &[0x83, 1, 2, 3]] {
            assert!(matches!(
                payload_to_json(other),
                Err(EncodingError::Unrecognised)
            ));
        }
    }

    #[test]
    fn configuration_names_json_or_cbor() {
        assert_eq!(Encoding::from_config(None).unwrap(), Encoding::Json);
        assert_eq!(Encoding::from_config(Some("json")).unwrap(), Encoding::Json);
        assert_eq!(Encoding::from_config(Some("cbor")).unwrap(), Encoding::Cbor);
        assert_eq!(
            Encoding::from_config(Some("msgpack"))
                .unwrap_err()
                .to_string(),
            "`encoding`: \"msgpack\" is not one of json, cbor"
        );
        assert_eq!(Encoding::Cbor.media_type(), "application/cbor");
        assert_eq!(Encoding::Cbor.extension(), "cbor");
        assert_eq!(Encoding::Json.media_type(), "application/json");
        assert_eq!(Encoding::Json.extension(), "json");
    }
}
