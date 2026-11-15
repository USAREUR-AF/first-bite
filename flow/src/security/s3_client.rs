//! The HTTP client the AWS SDK sends S3 requests through.
//!
//! The SDK's own client builds a crypto provider for itself, whose protocol
//! versions, key-exchange groups and client certificate cannot be set. This
//! one is the HTTP client every other sink uses, built by
//! [`super::http::secure`], so the S3 sink is held to the security block
//! like the rest.

use super::http::Secured;
use aws_sdk_s3::primitives::ByteStream;
use aws_smithy_runtime_api::client::http::{
    HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings, SharedHttpConnector,
};
use aws_smithy_runtime_api::client::orchestrator::{HttpRequest, HttpResponse};
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_runtime_api::http::StatusCode;
use aws_smithy_types::body::SdkBody;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Timeouts = (Option<Duration>, Option<Duration>);

/// Builds an HTTP client for one pair of connect and read timeouts.
type Build = dyn Fn(Timeouts) -> Result<reqwest::Client, reqwest::Error> + Send + Sync;

/// An [`HttpClient`] over reqwest. The SDK asks for a connector per set of
/// timeouts; one client is kept for each.
pub(crate) struct S3HttpClient {
    build: Box<Build>,
    clients: Mutex<HashMap<Timeouts, reqwest::Client>>,
}

impl S3HttpClient {
    /// `secured` is the sink's checked endpoint and TLS configuration.
    pub(crate) fn new(secured: Secured) -> Self {
        S3HttpClient {
            build: Box::new(move |(connect, read)| {
                // As with the SDK's own client, a redirect is not followed:
                // it would carry the signed request somewhere else.
                let mut builder = secured
                    .client_builder()
                    .redirect(reqwest::redirect::Policy::none());
                if let Some(connect) = connect {
                    builder = builder.connect_timeout(connect);
                }
                if let Some(read) = read {
                    builder = builder.read_timeout(read);
                }
                builder.build()
            }),
            clients: Mutex::new(HashMap::new()),
        }
    }
}

impl fmt::Debug for S3HttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("S3HttpClient")
    }
}

impl HttpClient for S3HttpClient {
    fn http_connector(
        &self,
        settings: &HttpConnectorSettings,
        _components: &RuntimeComponents,
    ) -> SharedHttpConnector {
        let timeouts = (settings.connect_timeout(), settings.read_timeout());
        let mut clients = match self.clients.lock() {
            Ok(clients) => clients,
            Err(poisoned) => poisoned.into_inner(),
        };
        let client = match clients.get(&timeouts) {
            Some(client) => Ok(client.clone()),
            None => (self.build)(timeouts).inspect(|client| {
                clients.insert(timeouts, client.clone());
            }),
        };
        SharedHttpConnector::new(Connector {
            client: client.map_err(|e| Arc::new(e.to_string())),
        })
    }
}

#[derive(Debug)]
struct Connector {
    /// The client, or why it could not be built.
    client: Result<reqwest::Client, Arc<String>>,
}

fn other(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> ConnectorError {
    ConnectorError::other(error.into(), None)
}

fn transport_error(error: reqwest::Error) -> ConnectorError {
    if error.is_timeout() {
        ConnectorError::timeout(error.into())
    } else if error.is_connect() || error.is_request() || error.is_body() {
        ConnectorError::io(error.into())
    } else {
        other(error)
    }
}

impl HttpConnector for Connector {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        let client = self.client.clone();
        HttpConnectorFuture::new(async move {
            let client = client.map_err(|e| other(e.as_str().to_string()))?;
            let method = reqwest::Method::from_bytes(request.method().as_bytes()).map_err(other)?;
            let mut outgoing = client.request(method, request.uri());
            for (name, value) in request.headers() {
                outgoing = outgoing.header(name, value);
            }
            // The sink's bodies are held in memory; a streamed one is read
            // to its end first, so the request always has a length.
            let body = match request.body().bytes() {
                Some(bytes) => bytes.to_vec(),
                None => ByteStream::new(request.into_body())
                    .collect()
                    .await
                    .map_err(other)?
                    .to_vec(),
            };
            let response = outgoing.body(body).send().await.map_err(transport_error)?;
            let status = StatusCode::try_from(response.status().as_u16()).map_err(other)?;
            let headers = response.headers().clone();
            let bytes = response.bytes().await.map_err(transport_error)?;
            let mut incoming = HttpResponse::new(status, SdkBody::from(bytes.to_vec()));
            for (name, value) in &headers {
                if let Ok(value) = value.to_str() {
                    incoming
                        .headers_mut()
                        .append(name.as_str().to_string(), value.to_string());
                }
            }
            Ok(incoming)
        })
    }
}
