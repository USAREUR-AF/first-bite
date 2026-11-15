//! What a Zenoh configuration will do on the wire, read before the session
//! is opened, and held against the security level.
//!
//! The Zenoh client is configured by its own JSON5 file. It builds its TLS
//! configuration from rustls's process default provider, so its protocol
//! versions and key-exchange groups are the security block's and cannot
//! differ per sink; its endpoints, client certificate and trust anchors are
//! what the file says, and that is what this module inspects.

use super::config::{Fips, Quic, Transport};
use super::http::{read_certificates, tls_config};
use super::posture::PathPosture;
use super::probe::ProbeTarget;
use super::{is_loopback_host, provider, MinTls, Policy};
use crate::error::Problem;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::PrivateKeyDer;
use rustls::RootCertStore;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

const KEY: &str = "zenoh_config";

/// One `connect` or `listen` endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Endpoint {
    /// As written, without its `?` and `#` parts.
    pub(crate) locator: String,
    /// `tcp`, `tls`, `quic`, ...
    pub(crate) protocol: String,
    pub(crate) host: String,
    pub(crate) port: Option<u16>,
}

impl Endpoint {
    pub(crate) fn parse(endpoint: &str) -> Self {
        let locator = endpoint
            .split(['?', '#'])
            .next()
            .unwrap_or(endpoint)
            .to_string();
        let (protocol, address) = locator.split_once('/').unwrap_or((locator.as_str(), ""));
        let (host, port) = match address.rsplit_once(':') {
            Some((host, port)) => (host, port.parse().ok()),
            None => (address, None),
        };
        Endpoint {
            protocol: protocol.to_string(),
            host: host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string(),
            port,
            locator: locator.clone(),
        }
    }

    fn encrypted(&self) -> bool {
        matches!(self.protocol.as_str(), "tls" | "quic")
    }

    fn local(&self) -> bool {
        is_loopback_host(&self.host)
    }
}

/// The parts of a Zenoh configuration that decide what leaves the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ZenohFacts {
    /// `client`, `peer` or `router`.
    pub(crate) mode: String,
    pub(crate) connect: Vec<Endpoint>,
    pub(crate) listen: Vec<Endpoint>,
    /// Whether `listen` is the mode's default rather than written.
    pub(crate) default_listen: bool,
    pub(crate) multicast_scouting: bool,
    pub(crate) root_ca: Option<String>,
    pub(crate) connect_certificate: Option<String>,
    pub(crate) connect_private_key: Option<String>,
    /// A certificate or key written into the configuration as base64.
    pub(crate) inline_identity: bool,
    pub(crate) enable_mtls: bool,
    pub(crate) verify_name_on_connect: bool,
}

