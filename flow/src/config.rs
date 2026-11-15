//! The YAML configuration files and where they are found.
//!
//! Each file is looked up the same way: the path in its environment variable,
//! else `config/<name>` beside the executable. Publishers fall back to a
//! built-in default, because publishing on the local ZeroMQ socket is what the
//! application is for. Subscribers and subpubs have no default: with no file
//! there are none.

use crate::error::ConfigError;
use log::info;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::{env, fs};

pub const PUBLISHER_CONFIG_ENV: &str = "PUBLISHER_CONFIG_PATH";
pub const SUBSCRIBER_CONFIG_ENV: &str = "SUBSCRIBER_CONFIG_PATH";
pub const SUBPUB_CONFIG_ENV: &str = "SUBPUB_CONFIG_PATH";

const PUBLISHER_CONFIG_FILE: &str = "config/publishers.yml";
const SUBSCRIBER_CONFIG_FILE: &str = "config/subscribers.yml";
const SUBPUB_CONFIG_FILE: &str = "config/subpubs.yml";

/// One ZeroMQ PUB socket bound on `address`: the single built-in publisher.
pub const DEFAULT_PUBLISHERS: &str = include_str!("../config/publishers.local.yml");

/// A ZeroMQ socket that records are published on.
#[derive(Debug, Clone, Deserialize)]
pub struct PublisherConfig {
    /// Used in log lines and in the names of generated key files.
    pub name: String,
    /// `PUB` or `PUSH`.
    pub publisher_type: String,
    /// Address the socket binds, e.g. `tcp://127.0.0.1:12000`.
    pub address: String,
    /// How the topic of each record is formed: `*`, `catchall`, `location`,
    /// `message_type` or `subsystem/message_type`.
    pub topic: String,
    /// In a subpub, appended to the incoming topic as `<topic>/<suffix>`.
    pub topic_suffix: Option<String>,
}

/// A reader of the local ZeroMQ feed and the sink (or, in a subpub, the
/// transform) its records go to. Which keys apply depends on
/// `subscriber_type` and `subtype`; `flow/config/README.md` lists them.
#[derive(Debug, Clone, Deserialize)]
pub struct SubscriberConfig {
    pub name: String,
    pub subscriber_type: String,
    pub subtype: Option<String>,
    /// Host and port of the publisher socket to read.
    pub zmq_url: Option<String>,
    pub zmq_port: Option<String>,
    /// Topics to forward; `*` or `all` forwards everything.
    pub topics: Vec<String>,
    // REST
    pub endpoint_url: Option<String>,
    pub endpoint_port: Option<String>,
    pub endpoint_path: Option<String>,
    /// Accepted for compatibility; nothing reads it.
    pub connection_string: Option<String>,
    // Azure Blob Storage
    pub cloud_account: Option<String>,
    pub cloud_access_key: Option<String>,
    pub cloud_container: Option<String>,
    /// `Emulator`, `Custom` or `Public`.
    pub cloud_location_type: Option<String>,
    pub cloud_address: Option<String>,
    pub cloud_port: Option<String>,
    pub cloud_uri: Option<String>,
    pub cloud_api_version: Option<String>,
    // S3
    pub aws_access_key_id: Option<String>,
    pub aws_secret_access_key: Option<String>,
    pub aws_default_region: Option<String>,
    pub aws_s3_endpoint: Option<String>,
    pub aws_bucket_name: Option<String>,
    // Local files
    pub local_logfile: Option<String>,
    // Zenoh
    /// Path of a Zenoh JSON5 configuration file; unset means Zenoh defaults.
    pub zenoh_config: Option<String>,
    /// Key expression each record is put on, with `{placeholders}`.
    pub key_template: Option<String>,
    /// Field mapping for the Cursor-on-Target transform of a subpub.
    pub cot: Option<cotify::Mapping>,
}

