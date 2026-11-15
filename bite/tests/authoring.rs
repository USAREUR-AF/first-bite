//! The authoring tools against the shipped dictionaries and the worked
//! example in `docs/icd-to-dictionary/06-worked-example/`.

use bite::authoring::canon::{canonical, format};
use bite::authoring::coverage::coverage;
use bite::authoring::diff::{diff, Bump, Impact};
use bite::authoring::encode::encode;
use bite::authoring::lint::lint;
use bite::authoring::model::{read_dir, FileSet, Tree};
use bite::authoring::rules::{self, LintConfig, RULES, UNITS};
use bite::authoring::stats::{stats, Payload};
use bite::authoring::table::{export, import, ImportOptions, Tables};
use bite::schema::VectorSpec;
use bite::vectors::{check_vector, load_vectors};
use bite::Dictionary;
use std::fs;
use std::path::PathBuf;

const SHIPPED: [&str; 4] = ["glasshouse", "toot", "vehicle-can", "weather-station"];

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn shipped(name: &str) -> PathBuf {
    repo().join("dictionaries").join(name)
}

fn worked_example() -> PathBuf {
    repo().join("docs/icd-to-dictionary/06-worked-example")
}

fn files(name: &str) -> FileSet {
    read_dir(&shipped(name)).unwrap()
}

fn tables_in(dir: PathBuf) -> Tables {
    let read = |name: &str| fs::read_to_string(dir.join(name)).unwrap();
    Tables {
        interfaces: read("interfaces.csv"),
        messages: read("messages.csv"),
        fields: read("fields.csv"),
        values: read("values.csv"),
    }
}

/// The interface files of a dictionary: everything `import-csv` writes.
fn interface_files(files: &FileSet) -> FileSet {
    files
        .iter()
        .filter(|(path, _)| *path != "dictionary.json" && !path.starts_with("vectors/"))
        .map(|(path, text)| (path.clone(), text.clone()))
        .collect()
}

fn imported(tables: &Tables) -> FileSet {
    let interfaces = import(tables, ImportOptions::default()).unwrap_or_else(|e| panic!("{e:#?}"));
    let tree = Tree {
        dictionary: None,
        interfaces,
        vectors: Vec::new(),
    };
    canonical(&tree).unwrap().to_files()
}

#[test]
fn every_shipped_dictionary_is_in_canonical_form() {
    for name in SHIPPED {
        let files = files(name);
        let formatted = format(&files).unwrap();
        for (path, text) in &formatted {
            assert_eq!(&files[path], text, "{name}/{path} is not canonical");
        }
        assert_eq!(format(&formatted).unwrap(), formatted, "{name}: fmt twice");
    }
}

#[test]
fn formatting_a_scrambled_dictionary_gives_the_committed_bytes() {
    // Reverse the key order of every object and re-indent: the same
    // definitions written by someone with other habits.
    fn scramble(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Object(map) => {
                let members: Vec<String> = map
                    .iter()
                    .rev()
                    .map(|(key, value)| format!("{}:  {}", serde_json::json!(key), scramble(value)))
                    .collect();
                format!("{{{}}}", members.join(",\n  "))
            }
            other => other.to_string(),
        }
    }
    for name in SHIPPED {
        let files = files(name);
        let scrambled: FileSet = files
            .iter()
            .map(|(path, text)| (path.clone(), scramble(&serde_json::from_str(text).unwrap())))
            .collect();
        assert_ne!(scrambled, files);
        assert_eq!(format(&scrambled).unwrap(), files, "{name}");
    }
}

#[test]
fn export_then_import_reproduces_every_shipped_dictionary() {
    for name in SHIPPED {
        let files = files(name);
        let tables =
            export(&Tree::parse(&files).unwrap()).unwrap_or_else(|e| panic!("{name}: {e:#?}"));
        assert_eq!(imported(&tables), interface_files(&files), "{name}");
        // And the tables themselves are stable.
        let again = export(&Tree::parse(&imported(&tables)).unwrap()).unwrap();
        assert_eq!(again, tables, "{name}");
    }
}

#[test]
fn the_worked_example_s_tables_give_the_committed_dictionary() {
    let tables = tables_in(worked_example().join("tables"));
    assert_eq!(imported(&tables), interface_files(&files("glasshouse")));
}

