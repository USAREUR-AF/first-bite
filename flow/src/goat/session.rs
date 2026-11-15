//! The Zenoh session a goat router accepts, and two ways of reading the
//! fabric back: a delivery probe and a watch.
//!
//! The client configuration is the one goat's own publishers use
//! (`goatstuff/ops/zenoh-shims/rest-pub-bridge`): client mode, a `tls/`
//! endpoint, and the whole TLS block set at once with `enable_mtls`.

use super::config::GoatConfig;
use super::keys::KeyLayout;
use super::GoatError;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::runtime::Builder;
use zenoh::bytes::Encoding;

/// How long the probe waits for its sample.
pub const PROBE_WAIT: Duration = Duration::from_secs(5);

/// Pause between the probe's puts: a subscription takes a moment to reach
/// the router, and a put before then goes nowhere.
const PROBE_INTERVAL: Duration = Duration::from_millis(250);

fn zenoh_error(error: zenoh::Error) -> GoatError {
    GoatError::Zenoh(error.to_string())
}

/// The Zenoh client configuration for this identity. With `pin` and a slot
/// identity the session identifier is the slot UUID; a reading session never
/// pins, because a router keeps one transport per identifier.
pub fn client_config(config: &GoatConfig, pin: bool) -> Result<Value, GoatError> {
    config.check_shape()?;
    let identity = &config.identity;
    let mut client = json!({
        "mode": "client",
        "connect": { "endpoints": config.goatnet.endpoints },
        "scouting": { "multicast": { "enabled": false } },
        "transport": { "link": { "tls": {
            "root_ca_certificate": identity.ca_certificate,
            "connect_certificate": identity.certificate,
            "connect_private_key": identity.private_key,
            "enable_mtls": true,
            "verify_name_on_connect": config.goatnet.verify_name_on_connect,
        } } },
    });
    if pin && identity.pin_session_id {
        if let Some(id) = config.principal()?.session_id() {
            client["id"] = json!(id);
        }
    }
    Ok(client)
}

/// The configuration as text. JSON is JSON5, so the output can be saved and
/// given to any Zenoh 1.9 tool.
pub fn client_config_text(config: &GoatConfig, pin: bool) -> Result<String, GoatError> {
    let client = client_config(config, pin)?;
    Ok(serde_json::to_string_pretty(&client).expect("a JSON value serialises"))
}

/// Open a session with the generated configuration.
pub(crate) async fn open(config: &GoatConfig, pin: bool) -> Result<zenoh::Session, GoatError> {
    let text = client_config_text(config, pin)?;
    let client = zenoh::Config::from_json5(&text).map_err(zenoh_error)?;
    zenoh::open(client).await.map_err(zenoh_error)
}

/// What the delivery probe found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// The router carried a sample put on `key` to a second session.
    Delivered { key: String, after: Duration },
    /// The session opened and the put returned success, but nothing arrived.
    NotDelivered { key: String, waited: Duration },
}

/// Put `payload` on `key` from `writer` until it arrives at a second session
/// opened with the same certificate, or `PROBE_WAIT` passes.
///
/// A goat router drops a write its access rules do not grant and reports
/// nothing, so reading the sample back is the only way a publisher can tell.
pub(crate) async fn probe_with(
    config: &GoatConfig,
    writer: &zenoh::Session,
    key: &str,
    payload: &str,
) -> Result<Delivery, GoatError> {
    let reader = open(config, false).await?;
    let subscriber = reader.declare_subscriber(key).await.map_err(zenoh_error)?;
    let started = Instant::now();
    let mut delivered = None;
    while started.elapsed() < PROBE_WAIT {
        writer
            .put(key, payload)
            .encoding(Encoding::APPLICATION_JSON)
            .await
            .map_err(zenoh_error)?;
        if let Ok(Ok(_)) = tokio::time::timeout(PROBE_INTERVAL, subscriber.recv_async()).await {
            delivered = Some(started.elapsed());
            break;
        }
    }
    drop(subscriber);
    let _ = reader.close().await;
    Ok(match delivered {
        Some(after) => Delivery::Delivered {
            key: key.to_string(),
            after,
        },
        None => Delivery::NotDelivered {
            key: key.to_string(),
            waited: PROBE_WAIT,
        },
    })
}

/// What to tell the operator when the probe's sample did not arrive.
pub fn not_delivered_advice(config: &GoatConfig, layout: &KeyLayout, key: &str) -> String {
    let common_name = config
        .principal()
        .map(|principal| principal.common_name().to_string())
        .unwrap_or_default();
    format!(
        "the router accepted the session but did not deliver a sample put on {key}. A goat \
         router drops writes its access rules do not grant and reports nothing. Ask the \
         goatnet's administrator to confirm that the certificate common name {common_name} is \
         granted put on {} and {}",
        layout.own_pattern(),
        "attest/schema/**"
    )
}

fn runtime() -> Result<tokio::runtime::Runtime, GoatError> {
    Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(GoatError::Runtime)
}

/// Open two sessions and run the delivery probe on the health key. The
/// writing session is not pinned, so the probe can run beside the service.
pub fn probe(config: &GoatConfig) -> Result<Delivery, GoatError> {
    let layout = KeyLayout::new(config)?;
    runtime()?.block_on(async {
        let writer = open(config, false).await?;
        let key = layout.health_key();
        let payload = json!({
            "status": "probe",
            "format": config.publish.format,
            "instance": config.publish.instance,
            "site": config.publish.site,
            "time": chrono::Utc::now().to_rfc3339(),
        })
        .to_string();
        let outcome = probe_with(config, &writer, &key, &payload).await;
        let _ = writer.close().await;
        outcome
    })
}

