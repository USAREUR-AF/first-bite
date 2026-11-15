//! What the loader reports for a dictionary with mistakes in it.

use bite::{Dictionary, LoadError, Severity};
use std::fs;
use std::path::{Path, PathBuf};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("bite-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const METADATA: &str = r#"{
    "port": 9000,
    "message_type": {"start": 0, "stop": 8, "values": {"1": "Ping", "2": "Missing"}}
}"#;

fn errors(dictionary: &Dictionary) -> Vec<String> {
    dictionary
        .issues()
        .iter()
        .filter(|issue| issue.severity == Severity::Error)
        .map(ToString::to_string)
        .collect()
}

#[test]
fn missing_directory() {
    let result = Dictionary::load("/nonexistent/bite-dictionary");
    assert!(matches!(result, Err(LoadError::NotADirectory { .. })));
}

#[test]
fn directory_without_interfaces() {
    let dir = TempDir::new("empty");
    assert!(matches!(
        Dictionary::load(dir.path()),
        Err(LoadError::Empty { .. })
    ));
}

#[test]
fn bad_fields_are_dropped_and_reported() {
    let dir = TempDir::new("bad-fields");
    dir.write("A/metadata.json", METADATA);
    dir.write(
        "A/messages/Ping.json",
        r#"{"fields": {
            "good": {"start": 0, "stop": 8},
            "inverted": {"start": 16, "stop": 8},
            "too_wide": {"start": 3, "stop": 70},
            "odd_float": {"start": 8, "stop": 24, "type": "float"},
            "unaligned_text": {"start": 4, "stop": 20, "type": "ascii"},
            "overlapping": {"start": 4, "stop": 12}
        }}"#,
    );
    let dictionary = Dictionary::load(dir.path()).unwrap();
    let message = dictionary.interface("A").unwrap().message("Ping").unwrap();
    let names: Vec<&str> = message.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["good", "overlapping"]);

    let errors = errors(&dictionary);
    assert_eq!(errors.len(), 4, "{errors:#?}");
    for field in ["inverted", "too_wide", "odd_float", "unaligned_text"] {
        assert!(
            errors
                .iter()
                .any(|e| e.contains(&format!("A/Ping/{field}"))),
            "{field} not reported in {errors:#?}"
        );
    }
    let warnings: Vec<String> = dictionary
        .issues()
        .iter()
        .filter(|issue| issue.severity == Severity::Warning)
        .map(ToString::to_string)
        .collect();
    assert!(
        warnings.iter().any(|w| w.contains("overlap")),
        "{warnings:#?}"
    );
    assert!(
        warnings.iter().any(|w| w.contains("A/Missing")),
        "{warnings:#?}"
    );
}

#[test]
fn unparseable_message_file_is_an_error_not_a_skip() {
    let dir = TempDir::new("bad-json");
    dir.write("A/metadata.json", METADATA);
    dir.write("A/messages/Ping.json", "{ not json");
    let dictionary = Dictionary::load(dir.path()).unwrap();
    assert!(dictionary.has_errors());
    assert!(dictionary.interface("A").unwrap().message("Ping").is_none());
}

#[test]
fn two_interfaces_on_one_port() {
    let dir = TempDir::new("port-clash");
    dir.write("A/metadata.json", METADATA);
    dir.write("B/metadata.json", METADATA);
    let dictionary = Dictionary::load(dir.path()).unwrap();
    assert_eq!(dictionary.interface_for_port(9000).unwrap().name, "A");
    assert!(errors(&dictionary).iter().any(|e| e.contains("port 9000")));
}

#[test]
fn wide_aligned_field_defaults_to_bytes() {
    let dir = TempDir::new("wide");
    dir.write("A/metadata.json", METADATA);
    dir.write(
        "A/messages/Ping.json",
        r#"{"fields": {"blob": {"start": 8, "stop": 104}}}"#,
    );
    let dictionary = Dictionary::load(dir.path()).unwrap();
    assert!(!dictionary.has_errors());
    let interface = dictionary.interface("A").unwrap();
    let mut payload = vec![1u8];
    payload.extend_from_slice(&[0xAB; 12]);
    let decoded = interface.decode(&payload).unwrap();
    assert_eq!(
        decoded.field("blob").unwrap().render().unwrap(),
        "ab".repeat(12)
    );
}
