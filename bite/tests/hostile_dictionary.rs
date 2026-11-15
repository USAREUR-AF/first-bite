//! Loading never panics and never costs more than the limits allow, whatever
//! the dictionary files hold: a dictionary is whatever someone handed over.

use bite::limits::{
    MAX_FIELDS_PER_MESSAGE, MAX_FILES, MAX_FILE_BYTES, MAX_PAYLOAD_BITS, MAX_VALUES,
};
use bite::{Dictionary, LoadError, Severity};
use std::fs;
use std::path::PathBuf;

/// xorshift64*: small, deterministic, good enough to vary the damage.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, limit: usize) -> usize {
        (self.next() % limit as u64) as usize
    }
}

const METADATA: &str =
    r#"{"port": 4000, "message_type": {"start": 0, "stop": 8, "values": {"1": "Ping"}}}"#;

fn with_message(message: &str) -> Dictionary {
    Dictionary::from_files([
        ("LINK/metadata.json", METADATA),
        ("LINK/messages/Ping.json", message),
    ])
    .unwrap()
}

fn errors(dictionary: &Dictionary) -> Vec<String> {
    dictionary
        .issues()
        .iter()
        .filter(|issue| issue.severity == Severity::Error)
        .map(ToString::to_string)
        .collect()
}

/// Load, then decode a few payloads against whatever survived.
fn exercise(dictionary: &Dictionary) {
    let payloads: [&[u8]; 4] = [&[], &[1], &[1; 16], &[0xFF; 512]];
    for interface in dictionary.interfaces() {
        for payload in payloads {
            for frame_id in [None, Some(0), Some(1), Some(u64::MAX)] {
                if let Ok(decoded) = interface.decode_frame(payload, frame_id) {
                    let _ = decoded.rendered();
                    serde_json::to_string(&decoded).unwrap();
                }
            }
        }
    }
    let _ = bite::vectors::run_vectors(dictionary);
    let _ = bite::jsonschema::dictionary_schemas(dictionary);
}

#[test]
fn hostile_documents_are_reported_not_fatal() {
    let nested = format!("{}1{}", "[".repeat(100_000), "]".repeat(100_000));
    let nested_object = format!(
        r#"{{"fields": {{"a": {{"start": 0, "stop": 8, "values": {{"1": {}{}}}}}}}}}"#,
        r#"{"x":"#.repeat(50_000),
        "1"
    );
    let documents: Vec<String> = vec![
        String::new(),
        "\u{feff}{}".to_string(),
        "null".to_string(),
        "[]".to_string(),
        "\"fields\"".to_string(),
        "{".to_string(),
        "{\"fields\": null}".to_string(),
        "{\"fields\": []}".to_string(),
        "{\"fields\": {\"a\": null}}".to_string(),
        "{\"fields\": {\"a\": {}}}".to_string(),
        "{\"fields\": {\"a\": {\"start\": -1, \"stop\": 8}}}".to_string(),
        "{\"fields\": {\"a\": {\"start\": 0.5, \"stop\": 8}}}".to_string(),
        "{\"fields\": {\"a\": {\"start\": \"0\", \"stop\": \"8\"}}}".to_string(),
        "{\"fields\": {\"a\": {\"start\": 0, \"stop\": 1e400}}}".to_string(),
        "{\"fields\": {\"a\": {\"start\": 0, \"stop\": 18446744073709551615}}}".to_string(),
        "{\"fields\": {\"a\": {\"start\": 0, \"stop\": 18446744073709551616}}}".to_string(),
        "{\"fields\": {\"a\": {\"start\": 18446744073709551614, \"stop\": 18446744073709551615}}}"
            .to_string(),
        "{\"fields\": {\"a\": {\"start\": 8, \"stop\": 8}}}".to_string(),
        "{\"fields\": {\"a\": {\"start\": 9, \"stop\": 8}}}".to_string(),
        "{\"fields\": {\"a\": {\"start\": 0, \"stop\": 8, \"type\": \"quaternion\"}}}".to_string(),
        "{\"fields\": {\"a\": {\"start\": 0, \"stop\": 8, \"scale\": 1e999}}}".to_string(),
        "{\"fields\": {\"a\": {\"start\": 0, \"stop\": 8, \"values\": []}}}".to_string(),
        "{\"fields\": {\"a\": {\"start\": 0, \"stop\": 8, \"values\": {\"--170141183460469231731687303715884105728\": \"x\"}}}}".to_string(),
        "{\"fields\": {\"a\": {\"start\": 0, \"stop\": 8, \"values\": {\"-0x-80000000000000000000000000000000\": \"x\"}}}}".to_string(),
        "{\"fields\": {\"a\": {\"start\": 0, \"stop\": 8, \"values\": {\"1\": {\"deep\": [[[[1]]]]}}}}}".to_string(),
        "{\"fields\": {\"\": {\"start\": 0, \"stop\": 8}, \"a\\u0000b\": {\"start\": 0, \"stop\": 8}, \"../../x\": {\"start\": 0, \"stop\": 8}}}".to_string(),
        "{\"fields\": {\"a\": {\"start\": 0, \"stop\": 8}, \"a\": {\"start\": 8, \"stop\": 16}}}".to_string(),
        nested,
        nested_object,
        "\0\0\0\0".to_string(),
    ];
    for document in &documents {
        // As a message, as an interface's metadata, as the dictionary file and
        // as a vector.
        let as_message = with_message(document);
        exercise(&as_message);
        for files in [
            vec![("LINK/metadata.json", document.as_str())],
            vec![
                ("dictionary.json", document.as_str()),
                ("LINK/metadata.json", METADATA),
            ],
            vec![
                ("LINK/metadata.json", METADATA),
                ("vectors/v.json", document.as_str()),
            ],
        ] {
            if let Ok(dictionary) = Dictionary::from_files(files) {
                exercise(&dictionary);
            }
        }
    }
}

