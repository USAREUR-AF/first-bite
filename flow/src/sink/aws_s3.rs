//! Store each record as an object in an S3 bucket, on AWS or on any store
//! that speaks the S3 API.

use super::object_path::{time_then_topic, topic_then_time};
use super::{required, required_secret, Built, Sink, SinkError};
use crate::config::SubscriberConfig;
use crate::encoding::Encoding;
use crate::error::Problem;
use crate::secret::scrub;
use crate::security::http::{secure, HttpSink, Roots};
use crate::security::s3_client::S3HttpClient;
use crate::security::{PathPosture, Policy, ProbeTarget, Security};
use async_trait::async_trait;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::error::{DisplayErrorContext, ProvideErrorMetadata, SdkError};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{BucketLocationConstraint, CreateBucketConfiguration};
use aws_sdk_s3::Client;
use chrono::Utc;
use log::{debug, info};

/// The one region whose buckets are created without a location constraint.
const REGION_WITHOUT_CONSTRAINT: &str = "us-east-1";

pub struct S3Sink {
    client: Client,
    bucket: String,
    region: String,
    encoding: Encoding,
    /// The access key id and the secret, kept to take them out of error text.
    credentials: [String; 2],
}

impl S3Sink {
    pub fn from_config(config: &SubscriberConfig) -> Result<Self, Problem> {
        Self::build(config, &Security::unstated().base).map(|(sink, _, _)| sink)
    }

    /// The sink held to `policy`; see [`crate::security`].
    pub(crate) fn secured(config: &SubscriberConfig, policy: &Policy) -> Result<Built, Problem> {
        let (sink, posture, target) = Self::build(config, policy)?;
        Ok(Built::new(sink, posture, target))
    }

    fn build(
        config: &SubscriberConfig,
        policy: &Policy,
    ) -> Result<(Self, PathPosture, Option<ProbeTarget>), Problem> {
        let access_key_id = required_secret(&config.aws_access_key_id, "aws_access_key_id")?;
        let secret_access_key =
            required_secret(&config.aws_secret_access_key, "aws_secret_access_key")?;
        let region = required(&config.aws_default_region, "aws_default_region")?;
        let endpoint = required(&config.aws_s3_endpoint, "aws_s3_endpoint")?;
        let bucket = required(&config.aws_bucket_name, "aws_bucket_name")?;

        // The AWS services themselves take no client certificate; a store
        // of the adopter's own that speaks the S3 API may.
        let aws_service = reqwest::Url::parse(endpoint)
            .ok()
            .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
            .is_some_and(|host| {
                host.ends_with(".amazonaws.com") || host.ends_with(".amazonaws.com.cn")
            });
        let mut secured = secure(
            &HttpSink {
                name: &config.name,
                kind: "objectstorage/awsobject",
                key: "aws_s3_endpoint",
                url: endpoint,
                roots: Roots::System,
                // The SDK's own client offered the hybrid group first.
                unstated_key_exchange: Some(crate::security::provider::default_key_exchange()),
                no_client_certificate: aws_service
                    .then_some("the AWS S3 service takes no client certificate"),
                outside_provider: Some(
                    "request signing (SigV4, computed by the AWS SDK with the `hmac` and `sha2` \
                     crates)",
                ),
            },
            policy,
        )?;
        let posture = secured.posture.clone();
        let target = secured.target.take();

        let sdk_config = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .http_client(S3HttpClient::new(secured))
            .region(Region::new(region.to_string()))
            .credentials_provider(Credentials::new(
                access_key_id.trim(),
                secret_access_key.trim(),
                None,
                None,
                "subscriber configuration",
            ))
            .endpoint_url(endpoint)
            .build();
        let sink = S3Sink {
            client: Client::from_conf(sdk_config),
            bucket: bucket.to_string(),
            region: region.to_string(),
            encoding: Encoding::from_config(config.encoding.as_deref())?,
            credentials: [
                access_key_id.trim().to_string(),
                secret_access_key.trim().to_string(),
            ],
        };
        Ok((sink, posture, target))
    }

    /// The SDK's error with its whole cause chain; its own `Display` says
    /// only which stage failed. A store's answer can quote the access key id
    /// it was sent, so both credentials are taken out of the text.
    fn error<E>(&self, error: SdkError<E>) -> SinkError
    where
        E: std::error::Error + 'static,
    {
        let text = DisplayErrorContext(&error).to_string();
        let [id, secret] = &self.credentials;
        SinkError::S3(scrub(&text, &[id, secret]))
    }
}

#[async_trait]
impl Sink for S3Sink {
    /// Create the bucket when it does not exist.
    async fn prepare(&self) -> Result<(), SinkError> {
        match self.client.head_bucket().bucket(&self.bucket).send().await {
            Ok(_) => {
                info!("bucket {} exists", self.bucket);
                return Ok(());
            }
            Err(SdkError::ServiceError(e)) if e.err().code() == Some("NotFound") => {}
            Err(e) => return Err(self.error(e)),
        }
        let mut create = self.client.create_bucket().bucket(&self.bucket);
        if self.region != REGION_WITHOUT_CONSTRAINT {
            create = create.create_bucket_configuration(
                CreateBucketConfiguration::builder()
                    .location_constraint(BucketLocationConstraint::from(self.region.as_str()))
                    .build(),
            );
        }
        create.send().await.map_err(|e| self.error(e))?;
        info!("created bucket {}", self.bucket);
        Ok(())
    }

