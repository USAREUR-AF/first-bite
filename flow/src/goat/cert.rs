//! Checks made before any connection: the configuration's shape, the private
//! key file, and the certificate against the configured identity, the clock
//! and the goatnet's root.
//!
//! A goat router matches the certificate's common name against its access
//! rules and says nothing when a name has no grant, so a wrong certificate is
//! far cheaper to find here than on the fabric.

use super::config::GoatConfig;
use super::GoatError;
use chrono::DateTime;
use rustls_pki_types::{CertificateDer, UnixTime};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use x509_parser::pem::Pem;

/// One check and how it went: what was found, or what to change.
#[derive(Debug)]
pub struct Check {
    pub name: &'static str,
    pub result: Result<String, GoatError>,
}

/// What the checks read from the leaf certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Leaf {
    common_name: String,
    not_before: i64,
    not_after: i64,
}

/// Seconds since the Unix epoch, now.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

fn utc(unix: i64) -> String {
    DateTime::from_timestamp(unix, 0).map_or_else(
        || format!("{unix} s after 1970"),
        |time| time.format("%Y-%m-%d %H:%M UTC").to_string(),
    )
}

/// Run every check with the clock reading `now` and report each one. A check
/// that depends on a failed one is left out.
pub fn checks(config: &GoatConfig, now: i64) -> Vec<Check> {
    let mut done = vec![Check {
        name: "configuration",
        result: describe_identity(config),
    }];
    let identity = &config.identity;
    done.push(Check {
        name: "private key",
        result: check_private_key(&identity.private_key),
    });

    let leaf_ders = read_certificates(&identity.certificate);
    let leaf = leaf_ders
        .as_ref()
        .map_err(clone_error)
        .and_then(|ders| parse_leaf(&identity.certificate, &ders[0]));
    done.push(Check {
        name: "certificate name",
        result: leaf
            .as_ref()
            .map_err(clone_error)
            .and_then(|leaf| check_common_name(config, leaf)),
    });
    if let Ok(leaf) = &leaf {
        done.push(Check {
            name: "certificate dates",
            result: check_dates(&identity.certificate, leaf, now),
        });
    }
    if let Ok(leaf_ders) = &leaf_ders {
        done.push(Check {
            name: "root certificate",
            result: check_chain(
                &identity.certificate,
                leaf_ders,
                &identity.ca_certificate,
                now,
            ),
        });
    }
    done
}

/// The first failed check, if any.
pub fn validate(config: &GoatConfig, now: i64) -> Result<(), GoatError> {
    for check in checks(config, now) {
        check.result?;
    }
    Ok(())
}

/// Errors hold an `io::Error`, which is not `Clone`; a check that reuses an
/// earlier failure reports it by its text.
fn clone_error(error: &GoatError) -> GoatError {
    GoatError::Bundle(error.to_string())
}

fn describe_identity(config: &GoatConfig) -> Result<String, GoatError> {
    config.check_shape()?;
    let principal = config.principal()?;
    let mut text = match &config.identity.slot {
        Some(_) => format!("slot {}", principal.prefix()),
        None => format!(
            "org {} (common name {})",
            principal.prefix(),
            principal.common_name()
        ),
    };
    if let Some(alias) = &config.identity.alias {
        text.push_str(&format!(", alias {alias}"));
    }
    if let Some(name) = &config.goatnet.name {
        text.push_str(&format!(", goatnet {name}"));
    }
    Ok(text)
}

