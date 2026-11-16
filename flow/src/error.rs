//! Errors raised while reading configuration and starting the feeds.

use std::path::PathBuf;

/// What is wrong with one entry of a configuration file.
#[derive(Debug, thiserror::Error)]
pub enum Problem {
    #[error("unknown subscriber_type {subscriber_type:?} with subtype {}", display_subtype(.subtype))]
    UnknownSink {
        subscriber_type: String,
        subtype: Option<String>,
    },
    #[error(
        "sink {sink} is not compiled in; rebuild with the cargo feature `{feature}` \
         (cargo build -p mind --features {feature})"
    )]
    FeatureOff {
        sink: &'static str,
        feature: &'static str,
    },
    #[error(
        "the `xf` transform is not compiled in; rebuild with the cargo feature \
         `goat-transformers` (cargo build -p mind --features goat-transformers)"
    )]
    XfFeatureOff,
    #[error("`{0}` is required")]
    MissingKey(&'static str),
    #[error("`{key}`: {reason}")]
    InvalidValue { key: &'static str, reason: String },
    #[error("unknown transform: subscriber_type {subscriber_type:?} with subtype {}; the transform is subscriber_type \"transformer\" with subtype \"cot\" or \"xf\"", display_subtype(.subtype))]
    UnknownTransform {
        subscriber_type: String,
        subtype: Option<String>,
    },
    #[error("the publisher binds port {0}, which is the port the subscriber reads; the output would feed the input")]
    Loopback(String),
    /// A requirement of the security block that the entry cannot meet.
    #[error("{0}")]
    Security(String),
    #[error("`{key}` refers to the environment variable {variable}, which is not set")]
    UnsetVariable { key: &'static str, variable: String },
    #[error(
        "`{key}` contains `${{` but is not one whole reference; a reference is the whole value, \
         written ${{NAME}}"
    )]
    MalformedReference { key: &'static str },
    #[error("`{key}` and `{key}_file` are both set; give one")]
    ValueAndFile { key: &'static str },
    #[error("`{key}_file`: cannot read {}: {reason}", path.display())]
    CredentialFile {
        key: &'static str,
        path: PathBuf,
        reason: String,
    },
}

fn display_subtype(subtype: &Option<String>) -> String {
    match subtype {
        Some(subtype) => format!("{subtype:?}"),
        None => "unset".to_string(),
    }
}

/// A configuration file that cannot be used, naming the entry at fault.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{origin}: {source}")]
    Parse {
        origin: String,
        source: serde_norway::Error,
    },
    #[error("publisher {publisher}: {problem}")]
    Publisher { publisher: String, problem: Problem },
    #[error("subscriber {subscriber}: {problem}")]
    Subscriber {
        subscriber: String,
        problem: Problem,
    },
    #[error("subpub {subpub}: {problem}")]
    SubPub { subpub: String, problem: Problem },
    /// The security block itself cannot be used.
    #[error("security: {0}")]
    Security(String),
}

/// Why publishers, subscribers or subpubs could not start.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("{what}: {source}")]
    Zmq { what: String, source: zmq::Error },
    #[error("cannot start the sink runtime: {0}")]
    Runtime(std::io::Error),
    #[error("cannot start the thread for {name}: {source}")]
    Thread {
        name: String,
        source: std::io::Error,
    },
}

impl Error {
    pub(crate) fn zmq(what: impl Into<String>) -> impl FnOnce(zmq::Error) -> Error {
        let what = what.into();
        move |source| Error::Zmq { what, source }
    }
}
