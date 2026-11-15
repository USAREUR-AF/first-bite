//! What each path will use: the report `mind security show` prints.
//!
//! The report is built from the same configuration and by the same code as
//! a capture run, and connects to nothing.

use super::config::{Fips, KeyExchange, Transport};
use super::probe::ProbeTarget;
use super::{provider, Security};
use crate::config::{
    load_publisher_configs, load_subpub_configs, load_subscriber_configs, PublisherConfig,
    SubscriberConfig,
};
use crate::error::ConfigError;
use serde::Serialize;
use std::fmt::Write;

/// One way records leave the process, or one socket they are served on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PathPosture {
    /// The publisher's or subscriber's `name`.
    pub name: String,
    /// `publisher (ZeroMQ PUB)`, `rest`, `objectstorage/awsobject`, ...
    pub kind: String,
    /// Where it binds or sends, without credentials.
    pub endpoint: String,
    /// Whether another host is involved.
    pub off_host: bool,
    /// The level this path is held to.
    pub level: Transport,
    /// `plaintext`, `TLS`, `mutual TLS`, or `none` for a local file.
    pub transport: String,
    pub protocol: String,
    /// Absent where there is no TLS.
    pub min_tls: Option<String>,
    pub key_exchange: Option<KeyExchange>,
    /// The groups offered, in order.
    pub groups: Vec<String>,
    pub client_auth: String,
    /// Which certificate authorities the server is checked against.
    pub trust: Option<String>,
    /// Whether this path's TLS configuration is in FIPS mode.
    pub fips: Option<bool>,
    pub notes: Vec<String>,
    /// The requirement keys this subscriber overrides, and why.
    pub overrides: Vec<String>,
    pub override_reason: Option<String>,
}

impl PathPosture {
    /// A path with no TLS: a socket or a file on this machine.
    pub fn local(name: &str, kind: &str, endpoint: &str, level: Transport) -> Self {
        PathPosture {
            name: name.to_string(),
            kind: kind.to_string(),
            endpoint: endpoint.to_string(),
            off_host: false,
            level,
            transport: "none".to_string(),
            protocol: "local file".to_string(),
            min_tls: None,
            key_exchange: None,
            groups: Vec::new(),
            client_auth: "none".to_string(),
            trust: None,
            fips: None,
            notes: Vec::new(),
            overrides: Vec::new(),
            override_reason: None,
        }
    }
}

/// The crypto provider, as the process finds it at run time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderPosture {
    pub name: String,
    /// Version of the linked AWS-LC library, asked of the library.
    pub library_version: String,
    /// Whether the library says it is the FIPS module in FIPS mode.
    pub fips_module_active: bool,
    /// Other crypto libraries compiled into this binary, and what uses them.
    pub also_compiled_in: Vec<String>,
}

impl ProviderPosture {
    pub fn current() -> Self {
        let mut also_compiled_in = Vec::new();
        if cfg!(feature = "zenoh") {
            let mut uses = "ring, a dependency of the Zenoh client, which verifies \
                            server-certificate signatures with it when `verify_name_on_connect` \
                            is false"
                .to_string();
            if cfg!(feature = "quic") {
                uses.push_str(
                    "; the QUIC library computes its stateless-reset and address-validation \
                     tokens with it",
                );
            }
            also_compiled_in.push(uses);
        }
        ProviderPosture {
            name: provider::PROVIDER_NAME.to_string(),
            library_version: provider::library_version().to_string(),
            fips_module_active: provider::fips_module_active(),
            also_compiled_in,
        }
    }
}

/// A path, or why it is refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PathReport {
    pub name: String,
    pub posture: Option<PathPosture>,
    /// The message startup stops with.
    pub refused: Option<String>,
}

/// Everything `mind security show` prints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Whether a `security` block is written.
    pub stated: bool,
    pub origin: String,
    pub level: Transport,
    pub min_tls: String,
    pub key_exchange: KeyExchange,
    pub groups: Vec<String>,
    pub fips: Fips,
    pub quic: String,
    pub allow_plaintext_bind: bool,
    pub provider: ProviderPosture,
    /// The cargo features this binary was built with.
    pub features: Vec<String>,
    /// A problem with the block itself; nothing else is evaluated then.
    pub refused: Option<String>,
    pub paths: Vec<PathReport>,
}

