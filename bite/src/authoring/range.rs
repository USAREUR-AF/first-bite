//! Conversion between the way an interface document addresses bits ("word 3,
//! bits 12-5") and the dictionary's half-open range `[start, stop)`.
//!
//! A document's addressing has five parts, and every one of them changes the
//! answer:
//!
//! * the word size in bits;
//! * whether bit 0 of a word is its most or its least significant bit;
//! * whether the first word is word 0 or word 1;
//! * whether the first bit is bit 0 or bit 1;
//! * whether a word is sent most significant byte first (`big`) or least
//!   significant byte first (`little`).
//!
//! Big-endian words give a range under `"bit_order": "msb0"`; little-endian
//! words give a range under `"bit_order": "lsb0"`.

use super::json::Node;
use crate::schema::BitOrder;

/// Which end of a word the document calls bit 0 (or bit 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bit0 {
    Msb,
    Lsb,
}

/// The order a word's bytes are sent in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordOrder {
    Big,
    Little,
}

/// How a document numbers words and bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Numbering {
    pub word_bits: u64,
    pub bit0: Bit0,
    /// The number of the first word: 0 or 1.
    pub first_word: u64,
    /// The number of the first bit of a word: 0 or 1.
    pub first_bit: u64,
    pub word_order: WordOrder,
}

/// A field as a document addresses it. In one word, `first_bit` and
/// `last_bit` are its two end bits in either order. Across words, `first_bit`
/// is the field's end bit in `first_word` and `last_bit` its end bit in
/// `last_word`; the field takes every bit between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IcdRange {
    pub first_word: u64,
    pub last_word: u64,
    pub first_bit: u64,
    pub last_bit: u64,
}

/// A field as a dictionary addresses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u64,
    pub stop: u64,
    pub bit_order: BitOrder,
}

impl IcdRange {
    /// Parse a word (`3` or `4-5`) and bits (`7`, `12-5` or `5-12`).
    pub fn parse(word: &str, bits: &str) -> Result<IcdRange, String> {
        let pair = |text: &str, what: &str| -> Result<(u64, u64), String> {
            let text = text.trim();
            let parts: Vec<&str> = text.split(['-', '\u{2013}', ':']).map(str::trim).collect();
            let number = |part: &str| {
                part.parse::<u64>()
                    .map_err(|_| format!("{what} {text:?} is not a number or a pair like 12-5"))
            };
            match parts[..] {
                [one] => Ok((number(one)?, number(one)?)),
                [a, b] => Ok((number(a)?, number(b)?)),
                _ => Err(format!(
                    "{what} {text:?} is not a number or a pair like 12-5"
                )),
            }
        };
        let (first_word, last_word) = pair(word, "word")?;
        let (first_bit, last_bit) = pair(bits, "bits")?;
        if last_word < first_word {
            return Err(format!("word {word:?}: the first word comes first"));
        }
        Ok(IcdRange {
            first_word,
            last_word,
            first_bit,
            last_bit,
        })
    }

    /// The word as a document writes it: `3` or `4-5`.
    pub fn word_text(&self) -> String {
        if self.first_word == self.last_word {
            self.first_word.to_string()
        } else {
            format!("{}-{}", self.first_word, self.last_word)
        }
    }

    /// The bits as a document writes them: `7` or `12-5`.
    pub fn bits_text(&self) -> String {
        if self.first_word == self.last_word && self.first_bit == self.last_bit {
            self.first_bit.to_string()
        } else {
            format!("{}-{}", self.first_bit, self.last_bit)
        }
    }
}

impl Numbering {
    /// Check the numbering itself.
    pub fn validate(&self) -> Result<(), String> {
        if self.word_bits == 0 || self.word_bits > 64 {
            return Err(format!(
                "a word of {} bits: the word size is 1 to 64",
                self.word_bits
            ));
        }
        if self.first_word > 1 || self.first_bit > 1 {
            return Err("words and bits are numbered from 0 or from 1".to_string());
        }
        if self.word_order == WordOrder::Little && !self.word_bits.is_multiple_of(8) {
            return Err(format!(
                "a little-endian word is whole bytes, and {} bits is not",
                self.word_bits
            ));
        }
        Ok(())
    }

    /// The `bit_order` the converted ranges are in.
    pub fn bit_order(&self) -> BitOrder {
        match self.word_order {
            WordOrder::Big => BitOrder::Msb0,
            WordOrder::Little => BitOrder::Lsb0,
        }
    }

    /// The offset of a bit inside its word, in the dictionary's numbering of
    /// that word: from the most significant end for big-endian words, from the
    /// least significant end for little-endian ones.
    fn offset_in_word(&self, bit: u64) -> u64 {
        let index = bit - self.first_bit;
        let from_msb = matches!(self.bit0, Bit0::Msb);
        let wants_msb = matches!(self.word_order, WordOrder::Big);
        if from_msb == wants_msb {
            index
        } else {
            self.word_bits - 1 - index
        }
    }

