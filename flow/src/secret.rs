//! Credential values, and the references a configuration file holds in their
//! place.
//!
//! A configuration file is meant to hold no credential value. A key is
//! written as a reference, `${NAME}`, and the value is taken from the
//! environment variable `NAME` when the file is loaded; or the key's `_file`
//! variant names a file that holds the value. Either way the value exists
//! only in this process, as a [`Secret`], which no format string can print.

use serde::Deserialize;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

/// What `Debug` shows in place of a credential.
pub const REDACTED: &str = "***";

/// Largest credential file read, in bytes.
pub const MAX_CREDENTIAL_FILE_BYTES: u64 = 64 * 1024;

/// Environment variable systemd sets to the directory holding the unit's
/// `LoadCredential=` files. A relative `_file` path is looked up there.
pub const CREDENTIALS_DIRECTORY_ENV: &str = "CREDENTIALS_DIRECTORY";

/// A credential value. `Debug` prints `***`; there is no `Display`. The value
/// is reached only through [`Secret::expose`], at the point it is used.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Secret(value.into())
    }

    /// The credential itself. Never pass the result to a log line or an
    /// error message.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

/// How a configuration value names an environment variable.
#[derive(Debug, PartialEq, Eq)]
pub enum Reference<'a> {
    /// The value is `${NAME}`.
    Variable(&'a str),
    /// The value contains `${` but is not one whole `${NAME}`.
    Malformed,
    /// The value is an ordinary value.
    None,
}

/// Read `value` as a reference. The whole value must be the reference:
/// `${NAME}` with a name of letters, digits and `_` that does not start with
/// a digit. Nothing is substituted inside a longer value.
pub fn reference(value: &str) -> Reference<'_> {
    if !value.contains("${") {
        return Reference::None;
    }
    let name = value
        .strip_prefix("${")
        .and_then(|rest| rest.strip_suffix('}'));
    match name {
        Some(name)
            if !name.is_empty()
                && !name.starts_with(|c: char| c.is_ascii_digit())
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') =>
        {
            Reference::Variable(name)
        }
        _ => Reference::Malformed,
    }
}

/// Where a `_file` path is read from: an absolute path as it is, a relative
/// one under `credentials_directory` when there is one.
pub fn credential_path(path: &str, credentials_directory: Option<&str>) -> PathBuf {
    let path = Path::new(path);
    match credentials_directory {
        Some(directory) if path.is_relative() => Path::new(directory).join(path),
        _ => path.to_path_buf(),
    }
}

/// The credential a file holds: its text without the line break at the end.
/// The reason for a failure never includes what was read.
pub fn read_credential_file(path: &Path) -> Result<Secret, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut text = String::new();
    file.take(MAX_CREDENTIAL_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::InvalidData => "the file is not UTF-8 text".to_string(),
            _ => e.to_string(),
        })?;
    if text.len() as u64 > MAX_CREDENTIAL_FILE_BYTES {
        return Err(format!(
            "the file is larger than {MAX_CREDENTIAL_FILE_BYTES} bytes"
        ));
    }
    let value = text.trim_end_matches(['\n', '\r']);
    if value.is_empty() {
        return Err("the file is empty".to_string());
    }
    Ok(Secret::new(value))
}

/// `text` with every occurrence of each credential replaced by `***`, for
/// error text that quotes what a server answered.
pub fn scrub(text: &str, credentials: &[&str]) -> String {
    let mut scrubbed = text.to_string();
    for credential in credentials {
        if !credential.is_empty() {
            scrubbed = scrubbed.replace(credential, REDACTED);
        }
    }
    scrubbed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_is_never_printed() {
        let secret = Secret::new("hunter2-value");
        assert_eq!(format!("{secret:?}"), "***");
        assert_eq!(format!("{:#?}", Some(&secret)), "Some(\n    ***,\n)");
        assert_eq!(secret.expose(), "hunter2-value");
    }

    #[test]
    fn only_a_whole_value_is_a_reference() {
        assert_eq!(reference("${BLOB_KEY}"), Reference::Variable("BLOB_KEY"));
        assert_eq!(reference("${_k9}"), Reference::Variable("_k9"));
        assert_eq!(reference("plain value"), Reference::None);
        assert_eq!(reference("$BLOB_KEY"), Reference::None);
        assert_eq!(reference("{system}/{message}"), Reference::None);
        for malformed in [
            "${}",
            "${9LIVES}",
            "${A-B}",
            "${A}${B}",
            "prefix-${A}",
            "${A}/suffix",
            " ${A}",
            "${A",
        ] {
            assert_eq!(reference(malformed), Reference::Malformed, "{malformed}");
        }
    }

    #[test]
    fn relative_credential_paths_are_under_the_credentials_directory() {
        let directory = Some("/run/credentials/first-bite.service");
        assert_eq!(
            credential_path("blob-key", directory),
            Path::new("/run/credentials/first-bite.service/blob-key")
        );
        assert_eq!(
            credential_path("/etc/first-bite/blob-key", directory),
            Path::new("/etc/first-bite/blob-key")
        );
        assert_eq!(credential_path("blob-key", None), Path::new("blob-key"));
    }

    #[test]
    fn a_credential_file_gives_its_text_without_the_final_line_break() {
        let path = crate::test_support::temp_path("credential");
        std::fs::write(&path, "s3cr3t value\r\n").unwrap();
        assert_eq!(
            read_credential_file(&path).unwrap().expose(),
            "s3cr3t value"
        );

        std::fs::write(&path, "\n").unwrap();
        assert_eq!(
            read_credential_file(&path).unwrap_err(),
            "the file is empty"
        );

        std::fs::write(&path, vec![b'a'; MAX_CREDENTIAL_FILE_BYTES as usize + 1]).unwrap();
        assert!(read_credential_file(&path)
            .unwrap_err()
            .starts_with("the file is larger than"));

        std::fs::write(&path, [0xFF, 0xFE, b's', b'e', b'c']).unwrap();
        assert_eq!(
            read_credential_file(&path).unwrap_err(),
            "the file is not UTF-8 text"
        );
        let _ = std::fs::remove_file(&path);
        assert!(read_credential_file(&path).is_err());
    }

    #[test]
    fn scrubbing_replaces_every_occurrence() {
        assert_eq!(
            scrub(
                "id AKIDEXAMPLE and again AKIDEXAMPLE, key s3cr3t",
                &["AKIDEXAMPLE", "s3cr3t", ""]
            ),
            "id *** and again ***, key ***"
        );
    }
}
