//! The `xf` transform: convert the payload of a record to another format
//! with the [goat-transformers](https://github.com/USAREUR-AF/goat-transformers)
//! engine.
//!
//! The engine reads a format from a data file, a pack. `tools/bite2pack`
//! writes a pack for each interface of a dictionary; the target formats
//! (Cursor-on-Target XML, GeoJSON) have packs of their own. A conversion
//! decodes the payload with the dictionary's pack and encodes what it holds
//! with the target's.
//!
//! The configuration ([`XfConfig`]) is always parsed. The transform itself is
//! compiled with the cargo feature `goat-transformers`; without it a
//! configuration that asks for the transform is refused at startup.

use serde::Deserialize;
use std::collections::BTreeMap;

/// The `xf:` block of a subscriber. In YAML:
///
/// ```yaml
/// xf:
///   to: cot-xml            # the format to write
///   context:               # values the payload does not carry
///     type: "a-f-G"
///     how: "m-g"
///   stale_minutes: 60
///   require: [uid, lat, lon]   # a record that gives none of these is not converted
///   # from: bite-weather-station-wx   # default: by the record's dictionary and interface
///   # packs: /etc/first-bite/packs    # packs of your own dictionaries
/// ```
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XfConfig {
    /// The format every record is converted to.
    pub to: String,
    /// The format of the payload. Unset, it is the pack generated for the
    /// record's `dictionary` and `subsystem`.
    pub from: Option<String>,
    /// Shared-model keys the payload does not carry, such as the
    /// Cursor-on-Target `type` and `how`. A value the payload does carry is
    /// kept.
    #[serde(default)]
    pub context: BTreeMap<String, Scalar>,
    /// Minutes after its creation at which the output goes stale; 60 when
    /// unset. Supplied to the conversion as `stale_ms`.
    pub stale_minutes: Option<u32>,
    /// Shared-model keys a record must give, from its payload or from the
    /// context. A record that lacks one is not converted, as the `cot`
    /// transform refuses a record without a field its mapping names.
    #[serde(default)]
    pub require: Vec<String>,
    /// A directory of further packs, as `tools/bite2pack` writes one:
    /// `model/`, `packs/` and `index.json`. Loaded after the packs built in.
    pub packs: Option<String>,
}

/// A context value as written in the file: text or a number.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Scalar {
    Integer(i64),
    Number(f64),
    Text(String),
}

impl std::fmt::Display for Scalar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Scalar::Integer(value) => write!(f, "{value}"),
            Scalar::Number(value) => write!(f, "{value}"),
            Scalar::Text(value) => f.write_str(value),
        }
    }
}

#[cfg(feature = "goat-transformers")]
pub use engine::{Converted, Xf, XfError, COT_XML};
/// The engine's crate, for a caller that reads a pack or an entity.
#[cfg(feature = "goat-transformers")]
pub use goat_transformers;

#[cfg(feature = "goat-transformers")]
mod engine {
    use super::XfConfig;
    use crate::error::Problem;
    use crate::xf_embedded;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    use chrono::{DateTime, Utc};
    use goat_transformers::{Engine, Value};
    use serde::{Deserialize, Serialize};
    use std::path::Path;
    use std::sync::Arc;

    /// The target format whose output is published as the `cot` transform
    /// publishes it.
    pub const COT_XML: &str = "cot-xml";

    /// Minutes an output stays valid when the configuration does not say:
    /// the default of the `cot` transform.
    const DEFAULT_STALE_MINUTES: u32 = cotify::DEFAULT_STALE_MINUTES;

