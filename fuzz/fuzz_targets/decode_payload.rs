//! Decode an arbitrary payload against every interface of every shipped
//! dictionary.
//!
//! Input: 8 bytes of frame identifier (little-endian), then the payload. The
//! payload is decoded three ways: with its message type read from the payload,
//! with the frame identifier, and as each message of the interface in turn
//! whatever its type says, so that field decoding always sees the bytes.
#![no_main]

use bite::Dictionary;
use first_bite_fuzz::{exercise, shipped};
use libfuzzer_sys::fuzz_target;
use std::sync::OnceLock;

static DICTIONARIES: OnceLock<Vec<(&'static str, Dictionary)>> = OnceLock::new();

fuzz_target!(|data: &[u8]| {
    let dictionaries = DICTIONARIES.get_or_init(shipped);
    let (frame_id, payload) = match data.split_first_chunk::<8>() {
        Some((id, rest)) => (u64::from_le_bytes(*id), rest),
        None => (0, data),
    };
    for (_, dictionary) in dictionaries {
        for interface in dictionary.interfaces() {
            let _ = interface.type_code(payload, Some(frame_id));
            if let Ok(decoded) = interface.decode(payload) {
                exercise(&decoded);
            }
            if let Ok(decoded) = interface.decode_frame(payload, Some(frame_id)) {
                exercise(&decoded);
            }
            for message in interface.messages() {
                for field in message.decode(payload) {
                    let _ = field.render();
                }
            }
        }
    }
});