    fn position(&self, word: u64, bit: u64) -> Result<u64, String> {
        if word < self.first_word {
            return Err(format!(
                "word {word}: words are numbered from {}",
                self.first_word
            ));
        }
        if bit < self.first_bit || bit - self.first_bit >= self.word_bits {
            return Err(format!(
                "bit {bit}: a {}-bit word has bits {} to {}",
                self.word_bits,
                self.first_bit,
                self.first_bit + self.word_bits - 1
            ));
        }
        (word - self.first_word)
            .checked_mul(self.word_bits)
            .and_then(|base| base.checked_add(self.offset_in_word(bit)))
            .ok_or_else(|| format!("word {word} is out of range"))
    }

    /// The dictionary range of a field the document addresses as `range`.
    pub fn to_span(&self, range: &IcdRange) -> Result<Span, String> {
        self.validate()?;
        let a = self.position(range.first_word, range.first_bit)?;
        let b = self.position(range.last_word, range.last_bit)?;
        Ok(Span {
            start: a.min(b),
            stop: a.max(b) + 1,
            bit_order: self.bit_order(),
        })
    }

    /// The bit number, inside its word, of a dictionary bit position.
    fn bit_number(&self, position: u64) -> u64 {
        // `offset_in_word` is its own inverse.
        self.offset_in_word(position % self.word_bits + self.first_bit) + self.first_bit
    }

    /// How the document addresses the dictionary range `[start, stop)`. In one
    /// word the bits read most significant first.
    pub fn to_icd(&self, start: u64, stop: u64) -> Result<IcdRange, String> {
        self.validate()?;
        if stop <= start {
            return Err(format!("[{start}, {stop}) is empty or inverted"));
        }
        let last = stop - 1;
        let first_word = start / self.word_bits + self.first_word;
        let last_word = last / self.word_bits + self.first_word;
        let (first, last) = (self.bit_number(start), self.bit_number(last));
        let most_significant_first = match self.word_order {
            WordOrder::Big => (first, last),
            WordOrder::Little if first_word == last_word => (last, first),
            WordOrder::Little => (first, last),
        };
        Ok(IcdRange {
            first_word,
            last_word,
            first_bit: most_significant_first.0,
            last_bit: most_significant_first.1,
        })
    }

    /// The numbering in words, for the reader to check against the document.
    pub fn describe(&self) -> String {
        format!(
            "{}-bit words numbered from {}, sent {} significant byte first; bit {} is the {} significant bit of a word",
            self.word_bits,
            self.first_word,
            match self.word_order {
                WordOrder::Big => "most",
                WordOrder::Little => "least",
            },
            self.first_bit,
            match self.bit0 {
                Bit0::Msb => "most",
                Bit0::Lsb => "least",
            },
        )
    }

    /// A picture of each word the range touches: the document's bit numbers,
    /// most significant on the left, with `#` under the bits of the field.
    pub fn ruler(&self, span: &Span) -> String {
        let mut out = String::new();
        let cell = if self.word_bits + self.first_bit > 10 {
            3
        } else {
            2
        };
        let first = span.start / self.word_bits;
        let last = (span.stop - 1) / self.word_bits;
        for word in first..=last {
            let base = word * self.word_bits;
            let bytes = if self.word_bits.is_multiple_of(8) {
                let from = base / 8;
                let to = from + self.word_bits / 8 - 1;
                if from == to {
                    format!(" (payload byte {from})")
                } else {
                    format!(" (payload bytes {from}-{to})")
                }
            } else {
                String::new()
            };
            out.push_str(&format!("word {}{bytes}\n", word + self.first_word));
            let mut numbers = String::from(" ");
            let mut marks = String::from(" ");
            // Most significant bit of the word on the left.
            for column in 0..self.word_bits {
                let position = match self.word_order {
                    WordOrder::Big => base + column,
                    WordOrder::Little => base + self.word_bits - 1 - column,
                };
                let inside = position >= span.start && position < span.stop;
                numbers.push_str(&format!("{:>cell$}", self.bit_number(position)));
                marks.push_str(&format!("{:>cell$}", if inside { '#' } else { '.' }));
            }
            out.push_str(&numbers);
            out.push('\n');
            out.push_str(&marks);
            out.push('\n');
        }
        out
    }

