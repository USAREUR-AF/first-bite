//! TLS for the sinks that speak HTTP: REST, Azure Blob Storage and S3.
//!
//! All three get their HTTP client from [`secure`], so one function decides
//! the provider, the protocol versions, the key-exchange groups, the trusted
//! authorities and the client certificate of every HTTP connection.

use super::config::{Fips, MinTls, Transport};
use super::posture::PathPosture;
use super::probe::ProbeTarget;
use super::{is_loopback_host, provider, Policy};
use crate::error::Problem;
use reqwest::Url;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore};
use std::path::Path;
use std::sync::Arc;

/// The authorities a sink trusts when no `ca_bundle` is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Roots {
    /// The Mozilla root programme, compiled in.
    BuiltIn,
    /// The machine's certificate store.
    #[cfg_attr(not(feature = "aws"), allow(dead_code))]
    System,
}

/// What a sink states about itself.
pub(crate) struct HttpSink<'a> {
    /// The subscriber's name.
    pub(crate) name: &'a str,
    /// `rest`, `objectstorage/azureobject`, `objectstorage/awsobject`.
    pub(crate) kind: &'static str,
    /// The configuration key that locates the endpoint.
    pub(crate) key: &'static str,
    /// The endpoint: scheme, host, port and any path.
    pub(crate) url: &'a str,
    pub(crate) roots: Roots,
    /// The key exchange this sink used before the security block existed,
    /// kept while no block is written; `None` for the classical groups.
    pub(crate) unstated_key_exchange: Option<super::KeyExchange>,
    /// Why the service cannot take a client certificate, when it cannot.
    pub(crate) no_client_certificate: Option<&'static str>,
    /// Cryptography the sink performs outside the provider, when it does.
    pub(crate) outside_provider: Option<&'static str>,
}

/// The endpoint without credentials or query.
fn display_url(url: &Url) -> String {
    let mut shown = url.clone();
    let _ = shown.set_username("");
    let _ = shown.set_password(None);
    shown.set_query(None);
    shown.set_fragment(None);
    shown.to_string()
}

fn refuse(message: String) -> Problem {
    Problem::Security(message)
}

/// A sink's endpoint, checked, with the TLS configuration built for it.
pub(crate) struct Secured {
    tls: Arc<ClientConfig>,
    https_only: bool,
    /// What the sink will use, for the posture report.
    pub(crate) posture: PathPosture,
    /// What a probe needs; present for an HTTPS endpoint.
    pub(crate) target: Option<ProbeTarget>,
}

impl Secured {
    /// An HTTP client builder that carries the TLS configuration.
    pub(crate) fn client_builder(&self) -> reqwest::ClientBuilder {
        let builder = reqwest::Client::builder().use_preconfigured_tls((*self.tls).clone());
        // Under a level that requires TLS a redirect may not leave it either.
        builder.https_only(self.https_only)
    }
}

