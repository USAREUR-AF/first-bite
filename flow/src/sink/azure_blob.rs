//! Store each record as a block blob in an Azure Blob Storage container.
//!
//! The sink speaks the Blob service REST API directly: `Put Blob` and
//! `Create Container`, authorised with the account's Shared Key.

use super::object_path::{time_then_topic, topic_then_time};
use super::{required, Sink, SinkError};
use crate::config::SubscriberConfig;
use crate::error::Problem;
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use log::{debug, info};
use reqwest::{Method, StatusCode, Url};
use sha2::Sha256;
use std::time::Duration;

/// Storage service version sent as `x-ms-version` when `cloud_api_version`
/// is not set. The request signature follows the rules of 2015-02-21 and
/// later.
pub const DEFAULT_API_VERSION: &str = "2023-11-03";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub struct AzureBlobSink {
    client: reqwest::Client,
    account: String,
    key: Vec<u8>,
    /// URL of the container.
    container_url: Url,
    container: String,
    api_version: String,
}

impl AzureBlobSink {
    pub fn from_config(config: &SubscriberConfig) -> Result<Self, Problem> {
        let account = required(&config.cloud_account, "cloud_account")?;
        let access_key = required(&config.cloud_access_key, "cloud_access_key")?;
        let container = required(&config.cloud_container, "cloud_container")?;
        let key = STANDARD
            .decode(access_key.trim())
            .map_err(|e| Problem::InvalidValue {
                key: "cloud_access_key",
                reason: format!("not base64: {e}"),
            })?;
        let (location_key, service_url) = service_url(config, account)?;
        let invalid_url = |reason: String| Problem::InvalidValue {
            key: location_key,
            reason,
        };
        let mut container_url = Url::parse(&service_url)
            .map_err(|e| invalid_url(format!("{service_url:?} is not a URL: {e}")))?;
        container_url
            .path_segments_mut()
            .map_err(|()| invalid_url(format!("{service_url:?} cannot carry a path")))?
            .pop_if_empty()
            .push(container);
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| invalid_url(format!("cannot build the HTTP client: {e}")))?;
        Ok(AzureBlobSink {
            client,
            account: account.to_string(),
            key,
            container_url,
            container: container.to_string(),
            api_version: config
                .cloud_api_version
                .as_deref()
                .unwrap_or(DEFAULT_API_VERSION)
                .to_string(),
        })
    }

    /// URL of a blob in the container; `/` in the name separates path
    /// segments and everything else is percent-encoded.
    fn blob_url(&self, name: &str) -> Url {
        let mut url = self.container_url.clone();
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.extend(name.split('/'));
        }
        url
    }

    /// Send one signed request and return the status and body of the answer.
    async fn request(
        &self,
        method: Method,
        url: Url,
        blob_type: Option<&str>,
        body: Option<&[u8]>,
    ) -> Result<(StatusCode, Vec<u8>), SinkError> {
        let mut headers = vec![
            ("x-ms-date".to_string(), http_date(Utc::now())),
            ("x-ms-version".to_string(), self.api_version.clone()),
        ];
        if let Some(blob_type) = blob_type {
            headers.push(("x-ms-blob-type".to_string(), blob_type.to_string()));
        }
        let content_type = body.map(|_| "application/json");
        let content_length = body.map_or(0, <[u8]>::len);
        let to_sign = string_to_sign(
            method.as_str(),
            content_length,
            content_type.unwrap_or(""),
            &mut headers,
            &self.account,
            &url,
        );
        let authorization = format!("SharedKey {}:{}", self.account, sign(&self.key, &to_sign));

        let mut request = self
            .client
            .request(method, url)
            .header("Authorization", authorization)
            .header("Content-Length", content_length);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        if let Some(content_type) = content_type {
            request = request.header("Content-Type", content_type);
        }
        let response = request
            .body(body.unwrap_or_default().to_vec())
            .send()
            .await?;
        let status = response.status();
        Ok((status, response.bytes().await?.to_vec()))
    }
}

