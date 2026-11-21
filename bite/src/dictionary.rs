//! A dictionary loaded from a directory or from files held in memory, checked,
//! and indexed for decoding.

use crate::limits::{
    quoted, MAX_FIELDS_PER_MESSAGE, MAX_FILES, MAX_FILE_BYTES, MAX_PAYLOAD_BITS, MAX_PAYLOAD_BYTES,
    MAX_VALUES,
};
use crate::schema::{
    parse_int, BitOrder, BitRange, ByteOrder, DictionarySpec, DiscriminatorSource, FieldKind,
    FieldSpec, Framing, Icd, InterfaceSpec, MessageSpec, Protocol,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// The dictionary could not be read at all.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not a directory")]
    NotADirectory { path: PathBuf },
    #[error(
        "no interface found under {}: expected sub-directories containing metadata.json",
        shown(.path)
    )]
    Empty { path: PathBuf },
    #[error(
        "{} holds more than {limit} files; a dictionary may hold at most that many",
        shown(.path)
    )]
    TooManyFiles { path: PathBuf, limit: usize },
}

/// A dictionary built from files in memory has no directory to name.
fn shown(path: &Path) -> std::borrow::Cow<'_, str> {
    if path.as_os_str().is_empty() {
        "the files given".into()
    } else {
        path.to_string_lossy()
    }
}

/// The text of one dictionary file, or why it could not be read.
type FileText = Result<String, String>;

/// A dictionary's files, read but not yet parsed. Both ways of loading a
/// dictionary produce this, and [`build`] turns it into a [`Dictionary`].
#[derive(Default)]
struct Source {
    /// `dictionary.json`, when there is one.
    dictionary: Option<FileText>,
    /// Interface directory name to its files.
    interfaces: BTreeMap<String, InterfaceSource>,
    /// `vectors/*.json`: file name to text.
    vectors: BTreeMap<String, FileText>,
    /// Why `vectors/` could not be listed, when it could not.
    vectors_error: Option<(io::ErrorKind, String)>,
}

impl Source {
    /// Files held: `dictionary.json`, metadata, messages and vectors.
    fn file_count(&self) -> usize {
        usize::from(self.dictionary.is_some())
            + self.vectors.len()
            + self
                .interfaces
                .values()
                .map(|interface| {
                    usize::from(interface.metadata.is_some()) + interface.messages.len()
                })
                .sum::<usize>()
    }
}

#[derive(Default)]
struct InterfaceSource {
    /// `metadata.json`; a directory without one is not an interface.
    metadata: Option<FileText>,
    has_messages_dir: bool,
    /// `messages/*.json`: file name to text.
    messages: BTreeMap<String, FileText>,
}

/// One file from a dictionary's `vectors/` directory, kept as text.
#[derive(Debug, Clone)]
pub struct VectorFile {
    /// The file stem.
    pub name: String,
    /// The file's text, or why it could not be read.
    pub text: Result<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Worth a look; the item is still used.
    Warning,
    /// The item named is unusable and was left out of the loaded dictionary.
    Error,
}

/// Something wrong in a dictionary file, found while loading.
#[derive(Debug, Clone)]
pub struct Issue {
    pub severity: Severity,
    /// `interface`, `interface/message` or `interface/message/field`.
    pub location: String,
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = match self.severity {
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        write!(f, "{severity}: {}: {}", self.location, self.message)
    }
}

/// One field of a message, ready to decode.
#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub start: usize,
    pub stop: usize,
    pub kind: FieldKind,
    pub bit_order: BitOrder,
    pub byte_order: ByteOrder,
    pub scale: Option<f64>,
    pub offset: Option<f64>,
    pub unit: Option<String>,
    pub description: Option<String>,
    labels: HashMap<i128, String>,
    /// Decimal places of a scaled integer, when `scale` and `offset` are
    /// short decimals; see [`decimal_places`].
    decimals: Option<u32>,
}

/// Most decimal places a scaled value is rounded to. A double carries 15
/// significant decimal digits.
const MAX_DECIMALS: u32 = 15;

