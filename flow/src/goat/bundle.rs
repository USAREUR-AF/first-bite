//! A goat configuration from the onboarding archive a goatnet's portal
//! issues. The archive's layout is fixed by goat's
//! `ops/goat-zenoh-portal/internal/certmint/certmint.go`:
//! `<prefix>-cert.pem`, `<prefix>-key.pem`, the root certificate and a
//! `NOTES.md` with a `Router endpoint:` line.

use super::config::GoatConfig;
use super::identity::is_slot_uuid;
use super::GoatError;
use std::fs;
use std::path::{Path, PathBuf};

/// What the caller supplies because the archive does not hold it.
#[derive(Debug, Clone)]
pub struct Naming {
    pub format: String,
    pub site: String,
    pub instance: String,
    /// Overrides the endpoint in `NOTES.md`.
    pub endpoint: Option<String>,
}

fn problem(text: String) -> GoatError {
    GoatError::Bundle(text)
}

/// The files of the unpacked archive in `dir` whose names end with `suffix`.
fn ending(names: &[String], suffix: &str) -> Vec<String> {
    names
        .iter()
        .filter(|name| name.ends_with(suffix))
        .cloned()
        .collect()
}

/// Write the text of a goat configuration for the unpacked archive in `dir`.
/// The certificate paths in it are absolute.
pub fn configuration(dir: &Path, naming: &Naming) -> Result<String, GoatError> {
    let dir = dir.canonicalize().map_err(|source| GoatError::File {
        what: "the onboarding archive directory",
        path: dir.to_path_buf(),
        source,
    })?;
    let mut names: Vec<String> = fs::read_dir(&dir)
        .map_err(|source| GoatError::File {
            what: "the onboarding archive directory",
            path: dir.clone(),
            source,
        })?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    let shown = dir.display();

    let certificates = ending(&names, "-cert.pem");
    let [certificate] = certificates.as_slice() else {
        return Err(problem(format!(
            "{shown} holds {} files named <prefix>-cert.pem; unpack one onboarding archive \
             (tar -xzf goat-onboarding-<prefix>-<stamp>.tar.gz) into an empty directory",
            certificates.len()
        )));
    };
    let prefix = certificate.trim_end_matches("-cert.pem");
    let key = format!("{prefix}-key.pem");
    if !names.contains(&key) {
        return Err(problem(format!(
            "{shown} has {certificate} but not {key}; the archive is incomplete"
        )));
    }
    let roots: Vec<&String> = names
        .iter()
        .filter(|name| {
            name.ends_with(".pem")
                && !name.ends_with("-cert.pem")
                && !name.ends_with("-key.pem")
                && !name.ends_with("-goat-cli-ca.pem")
        })
        .collect();
    let [root] = roots.as_slice() else {
        return Err(problem(format!(
            "{shown} holds {} candidates for the goatnet's root certificate (a .pem file that \
             is not the certificate or the key); expected one, named ca-root.pem by default",
            roots.len()
        )));
    };

    let endpoint = match &naming.endpoint {
        Some(endpoint) => endpoint.clone(),
        None => notes_endpoint(&dir).ok_or_else(|| {
            problem(format!(
                "{shown}/NOTES.md gives no router endpoint; pass --endpoint tls/<host>:<port> \
                 with the address the goatnet's administrator gave"
            ))
        })?,
    };

    let identity = if is_slot_uuid(prefix) {
        format!("  slot: \"{prefix}\"")
    } else {
        format!("  org: {prefix}")
    };
    let path = |name: &str| -> PathBuf { dir.join(name) };
    let text = format!(
        "# first-bite configuration for one goatnet identity.\n\
         # Written by `mind goat init`; check it with `mind goat check`.\n\
         goatnet:\n  endpoints: [\"{endpoint}\"]\n\
         identity:\n  certificate: \"{}\"\n  private_key: \"{}\"\n  ca_certificate: \"{}\"\n{identity}\n\
         publish:\n  format: {}\n  site: {}\n  instance: {}\n",
        path(certificate).display(),
        path(&key).display(),
        path(root).display(),
        naming.format,
        naming.site,
        naming.instance,
    );
    // What is written must load and pass the checks that need no certificate.
    GoatConfig::parse(&text, "the generated configuration", &dir)?.check_shape()?;
    Ok(text)
}

