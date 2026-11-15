//! Building a payload from field values: the inverse of decoding.
//!
//! This makes the bytes of a test vector from the values the interface
//! document says they hold. It shares the dictionary with the decoder, so a
//! payload built here and decoded there proves nothing about the dictionary:
//! the expected values of a vector still come from the document, and the
//! bytes are checked against it by hand or by a second implementation.

use super::model::{fields, InterfaceTree};
use crate::bits::swap_bytes;
use crate::dictionary::{Field, Interface, Message};
use crate::schema::{parse_int, BitOrder, ByteOrder, DiscriminatorSource, FieldKind};

/// One field written into a payload.
#[derive(Debug, Clone, PartialEq)]
pub struct EncodedField {
    pub name: String,
    pub start: usize,
    pub stop: usize,
    /// The value as given.
    pub given: String,
    /// The bits written, as an unsigned integer; `None` for bytes and text.
    pub raw: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Encoded {
    pub payload: Vec<u8>,
    /// The fields that were given a value, in bit order.
    pub fields: Vec<EncodedField>,
    /// Fields of the message that were given no value and are left zero.
    pub left_zero: Vec<String>,
}

/// Write the low `stop - start` bits of `raw` into `data` at `[start, stop)`.
/// The inverse of [`crate::bits::extract`].
pub fn insert(data: &mut [u8], start: usize, stop: usize, order: BitOrder, raw: u64) {
    let width = stop - start;
    for index in 0..width {
        let (position, bit) = match order {
            // Bit `start` is the most significant bit of the value.
            BitOrder::Msb0 => (start + index, (raw >> (width - 1 - index)) & 1),
            // Bit `start` is the least significant bit of the value.
            BitOrder::Lsb0 => (start + index, (raw >> index) & 1),
        };
        let mask = match order {
            BitOrder::Msb0 => 0x80u8 >> (position % 8),
            BitOrder::Lsb0 => 1u8 << (position % 8),
        };
        if bit == 1 {
            data[position / 8] |= mask;
        } else {
            data[position / 8] &= !mask;
        }
    }
}

/// The raw integer a label of the field stands for.
fn label_value(interface: &InterfaceTree, message: &str, field: &str, text: &str) -> Option<i128> {
    let node = interface.message(message)?;
    let view = fields(node).into_iter().find(|view| view.name == field)?;
    view.node
        .get("values")?
        .entries()
        .iter()
        .find(|(_, label)| label.as_str() == Some(text))
        .and_then(|(key, _)| parse_int(key))
}

fn fits(raw: i128, width: usize, signed: bool) -> bool {
    if signed {
        let half = 1i128 << (width - 1);
        raw >= -half && raw < half
    } else {
        raw >= 0 && raw < (1i128 << width)
    }
}

/// The bits of an integer or boolean field for the value `text`.
fn integer_bits(field: &Field, text: &str, label: Option<i128>) -> Result<u64, String> {
    let width = field.width();
    let signed = field.kind == FieldKind::Int;
    let scaled = field.scale.is_some() || field.offset.is_some();
    let raw: i128 = if let Some(raw) = label {
        raw
    } else if let Some(raw) = text.strip_prefix("raw:") {
        parse_int(raw).ok_or_else(|| format!("{raw:?} is not a whole number"))?
    } else if field.kind == FieldKind::Bool && (text == "true" || text == "false") {
        i128::from(text == "true")
    } else if scaled {
        let value: f64 = text
            .parse()
            .map_err(|_| format!("{text:?} is not a number or a label of the field"))?;
        let scale = field.scale.unwrap_or(1.0);
        let offset = field.offset.unwrap_or(0.0);
        let exact = (value - offset) / scale;
        let raw = exact.round();
        let back = raw * scale + offset;
        if !raw.is_finite() || (back - value).abs() > 1e-9 * value.abs().max(1.0) {
            let below = exact.floor() * scale + offset;
            let above = exact.ceil() * scale + offset;
            return Err(format!(
                "{text} is not a value the field can hold: the nearest are {below} and {above}"
            ));
        }
        raw as i128
    } else {
        parse_int(text)
            .ok_or_else(|| format!("{text:?} is not a whole number or a label of the field"))?
    };
    if !fits(raw, width, signed) {
        return Err(format!(
            "raw value {raw} does not fit {} {width}-bit field",
            if signed { "a signed" } else { "an unsigned" }
        ));
    }
    let mask = if width == 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    };
    Ok((raw as i64 as u64) & mask)
}

fn float_bits(field: &Field, text: &str) -> Result<u64, String> {
    let value: f64 = text
        .parse()
        .map_err(|_| format!("{text:?} is not a number"))?;
    let value = (value - field.offset.unwrap_or(0.0)) / field.scale.unwrap_or(1.0);
    Ok(if field.width() == 32 {
        u64::from((value as f32).to_bits())
    } else {
        value.to_bits()
    })
}

