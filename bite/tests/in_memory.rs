//! A dictionary built from files in memory is the dictionary loaded from a
//! directory with the same files.

use bite::vectors::run_vectors;
use bite::{Dictionary, Interface, LoadError};
use std::fs;
use std::path::{Path, PathBuf};

/// Every file under `root` as a `/`-separated relative path and its text.
fn files_under(root: &Path) -> Vec<(String, String)> {
    fn walk(dir: &Path, prefix: &str, files: &mut Vec<(String, String)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let relative = format!("{prefix}{name}");
            if path.is_dir() {
                walk(&path, &format!("{relative}/"), files);
            } else {
                files.push((relative, fs::read_to_string(&path).unwrap()));
            }
        }
    }
    let mut files = Vec::new();
    walk(root, "", &mut files);
    files
}

/// Everything about an interface that decoding depends on, in a form that
/// compares: `Interface` holds hash maps and has no equality of its own.
fn structure(interface: &Interface) -> String {
    let mut codes: Vec<_> = interface.discriminator.names.iter().collect();
    codes.sort();
    let mut text = format!(
        "{} port={:?} {:?} {:?} description={:?} icd={:?} protocol={:?} length={:?}\n\
         discriminator {:?} [{}, {}) mask={:?} {codes:?}\n",
        interface.name,
        interface.port,
        interface.bit_order,
        interface.byte_order,
        interface.description,
        interface.icd,
        interface.protocol,
        interface.message_length,
        interface.discriminator.source,
        interface.discriminator.start,
        interface.discriminator.stop,
        interface.discriminator.mask,
    );
    for message in interface.messages() {
        text.push_str(&format!("  {} {:?}\n", message.name, message.description));
        for field in &message.fields {
            let mut labels: Vec<&str> = field.labels().collect();
            labels.sort_unstable();
            text.push_str(&format!(
                "    {} [{}, {}) {:?} {:?} {:?} scale={:?} offset={:?} unit={:?} {:?} {labels:?}\n",
                field.name,
                field.start,
                field.stop,
                field.kind,
                field.bit_order,
                field.byte_order,
                field.scale,
                field.offset,
                field.unit,
                field.description,
            ));
        }
    }
    text
}

fn structures(dictionary: &Dictionary) -> Vec<String> {
    dictionary.interfaces().iter().map(structure).collect()
}

/// Issues as text, with the directory the files came from taken out of the
/// file paths they name.
fn issues(dictionary: &Dictionary) -> Vec<String> {
    let prefix = format!("{}/", dictionary.root.display());
    dictionary
        .issues()
        .iter()
        .map(|issue| match dictionary.root.as_os_str().is_empty() {
            true => issue.to_string(),
            false => issue.to_string().replace(&prefix, ""),
        })
        .collect()
}

fn assert_same(directory: &Path) -> (Dictionary, Dictionary) {
    let loaded = Dictionary::load(directory).expect("loads from the directory");
    let built = Dictionary::from_files(files_under(directory)).expect("builds from files");
    assert_eq!(structures(&loaded), structures(&built));
    assert_eq!(issues(&loaded), issues(&built));
    assert_eq!(
        format!("{:?}", loaded.spec),
        format!("{:?}", built.spec),
        "dictionary.json"
    );
    assert_eq!(
        bite::jsonschema::dictionary_schemas(&loaded),
        bite::jsonschema::dictionary_schemas(&built)
    );
    for interface in loaded.interfaces() {
        let port = interface.port;
        assert_eq!(
            port.and_then(|port| loaded.interface_for_port(port))
                .map(|i| &i.name),
            port.and_then(|port| built.interface_for_port(port))
                .map(|i| &i.name),
        );
    }
    let from_directory = run_vectors(&loaded).unwrap();
    let from_memory = run_vectors(&built).unwrap();
    assert_eq!(
        format!("{from_directory:?}"),
        format!("{from_memory:?}"),
        "vector results"
    );
    (loaded, built)
}

