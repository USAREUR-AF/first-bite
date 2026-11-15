//! Joining a goat data fabric. Optional: this module exists only in a build
//! with the cargo feature `goat`, and nothing in it runs unless a subscriber
//! of type `goat` is configured or a `mind goat` command is given.
//!
//! A goatnet's routers are stock Zenoh 1.9.0 behind mutual TLS, with an
//! access-control list that grants each certificate one key prefix.
//! `docs/goat/contract.md` lists what that requires of a publisher and where
//! goat states it. The pieces here:
//!
//! * [`config`]: the goat configuration file (`GOAT_CONFIG_PATH`);
//! * [`identity`]: the shapes of a slot UUID, an org prefix, an alias and a
//!   mesh peer name;
//! * [`cert`]: checks of the certificate, key and root before any connection;
//! * [`keys`]: the key every sample is put on;
//! * [`session`]: the Zenoh client configuration, and reading the fabric;
//! * [`sink`]: the sink behind `subscriber_type: goat`;
//! * [`bundle`]: a configuration from goat's onboarding archive.

pub mod bundle;
pub mod cert;
pub mod config;
pub mod identity;
pub mod keys;
pub mod session;
pub mod sink;

pub use config::{GoatConfig, GOAT_CONFIG_ENV};
pub use identity::Principal;
pub use keys::KeyLayout;

use serde_json::Value;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Why a goat configuration cannot be used. Each message says what to change.
#[derive(Debug, thiserror::Error)]
pub enum GoatError {
    #[error(
        "GOAT_CONFIG_PATH is not set; set it to the goat configuration file \
         (`mind goat init --bundle <dir>` writes one from an onboarding archive)"
    )]
    NoConfig,
    #[error("cannot read {what} {}: {source}", path.display())]
    File {
        what: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{origin}: {source}")]
    Parse {
        origin: String,
        source: serde_norway::Error,
    },
    #[error("goatnet.endpoints is empty; add the router endpoint the goatnet's administrator gave, for example tls/zenoh.goat.example:7447")]
    NoEndpoints,
    #[error(
        "goatnet.endpoints: {0:?} is not a goat router endpoint; write it as \
         tls/<host>:<port>, for example tls/zenoh.goat.example:7447"
    )]
    Endpoint(String),
    #[error(
        "set exactly one of identity.slot (the slot UUID, on a slot-pool goatnet) and \
         identity.org (the org prefix, on a per-org goatnet)"
    )]
    IdentityChoice,
    #[error(
        "identity.slot {value:?} is not a slot UUID (32 lower-case hexadecimal digits): {fix}"
    )]
    Uuid { value: String, fix: String },
    #[error("{field} {value:?} {problem}")]
    Name {
        field: &'static str,
        value: String,
        problem: &'static str,
    },
    #[error(
        "identity.mesh_peer_name {value:?} is not a goat peer name \
         (peer-<handle>[-<org>]-<site>-<YYYY-MM-DD>[-<hostname>]): {problem}; \
         copy the name the goatnet shows for this host, or remove the setting"
    )]
    PeerName {
        value: String,
        problem: &'static str,
    },
    #[error(
        "{field} {value:?} cannot be a key chunk (1 to 63 characters of a-z, 0-9, _ and -); \
         write it as {suggestion}"
    )]
    Chunk {
        field: &'static str,
        value: String,
        suggestion: String,
    },
    #[error("publish.health_interval_seconds must be at least 1")]
    HealthInterval,
    #[error("{} holds no {what}; it must be a PEM file with a {what}", path.display())]
    NotPem { what: &'static str, path: PathBuf },
    #[error("{} is not a certificate this program can read: {reason}", path.display())]
    BadCertificate { path: PathBuf, reason: String },
    #[error(
        "the private key {} can be read by other users (mode {mode:o}); run: chmod 600 {}",
        path.display(),
        path.display()
    )]
    KeyPermissions { path: PathBuf, mode: u32 },
    #[error(
        "the certificate {} has common name {found:?} but the configuration names {expected:?} \
         ({setting}); use the certificate issued for {expected:?}, or set {setting} to match \
         the certificate you were issued",
        path.display()
    )]
    CommonName {
        path: PathBuf,
        found: String,
        expected: String,
        setting: &'static str,
    },
    #[error(
        "the certificate {} expired at {not_after} (the clock here reads {now}); ask the \
         goatnet's administrator to issue a new one",
        path.display()
    )]
    Expired {
        path: PathBuf,
        not_after: String,
        now: String,
    },
    #[error(
        "the certificate {} is not valid until {not_before} (the clock here reads {now}); \
         correct this host's clock, or wait",
        path.display()
    )]
    NotYetValid {
        path: PathBuf,
        not_before: String,
        now: String,
    },
    #[error(
        "the certificate {} is not verified by the root {}: {reason}; use the root \
         certificate from the same onboarding archive as the certificate",
        leaf.display(),
        ca.display()
    )]
    Chain {
        leaf: PathBuf,
        ca: PathBuf,
        reason: String,
    },
    #[error("{0}")]
    Bundle(String),
    #[error(
        "{0:?} names the whole keyspace; a goat router grants that to named infrastructure \
         certificates only. Give a key expression with at least one literal chunk, for example {1}"
    )]
    WholeKeyspace(String, String),
    #[error("Zenoh: {0}")]
    Zenoh(String),
    #[error("cannot start the runtime: {0}")]
    Runtime(std::io::Error),
}

/// What `mind` knows and this crate does not: the messages of the loaded
/// dictionary and the schema of each record.
#[derive(Debug, Clone, Default)]
pub struct Declarations {
    /// The dictionary's name and version, where it states them.
    pub dictionary: Option<String>,
    pub messages: Vec<MessageDeclaration>,
}

/// One message of the dictionary.
#[derive(Debug, Clone)]
pub struct MessageDeclaration {
    pub interface: String,
    pub message: String,
    /// JSON Schema of the record published for this message, without `$id`.
    pub schema: Value,
}

static DECLARATIONS: OnceLock<Declarations> = OnceLock::new();

/// Give the sink the dictionary's messages. Called once, before the
/// subscribers are built; a second call is ignored.
pub fn register(declarations: Declarations) {
    let _ = DECLARATIONS.set(declarations);
}

pub(crate) fn registered() -> Declarations {
    DECLARATIONS.get().cloned().unwrap_or_default()
}
