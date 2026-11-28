//! `mind goat watch --save`: a sample's payload lands on disk unchanged.

use mind::save::{extension, file_name, save};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mind-save-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

const PCAP: &str = "application/vnd.tcpdump.pcap";

/// A real capture file, published as one sample, is saved byte for byte:
/// the SHA-256 of the file written equals the SHA-256 of the bytes received.
#[test]
fn a_saved_capture_has_the_hash_of_the_bytes_received() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/ntp.pcap");
    let received = fs::read(&fixture).unwrap();
    let dir = scratch("hash");
    let path = save(&dir, "slot/sensor/pcap/node-1", PCAP, &received, 1, 1).unwrap();
    let written = fs::read(&path).unwrap();
    assert_eq!(sha256(&written), sha256(&received));
    assert_eq!(written.len(), received.len());
    assert_eq!(path.extension().unwrap(), "pcap");
    fs::remove_dir_all(&dir).unwrap();
}

/// Every byte value survives, including ones that are not text.
#[test]
fn binary_payloads_are_not_altered() {
    let received: Vec<u8> = (0..=255u8).cycle().take(70_000).collect();
    let dir = scratch("binary");
    let path = save(&dir, "a/b", "zenoh/bytes", &received, 2, 1).unwrap();
    assert_eq!(sha256(&fs::read(&path).unwrap()), sha256(&received));
    fs::remove_dir_all(&dir).unwrap();
}

/// Successive samples on the same key become separate files; nothing is
/// overwritten and no temporary file is left behind.
#[test]
fn each_sample_is_its_own_file_and_none_is_overwritten() {
    let dir = scratch("many");
    let a = save(&dir, "k/pcap/n", PCAP, b"first", 3, 1).unwrap();
    let b = save(&dir, "k/pcap/n", PCAP, b"second", 3, 2).unwrap();
    assert_ne!(a, b);
    assert!(save(&dir, "k/pcap/n", PCAP, b"again", 3, 1).is_err());
    assert_eq!(fs::read(&a).unwrap(), b"first");
    let names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.iter().all(|n| !n.ends_with(".part")), "{names:?}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn names_are_safe_and_carry_the_key() {
    assert_eq!(
        file_name("slot/q/pcap/node-1", PCAP, 1700000000000, 7),
        "slot_q_pcap_node-1-1700000000000-000007.pcap"
    );
    assert_eq!(
        file_name("../x/y z", "text/plain", 1, 1),
        "_x_y-z-1-000001.bin"
    );
    assert_eq!(
        extension("zenoh/bytes;application/vnd.tcpdump.pcap"),
        "pcap"
    );
}
