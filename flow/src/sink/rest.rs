//! POST each record as JSON to an HTTP endpoint.

use super::{quoted, read_body, required, Sink, SinkError};
use crate::config::SubscriberConfig;
use crate::encoding::{json_to_cbor, Encoding};
use crate::error::Problem;
use async_trait::async_trait;
use log::{debug, warn};
use std::time::Duration;

/// Attempts after the first for a server error or a failed connection.
const MAX_RETRIES: u32 = 3;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Wait before the first retry; doubled for each one after.
const FIRST_RETRY_WAIT: Duration = Duration::from_secs(2);

pub struct RestSink {
    client: reqwest::Client,
    url: reqwest::Url,
    /// The endpoint as log lines name it: no user, password, query or
    /// fragment, any of which can hold a credential.
    shown: String,
    encoding: Encoding,
    first_retry_wait: Duration,
}

impl RestSink {
    /// The endpoint is `endpoint_url:endpoint_port` followed by
    /// `endpoint_path`. A URL without a scheme is taken as `http://`.
    pub fn from_config(config: &SubscriberConfig) -> Result<Self, Problem> {
        let host = required(&config.endpoint_url, "endpoint_url")?;
        let port = required(&config.endpoint_port, "endpoint_port")?;
        let path = config.endpoint_path.as_deref().unwrap_or("");
        let scheme = if host.starts_with("http://") || host.starts_with("https://") {
            ""
        } else {
            "http://"
        };
        // Records go to the endpoint that was configured and nowhere else:
        // a redirect is an error, not an instruction to post them again.
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| Problem::InvalidValue {
                key: "endpoint_url",
                reason: format!("cannot build the HTTP client: {e}"),
            })?;
        // The parser's error does not quote the text, which may hold a
        // password.
        let url = reqwest::Url::parse(&format!("{scheme}{host}:{port}{path}")).map_err(|e| {
            Problem::InvalidValue {
                key: "endpoint_url",
                reason: format!(
                    "endpoint_url, endpoint_port and endpoint_path do not make a URL: {e}"
                ),
            }
        })?;
        Ok(RestSink {
            client,
            shown: shown(&url),
            url,
            encoding: Encoding::from_config(config.encoding.as_deref())?,
            first_retry_wait: FIRST_RETRY_WAIT,
        })
    }

    async fn post(&self, body: &[u8]) -> Result<(), SinkError> {
        let response = self
            .client
            .post(self.url.clone())
            .header("Content-Type", self.encoding.media_type())
            .body(body.to_vec())
            .send()
            .await?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let body = quoted(&read_body(response).await);
        Err(SinkError::HttpStatus { status, body })
    }
}

/// `url` without user, password, query and fragment.
fn shown(url: &reqwest::Url) -> String {
    let mut shown = url.clone();
    let _ = shown.set_username("");
    let _ = shown.set_password(None);
    shown.set_query(None);
    shown.set_fragment(None);
    shown.to_string()
}

/// Whether sending again could succeed: the request never got an answer, or
/// the server said the fault was its own.
fn worth_retrying(error: &SinkError) -> bool {
    match error {
        SinkError::Http(_) => true,
        SinkError::HttpStatus { status, .. } => status.is_server_error(),
        _ => false,
    }
}

