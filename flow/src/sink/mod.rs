//! Where a subscriber's records go.
//!
//! Every destination implements [`Sink`]; [`build_sink`] is the one place
//! that turns a subscriber's configuration into a sink.
//!
//! | `subscriber_type` | `subtype` | Sink | Cargo feature |
//! |---|---|---|---|
//! | `objectstorage` | `azureobject` | Azure Blob Storage | `azure` |
//! | `objectstorage` | `awsobject` | S3 and S3-compatible stores | `aws` |
//! | `rest` | any; not used | HTTP POST | always built |
//! | `local` | unset, `locallog` | append to a local file | always built |
//! | `stateful` | unset, `messagerates` | per-topic counts to a local file | always built |
//! | `zenoh` | unset | Zenoh put | `zenoh` |
//! | `goat` | unset | Zenoh put on a goat data fabric | `goat` |

#[cfg(feature = "aws")]
pub mod aws_s3;
#[cfg(feature = "azure")]
pub mod azure_blob;
pub mod local_log;
pub mod message_rates;
#[cfg(any(feature = "aws", feature = "azure"))]
pub mod object_path;
pub mod rest;
#[cfg(feature = "zenoh")]
pub mod zenoh;

use crate::config::SubscriberConfig;
use crate::error::{ConfigError, Problem};
use crate::secret::{reference, Reference, Secret};
use async_trait::async_trait;

/// Why a sink did not take a record, or could not be prepared.
#[derive(Debug, thiserror::Error)]
pub enum SinkError {
    #[error("record is not valid JSON: {0}")]
    Record(#[from] serde_json::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP request failed: {}", causes(.0))]
    Http(reqwest::Error),
    #[error("endpoint answered {status}: {body}")]
    HttpStatus {
        status: reqwest::StatusCode,
        body: String,
    },
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

    /// Deliver one record that arrived on `topic`.
    async fn send(&self, topic: &str, record: &str) -> Result<(), SinkError>;
}

/// Build the sink a subscriber's configuration asks for. An unknown
/// `subscriber_type`/`subtype`, a sink whose cargo feature is off, and a
/// missing or invalid key are errors that name the subscriber.
pub fn build_sink(config: &SubscriberConfig) -> Result<Box<dyn Sink>, ConfigError> {
    select(config).map_err(|problem| ConfigError::Subscriber {
        subscriber: config.name.clone(),
        problem,
    })
}

fn select(config: &SubscriberConfig) -> Result<Box<dyn Sink>, Problem> {
    match (config.subscriber_type.as_str(), config.subtype.as_deref()) {
        ("objectstorage", Some("azureobject")) => azure_blob(config),
        ("objectstorage", Some("awsobject")) => aws_s3(config),
        ("rest", _) => Ok(Box::new(rest::RestSink::from_config(config)?)),
        ("local", None | Some("locallog")) => {
            Ok(Box::new(local_log::LocalLogSink::from_config(config)?))
        }
        ("stateful", None | Some("messagerates")) => Ok(Box::new(
            message_rates::MessageRatesSink::from_config(config),
        )),
        ("zenoh", None) => zenoh(config),
        ("goat", None) => goat(config),
        _ => Err(Problem::UnknownSink {
            subscriber_type: config.subscriber_type.clone(),
            subtype: config.subtype.clone(),
        }),
    }
}

#[cfg(feature = "azure")]
fn azure_blob(config: &SubscriberConfig) -> Result<Box<dyn Sink>, Problem> {
    Ok(Box::new(azure_blob::AzureBlobSink::from_config(config)?))
}

#[cfg(not(feature = "azure"))]
fn azure_blob(_: &SubscriberConfig) -> Result<Box<dyn Sink>, Problem> {
    Err(Problem::FeatureOff {
        sink: "objectstorage/azureobject",
        feature: "azure",
    })
}

#[cfg(feature = "aws")]
fn aws_s3(config: &SubscriberConfig) -> Result<Box<dyn Sink>, Problem> {
    Ok(Box::new(aws_s3::S3Sink::from_config(config)?))
}

#[cfg(not(feature = "aws"))]
fn aws_s3(_: &SubscriberConfig) -> Result<Box<dyn Sink>, Problem> {
    Err(Problem::FeatureOff {
        sink: "objectstorage/awsobject",
        feature: "aws",
    })
}

#[cfg(feature = "zenoh")]
fn zenoh(config: &SubscriberConfig) -> Result<Box<dyn Sink>, Problem> {
    Ok(Box::new(zenoh::ZenohSink::from_config(config)?))
}

#[cfg(not(feature = "zenoh"))]
fn zenoh(_: &SubscriberConfig) -> Result<Box<dyn Sink>, Problem> {
    Err(Problem::FeatureOff {
        sink: "zenoh",
        feature: "zenoh",
    })
}

#[cfg(feature = "goat")]
fn goat(config: &SubscriberConfig) -> Result<Box<dyn Sink>, Problem> {
    Ok(Box::new(crate::goat::sink::GoatSink::from_config(config)?))
}

#[cfg(not(feature = "goat"))]
fn goat(_: &SubscriberConfig) -> Result<Box<dyn Sink>, Problem> {
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
