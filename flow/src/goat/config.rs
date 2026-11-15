//! The goat configuration file: which goatnet, which identity, what to call
//! the data. It is read only when a subscriber of type `goat` is configured
//! or a `mind goat` command runs, from the path in `GOAT_CONFIG_PATH`.

use super::identity::{check_alias, check_org_prefix, check_slot_uuid, parse_peer_name, Principal};
use super::GoatError;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::{env, fs};

pub const GOAT_CONFIG_ENV: &str = "GOAT_CONFIG_PATH";

fn yes() -> bool {
    true
}

fn default_cn_suffix() -> String {
    "-trial-peer".to_string()
}

fn default_health_interval() -> u64 {
    30
}

/// The whole file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoatConfig {
    pub goatnet: Goatnet,
    pub identity: Identity,
    pub publish: Publish,
}

/// The network to join.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Goatnet {
    /// The goatnet's name; used in messages only.
    pub name: Option<String>,
    /// Router endpoints, `tls/<host>:<port>`.
    pub endpoints: Vec<String>,
    /// Check the router certificate against the endpoint's host name.
    #[serde(default = "yes")]
    pub verify_name_on_connect: bool,
}

/// Who this publisher is on the fabric. The three paths are references to
/// files; a relative path is taken from the directory of the configuration
/// file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub certificate: PathBuf,
    pub private_key: PathBuf,
    pub ca_certificate: PathBuf,
    /// Slot UUID, on a slot-pool goatnet. Exactly one of `slot` and `org`.
    pub slot: Option<String>,
    /// Org prefix, on a per-org goatnet.
    pub org: Option<String>,
    /// What a per-org goatnet appends to the prefix to form the common name.
    #[serde(default = "default_cn_suffix")]
    pub cn_suffix: String,
    /// The slot's human name. Shown by `mind goat check`; never in a key.
    pub alias: Option<String>,
    /// The host's name on the goatnet's mesh. Checked, never transmitted.
    pub mesh_peer_name: Option<String>,
    /// Use the slot UUID as the Zenoh session identifier.
    #[serde(default = "yes")]
    pub pin_session_id: bool,
}

/// The chunks of the keys.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publish {
    /// Name of the message set, the `<format>` chunk.
    pub format: String,
    /// Where the source is, the `<site>` chunk.
    pub site: String,
    /// This process among others publishing the same format.
    pub instance: String,
    /// Appends `/v<N>` to record and schema keys.
    pub version: Option<u32>,
    #[serde(default = "default_health_interval")]
    pub health_interval_seconds: u64,
    /// At startup, put one sample and confirm the router delivers it.
    #[serde(default = "yes")]
    pub self_check: bool,
}

impl GoatConfig {
    /// Read the file named by `GOAT_CONFIG_PATH`.
    pub fn from_env() -> Result<Self, GoatError> {
        let path = env::var_os(GOAT_CONFIG_ENV)
            .filter(|value| !value.is_empty())
            .ok_or(GoatError::NoConfig)?;
        Self::load(Path::new(&path))
    }

    /// Read `path`. Relative certificate paths become relative to its
    /// directory.
    pub fn load(path: &Path) -> Result<Self, GoatError> {
        let yaml = fs::read_to_string(path).map_err(|source| GoatError::File {
            what: "the goat configuration",
            path: path.to_path_buf(),
            source,
        })?;
        let base = path.parent().unwrap_or(Path::new("."));
        Self::parse(&yaml, &path.display().to_string(), base)
    }

    /// Parse the text of a configuration; `origin` names it in an error.
    pub fn parse(yaml: &str, origin: &str, base: &Path) -> Result<Self, GoatError> {
        let mut config: GoatConfig =
            serde_norway::from_str(yaml).map_err(|source| GoatError::Parse {
                origin: origin.to_string(),
                source,
            })?;
        for path in [
            &mut config.identity.certificate,
            &mut config.identity.private_key,
            &mut config.identity.ca_certificate,
        ] {
            if path.is_relative() {
                *path = base.join(&*path);
            }
        }
        Ok(config)
    }

