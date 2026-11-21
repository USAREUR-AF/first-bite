//! A dictionary as four flat tables, for transcription in a spreadsheet.
//!
//! | File | One row per |
//! |---|---|
//! | `interfaces.csv` | interface: everything in `metadata.json` except the message names |
//! | `messages.csv` | message: its identifier, whether it is defined, its description, length and source |
//! | `fields.csv` | field |
//! | `values.csv` | enumerated value of a field |
//!
//! [`export`] then [`import`] gives back the dictionary's interface files
//! byte for byte once they are in canonical form. What the tables cannot
//! hold, a key they have no column for, is an error on export and never
//! dropped.

use super::canon::{value_key, FIELD_KEYS};
use super::csv::{self, Row, Table};
use super::json::{number_text, Node};
use super::model::{fields, InterfaceTree, Tree, SOURCE_KEYS};
use super::range::{bit_order_name, IcdRange, Numbering};
use crate::schema::parse_int;
use std::collections::BTreeMap;

pub const INTERFACE_COLUMNS: &[&str] = &[
    "interface",
    "description",
    "port",
    "network",
    "transport",
    "bit_order",
    "byte_order",
    "type_source",
    "type_start",
    "type_stop",
    "type_mask",
    "length_start",
    "length_stop",
    "framing",
    "icd_version",
    "icd_date",
    "word_bits",
    "bit0",
    "first_word",
    "first_bit",
    "word_order",
    "source_document",
    "source_revision",
    "source_section",
    "source_table",
    "source_page",
    "note",
];

pub const MESSAGE_COLUMNS: &[&str] = &[
    "interface",
    "message",
    "type_id",
    "defined",
    "description",
    "declared_bits",
    "source_document",
    "source_revision",
    "source_section",
    "source_table",
    "source_page",
    "note",
];

pub const FIELD_COLUMNS: &[&str] = &[
    "interface",
    "message",
    "field",
    "start",
    "stop",
    "word",
    "bits",
    "type",
    "signed",
    "byte_order",
    "scale",
    "offset",
    "unit",
    "description",
    "source_document",
    "source_revision",
    "source_section",
    "source_table",
    "source_page",
    "note",
    "review",
];

pub const VALUE_COLUMNS: &[&str] = &["interface", "message", "field", "value", "label"];

/// The four tables as CSV text.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tables {
    pub interfaces: String,
    pub messages: String,
    pub fields: String,
    /// May be empty: a dictionary with no enumerated values.
    pub values: String,
}

pub const INTERFACES_FILE: &str = "interfaces.csv";
pub const MESSAGES_FILE: &str = "messages.csv";
pub const FIELDS_FILE: &str = "fields.csv";
pub const VALUES_FILE: &str = "values.csv";

/// Reads the keys of one JSON object into cells and remembers which keys were
/// read, so that a key with no column can be reported.
struct Reader<'a> {
    node: &'a Node,
    at: String,
    read: Vec<&'static str>,
}

impl<'a> Reader<'a> {
    fn new(node: &'a Node, at: impl Into<String>) -> Self {
        Reader {
            node,
            at: at.into(),
            read: Vec::new(),
        }
    }

    fn text(&mut self, key: &'static str, errors: &mut Vec<String>) -> String {
        self.read.push(key);
        match self.node.get(key) {
            None => String::new(),
            Some(Node::String(text)) if !text.is_empty() && text.trim() == text => text.clone(),
            Some(Node::String(text)) => {
                errors.push(format!(
                    "{}: {key} is {text:?}: empty text and text with space at an end do not survive a table",
                    self.at
                ));
                String::new()
            }
            Some(other) => {
                errors.push(format!(
                    "{}: {key} is {}, not text",
                    self.at,
                    other.to_line()
                ));
                String::new()
            }
        }
    }

