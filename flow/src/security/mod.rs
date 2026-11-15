//! Transport security: one statement of the level wanted, held against every
//! path a record can leave the process on.
//!
//! * [`config`] is the `security` block as written, and the override one
//!   subscriber may carry.
//! * [`Security`] is the block with its defaults filled in. [`activate`]
//!   reads it, refuses what this binary cannot do, installs the crypto
//!   provider and logs the level; the capture process calls it first.
//! * [`provider`] builds the one crypto provider; `http` hands the HTTP
//!   sinks their TLS configuration; `zenoh_config` reads what a Zenoh
//!   configuration file will do.
//! * [`posture`] is what each path will use, for `mind security show`;
//!   [`probe`] performs one handshake and reports what was negotiated.
//!
//! Every refusal happens before anything connects and names the entry and
//! the requirement it cannot meet. `docs/security/transport.md` is the
//! adopter's guide.

pub mod config;
pub(crate) mod http;
pub mod posture;
pub mod probe;
pub mod provider;
#[cfg(feature = "aws")]
pub(crate) mod s3_client;
#[cfg(feature = "zenoh")]
pub(crate) mod zenoh_config;

pub use self::config::{Fips, KeyExchange, MinTls, Quic, SecurityConfig, SinkOverride, Transport};
pub use self::posture::{PathPosture, Report};
pub use self::probe::ProbeTarget;

use self::config::SecurityFile;
use crate::config::{PublisherConfig, SubscriberConfig, SUBSCRIBER_CONFIG_ENV};
use crate::error::{ConfigError, Problem};
use log::{info, warn};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// Names a file whose top-level `security:` key is the block.
pub const SECURITY_CONFIG_ENV: &str = "SECURITY_CONFIG_PATH";
const SECURITY_CONFIG_FILE: &str = "config/security.yml";
const SUBSCRIBER_CONFIG_FILE: &str = "config/subscribers.yml";

/// What is required of one path: the block's requirements, or one
/// subscriber's after its override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// Whether a `security` block was written. Without one each sink keeps
    /// the key exchange it used before the block existed.
    pub stated: bool,
    pub transport: Transport,
    pub min_tls: MinTls,
    pub key_exchange: KeyExchange,
    pub fips: Fips,
    pub quic: Quic,
    pub client_cert: Option<PathBuf>,
    pub client_key: Option<PathBuf>,
    pub ca_bundle: Option<PathBuf>,
    /// The requirement keys this subscriber sets differently from the block.
    pub overrides: Vec<&'static str>,
    /// The reason given with them.
    pub reason: Option<String>,
}

impl Policy {
    /// The client certificate and key, when both are configured.
    pub fn identity(&self) -> Option<(&Path, &Path)> {
        Some((self.client_cert.as_deref()?, self.client_key.as_deref()?))
    }

    /// Whether this path's TLS settings are the ones the process-wide
    /// provider was built from.
    #[cfg(feature = "zenoh")]
    pub(crate) fn same_tls_settings(&self, other: &Policy) -> bool {
        self.min_tls == other.min_tls && self.key_exchange == other.key_exchange
    }
}

/// The security block with its defaults filled in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Security {
    /// Whether a `security` block was written. Without one nothing is
    /// required of any path.
    pub stated: bool,
    /// Where the block was read from.
    pub origin: String,
    pub base: Policy,
    pub allow_plaintext_bind: bool,
}

impl Default for Security {
    fn default() -> Self {
        Security::unstated()
    }
}

impl Security {
    /// What a configuration with no `security` block gets: no requirement,
    /// and the connections every earlier release made. Those offered the
    /// classical groups, except from the S3 sink, which keeps its own
    /// (`unstated_key_exchange` in the sink).
    pub fn unstated() -> Self {
        Security {
            stated: false,
            origin: "no security block".to_string(),
            base: Policy {
                stated: false,
                transport: Transport::Unenforced,
                min_tls: MinTls::Tls12,
                key_exchange: KeyExchange::Classical,
                fips: Fips::Off,
                quic: Quic::Allowed,
                client_cert: None,
                client_key: None,
                ca_bundle: None,
                overrides: Vec::new(),
                reason: None,
            },
            allow_plaintext_bind: false,
        }
    }

