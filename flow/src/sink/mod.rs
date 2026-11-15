//! Where a subscriber's records go.
//!
//! Every destination implements [`Sink`]; [`build_sink`] is the one place
//! that turns a subscriber's configuration into a sink.
//!
//! A sink is handed each record as JSON text and writes it in the encoding
//! its `encoding` key names; [`crate::encoding`] describes the two.
//!
//! | `subscriber_type` | `subtype` | Sink | Cargo feature |
//! |---|---|---|---|
//! | `objectstorage` | `azureobject` | Azure Blob Storage | `azure` |
//! | `objectstorage` | `awsobject` | S3 and S3-compatible stores | `aws` |
//! | `rest` | any; not used | HTTP POST | always built |
//! | `local` | unset, `locallog` | append to a local file | always built |
//! | `stateful` | unset, `messagerates` | per-topic counts to a local file | always built |
//! | `udp` | unset | one datagram per payload, unicast or multicast | always built |
//! | `tcp` | unset | a stream of payloads, optionally inside TLS | always built |
//! | `zenoh` | unset | Zenoh put | `zenoh` |
//! | `goat` | unset | Zenoh put on a goat data fabric | `goat` |

#[cfg(feature = "aws")]
pub mod aws_s3;
#[cfg(feature = "azure")]
pub mod azure_blob;
pub mod local_log;
pub mod message_rates;
pub mod network;
#[cfg(any(feature = "aws", feature = "azure"))]
pub mod object_path;
pub mod rest;
#[cfg(feature = "zenoh")]
pub mod zenoh;

use crate::config::SubscriberConfig;
use crate::encoding::Encoding;
use crate::error::{ConfigError, Problem};
use crate::secret::{reference, Reference, Secret};
use crate::security::{PathPosture, Policy, ProbeTarget, Security};
use async_trait::async_trait;

