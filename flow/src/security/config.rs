//! The `security` block as it is written: the one for the whole process and
//! the override a single subscriber may carry.

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// How records may leave the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
    /// Nothing is required of any path. This is what a configuration with
    /// no `security` block gets, and what every release before the block
    /// existed did.
    Unenforced,
    /// Plaintext on the loopback interface only: a sink that reaches another
    /// host is refused.
    LoopbackPlaintext,
    /// Every network sink uses TLS and authenticates the server.
    Tls,
    /// As `tls`, and every network sink presents a client certificate.
    Mtls,
}

impl Transport {
    pub fn as_str(self) -> &'static str {
        match self {
            Transport::Unenforced => "unenforced",
            Transport::LoopbackPlaintext => "loopback-plaintext",
            Transport::Tls => "tls",
            Transport::Mtls => "mtls",
        }
    }

    /// Whether the level requires TLS of a network sink.
    pub fn requires_tls(self) -> bool {
        matches!(self, Transport::Tls | Transport::Mtls)
    }
}

impl fmt::Display for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The oldest TLS version a sink may negotiate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum MinTls {
    #[serde(rename = "1.2")]
    Tls12,
    #[serde(rename = "1.3")]
    Tls13,
}

impl MinTls {
    pub fn as_str(self) -> &'static str {
        match self {
            MinTls::Tls12 => "1.2",
            MinTls::Tls13 => "1.3",
        }
    }
}

impl fmt::Display for MinTls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// YAML reads an unquoted `1.3` as a number, so both spellings are taken.
impl<'de> Deserialize<'de> for MinTls {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Version;
        impl Visitor<'_> for Version {
            type Value = MinTls;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("1.2 or 1.3")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<MinTls, E> {
                match value {
                    "1.2" => Ok(MinTls::Tls12),
                    "1.3" => Ok(MinTls::Tls13),
                    other => Err(E::custom(format!(
                        "min_tls {other:?} is neither 1.2 nor 1.3"
                    ))),
                }
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<MinTls, E> {
                // The two literals parse to exactly these values.
                if value == 1.2 {
                    Ok(MinTls::Tls12)
                } else if value == 1.3 {
                    Ok(MinTls::Tls13)
                } else {
                    Err(E::custom(format!("min_tls {value} is neither 1.2 nor 1.3")))
                }
            }
        }
        deserializer.deserialize_any(Version)
    }
}

/// Which key-exchange groups a TLS client offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeyExchange {
    /// X25519, P-256, P-384.
    Classical,
    /// X25519MLKEM768 first, the classical groups after it: a peer without
    /// the hybrid group still connects.
    HybridPqcPreferred,
    /// X25519MLKEM768 only: a peer without it is refused.
    HybridPqcRequired,
}

impl KeyExchange {
    pub fn as_str(self) -> &'static str {
        match self {
            KeyExchange::Classical => "classical",
            KeyExchange::HybridPqcPreferred => "hybrid-pqc-preferred",
            KeyExchange::HybridPqcRequired => "hybrid-pqc-required",
        }
    }

    pub fn is_hybrid(self) -> bool {
        !matches!(self, KeyExchange::Classical)
    }
}

impl fmt::Display for KeyExchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether the crypto provider must be the FIPS module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fips {
    Off,
    Required,
}

impl Fips {
    pub fn as_str(self) -> &'static str {
        match self {
            Fips::Off => "off",
            Fips::Required => "required",
        }
    }
}

impl fmt::Display for Fips {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether the Zenoh sink may, or must, use QUIC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Quic {
    Allowed,
    Required,
}

impl Quic {
    pub fn as_str(self) -> &'static str {
        match self {
            Quic::Allowed => "allowed",
            Quic::Required => "required",
        }
    }
}

impl fmt::Display for Quic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The `security:` block for the whole process.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityConfig {
    /// The level. A block that does not state one is refused.
    pub transport: Transport,
    pub min_tls: Option<MinTls>,
    pub key_exchange: Option<KeyExchange>,
    pub fips: Option<Fips>,
    pub quic: Option<Quic>,
    /// Client certificate chain, PEM, presented by the HTTP sinks.
    pub client_cert: Option<PathBuf>,
    /// Private key of `client_cert`, PEM.
    pub client_key: Option<PathBuf>,
    /// Certificate authorities the HTTP sinks trust, PEM, in place of the
    /// roots each uses otherwise.
    pub ca_bundle: Option<PathBuf>,
    /// Permit a ZeroMQ socket to bind an address that is not loopback.
    #[serde(default)]
    pub allow_plaintext_bind: bool,
}