#[test]
fn import_reports_every_bad_row_with_its_file_and_row() {
    let mut tables = tables_in(worked_example().join("tables"));
    tables.fields = tables
        .fields
        .replace(
            "ZONE,Zone Status,door_open,5,11,bool",
            "ZONE,Zone Status,door_open,5,16,boolean",
        )
        .replace("ZONE,Alarm,severity,5,7-6", "ZONE,Alarm,severity,0,7-6")
        .replace(
            "SUPV,Vent Command,spare_84,6,11-0",
            "SUPV,Vent Order,spare_84,6,11-0",
        );
    tables.values.push_str(
        "ZONE,Zone Status,operating_mode,two,X\nZONE,Zone Status,operating_mode,2,Again\nZONE,Zone Status,no_such,1,X\n",
    );
    let errors = import(&tables, ImportOptions::default()).unwrap_err();
    // The values of the refused severity row are not reported again.
    let expect = [
        "fields.csv: row 8: bit 16: a 16-bit word has bits 0 to 15",
        "fields.csv: row 62: word 0: words are numbered from 1",
        "fields.csv: row 106: message \"Vent Order\" of SUPV has no row in messages.csv",
        "values.csv: row 32: value \"two\" is not a whole number",
        "values.csv: row 33: value 2 of ZONE/Zone Status/operating_mode has a row already",
        "values.csv: row 34: field \"no_such\" of ZONE/Zone Status has no row in fields.csv",
    ];
    assert_eq!(errors, expect, "{errors:#?}");
}

#[test]
fn names_that_would_leave_the_dictionary_directory_are_refused() {
    let mut tables = tables_in(worked_example().join("tables"));
    tables.interfaces.push_str("../outside,Escapes.\n");
    tables.messages.push_str("ZONE,../Escape,9,yes\n");
    let errors = import(&tables, ImportOptions::default()).unwrap_err();
    assert_eq!(
        errors,
        [
            "interfaces.csv: row 4: interface name \"../outside\" cannot be a directory of the dictionary",
            "messages.csv: row 13: message name \"../Escape\" cannot be a file name",
        ]
    );
}

#[test]
fn start_and_stop_must_agree_with_word_and_bits() {
    let mut tables = export(&Tree::parse(&files("glasshouse")).unwrap()).unwrap();
    // The export carries both forms; move one field's start by a bit.
    assert!(tables.fields.contains("ZONE,Alarm,severity,72,74,5,7-6,"));
    tables.fields = tables.fields.replace(
        "ZONE,Alarm,severity,72,74,5,7-6,",
        "ZONE,Alarm,severity,73,74,5,7-6,",
    );
    let errors = import(&tables, ImportOptions::default()).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].ends_with(
            "start 73 and stop 74 disagree with word 5 bits 7-6, which is [72, 74) under msb0"
        ),
        "{errors:?}"
    );
}

#[test]
fn unreviewed_rows_are_refused_unless_allowed() {
    let mut tables = tables_in(worked_example().join("tables"));
    tables.fields = tables
        .fields
        .replacen("source_table,note\n", "source_table,note,review\n", 1);
    tables.fields = tables.fields.replace(
        "Set while the zone door is open.,,,\n",
        "Set while the zone door is open.,,,,unreviewed\n",
    );
    let errors = import(&tables, ImportOptions::default()).unwrap_err();
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0].contains("row 8: the row is marked unreviewed"),
        "{errors:?}"
    );
    let allowed = import(
        &tables,
        ImportOptions {
            allow_unreviewed: true,
        },
    );
    assert!(allowed.is_ok());
}

#[test]
fn shipped_dictionaries_pass_lint_and_use_every_allowance() {
    for name in SHIPPED {
        let tree = Tree::parse(&files(name)).unwrap();
        let config = LintConfig::read(tree.dictionary.as_ref());
        assert!(
            config.malformed.is_empty(),
            "{name}: {:?}",
            config.malformed
        );
        let mut used = vec![false; config.allows.len()];
        let sorted = rules::sort(lint(&tree, &config), &config.allows, &mut used);
        rules::sort(coverage(&tree).findings, &config.allows, &mut used);
        assert!(sorted.kept.is_empty(), "{name}: {:#?}", sorted.kept);
        assert!(
            used.iter().all(|used| *used),
            "{name}: an allowance matches nothing"
        );
    }
}

#[test]
fn coverage_of_the_shipped_dictionaries() {
    // The worked example is complete: nothing is found but the message the
    // document names and defines elsewhere.
    let report = coverage(&Tree::parse(&files("glasshouse")).unwrap());
    let found: Vec<String> = report.findings.iter().map(ToString::to_string).collect();
    assert_eq!(
        found,
        ["C004 ZONE/Maintenance Data: named in message_type and not defined"]
    );
    assert_eq!(report.messages().count(), 10);
    assert!(report
        .messages()
        .all(|m| m.gaps.is_empty() && m.vectors == 1 && m.declared_bits.is_some()));
    assert_eq!(
        (
            report.fields,
            report.without_source,
            report.without_description
        ),
        (103, 0, 0)
    );

    // vehicle-can leaves bit 39 of Engine Status unassigned.
    let report = coverage(&Tree::parse(&files("vehicle-can")).unwrap());
    let engine = report
        .messages()
        .find(|m| m.message == "Engine Status")
        .unwrap();
    assert_eq!(engine.gaps, [(39, 40)]);
    assert_eq!(report.count("C001"), 1);
    assert_eq!(report.count("C002"), 0);
}

