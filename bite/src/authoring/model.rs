//! A dictionary as its files, parsed but not interpreted.
//!
//! The decoder's loader ([`crate::Dictionary`]) drops what it cannot use: a
//! field with an inverted range is gone, an unknown key is ignored. The
//! authoring commands need the opposite, everything that was written, so they
//! read the same files into a [`Tree`] of [`Node`]s.

use super::json::Node;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

/// Dictionary files: path relative to the dictionary directory, with `/`
/// between components, to the file's text.
pub type FileSet = BTreeMap<String, String>;

/// Read the files of the dictionary layout under `root`: `dictionary.json`,
/// `<INTERFACE>/metadata.json`, `<INTERFACE>/messages/*.json` and
/// `vectors/*.json`. Other files are not read.
pub fn read_dir(root: &Path) -> io::Result<FileSet> {
    let mut files = FileSet::new();
    let mut read = |relative: String, path: &Path| -> io::Result<()> {
        files.insert(relative, fs::read_to_string(path)?);
        Ok(())
    };
    let dictionary = root.join("dictionary.json");
    if dictionary.is_file() {
        read("dictionary.json".to_string(), &dictionary)?;
    }
    for entry in sorted(root)? {
        let name = file_name(&entry);
        if !entry.is_dir() {
            continue;
        }
        if name == "vectors" {
            for path in sorted(&entry)? {
                if path.is_file() && is_json(&path) {
                    read(format!("vectors/{}", file_name(&path)), &path)?;
                }
            }
            continue;
        }
        let metadata = entry.join("metadata.json");
        if !metadata.is_file() {
            continue;
        }
        read(format!("{name}/metadata.json"), &metadata)?;
        let messages = entry.join("messages");
        if messages.is_dir() {
            for path in sorted(&messages)? {
                if path.is_file() && is_json(&path) {
                    read(format!("{name}/messages/{}", file_name(&path)), &path)?;
                }
            }
        }
    }
    Ok(files)
}

/// Write `files` under `root`, creating directories. Returns the paths whose
/// content changed or that did not exist.
pub fn write_dir(root: &Path, files: &FileSet) -> io::Result<Vec<String>> {
    let mut changed = Vec::new();
    for (relative, text) in files {
        let path = root.join(relative);
        if fs::read_to_string(&path).ok().as_deref() == Some(text.as_str()) {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, text)?;
        changed.push(relative.clone());
    }
    Ok(changed)
}

fn sorted(dir: &Path) -> io::Result<Vec<std::path::PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir)? {
        paths.push(entry?.path());
    }
    paths.sort();
    Ok(paths)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn is_json(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("json")
}

/// One interface directory.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceTree {
    pub name: String,
    /// `metadata.json`.
    pub metadata: Node,
    /// `messages/<name>.json`, by name.
    pub messages: Vec<(String, Node)>,
}

impl InterfaceTree {
    pub fn message(&self, name: &str) -> Option<&Node> {
        self.messages
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, node)| node)
    }

    /// `message_type.values` as written: key text and message name.
    pub fn type_values(&self) -> Vec<(&str, &str)> {
        self.metadata
            .get("message_type")
            .and_then(|node| node.get("values"))
            .map(Node::entries)
            .unwrap_or_default()
            .iter()
            .filter_map(|(key, name)| Some((key.as_str(), name.as_str()?)))
            .collect()
    }

    /// `msb0` unless the interface says `lsb0`.
    pub fn bit_order(&self) -> &str {
        self.metadata.str_at("bit_order").unwrap_or("msb0")
    }
}

/// Every file of a dictionary, parsed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tree {
    /// `dictionary.json`, when there is one.
    pub dictionary: Option<Node>,
    pub interfaces: Vec<InterfaceTree>,
    /// `vectors/<name>.json`, by name.
    pub vectors: Vec<(String, Node)>,
}