#[test]
fn a_doubled_sign_in_a_value_key_is_a_warning() {
    let dictionary = with_message(
        r#"{"fields": {"a": {"start": 8, "stop": 16, "values": {"--170141183460469231731687303715884105728": "low", "2": "two"}}}}"#,
    );
    let issues: Vec<String> = dictionary
        .issues()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(issues[0].contains("is not an integer"), "{issues:?}");
    let decoded = dictionary
        .interface("LINK")
        .unwrap()
        .decode(&[1, 2])
        .unwrap();
    assert_eq!(decoded.rendered()["a"].as_deref(), Some("two"));
}

#[test]
fn a_message_with_too_many_fields_is_left_out() {
    let fields = |count: usize| {
        let body: Vec<String> = (0..count)
            .map(|index| format!(r#""f{index}": {{"start": 8, "stop": 16}}"#))
            .collect();
        format!(r#"{{"fields": {{{}}}}}"#, body.join(","))
    };
    let at_the_limit = with_message(&fields(MAX_FIELDS_PER_MESSAGE));
    assert_eq!(errors(&at_the_limit), Vec::<String>::new());
    let message = at_the_limit.interface("LINK").unwrap().message("Ping");
    assert_eq!(message.unwrap().fields.len(), MAX_FIELDS_PER_MESSAGE);

    let over = with_message(&fields(MAX_FIELDS_PER_MESSAGE + 1));
    assert_eq!(
        errors(&over),
        [format!(
            "error: LINK/Ping: {} fields; a message may have at most {MAX_FIELDS_PER_MESSAGE}",
            MAX_FIELDS_PER_MESSAGE + 1
        )]
    );
    assert!(over.interface("LINK").unwrap().message("Ping").is_none());
}

#[test]
fn a_values_object_over_the_limit_is_refused() {
    let values = |count: usize| {
        let body: Vec<String> = (0..count).map(|key| format!(r#""{key}": "n""#)).collect();
        body.join(",")
    };
    let field = with_message(&format!(
        r#"{{"fields": {{"a": {{"start": 8, "stop": 32, "values": {{{}}}}}, "b": {{"start": 32, "stop": 40}}}}}}"#,
        values(MAX_VALUES + 1)
    ));
    assert_eq!(
        errors(&field),
        [format!(
            "error: LINK/Ping/a: {} values; a field may have at most {MAX_VALUES}",
            MAX_VALUES + 1
        )]
    );
    let message = field.interface("LINK").unwrap().message("Ping").unwrap();
    assert!(message.field("a").is_none());
    assert!(message.field("b").is_some());

    let interface = Dictionary::from_files([(
        "LINK/metadata.json",
        format!(
            r#"{{"message_type": {{"start": 0, "stop": 32, "values": {{{}}}}}}}"#,
            values(MAX_VALUES + 1)
        ),
    )])
    .unwrap();
    let found = errors(&interface);
    assert!(
        found.iter().any(|error| error.contains("message_type has")),
        "{found:?}"
    );
    assert!(interface
        .interface("LINK")
        .unwrap()
        .discriminator
        .names
        .is_empty());
}

#[test]
fn a_field_past_the_payload_limit_is_left_out() {
    let dictionary = with_message(&format!(
        r#"{{"fields": {{
            "last": {{"start": {}, "stop": {MAX_PAYLOAD_BITS}}},
            "beyond": {{"start": {MAX_PAYLOAD_BITS}, "stop": {}}},
            "vast": {{"start": 0, "stop": 1099511627776, "type": "bytes"}}
        }}}}"#,
        MAX_PAYLOAD_BITS - 8,
        MAX_PAYLOAD_BITS + 8
    ));
    let found = errors(&dictionary);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found
        .iter()
        .all(|error| error.contains("reach past the 65535 bytes")));
    let message = dictionary
        .interface("LINK")
        .unwrap()
        .message("Ping")
        .unwrap();
    assert_eq!(message.bit_length(), MAX_PAYLOAD_BITS);
}

