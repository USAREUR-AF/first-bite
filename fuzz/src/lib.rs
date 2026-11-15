//! What the fuzz targets share: the shipped dictionaries, embedded at build
//! time, and the walk over everything a decoded message can be asked for.

use bite::{Decoded, Dictionary};

include!(concat!(env!("OUT_DIR"), "/shipped.rs"));

/// Every dictionary under `dictionaries/`, built from the embedded files.
/// A shipped dictionary that does not load cleanly stops the target at start.
pub fn shipped() -> Vec<(&'static str, Dictionary)> {
    SHIPPED
        .iter()
        .map(|(name, files)| {
            let dictionary = Dictionary::from_files(files.iter().copied())
                .unwrap_or_else(|e| panic!("shipped dictionary {name}: {e}"));
            assert!(
                !dictionary.has_errors(),
                "shipped dictionary {name} has errors: {:?}",
                dictionary.issues()
            );
            (*name, dictionary)
        })
        .collect()
}

/// Ask a decoded message for everything the pipeline and the command line ask
/// of it: its type, each field rendered, and its JSON form.
pub fn exercise(decoded: &Decoded<'_>) {
    let _ = decoded.message_type();
    for field in &decoded.fields {
        let _ = field.render();
    }
    let rendered = decoded.rendered();
    let _ = serde_json::to_string(&rendered);
    // A non-finite float is the one value serde_json refuses; that is an
    // error, never a panic.
    let _ = serde_json::to_string(decoded);
}