    /// Read an interface's `icd_numbering` object.
    pub fn read(node: &Node) -> Result<Numbering, String> {
        let number = |key: &str, default: Option<u64>| -> Result<u64, String> {
            match node.get(key) {
                None => default.ok_or_else(|| format!("icd_numbering has no {key}")),
                Some(value) => value
                    .as_u64()
                    .ok_or_else(|| format!("icd_numbering.{key} is not a whole number")),
            }
        };
        let numbering = Numbering {
            word_bits: number("word_bits", None)?,
            bit0: parse_bit0(
                node.str_at("bit0")
                    .ok_or("icd_numbering has no bit0 (\"msb\" or \"lsb\")")?,
            )?,
            first_word: number("first_word", None)?,
            first_bit: number("first_bit", Some(0))?,
            word_order: parse_word_order(node.str_at("word_order").unwrap_or("big"))?,
        };
        numbering.validate()?;
        Ok(numbering)
    }
}

pub fn parse_bit0(text: &str) -> Result<Bit0, String> {
    match text.trim().to_ascii_lowercase().as_str() {
        "msb" => Ok(Bit0::Msb),
        "lsb" => Ok(Bit0::Lsb),
        other => Err(format!("bit0 is \"msb\" or \"lsb\", not {other:?}")),
    }
}

pub fn parse_word_order(text: &str) -> Result<WordOrder, String> {
    match text.trim().to_ascii_lowercase().as_str() {
        "big" => Ok(WordOrder::Big),
        "little" => Ok(WordOrder::Little),
        other => Err(format!(
            "word_order is \"big\" or \"little\", not {other:?}"
        )),
    }
}

