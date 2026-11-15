//! The YAML configuration files and where they are found.
//!
//! Each file is looked up the same way: the path in its environment variable,
//! else `config/<name>` beside the executable. Publishers fall back to a
//! built-in default, because publishing on the local ZeroMQ socket is what the
//! application is for. Subscribers and subpubs have no default: with no file
//! there are none.

use crate::error::{ConfigError, Problem};
use crate::secret::{
    credential_path, read_credential_file, reference, Reference, Secret, CREDENTIALS_DIRECTORY_ENV,
};
use log::{info, warn};
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
    /// How each record is written on the socket: `json` (the default) or
    /// `cbor`.
    pub encoding: Option<String>,
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
    /// How the sink writes each record: `json` (the default) or `cbor`.
    pub encoding: Option<String>,
    // REST
    pub endpoint_url: Option<String>,
    pub endpoint_port: Option<String>,
    pub endpoint_path: Option<String>,
    /// Accepted for compatibility; nothing reads it.
    pub connection_string: Option<Secret>,
    // Azure Blob Storage
    pub cloud_account: Option<String>,
    pub cloud_access_key: Option<Secret>,
    /// File holding `cloud_access_key`, in place of the key itself.
    pub cloud_access_key_file: Option<String>,
    pub cloud_container: Option<String>,
    /// `Emulator`, `Custom` or `Public`.
    pub cloud_location_type: Option<String>,
    pub cloud_address: Option<String>,
    pub cloud_port: Option<String>,
    pub cloud_uri: Option<String>,
    pub cloud_api_version: Option<String>,
    // S3
    pub aws_access_key_id: Option<Secret>,
    /// File holding `aws_access_key_id`, in place of the key itself.
    pub aws_access_key_id_file: Option<String>,
    pub aws_secret_access_key: Option<Secret>,
    /// File holding `aws_secret_access_key`, in place of the key itself.
    pub aws_secret_access_key_file: Option<String>,
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
    /// Field mapping for Cursor-on-Target: the transform of a subpub, or
    /// what a `udp` or `tcp` subscriber sends in place of the record.
    pub cot: Option<cotify::Mapping>,
    // Network: `udp` and `tcp`
    /// Where each payload is sent.
    pub destinations: Option<Vec<crate::sink::network::DestinationConfig>>,
    /// TLS for a `tcp` subscriber.
    pub tls: Option<crate::sink::network::TlsConfig>,
    /// `tcp`: payloads held per destination while its connection is down.
    pub queue: Option<usize>,
    /// Where this sink differs from the security block.
    pub security: Option<crate::security::SinkOverride>,
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

