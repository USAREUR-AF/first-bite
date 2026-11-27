//! The `mind goat` commands.

use super::schema::dictionary_label;
use bite::Dictionary;
use clap::{Parser, Subcommand};
use flow::goat::bundle::{self, Naming};
use flow::goat::cert::{checks, now_unix, validate};
use flow::goat::session::{self, Delivery, Read};
use flow::goat::{GoatConfig, GoatError, KeyLayout, GOAT_CONFIG_ENV};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

/// Join a goat data fabric: check the configuration, see what would be
/// published where, and read the fabric back.
#[derive(Parser, Debug)]
#[command(name = "mind goat", version)]
struct Opt {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Check the configuration and certificates without connecting, and print
    /// every key that would be published.
    Check {
        /// goat configuration file. Overrides GOAT_CONFIG_PATH.
        #[arg(long, value_name = "FILE")]
        config: Option<PathBuf>,
        /// Dictionary directory. Overrides BITE_DICT_PATH.
        #[arg(long, value_name = "DIR")]
        dictionary: Option<PathBuf>,
    },
    /// Print the Zenoh client configuration generated from the goat
    /// configuration.
    Config {
        #[arg(long, value_name = "FILE")]
        config: Option<PathBuf>,
    },
    /// Write a goat configuration from an unpacked onboarding archive.
    Init {
        /// Directory the goatnet's onboarding archive was unpacked into.
        #[arg(long, value_name = "DIR")]
        bundle: PathBuf,
        /// Name of the message set; the <format> chunk of the keys.
        #[arg(long)]
        format: String,
        /// Where the data source is; the <site> chunk of the keys.
        #[arg(long)]
        site: String,
        /// Name of this publisher among others of the same format.
        #[arg(long, default_value = "mind-1")]
        instance: String,
        /// Router endpoint, tls/<host>:<port>. Overrides the archive's NOTES.md.
        #[arg(long)]
        endpoint: Option<String>,
        /// File to write. Without it the configuration is printed.
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Connect, put one health sample and confirm the router delivers it.
    Probe {
        #[arg(long, value_name = "FILE")]
        config: Option<PathBuf>,
    },
    /// Print samples from the fabric. Without a key expression, everything
    /// this identity publishes.
    Watch {
        /// Key expression; it must have at least one literal chunk.
        key_expression: Option<String>,
        #[arg(long, value_name = "FILE")]
        config: Option<PathBuf>,
        /// Ask the router's storage instead of waiting for new samples.
        #[arg(long)]
        stored: bool,
        /// How long to read.
        #[arg(long, default_value_t = 30)]
        seconds: u64,
        /// Stop after this many samples.
        #[arg(long)]
        count: Option<usize>,
        /// Print each payload after its key.
        #[arg(long)]
        payload: bool,
        /// Save each sample's payload, unchanged, as one file in this
        /// directory: a tap's capture slice becomes a .pcap file.
        #[arg(long, value_name = "DIR")]
        save: Option<PathBuf>,
    },
}

pub(super) fn run(arguments: Vec<OsString>) -> ExitCode {
    let opt = Opt::parse_from(std::iter::once(OsString::from("mind goat")).chain(arguments));
    match execute(opt.command) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("mind goat: {e}");
            ExitCode::FAILURE
        }
    }
}

fn load(config: Option<PathBuf>) -> Result<(GoatConfig, String), GoatError> {
    match config {
        Some(path) => Ok((GoatConfig::load(&path)?, path.display().to_string())),
        None => {
            let origin = std::env::var(GOAT_CONFIG_ENV).unwrap_or_default();
            Ok((GoatConfig::from_env()?, origin))
        }
    }
}