/// The key must exist, hold a PEM private key, and be closed to other users.
fn check_private_key(path: &Path) -> Result<String, GoatError> {
    let bytes = fs::read(path).map_err(|source| GoatError::File {
        what: "the private key",
        path: path.to_path_buf(),
        source,
    })?;
    let holds_key = Pem::iter_from_buffer(&bytes)
        .filter_map(Result::ok)
        .any(|pem| pem.label.ends_with("PRIVATE KEY"));
    if !holds_key {
        return Err(GoatError::NotPem {
            what: "private key",
            path: path.to_path_buf(),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)
            .map_err(|source| GoatError::File {
                what: "the private key",
                path: path.to_path_buf(),
                source,
            })?
            .permissions()
            .mode()
            & 0o777;
        if mode & 0o077 != 0 {
            return Err(GoatError::KeyPermissions {
                path: path.to_path_buf(),
                mode,
            });
        }
    }
    Ok(format!("{} is readable by its owner only", path.display()))
}

/// Every certificate in a PEM file, DER-encoded, in file order.
fn read_certificates(path: &Path) -> Result<Vec<Vec<u8>>, GoatError> {
    let bytes = fs::read(path).map_err(|source| GoatError::File {
        what: "the certificate",
        path: path.to_path_buf(),
        source,
    })?;
    let certificates: Vec<Vec<u8>> = Pem::iter_from_buffer(&bytes)
        .filter_map(Result::ok)
        .filter(|pem| pem.label == "CERTIFICATE")
        .map(|pem| pem.contents)
        .collect();
    if certificates.is_empty() {
        return Err(GoatError::NotPem {
            what: "certificate",
            path: path.to_path_buf(),
        });
    }
    Ok(certificates)
}

fn parse_leaf(path: &Path, der: &[u8]) -> Result<Leaf, GoatError> {
    let bad = |reason: String| GoatError::BadCertificate {
        path: path.to_path_buf(),
        reason,
    };
    let (_, certificate) =
        x509_parser::parse_x509_certificate(der).map_err(|e| bad(e.to_string()))?;
    let common_name = certificate
        .subject()
        .iter_common_name()
        .next()
        .and_then(|attribute| attribute.as_str().ok())
        .ok_or_else(|| bad("its subject has no common name".to_string()))?
        .to_string();
    Ok(Leaf {
        common_name,
        not_before: certificate.validity().not_before.timestamp(),
        not_after: certificate.validity().not_after.timestamp(),
    })
}

fn check_common_name(config: &GoatConfig, leaf: &Leaf) -> Result<String, GoatError> {
    let principal = config.principal()?;
    if leaf.common_name == principal.common_name() {
        return Ok(format!("common name {}", leaf.common_name));
    }
    Err(GoatError::CommonName {
        path: config.identity.certificate.clone(),
        found: leaf.common_name.clone(),
        expected: principal.common_name().to_string(),
        setting: if config.identity.slot.is_some() {
            "identity.slot"
        } else {
            "identity.org and identity.cn_suffix"
        },
    })
}

fn check_dates(path: &Path, leaf: &Leaf, now: i64) -> Result<String, GoatError> {
    if now < leaf.not_before {
        return Err(GoatError::NotYetValid {
            path: path.to_path_buf(),
            not_before: utc(leaf.not_before),
            now: utc(now),
        });
    }
    if now > leaf.not_after {
        return Err(GoatError::Expired {
            path: path.to_path_buf(),
            not_after: utc(leaf.not_after),
            now: utc(now),
        });
    }
    let days = (leaf.not_after - now) / 86_400;
    Ok(format!(
        "valid until {} ({days} days from now)",
        utc(leaf.not_after)
    ))
}

/// Verify the leaf (and any intermediates that follow it in its file) up to
/// one of the roots in `ca_path`, for use as a TLS client.
fn check_chain(
    leaf_path: &Path,
    leaf_ders: &[Vec<u8>],
    ca_path: &Path,
    now: i64,
) -> Result<String, GoatError> {
    let fail = |reason: String| GoatError::Chain {
        leaf: leaf_path.to_path_buf(),
        ca: ca_path.to_path_buf(),
        reason,
    };
    let root_ders: Vec<CertificateDer> = read_certificates(ca_path)
        .map_err(|error| match error {
            GoatError::File { path, source, .. } => GoatError::File {
                what: "the root certificate",
                path,
                source,
            },
            GoatError::NotPem { path, .. } => GoatError::NotPem {
                what: "root certificate",
                path,
            },
            other => other,
        })?
        .into_iter()
        .map(CertificateDer::from)
        .collect();
    let anchors = root_ders
        .iter()
        .map(webpki::anchor_from_trusted_cert)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| GoatError::BadCertificate {
            path: PathBuf::from(ca_path),
            reason: e.to_string(),
        })?;
    let chain: Vec<CertificateDer> = leaf_ders
        .iter()
        .map(|der| CertificateDer::from(der.as_slice()))
        .collect();
    let leaf = webpki::EndEntityCert::try_from(&chain[0]).map_err(|e| fail(e.to_string()))?;
    leaf.verify_for_usage(
        webpki::ALL_VERIFICATION_ALGS,
        &anchors,
        &chain[1..],
        UnixTime::since_unix_epoch(Duration::from_secs(now.max(0) as u64)),
        webpki::KeyUsage::client_auth(),
        None,
        None,
    )
    .map_err(|e| {
        fail(match e {
            webpki::Error::UnknownIssuer => "it was issued by a different authority".to_string(),
            webpki::Error::CertExpired { .. } | webpki::Error::CertNotValidYet { .. } => format!(
                "a certificate in the chain is outside its validity period at {}",
                utc(now)
            ),
            other => other.to_string(),
        })
    })?;
    Ok(format!("{} verifies the certificate", ca_path.display()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::config::tests::{slot_config, SLOT};
    use super::*;
    use crate::test_support::temp_path;

    /// 2027-01-01 00:00 UTC: inside the validity of the fixture certificates,
    /// which run from 2026-11 for a hundred years.
    pub(crate) const NOW: i64 = 1_798_761_600;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/goat")
            .join(name)
    }

    /// A file shaped like a PEM private key; nothing parses the body.
    pub(crate) fn key_file(mode: u32) -> PathBuf {
        let path = temp_path("goat-key.pem");
        let label = format!("{} KEY", "PRIVATE");
        fs::write(
            &path,
            format!("-----BEGIN {label}-----\nAAAA\n-----END {label}-----\n"),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        }
        path
    }

    /// The slot configuration pointed at the fixture certificates and a
    /// throwaway key file.
    pub(crate) fn fixture_config() -> GoatConfig {
        let mut config = slot_config(Path::new("/x"));
        config.identity.certificate = fixture("slot-cert.pem");
        config.identity.ca_certificate = fixture("ca-root.pem");
        config.identity.private_key = key_file(0o600);
        config
    }

    fn failure(config: &GoatConfig, now: i64) -> String {
        validate(config, now).unwrap_err().to_string()
    }

    #[test]
    fn a_matching_certificate_key_and_root_pass_every_check() {
        let config = fixture_config();
        let done = checks(&config, NOW);
        let names: Vec<&str> = done.iter().map(|check| check.name).collect();
        assert_eq!(
            names,
            [
                "configuration",
                "private key",
                "certificate name",
                "certificate dates",
                "root certificate"
            ]
        );
        for check in &done {
            assert!(check.result.is_ok(), "{}: {:?}", check.name, check.result);
        }
        assert_eq!(
            done[0].result.as_ref().unwrap(),
            &format!("slot {SLOT}, alias alpha-operator, goatnet zagros")
        );
        assert_eq!(
            done[2].result.as_ref().unwrap(),
            &format!("common name {SLOT}")
        );
    }

    #[test]
    fn an_org_certificate_passes_with_the_org_configured() {
        let mut config = fixture_config();
        config.identity.slot = None;
        config.identity.org = Some("example-vendor".to_string());
        config.identity.certificate = fixture("org-cert.pem");
        validate(&config, NOW).unwrap();
    }

    #[test]
    fn a_certificate_for_another_identity_is_refused_with_both_names() {
        let mut config = fixture_config();
        config.identity.certificate = fixture("org-cert.pem");
        let error = failure(&config, NOW);
        assert!(
            error.contains("has common name \"example-vendor-trial-peer\"")
                && error.contains(&format!(
                    "the configuration names \"{SLOT}\" (identity.slot)"
                )),
            "{error}"
        );
    }

    #[test]
    fn dates_are_checked_against_the_given_clock() {
        let config = fixture_config();
        // 2020-01-01 and 2200-01-01.
        let before = failure(&config, 1_577_836_800);
        assert!(
            before.contains("is not valid until 2026-") && before.contains("reads 2020-01-01"),
            "{before}"
        );
        let after = failure(&config, 7_258_118_400);
        assert!(
            after.contains("expired at 21") && after.contains("issue a new one"),
            "{after}"
        );
    }

    #[test]
    fn a_root_that_did_not_issue_the_certificate_is_refused() {
        let mut config = fixture_config();
        config.identity.ca_certificate = fixture("other-ca-root.pem");
        let error = failure(&config, NOW);
        assert!(
            error.contains("is not verified by the root")
                && error.contains("it was issued by a different authority")
                && error.contains("same onboarding archive"),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_key_other_users_can_read_is_refused_with_the_command() {
        let mut config = fixture_config();
        config.identity.private_key = key_file(0o644);
        let error = failure(&config, NOW);
        assert!(
            error.contains("can be read by other users (mode 644); run: chmod 600 "),
            "{error}"
        );
    }

    #[test]
    fn files_that_are_missing_or_hold_something_else_are_named() {
        let mut config = fixture_config();
        config.identity.private_key = fixture("slot-cert.pem");
        assert!(failure(&config, NOW).contains("holds no private key"));

        let mut config = fixture_config();
        config.identity.certificate = PathBuf::from("/nonexistent/cert.pem");
        assert!(
            failure(&config, NOW).starts_with("cannot read the certificate /nonexistent/cert.pem")
        );

        let mut config = fixture_config();
        config.identity.ca_certificate = PathBuf::from("/nonexistent/ca.pem");
        assert!(failure(&config, NOW)
            .starts_with("cannot read the root certificate /nonexistent/ca.pem"));

        let mut config = fixture_config();
        config.identity.certificate = config.identity.private_key.clone();
        assert!(failure(&config, NOW).contains("holds no certificate"));
    }

    #[test]
    fn a_malformed_uuid_stops_before_any_file_is_read() {
        let mut config = fixture_config();
        config.identity.slot = Some(SLOT.to_uppercase());
        assert!(failure(&config, NOW).starts_with("identity.slot"));
    }
}
