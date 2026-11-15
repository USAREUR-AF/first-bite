//! Put each record on a Zenoh key expression rendered from the record.
//!
//! Endpoints, mode and TLS identity come from a standard Zenoh JSON5
//! configuration file (`zenoh_config`); without one the session uses Zenoh's
//! defaults. The payload is the record as JSON, tagged `application/json`,
//! or with `encoding: cbor` as CBOR, tagged `application/cbor`.

use super::{Built, Sink, SinkError};
use crate::config::SubscriberConfig;
use crate::encoding::Encoding as RecordEncoding;
use crate::error::Problem;
use crate::security::zenoh_config::{check, ZenohFacts};
use crate::security::{provider, Policy, Security};
use async_trait::async_trait;
use log::{debug, info};
use serde::Deserialize;
use std::path::PathBuf;
use tokio::sync::OnceCell;
use zenoh::bytes::Encoding;

/// Key used when the subscriber sets no `key_template`.
pub const DEFAULT_KEY_TEMPLATE: &str = "telemetry/{system}/{location}/{interface}/{message}";

/// What a placeholder renders as when the record has no value for it.
const UNKNOWN: &str = "unknown";

/// Characters with a meaning in a Zenoh key expression. They may not appear
/// in the fixed text of a template; in a value they are replaced.
const RESERVED: [char; 4] = ['*', '$', '#', '?'];

/// The parts of a record a key can be built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placeholder {
    /// `{interface}`: the record's `subsystem`.
    Interface,
    /// `{message}`: the record's `message_type`.
    Message,
    /// `{system}`: the record's `data_source_system_type`.
    System,
    /// `{location}`: the record's `data_source_geo_location`.
    Location,
    /// `{network}`: the record's `data_source_network_name`.
    Network,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Text(String),
    Value(Placeholder),
}

/// The fields of a record that placeholders read.
#[derive(Debug, Default, Deserialize)]
pub struct KeyFields {
    #[serde(default)]
    subsystem: Option<String>,
    #[serde(default)]
    message_type: Option<String>,
    #[serde(default)]
    data_source_system_type: Option<String>,
    #[serde(default)]
    data_source_geo_location: Option<String>,
    #[serde(default)]
    data_source_network_name: Option<String>,
}

/// Why a key template cannot be used.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum KeyTemplateError {
    #[error("the template is empty")]
    Empty,
    #[error("unknown placeholder {{{0}}}; the placeholders are {{system}}, {{location}}, {{network}}, {{interface}}, {{message}}")]
    UnknownPlaceholder(String),
    #[error("a `{{` is not closed")]
    Unclosed,
    #[error("a `}}` has no opening `{{`")]
    Unopened,
    #[error("{0:?} has a meaning in a key expression and cannot appear in a template")]
    Reserved(char),
    #[error("the key would have an empty chunk: no leading, trailing or doubled `/`")]
    EmptyChunk,
}

/// A key expression with `{placeholders}`, checked once at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyTemplate {
    segments: Vec<Segment>,
}

impl KeyTemplate {
    pub fn parse(template: &str) -> Result<Self, KeyTemplateError> {
        if template.is_empty() {
            return Err(KeyTemplateError::Empty);
        }
        let mut segments = Vec::new();
        let mut rest = template;
        while !rest.is_empty() {
            let open = rest.find('{');
            let text = &rest[..open.unwrap_or(rest.len())];
            if text.contains('}') {
                return Err(KeyTemplateError::Unopened);
            }
            if let Some(reserved) = text.chars().find(|c| RESERVED.contains(c)) {
                return Err(KeyTemplateError::Reserved(reserved));
            }
            if !text.is_empty() {
                segments.push(Segment::Text(text.to_string()));
            }
            let Some(open) = open else { break };
            let after = &rest[open + 1..];
            let close = after.find('}').ok_or(KeyTemplateError::Unclosed)?;
            let placeholder = match &after[..close] {
                "interface" => Placeholder::Interface,
                "message" => Placeholder::Message,
                "system" => Placeholder::System,
                "location" => Placeholder::Location,
                "network" => Placeholder::Network,
                other => return Err(KeyTemplateError::UnknownPlaceholder(other.to_string())),
            };
            segments.push(Segment::Value(placeholder));
            rest = &after[close + 1..];
        }
        let template = KeyTemplate { segments };
        // A value never renders empty and never contains `/`, so the chunks
        // of any rendered key are empty exactly when these are.
        if template
            .render(&KeyFields::default())
            .split('/')
            .any(str::is_empty)
        {
            return Err(KeyTemplateError::EmptyChunk);
        }
        Ok(template)
    }