/// The decimal places `raw * scale + offset` can have for an integer `raw`:
/// the larger of the decimal places `scale` and `offset` are written with.
/// `None` when either needs more than [`MAX_DECIMALS`], which is the case for
/// a number that is not a short decimal, such as 2^-32 or 1/3.
fn decimal_places(scale: Option<f64>, offset: Option<f64>) -> Option<u32> {
    let places = |number: Option<f64>| -> Option<u32> {
        let Some(number) = number else { return Some(0) };
        if !number.is_finite() {
            return None;
        }
        // The shortest decimal that reads back as the same double.
        let text = number.to_string();
        let places = text
            .split_once('.')
            .map_or(0, |(_, fraction)| fraction.len());
        u32::try_from(places)
            .ok()
            .filter(|places| *places <= MAX_DECIMALS)
    };
    Some(places(scale)?.max(places(offset)?))
}

impl Field {
    /// A raw integer with the field's scale and offset applied.
    ///
    /// A document gives a scale as a decimal, such as 0.01 or 1e-7, which a
    /// double holds only approximately, so the product carries an error in
    /// its last digits: 35 * 0.01 is 0.35000000000000003. The result is
    /// therefore rounded to the decimal places the scale and offset have,
    /// which gives the double nearest the exact decimal: 0.35. A scale that
    /// is not a short decimal (a power of two such as 2^-32, or 1/3) and a
    /// value with more than 15 significant digits are left as computed.
    pub fn scaled(&self, raw: f64) -> f64 {
        let value = self.scaled_float(raw);
        let Some(decimals) = self.decimals else {
            return value;
        };
        let power = 10f64.powi(decimals as i32);
        let shifted = (value * power).round();
        // Below 10^15 the shifted value is a whole number a double holds
        // exactly, and dividing it by a power of ten is correctly rounded.
        if shifted.abs() < 1e15 {
            shifted / power
        } else {
            value
        }
    }

    /// A `float` field's value with the scale and offset applied. The value
    /// on the wire is already a binary fraction, so nothing is rounded.
    pub fn scaled_float(&self, value: f64) -> f64 {
        value * self.scale.unwrap_or(1.0) + self.offset.unwrap_or(0.0)
    }

    pub fn width(&self) -> usize {
        self.stop - self.start
    }

    /// The label the dictionary gives a raw integer value, if any.
    pub fn label(&self, raw: i128) -> Option<&str> {
        self.labels.get(&raw).map(String::as_str)
    }

    pub fn has_labels(&self) -> bool {
        !self.labels.is_empty()
    }

    /// Every label the dictionary defines for this field, in no fixed order.
    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.labels.values().map(String::as_str)
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    pub name: String,
    pub description: Option<String>,
    /// Ordered by start bit, then name.
    pub fields: Vec<Field>,
}

impl Message {
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|field| field.name == name)
    }

    /// Bits needed to hold every field.
    pub fn bit_length(&self) -> usize {
        self.fields
            .iter()
            .map(|field| field.stop)
            .max()
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone)]
pub struct Discriminator {
    pub source: DiscriminatorSource,
    pub start: usize,
    pub stop: usize,
    pub mask: Option<u64>,
    /// Identifier to message name, including names with no message file.
    pub names: HashMap<u64, String>,
}

#[derive(Debug, Clone)]
pub struct Interface {
    pub name: String,
    pub description: Option<String>,
    pub icd: Option<Icd>,
    pub protocol: Option<Protocol>,
    pub port: Option<u16>,
    pub bit_order: BitOrder,
    pub byte_order: ByteOrder,
    pub discriminator: Discriminator,
    pub message_length: Option<BitRange>,
    /// [`Framing::Concatenated`] only when `message_length` is usable; see
    /// [`Interface::split`].
    pub framing: Framing,
    messages: Vec<Message>,
    by_name: HashMap<String, usize>,
}

impl Interface {
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn message(&self, name: &str) -> Option<&Message> {
        self.by_name.get(name).map(|&index| &self.messages[index])
    }
}

/// A set of interfaces. Immutable once loaded; share it with `Arc`.
#[derive(Debug, Clone)]
pub struct Dictionary {
    pub spec: DictionarySpec,
    pub root: PathBuf,
    interfaces: Vec<Interface>,
    by_name: HashMap<String, usize>,
    by_port: HashMap<u16, usize>,
    issues: Vec<Issue>,
    vectors: Vec<VectorFile>,
    vectors_error: Option<(io::ErrorKind, String)>,
}