    /// The block as written, with defaults: `min_tls` 1.3, the key exchange
    /// this binary prefers, `fips` off, `quic` allowed. Refuses a block this
    /// binary cannot honour.
    pub fn from_block(block: &SecurityConfig, origin: &str) -> Result<Self, ConfigError> {
        let base = Policy {
            stated: true,
            transport: block.transport,
            min_tls: block.min_tls.unwrap_or(MinTls::Tls13),
            key_exchange: block
                .key_exchange
                .unwrap_or_else(provider::default_key_exchange),
            fips: block.fips.unwrap_or(Fips::Off),
            quic: block.quic.unwrap_or(Quic::Allowed),
            client_cert: block.client_cert.clone(),
            client_key: block.client_key.clone(),
            ca_bundle: block.ca_bundle.clone(),
            overrides: Vec::new(),
            reason: None,
        };
        check_identity_pair(&base).map_err(ConfigError::Security)?;
        check_build(&base, provider::fips_module_active()).map_err(ConfigError::Security)?;
        Ok(Security {
            stated: true,
            origin: origin.to_string(),
            base,
            allow_plaintext_bind: block.allow_plaintext_bind,
        })
    }

    /// The block of a YAML document whose top-level `security:` key holds
    /// it; `None` when the document has no such key. `origin` names the
    /// document in errors and in the posture report.
    pub fn from_yaml(yaml: &str, origin: &str) -> Result<Option<Self>, ConfigError> {
        let file: SecurityFile = crate::config::parse(yaml, origin)?;
        file.security
            .map(|block| Security::from_block(&block, origin))
            .transpose()
    }

    /// The requirements for one subscriber: the block's, changed by the
    /// subscriber's own `security` key. A key that changes a requirement
    /// needs a `reason`.
    pub fn for_sink(&self, sink: Option<&SinkOverride>) -> Result<Policy, Problem> {
        let mut policy = self.base.clone();
        let Some(sink) = sink else {
            return Ok(policy);
        };
        if let Some(transport) = sink.transport.filter(|value| *value != policy.transport) {
            policy.transport = transport;
            policy.overrides.push("transport");
        }
        if let Some(min_tls) = sink.min_tls.filter(|value| *value != policy.min_tls) {
            policy.min_tls = min_tls;
            policy.overrides.push("min_tls");
        }
        if let Some(key_exchange) = sink
            .key_exchange
            .filter(|value| *value != policy.key_exchange)
        {
            policy.key_exchange = key_exchange;
            policy.overrides.push("key_exchange");
        }
        if let Some(fips) = sink.fips.filter(|value| *value != policy.fips) {
            policy.fips = fips;
            policy.overrides.push("fips");
        }
        if let Some(quic) = sink.quic.filter(|value| *value != policy.quic) {
            policy.quic = quic;
            policy.overrides.push("quic");
        }
        if sink.client_cert.is_some() || sink.client_key.is_some() {
            // An identity is replaced whole, never half.
            policy.client_cert = sink.client_cert.clone();
            policy.client_key = sink.client_key.clone();
        }
        if sink.ca_bundle.is_some() {
            policy.ca_bundle = sink.ca_bundle.clone();
        }
        let reason = sink
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|reason| !reason.is_empty());
        if let (Some(key), None) = (policy.overrides.first(), reason) {
            return Err(Problem::Security(format!(
                "`security.{key}` differs from the security block; say why in `security.reason`"
            )));
        }
        policy.reason = reason.map(str::to_string);
        check_identity_pair(&policy).map_err(Problem::Security)?;
        check_build(&policy, provider::fips_module_active()).map_err(Problem::Security)?;
        Ok(policy)
    }

    /// The line logged at start.
    pub fn startup_line(&self) -> String {
        let base = &self.base;
        format!(
            "security: level {} ({}); min TLS {}; key exchange {} [{}]; fips {}; provider {} \
             (AWS-LC {}), FIPS module {}; built with: {}",
            base.transport,
            self.origin,
            base.min_tls,
            base.key_exchange,
            provider::group_names(base.key_exchange).join(", "),
            base.fips,
            provider::PROVIDER_NAME,
            provider::library_version(),
            if provider::fips_module_active() {
                "active"
            } else {
                "not active"
            },
            provider::features_text(),
        )
    }
}

