//! Starting points: an empty dictionary and an empty test vector.

use super::json::Node;
use super::model::{fields, is_filler_name, FileSet, InterfaceTree};
use crate::schema::parse_int;

/// The `README.md` written into a new dictionary directory.
pub const DICTIONARY_README: &str = include_str!("dictionary-readme.md");

/// The files of a new dictionary: `dictionary.json`, a `metadata.json` for
/// each interface and a `README.md` that says what goes where. `interfaces`
/// pairs a name with its UDP or TCP port, if it has one.
pub fn new_dictionary(name: &str, interfaces: &[(String, Option<u16>)]) -> FileSet {
    let mut files = FileSet::new();
    let dictionary = Node::Object(vec![
        ("name".to_string(), Node::from(name)),
        ("version".to_string(), Node::from("0.1.0")),
    ]);
    files.insert("dictionary.json".to_string(), dictionary.to_text());
    for (interface, port) in interfaces {
        let mut metadata = Vec::new();
        if let Some(port) = port {
            metadata.push(("port".to_string(), Node::from(u64::from(*port))));
        }
        metadata.push((
            "message_type".to_string(),
            Node::Object(vec![
                ("start".to_string(), Node::from(0u64)),
                ("stop".to_string(), Node::from(8u64)),
                ("values".to_string(), Node::object()),
            ]),
        ));
        files.insert(
            format!("{interface}/metadata.json"),
            Node::Object(metadata).to_text(),
        );
    }
    files.insert("README.md".to_string(), DICTIONARY_README.to_string());
    files
}

/// A vector for `message` with every field listed and no value filled in:
/// the payload is empty and each expectation is `""`, so `bite check` fails
/// until the values are entered. The field on the message type bits is
/// filled with the message's own identifier or its label, and bits named as
/// spare or reserved with `"0"`.
pub fn new_vector(interface: &InterfaceTree, message: &str) -> Result<Node, String> {
    let node = interface
        .message(message)
        .ok_or_else(|| format!("{} has no message {message:?}", interface.name))?;
    let kind = interface.metadata.get("message_type");
    let from_frame = kind.and_then(|k| k.str_at("source")) == Some("frame_id");
    let mut ids: Vec<i128> = interface
        .type_values()
        .into_iter()
        .filter(|(_, name)| *name == message)
        .filter_map(|(key, _)| parse_int(key))
        .collect();
    ids.sort_unstable();
    let id = ids.first().copied();
    let type_range = kind
        .filter(|_| !from_frame)
        .and_then(|k| Some((k.u64_at("start")?, k.u64_at("stop")?)));

    let mut expected = Vec::new();
    for view in fields(node) {
        let value = if view.range().is_some() && view.range() == type_range {
            // What the decoder renders: the field's own label for the
            // identifier where it has one, the number otherwise.
            let label = view
                .node
                .get("values")
                .map(Node::entries)
                .unwrap_or_default()
                .iter()
                .find(|(key, _)| parse_int(key) == id && id.is_some())
                .and_then(|(_, label)| label.as_str());
            match (label, id) {
                (Some(label), _) => label.to_string(),
                (None, Some(id)) => id.to_string(),
                (None, None) => String::new(),
            }
        } else if is_filler_name(view.name) {
            "0".to_string()
        } else {
            String::new()
        };
        expected.push((view.name.to_string(), Node::from(value)));
    }

    let mut vector = vec![("interface".to_string(), Node::from(interface.name.as_str()))];
    if from_frame {
        if let Some(id) = id.and_then(|id| u64::try_from(id).ok()) {
            vector.push(("frame_id".to_string(), Node::from(id)));
        }
    }
    vector.push(("payload_hex".to_string(), Node::from("")));
    vector.push((
        "expect".to_string(),
        Node::Object(vec![
            ("message_type".to_string(), Node::from(message)),
            ("fields".to_string(), Node::Object(expected)),
        ]),
    ));
    vector.push(("note".to_string(), Node::from("")));
    Ok(Node::Object(vector))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::model::Tree;
    use crate::Dictionary;

    #[test]
    fn a_new_dictionary_loads_clean() {
        let files = new_dictionary(
            "example",
            &[("LINK".to_string(), Some(4000)), ("AUX".to_string(), None)],
        );
        let dictionary = Dictionary::from_files(files.iter()).unwrap();
        // In memory there is no messages/ directory, which is all it warns of.
        assert!(!dictionary.has_errors());
        assert!(dictionary
            .issues()
            .iter()
            .all(|issue| issue.message == "no messages directory"));
        assert_eq!(dictionary.interfaces().len(), 2);
        assert_eq!(dictionary.interface_for_port(4000).unwrap().name, "LINK");
        // And is already in canonical form.
        let mut json = files.clone();
        json.retain(|path, _| path.ends_with(".json"));
        assert_eq!(crate::authoring::canon::format(&json).unwrap(), json);
    }

    #[test]
    fn a_new_vector_lists_every_field() {
        let mut files = FileSet::new();
        files.insert(
            "A/metadata.json".into(),
            r#"{"message_type": {"start": 0, "stop": 8, "values": {"3": "M"}}}"#.into(),
        );
        files.insert(
            "A/messages/M.json".into(),
            r#"{"fields": {"kind": {"start": 0, "stop": 8, "values": {"3": "M"}},
                "x": {"start": 8, "stop": 12}, "spare_12": {"start": 12, "stop": 16}}}"#
                .into(),
        );
        let tree = Tree::parse(&files).unwrap();
        let vector = new_vector(&tree.interfaces[0], "M").unwrap();
        assert_eq!(
            vector.to_line(),
            r#"{"interface": "A", "payload_hex": "", "expect": {"message_type": "M", "fields": {"kind": "M", "x": "", "spare_12": "0"}}, "note": ""}"#
        );
        assert!(new_vector(&tree.interfaces[0], "N").is_err());
    }
}
