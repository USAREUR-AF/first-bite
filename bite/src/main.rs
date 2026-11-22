//! `bite`: inspect and check dictionaries, and decode payloads by hand.

mod authoring_cli;

use bite::authoring::rules::{rule_of_check_warning, LintConfig};
use bite::authoring::Node;
use bite::vectors::run_vectors;
use bite::{Dictionary, Severity};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    version,
    about = "Decode fixed-layout binary messages against a dictionary"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate a dictionary and run its test vectors.
    Check {
        dictionary: PathBuf,
        /// Treat warnings as failures.
        #[arg(long)]
        strict: bool,
    },
    /// List the interfaces and messages of a dictionary.
    List { dictionary: PathBuf },
    /// Print JSON Schema for the decoded form of every message.
    Schema { dictionary: PathBuf },
    /// Decode one payload given as hex.
    Decode {
        dictionary: PathBuf,
        /// Interface name.
        #[arg(long, conflicts_with = "port", required_unless_present = "port")]
        interface: Option<String>,
        /// UDP or TCP port; selects the interface that claims it.
        #[arg(long)]
        port: Option<u16>,
        /// Frame identifier, for interfaces that identify messages by it.
        #[arg(long)]
        frame_id: Option<u64>,
        /// Payload bytes as hex.
        payload_hex: String,
    },
    #[command(flatten)]
    Authoring(authoring_cli::Command),
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("bite: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    match cli.command {
        Command::Check { dictionary, strict } => check(&dictionary, strict),
        Command::List { dictionary } => list(&dictionary),
        Command::Schema { dictionary } => schema(&dictionary),
        Command::Decode {
            dictionary,
            interface,
            port,
            frame_id,
            payload_hex,
        } => decode(&dictionary, interface, port, frame_id, &payload_hex),
        Command::Authoring(command) => authoring_cli::run(command),
    }
}

fn load(path: &Path) -> Result<Dictionary, String> {
    Dictionary::load(path).map_err(|e| e.to_string())
}

/// The findings `dictionary.json` allows with a reason, under `lint.allow`.
fn allowed(path: &Path) -> LintConfig {
    let node = std::fs::read_to_string(path.join("dictionary.json"))
        .ok()
        .and_then(|text| Node::parse(&text).ok());
    LintConfig::read(node.as_ref())
}

fn check(path: &Path, strict: bool) -> Result<ExitCode, String> {
    let dictionary = load(path)?;
    let config = allowed(path);
    let mut allowed_warnings = 0;
    for issue in dictionary.issues() {
        let reason = (issue.severity == Severity::Warning)
            .then(|| rule_of_check_warning(&issue.message))
            .flatten()
            .and_then(|rule| {
                config
                    .allows
                    .iter()
                    .find(|allow| allow.covers_at(rule, &issue.location))
            });
        match reason {
            Some(allow) => {
                allowed_warnings += 1;
                println!(
                    "allowed: {}: {} ({})",
                    issue.location, issue.message, allow.reason
                );
            }
            None => println!("{issue}"),
        }
    }
    let errors = count(&dictionary, Severity::Error);
    let warnings = count(&dictionary, Severity::Warning) - allowed_warnings;

    let results = run_vectors(&dictionary).map_err(|e| e.to_string())?;
    let mut failed = 0;
    for result in &results {
        if !result.passed() {
            failed += 1;
            for failure in &result.failures {
                println!("vector {}: {failure}", result.name);
            }
        }
    }

    let messages: usize = dictionary
        .interfaces()
        .iter()
        .map(|interface| interface.messages().len())
        .sum();
    println!(
        "{} interfaces, {messages} messages, {errors} errors, {warnings} warnings{}, {}/{} vectors passed",
        dictionary.interfaces().len(),
        if allowed_warnings > 0 {
            format!(" ({allowed_warnings} allowed in dictionary.json)")
        } else {
            String::new()
        },
        results.len() - failed,
        results.len(),
    );
    let ok = errors == 0 && failed == 0 && !(strict && warnings > 0);
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn count(dictionary: &Dictionary, severity: Severity) -> usize {
    dictionary
        .issues()
        .iter()
        .filter(|issue| issue.severity == severity)
        .count()
}

fn list(path: &Path) -> Result<ExitCode, String> {
    let dictionary = load(path)?;
    for interface in dictionary.interfaces() {
        match interface.port {
            Some(port) => println!("{} (port {port})", interface.name),
            None => println!("{}", interface.name),
        }
        let mut codes: Vec<_> = interface.discriminator.names.iter().collect();
        codes.sort();
        for (code, name) in codes {
            match interface.message(name) {
                Some(message) => println!(
                    "  {code:>6}  {name}  ({} fields, {} bits)",
                    message.fields.len(),
                    message.bit_length()
                ),
                None => println!("  {code:>6}  {name}  (not defined)"),
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn schema(path: &Path) -> Result<ExitCode, String> {
    let dictionary = load(path)?;
    let schemas = bite::jsonschema::dictionary_schemas(&dictionary);
    println!(
        "{}",
        serde_json::to_string_pretty(&schemas).map_err(|e| e.to_string())?
    );
    Ok(ExitCode::SUCCESS)
}

fn decode(
    path: &Path,
    interface: Option<String>,
    port: Option<u16>,
    frame_id: Option<u64>,
    payload_hex: &str,
) -> Result<ExitCode, String> {
    let dictionary = load(path)?;
    let interface = match (&interface, port) {
        (Some(name), _) => dictionary
            .interface(name)
            .ok_or_else(|| format!("no interface named {name:?}"))?,
        (None, Some(port)) => dictionary
            .interface_for_port(port)
            .ok_or_else(|| format!("no interface claims port {port}"))?,
        (None, None) => return Err("give --interface or --port".to_string()),
    };
    let compact: String = payload_hex.chars().filter(|c| !c.is_whitespace()).collect();
    let payload = hex::decode(compact).map_err(|e| format!("payload: {e}"))?;
    // A payload of several messages (framing concatenated) prints one object
    // per message, in order.
    let messages = match frame_id {
        Some(_) => vec![&payload[..]],
        None => interface.split(&payload),
    };
    for message in messages {
        let decoded = interface
            .decode_frame(message, frame_id)
            .map_err(|e| e.to_string())?;
        let output = serde_json::json!({
            "interface": interface.name,
            "message_type": decoded.message_type(),
            "fields": decoded,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&output).map_err(|e| e.to_string())?
        );
    }
    Ok(ExitCode::SUCCESS)
}
