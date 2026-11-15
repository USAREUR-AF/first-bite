//! The version this build is released under, as the binary and the records
//! report it, and the `security` subcommand as a process.

use std::process::Command;

/// The release this tree is. Change it with `version` in `mind/Cargo.toml`.
const RELEASE: &str = "1.1.0";

fn mind() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mind"));
    for variable in [
        "PUBLISHER_CONFIG_PATH",
        "SUBSCRIBER_CONFIG_PATH",
        "SUBPUB_CONFIG_PATH",
        "SECURITY_CONFIG_PATH",
    ] {
        command.env_remove(variable);
    }
    command
}

#[test]
fn the_binary_and_the_records_report_the_release() {
    assert_eq!(env!("CARGO_PKG_VERSION"), RELEASE);
    let output = mind().arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("mind {RELEASE}")
    );
    // The record's `application_version` and the goat announcement's
    // `version` are this constant: `tests/replay.rs` reads the first back
    // from a record, and the tests of `src/goat/schema.rs` the second.
}

#[test]
fn security_check_passes_with_no_configuration() {
    let output = mind().args(["security", "check"]).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "security check passed: level unenforced (no security block), 1 path(s)"
    );
}

#[test]
fn security_show_reports_the_provider_as_json() {
    let output = mind()
        .args(["security", "show", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["level"], "unenforced");
    assert_eq!(report["provider"]["name"], "aws-lc-rs");
    assert_eq!(
        report["paths"][0]["posture"]["endpoint"],
        "tcp://127.0.0.1:12000"
    );
}
