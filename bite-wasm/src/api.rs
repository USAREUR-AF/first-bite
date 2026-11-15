//! What the WebAssembly module does, in plain Rust: JSON values in and out,
//! failures as the text the `bite` command line prints for the same mistake.

use bite::vectors::{check_vector, load_vectors};
use bite::{Dictionary, Interface, Severity};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// A dictionary built from files held in memory.
pub struct Session {
    dictionary: Dictionary,
}

impl Session {
    /// Build a dictionary from relative file path to file text.
    ///
    /// A folder picked in a browser arrives with its own name in front of
    /// every path (`my-dictionary/WX/metadata.json`). Leading directories that
    /// every path shares are dropped until an interface is found.
    pub fn load(files: BTreeMap<String, String>) -> Result<Self, String> {
        let mut files: Vec<(Vec<&str>, &str)> = files
            .iter()
            .map(|(path, text)| {
                let parts = path
                    .split('/')
                    .filter(|part| !part.is_empty() && *part != ".")
                    .collect();
                (parts, text.as_str())
            })
            .collect();
        while !has_interface(&files) && shares_first_directory(&files) {
            for (parts, _) in &mut files {
                parts.remove(0);
            }
        }
        Dictionary::from_files(files.iter().map(|(parts, text)| (parts.join("/"), *text)))
            .map(|dictionary| Session { dictionary })
            .map_err(|e| e.to_string())
    }

    /// `dictionary.json`: `{name, version, description}`, each possibly null.
    pub fn info(&self) -> Value {
        let spec = &self.dictionary.spec;
        json!({
            "name": spec.name,
            "version": spec.version,
            "description": spec.description,
        })
    }

    /// Every interface: its name, port, how it numbers bits and identifies
    /// messages, and its messages with the layout of their fields.
    pub fn interfaces(&self) -> Value {
        self.dictionary
            .interfaces()
            .iter()
            .map(describe_interface)
            .collect()
    }

    /// What loading found wrong, as `bite check` prints it (`text`) and in
    /// parts.
    pub fn issues(&self) -> Value {
        self.dictionary
            .issues()
            .iter()
            .map(|issue| {
                json!({
                    "severity": match issue.severity {
                        Severity::Warning => "warning",
                        Severity::Error => "error",
                    },
                    "location": issue.location,
                    "message": issue.message,
                    "text": issue.to_string(),
                })
            })
            .collect()
    }

    /// Decode a payload against the named interface: the JSON `bite decode`
    /// prints.
    pub fn decode(
        &self,
        interface: &str,
        payload_hex: &str,
        frame_id: Option<u64>,
    ) -> Result<Value, String> {
        let interface = self
            .dictionary
            .interface(interface)
            .ok_or_else(|| format!("no interface named {interface:?}"))?;
        decode(interface, payload_hex, frame_id)
    }

    /// Decode a payload against the interface that claims `port`.
    pub fn decode_by_port(
        &self,
        port: u16,
        payload_hex: &str,
        frame_id: Option<u64>,
    ) -> Result<Value, String> {
        let interface = self
            .dictionary
            .interface_for_port(port)
            .ok_or_else(|| format!("no interface claims port {port}"))?;
        decode(interface, payload_hex, frame_id)
    }

    /// JSON Schema for the decoded form of every message, keyed
    /// `interface/message`: what `bite schema` prints.
    pub fn schema(&self) -> Value {
        Value::Object(bite::jsonschema::dictionary_schemas(&self.dictionary))
    }

    /// The vectors the dictionary carries: `{name, interface, payload_hex,
    /// frame_id, expect}`, or `{name, error}` for a file that is not a vector.
    pub fn vectors(&self) -> Value {
        load_vectors(&self.dictionary)
            .unwrap_or_default()
            .into_iter()
            .map(|(name, vector)| match vector {
                Ok(vector) => {
                    let mut value = serde_json::to_value(vector).unwrap_or_default();
                    value["name"] = json!(name);
                    value
                }
                Err(error) => json!({"name": name, "error": error}),
            })
            .collect()
    }

    /// Run every vector: `{name, passed, failures}` for each.
    pub fn run_vectors(&self) -> Value {
        load_vectors(&self.dictionary)
            .unwrap_or_default()
            .into_iter()
            .map(|(name, vector)| {
                let failures = match vector {
                    Ok(vector) => check_vector(&self.dictionary, &vector),
                    Err(error) => vec![error],
                };
                json!({
                    "name": name,
                    "passed": failures.is_empty(),
                    "failures": failures,
                })
            })
            .collect()
    }
}

/// Whether the files, as they stand, hold an interface at the top level.
fn has_interface(files: &[(Vec<&str>, &str)]) -> bool {
    files
        .iter()
        .any(|(parts, _)| matches!(parts[..], [_, "metadata.json"]))
}

/// Whether every file is under the same top-level directory.
fn shares_first_directory(files: &[(Vec<&str>, &str)]) -> bool {
    match files.first() {
        Some((first, _)) if first.len() > 1 => files
            .iter()
            .all(|(parts, _)| parts.len() > 1 && parts[0] == first[0]),
        _ => false,
    }
}

