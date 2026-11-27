//! Saving samples read from a goat fabric to files, so a capture published by
//! a tap (`mind --tap-only --pcap-publish …`) can be opened in Wireshark,
//! replayed with `mind --pcap-file` or surveyed with `mind discover`.
//!
//! Each sample becomes one file. Its bytes are written exactly as received:
//! a pcap slice saved here is byte-for-byte the file the tap closed.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The content type a tap's published slice carries; see [`crate::slices`].
const PCAP_MARKER: &str = "vnd.tcpdump.pcap";

/// The file extension for a sample of this encoding: `pcap` for a capture
/// slice, `bin` for anything else.
pub fn extension(encoding: &str) -> &'static str {
    if encoding.contains(PCAP_MARKER) {
        "pcap"
    } else {
        "bin"
    }
}

/// A file name for the `index`th sample received on `key`, made only of
/// characters that are safe in a file name on every platform: the key's
/// chunks joined with `_`, then the receive time and the index.
pub fn file_name(key: &str, encoding: &str, received_unix_ms: u128, index: usize) -> String {
    let stem: String = key
        .chars()
        .map(|c| match c {
            '/' => '_',
            c if c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_') => c,
            _ => '-',
        })
        .collect();
    let stem = stem.trim_start_matches('.');
    format!(
        "{stem}-{received_unix_ms}-{index:06}.{}",
        extension(encoding)
    )
}

/// Write `payload` to `dir` under [`file_name`]. The bytes go to a temporary
/// name first and are renamed once complete, so a reader of the directory
/// never sees a half-written file; an existing file is never overwritten.
/// Returns the path written.
pub fn save(
    dir: &Path,
    key: &str,
    encoding: &str,
    payload: &[u8],
    received_unix_ms: u128,
    index: usize,
) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let name = file_name(key, encoding, received_unix_ms, index);
    let path = dir.join(&name);
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", path.display()),
        ));
    }
    let partial = dir.join(format!(".{name}.part"));
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)?;
        file.write_all(payload)?;
        file.sync_all()?;
    }
    fs::rename(&partial, &path)?;
    Ok(path)
}