fn json(config: &zenoh::Config, key: &str) -> Value {
    config
        .get_json(key)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

fn string(config: &zenoh::Config, key: &str) -> Option<String> {
    json(config, key)
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// A list of endpoints that is either written once or per mode.
fn endpoints(value: &Value, mode: &str) -> Option<Vec<Endpoint>> {
    let list = match value {
        Value::Array(list) => list,
        Value::Object(by_mode) => by_mode.get(mode)?.as_array()?,
        _ => return None,
    };
    Some(
        list.iter()
            .filter_map(Value::as_str)
            .map(Endpoint::parse)
            .collect(),
    )
}

impl ZenohFacts {
    /// Read the facts from a configuration; unset keys take Zenoh's
    /// defaults (mode `peer`, a TCP listener on every interface, multicast
    /// scouting on, server names verified).
    pub(crate) fn read(config: &zenoh::Config) -> Self {
        let mode = string(config, "mode").unwrap_or_else(|| "peer".to_string());
        let connect = endpoints(&json(config, "connect/endpoints"), &mode).unwrap_or_default();
        let written_listen = endpoints(&json(config, "listen/endpoints"), &mode);
        let default_listen = match mode.as_str() {
            "router" => vec![Endpoint::parse("tcp/[::]:7447")],
            "peer" => vec![Endpoint::parse("tcp/[::]:0")],
            _ => Vec::new(),
        };
        let uses_default = written_listen
            .as_ref()
            .is_none_or(|listen| *listen == default_listen);
        let tls = "transport/link/tls";
        ZenohFacts {
            connect,
            default_listen: uses_default && !default_listen.is_empty(),
            listen: written_listen.unwrap_or(default_listen),
            multicast_scouting: json(config, "scouting/multicast/enabled")
                .as_bool()
                .unwrap_or(true),
            root_ca: string(config, &format!("{tls}/root_ca_certificate")),
            connect_certificate: string(config, &format!("{tls}/connect_certificate")),
            connect_private_key: string(config, &format!("{tls}/connect_private_key")),
            inline_identity: [
                "root_ca_certificate_base64",
                "connect_certificate_base64",
                "connect_private_key_base64",
            ]
            .iter()
            .any(|key| !json(config, &format!("{tls}/{key}")).is_null()),
            enable_mtls: json(config, &format!("{tls}/enable_mtls"))
                .as_bool()
                .unwrap_or(false),
            verify_name_on_connect: json(config, &format!("{tls}/verify_name_on_connect"))
                .as_bool()
                .unwrap_or(true),
            mode,
        }
    }

    fn all(&self) -> impl Iterator<Item = &Endpoint> {
        self.connect.iter().chain(&self.listen)
    }

    /// Whether multicast scouting will run: a client with endpoints to
    /// connect to does not scout.
    fn scouts(&self) -> bool {
        self.multicast_scouting && !(self.mode == "client" && !self.connect.is_empty())
    }

    /// Whether the client presents its certificate.
    fn client_auth(&self) -> bool {
        self.enable_mtls && self.connect_certificate.is_some() && self.connect_private_key.is_some()
    }
}

fn refuse(message: String) -> Problem {
    Problem::InvalidValue {
        key: KEY,
        reason: message,
    }
}

/// Hold a Zenoh configuration against the requirements of its subscriber.
/// `base` is the security block, whose TLS settings the process-wide
/// provider carries; `quic_built` says whether this binary has the QUIC
/// transport.
pub(crate) fn check(
    name: &str,
    facts: &ZenohFacts,
    policy: &Policy,
    base: &Policy,
    quic_built: bool,
) -> Result<(PathPosture, Vec<ProbeTarget>), Problem> {
    let level = policy.transport;
    if !policy.same_tls_settings(base) {
        return Err(Problem::Security(
            "`security.min_tls` and `security.key_exchange` cannot differ for a Zenoh sink: the \
             Zenoh client takes both from the process-wide provider, which follows the security \
             block"
                .to_string(),
        ));
    }
    let enforced = level != Transport::Unenforced;
    let quic_endpoint = facts.all().find(|endpoint| endpoint.protocol == "quic");
    if let Some(endpoint) = quic_endpoint {
        if !quic_built && (enforced || policy.quic == Quic::Required) {
            return Err(refuse(format!(
                "endpoint {} needs a binary built with the cargo feature `quic`; this one was \
                 built with: {}",
                endpoint.locator,
                provider::features_text()
            )));
        }
    }
    if policy.quic == Quic::Required {
        if let Some(endpoint) = facts.all().find(|endpoint| endpoint.protocol != "quic") {
            return Err(refuse(format!(
                "`quic: required` refuses endpoint {}; every endpoint must be quic/",
                endpoint.locator
            )));
        }
        if facts.connect.is_empty() {
            return Err(refuse(
                "`quic: required` needs at least one quic/ endpoint in `connect.endpoints`"
                    .to_string(),
            ));
        }
    }

    match level {
        Transport::Unenforced => {}
        Transport::LoopbackPlaintext => {
            if let Some(endpoint) = facts.all().find(|endpoint| !endpoint.local()) {
                let default = if facts.default_listen && facts.listen.contains(endpoint) {
                    format!(" (the default listener of mode {})", facts.mode)
                } else {
                    String::new()
                };
                return Err(refuse(format!(
                    "level `loopback-plaintext` refuses a sink that leaves this host; endpoint \
                     {}{default} is not loopback",
                    endpoint.locator
                )));
            }
            if facts.scouts() {
                return Err(refuse(
                    "level `loopback-plaintext` refuses multicast scouting, which reaches other \
                     hosts; set `scouting.multicast.enabled: false`"
                        .to_string(),
                ));
            }
        }
        Transport::Tls | Transport::Mtls => {
            if let Some(endpoint) = facts.all().find(|endpoint| !endpoint.encrypted()) {
                let message = if facts.default_listen && facts.listen.contains(endpoint) {
                    format!(
                        "level `{level}` refuses the plaintext listener {} that mode {} opens \
                         by default; set `mode: \"client\"` or list tls/ or quic/ endpoints \
                         under `listen.endpoints`",
                        endpoint.locator, facts.mode
                    )
                } else {
                    format!(
                        "level `{level}` refuses the plaintext endpoint {}; use tls/ or quic/",
                        endpoint.locator
                    )
                };
                return Err(refuse(message));
            }
            if facts.scouts() {
                return Err(refuse(format!(
                    "level `{level}` refuses multicast scouting: a peer found that way is \
                     connected over whatever transport it announces; set \
                     `scouting.multicast.enabled: false`"
                )));
            }
            if facts.connect.is_empty() && facts.listen.is_empty() {
                return Err(refuse(format!(
                    "level `{level}` needs at least one tls/ or quic/ endpoint in \
                     `connect.endpoints`"
                )));
            }
        }
    }
    if enforced && facts.inline_identity {
        return Err(refuse(
            "a certificate or key is written into the Zenoh configuration as base64; name a \
             file with `root_ca_certificate`, `connect_certificate` and `connect_private_key`"
                .to_string(),
        ));
    }
    if level == Transport::Mtls && !facts.client_auth() {
        return Err(refuse(
            "level `mtls` needs `transport.link.tls.connect_certificate`, \
             `connect_private_key` and `enable_mtls: true`"
                .to_string(),
        ));
    }
    let encrypted_endpoints = facts.all().any(Endpoint::encrypted);
    if policy.fips == Fips::Required && encrypted_endpoints {
        if quic_endpoint.is_some() {
            return Err(refuse(
                "`fips: required` refuses a quic/ endpoint: the QUIC library computes its reset \
                 and address-validation tokens with `ring`, outside the FIPS module; use tls/"
                    .to_string(),
            ));
        }
        if !facts.verify_name_on_connect {
            return Err(refuse(
                "`fips: required` refuses `verify_name_on_connect: false`: with it the Zenoh \
                 client verifies the server's certificate signatures with `ring`, outside the \
                 FIPS module"
                    .to_string(),
            ));
        }
    }

    // The files the session will read are read now, so that an unreadable
    // one stops startup rather than every record.
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if encrypted_endpoints || enforced {
        if let Some(path) = &facts.root_ca {
            for certificate in read_certificates(KEY, Path::new(path))
                .map_err(|problem| in_zenoh("root_ca_certificate", problem))?
            {
                roots
                    .add(certificate)
                    .map_err(|e| refuse(format!("root_ca_certificate {path}: {e}")))?;
            }
        }
        if facts.client_auth() {
            let cert = facts.connect_certificate.as_deref().unwrap_or_default();
            let key = facts.connect_private_key.as_deref().unwrap_or_default();
            read_certificates(KEY, Path::new(cert))
                .map_err(|problem| in_zenoh("connect_certificate", problem))?;
            PrivateKeyDer::from_pem_file(key).map_err(|e| {
                refuse(format!(
                    "connect_private_key: cannot read a private key from {key}: {e}"
                ))
            })?;
        }
    }

    let protocols: Vec<&str> = {
        let mut seen = Vec::new();
        for endpoint in facts.all() {
            if !seen.contains(&endpoint.protocol.as_str()) {
                seen.push(endpoint.protocol.as_str());
            }
        }
        seen
    };
    let off_host = facts.all().any(|endpoint| !endpoint.local()) || facts.scouts();
    let shown = if facts.connect.is_empty() {
        "no connect endpoint".to_string()
    } else {
        facts
            .connect
            .iter()
            .map(|endpoint| endpoint.locator.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut posture = PathPosture::local(name, "zenoh", &shown, level);
    posture.off_host = off_host;
    posture.overrides = policy.overrides.iter().map(ToString::to_string).collect();
    posture.override_reason = policy.reason.clone();
    posture.protocol = format!(
        "Zenoh ({} mode) over {}",
        facts.mode,
        if protocols.is_empty() {
            "nothing configured".to_string()
        } else {
            protocols
                .iter()
                .map(|protocol| match *protocol {
                    "tls" => "TLS/TCP",
                    "quic" => "QUIC",
                    "tcp" => "TCP",
                    other => other,
                })
                .collect::<Vec<_>>()
                .join(" and ")
        }
    );
    let all_encrypted = encrypted_endpoints && facts.all().all(Endpoint::encrypted);
    posture.transport = match (all_encrypted, encrypted_endpoints, facts.client_auth()) {
        (true, _, true) => "mutual TLS",
        (true, _, false) => "TLS",
        (false, true, _) => "TLS on some endpoints, plaintext on others",
        (false, false, _) => "plaintext",
    }
    .to_string();
    if !facts.listen.is_empty() {
        posture.notes.push(format!(
            "listens on {}",
            facts
                .listen
                .iter()
                .map(|endpoint| endpoint.locator.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if facts.scouts() {
        posture.notes.push(
            "multicast scouting is on: peers on the local network are discovered and connected"
                .to_string(),
        );
    }
    if quic_endpoint.is_some() && !quic_built {
        posture.notes.push(
            "a quic/ endpoint is configured and this binary has no QUIC transport; Zenoh will \
             not open it"
                .to_string(),
        );
    }
    let mut targets = Vec::new();
    if encrypted_endpoints {
        let tls13_only = facts.client_auth() || protocols == ["quic"];
        posture.min_tls = Some(if tls13_only && base.min_tls == MinTls::Tls12 {
            "1.3 (the Zenoh client uses TLS 1.3 only with a client certificate, and QUIC always)"
                .to_string()
        } else {
            base.min_tls.to_string()
        });
        posture.key_exchange = Some(base.key_exchange);
        posture.groups = provider::group_names(base.key_exchange);
        posture.client_auth = match (&facts.connect_certificate, facts.client_auth()) {
            (Some(cert), true) => format!("certificate {cert}"),
            (Some(_), false) => {
                "none (a certificate is configured but `enable_mtls` is not true)".to_string()
            }
            _ => "none".to_string(),
        };
        posture.trust = Some(match &facts.root_ca {
            Some(path) => format!("{path} and the built-in web roots"),
            None => "built-in web roots".to_string(),
        });
        let in_module = provider::fips_module_active()
            && quic_endpoint.is_none()
            && facts.verify_name_on_connect;
        posture.fips = Some(in_module);
        if !facts.verify_name_on_connect {
            posture.notes.push(
                "`verify_name_on_connect: false`: the server's name is not checked, and its \
                 certificate signatures are verified with `ring`"
                    .to_string(),
            );
        }
        // A probe handshakes with each tls/ endpoint the way the session
        // will: same provider settings, trust anchors and identity.
        let identity = facts.client_auth().then(|| {
            (
                Path::new(facts.connect_certificate.as_deref().unwrap_or_default()),
                Path::new(facts.connect_private_key.as_deref().unwrap_or_default()),
            )
        });
        let min_tls = if facts.client_auth() {
            MinTls::Tls13
        } else {
            base.min_tls
        };
        let config = Arc::new(tls_config(
            min_tls,
            base.key_exchange,
            roots,
            identity,
            (KEY, KEY),
        )?);
        for endpoint in facts.connect.iter().filter(|e| e.protocol == "tls") {
            if let Some(port) = endpoint.port {
                targets.push(ProbeTarget {
                    sink: name.to_string(),
                    endpoint: endpoint.locator.clone(),
                    host: endpoint.host.clone(),
                    port,
                    config: Arc::clone(&config),
                });
            }
        }
    }
    Ok((posture, targets))
}

/// Name the Zenoh key in a problem raised while reading one of its files.
fn in_zenoh(zenoh_key: &str, problem: Problem) -> Problem {
    match problem {
        Problem::InvalidValue { reason, .. } => refuse(format!("{zenoh_key}: {reason}")),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::tests::security;
    use crate::security::Security;

    fn facts(json5: &str) -> ZenohFacts {
        ZenohFacts::read(&zenoh::Config::from_json5(json5).unwrap())
    }

    fn outcome(json5: &str, block: Option<&str>, quic_built: bool) -> Result<PathPosture, String> {
        let security = match block {
            Some(block) => security(block),
            None => Security::unstated(),
        };
        check(
            "Example",
            &facts(json5),
            &security.base,
            &security.base,
            quic_built,
        )
        .map(|(posture, _)| posture)
        .map_err(|problem| problem.to_string())
    }

    const CLIENT_TLS: &str =
        r#"{ mode: "client", connect: { endpoints: ["tls/router.example:7447"] } }"#;
    const CLIENT_TCP: &str =
        r#"{ mode: "client", connect: { endpoints: ["tcp/192.0.2.20:7447"] } }"#;
    const CLIENT_QUIC: &str =
        r#"{ mode: "client", connect: { endpoints: ["quic/router.example:7447"] } }"#;
    const CLIENT_LOCAL: &str =
        r#"{ mode: "client", connect: { endpoints: ["tcp/127.0.0.1:7447"] } }"#;

    #[test]
    fn endpoints_are_split_into_protocol_host_and_port() {
        let endpoint = Endpoint::parse("tls/router.example:7447?rel=1#so_sndbuf=65000");
        assert_eq!(endpoint.locator, "tls/router.example:7447");
        assert_eq!(endpoint.protocol, "tls");
        assert_eq!(endpoint.host, "router.example");
        assert_eq!(endpoint.port, Some(7447));
        let v6 = Endpoint::parse("tcp/[::1]:7447");
        assert_eq!((v6.host.as_str(), v6.port), ("::1", Some(7447)));
        assert!(v6.local());
        assert!(!Endpoint::parse("tcp/[::]:0").local());
    }

    #[test]
    fn the_defaults_of_an_empty_configuration_are_read() {
        let facts = ZenohFacts::read(&zenoh::Config::default());
        assert_eq!(facts.mode, "peer");
        assert!(facts.connect.is_empty());
        assert_eq!(facts.listen, [Endpoint::parse("tcp/[::]:0")]);
        assert!(facts.default_listen);
        assert!(facts.multicast_scouting);
        assert!(facts.verify_name_on_connect);
        assert!(!facts.enable_mtls);
        assert!(!facts.inline_identity);
    }

    #[test]
    fn a_written_configuration_is_read() {
        let facts = facts(
            r#"{
                mode: "client",
                connect: { endpoints: ["tls/router.example:7447", "quic/router.example:7448"] },
                scouting: { multicast: { enabled: false } },
                transport: { link: { tls: {
                    root_ca_certificate: "/etc/first-bite/tls/ca.pem",
                    connect_certificate: "/etc/first-bite/tls/client.pem",
                    connect_private_key: "/etc/first-bite/tls/client.key",
                    enable_mtls: true,
                    verify_name_on_connect: false,
                } } },
            }"#,
        );
        assert_eq!(facts.mode, "client");
        assert_eq!(facts.connect.len(), 2);
        assert!(facts.listen.is_empty());
        assert!(!facts.default_listen);
        assert!(!facts.multicast_scouting);
        assert_eq!(facts.root_ca.as_deref(), Some("/etc/first-bite/tls/ca.pem"));
        assert!(facts.client_auth());
        assert!(!facts.verify_name_on_connect);
    }

    #[test]
    fn an_inline_identity_is_seen() {
        let facts = facts(
            r#"{ mode: "client", connect: { endpoints: ["tls/router.example:7447"] },
                 transport: { link: { tls: { connect_private_key_base64: "QUJD" } } } }"#,
        );
        assert!(facts.inline_identity);
    }

    /// Level by configuration by build.
    #[test]
    fn each_level_against_each_configuration() {
        // (configuration, level, quic built) -> transport, or the refusal starts with
        type Case = (
            &'static str,
            Option<&'static str>,
            bool,
            Result<&'static str, &'static str>,
        );
        let cases: [Case; 15] = [
            (CLIENT_TCP, None, false, Ok("plaintext")),
            (CLIENT_TLS, None, false, Ok("TLS")),
            (CLIENT_QUIC, None, true, Ok("TLS")),
            (CLIENT_QUIC, None, false, Ok("TLS")),
            (CLIENT_LOCAL, Some("loopback-plaintext"), false, Ok("plaintext")),
            (
                CLIENT_TCP,
                Some("loopback-plaintext"),
                false,
                Err("`zenoh_config`: level `loopback-plaintext` refuses a sink that leaves this host; endpoint tcp/192.0.2.20:7447 is not loopback"),
            ),
            (
                CLIENT_TLS,
                Some("loopback-plaintext"),
                false,
                Err("`zenoh_config`: level `loopback-plaintext` refuses a sink that leaves this host"),
            ),
            (
                CLIENT_TCP,
                Some("tls"),
                false,
                Err("`zenoh_config`: level `tls` refuses the plaintext endpoint tcp/192.0.2.20:7447; use tls/ or quic/"),
            ),
            (
                CLIENT_LOCAL,
                Some("tls"),
                false,
                Err("`zenoh_config`: level `tls` refuses the plaintext endpoint tcp/127.0.0.1:7447"),
            ),
            (CLIENT_TLS, Some("tls"), false, Ok("TLS")),
            (CLIENT_QUIC, Some("tls"), true, Ok("TLS")),
            (
                CLIENT_QUIC,
                Some("tls"),
                false,
                Err("`zenoh_config`: endpoint quic/router.example:7447 needs a binary built with the cargo feature `quic`"),
            ),
            (
                CLIENT_TLS,
                Some("mtls"),
                false,
                Err("`zenoh_config`: level `mtls` needs `transport.link.tls.connect_certificate`, `connect_private_key` and `enable_mtls: true`"),
            ),
            (
                CLIENT_TCP,
                Some("mtls"),
                true,
                Err("`zenoh_config`: level `mtls` refuses the plaintext endpoint tcp/192.0.2.20:7447"),
            ),
            (
                "{}",
                Some("tls"),
                true,
                Err("`zenoh_config`: level `tls` refuses the plaintext listener tcp/[::]:0 that mode peer opens by default"),
            ),
        ];
        for (config, level, quic_built, expected) in cases {
            let block = level.map(|level| format!("  transport: {level}\n"));
            let got = outcome(config, block.as_deref(), quic_built);
            match (got, expected) {
                (Ok(posture), Ok(transport)) => {
                    assert_eq!(posture.transport, transport, "{config} {level:?}")
                }
                (Err(message), Err(start)) => {
                    assert!(message.starts_with(start), "{config} {level:?}: {message}")
                }
                (got, expected) => panic!("{config} {level:?}: {got:?}, expected {expected:?}"),
            }
        }
    }

    #[test]
    fn scouting_is_refused_where_it_could_bypass_the_level() {
        let peer = r#"{ mode: "peer", connect: { endpoints: ["tls/router.example:7447"] },
                        listen: { endpoints: ["tls/[::]:7447"] } }"#;
        let error = outcome(peer, Some("  transport: tls\n"), false).unwrap_err();
        assert!(
            error.starts_with("`zenoh_config`: level `tls` refuses multicast scouting"),
            "{error}"
        );
        let quiet = r#"{ mode: "peer", connect: { endpoints: ["tls/router.example:7447"] },
                         listen: { endpoints: ["tls/[::]:7447"] },
                         scouting: { multicast: { enabled: false } } }"#;
        let posture = outcome(quiet, Some("  transport: tls\n"), false).unwrap();
        assert_eq!(posture.transport, "TLS");
        assert_eq!(posture.notes, ["listens on tls/[::]:7447"]);
        // A client with an endpoint to connect to does not scout.
        assert!(outcome(CLIENT_TLS, Some("  transport: tls\n"), false).is_ok());
    }

    #[test]
    fn quic_required_takes_only_quic_endpoints() {
        let block = Some("  transport: tls\n  quic: required\n");
        if cfg!(feature = "quic") {
            assert!(outcome(CLIENT_QUIC, block, true).is_ok());
            let error = outcome(CLIENT_TLS, block, true).unwrap_err();
            assert_eq!(
                error,
                "`zenoh_config`: `quic: required` refuses endpoint tls/router.example:7447; \
                 every endpoint must be quic/"
            );
        }
    }

    #[test]
    fn tls_settings_cannot_differ_from_the_block() {
        let security = security("  transport: tls\n");
        let mut policy = security.base.clone();
        policy.min_tls = MinTls::Tls12;
        let error = check(
            "Example",
            &facts(CLIENT_TLS),
            &policy,
            &security.base,
            false,
        )
        .err()
        .unwrap()
        .to_string();
        assert!(
            error.starts_with(
                "`security.min_tls` and `security.key_exchange` cannot differ for a Zenoh sink"
            ),
            "{error}"
        );
    }

    #[test]
    fn an_inline_identity_is_refused_under_a_level() {
        let inline = r#"{ mode: "client", connect: { endpoints: ["tls/router.example:7447"] },
                          transport: { link: { tls: { connect_private_key_base64: "QUJD" } } } }"#;
        assert!(outcome(inline, None, false).is_ok());
        let error = outcome(inline, Some("  transport: tls\n"), false).unwrap_err();
        assert!(
            error.contains("written into the Zenoh configuration as base64"),
            "{error}"
        );
    }

    #[test]
    fn unreadable_identity_files_stop_startup() {
        let config = r#"{ mode: "client", connect: { endpoints: ["tls/router.example:7447"] },
            transport: { link: { tls: {
                root_ca_certificate: "/nonexistent/ca.pem",
                connect_certificate: "/nonexistent/client.pem",
                connect_private_key: "/nonexistent/client.key",
                enable_mtls: true } } } }"#;
        let error = outcome(config, Some("  transport: mtls\n"), false).unwrap_err();
        assert!(
            error.starts_with(
                "`zenoh_config`: root_ca_certificate: cannot read /nonexistent/ca.pem: "
            ),
            "{error}"
        );
    }

    #[test]
    fn the_posture_states_what_the_session_will_use() {
        let posture = outcome(
            CLIENT_TLS,
            Some("  transport: tls\n  min_tls: 1.2\n"),
            false,
        )
        .unwrap();
        assert_eq!(posture.endpoint, "tls/router.example:7447");
        assert_eq!(posture.protocol, "Zenoh (client mode) over TLS/TCP");
        assert_eq!(posture.min_tls.as_deref(), Some("1.2"));
        assert_eq!(posture.trust.as_deref(), Some("built-in web roots"));
        assert_eq!(posture.client_auth, "none");
        assert!(posture.off_host);
        let quic = outcome(
            CLIENT_QUIC,
            Some("  transport: tls\n  min_tls: 1.2\n"),
            true,
        )
        .unwrap();
        assert_eq!(quic.protocol, "Zenoh (client mode) over QUIC");
        assert!(quic.min_tls.unwrap().starts_with("1.3 ("));
        let defaults = outcome("{}", None, false).unwrap();
        assert_eq!(defaults.transport, "plaintext");
        assert!(defaults.off_host);
        assert_eq!(defaults.min_tls, None);
        assert_eq!(defaults.notes.len(), 2, "{:?}", defaults.notes);
    }

    #[cfg(feature = "fips")]
    #[test]
    fn fips_required_refuses_what_runs_outside_the_module() {
        let block = Some("  transport: tls\n  fips: required\n");
        assert_eq!(outcome(CLIENT_TLS, block, true).unwrap().fips, Some(true));
        let error = outcome(CLIENT_QUIC, block, true).unwrap_err();
        assert!(
            error.starts_with("`zenoh_config`: `fips: required` refuses a quic/ endpoint"),
            "{error}"
        );
        let unverified = r#"{ mode: "client", connect: { endpoints: ["tls/router.example:7447"] },
            transport: { link: { tls: { verify_name_on_connect: false } } } }"#;
        let error = outcome(unverified, block, true).unwrap_err();
        assert!(
            error.contains("refuses `verify_name_on_connect: false`"),
            "{error}"
        );
    }
}