/// The endpoint on the `Router endpoint:` line of `NOTES.md`, unless it is
/// the placeholder goat writes when the portal has none configured.
fn notes_endpoint(dir: &Path) -> Option<String> {
    let notes = fs::read_to_string(dir.join("NOTES.md")).ok()?;
    notes
        .lines()
        .find_map(|line| line.trim().strip_prefix("Router endpoint:"))
        .map(|value| value.trim().to_string())
        .filter(|value| value.starts_with("tls/"))
}

#[cfg(test)]
mod tests {
    use super::super::config::tests::SLOT;
    use super::*;
    use crate::test_support::temp_path;

    fn naming() -> Naming {
        Naming {
            format: "weather-station".to_string(),
            site: "hilltop-7".to_string(),
            instance: "tap-1".to_string(),
            endpoint: None,
        }
    }

    /// An unpacked archive: empty files with the names goat gives them.
    fn archive(prefix: &str, notes: Option<&str>) -> PathBuf {
        let dir = temp_path("goat-bundle");
        fs::create_dir_all(&dir).unwrap();
        for name in [
            format!("{prefix}-cert.pem"),
            format!("{prefix}-key.pem"),
            "ca-root.pem".to_string(),
            format!("{prefix}-goat-cli-ca.pem"),
            "first-publisher.py".to_string(),
        ] {
            fs::write(dir.join(name), "").unwrap();
        }
        if let Some(notes) = notes {
            fs::write(dir.join("NOTES.md"), notes).unwrap();
        }
        dir
    }

    const NOTES: &str = "# Vendor notes\n\nYour vendor prefix: x\nRouter endpoint:     tls/zenoh.goat.example:7447\nPortal:              https://portal.goat.example\n";

    #[test]
    fn a_slot_archive_gives_a_slot_configuration() {
        let dir = archive(SLOT, Some(NOTES));
        let text = configuration(&dir, &naming()).unwrap();
        let config = GoatConfig::parse(&text, "test", &dir).unwrap();
        assert_eq!(config.identity.slot.as_deref(), Some(SLOT));
        assert_eq!(config.goatnet.endpoints, ["tls/zenoh.goat.example:7447"]);
        let dir = dir.canonicalize().unwrap();
        assert_eq!(
            config.identity.certificate,
            dir.join(format!("{SLOT}-cert.pem"))
        );
        assert_eq!(
            config.identity.private_key,
            dir.join(format!("{SLOT}-key.pem"))
        );
        assert_eq!(config.identity.ca_certificate, dir.join("ca-root.pem"));
        assert_eq!(config.publish.format, "weather-station");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_org_archive_gives_an_org_configuration() {
        let dir = archive("example-vendor", Some(NOTES));
        let text = configuration(&dir, &naming()).unwrap();
        assert!(text.contains("\n  org: example-vendor\n"), "{text}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_endpoint_comes_from_the_notes_or_from_the_caller() {
        let placeholder = "Router endpoint:     <router-endpoint, ask operator>\n";
        let dir = archive(SLOT, Some(placeholder));
        let error = configuration(&dir, &naming()).unwrap_err().to_string();
        assert!(
            error.contains("pass --endpoint tls/<host>:<port>"),
            "{error}"
        );
        let given = Naming {
            endpoint: Some("tls/192.0.2.10:7447".to_string()),
            ..naming()
        };
        assert!(configuration(&dir, &given)
            .unwrap()
            .contains("endpoints: [\"tls/192.0.2.10:7447\"]"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_incomplete_archive_says_what_is_missing() {
        let dir = archive(SLOT, Some(NOTES));
        fs::remove_file(dir.join(format!("{SLOT}-key.pem"))).unwrap();
        let error = configuration(&dir, &naming()).unwrap_err().to_string();
        assert!(
            error.contains(&format!("but not {SLOT}-key.pem")),
            "{error}"
        );
        fs::remove_file(dir.join(format!("{SLOT}-cert.pem"))).unwrap();
        let error = configuration(&dir, &naming()).unwrap_err().to_string();
        assert!(
            error.contains("holds 0 files named <prefix>-cert.pem"),
            "{error}"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn names_that_cannot_be_key_chunks_are_refused_before_anything_is_written() {
        let dir = archive(SLOT, Some(NOTES));
        let wrong = Naming {
            site: "Hilltop 7".to_string(),
            ..naming()
        };
        let error = configuration(&dir, &wrong).unwrap_err().to_string();
        assert!(error.contains("write it as hilltop_7"), "{error}");
        fs::remove_dir_all(dir).unwrap();
    }
}