#[test]
fn every_shipped_dictionary_is_the_same_both_ways() {
    let shipped = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dictionaries");
    let mut checked = 0;
    for entry in fs::read_dir(shipped).unwrap() {
        let directory = entry.unwrap().path();
        if !directory.is_dir() {
            continue;
        }
        let (loaded, built) = assert_same(&directory);
        assert!(!loaded.has_errors(), "{:#?}", loaded.issues());
        assert!(built.root.as_os_str().is_empty());
        for dictionary in [&loaded, &built] {
            let results = run_vectors(dictionary).unwrap();
            assert!(!results.is_empty(), "{}: no vectors", directory.display());
            for result in &results {
                assert!(result.passed(), "{}: {:#?}", result.name, result.failures);
            }
        }
        checked += 1;
    }
    assert!(checked >= 3);
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("bite-memory-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const METADATA: &str = r#"{
    "port": 9000,
    "message_type": {"start": 0, "stop": 8, "values": {"1": "Ping", "2": "Missing", "x": "Bad"}}
}"#;

#[test]
fn a_dictionary_with_mistakes_reports_the_same_issues_both_ways() {
    let dir = TempDir::new("mistakes");
    dir.write("dictionary.json", "{ not json");
    dir.write("A/metadata.json", METADATA);
    dir.write(
        "A/messages/Ping.json",
        r#"{"fields": {
            "good": {"start": 0, "stop": 8},
            "inverted": {"start": 16, "stop": 8},
            "overlapping": {"start": 4, "stop": 12, "values": {"1": [1], "q": "label"}}
        }}"#,
    );
    // Sorts before `Ping.json` by file name, after it by message name.
    dir.write("A/messages/Ping Reply.json", r#"{"fields": {}}"#);
    dir.write("A/messages/Broken.json", "[");
    dir.write("A/messages/notes.txt", "not a message");
    dir.write("B/metadata.json", METADATA);
    dir.write("C/metadata.json", r#"{"port": 1}"#);
    dir.write("D/readme.txt", "not an interface");
    dir.write(
        "vectors/ping.json",
        r#"{"interface": "A", "payload_hex": "0102", "expect": {"message_type": "Ping", "fields": {"good": "1"}}}"#,
    );
    dir.write("vectors/broken.json", "{");
    dir.write("vectors/notes.txt", "not a vector");

    let (loaded, built) = assert_same(&dir.0);
    assert!(loaded.has_errors());
    // Enough went wrong for the comparison to mean something.
    assert!(loaded.issues().len() >= 8, "{:#?}", loaded.issues());
    let names: Vec<&str> = built.interfaces().iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["A", "B"]);
    let results = run_vectors(&built).unwrap();
    let outcome: Vec<(&str, bool)> = results
        .iter()
        .map(|result| (result.name.as_str(), result.passed()))
        .collect();
    assert_eq!(outcome, [("broken", false), ("ping", true)]);
}

#[test]
fn paths_outside_the_layout_are_ignored() {
    let dictionary = Dictionary::from_files([
        ("./A/metadata.json", METADATA),
        ("/A/messages/Ping.json", r#"{"fields": {}}"#),
        ("A/messages/nested/Deep.json", r#"{"fields": {}}"#),
        ("A/other/Ping.json", "{"),
        ("README.md", "text"),
    ])
    .unwrap();
    let interface = dictionary.interface("A").unwrap();
    assert_eq!(interface.messages().len(), 1);
    assert!(interface.message("Ping").is_some());
    assert!(run_vectors(&dictionary).unwrap().is_empty());
}

#[test]
fn no_files_is_an_empty_dictionary() {
    let error = Dictionary::from_files(Vec::<(String, String)>::new()).unwrap_err();
    assert!(matches!(error, LoadError::Empty { .. }));
    assert_eq!(
        error.to_string(),
        "no interface found under the files given: expected sub-directories containing metadata.json"
    );
}