fn execute(command: Command) -> Result<ExitCode, GoatError> {
    match command {
        Command::Check { config, dictionary } => check(config, dictionary),
        Command::Config { config } => {
            let (config, _) = load(config)?;
            println!("{}", session::client_config_text(&config, true)?);
            Ok(ExitCode::SUCCESS)
        }
        Command::Init {
            bundle,
            format,
            site,
            instance,
            endpoint,
            out,
        } => {
            let naming = Naming {
                format,
                site,
                instance,
                endpoint,
            };
            let text = bundle::configuration(&bundle, &naming)?;
            match out {
                Some(path) => {
                    std::fs::write(&path, &text).map_err(|source| GoatError::File {
                        what: "the output file",
                        path: path.clone(),
                        source,
                    })?;
                    println!(
                        "wrote {}; next: mind goat check --config {}",
                        path.display(),
                        path.display()
                    );
                }
                None => print!("{text}"),
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Probe { config } => {
            let (config, _) = load(config)?;
            validate(&config, now_unix())?;
            let layout = KeyLayout::new(&config)?;
            match session::probe(&config)? {
                Delivery::Delivered { key, after } => {
                    println!(
                        "delivered: a sample put on {key} came back through the router after {} ms",
                        after.as_millis()
                    );
                    Ok(ExitCode::SUCCESS)
                }
                Delivery::NotDelivered { key, waited } => {
                    println!(
                        "not delivered within {} s: {}",
                        waited.as_secs(),
                        session::not_delivered_advice(&config, &layout, &key)
                    );
                    Ok(ExitCode::FAILURE)
                }
            }
        }
        Command::Watch {
            key_expression,
            config,
            stored,
            seconds,
            count,
            payload,
            save,
        } => {
            let (config, _) = load(config)?;
            validate(&config, now_unix())?;
            let key_expression = match key_expression {
                Some(key_expression) => key_expression,
                None => KeyLayout::new(&config)?.own_pattern(),
            };
            let read = if stored { Read::Stored } else { Read::Live };
            let mut index = 0;
            let mut save_failed: Option<std::io::Error> = None;
            let seen = session::watch(
                &config,
                &key_expression,
                read,
                Duration::from_secs(seconds),
                count,
                |sample| {
                    println!(
                        "{}  {}  {} bytes",
                        sample.key,
                        sample.encoding,
                        sample.payload.len()
                    );
                    if payload {
                        println!("{}", String::from_utf8_lossy(&sample.payload));
                    }
                    if let (Some(dir), None) = (&save, &save_failed) {
                        index += 1;
                        let received = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis())
                            .unwrap_or_default();
                        match crate::save::save(
                            dir,
                            &sample.key,
                            &sample.encoding,
                            &sample.payload,
                            received,
                            index,
                        ) {
                            Ok(path) => println!("  saved {}", path.display()),
                            Err(e) => save_failed = Some(e),
                        }
                    }
                },
            )?;
            if let (Some(dir), Some(e)) = (&save, save_failed) {
                eprintln!("mind goat: could not save to {}: {e}", dir.display());
                return Ok(ExitCode::FAILURE);
            }
            println!("{seen} sample(s) on {key_expression}");
            Ok(if seen > 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
    }
}

fn check(config: Option<PathBuf>, dictionary: Option<PathBuf>) -> Result<ExitCode, GoatError> {
    let (config, origin) = load(config)?;
    println!("goat configuration {origin}");
    let mut failed = 0;
    for check in checks(&config, now_unix()) {
        match check.result {
            Ok(found) => println!("  ok    {:<18} {found}", check.name),
            Err(e) => {
                failed += 1;
                println!("  FAIL  {:<18} {e}", check.name);
            }
        }
    }

    // The keys depend on the configuration's shape only, so they are shown
    // even when a certificate check failed.
    if let (Ok(layout), Ok(principal)) = (KeyLayout::new(&config), config.principal()) {
        println!();
        println!(
            "router        {} (client mode, mutual TLS, host name {})",
            config.goatnet.endpoints.join(", "),
            if config.goatnet.verify_name_on_connect {
                "verified"
            } else {
                "not verified"
            }
        );
        match principal
            .session_id()
            .filter(|_| config.identity.pin_session_id)
        {
            Some(id) => println!("session       identifier {id} (the slot UUID)"),
            None => println!("session       identifier chosen by Zenoh"),
        }
        println!();
        println!(
            "the router must grant the certificate common name {}:",
            principal.common_name()
        );
        for grant in layout.write_grants() {
            println!("  put  {grant}");
        }
        println!();
        println!("would publish:");
        println!("  announcement  {}  (at startup)", layout.announce_key());
        println!(
            "  health        {}  (every {} s)",
            layout.health_key(),
            config.publish.health_interval_seconds
        );
        match dictionary.or_else(bite::dict_path_from_env) {
            None => println!(
                "  records       {}  (pass --dictionary or set {} to list each key)",
                layout.records_pattern(),
                bite::DICT_PATH_ENV
            ),
            Some(path) => match Dictionary::load(&path) {
                Err(e) => {
                    failed += 1;
                    println!("  FAIL  dictionary {}: {e}", path.display());
                }
                Ok(dictionary) => {
                    let label =
                        dictionary_label(&dictionary).unwrap_or_else(|| path.display().to_string());
                    println!("  records of dictionary {label}, each with its schema:");
                    for interface in dictionary.interfaces() {
                        for message in interface.messages() {
                            println!("    {}/{}", interface.name, message.name);
                            println!(
                                "      record  {}",
                                layout.record_key(&interface.name, &message.name)
                            );
                            println!(
                                "      schema  {}",
                                layout.schema_key(&interface.name, &message.name)
                            );
                        }
                    }
                }
            },
        }
    }

    println!();
    if failed == 0 {
        println!("all checks passed; nothing was sent");
        Ok(ExitCode::SUCCESS)
    } else {
        println!("{failed} check(s) failed; nothing was sent");
        Ok(ExitCode::FAILURE)
    }
}