    /// The key for one record.
    pub fn render(&self, fields: &KeyFields) -> String {
        let mut key = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Text(text) => key.push_str(text),
                Segment::Value(placeholder) => {
                    let value = match placeholder {
                        Placeholder::Interface => &fields.subsystem,
                        Placeholder::Message => &fields.message_type,
                        Placeholder::System => &fields.data_source_system_type,
                        Placeholder::Location => &fields.data_source_geo_location,
                        Placeholder::Network => &fields.data_source_network_name,
                    };
                    push_chunk(&mut key, value.as_deref());
                }
            }
        }
        key
    }
}

/// Append a value in the form a key may carry: lower case, and every
/// character outside `[a-z0-9_-]` replaced by `_`. A value that is missing or
/// empty is written as `unknown`.
fn push_chunk(key: &mut String, value: Option<&str>) {
    let value = value.filter(|value| !value.is_empty()).unwrap_or(UNKNOWN);
    key.extend(value.chars().map(|c| match c.to_ascii_lowercase() {
        c @ ('a'..='z' | '0'..='9' | '_' | '-') => c,
        _ => '_',
    }));
}

/// Render the key for a record given as JSON.
pub fn render_key(template: &KeyTemplate, record: &str) -> Result<String, serde_json::Error> {
    Ok(template.render(&serde_json::from_str(record)?))
}

fn zenoh_error(error: zenoh::Error) -> SinkError {
    SinkError::Zenoh(error.to_string())
}

pub struct ZenohSink {
    config_path: Option<PathBuf>,
    template: KeyTemplate,
    encoding: RecordEncoding,
    session: OnceCell<zenoh::Session>,
}

/// The Zenoh encoding that names a record encoding.
pub fn zenoh_encoding(encoding: RecordEncoding) -> Encoding {
    match encoding {
        RecordEncoding::Json => Encoding::APPLICATION_JSON,
        RecordEncoding::Cbor => Encoding::APPLICATION_CBOR,
    }
}

impl ZenohSink {
    pub fn from_config(config: &SubscriberConfig) -> Result<Self, Problem> {
        let base = Security::unstated().base;
        Self::build(config, &base, &base).map(|built| built.0)
    }

    /// The sink held to `policy`: the Zenoh configuration is read and what
    /// it will do on the wire is checked against the level. `base` is the
    /// security block, whose TLS settings the Zenoh client inherits.
    pub(crate) fn secured(
        config: &SubscriberConfig,
        policy: &Policy,
        base: &Policy,
    ) -> Result<Built, Problem> {
        let (sink, posture, targets) = Self::build(config, policy, base)?;
        Ok(Built::new(sink, posture, targets))
    }

    fn build(
        config: &SubscriberConfig,
        policy: &Policy,
        base: &Policy,
    ) -> Result<
        (
            Self,
            crate::security::PathPosture,
            Vec<crate::security::ProbeTarget>,
        ),
        Problem,
    > {
        let template = config
            .key_template
            .as_deref()
            .unwrap_or(DEFAULT_KEY_TEMPLATE);
        let template = KeyTemplate::parse(template).map_err(|e| Problem::InvalidValue {
            key: "key_template",
            reason: e.to_string(),
        })?;
        let config_path = config.zenoh_config.as_deref().map(PathBuf::from);
        // Read the file now so that a wrong path or a bad file stops startup.
        let zenoh_config = match &config_path {
            Some(path) => zenoh::Config::from_file(path).map_err(|e| Problem::InvalidValue {
                key: "zenoh_config",
                reason: format!("{}: {e}", path.display()),
            })?,
            None => zenoh::Config::default(),
        };
        let (posture, targets) = check(
            &config.name,
            &ZenohFacts::read(&zenoh_config),
            policy,
            base,
            cfg!(feature = "quic"),
        )?;
        provider::ensure_installed(base);
        let sink = ZenohSink {
            config_path,
            template,
            encoding: RecordEncoding::from_config(config.encoding.as_deref())?,
            session: OnceCell::new(),
        };
        Ok((sink, posture, targets))
    }