    /// The identity the configuration names, with its shape checked.
    pub fn principal(&self) -> Result<Principal, GoatError> {
        match (&self.identity.slot, &self.identity.org) {
            (Some(slot), None) => {
                check_slot_uuid(slot)?;
                Ok(Principal::Slot(slot.clone()))
            }
            (None, Some(org)) => {
                check_org_prefix(org)?;
                Ok(Principal::Org {
                    prefix: org.clone(),
                    common_name: format!("{org}{}", self.identity.cn_suffix),
                })
            }
            _ => Err(GoatError::IdentityChoice),
        }
    }

    /// Everything that can be checked without reading a certificate.
    pub fn check_shape(&self) -> Result<(), GoatError> {
        if self.goatnet.endpoints.is_empty() {
            return Err(GoatError::NoEndpoints);
        }
        for endpoint in &self.goatnet.endpoints {
            check_endpoint(endpoint)?;
        }
        self.principal()?;
        if let Some(alias) = &self.identity.alias {
            check_alias(alias)?;
        }
        if let Some(name) = &self.identity.mesh_peer_name {
            parse_peer_name(name)?;
        }
        check_chunk("publish.format", &self.publish.format)?;
        check_chunk("publish.site", &self.publish.site)?;
        check_chunk("publish.instance", &self.publish.instance)?;
        if self.publish.health_interval_seconds == 0 {
            return Err(GoatError::HealthInterval);
        }
        Ok(())
    }
}

/// `tls/<host>:<port>`: the only transport a goat router listens on for
/// publishers.
fn check_endpoint(endpoint: &str) -> Result<(), GoatError> {
    let fail = || GoatError::Endpoint(endpoint.to_string());
    let address = endpoint.strip_prefix("tls/").ok_or_else(fail)?;
    let (host, port) = address.rsplit_once(':').ok_or_else(fail)?;
    if host.is_empty() || port.parse::<u16>().is_err() || address.contains(['/', '#', '?', ' ']) {
        return Err(fail());
    }
    Ok(())
}