fn decode(
    interface: &Interface,
    payload_hex: &str,
    frame_id: Option<u64>,
) -> Result<Value, String> {
    let compact: String = payload_hex.chars().filter(|c| !c.is_whitespace()).collect();
    let payload = hex::decode(compact).map_err(|e| format!("payload: {e}"))?;
    let decoded = interface
        .decode_frame(&payload, frame_id)
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "interface": interface.name,
        "message_type": decoded.message_type(),
        "fields": decoded,
    }))
}

fn describe_interface(interface: &Interface) -> Value {
    let disc = &interface.discriminator;
    let mut codes: Vec<(&u64, &String)> = disc.names.iter().collect();
    codes.sort();

    // Messages with a file, in the dictionary's order, then the names the
    // interface identifies but has no file for.
    let mut messages: Vec<Value> = interface
        .messages()
        .iter()
        .map(|message| {
            let fields: Vec<Value> = message
                .fields
                .iter()
                .map(|field| {
                    json!({
                        "name": field.name,
                        "start": field.start,
                        "stop": field.stop,
                        "type": field.kind,
                        "byte_order": field.byte_order,
                        "scale": field.scale,
                        "offset": field.offset,
                        "unit": field.unit,
                        "description": field.description,
                        "labelled": field.has_labels(),
                    })
                })
                .collect();
            json!({
                "name": message.name,
                "ids": ids(&codes, &message.name),
                "defined": true,
                "description": message.description,
                "bit_length": message.bit_length(),
                "fields": fields,
            })
        })
        .collect();
    let mut undefined: Vec<&str> = codes
        .iter()
        .map(|(_, name)| name.as_str())
        .filter(|name| interface.message(name).is_none())
        .collect();
    undefined.sort_unstable();
    undefined.dedup();
    for name in undefined {
        messages.push(json!({
            "name": name,
            "ids": ids(&codes, name),
            "defined": false,
            "description": null,
            "bit_length": 0,
            "fields": [],
        }));
    }

    json!({
        "name": interface.name,
        "port": interface.port,
        "description": interface.description,
        "bit_order": interface.bit_order,
        "byte_order": interface.byte_order,
        "message_type": {
            "source": disc.source,
            "start": disc.start,
            "stop": disc.stop,
            "mask": disc.mask,
        },
        "messages": messages,
    })
}