    /// Why one record was not converted.
    #[derive(Debug, thiserror::Error)]
    pub enum XfError {
        #[error("record is not valid JSON: {0}")]
        Record(#[from] serde_json::Error),
        #[error("record carries no payload (`data_encoded_inner`)")]
        NoPayload,
        #[error("the payload is not base64: {0}")]
        Payload(#[from] base64::DecodeError),
        #[error(
            "no pack for dictionary {} interface {}; generate one with tools/bite2pack and name \
             its directory in `xf.packs`, or set `xf.from`",
            name(.dictionary), name(.interface)
        )]
        NoFormat {
            dictionary: Option<String>,
            interface: Option<String>,
        },
        #[error(
            "format {format} identifies a message by its frame identifier, which a record does \
             not carry"
        )]
        FrameId { format: String },
        #[error("the payload gives no `{key}`, which `xf.require` names")]
        Missing { key: String },
        #[error("{0}")]
        Convert(String),
    }

    fn name(value: &Option<String>) -> String {
        match value {
            Some(value) => format!("{value:?}"),
            None => "(not in the record)".to_string(),
        }
    }

    /// One line of `index.json`: the pack `tools/bite2pack` wrote for an
    /// interface of a dictionary.
    #[derive(Debug, Clone, Deserialize)]
    struct Generated {
        format: String,
        dictionary: String,
        interface: String,
        frame_id_bytes: usize,
    }

    /// The output of one conversion.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Converted {
        /// The time the output was created, as the output states it.
        pub datetime: String,
        /// The target format's text: XML, JSON, or hexadecimal for a binary
        /// format.
        pub text: String,
    }

    /// What is published for a Cursor-on-Target target: the object the `cot`
    /// transform publishes.
    #[derive(Serialize)]
    struct CotEnvelope<'a> {
        cot_datetime: &'a str,
        cot_xml_string: &'a str,
    }

    /// What is published for any other target.
    #[derive(Serialize)]
    struct Envelope<'a> {
        xf_datetime: &'a str,
        xf_format: &'a str,
        xf_string: &'a str,
    }

    /// A configured conversion: the packs, the target and the context.
    #[derive(Clone)]
    pub struct Xf {
        engine: Arc<Engine>,
        generated: Arc<Vec<Generated>>,
        to: String,
        from: Option<String>,
        context: Vec<(String, Value)>,
        require: Vec<String>,
        stale_minutes: u32,
    }

    impl std::fmt::Debug for Xf {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Xf")
                .field("to", &self.to)
                .field("from", &self.from)
                .field("context", &self.context)
                .field("require", &self.require)
                .field("stale_minutes", &self.stale_minutes)
                .finish_non_exhaustive()
        }
    }

    fn invalid(reason: impl std::fmt::Display) -> Problem {
        Problem::InvalidValue {
            key: "xf",
            reason: reason.to_string(),
        }
    }

    fn read(path: &Path) -> Result<String, Problem> {
        std::fs::read_to_string(path)
            .map_err(|e| invalid(format!("`packs`: cannot read {}: {e}", path.display())))
    }

    /// The `*.def` files of `dir`, or the `pack.def` of each of its
    /// directories, in name order.
    fn entries(dir: &Path) -> Result<Vec<std::path::PathBuf>, Problem> {
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| invalid(format!("`packs`: cannot read {}: {e}", dir.display())))?
            .flatten()
            .map(|entry| entry.path())
            .collect();
        paths.sort();
        Ok(paths)
    }

    /// The packs built in, then those of the `packs` directory.
    fn load(packs: Option<&str>) -> Result<(Engine, Vec<Generated>), Problem> {
        let mut engine = Engine::new();
        let mut generated: Vec<Generated> =
            serde_json::from_str(xf_embedded::INDEX).map_err(invalid)?;
        for text in xf_embedded::MODELS {
            engine.load_model(text).map_err(invalid)?;
        }
        for text in xf_embedded::PACKS {
            engine.load_def(text).map_err(invalid)?;
        }
        let Some(dir) = packs.map(Path::new) else {
            return Ok((engine, generated));
        };
        let from = |path: &Path, e: goat_transformers::Error| {
            invalid(format!("`packs`: {}: {e}", path.display()))
        };
        for path in entries(&dir.join("model"))? {
            // The shared keys are the ones built in.
            let is_module = path.extension().is_some_and(|ext| ext == "def");
            if is_module && path.file_name().is_some_and(|name| name != "core.def") {
                engine
                    .load_model(&read(&path)?)
                    .map_err(|e| from(&path, e))?;
            }
        }
        for path in entries(&dir.join("packs"))? {
            let def = path.join("pack.def");
            if def.is_file() {
                engine.load_def(&read(&def)?).map_err(|e| from(&def, e))?;
            }
        }
        let index = dir.join("index.json");
        let more: Vec<Generated> = serde_json::from_str(&read(&index)?)
            .map_err(|e| invalid(format!("`packs`: {}: {e}", index.display())))?;
        // A pack of the directory is found before one built in.
        generated.splice(0..0, more);
        Ok((engine, generated))
    }

    impl Xf {
        /// Load the packs and check the configuration against them: the
        /// formats exist and every context key is one a pack can hold.
        pub fn from_config(config: &XfConfig) -> Result<Self, Problem> {
            let (engine, generated) = load(config.packs.as_deref())?;
            let formats = engine.formats();
            for (key, format) in [("to", Some(&config.to)), ("from", config.from.as_ref())] {
                if format.is_some_and(|format| !formats.contains(format)) {
                    return Err(invalid(format!(
                        "`{key}`: no pack for format {:?}; the formats are {}",
                        format.unwrap(),
                        formats.join(", ")
                    )));
                }
            }
            let pairs: Vec<(String, String)> = config
                .context
                .iter()
                .map(|(key, value)| (key.clone(), value.to_string()))
                .collect();
            let context = engine
                .context_from_text(&pairs)
                .map_err(|e| invalid(format!("`context`: {e}")))?;
            // A required key is checked as a context key is: a pack declares it.
            let required: Vec<(String, String)> = config
                .require
                .iter()
                .filter(|key| !config.context.contains_key(*key))
                .map(|key| (key.clone(), "0".to_string()))
                .collect();
            if let Err(e) = engine.context_from_text(&required) {
                if e.msg.starts_with("undeclared-key") {
                    return Err(invalid(format!("`require`: {e}")));
                }
            }
            Ok(Xf {
                require: config.require.clone(),
                engine: Arc::new(engine),
                generated: Arc::new(generated),
                to: config.to.clone(),
                from: config.from.clone(),
                context,
                stale_minutes: config.stale_minutes.unwrap_or(DEFAULT_STALE_MINUTES),
            })
        }

        /// The format the output is written in.
        pub fn target(&self) -> &str {
            &self.to
        }

        /// The engine and its packs, for a caller that decodes without
        /// converting.
        pub fn engine(&self) -> &Engine {
            &self.engine
        }

        /// The pack of a record: `xf.from`, or the one generated for its
        /// dictionary and interface. A record of data model version 1 does
        /// not name its dictionary; the interface alone then has to be
        /// enough.
        fn format(&self, record: &serde_json::Value) -> Result<&str, XfError> {
            if let Some(from) = &self.from {
                return Ok(from);
            }
            let text = |key: &str| record.get(key).and_then(serde_json::Value::as_str);
            let (dictionary, interface) = (text("dictionary"), text("subsystem"));
            let found: Vec<&Generated> = self
                .generated
                .iter()
                .filter(|generated| {
                    Some(generated.interface.as_str()) == interface
                        && dictionary.is_none_or(|dictionary| generated.dictionary == dictionary)
                })
                .collect();
            match found.first() {
                Some(generated) if dictionary.is_some() || found.len() == 1 => {
                    if generated.frame_id_bytes > 0 {
                        return Err(XfError::FrameId {
                            format: generated.format.clone(),
                        });
                    }
                    Ok(&generated.format)
                }
                _ => Err(XfError::NoFormat {
                    dictionary: dictionary.map(str::to_string),
                    interface: interface.map(str::to_string),
                }),
            }
        }

        /// Convert the payload of one published record, with the output
        /// created at `now`.
        ///
        /// The conversion is given `time_ms` (now) and `stale_ms` (now plus
        /// `stale_minutes`) after the configured context, so a configured
        /// value of either wins, and a value the payload carries wins over
        /// both.
        pub fn convert_at(&self, record: &str, now: DateTime<Utc>) -> Result<Converted, XfError> {
            let record: serde_json::Value = serde_json::from_str(record)?;
            let payload = record
                .get("data_encoded_inner")
                .and_then(serde_json::Value::as_str)
                .ok_or(XfError::NoPayload)?;
            let payload = hex::encode(STANDARD.decode(payload)?);
            let from = self.format(&record)?;
            let time_ms = now.timestamp_millis();
            let mut context = self.context.clone();
            context.push(("time_ms".to_string(), Value::I64(time_ms)));
            context.push((
                "stale_ms".to_string(),
                Value::I64(time_ms + i64::from(self.stale_minutes) * 60_000),
            ));
            // The steps of the engine's `convert`, with the required keys
            // checked between completing the entity and writing it.
            let failed = |e: goat_transformers::Error| XfError::Convert(e.msg);
            let mut entity = self.engine.decode(from, &payload).map_err(failed)?;
            if from != self.to {
                entity.kind = None;
            }
            self.engine.fill(&mut entity, &context).map_err(failed)?;
            self.engine.complete(&mut entity);
            if let Some(key) = self
                .require
                .iter()
                .find(|key| !entity.fields.contains_key(*key))
            {
                return Err(XfError::Missing { key: key.clone() });
            }
            let text = self.engine.encode(&self.to, &entity).map_err(failed)?;
            Ok(Converted {
                datetime: now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
                text,
            })
        }

        /// [`Xf::convert_at`] as the JSON object a subpub publishes. For
        /// Cursor-on-Target it is the object the `cot` transform publishes,
        /// `{"cot_datetime": ..., "cot_xml_string": ...}`; for another
        /// target, `{"xf_datetime": ..., "xf_format": ..., "xf_string": ...}`.
        pub fn apply_at(&self, record: &str, now: DateTime<Utc>) -> Result<String, XfError> {
            let converted = self.convert_at(record, now)?;
            let json = if self.to == COT_XML {
                serde_json::to_string(&CotEnvelope {
                    cot_datetime: &converted.datetime,
                    cot_xml_string: &converted.text,
                })
            } else {
                serde_json::to_string(&Envelope {
                    xf_datetime: &converted.datetime,
                    xf_format: &self.to,
                    xf_string: &converted.text,
                })
            };
            Ok(json?)
        }

        /// [`Xf::apply_at`] with the output created now.
        pub fn apply(&self, record: &str) -> Result<String, XfError> {
            self.apply_at(record, Utc::now())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use chrono::TimeZone;

        /// The Position record of `examples/weather-station.records.jsonl`,
        /// cut to the keys the transform reads.
        const POSITION: &str = r#"{"data_encoded_inner":"AwAbS1g3AEBQNZ6D5CWvwGJ27L+xW1dDCIAA","subsystem":"WX","message_type":"Position","dictionary":"weather-station"}"#;

        fn config(yaml: &str) -> XfConfig {
            serde_norway::from_str(yaml).unwrap()
        }

        fn xf(yaml: &str) -> Xf {
            Xf::from_config(&config(yaml)).unwrap()
        }

        fn refusal(yaml: &str) -> String {
            Xf::from_config(&config(yaml)).unwrap_err().to_string()
        }

        fn noon() -> DateTime<Utc> {
            Utc.with_ymd_and_hms(2026, 3, 4, 12, 0, 0).unwrap()
        }

        #[test]
        fn a_position_payload_becomes_a_cot_event() {
            let xf = xf("to: cot-xml\ncontext: { type: a-f-G, how: m-g }\nstale_minutes: 10\n");
            let json = xf.apply_at(POSITION, noon()).unwrap();
            let event: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(event["cot_datetime"], "2026-03-04T12:00:00.000Z");
            assert_eq!(
                event["cot_xml_string"],
                r#"<event version="2.0" uid="KX7" type="a-f-G" time="2026-03-04T12:00:00.000Z" start="2026-03-04T12:00:00.000Z" stale="2026-03-04T12:10:00.000Z" how="m-g"><point lat="64.8378" lon="-147.7164" hae="136.5" ce="9999999" le="9999999"/><detail/></event>"#
            );
        }

        #[test]
        fn another_target_is_published_with_its_format_named() {
            let xf = xf("to: geojson\n");
            let json = xf.apply_at(POSITION, noon()).unwrap();
            let output: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(output["xf_format"], "geojson");
            assert_eq!(output["xf_datetime"], "2026-03-04T12:00:00.000Z");
            let feature: serde_json::Value =
                serde_json::from_str(output["xf_string"].as_str().unwrap()).unwrap();
            assert_eq!(feature["id"], "KX7");
            assert_eq!(
                feature["geometry"]["coordinates"],
                serde_json::json!([-147.7164, 64.8378, 136.5])
            );
            assert_eq!(feature["properties"]["time_ms"], 1_772_625_600_000_i64);
        }

        #[test]
        fn the_defaults_are_the_cot_transforms() {
            let xf = xf("to: cot-xml\n");
            let converted = xf.convert_at(POSITION, noon()).unwrap();
            // No type in the context: the pack's "not known". Stale after an hour.
            assert!(
                converted.text.contains(r#" type="a-u-X" "#),
                "{converted:?}"
            );
            assert!(
                converted
                    .text
                    .contains(r#" stale="2026-03-04T13:00:00.000Z""#),
                "{converted:?}"
            );
        }

        #[test]
        fn a_configured_time_wins_over_the_clock() {
            let xf = xf("to: cot-xml\ncontext: { stale_ms: 1772625600000 }\n");
            let converted = xf.convert_at(POSITION, noon()).unwrap();
            assert!(
                converted
                    .text
                    .contains(r#" stale="2026-03-04T12:00:00.000Z""#),
                "{converted:?}"
            );
        }

        #[test]
        fn the_configuration_is_checked_against_the_packs() {
            assert!(
                refusal("to: morse\n").starts_with("`xf`: `to`: no pack for format \"morse\""),
                "{}",
                refusal("to: morse\n")
            );
            assert!(refusal("to: cot-xml\nfrom: morse\n").contains("`from`: no pack"));
            let undeclared = refusal("to: cot-xml\ncontext: { colour: red }\n");
            assert!(
                undeclared.contains("`context`: undeclared-key"),
                "{undeclared}"
            );
            let mistyped = refusal("to: cot-xml\ncontext: { ce: wide }\n");
            assert!(
                mistyped.contains("`context`: key-type-mismatch"),
                "{mistyped}"
            );
            assert!(serde_norway::from_str::<XfConfig>("to: cot-xml\ntoo: 1\n").is_err());
        }

        #[test]
        fn records_that_cannot_be_converted_say_why() {
            let xf = xf("to: cot-xml\n");
            let error = |record: &str| xf.apply_at(record, noon()).unwrap_err().to_string();
            assert!(error("not json").starts_with("record is not valid JSON"));
            assert_eq!(
                error(r#"{"subsystem":"WX"}"#),
                "record carries no payload (`data_encoded_inner`)"
            );
            assert!(error(r#"{"data_encoded_inner":"!"}"#).starts_with("the payload is not base64"));
            assert!(error(
                r#"{"data_encoded_inner":"AA==","subsystem":"RADAR","dictionary":"private"}"#
            )
            .starts_with("no pack for dictionary \"private\" interface \"RADAR\""));
            assert!(error(
                r#"{"data_encoded_inner":"AA==","subsystem":"POWERTRAIN","dictionary":"vehicle-can"}"#
            )
            .starts_with("format bite-vehicle-can-powertrain identifies a message by its frame"));
        }

        /// A Heartbeat has no position. Nothing required: the event is
        /// written with what the payload gives. With `require`, the record
        /// is refused and the key is named.
        #[test]
        fn required_keys_are_checked_before_anything_is_written() {
            let heartbeat = r#"{"data_encoded_inner":"AQAMS1g3AAABUYBg","subsystem":"WX","dictionary":"weather-station"}"#;
            let lenient = xf("to: cot-xml\n");
            let converted = lenient.convert_at(heartbeat, noon()).unwrap();
            assert!(!converted.text.contains("lat="), "{converted:?}");
            let strict = xf("to: cot-xml\nrequire: [uid, lat, lon]\n");
            assert_eq!(
                strict.apply_at(heartbeat, noon()).unwrap_err().to_string(),
                "the payload gives no `uid`, which `xf.require` names"
            );
            assert!(strict.apply_at(POSITION, noon()).is_ok());
            let unknown = refusal("to: cot-xml\nrequire: [colour]\n");
            assert!(unknown.contains("`require`: undeclared-key"), "{unknown}");
        }

        /// A version 1 record does not name its dictionary: the interface
        /// name finds the pack when one dictionary has it.
        #[test]
        fn a_record_without_a_dictionary_is_matched_by_interface() {
            let xf = xf("to: cot-xml\n");
            let record =
                r#"{"data_encoded_inner":"AwAbS1g3AEBQNZ6D5CWvwGJ27L+xW1dDCIAA","subsystem":"WX"}"#;
            assert!(xf
                .convert_at(record, noon())
                .unwrap()
                .text
                .contains(r#"uid="KX7""#));
        }

        #[test]
        fn packs_of_another_directory_are_loaded_after_those_built_in() {
            let dir = std::env::temp_dir().join(format!("first-bite-xf-{}", std::process::id()));
            let pack = dir.join("packs").join("mine");
            std::fs::create_dir_all(&pack).unwrap();
            std::fs::create_dir_all(dir.join("model")).unwrap();
            std::fs::write(
                dir.join("model").join("mine.def"),
                "module mine\nversion 0.1.0\ndistribution public\nsource \"a test\"\nkey name string - \"a name\"\n",
            )
            .unwrap();
            std::fs::write(
                pack.join("pack.def"),
                "format mine\nversion 0.1.0\ndistribution public\nsource \"a test\"\nuses mine\nwire bits\nbit_order msb\nword_bytes 1\nfield uid 0:0:24 ascii\nfield mine.name 0:0:24 ascii\n",
            )
            .unwrap();
            std::fs::write(
                dir.join("index.json"),
                r#"[{"format":"mine","dictionary":"mine","interface":"M","frame_id_bytes":0}]"#,
            )
            .unwrap();
            let yaml = format!("to: cot-xml\npacks: {}\n", dir.display());
            let xf = xf(&yaml);
            let record = r#"{"data_encoded_inner":"S1g3","subsystem":"M","dictionary":"mine"}"#;
            let converted = xf.convert_at(record, noon());
            std::fs::remove_dir_all(&dir).unwrap();
            assert!(converted.unwrap().text.contains(r#"uid="KX7""#));
            let missing = refusal("to: cot-xml\npacks: /nonexistent/first-bite-packs\n");
            assert!(
                missing.starts_with("`xf`: `packs`: cannot read"),
                "{missing}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_values_are_text_or_numbers() {
        let config: XfConfig = serde_norway::from_str(
            "to: cot-xml\ncontext:\n  type: \"a-f-G\"\n  ce: 12.5\n  stale_ms: 1772625600000\n",
        )
        .unwrap();
        let text: Vec<String> = config.context.values().map(Scalar::to_string).collect();
        assert_eq!(text, ["12.5", "1772625600000", "a-f-G"]);
        assert_eq!(config.stale_minutes, None);
    }
}