/// Why a sink did not take a record, or could not be prepared.
#[derive(Debug, thiserror::Error)]
pub enum SinkError {
    #[error("record is not valid JSON: {0}")]
    Record(#[from] serde_json::Error),
    #[error("{0}")]
    Encoding(#[from] crate::encoding::EncodingError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP request failed: {}", causes(.0))]
    Http(reqwest::Error),
    #[error("endpoint answered {status}: {body}")]
    HttpStatus {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("Cursor-on-Target: {0}")]
    Cot(#[from] cotify::CotError),
    #[error("{0}")]
    Network(String),
    #[cfg(feature = "aws")]
    #[error("S3: {0}")]
    S3(String),
    #[cfg(feature = "zenoh")]
    #[error("Zenoh: {0}")]
    Zenoh(String),
}

/// The URL is left out of the error: it can carry credentials, and the log
/// line that reports the error names the endpoint in a form that cannot.
impl From<reqwest::Error> for SinkError {
    fn from(error: reqwest::Error) -> Self {
        SinkError::Http(error.without_url())
    }
}

/// An error and what caused it, on one line.
fn causes(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// Most bytes of an HTTP answer read into memory. An answer is read only to
/// report an error, or by a test.
pub(crate) const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Most characters of an answer quoted in an error.
pub(crate) const MAX_QUOTED_CHARS: usize = 512;

/// The body of `response`, up to [`MAX_RESPONSE_BYTES`]; the rest is not
/// read, so an endpoint cannot make the process hold an answer of any size.
pub(crate) async fn read_body(mut response: reqwest::Response) -> Vec<u8> {
    let mut body = Vec::new();
    while body.len() < MAX_RESPONSE_BYTES {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                let room = MAX_RESPONSE_BYTES - body.len();
                body.extend_from_slice(&chunk[..chunk.len().min(room)]);
            }
            Ok(None) | Err(_) => break,
        }
    }
    body
}

/// The start of an answer's body, for an error message.
pub(crate) fn quoted(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    match text.char_indices().nth(MAX_QUOTED_CHARS) {
        Some((end, _)) => format!("{}… ({} bytes read)", &text[..end], body.len()),
        None => text.into_owned(),
    }
}

/// A destination for records.
#[async_trait]
pub trait Sink: Send + Sync {
    /// One-time setup before the first record: create the bucket or
    /// container, open the session. The default does nothing.
    async fn prepare(&self) -> Result<(), SinkError> {
        Ok(())
    }

    /// Deliver one record that arrived on `topic`. `record` is JSON text.
    async fn send(&self, topic: &str, record: &str) -> Result<(), SinkError>;

    /// The run is ending: deliver what the sink still holds, within `within`,
    /// and close its connections. Returns how many records `send` accepted
    /// that were still undelivered at the end. A sink whose `send` returns
    /// only once the record is delivered holds nothing.
    async fn finish(&self, _within: std::time::Duration) -> u64 {
        0
    }

    /// Records `send` accepted that the sink then could not deliver.
    fn lost(&self) -> u64 {
        0
    }
}

/// Build the sink a subscriber's configuration asks for, held to the
/// security block in force. An unknown `subscriber_type`/`subtype`, a sink
/// whose cargo feature is off, a missing or invalid key, and a requirement
/// of the security block the sink cannot meet are errors that name the
/// subscriber.
pub fn build_sink(config: &SubscriberConfig) -> Result<Box<dyn Sink>, ConfigError> {
    build_sink_secured(config, &crate::security::current())
}

/// [`build_sink`], held to `security`.
pub fn build_sink_secured(
    config: &SubscriberConfig,
    security: &Security,
) -> Result<Box<dyn Sink>, ConfigError> {
    describe(config, security).map(|built| built.sink)
}

/// What a subscriber's sink will use on the wire, and the endpoints a probe
/// can handshake with. The sink is built exactly as for a capture run, which
/// connects to nothing, and then dropped.
pub fn describe_sink(
    config: &SubscriberConfig,
    security: &Security,
) -> Result<(PathPosture, Vec<ProbeTarget>), ConfigError> {
    describe(config, security).map(|built| (built.posture, built.targets))
}

/// A sink, with what it will use on the wire.
pub(crate) struct Built {
    sink: Box<dyn Sink>,
    posture: PathPosture,
    targets: Vec<ProbeTarget>,
}

impl Built {
    pub(crate) fn new(
        sink: impl Sink + 'static,
        posture: PathPosture,
        targets: impl IntoIterator<Item = ProbeTarget>,
    ) -> Self {
        Built {
            sink: Box::new(sink),
            posture,
            targets: targets.into_iter().collect(),
        }
    }
}

fn describe(config: &SubscriberConfig, security: &Security) -> Result<Built, ConfigError> {
    select(config, security).map_err(|problem| ConfigError::Subscriber {
        subscriber: config.name.clone(),
        problem,
    })
}

fn select(config: &SubscriberConfig, security: &Security) -> Result<Built, Problem> {
    // Checked for every sink, so a mistyped encoding is reported even where
    // the sink's feature is off.
    Encoding::from_config(config.encoding.as_deref())?;
    let policy = security.for_sink(config.security.as_ref())?;
    crate::security::check_feed(config, &policy)?;
    // A sink that writes a file on this machine has no network path.
    let file = |kind: &str, path: &str| {
        let mut posture = PathPosture::local(&config.name, kind, path, policy.transport);
        posture.overrides = policy.overrides.iter().map(ToString::to_string).collect();
        posture.override_reason = policy.reason.clone();
        posture
    };
    match (config.subscriber_type.as_str(), config.subtype.as_deref()) {
        ("objectstorage", Some("azureobject")) => azure_blob(config, &policy),
        ("objectstorage", Some("awsobject")) => aws_s3(config, &policy),
        ("rest", _) => rest::RestSink::secured(config, &policy),
        ("local", None | Some("locallog")) => Ok(Built::new(
            local_log::LocalLogSink::from_config(config)?,
            file("local", config.local_logfile.as_deref().unwrap_or("")),
            None,
        )),
        ("stateful", None | Some("messagerates")) => {
            json_only(config, "it counts records and stores none")?;
            Ok(Built::new(
                message_rates::MessageRatesSink::from_config(config),
                file(
                    "stateful",
                    config
                        .local_logfile
                        .as_deref()
                        .unwrap_or("messagerates.log"),
                ),
                None,
            ))
        }
        ("udp" | "tcp", None) => network::NetworkSink::secured(config, &policy),
        ("zenoh", None) => zenoh(config, &policy, security),
        ("goat", None) => goat(config, &policy, security),
        _ => Err(Problem::UnknownSink {
            subscriber_type: config.subscriber_type.clone(),
            subtype: config.subtype.clone(),
        }),
    }
}

#[cfg(feature = "azure")]
fn azure_blob(config: &SubscriberConfig, policy: &Policy) -> Result<Built, Problem> {
    azure_blob::AzureBlobSink::secured(config, policy)
}

#[cfg(not(feature = "azure"))]
fn azure_blob(_: &SubscriberConfig, _: &Policy) -> Result<Built, Problem> {
    Err(Problem::FeatureOff {
        sink: "objectstorage/azureobject",
        feature: "azure",
    })
}

#[cfg(feature = "aws")]
fn aws_s3(config: &SubscriberConfig, policy: &Policy) -> Result<Built, Problem> {
    aws_s3::S3Sink::secured(config, policy)
}

#[cfg(not(feature = "aws"))]
fn aws_s3(_: &SubscriberConfig, _: &Policy) -> Result<Built, Problem> {
    Err(Problem::FeatureOff {
        sink: "objectstorage/awsobject",
        feature: "aws",
    })
}

#[cfg(feature = "zenoh")]
fn zenoh(
    config: &SubscriberConfig,
    policy: &Policy,
    security: &Security,
) -> Result<Built, Problem> {
    zenoh::ZenohSink::secured(config, policy, &security.base)
}

#[cfg(not(feature = "zenoh"))]
fn zenoh(_: &SubscriberConfig, _: &Policy, _: &Security) -> Result<Built, Problem> {
    Err(Problem::FeatureOff {
        sink: "zenoh",
        feature: "zenoh",
    })
}

/// The goat sink opens a Zenoh session from a configuration it generates;
/// that configuration is held to the level like one read from a file.
#[cfg(feature = "goat")]
fn goat(config: &SubscriberConfig, policy: &Policy, security: &Security) -> Result<Built, Problem> {
    use crate::security::zenoh_config::{check, ZenohFacts};
    let sink = crate::goat::sink::GoatSink::from_config(config)?;
    let invalid = |reason: String| Problem::InvalidValue {
        key: "subscriber_type",
        reason: format!("goat: {reason}"),
    };
    let text = sink
        .zenoh_config_text()
        .map_err(|e| invalid(e.to_string()))?;
    let session = ::zenoh::Config::from_json5(&text).map_err(|e| invalid(e.to_string()))?;
    let (mut posture, targets) = check(
        &config.name,
        &ZenohFacts::read(&session),
        policy,
        &security.base,
        cfg!(feature = "quic"),
    )?;
    posture.kind = "goat".to_string();
    crate::security::provider::ensure_installed(&security.base);
    Ok(Built::new(sink, posture, targets))
}

#[cfg(not(feature = "goat"))]
fn goat(_: &SubscriberConfig, _: &Policy, _: &Security) -> Result<Built, Problem> {
    Err(Problem::FeatureOff {
        sink: "goat",
        feature: "goat",
    })
}

/// Options for a file a sink writes records or counts to: created when
/// missing and, on Unix, readable by its owner and group only, whatever the
/// umask allows beyond that. An existing file keeps its mode.
pub(crate) fn record_file_options() -> std::fs::OpenOptions {
    let mut options = std::fs::OpenOptions::new();
    options.create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o640);
    }
    options
}

