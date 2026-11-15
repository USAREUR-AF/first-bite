//! The process's one crypto provider.
//!
//! Every TLS connection the process makes goes through rustls with the
//! aws-lc-rs provider. The HTTP sinks are handed a TLS configuration that
//! carries the provider; the Zenoh client reads rustls's process default,
//! which [`install`] sets before anything can open a connection.

use super::config::{KeyExchange, MinTls};
use rustls::crypto::aws_lc_rs::kx_group;
use rustls::crypto::{CryptoProvider, SupportedKxGroup};
use rustls::{SupportedProtocolVersion, ALL_VERSIONS};
use std::sync::Arc;

// The FIPS module is linked statically into the binary on these targets.
// Elsewhere upstream builds it as a shared library that would have to ship
// beside the binary, which no release layout of this project carries.
#[cfg(all(
    feature = "fips",
    not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))
))]
compile_error!(
    "the cargo feature `fips` builds for x86-64 and 64-bit Arm Linux only (the FIPS module of \
     AWS-LC is linked statically there); see docs/security/transport.md"
);

/// Name of the library every TLS connection uses.
pub const PROVIDER_NAME: &str = "aws-lc-rs";

/// The cargo features of this binary that the posture report names.
pub fn features() -> Vec<&'static str> {
    let mut features = Vec::new();
    if cfg!(feature = "azure") {
        features.push("azure");
    }
    if cfg!(feature = "aws") {
        features.push("aws");
    }
    if cfg!(feature = "zenoh") {
        features.push("zenoh");
    }
    if cfg!(feature = "quic") {
        features.push("quic");
    }
    if cfg!(feature = "pqc") {
        features.push("pqc");
    }
    if cfg!(feature = "fips") {
        features.push("fips");
    }
    features
}

/// `features()` as it is printed: `azure, aws, pqc`, or `none`.
pub fn features_text() -> String {
    let features = features();
    if features.is_empty() {
        "none".to_string()
    } else {
        features.join(", ")
    }
}

/// Version of the AWS-LC library that is linked in, asked of the library.
pub fn library_version() -> &'static str {
    aws_lc_rs::awslc_version()
}

/// Whether the linked library is the FIPS module running in FIPS mode,
/// asked of the library at run time.
pub fn fips_module_active() -> bool {
    aws_lc_rs::try_fips_mode().is_ok()
}

/// The key-exchange preference this binary uses when the configuration does
/// not state one.
pub fn default_key_exchange() -> KeyExchange {
    if cfg!(feature = "pqc") {
        KeyExchange::HybridPqcPreferred
    } else {
        KeyExchange::Classical
    }
}

/// The groups a policy offers, in the order offered. In a `fips` build the
/// groups the module does not approve are left out.
///
/// The hybrid group is offered only by a `pqc` build; asking for it without
/// one is refused before this is reached.
pub fn groups(key_exchange: KeyExchange) -> Vec<&'static dyn SupportedKxGroup> {
    let classical: [&'static dyn SupportedKxGroup; 3] =
        [kx_group::X25519, kx_group::SECP256R1, kx_group::SECP384R1];
    let mut groups: Vec<&'static dyn SupportedKxGroup> = Vec::new();
    if key_exchange.is_hybrid() && cfg!(feature = "pqc") {
        groups.push(kx_group::X25519MLKEM768);
    }
    if key_exchange != KeyExchange::HybridPqcRequired {
        groups.extend(classical);
    }
    if cfg!(feature = "fips") {
        groups.retain(|group| group.fips());
    }
    groups
}

/// The names of [`groups`], as rustls prints them.
pub fn group_names(key_exchange: KeyExchange) -> Vec<String> {
    groups(key_exchange)
        .iter()
        .map(|group| format!("{:?}", group.name()))
        .collect()
}

/// The protocol versions a minimum allows.
pub fn versions(min_tls: MinTls) -> Vec<&'static SupportedProtocolVersion> {
    ALL_VERSIONS
        .iter()
        .copied()
        .filter(|version| min_tls == MinTls::Tls12 || **version == rustls::version::TLS13)
        .collect()
}

