//! Decoding a payload against an interface.

use crate::bits::{extract, sign_extend, swap_bytes};
use crate::dictionary::{Field, Interface, Message};
use crate::schema::{BitOrder, ByteOrder, DiscriminatorSource, FieldKind};
use serde::ser::{SerializeMap, Serializer};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DecodeError {
    #[error("payload of {payload_bits} bits is too short to hold the message type at bits [{start}, {stop})")]
    TooShortForMessageType {
        payload_bits: usize,
        start: usize,
        stop: usize,
    },
    #[error("interface {interface} identifies messages by frame identifier, and none was given")]
    MissingFrameId { interface: String },
}

/// A decoded field value.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Value {
    UInt(u64),
    Int(i64),
    Float(f64),
    Bool(bool),
    #[serde(serialize_with = "serialize_hex")]
    Bytes(Vec<u8>),
    Text(String),
}

fn serialize_hex<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&hex::encode(bytes))
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::UInt(value) => write!(f, "{value}"),
            Value::Int(value) => write!(f, "{value}"),
            Value::Float(value) => write!(f, "{value}"),
            Value::Bool(value) => write!(f, "{value}"),
            Value::Bytes(bytes) => f.write_str(&hex::encode(bytes)),
            Value::Text(text) => f.write_str(text),
        }
    }
}

/// One field of a decoded message.
#[derive(Debug, Clone)]
pub struct DecodedField<'d> {
    pub field: &'d Field,
    /// `None` when the field lies beyond the end of the payload.
    pub value: Option<Value>,
    /// The dictionary's name for the raw value, when it has one.
    pub label: Option<&'d str>,
}

impl DecodedField<'_> {
    /// The label if the dictionary names this value, otherwise the value.
    pub fn render(&self) -> Option<String> {
        match (self.label, &self.value) {
            (Some(label), _) => Some(label.to_string()),
            (None, Some(value)) => Some(value.to_string()),
            (None, None) => None,
        }
    }
}

/// A payload decoded against an interface.
#[derive(Debug, Clone)]
pub struct Decoded<'d> {
    pub interface: &'d Interface,
    /// The number that identified the message.
    pub type_code: u64,
    /// The name the interface gives `type_code`, if any.
    pub type_name: Option<&'d str>,
    /// The message definition; `None` when the interface names the type but has
    /// no definition for it, or does not know the type at all.
    pub message: Option<&'d Message>,
    /// In the order of the message definition: by start bit.
    pub fields: Vec<DecodedField<'d>>,
}

impl Decoded<'_> {
    /// The message name, or the type code in decimal when the interface has no
    /// name for it.
    pub fn message_type(&self) -> String {
        match self.type_name {
            Some(name) => name.to_string(),
            None => self.type_code.to_string(),
        }
    }

    pub fn field(&self, name: &str) -> Option<&DecodedField<'_>> {
        self.fields
            .iter()
            .find(|decoded| decoded.field.name == name)
    }

    /// Every field rendered as text: the label where the dictionary has one,
    /// the value in decimal otherwise, `None` for a field beyond the payload.
    pub fn rendered(&self) -> BTreeMap<&str, Option<String>> {
        self.fields
            .iter()
            .map(|decoded| (decoded.field.name.as_str(), decoded.render()))
            .collect()
    }
}

/// Serialises as a map of field name to label or typed value.
impl Serialize for Decoded<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.fields.len()))?;
        for decoded in &self.fields {
            match decoded.label {
                Some(label) => map.serialize_entry(&decoded.field.name, label)?,
                None => map.serialize_entry(&decoded.field.name, &decoded.value)?,
            }
        }
        map.end()
    }
}