#[test]
fn revision_c_of_the_worked_example_is_a_breaking_change() {
    let old_files = files("glasshouse");
    let old = Tree::parse(&old_files).unwrap();
    let mut new_files = imported(&tables_in(worked_example().join("revision-c/tables")));
    new_files.insert(
        "dictionary.json".to_string(),
        old_files["dictionary.json"].replace("\"1.0.0\"", "\"2.0.0\""),
    );
    let new = Tree::parse(&new_files).unwrap();
    let result = diff(&old, &new);
    let mut found: Vec<String> = result
        .changes
        .iter()
        .map(|c| format!("{} {} {}", c.impact.word(), c.kind, c.location))
        .collect();
    found.sort();
    assert_eq!(
        found,
        [
            "additive added ZONE/Climate Report/dew_point",
            "additive relabelled ZONE/Alarm/alarm_code",
            "additive relabelled ZONE/Zone Status/operating_mode",
            "breaking renamed ZONE/Vent Report/motor_current",
            "documentation documented ZONE/Climate Report/leaf_wetness",
            "value rescaled ZONE/Irrigation Report/flow_rate",
            "value resized ZONE/Climate Report",
        ]
    );
    assert_eq!(result.impact(), Some(Impact::Breaking));
    assert_eq!(result.required_bump(), Bump::Major);
    assert_eq!(result.version_is_sufficient(), Some(true));
    // The other direction loses a field and two labels.
    assert!(diff(&new, &old).is_breaking());
    assert!(diff(&old, &old).changes.is_empty());
}