impl Tree {
    /// Parse a set of dictionary files. Every file that is not valid JSON, or
    /// whose top level is not an object, is an error; all are reported.
    pub fn parse(files: &FileSet) -> Result<Tree, Vec<String>> {
        let mut errors = Vec::new();
        let mut tree = Tree::default();
        // Interface name to its metadata and its messages.
        type Parts = (Option<Node>, Vec<(String, Node)>);
        let mut interfaces: BTreeMap<String, Parts> = BTreeMap::new();
        for (path, text) in files {
            let node = match Node::parse(text) {
                Ok(node) if node.is_object() => node,
                Ok(_) => {
                    errors.push(format!("{path}: the top level is not an object"));
                    continue;
                }
                Err(message) => {
                    errors.push(format!("{path}: {message}"));
                    continue;
                }
            };
            let parts: Vec<&str> = path.split('/').collect();
            match parts[..] {
                ["dictionary.json"] => tree.dictionary = Some(node),
                ["vectors", file] => tree.vectors.push((stem(file), node)),
                [interface, "metadata.json"] => {
                    interfaces.entry(interface.to_string()).or_default().0 = Some(node);
                }
                [interface, "messages", file] => {
                    interfaces
                        .entry(interface.to_string())
                        .or_default()
                        .1
                        .push((stem(file), node));
                }
                _ => {}
            }
        }
        for (name, (metadata, messages)) in interfaces {
            match metadata {
                Some(metadata) => tree.interfaces.push(InterfaceTree {
                    name,
                    metadata,
                    messages,
                }),
                None => errors.push(format!(
                    "{name}/messages has files and {name}/metadata.json is missing or unreadable"
                )),
            }
        }
        if errors.is_empty() {
            Ok(tree)
        } else {
            Err(errors)
        }
    }

    /// Read and parse the dictionary directory `root`.
    pub fn load(root: &Path) -> Result<Tree, Vec<String>> {
        if !root.is_dir() {
            return Err(vec![format!("{} is not a directory", root.display())]);
        }
        let files = read_dir(root).map_err(|e| vec![format!("{}: {e}", root.display())])?;
        Tree::parse(&files)
    }

    /// The tree as files, each in the layout [`Node::to_text`] writes. Key
    /// order is the tree's; [`super::canon::canonical`] sets the standard one.
    pub fn to_files(&self) -> FileSet {
        let mut files = FileSet::new();
        if let Some(dictionary) = &self.dictionary {
            files.insert("dictionary.json".to_string(), dictionary.to_text());
        }
        for interface in &self.interfaces {
            files.insert(
                format!("{}/metadata.json", interface.name),
                interface.metadata.to_text(),
            );
            for (name, message) in &interface.messages {
                files.insert(
                    format!("{}/messages/{name}.json", interface.name),
                    message.to_text(),
                );
            }
        }
        for (name, vector) in &self.vectors {
            files.insert(format!("vectors/{name}.json"), vector.to_text());
        }
        files
    }

    pub fn interface(&self, name: &str) -> Option<&InterfaceTree> {
        self.interfaces
            .iter()
            .find(|interface| interface.name == name)
    }
}

fn stem(file: &str) -> String {
    file.strip_suffix(".json").unwrap_or(file).to_string()
}

/// One field of a message file, read leniently: a key of the wrong type is
/// `None` here and reported by the commands that care.
#[derive(Debug, Clone)]
pub struct FieldView<'a> {
    pub name: &'a str,
    pub node: &'a Node,
    pub start: Option<u64>,
    pub stop: Option<u64>,
}

impl<'a> FieldView<'a> {
    /// `[start, stop)` when both are present and the range is not empty.
    pub fn range(&self) -> Option<(u64, u64)> {
        match (self.start, self.stop) {
            (Some(start), Some(stop)) if stop > start => Some((start, stop)),
            _ => None,
        }
    }

    /// The type the decoder uses for this field, by the loader's rules.
    pub fn kind(&self) -> &'a str {
        if let Some(kind) = self.node.str_at("type") {
            return kind;
        }
        if self.node.get("signed").and_then(Node::as_bool) == Some(true) {
            return "int";
        }
        match self.range() {
            Some((start, stop)) if stop - start > 64 && start % 8 == 0 && stop % 8 == 0 => "bytes",
            _ => "uint",
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self.kind(), "uint" | "int" | "float")
    }

    pub fn has_values(&self) -> bool {
        self.node
            .get("values")
            .is_some_and(|v| !v.entries().is_empty())
    }
}

/// The fields of a message file, ordered by start bit, then stop, then name.
pub fn fields(message: &Node) -> Vec<FieldView<'_>> {
    let mut views: Vec<FieldView<'_>> = message
        .get("fields")
        .map(Node::entries)
        .unwrap_or_default()
        .iter()
        .map(|(name, node)| FieldView {
            name,
            node,
            start: node.u64_at("start"),
            stop: node.u64_at("stop"),
        })
        .collect();
    views.sort_by_key(|view| {
        (
            view.start.unwrap_or(u64::MAX),
            view.stop.unwrap_or(u64::MAX),
            view.name,
        )
    });
    views
}