/// Check the endpoint against the level and build the TLS configuration.
pub(crate) fn secure(sink: &HttpSink<'_>, policy: &Policy) -> Result<Secured, Problem> {
    let key = sink.key;
    let url = Url::parse(sink.url).map_err(|e| Problem::InvalidValue {
        key,
        // The text is not quoted: it may hold a password.
        reason: format!("the endpoint is not a URL: {e}"),
    })?;
    let shown = display_url(&url);
    let host = url.host_str().unwrap_or("").to_string();
    let https = url.scheme() == "https";
    let loopback = is_loopback_host(&host);
    // A service that takes no client certificate is never offered one.
    let identity = policy
        .identity()
        .filter(|_| sink.no_client_certificate.is_none());
    let has_identity = identity.is_some();

    match policy.transport {
        Transport::Unenforced => {}
        Transport::LoopbackPlaintext => {
            if !loopback {
                return Err(refuse(format!(
                    "level `loopback-plaintext` refuses a sink that leaves this host; `{key}` \
                     gives {shown}"
                )));
            }
        }
        Transport::Tls | Transport::Mtls => {
            if !https {
                let hint = if loopback {
                    "; for a local emulator give this subscriber `security: { transport: \
                     loopback-plaintext, reason: ... }`"
                } else {
                    ""
                };
                return Err(refuse(format!(
                    "level `{}` needs an https:// endpoint; `{key}` gives {shown}{hint}",
                    policy.transport
                )));
            }
        }
    }
    if policy.transport == Transport::Mtls {
        if let Some(why) = sink.no_client_certificate {
            return Err(refuse(format!(
                "level `mtls` needs a client certificate, and {why}; to accept server-only TLS \
                 for this sink give it `security: {{ transport: tls, reason: ... }}`"
            )));
        }
        if !has_identity {
            return Err(refuse(
                "level `mtls` needs `client_cert` and `client_key`, in the security block or in \
                 this subscriber's `security`"
                    .to_string(),
            ));
        }
    }
    if policy.fips == Fips::Required {
        if let Some(what) = sink.outside_provider {
            return Err(refuse(format!(
                "`fips: required`: {what} runs outside the FIPS module; to accept that for this \
                 sink give it `security: {{ fips: off, reason: ... }}`"
            )));
        }
    }

    let key_exchange = match sink.unstated_key_exchange {
        Some(before) if !policy.stated => before,
        _ => policy.key_exchange,
    };
    let (store, trust) = root_store(policy.ca_bundle.as_deref(), sink.roots)?;
    let config = tls_config(
        policy.min_tls,
        key_exchange,
        store,
        identity,
        ("security.client_cert", "security.client_key"),
    )?;
    if policy.fips == Fips::Required && !config.fips() {
        return Err(refuse(
            "`fips: required`: the TLS configuration built for this sink is not in FIPS mode"
                .to_string(),
        ));
    }
    let tls_fips = config.fips();
    let config = Arc::new(config);

    let mut posture = PathPosture::local(sink.name, sink.kind, &shown, policy.transport);
    posture.off_host = !loopback;
    posture.protocol = "HTTP/1.1 over TCP".to_string();
    posture.overrides = policy.overrides.iter().map(ToString::to_string).collect();
    posture.override_reason = policy.reason.clone();
    if let Some(what) = sink.outside_provider {
        posture
            .notes
            .push(format!("{what} runs outside the crypto provider"));
    }
    if https {
        posture.transport = if has_identity { "mutual TLS" } else { "TLS" }.to_string();
        posture.min_tls = Some(policy.min_tls.to_string());
        posture.key_exchange = Some(key_exchange);
        posture.groups = provider::group_names(key_exchange);
        posture.client_auth = match identity {
            Some((cert, _)) => format!(
                "certificate {} (presented when the server asks)",
                cert.display()
            ),
            None => "none".to_string(),
        };
        posture.trust = Some(trust);
        posture.fips = Some(tls_fips);
    } else {
        posture.transport = "plaintext".to_string();
        if has_identity {
            posture.notes.push(
                "a client certificate is configured but the endpoint is not HTTPS".to_string(),
            );
        }
    }

    let target = https.then(|| ProbeTarget {
        sink: sink.name.to_string(),
        endpoint: shown.clone(),
        host: host.clone(),
        port: url.port_or_known_default().unwrap_or(443),
        config: Arc::clone(&config),
    });
    Ok(Secured {
        tls: config,
        https_only: policy.transport.requires_tls(),
        posture,
        target,
    })
}