    /// The sink's one session, opened on first use.
    async fn session(&self) -> Result<&zenoh::Session, SinkError> {
        self.session
            .get_or_try_init(|| async {
                let config = match &self.config_path {
                    Some(path) => zenoh::Config::from_file(path).map_err(zenoh_error)?,
                    None => zenoh::Config::default(),
                };
                let session = zenoh::open(config).await.map_err(zenoh_error)?;
                info!("opened Zenoh session {}", session.zid());
                Ok(session)
            })
            .await
    }
}

#[async_trait]
impl Sink for ZenohSink {
    /// Open the session.
    async fn prepare(&self) -> Result<(), SinkError> {
        self.session().await.map(|_| ())
    }

    async fn send(&self, _topic: &str, record: &str) -> Result<(), SinkError> {
        let key = render_key(&self.template, record)?;
        self.session()
            .await?
            .put(&key, self.encoding.encode(record)?.into_owned())
            .encoding(zenoh_encoding(self.encoding))
            .await
            .map_err(zenoh_error)?;
        debug!("put a record on {key}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{free_port, subscriber, temp_path};
    use std::time::Duration;

    fn key(template: &str, record: &str) -> String {
        render_key(&KeyTemplate::parse(template).unwrap(), record).unwrap()
    }

    const RECORD: &str = r#"{
        "subsystem": "WX",
        "message_type": "Observation",
        "data_source_system_type": "Weather Station",
        "data_source_geo_location": "Hilltop-7",
        "data_source_network_name": "ops_net",
        "message": {"station_id": "KX7"}
    }"#;

    #[test]
    fn default_template_renders_from_the_record() {
        assert_eq!(
            key(DEFAULT_KEY_TEMPLATE, RECORD),
            "telemetry/weather_station/hilltop-7/wx/observation"
        );
    }

    #[test]
    fn network_placeholder_and_fixed_text() {
        assert_eq!(
            key("volatile/{network}/stream/{interface}.{message}", RECORD),
            "volatile/ops_net/stream/wx.observation"
        );
        assert_eq!(key("telemetry/fixed/key", RECORD), "telemetry/fixed/key");
    }

    #[test]
    fn missing_null_and_empty_values_render_as_unknown() {
        assert_eq!(
            key(
                DEFAULT_KEY_TEMPLATE,
                r#"{"subsystem": null, "data_source_geo_location": ""}"#
            ),
            "telemetry/unknown/unknown/unknown/unknown"
        );
    }

    #[test]
    fn values_cannot_inject_key_expression_syntax() {
        let hostile = r#"{
            "subsystem": "a/b",
            "message_type": "**",
            "data_source_system_type": "$*#?",
            "data_source_geo_location": "Zürich Nord/@adv"
        }"#;
        let rendered = key(DEFAULT_KEY_TEMPLATE, hostile);
        assert_eq!(rendered, "telemetry/____/z_rich_nord__adv/a_b/__");
        assert_eq!(rendered.split('/').count(), 5);
        assert!(!rendered.contains(['*', '$', '#', '?', '@']));
        // Every rendered key is one Zenoh accepts as a concrete key.
        assert!(zenoh::key_expr::KeyExpr::try_from(rendered.as_str()).is_ok());
    }

    #[test]
    fn templates_are_checked_when_parsed() {
        use KeyTemplateError::*;
        let cases = [
            ("", Empty),
            ("telemetry/{site}", UnknownPlaceholder("site".into())),
            ("telemetry/{system", Unclosed),
            ("telemetry/system}", Unopened),
            ("telemetry/**", Reserved('*')),
            ("telemetry/$x/{system}", Reserved('$')),
            ("telemetry/a#b", Reserved('#')),
            ("telemetry/a?b", Reserved('?')),
            ("/telemetry/{system}", EmptyChunk),
            ("telemetry/{system}/", EmptyChunk),
            ("telemetry//{system}", EmptyChunk),
        ];
        for (template, expected) in cases {
            assert_eq!(KeyTemplate::parse(template), Err(expected), "{template:?}");
        }
    }