impl Report {
    /// Every refusal, in the words startup uses.
    pub fn refusals(&self) -> Vec<&str> {
        self.refused
            .as_deref()
            .into_iter()
            .chain(self.paths.iter().filter_map(|path| path.refused.as_deref()))
            .collect()
    }

    fn header(security: &Security, refused: Option<String>) -> Self {
        let base = &security.base;
        Report {
            stated: security.stated,
            origin: security.origin.clone(),
            level: base.transport,
            min_tls: base.min_tls.to_string(),
            key_exchange: base.key_exchange,
            groups: provider::group_names(base.key_exchange),
            fips: base.fips,
            quic: base.quic.to_string(),
            allow_plaintext_bind: security.allow_plaintext_bind,
            provider: ProviderPosture::current(),
            features: provider::features()
                .into_iter()
                .map(str::to_string)
                .collect(),
            refused,
            paths: Vec::new(),
        }
    }

    /// The report for a security block and the entries of the three
    /// configuration files. Subpubs are listed when `subpubs` is given.
    pub fn build(
        security: &Security,
        publishers: &[PublisherConfig],
        subscribers: &[SubscriberConfig],
        subpub_publishers: &[PublisherConfig],
    ) -> (Self, Vec<ProbeTarget>) {
        let mut report = Report::header(security, None);
        let mut targets = Vec::new();
        for publisher in publishers.iter().chain(subpub_publishers) {
            let outcome = super::check_bind(publisher, security)
                .map(|(posture, _warning)| posture)
                .map_err(|problem| {
                    ConfigError::Publisher {
                        publisher: publisher.name.clone(),
                        problem,
                    }
                    .to_string()
                });
            report.paths.push(PathReport {
                name: publisher.name.clone(),
                posture: outcome.as_ref().ok().cloned(),
                refused: outcome.err(),
            });
        }
        for subscriber in subscribers {
            match crate::sink::describe_sink(subscriber, security) {
                Ok((posture, sink_targets)) => {
                    targets.extend(sink_targets);
                    report.paths.push(PathReport {
                        name: subscriber.name.clone(),
                        posture: Some(posture),
                        refused: None,
                    });
                }
                Err(error) => report.paths.push(PathReport {
                    name: subscriber.name.clone(),
                    posture: None,
                    refused: Some(error.to_string()),
                }),
            }
        }
        (report, targets)
    }

    /// The report for the configuration a capture run would load: the same
    /// environment variables and the same files. Nothing is bound and
    /// nothing is connected.
    pub fn load(with_subpubs: bool) -> (Self, Vec<ProbeTarget>) {
        let refused = |security: &Security, error: ConfigError| {
            (
                Report::header(security, Some(error.to_string())),
                Vec::new(),
            )
        };
        let security = match super::load() {
            Ok(security) => security,
            Err(error) => return refused(&Security::unstated(), error),
        };
        let publishers = match load_publisher_configs() {
            Ok(publishers) => publishers,
            Err(error) => return refused(&security, error),
        };
        let subscribers = match load_subscriber_configs() {
            Ok(subscribers) => subscribers,
            Err(error) => return refused(&security, error),
        };
        let subpub_publishers = if with_subpubs {
            match load_subpub_configs() {
                Ok(subpubs) => subpubs.into_iter().map(|subpub| subpub.publisher).collect(),
                Err(error) => return refused(&security, error),
            }
        } else {
            Vec::new()
        };
        Report::build(&security, &publishers, &subscribers, &subpub_publishers)
    }

