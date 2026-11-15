//! Load an arbitrary dictionary from files in memory, then use it.
//!
//! Input: fields separated by NUL bytes. They are taken in pairs, a path
//! relative to the dictionary directory and the text of that file; a last
//! field without a partner is a payload. A dictionary that loads is asked for
//! its issues, its JSON Schemas and its test vectors, and decodes the payload
//! with every interface and every message.
#![no_main]

use bite::jsonschema::dictionary_schemas;
use bite::vectors::run_vectors;
use bite::Dictionary;
use first_bite_fuzz::exercise;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let fields: Vec<&[u8]> = data.split(|&byte| byte == 0).collect();
    let pairs = fields.chunks_exact(2);
    let payload = pairs.remainder().first().copied().unwrap_or(&[]);
    let files = pairs.map(|pair| {
        (
            String::from_utf8_lossy(pair[0]).into_owned(),
            String::from_utf8_lossy(pair[1]).into_owned(),
        )
    });
    let Ok(dictionary) = Dictionary::from_files(files) else {
        return;
    };

    for issue in dictionary.issues() {
        let _ = issue.to_string();
    }
    let _ = dictionary.has_errors();
    let _ = serde_json::to_string(&dictionary_schemas(&dictionary));
    if let Ok(results) = run_vectors(&dictionary) {
        for result in results {
            let _ = result.passed();
        }
    }

    for interface in dictionary.interfaces() {
        if let Some(port) = interface.port {
            let _ = dictionary.interface_for_port(port);
        }
        if let Ok(decoded) = interface.decode(payload) {
            exercise(&decoded);
        }
        if let Ok(decoded) = interface.decode_frame(payload, Some(payload.len() as u64)) {
            exercise(&decoded);
        }
        for message in interface.messages() {
            let _ = message.bit_length();
            for field in message.decode(payload) {
                let _ = field.render();
            }
        }
    }
});
