//! The YAML files shipped in `flow/config` parse into the configuration
//! structs with the values written in them, and every entry builds.

use flow::config::{parse, PublisherConfigs, SubPubConfigs, SubscriberConfig, SubscriberConfigs};
use flow::{build_sink, Transform};

#[test]
fn publishers_local_parses() {
    let cfg: PublisherConfigs = parse(
        include_str!("../config/publishers.local.yml"),
        "publishers.local.yml",
    )
    .unwrap();
    assert_eq!(cfg.publishers.len(), 1);
    let publisher = &cfg.publishers[0];
    assert_eq!(publisher.name, "records");
    assert_eq!(publisher.publisher_type, "PUB");
    assert_eq!(publisher.address, "tcp://127.0.0.1:12000");
    assert_eq!(publisher.topic, "subsystem/message_type");
    assert_eq!(publisher.topic_suffix, None);
}

#[test]
fn subscribers_local_parses() {
    let cfg: SubscriberConfigs = parse(
        include_str!("../config/subscribers.local.yml"),
        "subscribers.local.yml",
    )
    .unwrap();
    let names: Vec<&str> = cfg.subscribers.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "All2BlobEmulator",
            "All2S3Mock",
            "MessageRates",
            "Write2LocalLog"
        ]
    );
    let emulator = &cfg.subscribers[0];
    // unquoted integers in the file land in the String fields as written
    assert_eq!(emulator.zmq_port.as_deref(), Some("12000"));
    assert_eq!(emulator.cloud_port.as_deref(), Some("10000"));
    assert_eq!(emulator.cloud_api_version.as_deref(), Some("2023-01-03"));
    assert_eq!(emulator.cloud_location_type.as_deref(), Some("Emulator"));
    assert_eq!(emulator.topics, ["*"]);
    let s3 = &cfg.subscribers[1];
    assert_eq!(s3.aws_s3_endpoint.as_deref(), Some("http://127.0.0.1:9090"));
    assert_eq!(s3.aws_bucket_name.as_deref(), Some("first-bite"));
    let local = &cfg.subscribers[3];
    assert_eq!(
        local.local_logfile.as_deref(),
        Some("first-bite-records.log")
    );
    assert_eq!(local.topics.len(), 3);
}

#[test]
fn subscribers_example_parses() {
    let cfg: SubscriberConfigs = parse(
        include_str!("../config/subscribers.example.yml"),
        "subscribers.example.yml",
    )
    .unwrap();
    let names: Vec<&str> = cfg.subscribers.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["ToRest", "AllToBlob", "AllToS3", "AllToZenoh"]);
    let rest = &cfg.subscribers[0];
    assert_eq!(rest.subscriber_type, "rest");
    assert_eq!(rest.subtype, None);
    assert_eq!(rest.endpoint_port.as_deref(), Some("5001"));
    assert_eq!(rest.endpoint_path.as_deref(), Some("/records"));
    let blob = &cfg.subscribers[1];
    // an unquoted URL with a port is a plain scalar, not a mapping
    assert_eq!(
        blob.cloud_uri.as_deref(),
        Some("https://blob.storage.example:10000")
    );
    assert_eq!(cfg.subscribers[2].subtype.as_deref(), Some("awsobject"));
    let zenoh = &cfg.subscribers[3];
    assert_eq!(zenoh.subscriber_type, "zenoh");
    assert_eq!(
        zenoh.key_template.as_deref(),
        Some("telemetry/{system}/{location}/{interface}/{message}")
    );
}

/// The variables the example file refers to, with values a sink accepts.
fn example_environment(name: &str) -> Option<String> {
    match name {
        // base64 of "example-access-key"
        "FIRST_BITE_BLOB_ACCESS_KEY" => Some("ZXhhbXBsZS1hY2Nlc3Mta2V5".to_string()),
        "FIRST_BITE_S3_ACCESS_KEY_ID" => Some("example-access-key-id".to_string()),
        "FIRST_BITE_S3_SECRET_ACCESS_KEY" => Some("example-secret-access-key".to_string()),
        _ => None,
    }
}

