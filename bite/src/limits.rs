//! Limits on what a dictionary may ask of the decoder.
//!
//! A dictionary is supplied by whoever runs the decoder, and in a browser by
//! whoever opens the page. These limits bound the memory and the time one can
//! cost, whatever its files say. Anything over a limit is reported like any
//! other mistake in a dictionary and left out; `docs/dictionary-format.md`
//! lists the limits for dictionary authors.

/// Largest dictionary file read, in bytes. A larger file is reported and not
/// read.
pub const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;

/// Most files one dictionary may hold: `dictionary.json`, every
/// `metadata.json`, every message file and every vector together. A
/// dictionary with more does not load.
pub const MAX_FILES: usize = 10_000;

/// Most fields in one message. A message with more is left out.
pub const MAX_FIELDS_PER_MESSAGE: usize = 4_096;

/// Most entries in one `values` object: the labels of a field, or the
/// message names of an interface's `message_type`. A field with more is left
/// out; an interface with more identifies no message.
pub const MAX_VALUES: usize = 65_536;

/// Longest payload considered, in bytes: the largest UDP datagram or TCP
/// segment. A field that reaches past it could never be decoded and is left
/// out, so decoding never looks beyond this many bytes of a payload.
pub const MAX_PAYLOAD_BYTES: usize = 65_535;

/// [`MAX_PAYLOAD_BYTES`] in bits: the largest `stop` a field may have.
pub const MAX_PAYLOAD_BITS: usize = MAX_PAYLOAD_BYTES * 8;

/// Longest piece of a dictionary file quoted in an issue, in characters.
pub(crate) const MAX_QUOTED_CHARS: usize = 80;

/// `text` cut to [`MAX_QUOTED_CHARS`], with an ellipsis when it was longer.
pub(crate) fn quoted(text: &str) -> String {
    match text.char_indices().nth(MAX_QUOTED_CHARS) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_cuts_on_a_character_boundary() {
        assert_eq!(quoted("short"), "short");
        let long = "é".repeat(MAX_QUOTED_CHARS + 5);
        let cut = quoted(&long);
        assert_eq!(cut.chars().count(), MAX_QUOTED_CHARS + 1);
        assert!(cut.ends_with('…'));
    }
}