/// A rustls client configuration on the one provider.
pub(crate) fn tls_config(
    min_tls: MinTls,
    key_exchange: super::KeyExchange,
    roots: RootCertStore,
    identity: Option<(&Path, &Path)>,
    identity_keys: (&'static str, &'static str),
) -> Result<ClientConfig, Problem> {
    let provider = Arc::new(provider::build(min_tls, key_exchange));
    let builder = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&provider::versions(min_tls))
        .map_err(|e| refuse(format!("cannot build the TLS configuration: {e}")))?
        .with_root_certificates(roots);
    match identity {
        None => Ok(builder.with_no_client_auth()),
        Some((cert, key)) => {
            let chain = read_certificates(identity_keys.0, cert)?;
            let key_der = PrivateKeyDer::from_pem_file(key).map_err(|e| Problem::InvalidValue {
                key: identity_keys.1,
                reason: format!("cannot read a private key from {}: {e}", key.display()),
            })?;
            builder
                .with_client_auth_cert(chain, key_der)
                .map_err(|e| Problem::InvalidValue {
                    key: identity_keys.1,
                    reason: format!(
                        "{} is not a usable key for {}: {e}",
                        key.display(),
                        cert.display()
                    ),
                })
        }
    }
}

/// Every certificate in a PEM file; an unreadable or empty file is an error
/// that names the key and the path.
pub(crate) fn read_certificates(
    key: &'static str,
    path: &Path,
) -> Result<Vec<CertificateDer<'static>>, Problem> {
    let invalid = |reason: String| Problem::InvalidValue { key, reason };
    let certificates = CertificateDer::pem_file_iter(path)
        .map_err(|e| invalid(format!("cannot read {}: {e}", path.display())))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| invalid(format!("cannot read {}: {e}", path.display())))?;
    if certificates.is_empty() {
        return Err(invalid(format!(
            "{} holds no PEM certificate",
            path.display()
        )));
    }
    Ok(certificates)
}

