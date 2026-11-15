//! The sink behind `subscriber_type: goat`: decoded records onto a goat data
//! fabric, with the service announcement, the schema declarations and a
//! heartbeat goat's conventions ask of a publisher.
//!
//! The record is put unchanged. Identity is not added to it: the router
//! takes it from the certificate, and it is the first chunk of every key.

use super::cert::{now_unix, validate};
use super::config::GoatConfig;
use super::keys::KeyLayout;
use super::session::{not_delivered_advice, open, probe_with, Delivery};
use super::{registered, Declarations, GoatError};
use crate::config::SubscriberConfig;
use crate::error::Problem;
use crate::sink::{Sink, SinkError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OnceCell;
use zenoh::bytes::Encoding;

fn sink_error(error: GoatError) -> SinkError {
    SinkError::Zenoh(error.to_string())
}

fn zenoh_error(error: zenoh::Error) -> SinkError {
    SinkError::Zenoh(error.to_string())
}

/// Counts reported in the heartbeat.
#[derive(Debug, Default)]
struct Counters {
    published: AtomicU64,
    undecoded: AtomicU64,
    failed: AtomicU64,
}

pub struct GoatSink {
    config: GoatConfig,
    layout: KeyLayout,
    declarations: Declarations,
    /// The record keys of the dictionary's messages: the only keys written.
    record_keys: HashSet<String>,
    config_digest: String,
    started: DateTime<Utc>,
    counters: Arc<Counters>,
    session: OnceCell<zenoh::Session>,
}

impl GoatSink {
    /// Build the sink from the file `GOAT_CONFIG_PATH` names. The
    /// configuration and the certificates are checked here, so a mistake
    /// stops startup before anything connects.
    pub fn from_config(config: &SubscriberConfig) -> Result<Self, Problem> {
        // docs/goat/contract.md: the record goes out as `application/json`
        // and its declaration is a JSON Schema.
        crate::sink::json_only(
            config,
            "the schema declared for each record on a goat fabric is a JSON Schema",
        )?;
        Self::from_env().map_err(|e| Problem::InvalidValue {
            key: "subscriber_type",
            reason: format!("goat: {e}"),
        })
    }

    fn from_env() -> Result<Self, GoatError> {
        let config = GoatConfig::from_env()?;
        validate(&config, now_unix())?;
        Self::new(config, registered())
    }

    fn new(config: GoatConfig, declarations: Declarations) -> Result<Self, GoatError> {
        let layout = KeyLayout::new(&config)?;
        let config_digest = config_digest(&config, &declarations);
        let record_keys = declarations
            .messages
            .iter()
            .map(|message| layout.record_key(&message.interface, &message.message))
            .collect();
        Ok(GoatSink {
            config,
            layout,
            declarations,
            record_keys,
            config_digest,
            started: Utc::now(),
            counters: Arc::new(Counters::default()),
            session: OnceCell::new(),
        })
    }

    async fn session(&self) -> Result<&zenoh::Session, SinkError> {
        self.session
            .get_or_try_init(|| async {
                let session = open(&self.config, true).await.map_err(sink_error)?;
                info!(
                    "goat: joined {} as {}; Zenoh session {}",
                    self.config.goatnet.endpoints.join(", "),
                    self.layout.own_pattern().trim_end_matches("/**"),
                    session.zid()
                );
                Ok(session)
            })
            .await
    }

    /// The key a record is put on, or `None` for a record that is not one of
    /// the dictionary's messages: a packet that did not decode, or a message
    /// type the dictionary has no definition for. Publishing those would add
    /// a key per unknown type code, and goat asks for a fixed set of keys.
    fn key_for(&self, record: &str) -> Result<Option<String>, serde_json::Error> {
        Ok(self
            .layout
            .record_key_of(record)?
            .filter(|key| self.record_keys.contains(key)))
    }

    async fn put_json(&self, key: &str, value: &Value) -> Result<(), SinkError> {
        self.session()
            .await?
            .put(key, value.to_string())
            .encoding(Encoding::APPLICATION_JSON)
            .await
            .map_err(zenoh_error)
    }
}

/// SHA-256 over what decides this publisher's keys and payloads: the
/// identity's prefix, the publish settings and the dictionary.
fn config_digest(config: &GoatConfig, declarations: &Declarations) -> String {
    let publish = &config.publish;
    let prefix = config
        .principal()
        .map(|principal| principal.prefix().to_string())
        .unwrap_or_default();
    let mut hasher = Sha256::new();
    for part in [
        prefix.as_str(),
        &publish.format,
        &publish.site,
        &publish.instance,
        &publish.version.map(|n| n.to_string()).unwrap_or_default(),
        declarations.dictionary.as_deref().unwrap_or_default(),
    ] {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    for message in &declarations.messages {
        hasher.update(message.interface.as_bytes());
        hasher.update([0]);
        hasher.update(message.message.as_bytes());
        hasher.update([0]);
    }
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// The announcement: the fields goat's shim convention names (ADR 0209).
pub fn announcement(
    config: &GoatConfig,
    layout: &KeyLayout,
    declarations: &Declarations,
    config_digest: &str,
) -> Value {
    json!({
        "class": config.publish.format,
        "version": declarations.version(),
        "instance": config.publish.instance,
        "site": config.publish.site,
        "capabilities": ["publish"],
        "translates_from": declarations
            .dictionary
            .clone()
            .unwrap_or_else(|| config.publish.format.clone()),
        "translates_to": [layout.records_pattern()],
        "config_digest": config_digest,
    })
}

fn heartbeat(
    config: &GoatConfig,
    version: &str,
    started: DateTime<Utc>,
    counters: &Counters,
) -> Value {
    json!({
        "status": "ok",
        "class": config.publish.format,
        "instance": config.publish.instance,
        "site": config.publish.site,
        "version": version,
        "started": started.to_rfc3339(),
        "time": Utc::now().to_rfc3339(),
        "records_published": counters.published.load(Ordering::Relaxed),
        "records_not_decoded": counters.undecoded.load(Ordering::Relaxed),
        "records_failed": counters.failed.load(Ordering::Relaxed),
    })
}

/// The schema declaration for one message: its record schema with `$id` set
/// to the key it is declared on.
pub fn schema_declaration(key: &str, schema: &Value) -> Value {
    let mut declaration = schema.clone();
    if let Some(object) = declaration.as_object_mut() {
        object.insert("$id".to_string(), json!(key));
    }
    declaration
}

#[async_trait]
impl Sink for GoatSink {
    /// Open the session; announce; declare the schemas; start the heartbeat;
    /// confirm that the router delivers what this certificate writes.
    async fn prepare(&self) -> Result<(), SinkError> {
        let session = self.session().await?.clone();
        let announce = announcement(
            &self.config,
            &self.layout,
            &self.declarations,
            &self.config_digest,
        );
        self.put_json(&self.layout.announce_key(), &announce)
            .await?;
        for message in &self.declarations.messages {
            let key = self.layout.schema_key(&message.interface, &message.message);
            self.put_json(&key, &schema_declaration(&key, &message.schema))
                .await?;
        }
        info!(
            "goat: announced on {} and declared {} schema(s) under {}",
            self.layout.announce_key(),
            self.declarations.messages.len(),
            self.layout.schemas_pattern()
        );
        if self.declarations.messages.is_empty() {
            warn!("goat: no dictionary is loaded, so no record will be decoded and none published");
        }

        if self.config.publish.self_check {
            let key = self.layout.health_key();
            let first = heartbeat(
                &self.config,
                self.declarations.version(),
                self.started,
                &self.counters,
            )
            .to_string();
            match probe_with(&self.config, &session, &key, &first).await {
                Ok(Delivery::Delivered { after, .. }) => info!(
                    "goat: the router delivers this publisher's samples (read back on {key} after {} ms)",
                    after.as_millis()
                ),
                Ok(Delivery::NotDelivered { .. }) => error!(
                    "goat: {}",
                    not_delivered_advice(&self.config, &self.layout, &key)
                ),
                Err(e) => warn!("goat: could not check delivery: {e}"),
            }
        }

        let config = self.config.clone();
        let key = self.layout.health_key();
        let started = self.started;
        let version = self.declarations.version().to_string();
        let counters = Arc::clone(&self.counters);
        let period = Duration::from_secs(config.publish.health_interval_seconds);
        tokio::spawn(async move {
            let mut ticks = tokio::time::interval(period);
            loop {
                ticks.tick().await;
                let beat = heartbeat(&config, &version, started, &counters).to_string();
                if let Err(e) = session
                    .put(&key, beat)
                    .encoding(Encoding::APPLICATION_JSON)
                    .await
                {
                    warn!("goat: heartbeat on {key} not sent: {e}");
                }
            }
        });
        Ok(())
    }

    async fn send(&self, _topic: &str, record: &str) -> Result<(), SinkError> {
        let Some(key) = self.key_for(record)? else {
            self.counters.undecoded.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        };
        let sent = async {
            self.session()
                .await?
                .put(&key, record)
                .encoding(Encoding::APPLICATION_JSON)
                .await
                .map_err(zenoh_error)
        }
        .await;
        match &sent {
            Ok(()) => {
                self.counters.published.fetch_add(1, Ordering::Relaxed);
                debug!("goat: put a record on {key}");
            }
            Err(_) => {
                self.counters.failed.fetch_add(1, Ordering::Relaxed);
            }
        }
        sent
    }
}

#[cfg(test)]
mod tests {
    use super::super::cert::tests::fixture_config;
    use super::super::config::tests::SLOT;
    use super::super::MessageDeclaration;
    use super::*;
    use crate::test_support::subscriber;

    fn declarations() -> Declarations {
        Declarations {
            application_version: Some("9.8.7".to_string()),
            dictionary: Some("weather-station 1.0".to_string()),
            messages: vec![MessageDeclaration {
                interface: "WX".to_string(),
                message: "Observation".to_string(),
                schema: json!({"type": "object", "title": "Observation"}),
            }],
        }
    }

    #[test]
    fn the_announcement_carries_the_fields_of_goats_shim_convention() {
        let sink = GoatSink::new(fixture_config(), declarations()).unwrap();
        let announce = announcement(
            &sink.config,
            &sink.layout,
            &sink.declarations,
            &sink.config_digest,
        );
        let fields: Vec<&str> = announce
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            fields,
            [
                "capabilities",
                "class",
                "config_digest",
                "instance",
                "site",
                "translates_from",
                "translates_to",
                "version"
            ]
        );
        assert_eq!(announce["class"], "weather-station");
        assert_eq!(announce["translates_from"], "weather-station 1.0");
        assert_eq!(
            announce["translates_to"][0],
            format!("{SLOT}/telemetry/weather-station/hilltop-7/**")
        );
        let digest = announce["config_digest"].as_str().unwrap();
        assert!(
            digest.starts_with("sha256:") && digest.len() == 71,
            "{digest}"
        );
    }

    #[test]
    fn the_digest_changes_with_the_keys_and_not_with_the_certificate_paths() {
        let base = GoatSink::new(fixture_config(), declarations()).unwrap();
        let mut moved = fixture_config();
        moved.identity.certificate = "/elsewhere/cert.pem".into();
        assert_eq!(base.config_digest, config_digest(&moved, &declarations()));
        let mut other_site = fixture_config();
        other_site.publish.site = "valley-2".to_string();
        assert_ne!(
            base.config_digest,
            config_digest(&other_site, &declarations())
        );
        assert_ne!(
            base.config_digest,
            config_digest(&fixture_config(), &Declarations::default())
        );
    }

    #[test]
    fn a_schema_declaration_is_identified_by_its_key() {
        let key =
            format!("attest/schema/{SLOT}/telemetry/weather-station/hilltop-7/wx/observation");
        let declaration = schema_declaration(&key, &declarations().messages[0].schema);
        assert_eq!(declaration["$id"], key);
        assert_eq!(declaration["title"], "Observation");
    }

    #[test]
    fn only_messages_of_the_dictionary_have_a_key() {
        let sink = GoatSink::new(fixture_config(), declarations()).unwrap();
        let record = |message: &str| format!(r#"{{"subsystem":"WX","message_type":"{message}"}}"#);
        assert_eq!(
            sink.key_for(&record("Observation")).unwrap(),
            Some(format!(
                "{SLOT}/telemetry/weather-station/hilltop-7/wx/observation"
            ))
        );
        // A type code the dictionary does not define, and an undecoded packet.
        assert_eq!(sink.key_for(&record("42")).unwrap(), None);
        assert_eq!(sink.key_for(r#"{"subsystem":null}"#).unwrap(), None);
        let no_dictionary = GoatSink::new(fixture_config(), Declarations::default()).unwrap();
        assert_eq!(no_dictionary.key_for(&record("Observation")).unwrap(), None);
    }

    #[test]
    fn announcement_and_heartbeat_report_the_application_version() {
        let sink = GoatSink::new(fixture_config(), declarations()).unwrap();
        let announce = announcement(
            &sink.config,
            &sink.layout,
            &sink.declarations,
            &sink.config_digest,
        );
        assert_eq!(announce["version"], "9.8.7");
        // With no application registered, this crate's own version.
        assert_eq!(Declarations::default().version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn the_heartbeat_reports_the_counts() {
        let sink = GoatSink::new(fixture_config(), declarations()).unwrap();
        sink.counters.published.store(7, Ordering::Relaxed);
        sink.counters.undecoded.store(2, Ordering::Relaxed);
        let beat = heartbeat(
            &sink.config,
            sink.declarations.version(),
            sink.started,
            &sink.counters,
        );
        assert_eq!(beat["status"], "ok");
        assert_eq!(beat["version"], "9.8.7");
        assert_eq!(beat["records_published"], 7);
        assert_eq!(beat["records_not_decoded"], 2);
        assert_eq!(beat["instance"], "tap-1");
    }

    /// The variable is this test's alone: no other test in the crate reads
    /// or writes `GOAT_CONFIG_PATH`.
    #[test]
    fn without_a_configuration_the_subscriber_is_refused_with_the_setting_to_add() {
        std::env::remove_var(crate::goat::GOAT_CONFIG_ENV);
        let error = match crate::build_sink(&subscriber("    subscriber_type: goat\n")) {
            Ok(_) => panic!("built a goat sink with no configuration"),
            Err(e) => e.to_string(),
        };
        assert!(
            error.starts_with(
                "subscriber Example: `subscriber_type`: goat: GOAT_CONFIG_PATH is not set"
            ),
            "{error}"
        );
    }
}
