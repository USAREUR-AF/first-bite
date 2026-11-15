use chrono::Utc;
use env_logger::fmt::Formatter;
use log::Record;

use colored::*;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::io::Write;

use datamodels::packets::LogMetadata;

#[derive(Serialize, Deserialize, Debug)]
pub enum MessageType {
    Misc,
    Packet,
    Telemetry,
    Heartbeat,
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageType::Misc => write!(f, "misc"),
            MessageType::Packet => write!(f, "packet"),
            MessageType::Telemetry => write!(f, "telemetry"),
            MessageType::Heartbeat => write!(f, "heartbeat"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LogMessage {
    pub message_type: MessageType, // Use the enum for type-safety
    pub message: serde_json::Value,
}

/// `message` with every control character and line separator written as an
/// escape, so that one log call is one line whatever its arguments hold.
///
/// Log arguments carry text from outside the process: a topic read from a
/// socket, the body of an HTTP error answer, names from a dictionary. A line
/// break in one of them would otherwise start a line that reads like a log
/// entry of its own, and an escape sequence would reach the terminal of
/// whoever reads the log.
pub fn single_line(message: &str) -> Cow<'_, str> {
    let breaks_a_line = |c: char| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}');
    if !message.contains(breaks_a_line) {
        return Cow::Borrowed(message);
    }
    let mut line = String::with_capacity(message.len() + 16);
    for c in message.chars() {
        match c {
            '\n' => line.push_str("\\n"),
            '\r' => line.push_str("\\r"),
            '\t' => line.push_str("\\t"),
            c if breaks_a_line(c) => line.push_str(&format!("\\u{{{:04x}}}", c as u32)),
            c => line.push(c),
        }
    }
    Cow::Owned(line)
}

pub fn setup_logger(metadata: &LogMetadata) {
    let metadata_json = metadata.to_json().unwrap_or_else(|e| {
        eprintln!("Error converting metadata to JSON: {}", e);
        String::from("Metadata unavailable")
    });

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format(move |buf: &mut Formatter, record: &Record| {
            let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
            let message = record.args().to_string();
            let message = single_line(&message);
            let level_str = match record.level() {
                log::Level::Debug => format!("{}", record.level()).purple(),
                log::Level::Info => format!("{}", record.level()).green(),
                log::Level::Warn => format!("{}", record.level()).yellow(),
                log::Level::Error => format!("{}", record.level()).red(),
                _ => format!("{}", record.level()).normal(), // Use `.normal()` here
            };
            // Check if the log level is Error
            if record.level() == log::Level::Error {
                let file = record.file().unwrap_or("unknown_file");
                let line = record.line().unwrap_or(0);
                writeln!(
                    buf,
                    "[{}][Level: {}][{{\"metadata\": {}}}][{}][{}:{}] --- {}",
                    timestamp,
                    level_str,
                    metadata_json.to_string().cyan(),
                    record.module_path().unwrap_or("unknown_module"),
                    file.to_string().red(),
                    format!("{}", line).red(),
                    message
                )
            } else {
                writeln!(
                    buf,
                    "[{}][Level: {}][{{\"metadata\": {}}}][{}] --- {}",
                    timestamp,
                    level_str,
                    metadata_json.to_string().cyan(),
                    record.module_path().unwrap_or("unknown_module"),
                    message
                )
            }
        })
        .init();

    match std::env::current_dir() {
        Ok(dir) => log::info!("Current dir: {}", dir.display()),
        Err(e) => log::warn!("Current dir is not readable: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::single_line;
    use std::borrow::Cow;

    #[test]
    fn ordinary_text_is_not_copied() {
        let line = single_line(r#"{"message_type":"Packet","message":{"a":"b\n"}}"#);
        assert!(matches!(line, Cow::Borrowed(_)));
    }

    #[test]
    fn line_breaks_and_escape_sequences_are_written_out() {
        assert_eq!(
            single_line("record on WX\n[2026-01-01][Level: ERROR] forged\r\x1b[2J\ttab\u{2028}x\0"),
            "record on WX\\n[2026-01-01][Level: ERROR] forged\\r\\u{001b}[2J\\ttab\\u{2028}x\\u{0000}"
        );
    }
}