/// Build the payload of `message` from `values`: field name and the value as
/// text. A value is the label where the field has one for it, the engineering
/// value for a scaled field, `raw:<n>` for the raw integer of any integer
/// field, hex for a bytes field, the text for an ascii field.
///
/// The payload is `length` bytes; without it, the declared length of the
/// message, or else the end of its last field. The decoder strips both NUL
/// and space padding from text, so the padding of an ascii field cannot be
/// known from its value: it is NUL, or space where `reference` (a payload the
/// result will be compared with) has a space at that byte. Where the interface identifies
/// messages by payload bits, those bits are set to the message's identifier
/// unless a value covers them. Every other bit not given is zero.
pub fn encode(
    interface: &Interface,
    labels: &InterfaceTree,
    message: &Message,
    values: &[(String, String)],
    length: Option<usize>,
    reference: Option<&[u8]>,
) -> Result<Encoded, Vec<String>> {
    let mut errors = Vec::new();
    let declared = labels
        .message(&message.name)
        .and_then(|node| node.u64_at("declared_bits"))
        .map_or(0, |bits| bits as usize);
    let bytes = length.unwrap_or_else(|| declared.max(message.bit_length()).div_ceil(8));
    let mut payload = vec![0u8; bytes];

    let disc = &interface.discriminator;
    let mut identified = false;
    if disc.source == DiscriminatorSource::Payload && disc.stop <= bytes * 8 {
        let mut codes: Vec<u64> = disc
            .names
            .iter()
            .filter(|(_, name)| **name == message.name)
            .map(|(code, _)| *code)
            .collect();
        codes.sort_unstable();
        if let Some(code) = codes.first() {
            insert(
                &mut payload,
                disc.start,
                disc.stop,
                interface.bit_order,
                *code,
            );
            identified = true;
        }
    }

    for (name, _) in values {
        if message.field(name).is_none() {
            errors.push(format!("{name}: no such field in {}", message.name));
        }
        if values.iter().filter(|(other, _)| other == name).count() > 1 {
            errors.push(format!("{name}: given more than once"));
        }
    }
    errors.sort();
    errors.dedup();

    let mut encoded = Vec::new();
    let mut left_zero = Vec::new();
    for field in &message.fields {
        let Some((_, text)) = values.iter().find(|(name, _)| name == &field.name) else {
            // The field on the message type bits holds the identifier.
            if !(identified && (field.start, field.stop) == (disc.start, disc.stop)) {
                left_zero.push(field.name.clone());
            }
            continue;
        };
        if field.stop > bytes * 8 {
            errors.push(format!(
                "{}: bits [{}, {}) lie past the end of a {bytes}-byte payload",
                field.name, field.start, field.stop
            ));
            continue;
        }
        let raw = match field.kind {
            FieldKind::Bytes => {
                let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
                match hex::decode(&compact) {
                    Ok(data) if data.len() == field.width() / 8 => {
                        payload[field.start / 8..field.stop / 8].copy_from_slice(&data);
                        Ok(None)
                    }
                    Ok(data) => Err(format!(
                        "{} bytes given for a field of {}",
                        data.len(),
                        field.width() / 8
                    )),
                    Err(e) => Err(format!("{text:?} is not hex: {e}")),
                }
            }
            FieldKind::Ascii => {
                let room = field.width() / 8;
                if !text.is_ascii() {
                    Err(format!("{text:?} is not ASCII"))
                } else if text.len() > room {
                    Err(format!(
                        "{} characters given for a field of {room}",
                        text.len()
                    ))
                } else {
                    let slot = &mut payload[field.start / 8..field.stop / 8];
                    slot.fill(0);
                    slot[..text.len()].copy_from_slice(text.as_bytes());
                    for (index, byte) in slot.iter_mut().enumerate().skip(text.len()) {
                        let at = field.start / 8 + index;
                        if reference.and_then(|payload| payload.get(at)) == Some(&b' ') {
                            *byte = b' ';
                        }
                    }
                    Ok(None)
                }
            }
            FieldKind::Float => float_bits(field, text).map(Some),
            FieldKind::Uint | FieldKind::Int | FieldKind::Bool => integer_bits(
                field,
                text,
                label_value(labels, &message.name, &field.name, text),
            )
            .map(Some),
        };
        match raw {
            Err(message) => errors.push(format!("{}: {message}", field.name)),
            Ok(raw) => {
                if let Some(raw) = raw {
                    let width = field.width();
                    let swapped = field.byte_order == ByteOrder::Little
                        && field.bit_order == BitOrder::Msb0
                        && width > 8;
                    let wire = if swapped { swap_bytes(raw, width) } else { raw };
                    insert(&mut payload, field.start, field.stop, field.bit_order, wire);
                }
                encoded.push(EncodedField {
                    name: field.name.clone(),
                    start: field.start,
                    stop: field.stop,
                    given: text.clone(),
                    raw,
                });
            }
        }
    }
    if errors.is_empty() {
        Ok(Encoded {
            payload,
            fields: encoded,
            left_zero,
        })
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bits::extract;

    #[test]
    fn insert_is_the_inverse_of_extract() {
        for order in [BitOrder::Msb0, BitOrder::Lsb0] {
            for (start, stop, raw) in [
                (0, 8, 0xA5u64),
                (3, 17, 0x2ABC),
                (7, 9, 0b10),
                (5, 69, 0xDEAD_BEEF_0123_4567),
                (40, 41, 1),
            ] {
                let mut data = [0xFFu8; 10];
                insert(&mut data, start, stop, order, raw);
                assert_eq!(
                    extract(&data, start, stop, order),
                    Some(raw),
                    "{order:?} [{start}, {stop})"
                );
                // Bits outside the field are untouched.
                let mut zero = [0u8; 10];
                insert(&mut zero, start, stop, order, raw);
                let ones: u32 = zero.iter().map(|b| b.count_ones()).sum();
                assert_eq!(ones, raw.count_ones());
            }
        }
    }

    #[test]
    fn signed_and_unsigned_ranges() {
        assert!(fits(255, 8, false) && !fits(256, 8, false) && !fits(-1, 8, false));
        assert!(fits(-128, 8, true) && fits(127, 8, true) && !fits(128, 8, true));
        assert!(fits(u64::MAX as i128, 64, false) && fits(i64::MIN as i128, 64, true));
    }
}