impl Dictionary {
    /// Load every interface under the directory `root`.
    ///
    /// Files that cannot be parsed and fields that cannot be decoded are left
    /// out and reported through [`Dictionary::issues`]; the rest of the
    /// dictionary stays usable. Use [`Dictionary::has_errors`] to refuse a
    /// dictionary with any such item.
    pub fn load(root: impl AsRef<Path>) -> Result<Self, LoadError> {
        let root = root.as_ref();
        if !root.is_dir() {
            return Err(LoadError::NotADirectory {
                path: root.to_path_buf(),
            });
        }
        build(read_directory(root)?, root.to_path_buf())
    }

    /// Build a dictionary from files held in memory: pairs of a path relative
    /// to the dictionary directory, with `/` between components
    /// (`WX/messages/Heartbeat.json`), and the text of that file.
    ///
    /// The result is the dictionary [`Dictionary::load`] gives for a directory
    /// with the same files, issues included; the paths named in issue text are
    /// relative, and [`Dictionary::root`] is empty. Paths that are not part of
    /// the dictionary layout are ignored.
    ///
    /// ```
    /// let dictionary = bite::Dictionary::from_files([
    ///     (
    ///         "LINK/metadata.json",
    ///         r#"{"message_type": {"start": 0, "stop": 8, "values": {"1": "Ping"}}}"#,
    ///     ),
    ///     (
    ///         "LINK/messages/Ping.json",
    ///         r#"{"fields": {"sequence": {"start": 8, "stop": 24}}}"#,
    ///     ),
    /// ])?;
    /// let decoded = dictionary.interface("LINK").unwrap().decode(&[1, 0, 7])?;
    /// assert_eq!(decoded.rendered()["sequence"].as_deref(), Some("7"));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn from_files<P, T>(files: impl IntoIterator<Item = (P, T)>) -> Result<Self, LoadError>
    where
        P: AsRef<str>,
        T: Into<String>,
    {
        let mut source = Source::default();
        for (path, text) in files {
            let parts: Vec<&str> = path
                .as_ref()
                .split('/')
                .filter(|part| !part.is_empty() && *part != ".")
                .collect();
            let text: String = text.into();
            let text = if text.len() > MAX_FILE_BYTES {
                Err(too_large(text.len() as u64))
            } else {
                Ok(text)
            };
            if let ["vectors", file] = parts[..] {
                if is_json(file) {
                    source.vectors.insert(file.to_string(), text.clone());
                }
            }
            match parts[..] {
                ["dictionary.json"] => source.dictionary = Some(text),
                [interface, "metadata.json"] => {
                    source
                        .interfaces
                        .entry(interface.to_string())
                        .or_default()
                        .metadata = Some(text);
                }
                [interface, "messages", ref rest @ ..] if !rest.is_empty() => {
                    let entry = source.interfaces.entry(interface.to_string()).or_default();
                    entry.has_messages_dir = true;
                    if let [file] = rest {
                        if is_json(file) {
                            entry.messages.insert(file.to_string(), text);
                        }
                    }
                }
                _ => {}
            }
            // Checked as files are kept, so an endless iterator stops here
            // and not when memory runs out.
            if source.file_count() > MAX_FILES {
                return Err(LoadError::TooManyFiles {
                    path: PathBuf::new(),
                    limit: MAX_FILES,
                });
            }
        }
        build(source, PathBuf::new())
    }

    pub fn interfaces(&self) -> &[Interface] {
        &self.interfaces
    }

    pub fn interface(&self, name: &str) -> Option<&Interface> {
        self.by_name.get(name).map(|&index| &self.interfaces[index])
    }

    /// The interface carried on a UDP or TCP port.
    pub fn interface_for_port(&self, port: u16) -> Option<&Interface> {
        self.by_port
            .get(&port)
            .map(|&index| &self.interfaces[index])
    }

    /// The interface a packet belongs to, judged by its ports. The source port
    /// is tried first.
    pub fn interface_for_ports(&self, source: u16, destination: u16) -> Option<&Interface> {
        self.interface_for_port(source)
            .or_else(|| self.interface_for_port(destination))
    }