pub fn bit_order_name(order: BitOrder) -> &'static str {
    match order {
        BitOrder::Msb0 => "msb0",
        BitOrder::Lsb0 => "lsb0",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bits::extract;

    fn numbering(
        word_bits: u64,
        bit0: Bit0,
        first_word: u64,
        first_bit: u64,
        word_order: WordOrder,
    ) -> Numbering {
        Numbering {
            word_bits,
            bit0,
            first_word,
            first_bit,
            word_order,
        }
    }

    /// Cases worked by hand: numbering, word, bits, expected start and stop.
    #[test]
    fn hand_worked_table() {
        use Bit0::{Lsb, Msb};
        use WordOrder::{Big, Little};
        #[rustfmt::skip]
        // Word size, bit 0, first word, first bit, word order; word, bits; start, stop.
        type Case = (u64, Bit0, u64, u64, WordOrder, &'static str, &'static str, u64, u64);
        let cases: &[Case] = &[
            // 16-bit words from 1, bit 0 the LSB: word 3 starts at bit 32;
            // bit 12 is 3 from the top, bit 5 is 10 from the top.
            (16, Lsb, 1, 0, Big, "3", "12-5", 35, 43),
            (16, Lsb, 1, 0, Big, "3", "5-12", 35, 43),
            (16, Lsb, 1, 0, Big, "1", "15-0", 0, 16),
            (16, Lsb, 1, 0, Big, "1", "15", 0, 1),
            (16, Lsb, 1, 0, Big, "1", "0", 15, 16),
            // The same word numbered from 0.
            (16, Lsb, 0, 0, Big, "2", "12-5", 35, 43),
            // Bit 0 the MSB: bit numbers are offsets.
            (16, Msb, 1, 0, Big, "3", "3-10", 35, 43),
            (16, Msb, 0, 0, Big, "0", "0-7", 0, 8),
            // Bits numbered from 1, bit 1 the MSB.
            (16, Msb, 1, 1, Big, "3", "4-11", 35, 43),
            // Bits numbered from 1, bit 1 the LSB: bits 16..1.
            (16, Lsb, 1, 1, Big, "3", "13-6", 35, 43),
            // Bytes from 0, bit 7 the MSB: byte 10 bits 5-4.
            (8, Lsb, 0, 0, Big, "10", "5-4", 82, 84),
            (8, Lsb, 0, 0, Big, "8-9", "7-0", 64, 80),
            // 32-bit words from 0, bit 31 the MSB.
            (32, Lsb, 0, 0, Big, "1", "31-28", 32, 36),
            (32, Lsb, 0, 0, Big, "1", "7-0", 56, 64),
            // Across words: word 5 bits 15-0 then word 6 bits 15-8 (24 bits).
            (16, Lsb, 1, 0, Big, "5-6", "15-8", 64, 88),
            // Word 6 bits 7-0 then word 7 bits 15-0 (24 bits).
            (16, Lsb, 1, 0, Big, "6-7", "7-0", 88, 112),
            // Word 4 bits 3-0 then word 5 bits 15-4 (16 bits).
            (16, Lsb, 1, 0, Big, "4-5", "3-4", 60, 76),
            // Little-endian 16-bit words under lsb0: word 3 from 1 starts at
            // bit 32, and bit numbers are offsets from the low end.
            (16, Lsb, 1, 0, Little, "3", "12-5", 37, 45),
            (16, Lsb, 0, 0, Little, "0", "15-0", 0, 16),
            // Across little-endian words the low part comes first: word 1
            // bits 15-12, then word 2 bits 11-0.
            (16, Lsb, 1, 0, Little, "1-2", "12-11", 12, 28),
            // Little-endian words whose bit 0 is the MSB.
            (16, Msb, 1, 0, Little, "1", "0-3", 12, 16),
        ];
        for &(word_bits, bit0, first_word, first_bit, order, word, bits, start, stop) in cases {
            let numbering = numbering(word_bits, bit0, first_word, first_bit, order);
            let range = IcdRange::parse(word, bits).unwrap();
            let span = numbering.to_span(&range).unwrap();
            assert_eq!(
                (span.start, span.stop),
                (start, stop),
                "{} word {word} bits {bits}",
                numbering.describe()
            );
            // And back: the reverse names the same bits.
            let back = numbering.to_icd(start, stop).unwrap();
            let again = numbering.to_span(&back).unwrap();
            assert_eq!((again.start, again.stop), (start, stop), "{back:?}");
        }
    }

    #[test]
    fn the_reverse_reads_most_significant_first() {
        let lsb = numbering(16, Bit0::Lsb, 1, 0, WordOrder::Big);
        let range = lsb.to_icd(35, 43).unwrap();
        assert_eq!(
            (range.word_text().as_str(), range.bits_text().as_str()),
            ("3", "12-5")
        );
        let msb = numbering(16, Bit0::Msb, 1, 0, WordOrder::Big);
        let range = msb.to_icd(35, 43).unwrap();
        assert_eq!(range.bits_text(), "3-10");
        let range = lsb.to_icd(60, 76).unwrap();
        assert_eq!(
            (range.word_text().as_str(), range.bits_text().as_str()),
            ("4-5", "3-4")
        );
        let range = lsb.to_icd(15, 16).unwrap();
        assert_eq!(range.bits_text(), "0");
        let little = numbering(16, Bit0::Lsb, 1, 0, WordOrder::Little);
        let range = little.to_icd(37, 45).unwrap();
        assert_eq!(
            (range.word_text().as_str(), range.bits_text().as_str()),
            ("3", "12-5")
        );
    }

    /// The converted range selects, in a payload, the bits the document means.
    #[test]
    fn agrees_with_the_decoder() {
        // Words 0x0000 0x0000 0x1FE0: word 3 bits 12-5 all ones.
        let payload = [0, 0, 0, 0, 0x1F, 0xE0];
        let big = numbering(16, Bit0::Lsb, 1, 0, WordOrder::Big);
        let span = big.to_span(&IcdRange::parse("3", "12-5").unwrap()).unwrap();
        assert_eq!(
            extract(
                &payload,
                span.start as usize,
                span.stop as usize,
                span.bit_order
            ),
            Some(0xFF)
        );
        // The same word sent low byte first.
        let payload = [0, 0, 0, 0, 0xE0, 0x1F];
        let little = numbering(16, Bit0::Lsb, 1, 0, WordOrder::Little);
        let span = little
            .to_span(&IcdRange::parse("3", "12-5").unwrap())
            .unwrap();
        assert_eq!(
            extract(
                &payload,
                span.start as usize,
                span.stop as usize,
                span.bit_order
            ),
            Some(0xFF)
        );
        // A 16-bit value 0xABCD in word 1 bits 15-12 and word 2 bits 11-0,
        // little-endian: low nibble first.
        let value: u32 = 0xABCD << 12;
        let payload = value.to_le_bytes();
        let span = little
            .to_span(&IcdRange::parse("1-2", "12-11").unwrap())
            .unwrap();
        assert_eq!(
            extract(
                &payload,
                span.start as usize,
                span.stop as usize,
                span.bit_order
            ),
            Some(0xABCD)
        );
    }

    #[test]
    fn bad_addresses_are_refused() {
        let numbering = numbering(16, Bit0::Lsb, 1, 0, WordOrder::Big);
        assert!(numbering
            .to_span(&IcdRange::parse("0", "3").unwrap())
            .is_err());
        assert!(numbering
            .to_span(&IcdRange::parse("1", "16").unwrap())
            .is_err());
        assert!(IcdRange::parse("5-4", "3").is_err());
        assert!(IcdRange::parse("x", "3").is_err());
        assert!(IcdRange::parse("1", "1-2-3").is_err());
        let odd = Numbering {
            word_bits: 12,
            word_order: WordOrder::Little,
            ..numbering
        };
        assert!(odd.validate().is_err());
    }

    #[test]
    fn ruler_marks_the_field() {
        let numbering = numbering(16, Bit0::Lsb, 1, 0, WordOrder::Big);
        let span = numbering
            .to_span(&IcdRange::parse("3", "12-5").unwrap())
            .unwrap();
        assert_eq!(
            numbering.ruler(&span),
            "word 3 (payload bytes 4-5)\n  15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0\n   .  .  .  #  #  #  #  #  #  #  #  .  .  .  .  .\n"
        );
    }
}