/// The identifiers that select the message called `name`.
fn ids(codes: &[(&u64, &String)], name: &str) -> Vec<u64> {
    codes
        .iter()
        .filter(|(_, known)| known.as_str() == name)
        .map(|(code, _)| **code)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// A shipped dictionary as path to text, each path under `prefix`.
    fn shipped(name: &str, prefix: &str) -> BTreeMap<String, String> {
        fn walk(dir: &Path, prefix: &str, files: &mut BTreeMap<String, String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let relative = format!("{prefix}{}", path.file_name().unwrap().to_string_lossy());
                if path.is_dir() {
                    walk(&path, &format!("{relative}/"), files);
                } else {
                    files.insert(relative, fs::read_to_string(&path).unwrap());
                }
            }
        }
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../dictionaries")
            .join(name);
        let mut files = BTreeMap::new();
        walk(&root, prefix, &mut files);
        files
    }

    fn session(name: &str) -> Session {
        Session::load(shipped(name, "")).unwrap()
    }

    #[test]
    fn decode_is_the_json_the_command_line_prints() {
        let session = session("weather-station");
        let decoded = session
            .decode("WX", "01 00 0c 4b58370000015180600000", None)
            .unwrap();
        assert_eq!(
            decoded,
            json!({
                "interface": "WX",
                "message_type": "Heartbeat",
                "fields": {
                    "message_type": "Heartbeat",
                    "message_length": 12,
                    "station_id": "KX7",
                    "uptime": 86400,
                    "battery_low": false,
                    "sensor_fault": true,
                    "mode": "Calibrating",
                    "reserved": 0,
                },
            })
        );
        assert_eq!(
            session.decode_by_port(40100, "2a0003", None).unwrap(),
            json!({"interface": "WX", "message_type": "42", "fields": {}})
        );
    }

    #[test]
    fn a_field_beyond_the_payload_is_null() {
        let decoded = session("weather-station")
            .decode("WX", "0200174b583700fb2e", None)
            .unwrap();
        assert_eq!(decoded["fields"]["temperature"], json!(-12.34));
        assert_eq!(decoded["fields"]["pressure"], Value::Null);
    }

    #[test]
    fn frame_identifier_selects_the_message() {
        let session = session("vehicle-can");
        let decoded = session
            .decode("POWERTRAIN", "c83282b54d6aff00", Some(0x100))
            .unwrap();
        assert_eq!(decoded["message_type"], "Engine Status");
        assert_eq!(decoded["fields"]["gear"], "Third");
        assert_eq!(decoded["fields"]["engine_speed"], json!(3250.0));
    }

    #[test]
    fn mistakes_are_reported_in_the_command_line_s_words() {
        let weather = session("weather-station");
        assert_eq!(
            weather.decode("NOPE", "00", None).unwrap_err(),
            "no interface named \"NOPE\""
        );
        assert_eq!(
            weather.decode_by_port(1, "00", None).unwrap_err(),
            "no interface claims port 1"
        );
        assert_eq!(
            weather.decode("WX", "0", None).unwrap_err(),
            "payload: Odd number of digits"
        );
        assert_eq!(
            weather.decode("WX", "", None).unwrap_err(),
            "payload of 0 bits is too short to hold the message type at bits [0, 8)"
        );
        assert_eq!(
            session("vehicle-can")
                .decode("POWERTRAIN", "00", None)
                .unwrap_err(),
            "interface POWERTRAIN identifies messages by frame identifier, and none was given"
        );
        assert_eq!(
            Session::load(BTreeMap::new()).err().unwrap(),
            "no interface found under the files given: expected sub-directories containing metadata.json"
        );
    }

    #[test]
    fn interfaces_carry_the_layout_of_every_field() {
        let interfaces = session("vehicle-can").interfaces();
        let powertrain = &interfaces[0];
        assert_eq!(powertrain["name"], "POWERTRAIN");
        assert_eq!(powertrain["port"], Value::Null);
        assert_eq!(powertrain["bit_order"], "lsb0");
        assert_eq!(powertrain["message_type"]["source"], "frame_id");
        assert_eq!(powertrain["message_type"]["mask"], 0x7FF);
        let engine = &powertrain["messages"][0];
        assert_eq!(engine["name"], "Engine Status");
        assert_eq!(engine["ids"], json!([0x100]));
        assert_eq!(engine["bit_length"], 56);
        assert_eq!(
            engine["fields"][3],
            json!({
                "name": "gear",
                "start": 34,
                "stop": 38,
                "type": "uint",
                "byte_order": "big",
                "scale": null,
                "offset": null,
                "unit": null,
                "description": null,
                "labelled": true,
            })
        );

        let interfaces = session("weather-station").interfaces();
        assert_eq!(interfaces[0]["port"], 40100);
        let names: Vec<&str> = interfaces[0]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Heartbeat", "Observation", "Position"]);
    }

    #[test]
    fn a_message_with_no_file_is_listed_as_not_defined() {
        let interfaces = session("toot").interfaces();
        let messages = interfaces[0]["messages"].as_array().unwrap();
        assert_eq!(messages[0]["name"], "FOMO Report Message");
        assert_eq!(messages[1]["name"], "None");
        assert_eq!(messages[1]["defined"], false);
        assert_eq!(messages[1]["ids"], json!([0]));
    }

    #[test]
    fn every_shipped_vector_passes_and_is_listed() {
        for (name, count) in [("weather-station", 5), ("vehicle-can", 2), ("toot", 1)] {
            let session = session(name);
            assert_eq!(session.info()["name"] == Value::Null, name == "toot");
            let results = session.run_vectors();
            let results = results.as_array().unwrap();
            assert_eq!(results.len(), count, "{name}");
            for result in results {
                assert_eq!(result["passed"], true, "{name}: {result}");
            }
            let vectors = session.vectors();
            let vectors = vectors.as_array().unwrap();
            assert_eq!(vectors.len(), count);
            for vector in vectors {
                assert!(vector["payload_hex"].is_string(), "{vector}");
                assert!(vector["interface"].is_string(), "{vector}");
            }
        }
    }

    #[test]
    fn issues_and_broken_vectors_are_reported() {
        let mut files = shipped("toot", "");
        files.insert("FAE/messages/Spare.json".into(), r#"{"fields": {}}"#.into());
        files.insert("vectors/broken.json".into(), "{".into());
        let session = Session::load(files).unwrap();
        let issues = session.issues();
        assert_eq!(issues.as_array().unwrap().len(), 2);
        assert_eq!(
            issues[0],
            json!({
                "severity": "warning",
                "location": "FAE/Spare",
                "message": "no message_type value maps to this message; it can never be selected",
                "text": "warning: FAE/Spare: no message_type value maps to this message; it can never be selected",
            })
        );
        assert_eq!(session.run_vectors()[0]["passed"], false);
        assert!(session.vectors()[0]["error"].is_string());
    }

    #[test]
    fn schema_is_keyed_by_interface_and_message() {
        let schema = session("weather-station").schema();
        assert_eq!(schema.as_object().unwrap().len(), 3);
        assert_eq!(
            schema["WX/Observation"]["properties"]["pressure"]["x-bits"],
            json!([72, 92])
        );
    }

    #[test]
    fn a_picked_folder_s_own_name_is_dropped() {
        let nested = Session::load(shipped("weather-station", "Downloads/weather-station/"));
        let interfaces = nested.unwrap().interfaces();
        assert_eq!(interfaces[0]["name"], "WX");

        // A dictionary of one interface and nothing else is already at the top.
        let mut files = shipped("toot", "");
        files.retain(|path, _| path.starts_with("FAE/"));
        let session = Session::load(files).unwrap();
        assert_eq!(session.interfaces()[0]["name"], "FAE");
    }
}