/// For every vector of every shipped dictionary: build a payload from the
/// values the vector expects, and the payload decodes to those values.
#[test]
fn encode_then_decode_gives_back_every_shipped_vector() {
    let mut checked = 0;
    for name in SHIPPED {
        let dictionary = Dictionary::load(shipped(name)).unwrap();
        let tree = Tree::parse(&files(name)).unwrap();
        for (vector_name, vector) in load_vectors(&dictionary).unwrap() {
            let vector = vector.unwrap();
            let interface = dictionary.interface(&vector.interface).unwrap();
            let labels = tree.interface(&vector.interface).unwrap();
            let Some(message) = vector
                .expect
                .message_type
                .as_deref()
                .and_then(|message| interface.message(message))
            else {
                continue; // a vector for an identifier with no definition
            };
            let original = hex::decode(vector.payload_hex.replace(' ', "")).unwrap();
            let values: Vec<(String, String)> = vector
                .expect
                .fields
                .iter()
                .filter_map(|(field, value)| Some((field.clone(), value.clone()?)))
                .collect();
            let encoded = encode(
                interface,
                labels,
                message,
                &values,
                Some(original.len()),
                Some(&original),
            )
            .unwrap_or_else(|e| panic!("{name}/{vector_name}: {e:#?}"));
            let rebuilt = VectorSpec {
                payload_hex: hex::encode(&encoded.payload),
                ..vector.clone()
            };
            let failures = check_vector(&dictionary, &rebuilt);
            assert!(failures.is_empty(), "{name}/{vector_name}: {failures:#?}");
            // The worked example's vectors give every field, so the bytes
            // themselves come back, up to the end of the definition.
            if name == "glasshouse" {
                let end = message.bit_length().div_ceil(8);
                assert_eq!(
                    encoded.payload[..end],
                    original[..end],
                    "{name}/{vector_name}"
                );
                assert!(encoded.left_zero.is_empty());
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 10 + 1 + 2 + 4);
}

#[test]
fn encode_refuses_what_a_field_cannot_hold() {
    let dictionary = Dictionary::load(shipped("glasshouse")).unwrap();
    let tree = Tree::parse(&files("glasshouse")).unwrap();
    let interface = dictionary.interface("ZONE").unwrap();
    let message = interface.message("Climate Report").unwrap();
    let values = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };
    let errors = encode(
        interface,
        tree.interface("ZONE").unwrap(),
        message,
        &values(&[
            ("relative_humidity", "81.35"),
            ("leaf_wetness", "64"),
            ("outside_temperature", "200"),
            ("air_temperature", "warm"),
            ("no_such_field", "1"),
        ]),
        None,
        None,
    )
    .unwrap_err();
    assert_eq!(
        errors,
        [
            "no_such_field: no such field in Climate Report",
            "air_temperature: \"warm\" is not a number or a label of the field",
            "relative_humidity: 81.35 is not a value the field can hold: the nearest are 81.30000000000001 and 81.4",
            "leaf_wetness: raw value 64 does not fit an unsigned 6-bit field",
            "outside_temperature: raw value 6048 does not fit an unsigned 12-bit field",
        ],
        "{errors:#?}"
    );
    // The message identifier is filled in without being asked for.
    let encoded = encode(
        interface,
        tree.interface("ZONE").unwrap(),
        message,
        &[],
        None,
        None,
    )
    .unwrap();
    assert_eq!(encoded.payload.len(), 20);
    assert_eq!(encoded.payload[0], 2);
}

#[test]
fn stats_over_the_worked_example_s_vectors() {
    let dictionary = Dictionary::load(shipped("glasshouse")).unwrap();
    let interface = dictionary.interface("ZONE").unwrap();
    let mut payloads: Vec<Payload> = load_vectors(&dictionary)
        .unwrap()
        .into_iter()
        .map(|(_, vector)| vector.unwrap())
        .filter(|vector| vector.interface == "ZONE")
        .map(|vector| Payload {
            frame_id: None,
            bytes: hex::decode(vector.payload_hex).unwrap(),
        })
        .collect();
    // A Maintenance Data message, an unknown identifier and a Zone Status
    // with a reserved mode and a spare bit set.
    payloads.push(Payload {
        frame_id: None,
        bytes: vec![7, 4, 0, 0, 0, 0, 0, 0],
    });
    payloads.push(Payload {
        frame_id: None,
        bytes: vec![99, 4, 0, 0, 0, 0, 0, 0],
    });
    payloads.push(Payload {
        frame_id: None,
        bytes: hex::decode("01080c1202932e009101096f4e332020").unwrap(),
    });
    let tree = Tree::parse(&files("glasshouse")).unwrap();
    let declared = |message: &str| {
        tree.interface("ZONE")?
            .message(message)?
            .u64_at("declared_bits")
    };
    let result = stats(interface, &declared, &payloads);
    assert_eq!(result.payloads, 9);
    assert_eq!(result.undefined["Maintenance Data"], 1);
    assert_eq!(result.unknown_types[&99], 1);
    let flags = result.flags();
    for flag in [
        "ZONE/Event Log: length differs from the declared 10 bytes in 1 payload",
        "ZONE/Event Log: 1 payload with non-zero bytes after the last field (byte 10)",
        "ZONE/Zone Status/operating_mode: 1 payload with a value that has no label: 9",
        "ZONE/Zone Status/spare_72: named as having no meaning and non-zero in 1 payload",
    ] {
        assert!(
            flags.iter().any(|f| f == flag),
            "missing {flag:?} in {flags:#?}"
        );
    }
}

/// The rule and unit tables of the conventions page are the ones the tool
/// uses.
#[test]
fn the_conventions_page_lists_every_rule_and_unit() {
    let page = fs::read_to_string(repo().join("docs/icd-to-dictionary/02-conventions.md")).unwrap();
    for rule in RULES {
        let row = format!("| `{}` | {} |", rule.id, rule.requires.replace('|', "\\|"));
        assert!(
            page.contains(&row),
            "02-conventions.md lacks the row: {row}"
        );
    }
    let rows = page
        .lines()
        .filter(|line| {
            line.starts_with("| `")
                && line[3..].starts_with(|c: char| c.is_ascii_uppercase())
                && line.as_bytes().get(7) == Some(&b'`')
        })
        .count();
    assert_eq!(
        rows,
        RULES.len(),
        "02-conventions.md has a rule row the tool does not have"
    );
    for (unit, meaning) in UNITS {
        let row = format!("| `{unit}` | {meaning} |");
        assert!(
            page.contains(&row),
            "02-conventions.md lacks the row: {row}"
        );
    }
}

/// The page that defines the format names every key the authoring tools read.
#[test]
fn the_format_page_documents_the_authoring_keys() {
    let page = fs::read_to_string(repo().join("docs/dictionary-format.md")).unwrap();
    for key in [
        "`source`",
        "`note`",
        "`declared_bits`",
        "`icd_numbering`",
        "`lint`",
        "`word_bits`",
        "`bit0`",
        "`first_word`",
        "`first_bit`",
        "`word_order`",
    ] {
        assert!(
            page.contains(key),
            "docs/dictionary-format.md does not mention {key}"
        );
    }
}
