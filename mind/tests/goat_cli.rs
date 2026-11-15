//! `mind goat …` beside the unchanged `mind <interface> …`.

use clap::Parser;
use mind::cli::Opt;
use std::process::Command;

fn mind() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mind"))
}

#[test]
fn interfaces_are_still_positional_arguments() {
    let opt = Opt::try_parse_from(["mind", "eth0", "eth1", "--filter", "udp"]).unwrap();
    assert_eq!(opt.device_name, ["eth0", "eth1"]);
    assert_eq!(opt.filter.as_deref(), Some("udp"));
    // An interface that shares the command's name is given after `--`.
    let opt = Opt::try_parse_from(["mind", "--", "goat"]).unwrap();
    assert_eq!(opt.device_name, ["goat"]);
}

/// A first argument other than `goat` reaches the capture path: with no
/// source it stops with that path's own message.
#[test]
fn other_invocations_reach_the_capture_path() {
    let output = mind().env_remove("BITE_DICT_PATH").output().unwrap();
    assert!(!output.status.success());
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(
        text.contains("give a network interface, --pcap-file or --mock"),
        "{text}"
    );
}

#[cfg(not(feature = "goat"))]
#[test]
fn without_the_feature_the_command_says_how_to_build_with_it() {
    let output = mind().args(["goat", "check"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "this mind was built without goat support; build one with it: \
         cargo build --release -p mind --features goat"
    );
    assert!(output.stdout.is_empty());
}

#[cfg(feature = "goat")]
mod with_the_feature {
    use super::mind;
    use std::fs;
    use std::path::{Path, PathBuf};

    const SLOT: &str = "e7a9c75b532e3d434bb09fefd3ce1ec7";

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../flow/tests/fixtures/goat")
            .join(name)
    }

    /// A directory with a configuration that points at the fixture
    /// certificates and a key file of the given mode.
    fn setup(name: &str, slot: &str, key_mode: u32) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mind-goat-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let label = format!("{} KEY", "PRIVATE");
        let key = dir.join("key.pem");
        fs::write(
            &key,
            format!("-----BEGIN {label}-----\nAAAA\n-----END {label}-----\n"),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&key, fs::Permissions::from_mode(key_mode)).unwrap();
        }
        fs::write(
            dir.join("goat.yml"),
            format!(
                "goatnet:\n  name: zagros\n  endpoints: [\"tls/zenoh.goat.example:7447\"]\n\
                 identity:\n  certificate: \"{}\"\n  private_key: key.pem\n  ca_certificate: \"{}\"\n  \
                 slot: {slot}\n  alias: alpha-operator\n\
                 publish:\n  format: weather-station\n  site: hilltop-7\n  instance: tap-1\n",
                fixture("slot-cert.pem").display(),
                fixture("ca-root.pem").display(),
            ),
        )
        .unwrap();
        dir
    }

    fn run(arguments: &[&str], config: &Path) -> (Option<i32>, String) {
        let output = mind()
            .args(arguments)
            .env("GOAT_CONFIG_PATH", config.join("goat.yml"))
            .env_remove("BITE_DICT_PATH")
            .env_remove("FAAD_DICT_PATH")
            .output()
            .unwrap();
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).to_string()
                + &String::from_utf8_lossy(&output.stderr),
        )
    }

    #[test]
    fn check_passes_and_lists_every_key_of_the_dictionary() {
        let dir = setup("check", SLOT, 0o600);
        let dictionary =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../dictionaries/weather-station");
        let (code, text) = run(
            &[
                "goat",
                "check",
                "--dictionary",
                dictionary.to_str().unwrap(),
            ],
            &dir,
        );
        assert_eq!(code, Some(0), "{text}");
        for expected in [
            format!("ok    configuration      slot {SLOT}, alias alpha-operator, goatnet zagros"),
            format!("ok    certificate name   common name {SLOT}"),
            "ok    root certificate".to_string(),
            format!("the router must grant the certificate common name {SLOT}:"),
            format!("  put  {SLOT}/**"),
            "  put  attest/schema/**".to_string(),
            format!("  announcement  {SLOT}/services/shims/weather-station/tap-1/announce"),
            format!("  health        {SLOT}/health/shims/weather-station/tap-1  (every 30 s)"),
            format!("      record  {SLOT}/telemetry/weather-station/hilltop-7/wx/observation"),
            format!("      schema  attest/schema/{SLOT}/telemetry/weather-station/hilltop-7/wx/observation"),
            "all checks passed; nothing was sent".to_string(),
        ] {
            assert!(text.contains(&expected), "missing {expected:?} in:\n{text}");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn check_fails_with_the_fix_for_each_wrong_input() {
        let other = "0123456789abcdef0123456789abcdef";
        let dir = setup("wrong-name", other, 0o600);
        let (code, text) = run(&["goat", "check"], &dir);
        assert_eq!(code, Some(1), "{text}");
        assert!(
            text.contains(&format!(
                "FAIL  certificate name   the certificate {} has common name \"{SLOT}\" but the configuration names \"{other}\" (identity.slot)",
                fixture("slot-cert.pem").display()
            )),
            "{text}"
        );
        assert!(
            text.contains("1 check(s) failed; nothing was sent"),
            "{text}"
        );
        fs::remove_dir_all(dir).unwrap();

        #[cfg(unix)]
        {
            let dir = setup("open-key", SLOT, 0o644);
            let (code, text) = run(&["goat", "check"], &dir);
            assert_eq!(code, Some(1), "{text}");
            assert!(
                text.contains("FAIL  private key") && text.contains("run: chmod 600"),
                "{text}"
            );
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn config_prints_the_zenoh_client_configuration() {
        let dir = setup("config", SLOT, 0o600);
        let (code, text) = run(&["goat", "config"], &dir);
        assert_eq!(code, Some(0), "{text}");
        let client: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(client["mode"], "client");
        assert_eq!(client["id"], SLOT);
        assert_eq!(
            client["connect"]["endpoints"][0],
            "tls/zenoh.goat.example:7447"
        );
        let tls = &client["transport"]["link"]["tls"];
        assert_eq!(tls["enable_mtls"], true);
        assert_eq!(tls["verify_name_on_connect"], true);
        assert_eq!(
            tls["connect_private_key"],
            dir.join("key.pem").to_str().unwrap()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn init_writes_a_configuration_that_check_reads() {
        let dir = setup("init", SLOT, 0o600);
        let bundle = dir.join("bundle");
        fs::create_dir_all(&bundle).unwrap();
        fs::copy(
            fixture("slot-cert.pem"),
            bundle.join(format!("{SLOT}-cert.pem")),
        )
        .unwrap();
        fs::copy(dir.join("key.pem"), bundle.join(format!("{SLOT}-key.pem"))).unwrap();
        fs::copy(fixture("ca-root.pem"), bundle.join("ca-root.pem")).unwrap();
        fs::write(
            bundle.join("NOTES.md"),
            "Router endpoint:     tls/zenoh.goat.example:7447\n",
        )
        .unwrap();
        let written = dir.join("written");
        fs::create_dir_all(&written).unwrap();
        let out = written.join("goat.yml");
        let (code, text) = run(
            &[
                "goat",
                "init",
                "--bundle",
                bundle.to_str().unwrap(),
                "--format",
                "weather-station",
                "--site",
                "hilltop-7",
                "--out",
                out.to_str().unwrap(),
            ],
            &dir,
        );
        assert_eq!(code, Some(0), "{text}");
        let (code, text) = run(&["goat", "check"], &written);
        assert_eq!(code, Some(0), "{text}");
        assert!(
            text.contains(&format!("{SLOT}/health/shims/weather-station/mind-1")),
            "{text}"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn watch_refuses_the_whole_keyspace_before_connecting() {
        let dir = setup("watch", SLOT, 0o600);
        let (code, text) = run(&["goat", "watch", "**"], &dir);
        assert_eq!(code, Some(1), "{text}");
        assert!(text.contains("\"**\" names the whole keyspace"), "{text}");
        fs::remove_dir_all(dir).unwrap();
    }
}