fn check_identity_pair(policy: &Policy) -> Result<(), String> {
    match (&policy.client_cert, &policy.client_key) {
        (Some(_), None) => Err("`client_cert` is set without `client_key`".to_string()),
        (None, Some(_)) => Err("`client_key` is set without `client_cert`".to_string()),
        _ => Ok(()),
    }
}

/// Refuse a requirement this binary was not built for. `fips_active` is the
/// provider's own answer, not the cargo feature.
pub(crate) fn check_build(policy: &Policy, fips_active: bool) -> Result<(), String> {
    check_features(
        policy,
        cfg!(feature = "pqc"),
        cfg!(feature = "fips"),
        cfg!(feature = "quic"),
        fips_active,
    )
}

/// [`check_build`] with the build stated, so every combination can be tested
/// from one binary.
pub(crate) fn check_features(
    policy: &Policy,
    pqc: bool,
    fips: bool,
    quic: bool,
    fips_active: bool,
) -> Result<(), String> {
    let rebuild = |feature: &str| {
        format!(
            "needs a binary built with the cargo feature `{feature}`; this one was built with: \
             {}",
            provider::features_text()
        )
    };
    if policy.key_exchange.is_hybrid() && !pqc {
        return Err(format!(
            "`key_exchange: {}` {}",
            policy.key_exchange,
            rebuild("pqc")
        ));
    }
    if policy.fips == Fips::Required {
        if !fips {
            return Err(format!("`fips: required` {}", rebuild("fips")));
        }
        if !fips_active {
            return Err(
                "`fips: required`: the crypto provider reports that the FIPS module is not \
                 active"
                    .to_string(),
            );
        }
    }
    if policy.quic == Quic::Required && !quic {
        return Err(format!("`quic: required` {}", rebuild("quic")));
    }
    Ok(())
}

static ACTIVE: OnceLock<Arc<Security>> = OnceLock::new();

/// The security block in force: the one [`activate`] read, or no
/// requirement when it has not been called.
pub fn current() -> Arc<Security> {
    ACTIVE
        .get()
        .cloned()
        .unwrap_or_else(|| Arc::new(Security::unstated()))
}

/// Read the security block, refuse what this binary cannot do, install the
/// crypto provider as the process default and log the level. The first call
/// decides; later calls return what it decided.
pub fn activate() -> Result<Arc<Security>, ConfigError> {
    if let Some(active) = ACTIVE.get() {
        return Ok(Arc::clone(active));
    }
    let security = Arc::new(load()?);
    provider::install(provider::build(
        security.base.min_tls,
        security.base.key_exchange,
    ))
    .map_err(|e| ConfigError::Security(e.to_string()))?;
    let active = ACTIVE.get_or_init(|| security);
    info!("{}", active.startup_line());
    Ok(Arc::clone(active))
}

