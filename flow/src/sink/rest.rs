//! POST each record as JSON to an HTTP endpoint.

use super::{required, Sink, SinkError};
use crate::config::SubscriberConfig;
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
    url: String,
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
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| Problem::InvalidValue {
                key: "endpoint_url",
                reason: format!("cannot build the HTTP client: {e}"),
            })?;
        Ok(RestSink {
            client,
            url: format!("{scheme}{host}:{port}{path}"),
            first_retry_wait: FIRST_RETRY_WAIT,
        })
    }

    async fn post(&self, body: &str) -> Result<(), SinkError> {
        let response = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .await?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let body = response.text().await.unwrap_or_default();
        Err(SinkError::HttpStatus { status, body })
    }
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
        let body = serde_json::from_str::<serde_json::Value>(record)?.to_string();
        let mut wait = self.first_retry_wait;
        let mut retries = 0;
        loop {
            match self.post(&body).await {
                Ok(()) => {
                    debug!("posted a record to {}", self.url);
                    return Ok(());
                }
                Err(e) if retries < MAX_RETRIES && worth_retrying(&e) => {
                    retries += 1;
                    warn!(
                        "{}: {e}; retry {retries} of {MAX_RETRIES} in {wait:?}",
                        self.url
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
        assert_eq!(sink(5001).url, "http://127.0.0.1:5001/records");
        let https = RestSink::from_config(&subscriber(
            "    subscriber_type: rest\n    endpoint_url: \"https://collector.example\"\n    endpoint_port: 443\n",
        ))
        .unwrap();
        assert_eq!(https.url, "https://collector.example:443");
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

    #[tokio::test]
    async fn a_record_that_is_not_json_is_not_sent() {
        let stub = HttpStub::answering(&[]);
        let error = sink(stub.port()).send("t", "not json").await.unwrap_err();
        assert!(matches!(error, SinkError::Record(_)), "{error}");
    }
}