/// A subscriber whose records are transformed and published again on a
/// second socket.
#[derive(Debug, Clone, Deserialize)]
pub struct SubPubConfig {
    pub name: String,
    pub subscriber: SubscriberConfig,
    pub publisher: PublisherConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PublisherConfigs {
    #[serde(default)]
    pub publishers: Vec<PublisherConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubscriberConfigs {
    #[serde(default)]
    pub subscribers: Vec<SubscriberConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubPubConfigs {
    #[serde(default)]
    pub subpubs: Vec<SubPubConfig>,
}

/// The configured publishers, or the built-in default when no file is given.
pub fn load_publisher_configs() -> Result<Vec<PublisherConfig>, ConfigError> {
    let configs: PublisherConfigs = match find(PUBLISHER_CONFIG_ENV, PUBLISHER_CONFIG_FILE) {
        Some(path) => read(&path)?,
        None => {
            info!(
                "no publisher configuration ({PUBLISHER_CONFIG_ENV} is unset and there is no \
                 {PUBLISHER_CONFIG_FILE} beside the executable); using the built-in publisher"
            );
            parse(DEFAULT_PUBLISHERS, "built-in publisher configuration")?
        }
    };
    Ok(configs.publishers)
}

/// The configured subscribers; none when no file is given.
pub fn load_subscriber_configs() -> Result<Vec<SubscriberConfig>, ConfigError> {
    match find(SUBSCRIBER_CONFIG_ENV, SUBSCRIBER_CONFIG_FILE) {
        Some(path) => Ok(read::<SubscriberConfigs>(&path)?.subscribers),
        None => {
            info!(
                "no subscriber configuration ({SUBSCRIBER_CONFIG_ENV} is unset and there is no \
                 {SUBSCRIBER_CONFIG_FILE} beside the executable); running no subscribers"
            );
            Ok(Vec::new())
        }
    }
}

/// The configured subpubs; none when no file is given.
pub fn load_subpub_configs() -> Result<Vec<SubPubConfig>, ConfigError> {
    match find(SUBPUB_CONFIG_ENV, SUBPUB_CONFIG_FILE) {
        Some(path) => Ok(read::<SubPubConfigs>(&path)?.subpubs),
        None => {
            info!(
                "no subpub configuration ({SUBPUB_CONFIG_ENV} is unset and there is no \
                 {SUBPUB_CONFIG_FILE} beside the executable); running no subpubs"
            );
            Ok(Vec::new())
        }
    }
}

/// The file to read: the one the environment variable names, else the default
/// file beside the executable when it exists.
fn find(env_var: &str, default_file: &str) -> Option<PathBuf> {
    if let Some(path) = env::var_os(env_var).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let beside_executable = env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(default_file)));
    beside_executable.filter(|path| path.is_file())
}

fn read<T: DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
    let yaml = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    info!("reading {}", path.display());
    parse(&yaml, &path.display().to_string())
}

/// Parse one configuration file; `origin` names it in the error.
pub fn parse<T: DeserializeOwned>(yaml: &str, origin: &str) -> Result<T, ConfigError> {
    serde_norway::from_str(yaml).map_err(|source| ConfigError::Parse {
        origin: origin.to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_variable_and_no_file_finds_nothing() {
        let found = find(
            "FLOW_TEST_VARIABLE_THAT_IS_NEVER_SET",
            "config/flow-test-file-that-does-not-exist.yml",
        );
        assert_eq!(found, None);
    }

    #[test]
    fn the_built_in_publisher_is_one_local_pub_socket() {
        let configs: PublisherConfigs = parse(DEFAULT_PUBLISHERS, "built-in").unwrap();
        assert_eq!(configs.publishers.len(), 1);
        assert_eq!(configs.publishers[0].publisher_type, "PUB");
        assert_eq!(configs.publishers[0].address, "tcp://127.0.0.1:12000");
    }

    #[test]
    fn a_file_that_does_not_parse_is_named_in_the_error() {
        let error = parse::<SubscriberConfigs>("subscribers: 3", "somewhere.yml").unwrap_err();
        assert!(error.to_string().starts_with("somewhere.yml: "), "{error}");
    }

    #[test]
    fn a_missing_file_is_named_in_the_error() {
        let error = read::<SubscriberConfigs>(Path::new("/nonexistent/subscribers.yml"));
        assert!(matches!(error, Err(ConfigError::Read { .. })));
    }
}