/// The authorities to trust: the bundle when one is given, else the sink's
/// usual roots.
pub(crate) fn root_store(
    ca_bundle: Option<&Path>,
    roots: Roots,
) -> Result<(RootCertStore, String), Problem> {
    let mut store = RootCertStore::empty();
    if let Some(path) = ca_bundle {
        for certificate in read_certificates("security.ca_bundle", path)? {
            store.add(certificate).map_err(|e| Problem::InvalidValue {
                key: "security.ca_bundle",
                reason: format!("{}: {e}", path.display()),
            })?;
        }
        return Ok((store, format!("bundle {}", path.display())));
    }
    match roots {
        Roots::BuiltIn => {
            store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            Ok((store, "built-in web roots".to_string()))
        }
        Roots::System => {
            #[cfg(feature = "aws")]
            {
                let loaded = rustls_native_certs::load_native_certs();
                let (added, _ignored) = store.add_parsable_certificates(loaded.certs);
                if added == 0 {
                    log::warn!(
                        "the machine's certificate store holds no usable root certificate; an \
                         HTTPS endpoint checked against it will be refused"
                    );
                }
            }
            Ok((store, "the machine's certificate store".to_string()))
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::security::tests::security;
    use crate::security::{KeyExchange, Security};
    use crate::test_support::temp_path;

    fn sink(url: &str) -> HttpSink<'_> {
        HttpSink {
            name: "Example",
            kind: "rest",
            key: "endpoint_url",
            url,
            roots: Roots::BuiltIn,
            unstated_key_exchange: None,
            no_client_certificate: None,
            outside_provider: None,
        }
    }

    fn outcome(url: &str, block: Option<&str>) -> Result<PathPosture, String> {
        let policy = match block {
            Some(block) => security(block).base,
            None => Security::unstated().base,
        };
        secure(&sink(url), &policy)
            .map(|secured| secured.posture)
            .map_err(|problem| problem.to_string())
    }

    /// Level by endpoint: what each level does with each kind of endpoint.
    #[test]
    fn each_level_against_each_endpoint() {
        const REMOTE_HTTP: &str = "http://203.0.113.10:5001/records";
        const REMOTE_HTTPS: &str = "https://collector.example:8443/records";
        const LOCAL_HTTP: &str = "http://127.0.0.1:10000/devstoreaccount1";
        const LOCAL_HTTPS: &str = "https://localhost:8443/";
        // (level, endpoint) -> accepted, or the refusal starts with
        type Case = (
            Option<&'static str>,
            &'static str,
            Result<&'static str, &'static str>,
        );
        let cases: [Case; 16] = [
            (None, REMOTE_HTTP, Ok("plaintext")),
            (None, REMOTE_HTTPS, Ok("TLS")),
            (None, LOCAL_HTTP, Ok("plaintext")),
            (None, LOCAL_HTTPS, Ok("TLS")),
            (
                Some("loopback-plaintext"),
                REMOTE_HTTP,
                Err("level `loopback-plaintext` refuses a sink that leaves this host; `endpoint_url` gives http://203.0.113.10:5001/records"),
            ),
            (
                Some("loopback-plaintext"),
                REMOTE_HTTPS,
                Err("level `loopback-plaintext` refuses a sink that leaves this host"),
            ),
            (Some("loopback-plaintext"), LOCAL_HTTP, Ok("plaintext")),
            (Some("loopback-plaintext"), LOCAL_HTTPS, Ok("TLS")),
            (
                Some("tls"),
                REMOTE_HTTP,
                Err("level `tls` needs an https:// endpoint; `endpoint_url` gives http://203.0.113.10:5001/records"),
            ),
            (Some("tls"), REMOTE_HTTPS, Ok("TLS")),
            (
                Some("tls"),
                LOCAL_HTTP,
                Err("level `tls` needs an https:// endpoint; `endpoint_url` gives http://127.0.0.1:10000/devstoreaccount1; for a local emulator give this subscriber `security: { transport: loopback-plaintext, reason: ... }`"),
            ),
            (Some("tls"), LOCAL_HTTPS, Ok("TLS")),
            (
                Some("mtls"),
                REMOTE_HTTP,
                Err("level `mtls` needs an https:// endpoint"),
            ),
            (
                Some("mtls"),
                REMOTE_HTTPS,
                Err("level `mtls` needs `client_cert` and `client_key`"),
            ),
            (Some("mtls"), LOCAL_HTTP, Err("level `mtls` needs an https:// endpoint")),
            (
                Some("mtls"),
                LOCAL_HTTPS,
                Err("level `mtls` needs `client_cert` and `client_key`"),
            ),
        ];
        for (level, url, expected) in cases {
            let block = level.map(|level| format!("  transport: {level}\n"));
            let got = outcome(url, block.as_deref());
            match (got, expected) {
                (Ok(posture), Ok(transport)) => {
                    assert_eq!(posture.transport, transport, "{level:?} {url}")
                }
                (Err(message), Err(start)) => {
                    assert!(message.starts_with(start), "{level:?} {url}: {message}")
                }
                (got, expected) => panic!("{level:?} {url}: {got:?}, expected {expected:?}"),
            }
        }
    }

    /// With no block a sink keeps the key exchange it had; a block decides
    /// for every sink.
    #[test]
    fn without_a_block_each_sink_keeps_its_key_exchange() {
        let mut s3_like = sink("https://s3.example");
        s3_like.unstated_key_exchange = Some(provider::default_key_exchange());
        let unstated = Security::unstated().base;
        let posture = secure(&s3_like, &unstated).unwrap().posture;
        assert_eq!(posture.key_exchange, Some(provider::default_key_exchange()));
        let rest = secure(&sink("https://collector.example"), &unstated).unwrap();
        assert_eq!(rest.posture.key_exchange, Some(KeyExchange::Classical));
        let stated = security("  transport: tls\n  key_exchange: classical\n").base;
        let posture = secure(&s3_like, &stated).unwrap().posture;
        assert_eq!(posture.key_exchange, Some(KeyExchange::Classical));
    }

    #[test]
    fn credentials_and_query_are_not_shown() {
        let posture = outcome(
            "https://user:secret@collector.example.org/records?sig=abc",
            None,
        )
        .unwrap();
        assert_eq!(posture.endpoint, "https://collector.example.org/records");
    }

    #[test]
    fn a_service_without_client_certificates_is_refused_under_mtls() {
        let mut public = sink("https://account.blob.core.windows.net");
        public.no_client_certificate =
            Some("the public Azure Blob service authorises requests by Shared Key only");
        let error = secure(&public, &security("  transport: mtls\n").base)
            .err()
            .unwrap()
            .to_string();
        assert_eq!(
            error,
            "level `mtls` needs a client certificate, and the public Azure Blob service \
             authorises requests by Shared Key only; to accept server-only TLS for this sink \
             give it `security: { transport: tls, reason: ... }`"
        );
    }

    #[cfg(feature = "fips")]
    #[test]
    fn work_outside_the_module_is_refused_under_fips_required() {
        let mut signing = sink("https://s3.example");
        signing.outside_provider = Some("request signing");
        let error = secure(
            &signing,
            &security("  transport: tls\n  fips: required\n").base,
        )
        .err()
        .unwrap()
        .to_string();
        assert!(
            error.starts_with("`fips: required`: request signing runs outside the FIPS module"),
            "{error}"
        );
        let secured = secure(
            &sink("https://s3.example"),
            &security("  transport: tls\n  fips: required\n").base,
        )
        .unwrap();
        assert_eq!(secured.posture.fips, Some(true));
    }

    #[test]
    fn the_posture_states_versions_groups_and_trust() {
        let posture = outcome(
            "https://collector.example:8443/records",
            Some("  transport: tls\n  min_tls: 1.2\n  key_exchange: classical\n"),
        )
        .unwrap();
        assert_eq!(posture.min_tls.as_deref(), Some("1.2"));
        assert_eq!(posture.key_exchange, Some(KeyExchange::Classical));
        assert_eq!(
            posture.groups,
            provider::group_names(KeyExchange::Classical)
        );
        assert_eq!(posture.client_auth, "none");
        assert_eq!(posture.trust.as_deref(), Some("built-in web roots"));
        assert!(posture.off_host);
        let plain = outcome("http://127.0.0.1:5001", None).unwrap();
        assert_eq!(plain.min_tls, None);
        assert!(plain.groups.is_empty());
        assert!(!plain.off_host);
    }

    #[test]
    fn unreadable_identity_files_name_the_key_and_the_path() {
        let missing = "  transport: mtls\n  client_cert: /nonexistent/client.pem\n  client_key: /nonexistent/client.key\n";
        let error = outcome("https://collector.example", Some(missing)).unwrap_err();
        assert!(
            error.starts_with("`security.client_cert`: cannot read /nonexistent/client.pem: "),
            "{error}"
        );
        let bundle = "  transport: tls\n  ca_bundle: /nonexistent/ca.pem\n";
        let error = outcome("https://collector.example", Some(bundle)).unwrap_err();
        assert!(
            error.starts_with("`security.ca_bundle`: cannot read /nonexistent/ca.pem: "),
            "{error}"
        );
        // A file that exists but holds no certificate.
        let empty = temp_path("empty.pem");
        std::fs::write(&empty, "not a certificate\n").unwrap();
        let block = format!("  transport: tls\n  ca_bundle: {}\n", empty.display());
        let error = outcome("https://collector.example", Some(&block)).unwrap_err();
        assert!(error.ends_with("holds no PEM certificate"), "{error}");
        let _ = std::fs::remove_file(&empty);
    }

    #[test]
    fn a_probe_target_exists_for_https_only() {
        let policy = Security::unstated().base;
        let secured = secure(&sink("https://collector.example/records"), &policy).unwrap();
        let target = secured.target.unwrap();
        assert_eq!(
            (target.host.as_str(), target.port),
            ("collector.example", 443)
        );
        let plain = secure(&sink("http://collector.example/records"), &policy).unwrap();
        assert!(plain.target.is_none());
    }
}