    #[test]
    fn a_bad_template_or_config_path_stops_startup() {
        let problem = |body: &str| match ZenohSink::from_config(&subscriber(body)) {
            Ok(_) => panic!("{body} built a sink"),
            Err(problem) => problem.to_string(),
        };
        assert!(
            problem("    subscriber_type: zenoh\n    key_template: \"telemetry/{nope}\"\n")
                .starts_with("`key_template`: unknown placeholder {nope}")
        );
        assert!(problem(
            "    subscriber_type: zenoh\n    zenoh_config: /nonexistent/zenoh.json5\n"
        )
        .starts_with("`zenoh_config`: /nonexistent/zenoh.json5: "));
        assert!(ZenohSink::from_config(&subscriber("    subscriber_type: zenoh\n")).is_ok());
    }

    /// A subscriber session listens on a loopback TCP port; the sink connects
    /// to it through a JSON5 configuration file and puts one record.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn record_round_trips_over_loopback_tcp() {
        // The port is free when chosen; another process could take it before
        // the listener binds, so a failed open picks again.
        let mut attempt = 0;
        let (listener, port) = loop {
            let port = free_port();
            let config = zenoh::Config::from_json5(&format!(
                r#"{{
                    mode: "peer",
                    listen: {{ endpoints: ["tcp/127.0.0.1:{port}"] }},
                    scouting: {{ multicast: {{ enabled: false }}, gossip: {{ enabled: false }} }},
                }}"#
            ))
            .unwrap();
            match zenoh::open(config).await {
                Ok(session) => break (session, port),
                Err(e) if attempt < 5 => {
                    attempt += 1;
                    eprintln!("port {port} was taken ({e}); trying another");
                }
                Err(e) => panic!("cannot open the listening session: {e}"),
            }
        };
        let receiver = listener.declare_subscriber("telemetry/**").await.unwrap();

        let config_path = temp_path("zenoh-sink.json5");
        std::fs::write(
            &config_path,
            format!(
                r#"{{
                    mode: "peer",
                    connect: {{ endpoints: ["tcp/127.0.0.1:{port}"] }},
                    listen: {{ endpoints: ["tcp/127.0.0.1:0"] }},
                    scouting: {{ multicast: {{ enabled: false }}, gossip: {{ enabled: false }} }},
                }}"#
            ),
        )
        .unwrap();
        let record = r#"{"subsystem":"WX","message_type":"Position","data_source_system_type":"Weather Station","data_source_geo_location":"Hilltop-7","message":{"station_id":"KX7","altitude":136.5}}"#;
        for (setting, encoding) in [
            ("", Encoding::APPLICATION_JSON),
            ("    encoding: json\n", Encoding::APPLICATION_JSON),
            ("    encoding: cbor\n", Encoding::APPLICATION_CBOR),
        ] {
            let sink = ZenohSink::from_config(&subscriber(&format!(
                "    subscriber_type: zenoh\n    zenoh_config: \"{}\"\n{setting}",
                config_path.display()
            )))
            .unwrap();
            sink.prepare().await.unwrap();

            // The subscription takes a moment to reach the sink's session,
            // and a put before then goes nowhere; put until one arrives.
            let mut sample = None;
            for _ in 0..100 {
                sink.send("WX/Position", record).await.unwrap();
                if let Ok(Ok(received)) =
                    tokio::time::timeout(Duration::from_millis(100), receiver.recv_async()).await
                {
                    sample = Some(received);
                    break;
                }
            }
            let sample = sample.expect("no sample arrived within ten seconds");
            assert_eq!(
                sample.key_expr().as_str(),
                "telemetry/weather_station/hilltop-7/wx/position"
            );
            assert_eq!(sample.encoding(), &encoding, "{setting:?}");
            let payload = sample.payload().to_bytes();
            if encoding == Encoding::APPLICATION_CBOR {
                assert_eq!(payload.first(), Some(&0xa5));
                let back: serde_json::Value =
                    serde_json::from_str(&crate::encoding::cbor_to_json(&payload).unwrap())
                        .unwrap();
                let sent: serde_json::Value = serde_json::from_str(record).unwrap();
                assert_eq!(back, sent);
            } else {
                assert_eq!(payload.as_ref(), record.as_bytes());
            }
            // Samples the retry loop put after the one that arrived.
            while tokio::time::timeout(Duration::from_millis(200), receiver.recv_async())
                .await
                .is_ok()
            {}
        }
        let _ = std::fs::remove_file(&config_path);
    }
}