/// The encoding of a sink that writes JSON only. `why` completes "this sink
/// writes JSON only: ".
pub(crate) fn json_only(config: &SubscriberConfig, why: &str) -> Result<(), Problem> {
    match Encoding::from_config(config.encoding.as_deref())? {
        Encoding::Json => Ok(()),
        other => Err(Problem::InvalidValue {
            key: "encoding",
            reason: format!(
                "{:?} cannot be used here; this sink writes JSON only: {why}",
                other.name()
            ),
        }),
    }
}

/// The value of a key the sink cannot work without.
pub(crate) fn required<'a>(
    value: &'a Option<String>,
    key: &'static str,
) -> Result<&'a str, Problem> {
    value
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or(Problem::MissingKey(key))
}

/// The credential in a key the sink cannot work without. A value that is
/// still a reference was never resolved, and is refused here so that the
/// text of a reference is not sent to a service as a credential.
#[cfg_attr(not(any(feature = "aws", feature = "azure")), allow(dead_code))]
pub(crate) fn required_secret<'a>(
    value: &'a Option<Secret>,
    key: &'static str,
) -> Result<&'a str, Problem> {
    let value = value
        .as_ref()
        .map(Secret::expose)
        .filter(|value| !value.is_empty())
        .ok_or(Problem::MissingKey(key))?;
    match reference(value) {
        Reference::None => Ok(value),
        _ => Err(Problem::InvalidValue {
            key,
            reason: "is a reference that has not been resolved".to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::subscriber;

    fn refusal(body: &str) -> String {
        match build_sink(&subscriber(body)) {
            Ok(_) => panic!("{body} built a sink"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn a_long_answer_is_quoted_in_part() {
        assert_eq!(quoted(b"short"), "short");
        let long = "é".repeat(MAX_QUOTED_CHARS * 2);
        let cut = quoted(long.as_bytes());
        assert!(cut.starts_with(&"é".repeat(MAX_QUOTED_CHARS)));
        assert!(
            cut.ends_with(&format!("… ({} bytes read)", long.len())),
            "{cut}"
        );
    }

    #[test]
    fn a_credential_that_is_still_a_reference_is_refused() {
        use crate::secret::Secret;
        let unresolved = Some(Secret::new("${BLOB_KEY}"));
        assert_eq!(
            required_secret(&unresolved, "cloud_access_key")
                .unwrap_err()
                .to_string(),
            "`cloud_access_key`: is a reference that has not been resolved"
        );
        assert_eq!(
            required_secret(&None, "cloud_access_key")
                .unwrap_err()
                .to_string(),
            "`cloud_access_key` is required"
        );
        assert_eq!(
            required_secret(&Some(Secret::new("value")), "cloud_access_key").unwrap(),
            "value"
        );
    }

    #[test]
    fn sinks_that_are_always_built() {
        for body in [
            "    subscriber_type: rest\n    subtype: echo\n    endpoint_url: \"127.0.0.1\"\n    endpoint_port: 5001\n",
            "    subscriber_type: rest\n    endpoint_url: \"127.0.0.1\"\n    endpoint_port: 5001\n    endpoint_path: /records\n",
            "    subscriber_type: local\n    subtype: locallog\n    local_logfile: records.log\n",
            "    subscriber_type: stateful\n    subtype: messagerates\n",
        ] {
            assert!(build_sink(&subscriber(body)).is_ok(), "{body}");
        }
    }

    #[test]
    fn an_unknown_encoding_names_the_subscriber_and_the_choices() {
        assert_eq!(
            refusal("    subscriber_type: local\n    local_logfile: records.log\n    encoding: msgpack\n"),
            "subscriber Example: `encoding`: \"msgpack\" is not one of json, cbor"
        );
        // Also where the sink itself is not compiled in or not known.
        assert_eq!(
            refusal("    subscriber_type: carrierpigeon\n    encoding: xml\n"),
            "subscriber Example: `encoding`: \"xml\" is not one of json, cbor"
        );
    }

    #[test]
    fn sinks_that_count_or_declare_json_refuse_cbor() {
        let error = refusal("    subscriber_type: stateful\n    encoding: cbor\n");
        assert!(
            error.starts_with("subscriber Example: `encoding`: \"cbor\" cannot be used here; this sink writes JSON only: "),
            "{error}"
        );
        for body in [
            "    subscriber_type: rest\n    endpoint_url: \"127.0.0.1\"\n    endpoint_port: 5001\n    encoding: cbor\n",
            "    subscriber_type: local\n    local_logfile: records.cborseq\n    encoding: cbor\n",
            "    subscriber_type: local\n    local_logfile: records.log\n    encoding: json\n",
        ] {
            assert!(build_sink(&subscriber(body)).is_ok(), "{body}");
        }
    }

    #[test]
    fn unknown_types_name_the_subscriber() {
        assert_eq!(
            refusal("    subscriber_type: carrierpigeon\n"),
            "subscriber Example: unknown subscriber_type \"carrierpigeon\" with subtype unset"
        );
        assert_eq!(
            refusal("    subscriber_type: objectstorage\n    subtype: gcsobject\n"),
            "subscriber Example: unknown subscriber_type \"objectstorage\" with subtype \"gcsobject\""
        );
        // The database sinks never stored anything and are gone.
        assert!(
            refusal("    subscriber_type: database\n    subtype: azuredatabase\n")
                .contains("unknown subscriber_type \"database\"")
        );
    }

    #[test]
    fn missing_keys_name_the_subscriber_and_the_key() {
        assert_eq!(
            refusal("    subscriber_type: rest\n    subtype: echo\n    endpoint_port: 5001\n"),
            "subscriber Example: `endpoint_url` is required"
        );
        assert_eq!(
            refusal("    subscriber_type: local\n"),
            "subscriber Example: `local_logfile` is required"
        );
    }

    #[cfg(not(feature = "azure"))]
    #[test]
    fn azure_needs_its_feature() {
        let error = refusal("    subscriber_type: objectstorage\n    subtype: azureobject\n");
        assert!(
            error.starts_with("subscriber Example: sink objectstorage/azureobject is not compiled in; rebuild with the cargo feature `azure`"),
            "{error}"
        );
    }

    #[cfg(not(feature = "aws"))]
    #[test]
    fn aws_needs_its_feature() {
        let error = refusal("    subscriber_type: objectstorage\n    subtype: awsobject\n");
        assert!(
            error.contains(
                "objectstorage/awsobject is not compiled in; rebuild with the cargo feature `aws`"
            ),
            "{error}"
        );
    }

    #[cfg(feature = "goat")]
    #[test]
    fn the_goat_sink_refuses_cbor() {
        let error = refusal("    subscriber_type: goat\n    encoding: cbor\n");
        assert_eq!(
            error,
            "subscriber Example: `encoding`: \"cbor\" cannot be used here; this sink writes \
             JSON only: the schema declared for each record on a goat fabric is a JSON Schema"
        );
    }

    #[cfg(not(feature = "goat"))]
    #[test]
    fn goat_needs_its_feature() {
        let error = refusal("    subscriber_type: goat\n");
        assert!(
            error.contains("sink goat is not compiled in; rebuild with the cargo feature `goat`"),
            "{error}"
        );
    }

    #[cfg(not(feature = "zenoh"))]
    #[test]
    fn zenoh_needs_its_feature() {
        let error = refusal("    subscriber_type: zenoh\n");
        assert!(
            error.contains("sink zenoh is not compiled in; rebuild with the cargo feature `zenoh`"),
            "{error}"
        );
    }
}