    pub fn issues(&self) -> &[Issue] {
        &self.issues
    }

    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity == Severity::Error)
    }

    /// The files of the dictionary's `vectors/` directory, by file name.
    /// Empty when the dictionary carries none.
    pub fn vector_files(&self) -> Result<&[VectorFile], LoadError> {
        match &self.vectors_error {
            None => Ok(&self.vectors),
            Some((kind, message)) => Err(LoadError::Io {
                path: self.root.join("vectors"),
                source: io::Error::new(*kind, message.clone()),
            }),
        }
    }
}

/// Read the files of the dictionary directory `root` into memory.
fn read_directory(root: &Path) -> Result<Source, LoadError> {
    let mut source = Source::default();
    let too_many = || LoadError::TooManyFiles {
        path: root.to_path_buf(),
        limit: MAX_FILES,
    };
    // Files read so far; checked before each read.
    let mut files = 0usize;
    let mut count = || {
        files += 1;
        if files > MAX_FILES {
            Err(too_many())
        } else {
            Ok(())
        }
    };
    let dictionary = root.join("dictionary.json");
    if dictionary.is_file() {
        count()?;
        source.dictionary = Some(read_text(&dictionary));
    }
    for dir in sorted_entries(root)? {
        let metadata = dir.join("metadata.json");
        if !dir.is_dir() || !metadata.is_file() {
            continue;
        }
        count()?;
        let mut interface = InterfaceSource {
            metadata: Some(read_text(&metadata)),
            ..InterfaceSource::default()
        };
        let messages_dir = dir.join("messages");
        if messages_dir.is_dir() {
            interface.has_messages_dir = true;
            for path in sorted_entries(&messages_dir)? {
                let name = file_name(&path);
                if path.is_file() && is_json(&name) {
                    count()?;
                    interface.messages.insert(name, read_text(&path));
                }
            }
        }
        source.interfaces.insert(file_name(&dir), interface);
    }
    let vectors_dir = root.join("vectors");
    if vectors_dir.is_dir() {
        match sorted_entries(&vectors_dir) {
            Ok(paths) => {
                for path in paths {
                    let name = file_name(&path);
                    if path.is_file() && is_json(&name) {
                        count()?;
                        source.vectors.insert(name, read_text(&path));
                    }
                }
            }
            // Reported by `vector_files`, to whoever asks for the vectors: a
            // dictionary decodes without them.
            Err(LoadError::Io { source: error, .. }) => {
                source.vectors_error = Some((error.kind(), error.to_string()));
            }
            Err(other) => return Err(other),
        }
    }
    Ok(source)
}

/// Check and index a dictionary's files. `root` is where they came from, and
/// prefixes the file paths named in issues.
fn build(source: Source, root: PathBuf) -> Result<Dictionary, LoadError> {
    let mut issues = Vec::new();
    let spec = match &source.dictionary {
        None => None,
        Some(text) => match parse_json::<DictionarySpec>(text, &root.join("dictionary.json")) {
            Ok(spec) => Some(spec),
            Err(message) => {
                issues.push(Issue {
                    severity: Severity::Error,
                    location: "dictionary.json".to_string(),
                    message,
                });
                None
            }
        },
    }
    .unwrap_or_default();

    let mut interfaces = Vec::new();
    for (name, files) in &source.interfaces {
        let Some(metadata) = &files.metadata else {
            continue;
        };
        let dir = root.join(name);
        match parse_json::<InterfaceSpec>(metadata, &dir.join("metadata.json")) {
            Ok(spec) => interfaces.push(build_interface(
                name.clone(),
                spec,
                files,
                &dir,
                &mut issues,
            )),
            Err(message) => issues.push(Issue {
                severity: Severity::Error,
                location: name.clone(),
                message,
            }),
        }
    }
    if interfaces.is_empty() && issues.is_empty() {
        return Err(LoadError::Empty { path: root });
    }

    let mut by_name = HashMap::new();
    let mut by_port = HashMap::new();
    for (index, interface) in interfaces.iter().enumerate() {
        by_name.insert(interface.name.clone(), index);
        if let Some(port) = interface.port {
            if let Some(&other) = by_port.get(&port) {
                let other: &Interface = &interfaces[other];
                issues.push(Issue {
                    severity: Severity::Error,
                    location: interface.name.clone(),
                    message: format!(
                        "port {port} is already claimed by interface {}; packets on it go to {}",
                        other.name, other.name
                    ),
                });
            } else {
                by_port.insert(port, index);
            }
        }
    }

    let vectors = source
        .vectors
        .into_iter()
        .map(|(file, text)| VectorFile {
            name: stem(&file),
            text,
        })
        .collect();

    Ok(Dictionary {
        spec,
        root,
        interfaces,
        by_name,
        by_port,
        issues,
        vectors,
        vectors_error: source.vectors_error,
    })
}