impl Interface {
    /// The number that identifies the message in `payload`.
    pub fn type_code(&self, payload: &[u8], frame_id: Option<u64>) -> Result<u64, DecodeError> {
        let disc = &self.discriminator;
        match disc.source {
            DiscriminatorSource::Payload => extract(payload, disc.start, disc.stop, self.bit_order)
                .ok_or(DecodeError::TooShortForMessageType {
                    payload_bits: payload.len() * 8,
                    start: disc.start,
                    stop: disc.stop,
                }),
            DiscriminatorSource::FrameId => {
                let id = frame_id.ok_or_else(|| DecodeError::MissingFrameId {
                    interface: self.name.clone(),
                })?;
                Ok(disc.mask.map_or(id, |mask| id & mask))
            }
        }
    }

    /// Decode a payload whose message type is carried in the payload.
    pub fn decode<'d>(&'d self, payload: &[u8]) -> Result<Decoded<'d>, DecodeError> {
        self.decode_frame(payload, None)
    }

    /// Decode a payload that arrived with a frame identifier.
    pub fn decode_frame<'d>(
        &'d self,
        payload: &[u8],
        frame_id: Option<u64>,
    ) -> Result<Decoded<'d>, DecodeError> {
        let type_code = self.type_code(payload, frame_id)?;
        let type_name = self.discriminator.names.get(&type_code).map(String::as_str);
        let message = type_name.and_then(|name| self.message(name));
        let fields = message.map_or_else(Vec::new, |message| message.decode(payload));
        Ok(Decoded {
            interface: self,
            type_code,
            type_name,
            message,
            fields,
        })
    }
}

impl Message {
    /// Decode every field. A field beyond the end of `payload` has no value.
    pub fn decode<'d>(&'d self, payload: &[u8]) -> Vec<DecodedField<'d>> {
        self.fields
            .iter()
            .map(|field| field.decode(payload))
            .collect()
    }
}

impl Field {
    pub fn decode<'d>(&'d self, payload: &[u8]) -> DecodedField<'d> {
        let (value, label) = match self.kind {
            FieldKind::Bytes | FieldKind::Ascii => (self.decode_bytes(payload), None),
            _ => self.decode_number(payload),
        };
        DecodedField {
            field: self,
            value,
            label,
        }
    }

    fn decode_bytes(&self, payload: &[u8]) -> Option<Value> {
        let bytes = payload.get(self.start / 8..self.stop / 8)?;
        Some(match self.kind {
            FieldKind::Ascii => Value::Text(
                String::from_utf8_lossy(bytes)
                    .trim_end_matches(['\0', ' '])
                    .to_string(),
            ),
            _ => Value::Bytes(bytes.to_vec()),
        })
    }

    fn decode_number(&self, payload: &[u8]) -> (Option<Value>, Option<&str>) {
        let width = self.width();
        let Some(mut raw) = extract(payload, self.start, self.stop, self.bit_order) else {
            return (None, None);
        };
        if self.byte_order == ByteOrder::Little && self.bit_order == BitOrder::Msb0 && width > 8 {
            raw = swap_bytes(raw, width);
        }
        match self.kind {
            FieldKind::Float => {
                let value = if width == 32 {
                    f32::from_bits(raw as u32) as f64
                } else {
                    f64::from_bits(raw)
                };
                (Some(Value::Float(self.scaled(value))), None)
            }
            FieldKind::Bool => {
                let label = self.label(raw as i128);
                (Some(Value::Bool(raw != 0)), label)
            }
            FieldKind::Int => {
                let value = sign_extend(raw, width);
                let label = self.label(value as i128);
                (Some(self.integer(Value::Int(value), value as f64)), label)
            }
            _ => {
                let label = self.label(raw as i128);
                (Some(self.integer(Value::UInt(raw), raw as f64)), label)
            }
        }
    }

    fn integer(&self, raw: Value, as_float: f64) -> Value {
        if self.scale.is_some() || self.offset.is_some() {
            Value::Float(self.scaled(as_float))
        } else {
            raw
        }
    }

    fn scaled(&self, value: f64) -> f64 {
        value * self.scale.unwrap_or(1.0) + self.offset.unwrap_or(0.0)
    }
}