    fn number(&mut self, key: &'static str, errors: &mut Vec<String>) -> String {
        self.read.push(key);
        match self.node.get(key) {
            None => String::new(),
            Some(Node::Number(number)) => number_text(number),
            Some(other) => {
                errors.push(format!(
                    "{}: {key} is {}, not a number",
                    self.at,
                    other.to_line()
                ));
                String::new()
            }
        }
    }

    fn flag(&mut self, key: &'static str, errors: &mut Vec<String>) -> String {
        self.read.push(key);
        match self.node.get(key) {
            None => String::new(),
            Some(Node::Bool(flag)) => flag.to_string(),
            Some(other) => {
                errors.push(format!(
                    "{}: {key} is {}, not true or false",
                    self.at,
                    other.to_line()
                ));
                String::new()
            }
        }
    }

    /// A nested object: its reader, or one over nothing when the key is absent.
    fn object(&mut self, key: &'static str, errors: &mut Vec<String>) -> Reader<'a> {
        const EMPTY: &Node = &Node::Null;
        self.read.push(key);
        let node = match self.node.get(key) {
            None => EMPTY,
            Some(node) if node.is_object() => node,
            Some(other) => {
                errors.push(format!(
                    "{}: {key} is {}, not an object",
                    self.at,
                    other.to_line()
                ));
                EMPTY
            }
        };
        Reader::new(node, format!("{}: {key}", self.at))
    }

    /// The five `source_*` cells.
    fn source(&mut self, errors: &mut Vec<String>) -> Vec<String> {
        let mut source = self.object("source", errors);
        let cells = SOURCE_KEYS
            .iter()
            .map(|key| {
                source.read.push(*key);
                match source.node.get(key) {
                    None => String::new(),
                    Some(Node::Number(number)) => number_text(number),
                    Some(Node::String(text)) if !text.is_empty() && text.trim() == text => {
                        text.clone()
                    }
                    Some(other) => {
                        errors.push(format!(
                            "{}: {key} is {}, not text",
                            source.at,
                            other.to_line()
                        ));
                        String::new()
                    }
                }
            })
            .collect();
        source.finish(errors);
        cells
    }

    /// Report every key that was not read.
    fn finish(self, errors: &mut Vec<String>) {
        for (key, _) in self.node.entries() {
            if !self.read.contains(&key.as_str()) {
                errors.push(format!(
                    "{}: key {key:?} has no column in the tables",
                    self.at
                ));
            }
        }
    }
}

/// The numeric order of a message in its interface: its lowest identifier.
fn lowest_id(ids: &[&str]) -> i128 {
    ids.iter()
        .filter_map(|id| parse_int(id))
        .min()
        .unwrap_or(i128::MAX)
}

/// Write a dictionary's interfaces as the four tables.
pub fn export(tree: &Tree) -> Result<Tables, Vec<String>> {
    let mut errors = Vec::new();
    let mut interfaces: Vec<Vec<String>> = vec![header(INTERFACE_COLUMNS)];
    let mut messages: Vec<Vec<String>> = vec![header(MESSAGE_COLUMNS)];
    let mut field_rows: Vec<Vec<String>> = vec![header(FIELD_COLUMNS)];
    let mut values: Vec<Vec<String>> = vec![header(VALUE_COLUMNS)];

    for interface in &tree.interfaces {
        let name = &interface.name;
        let errors = &mut errors;
        let mut meta = Reader::new(&interface.metadata, format!("{name}/metadata.json"));
        if meta.node.get("mission").is_some() {
            errors.push(format!(
                "{}: mission is the older name of description; run bite fmt first",
                meta.at
            ));
        }
        let mut row = vec![
            name.clone(),
            meta.text("description", errors),
            meta.number("port", errors),
        ];
        let mut protocol = meta.object("protocol", errors);
        row.push(protocol.text("network", errors));
        row.push(protocol.text("transport", errors));
        protocol.finish(errors);
        row.push(meta.text("bit_order", errors));
        row.push(meta.text("byte_order", errors));
        let mut kind = meta.object("message_type", errors);
        row.push(kind.text("source", errors));
        row.push(kind.number("start", errors));
        row.push(kind.number("stop", errors));
        row.push(kind.text("mask", errors));
        kind.read.push("values");
        kind.finish(errors);
        let mut length = meta.object("message_length", errors);
        row.push(length.number("start", errors));
        row.push(length.number("stop", errors));
        length.finish(errors);
        row.push(meta.text("framing", errors));
        let mut icd = meta.object("icd", errors);
        row.push(icd.text("version", errors));
        row.push(icd.text("date", errors));
        icd.finish(errors);
        let mut numbering = meta.object("icd_numbering", errors);
        row.push(numbering.number("word_bits", errors));
        row.push(numbering.text("bit0", errors));
        row.push(numbering.number("first_word", errors));
        row.push(numbering.number("first_bit", errors));
        row.push(numbering.text("word_order", errors));
        numbering.finish(errors);
        row.extend(meta.source(errors));
        row.push(meta.text("note", errors));
        meta.finish(errors);
        interfaces.push(row);

        let numbering = interface
            .metadata
            .get("icd_numbering")
            .and_then(|node| Numbering::read(node).ok());

        // Every message: those with a file, and those only named.
        let mut ids: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (id, message) in interface.type_values() {
            ids.entry(message).or_default().push(id);
        }
        for (message, _) in &interface.messages {
            ids.entry(message.as_str()).or_default();
        }
        let mut order: Vec<(&str, Vec<&str>)> = ids.into_iter().collect();
        order.sort_by_key(|(message, ids)| (lowest_id(ids), *message));

        for (message_name, ids) in order {
            let at = format!("{name}/messages/{message_name}.json");
            let mut row = vec![name.clone(), message_name.to_string(), ids.join(" ")];
            let Some(message) = interface.message(message_name) else {
                row.push("no".to_string());
                row.resize(MESSAGE_COLUMNS.len(), String::new());
                messages.push(row);
                continue;
            };
            let mut reader = Reader::new(message, at.clone());
            row.push("yes".to_string());
            row.push(reader.text("description", errors));
            row.push(reader.number("declared_bits", errors));
            row.extend(reader.source(errors));
            row.push(reader.text("note", errors));
            reader.read.push("fields");
            reader.finish(errors);
            messages.push(row);

            for view in fields(message) {
                let at = format!("{at}: {}", view.name);
                let mut reader = Reader::new(view.node, at.clone());
                let mut row = vec![
                    name.clone(),
                    message_name.to_string(),
                    view.name.to_string(),
                    reader.number("start", errors),
                    reader.number("stop", errors),
                ];
                let icd = view
                    .range()
                    .zip(numbering)
                    .and_then(|((start, stop), numbering)| numbering.to_icd(start, stop).ok());
                row.push(icd.map(|range| range.word_text()).unwrap_or_default());
                row.push(icd.map(|range| range.bits_text()).unwrap_or_default());
                row.push(reader.text("type", errors));
                row.push(reader.flag("signed", errors));
                row.push(reader.text("byte_order", errors));
                row.push(reader.number("scale", errors));
                row.push(reader.number("offset", errors));
                row.push(reader.text("unit", errors));
                row.push(reader.text("description", errors));
                row.extend(reader.source(errors));
                row.push(reader.text("note", errors));
                row.push(String::new());
                reader.read.push("values");
                debug_assert!(FIELD_KEYS.iter().all(|key| reader.read.contains(key)));
                reader.finish(errors);
                field_rows.push(row);

                match view.node.get("values") {
                    None => {}
                    Some(node) if node.is_object() => {
                        for (key, label) in node.entries() {
                            let label = match label {
                                Node::String(text) => text.clone(),
                                Node::Bool(flag) => flag.to_string(),
                                Node::Number(number) => number_text(number),
                                other => {
                                    errors.push(format!(
                                        "{at}: the label of {key} is {}, not text",
                                        other.to_line()
                                    ));
                                    continue;
                                }
                            };
                            values.push(vec![
                                name.clone(),
                                message_name.to_string(),
                                view.name.to_string(),
                                key.clone(),
                                label,
                            ]);
                        }
                    }
                    Some(other) => errors.push(format!(
                        "{at}: values is {}, not an object",
                        other.to_line()
                    )),
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(Tables {
            interfaces: csv::write(interfaces),
            messages: csv::write(messages),
            fields: csv::write(field_rows),
            values: csv::write(values),
        })
    } else {
        Err(errors)
    }
}

fn header(columns: &[&str]) -> Vec<String> {
    columns.iter().map(|column| column.to_string()).collect()
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ImportOptions {
    /// Accept rows of `fields.csv` whose `review` cell is `unreviewed`.
    pub allow_unreviewed: bool,
}

/// A number cell as a JSON number.
fn number(text: &str) -> Option<Node> {
    if let Ok(value) = text.parse::<u64>() {
        return Some(Node::from(value));
    }
    let value = text.parse::<f64>().ok()?;
    value.is_finite().then(|| Node::number(value))
}

fn whole(text: &str) -> Option<Node> {
    text.parse::<u64>().ok().map(Node::from)
}

/// Collects the keys of one JSON object from the cells of a row.
struct Writer<'a> {
    table: &'a Table,
    row: &'a Row,
    out: Vec<(String, Node)>,
}

impl<'a> Writer<'a> {
    fn new(table: &'a Table, row: &'a Row) -> Self {
        Writer {
            table,
            row,
            out: Vec::new(),
        }
    }

    fn text(&mut self, column: &str, key: &str) {
        let cell = self.table.cell(self.row, column);
        if !cell.is_empty() {
            self.out.push((key.to_string(), Node::from(cell)));
        }
    }

    fn parsed(
        &mut self,
        column: &str,
        key: &str,
        parse: fn(&str) -> Option<Node>,
        what: &str,
        errors: &mut Vec<String>,
    ) {
        let cell = self.table.cell(self.row, column);
        if cell.is_empty() {
            return;
        }
        match parse(cell) {
            Some(node) => self.out.push((key.to_string(), node)),
            None => errors.push(format!(
                "{}: {column} {cell:?} is not {what}",
                self.table.at(self.row)
            )),
        }
    }

    fn one_of(&mut self, column: &str, key: &str, allowed: &[&str], errors: &mut Vec<String>) {
        let cell = self.table.cell(self.row, column);
        if cell.is_empty() {
            return;
        }
        if allowed.contains(&cell) {
            self.out.push((key.to_string(), Node::from(cell)));
        } else {
            errors.push(format!(
                "{}: {column} {cell:?} is not one of {}",
                self.table.at(self.row),
                allowed.join(", ")
            ));
        }
    }

    /// The `source` object from the five `source_*` cells.
    fn source(&mut self) {
        let members: Vec<(String, Node)> = SOURCE_KEYS
            .iter()
            .filter_map(|key| {
                let cell = self.table.cell(self.row, &format!("source_{key}"));
                (!cell.is_empty()).then(|| (key.to_string(), Node::from(cell)))
            })
            .collect();
        self.object("source", members);
    }

    fn object(&mut self, key: &str, members: Vec<(String, Node)>) {
        if !members.is_empty() {
            self.out.push((key.to_string(), Node::Object(members)));
        }
    }

    fn take(&mut self) -> Vec<(String, Node)> {
        std::mem::take(&mut self.out)
    }
}

fn flag(text: &str) -> Option<Node> {
    match text {
        "true" => Some(Node::Bool(true)),
        "false" => Some(Node::Bool(false)),
        _ => None,
    }
}

struct MessageDraft {
    /// The row of `messages.csv`, for messages.
    at: String,
    defined: bool,
    head: Vec<(String, Node)>,
    fields: Vec<(String, Node)>,
}

struct InterfaceDraft {
    metadata: Vec<(String, Node)>,
    message_type: Vec<(String, Node)>,
    type_values: Vec<(i128, String, String)>,
    numbering: Option<Numbering>,
    bit_order: String,
    messages: Vec<(String, MessageDraft)>,
}

/// Build interface files from the four tables. Every problem is reported
/// with its file and row; nothing is built if there is any.
pub fn import(tables: &Tables, options: ImportOptions) -> Result<Vec<InterfaceTree>, Vec<String>> {
    let mut errors: Vec<String> = Vec::new();
    let mut read = |name: &str, text: &str, columns: &'static [&'static str], required: &[&str]| {
        match Table::read(name, text, columns, required) {
            Ok(table) => Some(table),
            Err(found) => {
                errors.extend(found);
                None
            }
        }
    };
    let interfaces = read(
        INTERFACES_FILE,
        &tables.interfaces,
        INTERFACE_COLUMNS,
        &["interface"],
    );
    let messages = read(
        MESSAGES_FILE,
        &tables.messages,
        MESSAGE_COLUMNS,
        &["interface", "message", "type_id", "defined"],
    );
    let field_table = read(
        FIELDS_FILE,
        &tables.fields,
        FIELD_COLUMNS,
        &["interface", "message", "field"],
    );
    let values = if tables.values.trim().is_empty() {
        None
    } else {
        read(VALUES_FILE, &tables.values, VALUE_COLUMNS, VALUE_COLUMNS)
    };
    let (Some(interfaces), Some(messages), Some(field_table)) = (interfaces, messages, field_table)
    else {
        return Err(errors);
    };
    if !tables.values.trim().is_empty() && values.is_none() {
        return Err(errors);
    }

    let mut drafts: Vec<(String, InterfaceDraft)> = Vec::new();
    for row in &interfaces.rows {
        let at = interfaces.at(row);
        let name = interfaces.cell(row, "interface");
        if name.is_empty() {
            errors.push(format!("{at}: no interface name"));
            continue;
        }
        if name.contains(['/', '\\']) || name.starts_with('.') || name == "vectors" {
            errors.push(format!(
                "{at}: interface name {name:?} cannot be a directory of the dictionary"
            ));
            continue;
        }
        if drafts.iter().any(|(known, _)| known == name) {
            errors.push(format!("{at}: interface {name:?} has a row already"));
            continue;
        }
        let errors = &mut errors;
        let mut w = Writer::new(&interfaces, row);
        w.text("description", "description");
        w.parsed("port", "port", whole, "a whole number", errors);
        let mut metadata = w.take();
        w.text("network", "network");
        w.text("transport", "transport");
        let protocol = w.take();
        if !protocol.is_empty() {
            metadata.push(("protocol".to_string(), Node::Object(protocol)));
        }
        w.one_of("bit_order", "bit_order", &["msb0", "lsb0"], errors);
        w.one_of("byte_order", "byte_order", &["big", "little"], errors);
        metadata.extend(w.take());

        w.one_of("type_source", "source", &["payload", "frame_id"], errors);
        w.parsed("type_start", "start", whole, "a whole number", errors);
        w.parsed("type_stop", "stop", whole, "a whole number", errors);
        w.text("type_mask", "mask");
        let message_type = w.take();

        w.parsed("length_start", "start", whole, "a whole number", errors);
        w.parsed("length_stop", "stop", whole, "a whole number", errors);
        let length = w.take();
        if length.len() == 1 {
            errors.push(format!(
                "{at}: give both length_start and length_stop, or neither"
            ));
        }
        w.one_of("framing", "framing", &["single", "concatenated"], errors);
        let framing = w.take();

        w.text("icd_version", "version");
        w.text("icd_date", "date");
        let icd = w.take();
        if !icd.is_empty() && icd[0].0 != "version" {
            errors.push(format!("{at}: icd_date needs icd_version"));
        }

        w.parsed("word_bits", "word_bits", whole, "a whole number", errors);
        w.one_of("bit0", "bit0", &["msb", "lsb"], errors);
        w.parsed("first_word", "first_word", whole, "a whole number", errors);
        w.parsed("first_bit", "first_bit", whole, "a whole number", errors);
        w.one_of("word_order", "word_order", &["big", "little"], errors);
        let numbering_members = w.take();
        let bit_order = interfaces.cell(row, "bit_order");
        let bit_order = if bit_order.is_empty() {
            "msb0"
        } else {
            bit_order
        };
        let numbering = if numbering_members.is_empty() {
            None
        } else {
            match Numbering::read(&Node::Object(numbering_members.clone())) {
                Ok(numbering) if bit_order_name(numbering.bit_order()) != bit_order => {
                    errors.push(format!(
                        "{at}: this word_order gives ranges under bit_order {}, and the interface is {bit_order}",
                        bit_order_name(numbering.bit_order()),
                    ));
                    None
                }
                Ok(numbering) => Some(numbering),
                Err(message) => {
                    errors.push(format!("{at}: {message}"));
                    None
                }
            }
        };

        w.object("icd", icd);
        w.source();
        w.text("note", "note");
        metadata.extend(w.take());
        if !numbering_members.is_empty() {
            metadata.push(("icd_numbering".to_string(), Node::Object(numbering_members)));
        }
        if !length.is_empty() {
            metadata.push(("message_length".to_string(), Node::Object(length)));
        }
        metadata.extend(framing);
        drafts.push((
            name.to_string(),
            InterfaceDraft {
                metadata,
                message_type,
                type_values: Vec::new(),
                numbering,
                bit_order: bit_order.to_string(),
                messages: Vec::new(),
            },
        ));
    }

    for row in &messages.rows {
        let at = messages.at(row);
        let interface = messages.cell(row, "interface");
        let name = messages.cell(row, "message");
        let Some((_, draft)) = drafts.iter_mut().find(|(known, _)| known == interface) else {
            errors.push(format!(
                "{at}: interface {interface:?} has no row in {INTERFACES_FILE}"
            ));
            continue;
        };
        if name.is_empty() {
            errors.push(format!("{at}: no message name"));
            continue;
        }
        if name.contains(['/', '\\']) || name.starts_with('.') {
            errors.push(format!("{at}: message name {name:?} cannot be a file name"));
            continue;
        }
        if draft.messages.iter().any(|(known, _)| known == name) {
            errors.push(format!("{at}: message {name:?} has a row already"));
            continue;
        }
        for id in messages.cell(row, "type_id").split_whitespace() {
            match parse_int(id) {
                None => errors.push(format!("{at}: type_id {id:?} is not a whole number")),
                Some(number) if number < 0 => {
                    errors.push(format!("{at}: type_id {id:?} is negative"))
                }
                Some(number) => {
                    match draft
                        .type_values
                        .iter()
                        .find(|(known, _, _)| *known == number)
                    {
                        Some((_, _, other)) => {
                            errors.push(format!("{at}: type_id {id} already identifies {other:?}"))
                        }
                        None => draft
                            .type_values
                            .push((number, value_key(id), name.to_string())),
                    }
                }
            }
        }
        let defined = match messages.cell(row, "defined") {
            "yes" => true,
            "no" => false,
            other => {
                errors.push(format!("{at}: defined is \"yes\" or \"no\", not {other:?}"));
                continue;
            }
        };
        let errors = &mut errors;
        let mut w = Writer::new(&messages, row);
        w.text("description", "description");
        w.parsed(
            "declared_bits",
            "declared_bits",
            whole,
            "a whole number",
            errors,
        );
        w.source();
        w.text("note", "note");
        let head = w.take();
        if !defined && !head.is_empty() {
            errors.push(format!(
                "{at}: a message that is not defined has no file to hold {}",
                head.iter()
                    .map(|(key, _)| key.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        draft.messages.push((
            name.to_string(),
            MessageDraft {
                at,
                defined,
                head,
                fields: Vec::new(),
            },
        ));
    }

    // Every field that has a row, usable or not: a value of a field whose row
    // was refused is not reported a second time.
    let mut seen: Vec<(&str, &str, &str)> = Vec::new();
    for row in &field_table.rows {
        let at = field_table.at(row);
        let interface = field_table.cell(row, "interface");
        let message = field_table.cell(row, "message");
        let name = field_table.cell(row, "field");
        seen.push((interface, message, name));
        let Some((_, draft)) = drafts.iter_mut().find(|(known, _)| known == interface) else {
            errors.push(format!(
                "{at}: interface {interface:?} has no row in {INTERFACES_FILE}"
            ));
            continue;
        };
        let numbering = draft.numbering;
        let bit_order = draft.bit_order.clone();
        let Some((_, target)) = draft
            .messages
            .iter_mut()
            .find(|(known, _)| known == message)
        else {
            errors.push(format!(
                "{at}: message {message:?} of {interface} has no row in {MESSAGES_FILE}"
            ));
            continue;
        };
        if !target.defined {
            errors.push(format!(
                "{at}: message {message:?} is marked not defined in {MESSAGES_FILE} ({}) and has a field",
                target.at
            ));
            continue;
        }
        if name.is_empty() {
            errors.push(format!("{at}: no field name"));
            continue;
        }
        if target.fields.iter().any(|(known, _)| known == name) {
            errors.push(format!(
                "{at}: field {name:?} of {interface}/{message} has a row already"
            ));
            continue;
        }
        match field_table.cell(row, "review") {
            "" | "reviewed" => {}
            "unreviewed" if options.allow_unreviewed => {}
            "unreviewed" => {
                errors.push(format!(
                    "{at}: the row is marked unreviewed; check it against the document and clear the mark"
                ));
                continue;
            }
            other => {
                errors.push(format!(
                    "{at}: review is empty, \"reviewed\" or \"unreviewed\", not {other:?}"
                ));
                continue;
            }
        }

        let cell = |column: &str| field_table.cell(row, column);
        let given = match (cell("start"), cell("stop")) {
            ("", "") => None,
            (start, stop) => match (start.parse::<u64>(), stop.parse::<u64>()) {
                (Ok(start), Ok(stop)) => Some((start, stop)),
                _ => {
                    errors.push(format!(
                        "{at}: start {start:?} and stop {stop:?} are not two whole numbers"
                    ));
                    continue;
                }
            },
        };
        let converted = match (cell("word"), cell("bits")) {
            ("", "") => None,
            ("", _) | (_, "") => {
                errors.push(format!("{at}: give both word and bits, or neither"));
                continue;
            }
            (word, bits) => {
                let Some(numbering) = numbering else {
                    errors.push(format!(
                        "{at}: word and bits need the numbering of {interface} (word_bits, bit0, first_word) in {INTERFACES_FILE}"
                    ));
                    continue;
                };
                match IcdRange::parse(word, bits).and_then(|range| numbering.to_span(&range)) {
                    Ok(span) => Some((span.start, span.stop)),
                    Err(message) => {
                        errors.push(format!("{at}: {message}"));
                        continue;
                    }
                }
            }
        };
        let (start, stop) = match (given, converted) {
            (None, None) => {
                errors.push(format!("{at}: no start and stop, and no word and bits"));
                continue;
            }
            (Some(range), None) | (None, Some(range)) => range,
            (Some(given), Some(converted)) if given == converted => given,
            (Some(given), Some(converted)) => {
                errors.push(format!(
                    "{at}: start {} and stop {} disagree with word {} bits {}, which is [{}, {}) under {bit_order}",
                    given.0,
                    given.1,
                    cell("word"),
                    cell("bits"),
                    converted.0,
                    converted.1
                ));
                continue;
            }
        };

        let errors = &mut errors;
        let mut w = Writer::new(&field_table, row);
        w.out.push(("start".to_string(), Node::from(start)));
        w.out.push(("stop".to_string(), Node::from(stop)));
        w.one_of(
            "type",
            "type",
            &["uint", "int", "float", "bool", "bytes", "ascii"],
            errors,
        );
        w.parsed("signed", "signed", flag, "true or false", errors);
        w.one_of("byte_order", "byte_order", &["big", "little"], errors);
        w.parsed("scale", "scale", number, "a number", errors);
        w.parsed("offset", "offset", number, "a number", errors);
        w.text("unit", "unit");
        w.text("description", "description");
        w.source();
        w.text("note", "note");
        target
            .fields
            .push((name.to_string(), Node::Object(w.take())));
    }

    // Enumerated values, in the order of the file; `canonical` sorts them.
    // Interface, message and field to the field's values: number, key, label.
    type Labels = BTreeMap<(String, String, String), Vec<(i128, String, Node)>>;
    let mut labels: Labels = BTreeMap::new();
    if let Some(values) = &values {
        for row in &values.rows {
            let at = values.at(row);
            let interface = values.cell(row, "interface");
            let message = values.cell(row, "message");
            let field = values.cell(row, "field");
            let exists = drafts
                .iter()
                .find(|(known, _)| known == interface)
                .and_then(|(_, draft)| draft.messages.iter().find(|(known, _)| known == message))
                .is_some_and(|(_, target)| target.fields.iter().any(|(known, _)| known == field));
            if !exists && seen.contains(&(interface, message, field)) {
                continue;
            }
            if !exists {
                errors.push(format!(
                    "{at}: field {field:?} of {interface}/{message} has no row in {FIELDS_FILE}"
                ));
                continue;
            }
            let key = values.cell(row, "value");
            let Some(number) = parse_int(key) else {
                errors.push(format!("{at}: value {key:?} is not a whole number"));
                continue;
            };
            // The label keeps the spaces it was written with.
            let label = values.raw(row, "label");
            let entry = labels
                .entry((
                    interface.to_string(),
                    message.to_string(),
                    field.to_string(),
                ))
                .or_default();
            if entry.iter().any(|(known, _, _)| *known == number) {
                errors.push(format!(
                    "{at}: value {key} of {interface}/{message}/{field} has a row already"
                ));
                continue;
            }
            entry.push((number, value_key(key), Node::from(label)));
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    let mut out = Vec::new();
    for (name, mut draft) in drafts {
        draft.type_values.sort();
        let mut message_type = draft.message_type;
        message_type.push((
            "values".to_string(),
            Node::Object(
                draft
                    .type_values
                    .into_iter()
                    .map(|(_, key, message)| (key, Node::from(message)))
                    .collect(),
            ),
        ));
        let mut metadata = draft.metadata;
        metadata.push(("message_type".to_string(), Node::Object(message_type)));
        let mut files = Vec::new();
        for (message, target) in draft.messages {
            if !target.defined {
                continue;
            }
            let mut field_nodes = Vec::new();
            for (field, mut node) in target.fields {
                if let Some(mut entries) =
                    labels.remove(&(name.clone(), message.clone(), field.clone()))
                {
                    entries.sort_by_key(|(number, _, _)| *number);
                    node.set(
                        "values",
                        Node::Object(
                            entries
                                .into_iter()
                                .map(|(_, key, label)| (key, label))
                                .collect(),
                        ),
                    );
                }
                field_nodes.push((field, node));
            }
            let mut head = target.head;
            head.push(("fields".to_string(), Node::Object(field_nodes)));
            files.push((message, Node::Object(head)));
        }
        out.push(InterfaceTree {
            name,
            metadata: Node::Object(metadata),
            messages: files,
        });
    }
    Ok(out)
}