/// Where references are resolved from: the process environment in a running
/// service, a fixed table in a test.
pub type Lookup<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The process environment. A variable that is set but empty counts as unset.
pub fn process_environment(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

/// Replace `value` with the variable it refers to, when it is a reference.
fn resolve_value(
    key: &'static str,
    value: &mut Option<String>,
    lookup: Lookup<'_>,
) -> Result<(), Problem> {
    let Some(text) = value.as_deref() else {
        return Ok(());
    };
    match reference(text) {
        Reference::None => Ok(()),
        Reference::Malformed => Err(Problem::MalformedReference { key }),
        Reference::Variable(variable) => match lookup(variable) {
            Some(resolved) => {
                *value = Some(resolved);
                Ok(())
            }
            None => Err(Problem::UnsetVariable {
                key,
                variable: variable.to_string(),
            }),
        },
    }
}

/// Resolve a credential key and its `_file` variant into the credential.
/// `Ok(true)` when the file holds the credential itself, as a plain value.
fn resolve_secret(
    key: &'static str,
    secret: &mut Option<Secret>,
    file: Option<&mut Option<String>>,
    lookup: Lookup<'_>,
) -> Result<bool, Problem> {
    if let Some(file) = file {
        // The path may itself be a reference.
        let mut path = file.take();
        resolve_file_path(key, &mut path, lookup)?;
        if let Some(path) = path {
            if secret.is_some() {
                return Err(Problem::ValueAndFile { key });
            }
            let directory = lookup(CREDENTIALS_DIRECTORY_ENV);
            let path = credential_path(&path, directory.as_deref());
            let read = read_credential_file(&path).map_err(|reason| Problem::CredentialFile {
                key,
                path: path.clone(),
                reason,
            })?;
            *secret = Some(read);
            return Ok(false);
        }
    }
    let Some(value) = secret.as_ref() else {
        return Ok(false);
    };
    match reference(value.expose()) {
        Reference::None => Ok(!value.expose().is_empty()),
        Reference::Malformed => Err(Problem::MalformedReference { key }),
        Reference::Variable(variable) => match lookup(variable) {
            Some(resolved) => {
                *secret = Some(Secret::new(resolved));
                Ok(false)
            }
            None => Err(Problem::UnsetVariable {
                key,
                variable: variable.to_string(),
            }),
        },
    }
}

/// A reference in a `_file` path is reported under the `_file` key's name.
fn resolve_file_path(
    key: &'static str,
    path: &mut Option<String>,
    lookup: Lookup<'_>,
) -> Result<(), Problem> {
    resolve_value(key, path, lookup).map_err(|problem| {
        let reason = match problem {
            Problem::UnsetVariable { variable, .. } => {
                format!("refers to the environment variable {variable}, which is not set")
            }
            _ => "contains `${` but is not one whole reference".to_string(),
        };
        Problem::InvalidValue {
            key,
            reason: format!("the path in its `_file` key {reason}"),
        }
    })
}

impl SubscriberConfig {
    /// Replace every reference with the value it names, and read every
    /// `_file` credential. `name`, the types, `topics` and `cot` are taken
    /// as written.
    ///
    /// Returns the credential keys whose value is written in the file itself,
    /// for the caller to warn about. An unset variable, a malformed
    /// reference, an unreadable credential file, or a key given both ways is
    /// an error naming the key (and never a value).
    pub fn resolve(&mut self, lookup: Lookup<'_>) -> Result<Vec<&'static str>, Problem> {
        for (key, value) in [
            ("zmq_url", &mut self.zmq_url),
            ("zmq_port", &mut self.zmq_port),
            ("endpoint_url", &mut self.endpoint_url),
            ("endpoint_port", &mut self.endpoint_port),
            ("endpoint_path", &mut self.endpoint_path),
            ("cloud_account", &mut self.cloud_account),
            ("cloud_container", &mut self.cloud_container),
            ("cloud_location_type", &mut self.cloud_location_type),
            ("cloud_address", &mut self.cloud_address),
            ("cloud_port", &mut self.cloud_port),
            ("cloud_uri", &mut self.cloud_uri),
            ("cloud_api_version", &mut self.cloud_api_version),
            ("aws_default_region", &mut self.aws_default_region),
            ("aws_s3_endpoint", &mut self.aws_s3_endpoint),
            ("aws_bucket_name", &mut self.aws_bucket_name),
            ("local_logfile", &mut self.local_logfile),
            ("zenoh_config", &mut self.zenoh_config),
            ("key_template", &mut self.key_template),
        ] {
            resolve_value(key, value, lookup)?;
        }
        let mut plain = Vec::new();
        for (key, secret, file) in [
            (
                "cloud_access_key",
                &mut self.cloud_access_key,
                Some(&mut self.cloud_access_key_file),
            ),
            (
                "aws_access_key_id",
                &mut self.aws_access_key_id,
                Some(&mut self.aws_access_key_id_file),
            ),
            (
                "aws_secret_access_key",
                &mut self.aws_secret_access_key,
                Some(&mut self.aws_secret_access_key_file),
            ),
            ("connection_string", &mut self.connection_string, None),
        ] {
            if resolve_secret(key, secret, file, lookup)? {
                plain.push(key);
            }
        }
        Ok(plain)
    }
}

impl PublisherConfig {
    /// Replace `address` with the variable it refers to, when it is a
    /// reference.
    pub fn resolve(&mut self, lookup: Lookup<'_>) -> Result<(), Problem> {
        let mut address = Some(std::mem::take(&mut self.address));
        let resolved = resolve_value("address", &mut address, lookup);
        self.address = address.unwrap_or_default();
        resolved
    }
}

/// One warning per credential written in a configuration file as a plain
/// value. It names the entry and the key, never the value.
fn warn_about_plain_credentials(entry: &str, keys: &[&'static str]) {
    for key in keys {
        warn!(
            "{entry}: `{key}` is written in the configuration file as a plain value; write \
             `{key}: ${{NAME}}` and set NAME in the service environment, or use `{key}_file`"
        );
    }
}

/// The configured publishers, or the built-in default when no file is given.
pub fn load_publisher_configs() -> Result<Vec<PublisherConfig>, ConfigError> {
    let mut configs = read_publisher_configs()?;
    for config in &mut configs {
        config
            .resolve(&process_environment)
            .map_err(|problem| ConfigError::Publisher {
                publisher: config.name.clone(),
                problem,
            })?;
    }
    Ok(configs)
}

fn read_publisher_configs() -> Result<Vec<PublisherConfig>, ConfigError> {
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

/// The configured subscribers, with references resolved from the process
/// environment; none when no file is given.
pub fn load_subscriber_configs() -> Result<Vec<SubscriberConfig>, ConfigError> {
    let mut configs = read_subscriber_configs()?;
    for config in &mut configs {
        let plain =
            config
                .resolve(&process_environment)
                .map_err(|problem| ConfigError::Subscriber {
                    subscriber: config.name.clone(),
                    problem,
                })?;
        warn_about_plain_credentials(&format!("subscriber {}", config.name), &plain);
    }
    Ok(configs)
}

fn read_subscriber_configs() -> Result<Vec<SubscriberConfig>, ConfigError> {
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

/// The configured subpubs, with references resolved from the process
/// environment; none when no file is given.
pub fn load_subpub_configs() -> Result<Vec<SubPubConfig>, ConfigError> {
    let mut configs = read_subpub_configs()?;
    for config in &mut configs {
        let in_subpub = |problem| ConfigError::SubPub {
            subpub: config.name.clone(),
            problem,
        };
        let plain = config
            .subscriber
            .resolve(&process_environment)
            .map_err(in_subpub)?;
        config
            .publisher
            .resolve(&process_environment)
            .map_err(in_subpub)?;
        warn_about_plain_credentials(&format!("subpub {}", config.name), &plain);
    }
    Ok(configs)
}

fn read_subpub_configs() -> Result<Vec<SubPubConfig>, ConfigError> {
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
pub(crate) fn find(env_var: &str, default_file: &str) -> Option<PathBuf> {
    if let Some(path) = env::var_os(env_var).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let beside_executable = env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(default_file)));
    beside_executable.filter(|path| path.is_file())
}

pub(crate) fn read<T: DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
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

    fn subscriber(body: &str) -> SubscriberConfig {
        crate::test_support::subscriber(body)
    }

    /// A lookup over a fixed table, so no test changes the environment.
    fn table(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        }
    }

    const S3: &str = "    subscriber_type: objectstorage\n    subtype: awsobject\n    aws_default_region: us-east-1\n    aws_bucket_name: records\n";

    #[test]
    fn references_are_resolved_from_the_environment() {
        let mut config = subscriber(&format!(
            "{S3}    aws_access_key_id: \"${{S3_ID}}\"\n    aws_secret_access_key: ${{S3_SECRET}}\n    aws_s3_endpoint: \"${{S3_ENDPOINT}}\"\n"
        ));
        let plain = config
            .resolve(&table(&[
                ("S3_ID", "resolved-id"),
                ("S3_SECRET", "resolved-secret"),
                ("S3_ENDPOINT", "https://s3.example"),
            ]))
            .unwrap();
        assert!(plain.is_empty(), "{plain:?}");
        assert_eq!(config.aws_access_key_id.unwrap().expose(), "resolved-id");
        assert_eq!(
            config.aws_secret_access_key.unwrap().expose(),
            "resolved-secret"
        );
        assert_eq!(
            config.aws_s3_endpoint.as_deref(),
            Some("https://s3.example")
        );
        // Keys with no reference are as written.
        assert_eq!(config.aws_bucket_name.as_deref(), Some("records"));
    }

    #[test]
    fn an_unset_variable_names_the_subscriber_the_key_and_the_variable() {
        let mut config = subscriber(&format!(
            "{S3}    aws_access_key_id: id\n    aws_secret_access_key: \"${{S3_SECRET}}\"\n"
        ));
        let problem = config.resolve(&table(&[])).unwrap_err();
        let error = ConfigError::Subscriber {
            subscriber: config.name.clone(),
            problem,
        };
        assert_eq!(
            error.to_string(),
            "subscriber Example: `aws_secret_access_key` refers to the environment variable \
             S3_SECRET, which is not set"
        );
    }

    #[test]
    fn a_partial_reference_is_refused_and_not_used_as_a_value() {
        for value in ["${A}${B}", "key-${A}", "${a-b}", "${}"] {
            let mut config = subscriber(&format!("{S3}    aws_secret_access_key: \"{value}\"\n"));
            let problem = config
                .resolve(&table(&[("A", "a"), ("B", "b")]))
                .unwrap_err();
            assert!(
                matches!(problem, Problem::MalformedReference { key } if key == "aws_secret_access_key"),
                "{value}: {problem}"
            );
        }
    }

    #[test]
    fn a_plain_credential_still_works_and_is_reported_by_key() {
        let mut config = subscriber(&format!(
            "{S3}    aws_access_key_id: plain-id\n    aws_secret_access_key: plain-secret\n    connection_string: \"${{CONNECTION}}\"\n"
        ));
        let plain = config
            .resolve(&table(&[("CONNECTION", "from-the-environment")]))
            .unwrap();
        assert_eq!(plain, ["aws_access_key_id", "aws_secret_access_key"]);
        assert_eq!(
            config.aws_secret_access_key.unwrap().expose(),
            "plain-secret"
        );
    }

    #[test]
    fn a_credential_can_be_read_from_a_file() {
        let directory = crate::test_support::temp_path("credentials");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("blob-key"), "a2V5\n").unwrap();
        let absolute = directory.join("blob-key").display().to_string();
        let directory_text: &'static str =
            Box::leak(directory.display().to_string().into_boxed_str());

        // An absolute path, and a relative one under systemd's credentials
        // directory.
        for (path, lookup) in [
            (absolute.as_str(), table(&[])),
            (
                "blob-key",
                table(Box::leak(Box::new([(
                    "CREDENTIALS_DIRECTORY",
                    directory_text,
                )]))),
            ),
        ] {
            let mut config = subscriber(&format!(
                "    subscriber_type: objectstorage\n    cloud_access_key_file: \"{path}\"\n"
            ));
            let plain = config.resolve(&lookup).unwrap();
            assert!(plain.is_empty());
            assert_eq!(config.cloud_access_key.unwrap().expose(), "a2V5");
        }

        let mut both = subscriber(&format!(
            "    subscriber_type: objectstorage\n    cloud_access_key: a2V5\n    cloud_access_key_file: \"{absolute}\"\n"
        ));
        assert_eq!(
            both.resolve(&table(&[])).unwrap_err().to_string(),
            "`cloud_access_key` and `cloud_access_key_file` are both set; give one"
        );

        let mut missing = subscriber(
            "    subscriber_type: objectstorage\n    cloud_access_key_file: /nonexistent/blob-key\n",
        );
        let error = missing.resolve(&table(&[])).unwrap_err().to_string();
        assert!(
            error.starts_with("`cloud_access_key_file`: cannot read /nonexistent/blob-key: "),
            "{error}"
        );
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_configuration_printed_at_any_level_shows_no_credential() {
        let mut config = subscriber(&format!(
            "{S3}    aws_access_key_id: AKIDPLAINVALUE\n    aws_secret_access_key: \"${{S3_SECRET}}\"\n    cloud_access_key: Zm9ydHkgdHdv\n    connection_string: \"AccountKey=Zm9ydHkgdHdv\"\n"
        ));
        config
            .resolve(&table(&[("S3_SECRET", "resolved-secret-value")]))
            .unwrap();
        let subpub = SubPubConfig {
            name: "pair".to_string(),
            subscriber: config.clone(),
            publisher: parse::<PublisherConfigs>(DEFAULT_PUBLISHERS, "built-in")
                .unwrap()
                .publishers
                .remove(0),
        };
        for printed in [
            format!("{config:?}"),
            format!("{config:#?}"),
            format!("{subpub:?}"),
            format!(
                "{:?}",
                SubscriberConfigs {
                    subscribers: vec![config.clone()]
                }
            ),
        ] {
            for credential in [
                "AKIDPLAINVALUE",
                "resolved-secret-value",
                "Zm9ydHkgdHdv",
                "AccountKey",
            ] {
                assert!(!printed.contains(credential), "{credential} in {printed}");
            }
            assert!(
                printed.contains("aws_secret_access_key: Some(***)") || printed.contains("***,"),
                "{printed}"
            );
        }
    }

    #[test]
    fn a_publisher_address_can_be_a_reference() {
        let mut config = parse::<PublisherConfigs>(
            "publishers:\n  - { name: records, publisher_type: PUB, address: \"${RECORDS_ADDRESS}\", topic: \"*\" }\n",
            "test",
        )
        .unwrap()
        .publishers
        .remove(0);
        assert_eq!(
            config.resolve(&table(&[])).unwrap_err().to_string(),
            "`address` refers to the environment variable RECORDS_ADDRESS, which is not set"
        );
        config
            .resolve(&table(&[("RECORDS_ADDRESS", "tcp://127.0.0.1:12000")]))
            .unwrap();
        assert_eq!(config.address, "tcp://127.0.0.1:12000");
    }

    #[test]
    fn a_missing_file_is_named_in_the_error() {
        let error = read::<SubscriberConfigs>(Path::new("/nonexistent/subscribers.yml"));
        assert!(matches!(error, Err(ConfigError::Read { .. })));
    }
}
