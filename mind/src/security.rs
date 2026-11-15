//! `mind security show` and `mind security check`: what every path a record
//! can leave on will use, read from the configuration a capture run would
//! load. Neither binds a socket or sends a record.

use crate::cli::{SecurityAction, SecurityOpt};
use crate::error::Error;
use flow::security::probe::{probe, Negotiated};
use flow::security::Report;
use std::process::ExitCode;

/// Run the subcommand. `load_environment` is the capture run's `--env-file`
/// loader, so the same file names the same configuration.
pub fn run(opt: &SecurityOpt, load_environment: fn(&str) -> Result<(), Error>) -> ExitCode {
    let (common, show) = match &opt.action {
        SecurityAction::Show(show) => (&show.common, Some(show)),
        SecurityAction::Check(common) => (common, None),
    };
    if let Some(path) = &common.env_file {
        // As in a capture run, a missing env file is reported and tolerated.
        if let Err(e) = load_environment(path) {
            eprintln!("{e}");
        }
    }
    let (report, targets) = Report::load(common.activate_subpubs);
    let Some(show) = show else {
        return check(&report);
    };

    let mut probes: Vec<Negotiated> = Vec::new();
    let mut failed = false;
    let mut probe_errors = Vec::new();
    if let Some(sink) = &show.probe {
        let wanted: Vec<_> = targets.iter().filter(|t| &t.sink == sink).collect();
        if wanted.is_empty() {
            probe_errors.push(format!(
                "probe {sink}: no TLS endpoint to handshake with (the probe covers https:// \
                 endpoints and Zenoh tls/ endpoints of a subscriber that is not refused)"
            ));
        }
        for target in wanted {
            match probe(target) {
                Ok(negotiated) => probes.push(negotiated),
                Err(e) => probe_errors.push(e.to_string()),
            }
        }
        failed = !probe_errors.is_empty();
    }

    if show.json {
        let mut json = serde_json::to_value(&report).unwrap_or_default();
        if show.probe.is_some() {
            json["probes"] = serde_json::to_value(&probes).unwrap_or_default();
            json["probe_errors"] = serde_json::to_value(&probe_errors).unwrap_or_default();
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&json).unwrap_or_default()
        );
    } else {
        print!("{}", report.to_text());
        if show.probe.is_some() {
            println!();
        }
        for negotiated in &probes {
            println!("{negotiated}");
        }
        for error in &probe_errors {
            eprintln!("{error}");
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Print every refusal, in the words startup stops with, and fail if there
/// is one.
fn check(report: &Report) -> ExitCode {
    let refusals = report.refusals();
    if refusals.is_empty() {
        println!(
            "security check passed: level {} ({}), {} path(s)",
            report.level,
            report.origin,
            report.paths.len()
        );
        return ExitCode::SUCCESS;
    }
    for refusal in refusals {
        eprintln!("{refusal}");
    }
    ExitCode::FAILURE
}