/// The block from its own file, or from the subscriber file. Written in
/// both, it is refused: one statement covers the process.
pub fn load() -> Result<Security, ConfigError> {
    let own = crate::config::find(SECURITY_CONFIG_ENV, SECURITY_CONFIG_FILE);
    let subscribers = crate::config::find(SUBSCRIBER_CONFIG_ENV, SUBSCRIBER_CONFIG_FILE);
    let read = |path: &Option<PathBuf>| -> Result<Option<(SecurityConfig, String)>, ConfigError> {
        let Some(path) = path else {
            return Ok(None);
        };
        let file: SecurityFile = crate::config::read(path)?;
        Ok(file
            .security
            .map(|block| (block, path.display().to_string())))
    };
    let from_own = read(&own)?;
    if own.is_some() && from_own.is_none() {
        return Err(ConfigError::Security(format!(
            "{} has no top-level `security:` key",
            own.as_deref().unwrap_or(Path::new("")).display()
        )));
    }
    match (from_own, read(&subscribers)?) {
        (Some((_, first)), Some((_, second))) => Err(ConfigError::Security(format!(
            "the block is written in both {first} and {second}; keep one"
        ))),
        (Some((block, origin)), None) | (None, Some((block, origin))) => {
            Security::from_block(&block, &origin)
        }
        (None, None) => Ok(Security::unstated()),
    }
}

/// Whether a host name or address is this machine's loopback.
pub(crate) fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

/// Whether a ZeroMQ address can be reached from this machine only.
fn is_local_bind(address: &str) -> bool {
    if address.starts_with("ipc://") || address.starts_with("inproc://") {
        return true;
    }
    let Some(rest) = address.strip_prefix("tcp://") else {
        return false;
    };
    // `host:port`, where an IPv6 host is in brackets.
    let host = match rest.rsplit_once(':') {
        Some((host, _port)) => host,
        None => rest,
    };
    is_loopback_host(host)
}

/// What a ZeroMQ socket's bind address exposes, and whether the block
/// permits it. The warning, when there is one, names the address.
pub fn check_bind(
    publisher: &PublisherConfig,
    security: &Security,
) -> Result<(PathPosture, Option<String>), Problem> {
    let local = is_local_bind(&publisher.address);
    let exposed = format!(
        "binds {}, which is not loopback: the socket is unauthenticated and unencrypted, and \
         cannot check which hosts on that network read the records",
        publisher.address
    );
    let warning = match (local, security.stated, security.allow_plaintext_bind) {
        (true, _, _) => None,
        // With no block the publisher logs its own warning when it binds.
        (false, false, _) => None,
        (false, true, true) => Some(format!(
            "publisher {} {exposed} (`allow_plaintext_bind: true`)",
            publisher.name
        )),
        (false, true, false) => {
            return Err(Problem::Security(format!(
                "`address` {} is not loopback, and the ZeroMQ socket is unauthenticated and \
                 unencrypted; bind 127.0.0.1 and send off-host through the Zenoh or HTTPS sinks, \
                 or set `allow_plaintext_bind: true` in the security block",
                publisher.address
            )))
        }
    };
    let mut posture = PathPosture::local(
        &publisher.name,
        &format!("publisher (ZeroMQ {})", publisher.publisher_type),
        &publisher.address,
        security.base.transport,
    );
    posture.off_host = !local;
    posture.transport = "plaintext".to_string();
    posture.protocol = "ZeroMQ over TCP".to_string();
    if publisher.address.starts_with("ipc://") || publisher.address.starts_with("inproc://") {
        posture.protocol = "ZeroMQ, local socket".to_string();
    }
    if !local {
        posture
            .notes
            .push("reachable from other hosts without authentication or encryption".to_string());
        if security.stated {
            posture
                .notes
                .push("permitted by `allow_plaintext_bind: true`".to_string());
        }
    }
    Ok((posture, warning))
}

/// A subscriber reads the ZeroMQ feed in plaintext, so under any stated
/// level it reads it on this host.
pub(crate) fn check_feed(config: &SubscriberConfig, policy: &Policy) -> Result<(), Problem> {
    let Some(host) = config.zmq_url.as_deref() else {
        return Ok(());
    };
    if policy.transport == Transport::Unenforced || is_loopback_host(host) {
        return Ok(());
    }
    Err(Problem::Security(format!(
        "`zmq_url` {host} is not loopback: the ZeroMQ feed is unauthenticated and unencrypted, \
         and level `{}` keeps it on this host",
        policy.transport
    )))
}

