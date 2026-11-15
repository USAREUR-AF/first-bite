//! The keys a goat publisher writes. Every one starts with the prefix the
//! certificate is granted, or with `attest/schema/` followed by it.

use super::config::GoatConfig;
use super::GoatError;
use crate::sink::zenoh::{render_key, KeyTemplate};
use serde::Deserialize;
use serde_json::json;

/// The fields that say whether a record was decoded.
#[derive(Deserialize)]
struct Decoded<'a> {
    #[serde(borrow, default)]
    subsystem: Option<std::borrow::Cow<'a, str>>,
    #[serde(borrow, default)]
    message_type: Option<std::borrow::Cow<'a, str>>,
}

/// Where each kind of sample goes, for one configuration.
#[derive(Debug, Clone)]
pub struct KeyLayout {
    prefix: String,
    format: String,
    site: String,
    instance: String,
    record: KeyTemplate,
}

impl KeyLayout {
    pub fn new(config: &GoatConfig) -> Result<Self, GoatError> {
        config.check_shape()?;
        let prefix = config.principal()?.prefix().to_string();
        let publish = &config.publish;
        let version = publish
            .version
            .map(|n| format!("/v{n}"))
            .unwrap_or_default();
        // The interface and message chunks are rendered by the generic Zenoh
        // sink's template, so they are sanitised exactly as it does.
        let template = format!(
            "{prefix}/telemetry/{}/{}/{{interface}}/{{message}}{version}",
            publish.format, publish.site
        );
        let record = KeyTemplate::parse(&template)
            .map_err(|e| GoatError::Zenoh(format!("key template {template}: {e}")))?;
        Ok(KeyLayout {
            prefix,
            format: publish.format.clone(),
            site: publish.site.clone(),
            instance: publish.instance.clone(),
            record,
        })
    }

    /// The key for a record given as JSON, or `None` for a record the
    /// dictionary did not decode: those are not published on the fabric.
    pub fn record_key_of(&self, record: &str) -> Result<Option<String>, serde_json::Error> {
        let decoded: Decoded = serde_json::from_str(record)?;
        let present = |value: &Option<std::borrow::Cow<str>>| {
            value.as_deref().is_some_and(|value| !value.is_empty())
        };
        if !present(&decoded.subsystem) || !present(&decoded.message_type) {
            return Ok(None);
        }
        render_key(&self.record, record).map(Some)
    }

    /// The key records of one dictionary message are put on.
    pub fn record_key(&self, interface: &str, message: &str) -> String {
        let fields = json!({"subsystem": interface, "message_type": message}).to_string();
        render_key(&self.record, &fields).expect("the fields were serialised a line above")
    }

    /// Where the schema of that message's records is declared.
    pub fn schema_key(&self, interface: &str, message: &str) -> String {
        format!("attest/schema/{}", self.record_key(interface, message))
    }

    /// The service announcement, put once at startup.
    pub fn announce_key(&self) -> String {
        format!(
            "{}/services/shims/{}/{}/announce",
            self.prefix, self.format, self.instance
        )
    }

    /// The heartbeat.
    pub fn health_key(&self) -> String {
        format!(
            "{}/health/shims/{}/{}",
            self.prefix, self.format, self.instance
        )
    }

    /// Every record key this configuration can produce.
    pub fn records_pattern(&self) -> String {
        format!("{}/telemetry/{}/{}/**", self.prefix, self.format, self.site)
    }

    /// Everything this publisher writes outside `attest/schema/`.
    pub fn own_pattern(&self) -> String {
        format!("{}/**", self.prefix)
    }

    /// Every schema declaration of this publisher.
    pub fn schemas_pattern(&self) -> String {
        format!("attest/schema/{}/**", self.prefix)
    }

    /// The `put` grants the router must hold for this certificate.
    pub fn write_grants(&self) -> [String; 2] {
        [self.own_pattern(), "attest/schema/**".to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::super::config::tests::{slot_config, SLOT};
    use super::*;
    use std::path::Path;

    fn layout() -> KeyLayout {
        KeyLayout::new(&slot_config(Path::new("/x"))).unwrap()
    }

    #[test]
    fn every_key_sits_under_the_granted_prefix() {
        let layout = layout();
        assert_eq!(
            layout.record_key("WX", "Observation"),
            format!("{SLOT}/telemetry/weather-station/hilltop-7/wx/observation")
        );
        assert_eq!(
            layout.schema_key("WX", "Observation"),
            format!("attest/schema/{SLOT}/telemetry/weather-station/hilltop-7/wx/observation")
        );
        assert_eq!(
            layout.announce_key(),
            format!("{SLOT}/services/shims/weather-station/tap-1/announce")
        );
        assert_eq!(
            layout.health_key(),
            format!("{SLOT}/health/shims/weather-station/tap-1")
        );
        assert_eq!(
            layout.records_pattern(),
            format!("{SLOT}/telemetry/weather-station/hilltop-7/**")
        );
        assert_eq!(
            layout.write_grants(),
            [format!("{SLOT}/**"), "attest/schema/**".to_string()]
        );
        for key in [
            layout.record_key("WX", "Observation"),
            layout.announce_key(),
            layout.health_key(),
            layout.schema_key("WX", "Observation"),
        ] {
            assert!(
                zenoh::key_expr::KeyExpr::try_from(key.as_str()).is_ok(),
                "{key}"
            );
        }
    }

    #[test]
    fn a_version_adds_a_tail_to_record_and_schema_keys() {
        let mut config = slot_config(Path::new("/x"));
        config.publish.version = Some(2);
        let layout = KeyLayout::new(&config).unwrap();
        assert!(layout
            .record_key("WX", "Observation")
            .ends_with("/wx/observation/v2"));
        assert!(layout
            .schema_key("WX", "Observation")
            .ends_with("/wx/observation/v2"));
        assert!(layout.health_key().ends_with("/tap-1"));
    }

    #[test]
    fn an_org_writes_under_its_prefix() {
        let mut config = slot_config(Path::new("/x"));
        config.identity.slot = None;
        config.identity.org = Some("example-vendor".to_string());
        let layout = KeyLayout::new(&config).unwrap();
        assert_eq!(
            layout.record_key("WX", "Heartbeat"),
            "example-vendor/telemetry/weather-station/hilltop-7/wx/heartbeat"
        );
    }

    #[test]
    fn dictionary_names_cannot_add_a_level_or_a_wildcard() {
        let key = layout().record_key("a/b", "**");
        assert_eq!(
            key,
            format!("{SLOT}/telemetry/weather-station/hilltop-7/a_b/__")
        );
        assert_eq!(key.split('/').count(), 6);
    }

    #[test]
    fn records_key_from_their_own_fields_and_undecoded_ones_have_no_key() {
        let layout = layout();
        let decoded = r#"{"subsystem":"WX","message_type":"Observation","message":{"a":"1"}}"#;
        assert_eq!(
            layout.record_key_of(decoded).unwrap(),
            Some(layout.record_key("WX", "Observation"))
        );
        for undecoded in [
            r#"{"subsystem":null,"message_type":null}"#,
            r#"{"subsystem":"WX","message_type":null}"#,
            r#"{"src_port":"1"}"#,
            r#"{"subsystem":"WX","message_type":""}"#,
        ] {
            assert_eq!(
                layout.record_key_of(undecoded).unwrap(),
                None,
                "{undecoded}"
            );
        }
        assert!(layout.record_key_of("not json").is_err());
    }
}