    /// The report as JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
    }

    /// The report as text.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let yes_no = |value: bool| if value { "yes" } else { "no" };
        let _ = writeln!(out, "security level     {} ({})", self.level, self.origin);
        let _ = writeln!(out, "minimum TLS        {}", self.min_tls);
        let _ = writeln!(
            out,
            "key exchange       {} [{}]",
            self.key_exchange,
            self.groups.join(", ")
        );
        let _ = writeln!(out, "fips               {}", self.fips);
        let _ = writeln!(out, "quic               {}", self.quic);
        let _ = writeln!(
            out,
            "crypto provider    {} (AWS-LC {})",
            self.provider.name, self.provider.library_version
        );
        let _ = writeln!(
            out,
            "FIPS module active {}",
            yes_no(self.provider.fips_module_active)
        );
        for other in &self.provider.also_compiled_in {
            let _ = writeln!(out, "also compiled in   {other}");
        }
        let features = if self.features.is_empty() {
            "none".to_string()
        } else {
            self.features.join(", ")
        };
        let _ = writeln!(out, "built with         {features}");
        if let Some(refused) = &self.refused {
            let _ = writeln!(out, "\nREFUSED  {refused}");
            return out;
        }
        for path in &self.paths {
            let _ = writeln!(out);
            let Some(posture) = &path.posture else {
                let _ = writeln!(out, "{}", path.name);
                let _ = writeln!(
                    out,
                    "  REFUSED          {}",
                    path.refused.as_deref().unwrap_or("")
                );
                continue;
            };
            let _ = writeln!(out, "{}  [{}]", posture.name, posture.kind);
            let _ = writeln!(out, "  endpoint         {}", posture.endpoint);
            let _ = writeln!(out, "  leaves the host  {}", yes_no(posture.off_host));
            let _ = writeln!(out, "  transport        {}", posture.transport);
            let _ = writeln!(out, "  protocol         {}", posture.protocol);
            if let Some(min_tls) = &posture.min_tls {
                let _ = writeln!(out, "  minimum TLS      {min_tls}");
            }
            if let Some(key_exchange) = posture.key_exchange {
                let _ = writeln!(
                    out,
                    "  key exchange     {key_exchange} [{}]",
                    posture.groups.join(", ")
                );
            }
            if posture.min_tls.is_some() {
                let _ = writeln!(out, "  client auth      {}", posture.client_auth);
            }
            if let Some(trust) = &posture.trust {
                let _ = writeln!(out, "  server trust     {trust}");
            }
            if let Some(fips) = posture.fips {
                let _ = writeln!(out, "  FIPS mode        {}", yes_no(fips));
            }
            if !posture.overrides.is_empty() {
                let _ = writeln!(
                    out,
                    "  overrides        {} (level {}): {}",
                    posture.overrides.join(", "),
                    posture.level,
                    posture.override_reason.as_deref().unwrap_or("")
                );
            }
            for note in &posture.notes {
                let _ = writeln!(out, "  note             {note}");
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{parse, PublisherConfigs, SubscriberConfigs, DEFAULT_PUBLISHERS};
    use crate::security::tests::security;

    fn subscribers(yaml: &str) -> Vec<SubscriberConfig> {
        parse::<SubscriberConfigs>(yaml, "test")
            .unwrap()
            .subscribers
    }

    fn publishers() -> Vec<PublisherConfig> {
        parse::<PublisherConfigs>(DEFAULT_PUBLISHERS, "built-in")
            .unwrap()
            .publishers
    }

    const SINKS: &str = "subscribers:\n  \
        - { name: ToCollector, subscriber_type: rest, zmq_url: \"127.0.0.1\", zmq_port: 12000, topics: [\"*\"], endpoint_url: \"https://collector.example\", endpoint_port: 8443, endpoint_path: /records }\n  \
        - { name: ToEcho, subscriber_type: rest, zmq_url: \"127.0.0.1\", zmq_port: 12000, topics: [\"*\"], endpoint_url: \"203.0.113.10\", endpoint_port: 5001 }\n  \
        - { name: ToFile, subscriber_type: local, zmq_url: \"127.0.0.1\", zmq_port: 12000, topics: [\"*\"], local_logfile: records.log }\n";

    #[test]
    fn without_a_block_every_path_is_described_and_none_refused() {
        let (report, targets) = Report::build(
            &Security::unstated(),
            &publishers(),
            &subscribers(SINKS),
            &[],
        );
        assert!(report.refusals().is_empty(), "{:?}", report.refusals());
        assert_eq!(report.level, Transport::Unenforced);
        assert_eq!(report.paths.len(), 4);
        let by_name = |name: &str| {
            report
                .paths
                .iter()
                .find(|path| path.name == name)
                .and_then(|path| path.posture.clone())
                .unwrap()
        };
        let publisher = by_name("records");
        assert_eq!(publisher.transport, "plaintext");
        assert!(!publisher.off_host);
        let collector = by_name("ToCollector");
        assert_eq!(collector.transport, "TLS");
        assert_eq!(collector.endpoint, "https://collector.example:8443/records");
        assert_eq!(collector.min_tls.as_deref(), Some("1.2"));
        assert_eq!(collector.groups, provider::group_names(report.key_exchange));
        assert_eq!(collector.fips, Some(cfg!(feature = "fips")));
        let echo = by_name("ToEcho");
        assert_eq!(echo.transport, "plaintext");
        assert!(echo.off_host);
        let file = by_name("ToFile");
        assert_eq!(file.transport, "none");
        // Only the HTTPS endpoint can be probed.
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].sink, "ToCollector");
    }

    #[test]
    fn under_tls_the_plaintext_sink_is_refused_and_the_rest_reported() {
        let (report, _) = Report::build(
            &security("  transport: tls\n"),
            &publishers(),
            &subscribers(SINKS),
            &[],
        );
        assert_eq!(
            report.refusals(),
            [
                "subscriber ToEcho: level `tls` needs an https:// endpoint; `endpoint_url` gives \
              http://203.0.113.10:5001/"
            ]
        );
        let text = report.to_text();
        assert!(text.contains("security level     tls (test)"), "{text}");
        assert!(text.contains("minimum TLS        1.3"), "{text}");
        assert!(
            text.contains("ToEcho\n  REFUSED          subscriber ToEcho: level `tls`"),
            "{text}"
        );
        assert!(
            text.contains(
                "ToCollector  [rest]\n  endpoint         https://collector.example:8443/records"
            ),
            "{text}"
        );
        assert!(text.contains("ToFile  [local]"), "{text}");
    }

    #[test]
    fn the_text_states_the_provider_and_the_build() {
        let (report, _) = Report::build(&Security::unstated(), &publishers(), &[], &[]);
        let text = report.to_text();
        assert!(
            text.starts_with("security level     unenforced (no security block)\n"),
            "{text}"
        );
        assert!(
            text.contains("\ncrypto provider    aws-lc-rs (AWS-LC "),
            "{text}"
        );
        let active = if cfg!(feature = "fips") { "yes" } else { "no" };
        assert!(
            text.contains(&format!("\nFIPS module active {active}\n")),
            "{text}"
        );
        assert!(
            text.contains(&format!(
                "\nbuilt with         {}\n",
                provider::features_text()
            )),
            "{text}"
        );
        assert!(text.contains("records  [publisher (ZeroMQ PUB)]\n  endpoint         tcp://127.0.0.1:12000\n  leaves the host  no\n  transport        plaintext\n"), "{text}");
    }

    #[test]
    fn the_json_carries_the_same_fields() {
        let (report, _) = Report::build(
            &security("  transport: tls\n"),
            &publishers(),
            &subscribers(SINKS),
            &[],
        );
        let json: serde_json::Value = serde_json::from_str(&report.to_json()).unwrap();
        assert_eq!(json["level"], "tls");
        assert_eq!(json["min_tls"], "1.3");
        assert_eq!(json["stated"], true);
        assert_eq!(json["provider"]["name"], "aws-lc-rs");
        assert_eq!(
            json["provider"]["fips_module_active"],
            cfg!(feature = "fips")
        );
        assert_eq!(json["paths"][1]["posture"]["transport"], "TLS");
        assert_eq!(json["paths"][1]["posture"]["level"], "tls");
        assert!(json["paths"][2]["refused"]
            .as_str()
            .unwrap()
            .starts_with("subscriber ToEcho: "));
        assert!(json["features"].is_array());
    }

    #[test]
    fn a_refused_bind_is_in_the_report() {
        let mut exposed = publishers();
        exposed[0].address = "tcp://0.0.0.0:12000".into();
        let (report, _) = Report::build(&security("  transport: tls\n"), &exposed, &[], &[]);
        assert_eq!(report.refusals().len(), 1);
        assert!(
            report.refusals()[0]
                .starts_with("publisher records: `address` tcp://0.0.0.0:12000 is not loopback"),
            "{:?}",
            report.refusals()
        );
    }
}