/// The `security:` key of one subscriber: where that sink differs from the
/// block. A key that changes a requirement needs `reason`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SinkOverride {
    pub transport: Option<Transport>,
    pub min_tls: Option<MinTls>,
    pub key_exchange: Option<KeyExchange>,
    pub fips: Option<Fips>,
    pub quic: Option<Quic>,
    pub client_cert: Option<PathBuf>,
    pub client_key: Option<PathBuf>,
    pub ca_bundle: Option<PathBuf>,
    /// Why this sink differs; printed in the posture report.
    pub reason: Option<String>,
}

/// A file whose top-level `security:` key is the block. Other keys are
/// ignored, so the subscriber file can carry it.
#[derive(Debug, Deserialize)]
pub(crate) struct SecurityFile {
    pub(crate) security: Option<SecurityConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse;

    fn block(yaml: &str) -> Result<SecurityConfig, String> {
        parse::<SecurityFile>(yaml, "test")
            .map(|file| file.security.expect("a security block"))
            .map_err(|e| e.to_string())
    }

    #[test]
    fn every_key_parses() {
        let config = block(
            "security:\n  transport: mtls\n  min_tls: 1.3\n  key_exchange: hybrid-pqc-required\n  \
             fips: required\n  quic: required\n  client_cert: /etc/first-bite/tls/client.pem\n  \
             client_key: /etc/first-bite/tls/client.key\n  ca_bundle: /etc/first-bite/tls/ca.pem\n  \
             allow_plaintext_bind: true\n",
        )
        .unwrap();
        assert_eq!(config.transport, Transport::Mtls);
        assert_eq!(config.min_tls, Some(MinTls::Tls13));
        assert_eq!(config.key_exchange, Some(KeyExchange::HybridPqcRequired));
        assert_eq!(config.fips, Some(Fips::Required));
        assert_eq!(config.quic, Some(Quic::Required));
        assert_eq!(
            config.client_cert.as_deref(),
            Some(std::path::Path::new("/etc/first-bite/tls/client.pem"))
        );
        assert!(config.allow_plaintext_bind);
    }

    #[test]
    fn the_level_is_the_only_required_key() {
        let config = block("security:\n  transport: loopback-plaintext\n").unwrap();
        assert_eq!(config.transport, Transport::LoopbackPlaintext);
        assert_eq!(config.min_tls, None);
        assert!(!config.allow_plaintext_bind);
        let error = block("security:\n  min_tls: 1.3\n").unwrap_err();
        assert!(error.contains("missing field `transport`"), "{error}");
    }

    #[test]
    fn min_tls_is_taken_quoted_or_not() {
        for text in ["1.2", "\"1.2\"", "'1.2'"] {
            let config = block(&format!("security:\n  transport: tls\n  min_tls: {text}\n"));
            assert_eq!(config.unwrap().min_tls, Some(MinTls::Tls12), "{text}");
        }
        for text in ["1.1", "\"1.0\"", "2", "tls13"] {
            let error = block(&format!("security:\n  transport: tls\n  min_tls: {text}\n"));
            assert!(error.is_err(), "{text} was accepted");
        }
    }

    #[test]
    fn unknown_keys_and_values_are_refused() {
        let unknown_key = block("security:\n  transport: tls\n  cipher: aes\n").unwrap_err();
        assert!(
            unknown_key.contains("unknown field `cipher`"),
            "{unknown_key}"
        );
        let unknown_level = block("security:\n  transport: ssl\n").unwrap_err();
        assert!(
            unknown_level.contains("unknown variant `ssl`"),
            "{unknown_level}"
        );
        let unknown_kx =
            block("security:\n  transport: tls\n  key_exchange: quantum\n").unwrap_err();
        assert!(
            unknown_kx.contains("unknown variant `quantum`"),
            "{unknown_kx}"
        );
        // An inline certificate is not a path key.
        let inline = block("security:\n  transport: mtls\n  client_cert_pem: abc\n").unwrap_err();
        assert!(
            inline.contains("unknown field `client_cert_pem`"),
            "{inline}"
        );
    }

    #[test]
    fn a_file_without_the_key_has_no_block() {
        let file: SecurityFile = parse("subscribers: []\n", "test").unwrap();
        assert!(file.security.is_none());
    }
}