    async fn send(&self, topic: &str, record: &str) -> Result<(), SinkError> {
        let now = Utc::now();
        let body = self.encoding.encode(record)?;
        let extension = self.encoding.extension();
        for key in [
            topic_then_time(topic, &body, extension, now),
            time_then_topic(topic, &body, extension, now),
        ] {
            self.client
                .put_object()
                .bucket(&self.bucket)
                .key(&key)
                .content_type(self.encoding.media_type())
                .body(ByteStream::from(body.to_vec()))
                .send()
                .await
                .map_err(|e| self.error(e))?;
            debug!("stored {key} in bucket {}", self.bucket);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{subscriber, HttpStub};

    fn sink(port: u16, region: &str) -> S3Sink {
        S3Sink::from_config(&subscriber(&format!(
            "    subscriber_type: objectstorage\n    subtype: awsobject\n    aws_access_key_id: exampleaccesskey\n    aws_secret_access_key: examplesecretkey\n    aws_default_region: {region}\n    aws_s3_endpoint: \"http://127.0.0.1:{port}\"\n    aws_bucket_name: records\n"
        )))
        .unwrap()
    }

    #[test]
    fn every_connection_key_is_required() {
        let problem = match S3Sink::from_config(&subscriber(
            "    subscriber_type: objectstorage\n    subtype: awsobject\n    aws_access_key_id: a\n    aws_secret_access_key: b\n    aws_s3_endpoint: \"http://127.0.0.1:9000\"\n    aws_bucket_name: records\n",
        )) {
            Ok(_) => panic!("built a sink without a region"),
            Err(problem) => problem.to_string(),
        };
        assert_eq!(problem, "`aws_default_region` is required");
    }

    #[tokio::test]
    async fn record_is_stored_under_both_names() {
        let stub = HttpStub::answering(&[200, 200]);
        let record = r#"{"station_id":"KX7"}"#;
        sink(stub.port(), "us-east-1")
            .send("WX/Heartbeat", record)
            .await
            .unwrap();
        let requests = stub.requests();
        assert_eq!(requests.len(), 2);
        // An endpoint given by address is used path-style: the bucket is the
        // first path segment, not a host name. Spaces in a topic would be
        // percent-encoded, so the topic here has none.
        assert!(
            requests[0]
                .path
                .starts_with("/records/all-topic-time/WX/Heartbeat/"),
            "{}",
            requests[0].path
        );
        assert!(
            requests[1].path.starts_with("/records/all-time-topic/"),
            "{}",
            requests[1].path
        );
        for request in &requests {
            assert_eq!(request.method, "PUT");
            assert!(request.path.contains(".json?"), "{}", request.path);
            assert_eq!(request.header("content-type"), Some("application/json"));
            assert_eq!(request.body, record.as_bytes());
            assert!(request
                .header("authorization")
                .is_some_and(|value| value.contains("Credential=exampleaccesskey/")));
        }
    }

    #[tokio::test]
    async fn an_answer_that_quotes_the_access_key_id_is_reported_without_it() {
        let answer =
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>InvalidAccessKeyId</Code>\
                      <Message>The access key id exampleaccesskey does not exist</Message>\
                      <AWSAccessKeyId>exampleaccesskey</AWSAccessKeyId></Error>";
        let stub = HttpStub::answering_with_body(403, answer.as_bytes().to_vec());
        let error = sink(stub.port(), "us-east-1")
            .send("WX/Heartbeat", "{}")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("InvalidAccessKeyId"), "{error}");
        assert!(!error.contains("exampleaccesskey"), "{error}");
        assert!(!error.contains("examplesecretkey"), "{error}");
    }

    #[tokio::test]
    async fn a_cbor_sink_stores_cbor_objects() {
        let stub = HttpStub::answering(&[200, 200]);
        let sink = S3Sink::from_config(&subscriber(&format!(
            "    subscriber_type: objectstorage\n    subtype: awsobject\n    aws_access_key_id: exampleaccesskey\n    aws_secret_access_key: examplesecretkey\n    aws_default_region: us-east-1\n    aws_s3_endpoint: \"http://127.0.0.1:{}\"\n    aws_bucket_name: records\n    encoding: cbor\n",
            stub.port()
        )))
        .unwrap();
        sink.send("WX/Heartbeat", r#"{"n":1}"#).await.unwrap();
        let requests = stub.requests();
        assert_eq!(requests.len(), 2);
        for request in &requests {
            assert!(request.path.contains(".cbor?"), "{}", request.path);
            assert_eq!(request.header("content-type"), Some("application/cbor"));
            assert_eq!(request.body, [0xa1, 0x61, b'n', 0x01]);
        }
    }

    #[tokio::test]
    async fn prepare_creates_a_bucket_that_is_missing() {
        let stub = HttpStub::answering(&[404, 200]);
        sink(stub.port(), "eu-central-1").prepare().await.unwrap();
        let requests = stub.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "HEAD");
        assert_eq!(requests[1].method, "PUT");
        assert!(
            requests[1].path.starts_with("/records"),
            "{}",
            requests[1].path
        );
        let body = String::from_utf8_lossy(&requests[1].body);
        assert!(
            body.contains("<LocationConstraint>eu-central-1</LocationConstraint>"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn the_default_region_takes_no_location_constraint() {
        let stub = HttpStub::answering(&[404, 200]);
        sink(stub.port(), "us-east-1").prepare().await.unwrap();
        assert!(stub.requests()[1].body.is_empty());
    }
}
