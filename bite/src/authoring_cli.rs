//! The `bite` subcommands for writing and maintaining a dictionary.

use bite::authoring::canon;
use bite::authoring::coverage::coverage;
use bite::authoring::diff::diff;
use bite::authoring::encode::encode;
use bite::authoring::lint::lint;
use bite::authoring::model::{read_dir, write_dir, FileSet, Tree};
use bite::authoring::range::{
    bit_order_name, parse_bit0, parse_word_order, IcdRange, Numbering, Span,
};
use bite::authoring::rules::{self, Finding, LintConfig, RULES, UNITS};
use bite::authoring::scaffold::{new_dictionary, new_vector};
use bite::authoring::stats::{parse_payloads, stats};
use bite::authoring::table::{
    self, ImportOptions, Tables, FIELDS_FILE, INTERFACES_FILE, MESSAGES_FILE, VALUES_FILE,
};
use bite::authoring::Node;
use bite::{BitOrder, Dictionary};
use clap::{Args, Subcommand};
use serde_json::json;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Subcommand)]
pub enum Command {
    /// Start a dictionary: dictionary.json, one metadata.json per interface, a README.
    New {
        /// Directory to create.
        dir: PathBuf,
        /// Dictionary name; the directory name without it.
        #[arg(long)]
        name: Option<String>,
        /// Interface to create; repeat for more.
        #[arg(long, required = true)]
        interface: Vec<String>,
        /// Port of the interface, in the order the interfaces are given.
        #[arg(long)]
        port: Vec<u16>,
        /// Make <DIR> a dictionary repository (Makefile, CI workflow,
        /// pull-request checklist, issues register) with the dictionary in
        /// <DIR>/dictionary.
        #[arg(long)]
        repo: bool,
    },
    /// Convert between a document's word-and-bit addressing and [start, stop).
    Range(RangeArgs),
    /// Build a dictionary's interface files from interfaces.csv, messages.csv, fields.csv and values.csv.
    ImportCsv {
        /// Directory holding the tables.
        tables: PathBuf,
        /// Dictionary directory to write into.
        #[arg(long)]
        into: PathBuf,
        /// Accept rows of fields.csv marked "unreviewed".
        #[arg(long)]
        allow_unreviewed: bool,
        /// Delete interface and message files the tables do not hold.
        #[arg(long)]
        prune: bool,
    },
    /// Write a dictionary's interfaces as interfaces.csv, messages.csv, fields.csv and values.csv.
    ExportCsv {
        dictionary: PathBuf,
        /// Directory to write the tables into.
        #[arg(long)]
        out: PathBuf,
    },
    /// Rewrite a dictionary's files in canonical form.
    Fmt {
        dictionary: PathBuf,
        /// Change nothing; list the files that are not canonical and fail if there are any.
        #[arg(long)]
        check: bool,
    },
    /// Report the bits each message covers, undefined messages, and fields without source, description or unit.
    Coverage(CoverageArgs),
    /// Check a dictionary against the conventions.
    Lint {
        #[arg(required_unless_present_any = ["rules", "units"])]
        dictionary: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        /// Print every rule and stop.
        #[arg(long)]
        rules: bool,
        /// Print the unit vocabulary and stop.
        #[arg(long)]
        units: bool,
    },
    /// Compare two versions of a dictionary. Fails when a change breaks consumers of the decoded records.
    ///
    /// Exit status 0: every change is backward compatible. 1: at least one is
    /// breaking. With --check-version the status is about the version number
    /// instead: 0 when it was raised by at least what the changes ask for.
    Diff {
        old: PathBuf,
        new: PathBuf,
        #[arg(long)]
        json: bool,
        /// Succeed if, and only if, the new version is raised by at least what the changes ask for.
        #[arg(long)]
        check_version: bool,
    },
    /// Scaffold a test vector, or build a vector's payload from field values.
    Vectors {
        #[command(subcommand)]
        command: VectorsCommand,
    },
    /// Count what a set of payloads holds, field by field, and flag what to look at.
    Stats {
        dictionary: PathBuf,
        #[arg(long, conflicts_with = "port", required_unless_present = "port")]
        interface: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        /// File of payloads, one per line as hex ("-" for standard input).
        #[arg(long)]
        payloads: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
pub struct RangeArgs {
    /// Word size in bits.
    #[arg(long)]
    word_bits: Option<u64>,
    /// Which end of a word the document numbers first: msb or lsb.
    #[arg(long)]
    bit0: Option<String>,
    /// Number of the first word: 0 or 1.
    #[arg(long)]
    first_word: Option<u64>,
    /// Number of the first bit: 0 or 1.
    #[arg(long, default_value_t = 0)]
    first_bit: u64,
    /// Order a word's bytes are sent in: big or little.
    #[arg(long, default_value = "big")]
    word_order: String,
    /// Take the numbering from an interface's icd_numbering instead.
    #[arg(long, requires = "interface", conflicts_with_all = ["word_bits", "bit0", "first_word"])]
    dictionary: Option<PathBuf>,
    #[arg(long, requires = "dictionary")]
    interface: Option<String>,
    /// Word as the document writes it: 3, or 4-5 for a field across words.
    #[arg(long, requires = "bits", conflicts_with_all = ["start", "stop"])]
    word: Option<String>,
    /// Bits as the document writes them: 12-5, or 7 for one bit.
    #[arg(long, requires = "word")]
    bits: Option<String>,
    /// Dictionary start bit, for the reverse conversion.
    #[arg(long, requires = "stop")]
    start: Option<u64>,
    #[arg(long, requires = "start")]
    stop: Option<u64>,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
pub struct CoverageArgs {
    dictionary: PathBuf,
    /// Fail on a bit that belongs to no field (C001).
    #[arg(long)]
    require_full_coverage: bool,
    /// Fail on a message without declared_bits (C006).
    #[arg(long)]
    require_declared: bool,
    /// Fail on a message named and not defined, or defined and not named (C004, C005).
    #[arg(long)]
    require_defined: bool,
    /// Fail on a field without a complete source (C007).
    #[arg(long)]
    require_source: bool,
    /// Fail on a field without a description (C008).
    #[arg(long)]
    require_description: bool,
    /// Fail on a numeric field without a unit (C009).
    #[arg(long)]
    require_unit: bool,
    /// Fail on a message no vector expects (C010).
    #[arg(long)]
    require_vectors: bool,
    /// All of the --require flags.
    #[arg(long)]
    strict: bool,
    /// List every finding, not only those that fail.
    #[arg(long)]
    all: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
pub enum VectorsCommand {
    /// Write vectors/<name>.json for a message with every field listed and no value filled in.
    New {
        dictionary: PathBuf,
        #[arg(long)]
        interface: String,
        #[arg(long)]
        message: String,
        /// File name without .json; the message name in lower case with hyphens without it.
        #[arg(long)]
        name: Option<String>,
        /// An expected value from the document, as field=value; repeat for more.
        #[arg(long, value_name = "FIELD=VALUE")]
        set: Vec<String>,
        /// The payload as hex, when the document gives one.
        #[arg(long)]
        payload: Option<String>,
        /// Where the expected values come from.
        #[arg(long)]
        note: Option<String>,
    },
    /// Build a payload from field values. The values still come from the document.
    Encode {
        dictionary: PathBuf,
        /// Take the interface, message and values from vectors/<VECTOR>.json.
        #[arg(long, conflicts_with_all = ["interface", "message", "set"])]
        vector: Option<String>,
        /// With --vector: write the payload into the vector file.
        #[arg(long, requires = "vector")]
        write: bool,
        #[arg(long, required_unless_present = "vector", requires = "message")]
        interface: Option<String>,
        #[arg(long)]
        message: Option<String>,
        /// A field and its value, as field=value; repeat for more.
        #[arg(long, value_name = "FIELD=VALUE")]
        set: Vec<String>,
        /// Payload length in bytes; the message's length without it.
        #[arg(long)]
        length: Option<usize>,
    },
}

pub fn run(command: Command) -> Result<ExitCode, String> {
    match command {
        Command::New {
            dir,
            name,
            interface,
            port,
            repo,
        } => new(&dir, name, &interface, &port, repo),
        Command::Range(args) => range(&args),
        Command::ImportCsv {
            tables,
            into,
            allow_unreviewed,
            prune,
        } => import_csv(&tables, &into, allow_unreviewed, prune),
        Command::ExportCsv { dictionary, out } => export_csv(&dictionary, &out),
        Command::Fmt { dictionary, check } => fmt(&dictionary, check),
        Command::Coverage(args) => coverage_command(&args),
        Command::Lint {
            dictionary,
            json,
            rules,
            units,
        } => lint_command(dictionary.as_deref(), json, rules, units),
        Command::Diff {
            old,
            new,
            json,
            check_version,
        } => diff_command(&old, &new, json, check_version),
        Command::Vectors { command } => match command {
            VectorsCommand::New {
                dictionary,
                interface,
                message,
                name,
                set,
                payload,
                note,
            } => vectors_new(&dictionary, &interface, &message, name, &set, payload, note),
            VectorsCommand::Encode {
                dictionary,
                vector,
                write,
                interface,
                message,
                set,
                length,
            } => vectors_encode(&dictionary, vector, write, interface, message, &set, length),
        },
        Command::Stats {
            dictionary,
            interface,
            port,
            payloads,
            json,
        } => stats_command(&dictionary, interface, port, &payloads, json),
    }
}

/// `1 file`, `2 files`.
fn count(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

fn load_tree(path: &Path) -> Result<Tree, String> {
    Tree::load(path).map_err(|errors| errors.join("\n"))
}

fn print_json(value: &serde_json::Value) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn io_error(path: &Path) -> impl Fn(std::io::Error) -> String + '_ {
    move |e| format!("{}: {e}", path.display())
}

/// The files of `tools/dictionary-repo-template/`, with `{{NAME}}` standing
/// for the dictionary name.
const REPO_TEMPLATE: &[(&str, &str)] = &[
    (
        "README.md",
        include_str!("../../tools/dictionary-repo-template/README.md"),
    ),
    (
        "Makefile",
        include_str!("../../tools/dictionary-repo-template/Makefile"),
    ),
    (
        ".gitignore",
        include_str!("../../tools/dictionary-repo-template/.gitignore"),
    ),
    (
        "CHANGELOG.md",
        include_str!("../../tools/dictionary-repo-template/CHANGELOG.md"),
    ),
    (
        "CODEOWNERS",
        include_str!("../../tools/dictionary-repo-template/CODEOWNERS"),
    ),
    (
        "icd-notes/issues.md",
        include_str!("../../tools/dictionary-repo-template/icd-notes/issues.md"),
    ),
    (
        "tables/README.md",
        include_str!("../../tools/dictionary-repo-template/tables/README.md"),
    ),
    (
        ".github/workflows/check.yml",
        include_str!("../../tools/dictionary-repo-template/.github/workflows/check.yml"),
    ),
    (
        ".github/pull_request_template.md",
        include_str!("../../tools/dictionary-repo-template/.github/pull_request_template.md"),
    ),
];

fn new(
    dir: &Path,
    name: Option<String>,
    interfaces: &[String],
    ports: &[u16],
    repo: bool,
) -> Result<ExitCode, String> {
    if ports.len() > interfaces.len() {
        return Err("more --port than --interface".to_string());
    }
    if let Some(bad) = interfaces
        .iter()
        .find(|name| name.contains(['/', '\\']) || name.starts_with('.') || *name == "vectors")
    {
        return Err(format!("--interface {bad:?} cannot be a directory name"));
    }
    if dir.exists() && fs::read_dir(dir).map_err(io_error(dir))?.next().is_some() {
        return Err(format!("{} exists and is not empty", dir.display()));
    }
    let name = match name {
        Some(name) => name,
        None => dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or("give --name: the directory has no name to use")?,
    };
    let pairs: Vec<(String, Option<u16>)> = interfaces
        .iter()
        .enumerate()
        .map(|(index, interface)| (interface.clone(), ports.get(index).copied()))
        .collect();
    let dictionary_dir = if repo {
        dir.join("dictionary")
    } else {
        dir.to_path_buf()
    };
    let mut written = write_dir(&dictionary_dir, &new_dictionary(&name, &pairs))
        .map_err(io_error(&dictionary_dir))?;
    // Empty until the first message and the first vector are written.
    for interface in interfaces {
        let messages = dictionary_dir.join(interface).join("messages");
        fs::create_dir_all(&messages).map_err(io_error(&messages))?;
    }
    fs::create_dir_all(dictionary_dir.join("vectors")).map_err(io_error(&dictionary_dir))?;
    if repo {
        written = written
            .into_iter()
            .map(|path| format!("dictionary/{path}"))
            .collect();
        let files: FileSet = REPO_TEMPLATE
            .iter()
            .map(|(path, text)| (path.to_string(), text.replace("{{NAME}}", &name)))
            .collect();
        written.extend(write_dir(dir, &files).map_err(io_error(dir))?);
        written.sort();
    }
    for path in &written {
        println!("wrote {}", dir.join(path).display());
    }
    println!(
        "{}: dictionary {name:?} with {} and an empty vectors/",
        dictionary_dir.display(),
        if interfaces.len() == 1 {
            "1 interface".to_string()
        } else {
            format!("{} interfaces", interfaces.len())
        }
    );
    Ok(ExitCode::SUCCESS)
}

fn range(args: &RangeArgs) -> Result<ExitCode, String> {
    let numbering = match (&args.dictionary, &args.interface) {
        (Some(dictionary), Some(interface)) => {
            let tree = load_tree(dictionary)?;
            let node = tree
                .interface(interface)
                .ok_or_else(|| format!("no interface named {interface:?}"))?
                .metadata
                .get("icd_numbering")
                .ok_or_else(|| format!("{interface}/metadata.json has no icd_numbering"))?;
            Numbering::read(node)?
        }
        _ => Numbering {
            word_bits: args.word_bits.ok_or("give --word-bits")?,
            bit0: parse_bit0(
                args.bit0
                    .as_deref()
                    .ok_or("give --bit0 msb or --bit0 lsb")?,
            )?,
            first_word: args
                .first_word
                .ok_or("give --first-word 0 or --first-word 1")?,
            first_bit: args.first_bit,
            word_order: parse_word_order(&args.word_order)?,
        },
    };
    numbering.validate()?;
    let (span, icd, forward) = match (&args.word, &args.bits, args.start, args.stop) {
        (Some(word), Some(bits), _, _) => {
            let icd = IcdRange::parse(word, bits)?;
            (numbering.to_span(&icd)?, icd, true)
        }
        (_, _, Some(start), Some(stop)) => {
            let icd = numbering.to_icd(start, stop)?;
            let span = Span {
                start,
                stop,
                bit_order: numbering.bit_order(),
            };
            (span, icd, false)
        }
        _ => return Err("give --word and --bits, or --start and --stop".to_string()),
    };
    let order = bit_order_name(span.bit_order);
    if args.json {
        print_json(&json!({
            "start": span.start,
            "stop": span.stop,
            "width": span.stop - span.start,
            "bit_order": order,
            "word": icd.word_text(),
            "bits": icd.bits_text(),
            "numbering": numbering.describe(),
        }))?;
        return Ok(ExitCode::SUCCESS);
    }
    if forward {
        println!("start {}", span.start);
        println!("stop  {}", span.stop);
    } else {
        println!("word {}", icd.word_text());
        println!("bits {}", icd.bits_text());
    }
    println!(
        "width {} bits, under \"bit_order\": \"{order}\"",
        span.stop - span.start
    );
    println!("numbering: {}", numbering.describe());
    print!("{}", numbering.ruler(&span));
    if span.bit_order == BitOrder::Lsb0 {
        println!("(each word is drawn most significant bit first; its low byte is sent first)");
    }
    Ok(ExitCode::SUCCESS)
}

fn read_table(dir: &Path, name: &str, required: bool) -> Result<String, String> {
    let path = dir.join(name);
    match fs::read_to_string(&path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !required => Ok(String::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn import_csv(
    dir: &Path,
    into: &Path,
    allow_unreviewed: bool,
    prune: bool,
) -> Result<ExitCode, String> {
    let tables = Tables {
        interfaces: read_table(dir, INTERFACES_FILE, true)?,
        messages: read_table(dir, MESSAGES_FILE, true)?,
        fields: read_table(dir, FIELDS_FILE, true)?,
        values: read_table(dir, VALUES_FILE, false)?,
    };
    let interfaces = match table::import(&tables, ImportOptions { allow_unreviewed }) {
        Ok(interfaces) => interfaces,
        Err(errors) => {
            for error in &errors {
                println!("{error}");
            }
            println!("{}; nothing was written", count(errors.len(), "problem"));
            return Ok(ExitCode::FAILURE);
        }
    };
    let existing = if into.is_dir() {
        read_dir(into).map_err(io_error(into))?
    } else {
        FileSet::new()
    };
    let tree = Tree {
        dictionary: None,
        interfaces,
        vectors: Vec::new(),
    };
    let files = canon::canonical(&tree)
        .map_err(|errors| errors.join("\n"))?
        .to_files();
    let stale: Vec<&String> = existing
        .keys()
        .filter(|path| {
            *path != "dictionary.json"
                && !path.starts_with("vectors/")
                && !files.contains_key(*path)
        })
        .collect();
    if !stale.is_empty() && !prune {
        for path in &stale {
            println!("{}: not in the tables", into.join(path).display());
        }
        println!(
            "{} files are in the dictionary and not in the tables; nothing was written. Add them to the tables, or pass --prune to delete them",
            stale.len()
        );
        return Ok(ExitCode::FAILURE);
    }
    for path in &stale {
        fs::remove_file(into.join(path)).map_err(io_error(&into.join(path)))?;
        println!("deleted {}", into.join(path).display());
    }
    let changed = write_dir(into, &files).map_err(io_error(into))?;
    for path in &changed {
        println!("wrote {}", into.join(path).display());
    }
    let messages: usize = tree.interfaces.iter().map(|i| i.messages.len()).sum();
    println!(
        "{}, {}: {} written, {} unchanged",
        count(tree.interfaces.len(), "interface"),
        count(messages, "message"),
        count(changed.len(), "file"),
        files.len() - changed.len()
    );
    Ok(ExitCode::SUCCESS)
}

fn export_csv(dictionary: &Path, out: &Path) -> Result<ExitCode, String> {
    let tree = load_tree(dictionary)?;
    let tables = match table::export(&tree) {
        Ok(tables) => tables,
        Err(errors) => {
            for error in &errors {
                println!("{error}");
            }
            println!(
                "{} things the tables cannot hold; nothing was written",
                errors.len()
            );
            return Ok(ExitCode::FAILURE);
        }
    };
    fs::create_dir_all(out).map_err(io_error(out))?;
    for (name, text) in [
        (INTERFACES_FILE, &tables.interfaces),
        (MESSAGES_FILE, &tables.messages),
        (FIELDS_FILE, &tables.fields),
        (VALUES_FILE, &tables.values),
    ] {
        let path = out.join(name);
        fs::write(&path, text).map_err(io_error(&path))?;
        println!(
            "wrote {} ({} rows)",
            path.display(),
            text.lines().count().saturating_sub(1)
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn fmt(dictionary: &Path, check: bool) -> Result<ExitCode, String> {
    if !dictionary.is_dir() {
        return Err(format!("{} is not a directory", dictionary.display()));
    }
    let files = read_dir(dictionary).map_err(io_error(dictionary))?;
    let formatted = match canon::format(&files) {
        Ok(formatted) => formatted,
        Err(errors) => {
            for error in &errors {
                println!("{error}");
            }
            println!("{}; nothing was changed", count(errors.len(), "problem"));
            return Ok(ExitCode::FAILURE);
        }
    };
    let changed: Vec<&String> = formatted
        .iter()
        .filter(|(path, text)| files.get(*path) != Some(*text))
        .map(|(path, _)| path)
        .collect();
    if check {
        for path in &changed {
            println!("not canonical: {}", dictionary.join(path).display());
        }
        println!(
            "{}, {} not canonical",
            count(formatted.len(), "file"),
            changed.len()
        );
        return Ok(if changed.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }
    let written = write_dir(dictionary, &formatted).map_err(io_error(dictionary))?;
    for path in &written {
        println!("formatted {}", dictionary.join(path).display());
    }
    println!(
        "{}, {} rewritten",
        count(formatted.len(), "file"),
        written.len()
    );
    Ok(ExitCode::SUCCESS)
}

fn coverage_command(args: &CoverageArgs) -> Result<ExitCode, String> {
    let tree = load_tree(&args.dictionary)?;
    let report = coverage(&tree);
    let config = LintConfig::read(tree.dictionary.as_ref());
    let mut used = vec![false; config.allows.len()];
    let sorted = rules::sort(report.findings.clone(), &config.allows, &mut used);

    let mut enforced = vec!["C002", "C003", "C011"];
    let flags = [
        (args.require_full_coverage, &["C001"][..]),
        (args.require_defined, &["C004", "C005"][..]),
        (args.require_declared, &["C006"][..]),
        (args.require_source, &["C007"][..]),
        (args.require_description, &["C008"][..]),
        (args.require_unit, &["C009"][..]),
        (args.require_vectors, &["C010"][..]),
    ];
    for (on, ids) in flags {
        if on || args.strict {
            enforced.extend_from_slice(ids);
        }
    }
    let (failing, other): (Vec<&Finding>, Vec<&Finding>) = sorted
        .kept
        .iter()
        .partition(|finding| enforced.contains(&finding.rule));

    if args.json {
        let mut value = report.to_json();
        value["failing"] = failing.iter().map(|f| f.to_json()).collect();
        value["not_enforced"] = other.iter().map(|f| f.to_json()).collect();
        value["allowed"] = sorted
            .allowed
            .iter()
            .map(|(finding, reason)| {
                let mut value = finding.to_json();
                value["reason"] = json!(reason);
                value
            })
            .collect();
        print_json(&value)?;
    } else {
        print!("{}", report.to_text());
        for finding in &failing {
            println!("{finding}");
        }
        if args.all {
            for finding in &other {
                println!("not enforced: {finding}");
            }
            for (finding, reason) in &sorted.allowed {
                println!("allowed: {finding} ({reason})");
            }
        }
        println!(
            "{} failing, {} not enforced by the flags given, {} allowed in dictionary.json",
            count(failing.len(), "finding"),
            other.len(),
            sorted.allowed.len()
        );
    }
    Ok(if failing.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn lint_command(
    dictionary: Option<&Path>,
    as_json: bool,
    list_rules: bool,
    list_units: bool,
) -> Result<ExitCode, String> {
    if list_rules {
        for rule in RULES {
            println!("{}  {}", rule.id, rule.requires);
        }
        return Ok(ExitCode::SUCCESS);
    }
    if list_units {
        for (unit, meaning) in UNITS {
            println!("{unit:<8}{meaning}");
        }
        return Ok(ExitCode::SUCCESS);
    }
    let dictionary = dictionary.ok_or("give a dictionary directory")?;
    let tree = load_tree(dictionary)?;
    let config = LintConfig::read(tree.dictionary.as_ref());
    let mut used = vec![false; config.allows.len()];
    let mut sorted = rules::sort(lint(&tree, &config), &config.allows, &mut used);
    // An allow entry may be for a coverage rule; count those as used too.
    rules::sort(coverage(&tree).findings, &config.allows, &mut used);
    sorted.kept.extend(config.malformed.iter().cloned());
    for (allow, used) in config.allows.iter().zip(&used) {
        if !used {
            sorted.kept.push(Finding::new(
                "X002",
                "dictionary.json",
                format!(
                    "lint.allow entry for {}{} matches nothing; remove it",
                    allow.rule,
                    allow
                        .at
                        .as_ref()
                        .map_or(String::new(), |at| format!(" at {at}"))
                ),
            ));
        }
    }
    if as_json {
        print_json(&json!({
            "findings": sorted.kept.iter().map(Finding::to_json).collect::<Vec<_>>(),
            "allowed": sorted.allowed.iter().map(|(finding, reason)| {
                let mut value = finding.to_json();
                value["reason"] = json!(reason);
                value
            }).collect::<Vec<_>>(),
        }))?;
    } else {
        for finding in &sorted.kept {
            println!("{finding}");
        }
        println!(
            "{}, {} allowed in dictionary.json",
            count(sorted.kept.len(), "finding"),
            sorted.allowed.len()
        );
    }
    Ok(if sorted.kept.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn diff_command(
    old: &Path,
    new: &Path,
    as_json: bool,
    check_version: bool,
) -> Result<ExitCode, String> {
    let result = diff(&load_tree(old)?, &load_tree(new)?);
    if as_json {
        print_json(&result.to_json())?;
    } else {
        print!("{}", result.to_text());
    }
    if check_version {
        if result.version_is_sufficient().is_none() && !as_json {
            println!("--check-version needs a MAJOR.MINOR.PATCH version in both dictionaries");
        }
        return Ok(if result.version_is_sufficient() == Some(true) {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }
    Ok(if result.is_breaking() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn vector_file_name(message: &str) -> String {
    message
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

fn split_pairs(set: &[String]) -> Result<Vec<(String, String)>, String> {
    set.iter()
        .map(|pair| {
            pair.split_once('=')
                .map(|(field, value)| (field.to_string(), value.to_string()))
                .ok_or_else(|| format!("--set {pair:?} is not field=value"))
        })
        .collect()
}

fn vectors_new(
    dictionary: &Path,
    interface: &str,
    message: &str,
    name: Option<String>,
    set: &[String],
    payload: Option<String>,
    note: Option<String>,
) -> Result<ExitCode, String> {
    let tree = load_tree(dictionary)?;
    let interface = tree
        .interface(interface)
        .ok_or_else(|| format!("no interface named {interface:?}"))?;
    let mut vector = new_vector(interface, message)?;
    let mut expected: Vec<(String, Node)> = vector
        .get("expect")
        .and_then(|expect| expect.get("fields"))
        .map(|fields| fields.entries().to_vec())
        .unwrap_or_default();
    for (field, value) in split_pairs(set)? {
        match expected.iter_mut().find(|(known, _)| *known == field) {
            Some(entry) => entry.1 = Node::from(value),
            None => return Err(format!("{field}: no such field in {message}")),
        }
    }
    let unfilled = expected
        .iter()
        .filter(|(_, value)| value.as_str() == Some(""))
        .count();
    vector.set(
        "expect",
        Node::Object(vec![
            ("message_type".to_string(), Node::from(message)),
            ("fields".to_string(), Node::Object(expected)),
        ]),
    );
    if let Some(payload) = payload {
        let compact: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
        hex::decode(&compact).map_err(|e| format!("--payload: {e}"))?;
        vector.set("payload_hex", Node::from(compact.to_ascii_lowercase()));
    }
    if let Some(note) = note {
        vector.set("note", Node::from(note));
    }
    let name = name.unwrap_or_else(|| vector_file_name(message));
    if name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(format!("--name {name:?} is not a file name"));
    }
    let path = dictionary.join("vectors").join(format!("{name}.json"));
    if path.exists() {
        return Err(format!("{} exists", path.display()));
    }
    fs::create_dir_all(dictionary.join("vectors")).map_err(io_error(dictionary))?;
    fs::write(&path, vector.to_text()).map_err(io_error(&path))?;
    println!("wrote {}", path.display());
    if unfilled > 0 {
        println!(
            "{unfilled} expected values are empty: fill them in from the document, then run bite vectors encode {} --vector {name}",
            dictionary.display()
        );
    }
    Ok(ExitCode::SUCCESS)
}

#[allow(clippy::too_many_arguments)]
fn vectors_encode(
    path: &Path,
    vector: Option<String>,
    write: bool,
    interface: Option<String>,
    message: Option<String>,
    set: &[String],
    length: Option<usize>,
) -> Result<ExitCode, String> {
    let tree = load_tree(path)?;
    let dictionary = Dictionary::load(path).map_err(|e| e.to_string())?;
    let mut existing_payload = None;
    let (interface_name, message_name, values): (String, String, Vec<(String, String)>) =
        match &vector {
            Some(name) => {
                let (_, node) = tree
                    .vectors
                    .iter()
                    .find(|(known, _)| known == name)
                    .ok_or_else(|| format!("no vectors/{name}.json"))?;
                let expect = node.get("expect");
                let message = expect
                    .and_then(|e| e.str_at("message_type"))
                    .ok_or("the vector has no expect.message_type")?;
                let mut values = Vec::new();
                for (field, value) in expect
                    .and_then(|e| e.get("fields"))
                    .map(Node::entries)
                    .unwrap_or_default()
                {
                    match value {
                        // A field expected to lie beyond the payload.
                        Node::Null => {}
                        Node::String(text) if text.is_empty() => {
                            return Err(format!("{field}: no value filled in"))
                        }
                        Node::String(text) => values.push((field.clone(), text.clone())),
                        other => {
                            return Err(format!(
                                "{field}: expected value {} is not text",
                                other.to_line()
                            ))
                        }
                    }
                }
                existing_payload = node
                    .str_at("payload_hex")
                    .map(|text| {
                        text.chars()
                            .filter(|c| !c.is_whitespace())
                            .collect::<String>()
                    })
                    .filter(|text| !text.is_empty())
                    .map(|text| hex::decode(&text).map_err(|e| format!("payload_hex: {e}")))
                    .transpose()?;
                (
                    node.str_at("interface")
                        .ok_or("the vector has no interface")?
                        .to_string(),
                    message.to_string(),
                    values,
                )
            }
            None => {
                let values = split_pairs(set)?;
                (
                    interface.ok_or("give --interface")?,
                    message.ok_or("give --message")?,
                    values,
                )
            }
        };
    let labels = tree
        .interface(&interface_name)
        .ok_or_else(|| format!("no interface named {interface_name:?}"))?;
    let interface = dictionary
        .interface(&interface_name)
        .ok_or_else(|| format!("no interface named {interface_name:?}"))?;
    let message = interface
        .message(&message_name)
        .ok_or_else(|| format!("{interface_name} has no message {message_name:?}"))?;
    let encoded = match encode(
        interface,
        labels,
        message,
        &values,
        length,
        existing_payload.as_deref(),
    ) {
        Ok(encoded) => encoded,
        Err(errors) => {
            for error in &errors {
                println!("{error}");
            }
            return Ok(ExitCode::FAILURE);
        }
    };
    let payload_hex = hex::encode(&encoded.payload);
    println!("payload_hex {payload_hex}");
    let width = encoded
        .fields
        .iter()
        .map(|f| f.name.len())
        .max()
        .unwrap_or(5)
        .max(5);
    println!("{:<width$}  {:<12}  {:<18}  value", "field", "bits", "raw");
    for field in &encoded.fields {
        println!(
            "{:<width$}  {:<12}  {:<18}  {}",
            field.name,
            format!("[{}, {})", field.start, field.stop),
            field.raw.map_or(String::new(), |raw| format!("0x{raw:x}")),
            field.given
        );
    }
    if !encoded.left_zero.is_empty() {
        println!("left zero: {}", encoded.left_zero.join(", "));
    }
    // What the decoder makes of the payload, to catch a value that was
    // accepted and reads back differently.
    let frame_id = vector
        .as_ref()
        .and_then(|name| tree.vectors.iter().find(|(known, _)| known == name))
        .and_then(|(_, node)| node.u64_at("frame_id"));
    if let Ok(decoded) = interface.decode_frame(&encoded.payload, frame_id) {
        let rendered = decoded.rendered();
        for (field, given) in &values {
            if let Some(Some(back)) = rendered.get(field.as_str()) {
                let same_number = matches!(
                    (given.parse::<f64>(), back.parse::<f64>()),
                    (Ok(a), Ok(b)) if (a - b).abs() <= 1e-9 * a.abs().max(b.abs())
                );
                if back != given && !same_number && !given.starts_with("raw:") {
                    println!("note: {field} was given as {given} and decodes as {back}");
                }
            }
        }
    }
    println!("These bytes were built with the dictionary. Check them against the document by hand or with a second implementation.");

    let Some(name) = vector else {
        return Ok(ExitCode::SUCCESS);
    };
    if write {
        let file = path.join("vectors").join(format!("{name}.json"));
        let mut files = read_dir(path).map_err(io_error(path))?;
        let key = format!("vectors/{name}.json");
        let mut node = Node::parse(files.get(&key).ok_or("the vector file is gone")?)?;
        node.set("payload_hex", Node::from(payload_hex));
        files.insert(key.clone(), node.to_text());
        let formatted = canon::format(&files).map_err(|errors| errors.join("\n"))?;
        fs::write(&file, &formatted[&key]).map_err(io_error(&file))?;
        println!("wrote {}", file.display());
        return Ok(ExitCode::SUCCESS);
    }
    let Some(existing) = existing_payload else {
        println!("the vector has no payload_hex to compare with; --write puts this one in");
        return Ok(ExitCode::SUCCESS);
    };
    // Bytes after the message's definition are not built from values.
    let built = encoded.payload.len();
    if existing.len() > built {
        println!(
            "the vector's payload has {} more bytes after the definition ends; they are not compared",
            existing.len() - built
        );
    }
    if existing.len() >= built && existing[..built] == encoded.payload[..] {
        println!("matches the vector's payload_hex");
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "DIFFERS from the vector's payload_hex {}",
        hex::encode(&existing)
    );
    for (index, (a, b)) in encoded.payload.iter().zip(&existing).enumerate() {
        if a != b {
            let names: Vec<&str> = message
                .fields
                .iter()
                .filter(|field| field.start / 8 <= index && index < field.stop.div_ceil(8))
                .map(|field| field.name.as_str())
                .collect();
            println!(
                "  byte {index}: built {a:02x}, vector {b:02x} ({})",
                if names.is_empty() {
                    "no field".to_string()
                } else {
                    names.join(", ")
                }
            );
        }
    }
    Ok(ExitCode::FAILURE)
}

fn stats_command(
    path: &Path,
    interface: Option<String>,
    port: Option<u16>,
    payloads: &Path,
    as_json: bool,
) -> Result<ExitCode, String> {
    let dictionary = Dictionary::load(path).map_err(|e| e.to_string())?;
    let tree = load_tree(path)?;
    let interface = match (&interface, port) {
        (Some(name), _) => dictionary
            .interface(name)
            .ok_or_else(|| format!("no interface named {name:?}"))?,
        (None, Some(port)) => dictionary
            .interface_for_port(port)
            .ok_or_else(|| format!("no interface claims port {port}"))?,
        (None, None) => return Err("give --interface or --port".to_string()),
    };
    let text = if payloads.as_os_str() == "-" {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        text
    } else {
        fs::read_to_string(payloads).map_err(io_error(payloads))?
    };
    let payloads = parse_payloads(&text)?;
    let files = tree.interface(&interface.name);
    let declared = |message: &str| {
        files
            .and_then(|files| files.message(message))
            .and_then(|node| node.u64_at("declared_bits"))
    };
    let result = stats(interface, &declared, &payloads);
    if as_json {
        print_json(&result.to_json())?;
    } else {
        print!("{}", result.to_text());
    }
    Ok(ExitCode::SUCCESS)
}