/// The provider for one policy: aws-lc-rs with the policy's key-exchange
/// groups, and without the TLS 1.2 cipher suites when the minimum is 1.3.
/// A client that builds its configuration from this provider with rustls's
/// default versions is therefore held to the minimum as well.
pub fn build(min_tls: MinTls, key_exchange: KeyExchange) -> CryptoProvider {
    let mut provider = rustls::crypto::aws_lc_rs::default_provider();
    provider.kx_groups = groups(key_exchange);
    if min_tls == MinTls::Tls13 {
        provider
            .cipher_suites
            .retain(|suite| suite.version() == &rustls::version::TLS13);
    }
    provider
}

/// Why the process default could not be set.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "another TLS crypto provider was installed before first-bite's; every connection must \
     use the one provider, so the process stops"
)]
pub struct AlreadyInstalled;

/// Make `provider` rustls's process default. Called once, at start.
pub fn install(provider: CryptoProvider) -> Result<Arc<CryptoProvider>, AlreadyInstalled> {
    provider.install_default().map_err(|_| AlreadyInstalled)?;
    CryptoProvider::get_default()
        .cloned()
        .ok_or(AlreadyInstalled)
}

/// The Zenoh client builds its TLS configuration from rustls's process
/// default and installs `ring` when there is none. [`super::activate`] has
/// normally set the default; a caller that skipped it still gets the one
/// provider, built from the security block's settings.
#[cfg(feature = "zenoh")]
pub(crate) fn ensure_installed(base: &super::Policy) {
    if CryptoProvider::get_default().is_none() {
        let _ = install(build(base.min_tls, base.key_exchange));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classical_offers_the_three_classical_groups() {
        let names = group_names(KeyExchange::Classical);
        if cfg!(feature = "fips") {
            assert_eq!(names, ["secp256r1", "secp384r1"]);
        } else {
            assert_eq!(names, ["X25519", "secp256r1", "secp384r1"]);
        }
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn hybrid_is_offered_first_or_alone() {
        let preferred = group_names(KeyExchange::HybridPqcPreferred);
        assert_eq!(preferred[0], "X25519MLKEM768");
        assert!(preferred.len() > 1);
        assert_eq!(
            preferred[1..],
            group_names(KeyExchange::Classical),
            "the classical groups follow"
        );
        assert_eq!(
            group_names(KeyExchange::HybridPqcRequired),
            ["X25519MLKEM768"]
        );
    }

    #[cfg(not(feature = "pqc"))]
    #[test]
    fn a_build_without_pqc_never_offers_the_hybrid_group() {
        for key_exchange in [
            KeyExchange::Classical,
            KeyExchange::HybridPqcPreferred,
            KeyExchange::HybridPqcRequired,
        ] {
            assert!(!group_names(key_exchange).contains(&"X25519MLKEM768".to_string()));
        }
        assert_eq!(default_key_exchange(), KeyExchange::Classical);
    }

    #[test]
    fn a_minimum_of_1_3_leaves_no_tls_1_2_cipher_suite() {
        let provider = build(MinTls::Tls13, KeyExchange::Classical);
        assert!(!provider.cipher_suites.is_empty());
        assert!(provider
            .cipher_suites
            .iter()
            .all(|suite| suite.version() == &rustls::version::TLS13));
        let both = build(MinTls::Tls12, KeyExchange::Classical);
        assert!(both
            .cipher_suites
            .iter()
            .any(|suite| suite.version() == &rustls::version::TLS12));
        assert_eq!(versions(MinTls::Tls13).len(), 1);
        assert_eq!(versions(MinTls::Tls12).len(), 2);
    }

    /// A client that takes rustls's default versions, as the Zenoh client
    /// does, still builds from a provider restricted to TLS 1.3.
    #[test]
    fn default_versions_work_with_a_1_3_only_provider() {
        let provider = Arc::new(build(MinTls::Tls13, KeyExchange::Classical));
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions();
        assert!(config.is_ok());
    }

    #[test]
    fn the_fips_query_matches_the_build() {
        assert_eq!(fips_module_active(), cfg!(feature = "fips"));
        assert!(!library_version().is_empty());
        assert_eq!(
            build(MinTls::Tls12, default_key_exchange()).fips(),
            cfg!(feature = "fips")
        );
    }
}