#[test]
fn a_file_over_the_size_limit_is_not_parsed() {
    // Valid JSON, so only its size can be the reason it is refused.
    let huge = format!(
        r#"{{"description": "{}", "fields": {{"a": {{"start": 8, "stop": 16}}}}}}"#,
        "x".repeat(MAX_FILE_BYTES)
    );
    let in_memory = with_message(&huge);
    let found = errors(&in_memory);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("the largest dictionary file read is 4194304"),
        "{found:?}"
    );
    assert!(in_memory
        .interface("LINK")
        .unwrap()
        .message("Ping")
        .is_none());

    let root = temp_dir("size");
    fs::create_dir_all(root.join("LINK/messages")).unwrap();
    fs::write(root.join("LINK/metadata.json"), METADATA).unwrap();
    fs::write(root.join("LINK/messages/Ping.json"), &huge).unwrap();
    let on_disk = Dictionary::load(&root).unwrap();
    let found = errors(&on_disk);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("the largest dictionary file read is 4194304"),
        "{found:?}"
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_dictionary_with_too_many_files_does_not_load() {
    let files = (0..=MAX_FILES).map(|index| (format!("vectors/v{index}.json"), "{}"));
    let result = Dictionary::from_files(
        std::iter::once(("LINK/metadata.json".to_string(), METADATA)).chain(files),
    );
    assert!(
        matches!(result, Err(LoadError::TooManyFiles { limit, .. }) if limit == MAX_FILES),
        "{:?}",
        result.err().map(|e| e.to_string())
    );

    // An iterator with no end is stopped at the limit.
    let endless = (0u64..).map(|index| (format!("LINK/messages/m{index}.json"), "{}"));
    assert!(matches!(
        Dictionary::from_files(endless),
        Err(LoadError::TooManyFiles { .. })
    ));
}

#[test]
fn files_on_disk_that_are_not_text_are_reported() {
    let root = temp_dir("bytes");
    fs::create_dir_all(root.join("LINK/messages")).unwrap();
    fs::create_dir_all(root.join("vectors/nested.json")).unwrap();
    fs::write(root.join("LINK/metadata.json"), METADATA).unwrap();
    fs::write(
        root.join("LINK/messages/Ping.json"),
        [0xFF, 0xFE, 0x00, 0x80],
    )
    .unwrap();
    fs::write(root.join("vectors/v.json"), [0xC3, 0x28]).unwrap();
    let dictionary = Dictionary::load(&root).unwrap();
    assert_eq!(errors(&dictionary).len(), 1);
    exercise(&dictionary);
    fs::remove_dir_all(&root).unwrap();
}

/// Damage the files of every shipped dictionary and load the result.
#[test]
fn damaged_copies_of_the_shipped_dictionaries_never_panic() {
    let shipped = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dictionaries");
    let mut originals: Vec<Vec<(String, Vec<u8>)>> = Vec::new();
    for entry in fs::read_dir(shipped).unwrap() {
        let root = entry.unwrap().path();
        if !root.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        collect(&root, "", &mut files);
        originals.push(files);
    }
    assert!(originals.len() >= 3);

    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut loaded = 0usize;
    for round in 0..3_000 {
        let original = &originals[round % originals.len()];
        let mut files: Vec<(String, String)> = Vec::new();
        for (path, bytes) in original {
            let mut bytes = bytes.clone();
            // Most files are left whole so that the damage in one is reached.
            if rng.below(3) == 0 && !bytes.is_empty() {
                match rng.below(5) {
                    0 => bytes.truncate(rng.below(bytes.len())),
                    1 => {
                        let at = rng.below(bytes.len());
                        bytes[at] = rng.next() as u8;
                    }
                    2 => {
                        // A digit becomes a long run of digits.
                        if let Some(at) = bytes.iter().position(u8::is_ascii_digit) {
                            let run = vec![b'9'; 1 + rng.below(40)];
                            bytes.splice(at..=at, run);
                        }
                    }
                    3 => {
                        let at = rng.below(bytes.len());
                        let insert =
                            [b"{", b"[", b"\"", b"-", b"}", b"]", b":", b","][rng.below(8)];
                        bytes.splice(at..at, insert.iter().copied());
                    }
                    _ => {
                        let from = rng.below(bytes.len());
                        let to = from + rng.below(bytes.len() - from);
                        let copy = bytes[from..to].to_vec();
                        let at = rng.below(bytes.len());
                        bytes.splice(at..at, copy);
                    }
                }
            }
            files.push((path.clone(), String::from_utf8_lossy(&bytes).into_owned()));
        }
        if let Ok(dictionary) = Dictionary::from_files(files) {
            loaded += 1;
            exercise(&dictionary);
        }
    }
    assert!(loaded > 2_000, "only {loaded} damaged dictionaries loaded");
}

fn collect(dir: &std::path::Path, prefix: &str, files: &mut Vec<(String, Vec<u8>)>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        let name = format!("{prefix}{}", path.file_name().unwrap().to_string_lossy());
        if path.is_dir() {
            collect(&path, &format!("{name}/"), files);
        } else {
            files.push((name, fs::read(&path).unwrap()));
        }
    }
}

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("bite-hostile-{}-{name}", std::process::id()))
}