/// A configured key chunk must already be in the form a key carries, so the
/// key that is published is the one that was written down.
fn check_chunk(field: &'static str, value: &str) -> Result<(), GoatError> {
    let canonical = (1..=63).contains(&value.len())
        && value
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'));
    if canonical {
        return Ok(());
    }
    let suggestion: String = value
        .chars()
        .take(63)
        .map(|c| match c.to_ascii_lowercase() {
            c @ ('a'..='z' | '0'..='9' | '_' | '-') => c,
            _ => '_',
        })
        .collect();
    Err(GoatError::Chunk {
        field,
        value: value.to_string(),
        suggestion: if suggestion.is_empty() {
            "example".to_string()
        } else {
            suggestion
        },
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const SLOT: &str = "e7a9c75b532e3d434bb09fefd3ce1ec7";

    /// A slot-pool configuration whose certificate paths are under `base`.
    pub(crate) fn slot_config(base: &Path) -> GoatConfig {
        GoatConfig::parse(
            &format!(
                "goatnet:\n  name: zagros\n  endpoints: [\"tls/zenoh.goat.example:7447\"]\n\
                 identity:\n  certificate: {SLOT}-cert.pem\n  private_key: {SLOT}-key.pem\n  \
                 ca_certificate: ca-root.pem\n  slot: {SLOT}\n  alias: alpha-operator\n\
                 publish:\n  format: weather-station\n  site: hilltop-7\n  instance: tap-1\n"
            ),
            "test",
            base,
        )
        .unwrap()
    }

    fn with(replace: &str, by: &str) -> GoatConfig {
        let yaml = format!(
            "goatnet:\n  endpoints: [\"tls/zenoh.goat.example:7447\"]\n\
             identity:\n  certificate: /c.pem\n  private_key: /k.pem\n  ca_certificate: /ca.pem\n  \
             slot: {SLOT}\n\
             publish:\n  format: weather-station\n  site: hilltop-7\n  instance: tap-1\n"
        );
        assert!(yaml.contains(replace), "{replace}");
        GoatConfig::parse(&yaml.replace(replace, by), "test", Path::new("/")).unwrap()
    }

    fn refusal(replace: &str, by: &str) -> String {
        with(replace, by).check_shape().unwrap_err().to_string()
    }

    #[test]
    fn a_complete_configuration_passes_and_resolves_paths() {
        let config = slot_config(Path::new("/etc/first-bite/goat"));
        config.check_shape().unwrap();
        assert_eq!(
            config.identity.certificate,
            Path::new("/etc/first-bite/goat").join(format!("{SLOT}-cert.pem"))
        );
        assert_eq!(
            config.principal().unwrap(),
            Principal::Slot(SLOT.to_string())
        );
        assert!(config.goatnet.verify_name_on_connect);
        assert!(config.identity.pin_session_id);
        assert_eq!(config.publish.health_interval_seconds, 30);
    }

    #[test]
    fn an_org_identity_forms_its_common_name_from_the_suffix() {
        let config = with(&format!("slot: {SLOT}"), "org: example-vendor");
        assert_eq!(
            config.principal().unwrap().common_name(),
            "example-vendor-trial-peer"
        );
        let config = with(
            &format!("slot: {SLOT}"),
            "org: example-vendor\n  cn_suffix: \"-peer\"",
        );
        assert_eq!(
            config.principal().unwrap().common_name(),
            "example-vendor-peer"
        );
    }

    #[test]
    fn each_mistake_names_its_fix() {
        assert_eq!(
            refusal("tls/zenoh.goat.example:7447", "tcp/zenoh.goat.example:7447"),
            "goatnet.endpoints: \"tcp/zenoh.goat.example:7447\" is not a goat router endpoint; \
             write it as tls/<host>:<port>, for example tls/zenoh.goat.example:7447"
        );
        assert_eq!(
            refusal(
                &format!("slot: {SLOT}"),
                &format!("slot: {SLOT}\n  org: example-vendor")
            ),
            "set exactly one of identity.slot (the slot UUID, on a slot-pool goatnet) and \
             identity.org (the org prefix, on a per-org goatnet)"
        );
        assert_eq!(
            refusal("format: weather-station", "format: Weather Station"),
            "publish.format \"Weather Station\" cannot be a key chunk (1 to 63 characters of \
             a-z, 0-9, _ and -); write it as weather_station"
        );
        assert!(refusal(
            &format!("slot: {SLOT}"),
            &format!("slot: {SLOT}\n  alias: Alpha Operator")
        )
        .starts_with("identity.alias \"Alpha Operator\" must be 2 to 63 characters"));
        assert!(refusal(
            &format!("slot: {SLOT}"),
            &format!("slot: {SLOT}\n  mesh_peer_name: thor-a")
        )
        .contains("does not start with peer-"));
        assert!(refusal(
            "instance: tap-1",
            "instance: tap-1\n  health_interval_seconds: 0"
        )
        .contains("publish.health_interval_seconds"));
        let empty = with("[\"tls/zenoh.goat.example:7447\"]", "[]");
        assert!(matches!(empty.check_shape(), Err(GoatError::NoEndpoints)));
    }

    #[test]
    fn the_shipped_example_is_a_complete_configuration() {
        let config = GoatConfig::parse(
            include_str!("../../config/goat.example.yml"),
            "goat.example.yml",
            Path::new("/etc/first-bite/goat"),
        )
        .unwrap();
        config.check_shape().unwrap();
        assert_eq!(
            config.principal().unwrap(),
            Principal::Slot(SLOT.to_string())
        );
        assert_eq!(config.identity.alias.as_deref(), Some("alpha-operator"));
        assert_eq!(config.publish.instance, "tap-1");
    }

    #[test]
    fn unknown_keys_and_missing_files_are_named() {
        let error = GoatConfig::parse(
            "goatnet: {endpoints: []}\nextra: 1\n",
            "goat.yml",
            Path::new("/"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.starts_with("goat.yml: "), "{error}");
        let error = GoatConfig::load(Path::new("/nonexistent/goat.yml")).unwrap_err();
        assert!(error
            .to_string()
            .starts_with("cannot read the goat configuration /nonexistent/goat.yml: "));
    }
}
