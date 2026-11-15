use chrono::Utc;
use env_logger::fmt::Formatter;
use log::Record;

use colored::*;
use serde::{Deserialize, Serialize};
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

pub fn setup_logger(metadata: &LogMetadata) {
    let metadata_json = metadata.to_json().unwrap_or_else(|e| {
        eprintln!("Error converting metadata to JSON: {}", e);
        String::from("Metadata unavailable")
    });

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format(move |buf: &mut Formatter, record: &Record| {
            let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
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
                    record.args()
                )
            } else {
                writeln!(
                    buf,
                    "[{}][Level: {}][{{\"metadata\": {}}}][{}] --- {}",
                    timestamp,
                    level_str,
                    metadata_json.to_string().cyan(),
                    record.module_path().unwrap_or("unknown_module"),
                    record.args()
                )
            }
        })
        .init();

    match std::env::current_dir() {
        Ok(dir) => log::info!("Current dir: {}", dir.display()),
        Err(e) => log::warn!("Current dir is not readable: {}", e),
    }
}