/// Log the bind warning, if any, and return the posture.
pub(crate) fn permit_bind(
    publisher: &PublisherConfig,
    security: &Security,
) -> Result<PathPosture, Problem> {
    let (posture, warning) = check_bind(publisher, security)?;
    if let Some(warning) = warning {
        warn!("{warning}");
    }
    Ok(posture)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::config::parse;
    use crate::test_support::subscriber;

    /// The security block of a YAML fragment, with defaults.
    pub(crate) fn security(block: &str) -> Security {
        try_security(block).unwrap()
    }

    pub(crate) fn try_security(block: &str) -> Result<Security, ConfigError> {
        let file: SecurityFile = parse(&format!("security:\n{block}"), "test")?;
        Security::from_block(&file.security.unwrap(), "test")
    }

    fn publisher(address: &str) -> PublisherConfig {
        PublisherConfig {
            name: "records".into(),
            publisher_type: "PUB".into(),
            address: address.into(),
            topic: "*".into(),
            topic_suffix: None,
            encoding: None,
        }
    }

    #[test]
    fn no_block_requires_nothing_and_keeps_tls_1_2() {
        let security = Security::unstated();
        assert!(!security.stated);
        assert_eq!(security.base.transport, Transport::Unenforced);
        assert_eq!(security.base.min_tls, MinTls::Tls12);
        assert_eq!(security.base.fips, Fips::Off);
        assert_eq!(security.base.key_exchange, KeyExchange::Classical);
        assert!(!security.base.stated);
        assert_eq!(*current(), security);
    }

    #[test]
    fn a_block_defaults_to_tls_1_3() {
        let security = security("  transport: tls\n");
        assert!(security.stated);
        assert_eq!(security.base.min_tls, MinTls::Tls13);
        assert_eq!(security.base.key_exchange, provider::default_key_exchange());
        assert_eq!(security.base.quic, Quic::Allowed);
        assert!(!security.allow_plaintext_bind);
    }

    #[test]
    fn half_an_identity_is_refused() {
        let error = try_security("  transport: mtls\n  client_cert: /tmp/c.pem\n").unwrap_err();
        assert_eq!(
            error.to_string(),
            "security: `client_cert` is set without `client_key`"
        );
    }

    fn policy(key_exchange: KeyExchange, fips: Fips, quic: Quic) -> Policy {
        Policy {
            key_exchange,
            fips,
            quic,
            ..Security::unstated().base
        }
    }

    /// Requirement by build: each needs its feature, and `fips` also needs
    /// the module to say it is active.
    #[test]
    fn a_requirement_the_binary_was_not_built_for_is_refused() {
        use KeyExchange::*;
        // (policy, pqc, fips, quic, module active) -> the refusal starts with
        type Case = (Policy, bool, bool, bool, bool, Option<&'static str>);
        let cases: [Case; 11] = [
            (policy(Classical, Fips::Off, Quic::Allowed), false, false, false, false, None),
            (policy(HybridPqcPreferred, Fips::Off, Quic::Allowed), true, false, false, false, None),
            (
                policy(HybridPqcPreferred, Fips::Off, Quic::Allowed),
                false, false, false, false,
                Some("`key_exchange: hybrid-pqc-preferred` needs a binary built with the cargo feature `pqc`"),
            ),
            (
                policy(HybridPqcRequired, Fips::Off, Quic::Allowed),
                false, true, true, true,
                Some("`key_exchange: hybrid-pqc-required` needs a binary built with the cargo feature `pqc`"),
            ),
            (policy(HybridPqcRequired, Fips::Off, Quic::Allowed), true, false, false, false, None),
            (
                policy(Classical, Fips::Required, Quic::Allowed),
                true, false, true, false,
                Some("`fips: required` needs a binary built with the cargo feature `fips`"),
            ),
            (
                policy(Classical, Fips::Required, Quic::Allowed),
                true, true, true, false,
                Some("`fips: required`: the crypto provider reports that the FIPS module is not active"),
            ),
            (policy(Classical, Fips::Required, Quic::Allowed), false, true, false, true, None),
            (policy(HybridPqcRequired, Fips::Required, Quic::Required), true, true, true, true, None),
            (
                policy(Classical, Fips::Off, Quic::Required),
                true, true, false, true,
                Some("`quic: required` needs a binary built with the cargo feature `quic`"),
            ),
            (policy(Classical, Fips::Off, Quic::Required), false, false, true, false, None),
        ];
        for (policy, pqc, fips, quic, active, expected) in cases {
            let outcome = check_features(&policy, pqc, fips, quic, active);
            match (outcome, expected) {
                (Ok(()), None) => {}
                (Err(message), Some(start)) => {
                    assert!(message.starts_with(start), "{message}");
                    if !message.contains("FIPS module is not active") {
                        assert!(message.contains("this one was built with: "), "{message}");
                    }
                }
                (outcome, expected) => panic!("{policy:?}: {outcome:?}, expected {expected:?}"),
            }
        }
    }

    #[cfg(not(feature = "fips"))]
    #[test]
    fn fips_required_stops_a_build_without_the_module() {
        let error = try_security("  transport: tls\n  fips: required\n").unwrap_err();
        assert!(
            error.to_string().starts_with(
                "security: `fips: required` needs a binary built with the cargo feature `fips`; \
                 this one was built with: "
            ),
            "{error}"
        );
    }

    #[cfg(feature = "fips")]
    #[test]
    fn fips_required_is_met_by_the_module() {
        let security = security("  transport: tls\n  fips: required\n");
        assert_eq!(security.base.fips, Fips::Required);
        assert!(provider::fips_module_active());
    }

    #[cfg(not(feature = "pqc"))]
    #[test]
    fn hybrid_stops_a_build_without_pqc() {
        let error =
            try_security("  transport: tls\n  key_exchange: hybrid-pqc-required\n").unwrap_err();
        assert!(error.to_string().contains("cargo feature `pqc`"), "{error}");
    }

    #[test]
    fn an_override_that_changes_a_requirement_needs_a_reason() {
        let security = security("  transport: mtls\n");
        let sink = subscriber("    subscriber_type: rest\n    security:\n      transport: tls\n");
        let error = security.for_sink(sink.security.as_ref()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "`security.transport` differs from the security block; say why in `security.reason`"
        );
        let sink = subscriber(
            "    subscriber_type: rest\n    security:\n      transport: tls\n      min_tls: 1.2\n      reason: \"the collector has no client-certificate support\"\n",
        );
        let policy = security.for_sink(sink.security.as_ref()).unwrap();
        assert_eq!(policy.transport, Transport::Tls);
        assert_eq!(policy.min_tls, MinTls::Tls12);
        assert_eq!(policy.overrides, ["transport", "min_tls"]);
        assert_eq!(
            policy.reason.as_deref(),
            Some("the collector has no client-certificate support")
        );
    }

    #[test]
    fn an_override_that_only_adds_an_identity_needs_no_reason() {
        let security = security("  transport: mtls\n");
        let sink = subscriber(
            "    subscriber_type: rest\n    security:\n      transport: mtls\n      client_cert: /tmp/c.pem\n      client_key: /tmp/c.key\n      ca_bundle: /tmp/ca.pem\n",
        );
        let policy = security.for_sink(sink.security.as_ref()).unwrap();
        assert!(policy.overrides.is_empty());
        assert_eq!(policy.reason, None);
        assert!(policy.identity().is_some());
        assert_eq!(policy.ca_bundle.as_deref(), Some(Path::new("/tmp/ca.pem")));
        // No override at all is the block.
        assert_eq!(security.for_sink(None).unwrap(), security.base);
    }

    #[test]
    fn a_sink_override_has_no_unknown_keys() {
        let error = parse::<crate::config::SubscriberConfigs>(
            "subscribers:\n  - name: A\n    subscriber_type: rest\n    topics: [\"*\"]\n    security:\n      allow_plaintext_bind: true\n",
            "test",
        )
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"), "{error}");
    }

    #[test]
    fn loopback_hosts() {
        for host in [
            "127.0.0.1",
            "127.8.9.1",
            "localhost",
            "LOCALHOST",
            "::1",
            "[::1]",
        ] {
            assert!(is_loopback_host(host), "{host}");
        }
        for host in [
            "0.0.0.0",
            "*",
            "192.0.2.10",
            "collector.example",
            "::",
            "lo",
        ] {
            assert!(!is_loopback_host(host), "{host}");
        }
    }

    #[test]
    fn a_loopback_bind_is_always_permitted() {
        for security in [Security::unstated(), security("  transport: mtls\n")] {
            for address in [
                "tcp://127.0.0.1:12000",
                "tcp://[::1]:12000",
                "ipc:///run/fb.sock",
            ] {
                let (posture, warning) = check_bind(&publisher(address), &security).unwrap();
                assert_eq!(warning, None, "{address}");
                assert!(!posture.off_host);
                assert_eq!(posture.transport, "plaintext");
            }
        }
    }

    #[test]
    fn without_a_block_another_address_is_permitted() {
        let (posture, warning) =
            check_bind(&publisher("tcp://0.0.0.0:12000"), &Security::unstated()).unwrap();
        assert!(posture.off_host);
        // The publisher itself warns when it binds.
        assert_eq!(warning, None);
    }

    #[test]
    fn with_a_block_another_address_is_refused_unless_allowed() {
        for level in ["loopback-plaintext", "tls", "mtls", "unenforced"] {
            let refused = check_bind(
                &publisher("tcp://*:12000"),
                &security(&format!("  transport: {level}\n")),
            )
            .unwrap_err();
            assert!(
                refused
                    .to_string()
                    .starts_with("`address` tcp://*:12000 is not loopback"),
                "{refused}"
            );
            let (posture, warning) = check_bind(
                &publisher("tcp://192.0.2.10:12000"),
                &security(&format!(
                    "  transport: {level}\n  allow_plaintext_bind: true\n"
                )),
            )
            .unwrap();
            assert!(posture.off_host);
            let warning = warning.unwrap();
            assert!(warning.contains("tcp://192.0.2.10:12000"), "{warning}");
            assert!(warning.contains("allow_plaintext_bind"), "{warning}");
        }
    }

    #[test]
    fn a_stated_level_keeps_the_feed_on_this_host() {
        let mut config = subscriber("    subscriber_type: stateful\n");
        config.zmq_url = Some("192.0.2.10".into());
        assert!(check_feed(&config, &Security::unstated().base).is_ok());
        let error = check_feed(&config, &security("  transport: tls\n").base).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("`zmq_url` 192.0.2.10 is not loopback"),
            "{error}"
        );
        config.zmq_url = Some("127.0.0.1".into());
        assert!(check_feed(&config, &security("  transport: tls\n").base).is_ok());
    }

    #[test]
    fn the_startup_line_states_the_level_and_the_provider() {
        let line = Security::unstated().startup_line();
        assert!(
            line.starts_with(
                "security: level unenforced (no security block); min TLS 1.2; key exchange "
            ),
            "{line}"
        );
        assert!(line.contains("provider aws-lc-rs (AWS-LC "), "{line}");
        assert!(line.contains("built with: "), "{line}");
        let fips = if cfg!(feature = "fips") {
            "FIPS module active"
        } else {
            "FIPS module not active"
        };
        assert!(line.contains(fips), "{line}");
    }
}