fn build_interface(
    name: String,
    spec: InterfaceSpec,
    files: &InterfaceSource,
    dir: &Path,
    issues: &mut Vec<Issue>,
) -> Interface {
    let mut report = |severity, location: String, message: String| {
        issues.push(Issue {
            severity,
            location,
            message,
        })
    };

    let disc = &spec.message_type;
    if disc.source == DiscriminatorSource::Payload {
        if disc.stop <= disc.start || disc.stop - disc.start > 64 {
            report(
                Severity::Error,
                name.clone(),
                format!(
                    "message_type bits [{}, {}) must span 1 to 64 bits",
                    disc.start, disc.stop
                ),
            );
        } else if disc.stop > MAX_PAYLOAD_BITS {
            report(
                Severity::Error,
                name.clone(),
                format!(
                    "message_type bits [{}, {}) reach past the {MAX_PAYLOAD_BYTES} bytes of a \
                     payload that are considered",
                    disc.start, disc.stop
                ),
            );
        }
    }
    let framing = match spec.framing {
        Framing::Single => Framing::Single,
        Framing::Concatenated => {
            let problem = match spec.message_length {
                _ if disc.source != DiscriminatorSource::Payload => {
                    Some("needs message_type source payload".to_string())
                }
                None => Some("needs message_length".to_string()),
                Some(range)
                    if range.stop <= range.start
                        || range.stop - range.start > 64
                        || range.stop > MAX_PAYLOAD_BITS =>
                {
                    Some(format!(
                        "message_length bits [{}, {}) must span 1 to 64 bits within a payload",
                        range.start, range.stop
                    ))
                }
                Some(_) => None,
            };
            match problem {
                Some(problem) => {
                    report(
                        Severity::Error,
                        name.clone(),
                        format!(
                            "framing concatenated {problem}; every payload is read as one message"
                        ),
                    );
                    Framing::Single
                }
                None => Framing::Concatenated,
            }
        }
    };
    let mask = match disc.mask.as_deref() {
        None => None,
        Some(text) => match parse_int(text).and_then(|value| u64::try_from(value).ok()) {
            Some(value) => Some(value),
            None => {
                report(
                    Severity::Error,
                    name.clone(),
                    format!("message_type mask {text:?} is not an unsigned integer"),
                );
                None
            }
        },
    };
    let mut names = HashMap::new();
    let values = if disc.values.len() > MAX_VALUES {
        report(
            Severity::Error,
            name.clone(),
            format!(
                "message_type has {} values; at most {MAX_VALUES} are read, so none is and the \
                 interface identifies no message",
                disc.values.len()
            ),
        );
        None
    } else {
        Some(&disc.values)
    };
    for (key, message_name) in values.into_iter().flatten() {
        match parse_int(key).and_then(|value| u64::try_from(value).ok()) {
            Some(code) => {
                names.insert(code, message_name.clone());
            }
            None => report(
                Severity::Error,
                name.clone(),
                format!(
                    "message_type value {:?} is not an unsigned integer",
                    quoted(key)
                ),
            ),
        }
    }

    let mut messages = Vec::new();
    if files.has_messages_dir {
        let selectable: HashSet<&str> = names.values().map(String::as_str).collect();
        for (file, text) in &files.messages {
            let message_name = stem(file);
            let location = format!("{name}/{message_name}");
            match parse_json::<MessageSpec>(text, &dir.join("messages").join(file)) {
                Ok(message_spec) if message_spec.fields.len() > MAX_FIELDS_PER_MESSAGE => report(
                    Severity::Error,
                    location,
                    format!(
                        "{} fields; a message may have at most {MAX_FIELDS_PER_MESSAGE}",
                        message_spec.fields.len()
                    ),
                ),
                Ok(message_spec) => {
                    if !selectable.contains(message_name.as_str()) {
                        report(
                            Severity::Warning,
                            location.clone(),
                            "no message_type value maps to this message; it can never be selected"
                                .to_string(),
                        );
                    }
                    messages.push(build_message(
                        message_name,
                        message_spec,
                        &spec,
                        &location,
                        &mut report,
                    ));
                }
                Err(message) => report(Severity::Error, location, message),
            }
        }
    } else {
        report(
            Severity::Warning,
            name.clone(),
            "no messages directory".to_string(),
        );
    }

    let by_name: HashMap<String, usize> = messages
        .iter()
        .enumerate()
        .map(|(index, message)| (message.name.clone(), index))
        .collect();
    let mut undefined: Vec<&String> = names
        .values()
        .filter(|message_name| !by_name.contains_key(*message_name))
        .collect();
    undefined.sort();
    undefined.dedup();
    for message_name in undefined {
        report(
            Severity::Warning,
            format!("{name}/{message_name}"),
            "named by message_type but has no file in messages/; it is identified but not decoded"
                .to_string(),
        );
    }

    Interface {
        description: spec.mission.clone(),
        icd: spec.icd.clone(),
        protocol: spec.protocol.clone(),
        port: spec.port,
        bit_order: spec.bit_order,
        byte_order: spec.byte_order,
        discriminator: Discriminator {
            source: disc.source,
            start: disc.start,
            stop: disc.stop,
            mask,
            names,
        },
        message_length: spec.message_length,
        framing,
        messages,
        by_name,
        name,
    }
}