/// The example file shows the reference form: it holds no credential value,
/// and without its variables set each credential is refused by name.
#[test]
fn the_example_holds_references_and_no_credential() {
    let example: SubscriberConfigs = parse(
        include_str!("../config/subscribers.example.yml"),
        "subscribers.example.yml",
    )
    .unwrap();
    let mut referenced = Vec::new();
    for mut config in example.subscribers {
        let unset = config.clone().resolve(&|_| None);
        if let Err(problem) = unset {
            referenced.push(format!("{}: {problem}", config.name));
        }
        let plain = config.resolve(&example_environment).unwrap();
        assert!(plain.is_empty(), "{}: plain {plain:?}", config.name);
    }
    assert_eq!(
        referenced,
        [
            "AllToBlob: `cloud_access_key` refers to the environment variable FIRST_BITE_BLOB_ACCESS_KEY, which is not set",
            "AllToS3: `aws_access_key_id` refers to the environment variable FIRST_BITE_S3_ACCESS_KEY_ID, which is not set",
        ]
    );
}

/// Every subscriber in the shipped files builds its sink when the sink's
/// feature is compiled in, and is refused by name when it is not.
#[test]
fn shipped_subscribers_build_or_name_their_feature() {
    let local: SubscriberConfigs = parse(
        include_str!("../config/subscribers.local.yml"),
        "subscribers.local.yml",
    )
    .unwrap();
    let example: SubscriberConfigs = parse(
        include_str!("../config/subscribers.example.yml"),
        "subscribers.example.yml",
    )
    .unwrap();
    let resolved: Vec<SubscriberConfig> = local
        .subscribers
        .into_iter()
        .chain(example.subscribers)
        .map(|mut config| {
            config.resolve(&example_environment).unwrap();
            config
        })
        .collect();
    for config in &resolved {
        let feature = match (config.subscriber_type.as_str(), config.subtype.as_deref()) {
            ("objectstorage", Some("azureobject")) => Some(("azure", cfg!(feature = "azure"))),
            ("objectstorage", Some("awsobject")) => Some(("aws", cfg!(feature = "aws"))),
            ("zenoh", _) => Some(("zenoh", cfg!(feature = "zenoh"))),
            _ => None,
        };
        match (build_sink(config), feature) {
            (Ok(_), None | Some((_, true))) => {}
            // The example names a Zenoh configuration file that is not on
            // this machine; the entry got as far as reading it.
            (Err(e), Some(("zenoh", true))) => {
                assert!(e.to_string().contains("`zenoh_config`"), "{e}");
            }
            (Err(e), Some((feature, false))) => {
                let expected = format!(
                    "subscriber {}: sink {} is not compiled in; rebuild with the cargo feature `{feature}`",
                    config.name,
                    match config.subtype.as_deref() {
                        Some(subtype) => format!("{}/{subtype}", config.subscriber_type),
                        None => config.subscriber_type.clone(),
                    }
                );
                assert!(e.to_string().starts_with(&expected), "{e}");
            }
            (Ok(_), Some((feature, false))) => {
                panic!("{} built without the feature {feature}", config.name)
            }
            (Err(e), _) => panic!("{}: {e}", config.name),
        }
    }
}

#[test]
fn subpubs_parse_and_carry_a_cot_mapping() {
    for (name, yaml, suffix) in [
        (
            "subpubs.local.yml",
            include_str!("../config/subpubs.local.yml"),
            "JSON2CoT",
        ),
        (
            "subpubs.example.yml",
            include_str!("../config/subpubs.example.yml"),
            "CoT",
        ),
    ] {
        let cfg: SubPubConfigs = parse(yaml, name).unwrap();
        assert_eq!(cfg.subpubs.len(), 1, "{name}");
        let subpub = &cfg.subpubs[0];
        assert_eq!(subpub.subscriber.zmq_port.as_deref(), Some("12000"));
        assert_eq!(subpub.subscriber.topics, ["WX/Position"]);
        assert_eq!(subpub.publisher.address, "tcp://127.0.0.1:12001");
        assert_eq!(subpub.publisher.topic_suffix.as_deref(), Some(suffix));
        let Transform::Cot(mapping) = Transform::from_config(&subpub.subscriber).unwrap();
        assert_eq!(mapping.uid, ["station_id"]);
        assert_eq!(mapping.stale_minutes, 60);
    }
}