/// The configuration key that locates the service, and the service URL:
/// `Emulator` is `http://<cloud_address>:<cloud_port>/<account>`, `Public`
/// is `https://<account>.blob.core.windows.net`, `Custom` is `cloud_uri`.
fn service_url(
    config: &SubscriberConfig,
    account: &str,
) -> Result<(&'static str, String), Problem> {
    match required(&config.cloud_location_type, "cloud_location_type")? {
        "Emulator" => {
            let address = required(&config.cloud_address, "cloud_address")?;
            let port = required(&config.cloud_port, "cloud_port")?;
            let port: u16 = port.parse().map_err(|_| Problem::InvalidValue {
                key: "cloud_port",
                reason: format!("{port:?} is not a port number"),
            })?;
            Ok((
                "cloud_address",
                format!("http://{address}:{port}/{account}"),
            ))
        }
        "Custom" => Ok((
            "cloud_uri",
            required(&config.cloud_uri, "cloud_uri")?.to_string(),
        )),
        "Public" => Ok((
            "cloud_account",
            format!("https://{account}.blob.core.windows.net"),
        )),
        other => Err(Problem::InvalidValue {
            key: "cloud_location_type",
            reason: format!("{other:?} is not one of Emulator, Custom, Public"),
        }),
    }
}

/// A time in the form HTTP headers carry: `Wed, 04 Mar 2026 12:00:00 GMT`.
fn http_date(time: DateTime<Utc>) -> String {
    time.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// The text a Shared Key signature covers, for service versions 2015-02-21
/// and later: the verb, eleven standard headers (of which this sink sends
/// only Content-Length and Content-Type), the `x-ms-` headers sorted by name,
/// and the resource with its query parameters sorted by name.
///
/// `x_ms_headers` holds lower-case names and is sorted in place.
fn string_to_sign(
    method: &str,
    content_length: usize,
    content_type: &str,
    x_ms_headers: &mut [(String, String)],
    account: &str,
    url: &Url,
) -> String {
    // A zero length is signed as an empty string.
    let content_length = match content_length {
        0 => String::new(),
        length => length.to_string(),
    };
    // Content-Encoding, Content-Language, Content-Length, Content-MD5,
    // Content-Type, Date, If-Modified-Since, If-Match, If-None-Match,
    // If-Unmodified-Since, Range.
    let mut text = format!("{method}\n\n\n{content_length}\n\n{content_type}\n\n\n\n\n\n\n");
    x_ms_headers.sort();
    for (name, value) in x_ms_headers.iter() {
        text.push_str(&format!("{name}:{}\n", value.trim()));
    }
    text.push('/');
    text.push_str(account);
    text.push_str(url.path());
    let mut query: Vec<(String, String)> = url
        .query_pairs()
        .map(|(name, value)| (name.to_lowercase(), value.into_owned()))
        .collect();
    query.sort();
    for (name, value) in query {
        text.push_str(&format!("\n{name}:{value}"));
    }
    text
}

/// Base64 of the HMAC-SHA256 of `text` under the account key.
fn sign(key: &[u8], text: &str) -> String {
    // HMAC accepts a key of any length, so this never takes the error arm.
    match Hmac::<Sha256>::new_from_slice(key) {
        Ok(mut mac) => {
            mac.update(text.as_bytes());
            STANDARD.encode(mac.finalize().into_bytes())
        }
        Err(_) => String::new(),
    }
}

fn status_error(status: StatusCode, body: &[u8]) -> SinkError {
    SinkError::HttpStatus {
        status,
        body: String::from_utf8_lossy(body).into_owned(),
    }
}

#[async_trait]
impl Sink for AzureBlobSink {
    /// Create the container; one that already exists is left as it is.
    async fn prepare(&self) -> Result<(), SinkError> {
        let mut url = self.container_url.clone();
        url.set_query(Some("restype=container"));
        let (status, body) = self.request(Method::PUT, url, None, None).await?;
        match status {
            StatusCode::CREATED => info!("created container {}", self.container),
            StatusCode::CONFLICT => info!("container {} exists", self.container),
            _ => return Err(status_error(status, &body)),
        }
        Ok(())
    }

    async fn send(&self, topic: &str, record: &str) -> Result<(), SinkError> {
        let now = Utc::now();
        for blob in [
            time_then_topic(topic, record, now),
            topic_then_time(topic, record, now),
        ] {
            let (status, body) = self
                .request(
                    Method::PUT,
                    self.blob_url(&blob),
                    Some("BlockBlob"),
                    Some(record.as_bytes()),
                )
                .await?;
            if status != StatusCode::CREATED {
                return Err(status_error(status, &body));
            }
            debug!("stored {blob} in container {}", self.container);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{subscriber, HttpStub};
    use chrono::TimeZone;

    /// The storage emulator's published development account key.
    const EMULATOR_KEY: &str =
        "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";

    fn account() -> String {
        format!(
            "    subscriber_type: objectstorage\n    subtype: azureobject\n    cloud_account: devstoreaccount1\n    cloud_access_key: \"{EMULATOR_KEY}\"\n    cloud_container: records\n"
        )
    }

    fn emulator(port: u16) -> AzureBlobSink {
        AzureBlobSink::from_config(&subscriber(&format!(
            "{}    cloud_location_type: Emulator\n    cloud_address: \"127.0.0.1\"\n    cloud_port: {port}\n    cloud_api_version: \"2023-01-03\"\n",
            account()
        )))
        .unwrap()
    }

    fn built(body: &str) -> Result<AzureBlobSink, String> {
        AzureBlobSink::from_config(&subscriber(&format!("{}{body}", account())))
            .map_err(|problem| problem.to_string())
    }

    fn problem(body: &str) -> String {
        match built(body) {
            Ok(_) => panic!("{body} built a sink"),
            Err(problem) => problem,
        }
    }

    #[test]
    fn each_location_type_needs_its_own_keys() {
        assert_eq!(problem(""), "`cloud_location_type` is required");
        assert_eq!(
            problem("    cloud_location_type: Emulator\n    cloud_address: \"127.0.0.1\"\n"),
            "`cloud_port` is required"
        );
        assert_eq!(
            problem("    cloud_location_type: Custom\n"),
            "`cloud_uri` is required"
        );
        assert!(problem("    cloud_location_type: Moon\n").starts_with("`cloud_location_type`: "));
        assert!(
            problem("    cloud_location_type: Custom\n    cloud_uri: \"not a url\"\n")
                .starts_with("`cloud_uri`: ")
        );
    }

    #[test]
    fn each_location_type_has_its_container_url() {
        assert_eq!(
            emulator(10000).container_url.as_str(),
            "http://127.0.0.1:10000/devstoreaccount1/records"
        );
        let public = built("    cloud_location_type: Public\n").unwrap();
        assert_eq!(
            public.container_url.as_str(),
            "https://devstoreaccount1.blob.core.windows.net/records"
        );
        assert_eq!(public.api_version, DEFAULT_API_VERSION);
        for uri in ["https://127.0.0.1:10000", "https://127.0.0.1:10000/"] {
            let custom = built(&format!(
                "    cloud_location_type: Custom\n    cloud_uri: {uri}\n"
            ))
            .unwrap();
            assert_eq!(
                custom.container_url.as_str(),
                "https://127.0.0.1:10000/records"
            );
        }
    }

    #[test]
    fn an_access_key_that_is_not_base64_is_refused() {
        let error = AzureBlobSink::from_config(&subscriber(
            "    subscriber_type: objectstorage\n    subtype: azureobject\n    cloud_account: a\n    cloud_access_key: \"not base64!\"\n    cloud_container: c\n    cloud_location_type: Public\n",
        ))
        .err()
        .unwrap()
        .to_string();
        assert!(
            error.starts_with("`cloud_access_key`: not base64"),
            "{error}"
        );
    }

    #[test]
    fn blob_names_are_percent_encoded_by_segment() {
        assert_eq!(
            emulator(10000)
                .blob_url("all-topic-time/WX/Wind Gust/a.json")
                .as_str(),
            "http://127.0.0.1:10000/devstoreaccount1/records/all-topic-time/WX/Wind%20Gust/a.json"
        );
    }

    /// The worked example in Microsoft's "Authorize with Shared Key": a
    /// Create Container request with a timeout.
    #[test]
    fn string_to_sign_matches_the_documented_example() {
        let url = Url::parse(
            "https://myaccount.blob.core.windows.net/mycontainer?restype=container&timeout=30",
        )
        .unwrap();
        // Given out of order: the function sorts them.
        let mut headers = vec![
            ("x-ms-version".to_string(), "2015-02-21".to_string()),
            (
                "x-ms-date".to_string(),
                "Fri, 26 Jun 2015 23:39:12 GMT".to_string(),
            ),
        ];
        assert_eq!(
            string_to_sign("PUT", 0, "", &mut headers, "myaccount", &url),
            "PUT\n\n\n\n\n\n\n\n\n\n\n\nx-ms-date:Fri, 26 Jun 2015 23:39:12 GMT\nx-ms-version:2015-02-21\n/myaccount/mycontainer\nrestype:container\ntimeout:30"
        );
    }

    /// A Put Blob against the emulator, where the account name is also the
    /// first path segment. The expected signature was computed with Python's
    /// `hmac` and `hashlib` over the same string.
    #[test]
    fn put_blob_signature_matches_an_independent_computation() {
        let url =
            Url::parse("http://127.0.0.1:10000/devstoreaccount1/records/WX/Wind%20Gust/a.json")
                .unwrap();
        let mut headers = vec![
            (
                "x-ms-date".to_string(),
                http_date(Utc.with_ymd_and_hms(2026, 3, 4, 12, 0, 0).unwrap()),
            ),
            ("x-ms-version".to_string(), "2023-11-03".to_string()),
            ("x-ms-blob-type".to_string(), "BlockBlob".to_string()),
        ];
        let text = string_to_sign(
            "PUT",
            20,
            "application/json",
            &mut headers,
            "devstoreaccount1",
            &url,
        );
        assert_eq!(
            text,
            "PUT\n\n\n20\n\napplication/json\n\n\n\n\n\n\nx-ms-blob-type:BlockBlob\nx-ms-date:Wed, 04 Mar 2026 12:00:00 GMT\nx-ms-version:2023-11-03\n/devstoreaccount1/devstoreaccount1/records/WX/Wind%20Gust/a.json"
        );
        let key = STANDARD.decode(EMULATOR_KEY).unwrap();
        assert_eq!(
            sign(&key, &text),
            "kd/GsnOl7NnztXjug7RnarXTFWFeqddTC4KPgBoml3g="
        );
    }

    #[tokio::test]
    async fn record_is_stored_under_both_names_with_the_configured_api_version() {
        let stub = HttpStub::answering(&[201, 201]);
        let record = r#"{"station_id":"KX7"}"#;
        emulator(stub.port())
            .send("WX/Heartbeat", record)
            .await
            .unwrap();
        let requests = stub.requests();
        assert_eq!(requests.len(), 2);
        for (request, prefix) in requests.iter().zip([
            "/devstoreaccount1/records/all-time-topic/",
            "/devstoreaccount1/records/all-topic-time/WX/Heartbeat/",
        ]) {
            assert_eq!(request.method, "PUT");
            assert!(request.path.starts_with(prefix), "{}", request.path);
            assert!(request.path.ends_with(".json"), "{}", request.path);
            assert_eq!(request.header("x-ms-version"), Some("2023-01-03"));
            assert_eq!(request.header("x-ms-blob-type"), Some("BlockBlob"));
            assert_eq!(request.header("content-type"), Some("application/json"));
            assert!(request
                .header("authorization")
                .is_some_and(|value| value.starts_with("SharedKey devstoreaccount1:")));
            assert_eq!(request.body, record.as_bytes());
        }
    }

    #[tokio::test]
    async fn prepare_creates_the_container_and_accepts_one_that_exists() {
        for status in [201, 409] {
            let stub = HttpStub::answering(&[status]);
            emulator(stub.port()).prepare().await.unwrap();
            let requests = stub.requests();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].method, "PUT");
            assert_eq!(
                requests[0].path,
                "/devstoreaccount1/records?restype=container"
            );
            assert_eq!(requests[0].header("content-length"), Some("0"));
        }
    }

    #[tokio::test]
    async fn a_refused_request_is_reported_with_its_status() {
        let stub = HttpStub::answering(&[403]);
        let error = emulator(stub.port()).prepare().await.unwrap_err();
        assert!(
            matches!(error, SinkError::HttpStatus { status, .. } if status == StatusCode::FORBIDDEN),
            "{error}"
        );
    }

    /// Against a running Azurite: create a container, store a record, read
    /// one of its blobs back with a signed GET. Runs when `AZURITE_ENDPOINT`
    /// is set to the emulator's `host:port`, e.g. `127.0.0.1:10000`.
    #[tokio::test]
    async fn round_trip_against_azurite() {
        let Ok(endpoint) = std::env::var("AZURITE_ENDPOINT") else {
            eprintln!("AZURITE_ENDPOINT is not set; skipping");
            return;
        };
        let (address, port) = endpoint.rsplit_once(':').expect("host:port");
        let container = format!("flow-test-{}", std::process::id());
        let sink = AzureBlobSink::from_config(&subscriber(&format!(
            "    subscriber_type: objectstorage\n    subtype: azureobject\n    cloud_account: devstoreaccount1\n    cloud_access_key: \"{EMULATOR_KEY}\"\n    cloud_container: {container}\n    cloud_location_type: Emulator\n    cloud_address: \"{address}\"\n    cloud_port: {port}\n"
        )))
        .unwrap();

        sink.prepare().await.unwrap();
        // A second time, the container exists.
        sink.prepare().await.unwrap();
        println!("azurite: container {container} created, second create accepted");

        // A topic with a space exercises the signature over an encoded path.
        let record =
            r#"{"subsystem":"WX","message_type":"Wind Gust","message":{"station_id":"KX7"}}"#;
        sink.send("WX/Wind Gust", record).await.unwrap();
        println!("azurite: record stored under both names");

        let mut list = sink.container_url.clone();
        list.set_query(Some("restype=container&comp=list"));
        let (status, listing) = sink.request(Method::GET, list, None, None).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        let listing = String::from_utf8(listing).unwrap();
        let names: Vec<&str> = listing
            .split("<Name>")
            .skip(1)
            .filter_map(|rest| rest.split("</Name>").next())
            .collect();
        println!("azurite: container lists {names:?}");
        assert_eq!(names.len(), 2, "{listing}");
        assert!(names[0].starts_with("all-time-topic/"));
        assert!(names[1].starts_with("all-topic-time/WX/Wind Gust/"));

        for name in names {
            let (status, body) = sink
                .request(Method::GET, sink.blob_url(name), None, None)
                .await
                .unwrap();
            assert_eq!(status, StatusCode::OK, "{name}");
            assert_eq!(body, record.as_bytes(), "{name}");
        }
        println!("azurite: both blobs read back with a signed GET and match the record");

        let mut delete = sink.container_url.clone();
        delete.set_query(Some("restype=container"));
        let (status, _) = sink
            .request(Method::DELETE, delete, None, None)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::ACCEPTED);
        println!("azurite: container {container} deleted");
    }
}