#[async_trait]
impl Sink for RestSink {
    async fn send(&self, _topic: &str, record: &str) -> Result<(), SinkError> {
        // Parsing refuses a record that is not JSON before anything is sent,
        // and the endpoint receives it in compact form.
        let body = match self.encoding {
            Encoding::Json => serde_json::from_str::<serde_json::Value>(record)?
                .to_string()
                .into_bytes(),
            Encoding::Cbor => json_to_cbor(record)?,
        };
        let mut wait = self.first_retry_wait;
        let mut retries = 0;
        loop {
            match self.post(&body).await {
                Ok(()) => {
                    debug!("posted a record to {}", self.shown);
                    return Ok(());
                }
                Err(e) if retries < MAX_RETRIES && worth_retrying(&e) => {
                    retries += 1;
                    warn!(
                        "{}: {e}; retry {retries} of {MAX_RETRIES} in {wait:?}",
                        self.shown
                    );
                    tokio::time::sleep(wait).await;
                    wait *= 2;
                }
                Err(e) => return Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{subscriber, HttpStub};

    fn sink(port: u16) -> RestSink {
        let mut sink = RestSink::from_config(&subscriber(&format!(
            "    subscriber_type: rest\n    subtype: echo\n    endpoint_url: \"127.0.0.1\"\n    endpoint_port: {port}\n    endpoint_path: /records\n"
        )))
        .unwrap();
        sink.first_retry_wait = Duration::from_millis(5);
        sink
    }

    #[test]
    fn url_is_built_from_host_port_and_path() {
        assert_eq!(sink(5001).url.as_str(), "http://127.0.0.1:5001/records");
        let https = RestSink::from_config(&subscriber(
            "    subscriber_type: rest\n    endpoint_url: \"https://collector.example\"\n    endpoint_port: 443\n",
        ))
        .unwrap();
        // The URL type writes the root path and leaves out a default port.
        assert_eq!(https.url.as_str(), "https://collector.example/");
    }

    #[tokio::test]
    async fn record_is_posted_as_compact_json() {
        let stub = HttpStub::answering(&[200]);
        sink(stub.port())
            .send("WX/Heartbeat", r#" { "station_id" : "KX7" } "#)
            .await
            .unwrap();
        let requests = stub.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/records");
        assert_eq!(requests[0].header("content-type"), Some("application/json"));
        assert_eq!(requests[0].body, br#"{"station_id":"KX7"}"#);
    }

    #[tokio::test]
    async fn a_cbor_sink_posts_the_record_as_application_cbor() {
        let stub = HttpStub::answering(&[200]);
        let mut sink = RestSink::from_config(&subscriber(&format!(
            "    subscriber_type: rest\n    endpoint_url: \"127.0.0.1\"\n    endpoint_port: {}\n    endpoint_path: /records\n    encoding: cbor\n",
            stub.port()
        )))
        .unwrap();
        sink.first_retry_wait = Duration::from_millis(5);
        sink.send("WX/Heartbeat", r#"{"station_id":"KX7","uptime":86400}"#)
            .await
            .unwrap();
        let requests = stub.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].header("content-type"), Some("application/cbor"));
        // {"uptime": 86400, "station_id": "KX7"}: shorter key first.
        assert_eq!(
            requests[0].body,
            [
                &[0xa2, 0x66][..],
                b"uptime",
                &[0x1a, 0x00, 0x01, 0x51, 0x80, 0x6a],
                b"station_id",
                &[0x63],
                b"KX7",
            ]
            .concat()
        );
    }

    #[tokio::test]
    async fn server_errors_are_retried() {
        let stub = HttpStub::answering(&[503, 500, 200]);
        sink(stub.port()).send("t", "{}").await.unwrap();
        assert_eq!(stub.requests().len(), 3);
    }

    #[tokio::test]
    async fn client_errors_are_reported_with_their_status() {
        let stub = HttpStub::answering(&[422]);
        let error = sink(stub.port()).send("t", "{}").await.unwrap_err();
        assert!(
            matches!(error, SinkError::HttpStatus { status, .. } if status.as_u16() == 422),
            "{error}"
        );
        assert_eq!(stub.requests().len(), 1);
    }

    #[tokio::test]
    async fn retries_stop_after_the_limit() {
        let stub = HttpStub::answering(&[500, 500, 500, 500]);
        let error = sink(stub.port()).send("t", "{}").await.unwrap_err();
        assert!(matches!(error, SinkError::HttpStatus { .. }), "{error}");
        assert_eq!(stub.requests().len(), 4);
    }

    #[test]
    fn credentials_in_the_url_stay_out_of_log_lines_and_errors() {
        let sink = RestSink::from_config(&subscriber(
            "    subscriber_type: rest\n    endpoint_url: \"https://ingest:hunter2@collector.example.org\"\n    endpoint_port: 8443\n    endpoint_path: \"/records?token=hunter3#hunter4\"\n",
        ))
        .unwrap();
        assert_eq!(sink.shown, "https://collector.example.org:8443/records");
        assert_eq!(
            sink.url.as_str(),
            "https://ingest:hunter2@collector.example.org:8443/records?token=hunter3#hunter4"
        );

        let refused = RestSink::from_config(&subscriber(
            "    subscriber_type: rest\n    endpoint_url: \"https://ingest:hunter2@collector.example.org\"\n    endpoint_port: notaport\n",
        ));
        let reason = match refused {
            Ok(_) => panic!("built a sink for a URL that does not parse"),
            Err(problem) => problem.to_string(),
        };
        assert!(reason.starts_with("`endpoint_url`: "), "{reason}");
        assert!(!reason.contains("hunter2"), "{reason}");
    }

    #[tokio::test]
    async fn a_failed_connection_is_reported_without_the_url() {
        let port = crate::test_support::free_port();
        let mut sink = RestSink::from_config(&subscriber(&format!(
            "    subscriber_type: rest\n    endpoint_url: \"http://ingest:hunter2@127.0.0.1\"\n    endpoint_port: {port}\n    endpoint_path: \"/records?token=hunter3\"\n"
        )))
        .unwrap();
        sink.first_retry_wait = Duration::from_millis(1);
        let error = sink.send("t", "{}").await.unwrap_err().to_string();
        assert!(error.starts_with("HTTP request failed: "), "{error}");
        assert!(!error.contains("hunter"), "{error}");
    }

    #[tokio::test]
    async fn a_redirect_is_an_error_and_is_not_followed() {
        let elsewhere = HttpStub::answering(&[200]);
        let stub = HttpStub::redirecting(
            &[307],
            &format!("http://127.0.0.1:{}/elsewhere", elsewhere.port()),
        );
        let error = sink(stub.port()).send("t", "{}").await.unwrap_err();
        assert!(
            matches!(error, SinkError::HttpStatus { status, .. } if status.as_u16() == 307),
            "{error}"
        );
        assert_eq!(stub.requests().len(), 1);
        assert!(
            elsewhere.requests().is_empty(),
            "the record was posted to the redirect target"
        );
    }

    #[tokio::test]
    async fn a_long_error_answer_is_read_and_quoted_in_part() {
        let stub = HttpStub::answering_with_body(422, vec![b'x'; 3 * 1024 * 1024]);
        let error = sink(stub.port()).send("t", "{}").await.unwrap_err();
        let text = error.to_string();
        assert!(text.len() < 1024, "{} characters", text.len());
        assert!(text.ends_with("(1048576 bytes read)"), "{text}");
    }

    #[tokio::test]
    async fn a_record_that_is_not_json_is_not_sent() {
        let stub = HttpStub::answering(&[]);
        let error = sink(stub.port()).send("t", "not json").await.unwrap_err();
        assert!(matches!(error, SinkError::Record(_)), "{error}");
    }
}