fn build_message(
    name: String,
    spec: MessageSpec,
    interface: &InterfaceSpec,
    location: &str,
    report: &mut impl FnMut(Severity, String, String),
) -> Message {
    let mut fields: Vec<Field> = Vec::new();
    for (field_name, field_spec) in spec.fields {
        let field_location = format!("{location}/{field_name}");
        match build_field(field_name, field_spec, interface) {
            Ok((field, warnings)) => {
                for warning in warnings {
                    report(Severity::Warning, field_location.clone(), warning);
                }
                fields.push(field);
            }
            Err(message) => report(Severity::Error, field_location, message),
        }
    }
    fields.sort_by(|a, b| (a.start, &a.name).cmp(&(b.start, &b.name)));
    for pair in fields.windows(2) {
        if pair[1].start < pair[0].stop {
            report(
                Severity::Warning,
                format!("{location}/{}", pair[1].name),
                format!(
                    "bits [{}, {}) overlap {} [{}, {})",
                    pair[1].start, pair[1].stop, pair[0].name, pair[0].start, pair[0].stop
                ),
            );
        }
    }
    Message {
        name,
        description: spec.description,
        fields,
    }
}

fn build_field(
    name: String,
    spec: FieldSpec,
    interface: &InterfaceSpec,
) -> Result<(Field, Vec<String>), String> {
    if spec.stop <= spec.start {
        return Err(format!(
            "bits [{}, {}) are empty or inverted",
            spec.start, spec.stop
        ));
    }
    if spec.stop > MAX_PAYLOAD_BITS {
        return Err(format!(
            "bits [{}, {}) reach past the {MAX_PAYLOAD_BYTES} bytes of a payload that are \
             considered",
            spec.start, spec.stop
        ));
    }
    let width = spec.stop - spec.start;
    let aligned = spec.start % 8 == 0 && spec.stop % 8 == 0;
    let kind = match (spec.kind, spec.signed) {
        (Some(kind), _) => kind,
        (None, Some(true)) => FieldKind::Int,
        // A byte-aligned field too wide for an integer is carried as bytes.
        (None, _) if width > 64 && aligned => FieldKind::Bytes,
        (None, _) => FieldKind::Uint,
    };
    match kind {
        FieldKind::Uint | FieldKind::Int | FieldKind::Bool if width > 64 => {
            return Err(format!(
                "{width} bits is wider than the 64 an integer field can hold"
            ));
        }
        FieldKind::Float if width != 32 && width != 64 => {
            return Err(format!("a float field is 32 or 64 bits, not {width}"));
        }
        FieldKind::Bytes | FieldKind::Ascii if !aligned => {
            return Err(
                "a bytes or ascii field must start and stop on a byte boundary".to_string(),
            );
        }
        _ => {}
    }
    let byte_order = spec.byte_order.unwrap_or(interface.byte_order);
    if byte_order == ByteOrder::Little
        && interface.bit_order == BitOrder::Msb0
        && !aligned
        && !matches!(kind, FieldKind::Bytes | FieldKind::Ascii)
    {
        return Err(
            "little-endian byte order needs a byte-aligned field; use bit_order lsb0 for \
             little-endian bit fields"
                .to_string(),
        );
    }

    let mut warnings = Vec::new();
    let mut labels = HashMap::new();
    let values = spec.values.unwrap_or_default();
    if values.len() > MAX_VALUES {
        return Err(format!(
            "{} values; a field may have at most {MAX_VALUES}",
            values.len()
        ));
    }
    for (key, value) in values {
        let label = match value {
            serde_json::Value::String(text) => text,
            serde_json::Value::Bool(flag) => flag.to_string(),
            serde_json::Value::Number(number) => number.to_string(),
            other => {
                warnings.push(format!(
                    "value {:?} has a label that is not text: {}",
                    quoted(&key),
                    quoted(&other.to_string())
                ));
                continue;
            }
        };
        match parse_int(&key) {
            Some(raw) => {
                labels.insert(raw, label);
            }
            None => warnings.push(format!("value key {:?} is not an integer", quoted(&key))),
        }
    }
    if !labels.is_empty() && matches!(kind, FieldKind::Float | FieldKind::Bytes | FieldKind::Ascii)
    {
        warnings.push("values are ignored on float, bytes and ascii fields".to_string());
        labels.clear();
    }

    Ok((
        Field {
            name,
            start: spec.start,
            stop: spec.stop,
            kind,
            bit_order: interface.bit_order,
            byte_order,
            scale: spec.scale,
            offset: spec.offset,
            unit: spec.unit,
            description: spec.description,
            labels,
            decimals: (spec.scale.is_some() || spec.offset.is_some())
                .then(|| decimal_places(spec.scale, spec.offset))
                .flatten(),
        },
        warnings,
    ))
}