/// A key expression made only of wildcards names the whole keyspace, which a
/// goat router does not grant.
pub fn check_key_expression(key_expression: &str, example: &str) -> Result<(), GoatError> {
    let literal = key_expression
        .split('/')
        .any(|chunk| !chunk.is_empty() && !chunk.contains(['*', '$']));
    if literal {
        Ok(())
    } else {
        Err(GoatError::WholeKeyspace(
            key_expression.to_string(),
            example.to_string(),
        ))
    }
}

/// One sample read from the fabric.
#[derive(Debug, Clone)]
pub struct Seen {
    pub key: String,
    pub encoding: String,
    pub payload: Vec<u8>,
}

/// How [`watch`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    /// Subscribe and report samples as they are published.
    Live,
    /// Ask for what the router's storage holds.
    Stored,
}

/// Read samples matching `key_expression` for up to `wait`, or until `limit`
/// have been seen, calling `seen` for each. Returns the count.
pub fn watch(
    config: &GoatConfig,
    key_expression: &str,
    read: Read,
    wait: Duration,
    limit: Option<usize>,
    mut seen: impl FnMut(&Seen),
) -> Result<usize, GoatError> {
    let layout = KeyLayout::new(config)?;
    check_key_expression(key_expression, &layout.own_pattern())?;
    let enough = |count: usize| limit.is_some_and(|limit| count >= limit);
    runtime()?.block_on(async {
        let session = open(config, false).await?;
        let deadline = tokio::time::Instant::now() + wait;
        let mut count = 0;
        match read {
            Read::Live => {
                let subscriber = session
                    .declare_subscriber(key_expression)
                    .await
                    .map_err(zenoh_error)?;
                while !enough(count) {
                    match tokio::time::timeout_at(deadline, subscriber.recv_async()).await {
                        Ok(Ok(sample)) => {
                            count += 1;
                            seen(&Seen {
                                key: sample.key_expr().as_str().to_string(),
                                encoding: sample.encoding().to_string(),
                                payload: sample.payload().to_bytes().to_vec(),
                            });
                        }
                        _ => break,
                    }
                }
            }
            Read::Stored => {
                let replies = session
                    .get(key_expression)
                    .timeout(wait)
                    .await
                    .map_err(zenoh_error)?;
                while !enough(count) {
                    match tokio::time::timeout_at(deadline, replies.recv_async()).await {
                        Ok(Ok(reply)) => {
                            if let Ok(sample) = reply.result() {
                                count += 1;
                                seen(&Seen {
                                    key: sample.key_expr().as_str().to_string(),
                                    encoding: sample.encoding().to_string(),
                                    payload: sample.payload().to_bytes().to_vec(),
                                });
                            }
                        }
                        _ => break,
                    }
                }
            }
        }
        let _ = session.close().await;
        Ok(count)
    })
}

#[cfg(test)]
mod tests {
    use super::super::config::tests::{slot_config, SLOT};
    use super::*;
    use std::path::Path;

    #[test]
    fn the_client_configuration_is_goats_mutual_tls_idiom() {
        let config = slot_config(Path::new("/etc/first-bite/goat"));
        let client = client_config(&config, true).unwrap();
        assert_eq!(
            client,
            json!({
                "mode": "client",
                "id": SLOT,
                "connect": { "endpoints": ["tls/zenoh.goat.example:7447"] },
                "scouting": { "multicast": { "enabled": false } },
                "transport": { "link": { "tls": {
                    "root_ca_certificate": "/etc/first-bite/goat/ca-root.pem",
                    "connect_certificate": format!("/etc/first-bite/goat/{SLOT}-cert.pem"),
                    "connect_private_key": format!("/etc/first-bite/goat/{SLOT}-key.pem"),
                    "enable_mtls": true,
                    "verify_name_on_connect": true,
                } } },
            })
        );
    }

    #[test]
    fn zenoh_accepts_the_generated_text() {
        let config = slot_config(Path::new("/etc/first-bite/goat"));
        for pin in [true, false] {
            let text = client_config_text(&config, pin).unwrap();
            zenoh::Config::from_json5(&text).unwrap();
        }
    }

    #[test]
    fn the_session_identifier_is_pinned_only_for_a_writing_slot() {
        let mut config = slot_config(Path::new("/x"));
        assert!(client_config(&config, false).unwrap().get("id").is_none());
        config.identity.pin_session_id = false;
        assert!(client_config(&config, true).unwrap().get("id").is_none());
        config.identity.pin_session_id = true;
        config.identity.slot = None;
        config.identity.org = Some("example-vendor".to_string());
        assert!(client_config(&config, true).unwrap().get("id").is_none());
    }

    #[test]
    fn a_configuration_that_fails_its_checks_gives_no_client_configuration() {
        let mut config = slot_config(Path::new("/x"));
        config.goatnet.endpoints = vec!["tcp/127.0.0.1:7447".to_string()];
        assert!(matches!(
            client_config(&config, true),
            Err(GoatError::Endpoint(_))
        ));
    }

    #[test]
    fn expressions_that_name_the_whole_keyspace_are_refused() {
        for whole in ["**", "*/**", "*/*/**", "*", "$*/**"] {
            let error = check_key_expression(whole, "abc/**")
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("names the whole keyspace"),
                "{whole}: {error}"
            );
        }
        for filter in [
            "abc/**",
            "**/telemetry/**",
            "attest/schema/**",
            "*/health/**",
        ] {
            assert!(check_key_expression(filter, "abc/**").is_ok(), "{filter}");
        }
    }
}