/// Whether a field name marks bits with no meaning: `spare_<n>`, `reserved_<n>`
/// and the looser spellings the lint rule N004 reports.
pub fn is_filler_name(name: &str) -> bool {
    const WORDS: [&str; 9] = [
        "spare",
        "reserved",
        "unused",
        "padding",
        "pad",
        "rsvd",
        "resv",
        "filler",
        "unassigned",
    ];
    let lower = name.to_ascii_lowercase();
    WORDS.iter().any(|word| {
        lower.strip_prefix(word).is_some_and(|rest| {
            let rest = rest.strip_prefix('_').unwrap_or(rest);
            rest.is_empty()
                || rest == "bit"
                || rest == "bits"
                || rest.chars().all(|c| c.is_ascii_digit())
        })
    })
}

/// Where a definition was transcribed from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceRef {
    pub document: Option<String>,
    pub revision: Option<String>,
    pub section: Option<String>,
    pub table: Option<String>,
    pub page: Option<String>,
}

pub const SOURCE_KEYS: [&str; 5] = ["document", "revision", "section", "table", "page"];

impl SourceRef {
    /// Read a `source` object. Members may be text or numbers.
    pub fn read(owner: &Node) -> SourceRef {
        let text = |key: &str| -> Option<String> {
            match owner.get("source")?.get(key)? {
                Node::String(text) if !text.trim().is_empty() => Some(text.clone()),
                Node::Number(number) => Some(super::json::number_text(number)),
                _ => None,
            }
        };
        SourceRef {
            document: text("document"),
            revision: text("revision"),
            section: text("section"),
            table: text("table"),
            page: text("page"),
        }
    }

    /// This reference with every member it lacks taken from `outer`.
    pub fn or(self, outer: &SourceRef) -> SourceRef {
        SourceRef {
            document: self.document.or_else(|| outer.document.clone()),
            revision: self.revision.or_else(|| outer.revision.clone()),
            section: self.section.or_else(|| outer.section.clone()),
            table: self.table.or_else(|| outer.table.clone()),
            page: self.page.or_else(|| outer.page.clone()),
        }
    }

    pub fn has_document(&self) -> bool {
        self.document.is_some() && self.revision.is_some()
    }

    /// A section, a table or a page: something a reader can turn to.
    pub fn has_locator(&self) -> bool {
        self.section.is_some() || self.table.is_some() || self.page.is_some()
    }

    pub fn is_complete(&self) -> bool {
        self.has_document() && self.has_locator()
    }
}

/// The source of an interface: its own, completed from the dictionary's, with
/// `icd.version` standing in for a missing revision.
pub fn interface_source(tree: &Tree, interface: &InterfaceTree) -> SourceRef {
    let outer = tree
        .dictionary
        .as_ref()
        .map(SourceRef::read)
        .unwrap_or_default();
    let mut source = SourceRef::read(&interface.metadata).or(&outer);
    if source.revision.is_none() {
        source.revision = interface
            .metadata
            .get("icd")
            .and_then(|icd| icd.str_at("version"))
            .map(str::to_string);
    }
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filler_names() {
        for name in [
            "spare",
            "spare_40",
            "reserved_8",
            "Reserved",
            "pad2",
            "unused_bits",
        ] {
            assert!(is_filler_name(name), "{name}");
        }
        for name in [
            "spare_tire_pressure_ok",
            "padlock",
            "reservoir",
            "sparetime",
        ] {
            assert!(!is_filler_name(name), "{name}");
        }
    }

    #[test]
    fn a_broken_file_is_named() {
        let mut files = FileSet::new();
        files.insert("A/metadata.json".into(), "{".into());
        files.insert("A/messages/M.json".into(), "[]".into());
        let errors = Tree::parse(&files).unwrap_err();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors[0].starts_with("A/messages/M.json: the top level"));
        assert!(errors[1].starts_with("A/metadata.json: "));
    }

    #[test]
    fn source_members_are_inherited() {
        let outer = SourceRef::read(
            &Node::parse(r#"{"source": {"document": "D-1", "revision": "B"}}"#).unwrap(),
        );
        let inner = SourceRef::read(&Node::parse(r#"{"source": {"table": 7}}"#).unwrap());
        let resolved = inner.or(&outer);
        assert!(resolved.is_complete());
        assert_eq!(resolved.table.as_deref(), Some("7"));
    }
}
