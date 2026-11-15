//! The shipped example of the security block parses, and the shipped
//! subscriber files meet the level each is written for.

use flow::config::{parse, SubscriberConfigs};
use flow::security::{Fips, KeyExchange, MinTls, Quic, Security, Transport};
use flow::{build_sink_secured, describe_sink};

#[test]
fn the_example_block_parses_with_the_values_written_in_it() {
    let security = Security::from_yaml(
        include_str!("../config/security.example.yml"),
        "security.example.yml",
    );
    // A build without `pqc` refuses the example's hybrid preference by name.
    if !cfg!(feature = "pqc") {
        let error = security.unwrap_err().to_string();
        assert!(error.contains("cargo feature `pqc`"), "{error}");
        return;
    }
    let security = security.unwrap().expect("the file has a security block");
    assert!(security.stated);
    assert_eq!(security.base.transport, Transport::Tls);
    assert_eq!(security.base.min_tls, MinTls::Tls13);
    assert_eq!(security.base.key_exchange, KeyExchange::HybridPqcPreferred);
    assert_eq!(security.base.fips, Fips::Off);
    assert_eq!(security.base.quic, Quic::Allowed);
    assert!(!security.allow_plaintext_bind);
}

#[test]
fn a_document_without_the_key_has_no_block() {
    let none = Security::from_yaml(
        include_str!("../config/subscribers.local.yml"),
        "subscribers.local.yml",
    )
    .unwrap();
    assert!(none.is_none());
}

/// The local development file talks to emulators over plain HTTP on
/// loopback: accepted with no block and under `loopback-plaintext`, refused
/// under `tls`.
#[test]
fn the_local_subscribers_are_loopback_plaintext() {
    let local: SubscriberConfigs = parse(
        include_str!("../config/subscribers.local.yml"),
        "subscribers.local.yml",
    )
    .unwrap();
    let level = |level: &str| {
        Security::from_yaml(
            &format!("security:\n  transport: {level}\n  key_exchange: classical\n"),
            "test",
        )
        .unwrap()
        .unwrap()
    };
    for config in &local.subscribers {
        let network = config.subscriber_type == "objectstorage";
        let subtype = config.subtype.as_deref();
        if (subtype == Some("azureobject") && !cfg!(feature = "azure"))
            || (subtype == Some("awsobject") && !cfg!(feature = "aws"))
        {
            continue;
        }
        assert!(
            build_sink_secured(config, &Security::unstated()).is_ok(),
            "{}",
            config.name
        );
        let (posture, _) = describe_sink(config, &level("loopback-plaintext")).unwrap();
        assert!(!posture.off_host, "{}", config.name);
        let under_tls = describe_sink(config, &level("tls"));
        assert_eq!(under_tls.is_err(), network, "{}", config.name);
        if let Err(error) = under_tls {
            assert!(
                error.to_string().contains("needs an https:// endpoint"),
                "{error}"
            );
        }
    }
}