fn sorted_entries(dir: &Path) -> Result<Vec<PathBuf>, LoadError> {
    let io = |source| LoadError::Io {
        path: dir.to_path_buf(),
        source,
    };
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir).map_err(io)? {
        // A dictionary holds at most MAX_FILES files, so no directory of one
        // is listed past that many entries.
        if paths.len() >= MAX_FILES {
            return Err(LoadError::TooManyFiles {
                path: dir.to_path_buf(),
                limit: MAX_FILES,
            });
        }
        paths.push(entry.map_err(io)?.path());
    }
    paths.sort();
    Ok(paths)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Whether a file name has the extension of a dictionary file.
fn is_json(file: &str) -> bool {
    Path::new(file).extension().and_then(|e| e.to_str()) == Some("json")
}

fn stem(file: &str) -> String {
    Path::new(file)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Why a file of `bytes` bytes is not read.
fn too_large(bytes: u64) -> String {
    format!("the file is {bytes} bytes; the largest dictionary file read is {MAX_FILE_BYTES}")
}

/// The text of the file at `path`, when it is no larger than
/// [`MAX_FILE_BYTES`]. The read itself stops at the limit, so a file that
/// grows, or one whose size the filesystem does not report, cannot exceed it.
fn read_text(path: &Path) -> FileText {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    if size > MAX_FILE_BYTES as u64 {
        return Err(too_large(size));
    }
    let mut text = String::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() > MAX_FILE_BYTES {
        return Err(too_large(text.len() as u64));
    }
    Ok(text)
}

/// Parse the text of the file at `path`, which is named in the error.
fn parse_json<T: serde::de::DeserializeOwned>(text: &FileText, path: &Path) -> Result<T, String> {
    let text = text
        .as_ref()
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(text).map_err(|e| format!("{}: {e}", path.display()))
}
