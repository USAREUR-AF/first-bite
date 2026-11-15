//! Decode fixed-layout binary messages against a declarative dictionary.
//!
//! A dictionary is a directory of JSON files ([`schema`]) that names the
//! messages of one or more interfaces and gives the bit range, type and value
//! labels of every field. Nothing about a particular message set is compiled
//! into this crate. [`Dictionary::load`] reads the directory;
//! [`Dictionary::from_files`] takes the same files from memory, for targets
//! with no filesystem.
//!
//! ```no_run
//! use bite::Dictionary;
//!
//! let dictionary = Dictionary::load("dictionaries/toot")?;
//! let interface = dictionary.interface_for_port(22200).expect("known port");
//! let payload = [0u8; 14];
//! let decoded = interface.decode(&payload)?;
//! println!("{}: {}", decoded.message_type(), serde_json::to_string(&decoded)?);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod authoring;
pub mod bits;
pub mod decode;
pub mod dictionary;
pub mod jsonschema;
pub mod limits;
pub mod schema;
pub mod vectors;

pub use decode::{DecodeError, Decoded, DecodedField, Value};
pub use dictionary::{
    Dictionary, Field, Interface, Issue, LoadError, Message, Severity, VectorFile,
};
pub use schema::{BitOrder, ByteOrder, FieldKind};

/// Environment variable naming the dictionary directory.
pub const DICT_PATH_ENV: &str = "BITE_DICT_PATH";
/// Older name for [`DICT_PATH_ENV`], still read.
pub const LEGACY_DICT_PATH_ENV: &str = "FAAD_DICT_PATH";

/// The dictionary directory named by the environment, if any.
pub fn dict_path_from_env() -> Option<std::path::PathBuf> {
    [DICT_PATH_ENV, LEGACY_DICT_PATH_ENV]
        .iter()
        .find_map(std::env::var_os)
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
}
