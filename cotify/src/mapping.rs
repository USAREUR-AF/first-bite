//! Which fields of a decoded message supply the parts of a Cursor-on-Target
//! event.
//!
//! A mapping is configuration. In YAML:
//!
//! ```yaml
//! uid: [station_id]            # field values joined with "_"
//! type:
//!   value: "a-f-G"             # a constant CoT type, or a lookup:
//! # type: { field: affiliation, values: { "Hostile": "a-h-A" }, default: "a-u-A" }
//! position:
//!   geodetic: [latitude, longitude, altitude]   # degrees, degrees, metres HAE
//!   # or: ecef: [x, y, z]                        # metres, WGS 84 Earth-centred
//! stale_minutes: 60
//! form: event                  # or `flat`; see [`Form`]
//! ```

use serde::Deserialize;
use std::collections::BTreeMap;

/// Minutes an event stays valid when the mapping does not say.
pub const DEFAULT_STALE_MINUTES: u32 = 60;

/// Why a mapping is not usable.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum MappingError {
    #[error("uid: name at least one field")]
    EmptyUid,
    #[error("type: give `value` or `field`")]
    TypeMissing,
    #[error("type: give either `value` or `field`, not both")]
    TypeAmbiguous,
    #[error("type: `values` and `default` apply only together with `field`")]
    TypeConstantWithLookup,
    #[error("type: `default` is required together with `field`")]
    TypeDefaultMissing,
    #[error("position: give `geodetic` or `ecef`")]
    PositionMissing,
    #[error("position: give either `geodetic` or `ecef`, not both")]
    PositionAmbiguous,
}

/// The XML an event is written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Form {
    /// A complete CoT 2.0 event: `<event …><point lat lon hae ce le/><detail/></event>`.
    /// The position errors `ce` and `le` are written as [`UNKNOWN_ERROR`].
    #[default]
    Event,
    /// One unclosed `<event>` start tag carrying the position as attributes
    /// (`point="h-e" lat lon hae detail="a-f-G-U-C"`): the output of releases
    /// before `form` existed, for consumers written against it.
    Flat,
}

/// What `ce` and `le` are set to: the CoT value for an error that is not known.
pub const UNKNOWN_ERROR: &str = "9999999";

/// How one kind of decoded message becomes a CoT event.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RawMapping")]
pub struct Mapping {
    /// Fields whose values, joined with `_`, identify the object.
    pub uid: Vec<String>,
    pub event_type: EventType,
    pub position: Position,
    /// Minutes after its creation at which the event goes stale.
    pub stale_minutes: u32,
    /// The XML the event is written as.
    pub form: Form,
}

/// Where the CoT type string comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventType {
    /// The same type for every message.
    Constant(String),
    /// The value of `field`, translated through `values`; a value that is not
    /// listed gives `default`.
    FromField {
        field: String,
        values: BTreeMap<String, String>,
        default: String,
    },
}

/// Which fields carry the position, and in which coordinate system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Position {
    /// Latitude and longitude in degrees, altitude in metres above the WGS 84
    /// ellipsoid.
    Geodetic {
        latitude: String,
        longitude: String,
        altitude: String,
    },
    /// WGS 84 Earth-centred, Earth-fixed coordinates in metres.
    Ecef { x: String, y: String, z: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMapping {
    uid: Vec<String>,
    #[serde(rename = "type")]
    event_type: RawEventType,
    position: RawPosition,
    #[serde(default = "default_stale_minutes")]
    stale_minutes: u32,
    #[serde(default)]
    form: Form,
}

fn default_stale_minutes() -> u32 {
    DEFAULT_STALE_MINUTES
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEventType {
    value: Option<String>,
    field: Option<String>,
    values: Option<BTreeMap<String, String>>,
    default: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPosition {
    geodetic: Option<[String; 3]>,
    ecef: Option<[String; 3]>,
}

impl TryFrom<RawMapping> for Mapping {
    type Error = MappingError;

    fn try_from(raw: RawMapping) -> Result<Self, MappingError> {
        if raw.uid.is_empty() {
            return Err(MappingError::EmptyUid);
        }
        Ok(Mapping {
            uid: raw.uid,
            event_type: raw.event_type.try_into()?,
            position: raw.position.try_into()?,
            stale_minutes: raw.stale_minutes,
            form: raw.form,
        })
    }
}

impl TryFrom<RawEventType> for EventType {
    type Error = MappingError;

    fn try_from(raw: RawEventType) -> Result<Self, MappingError> {
        match (raw.value, raw.field) {
            (None, None) => Err(MappingError::TypeMissing),
            (Some(_), Some(_)) => Err(MappingError::TypeAmbiguous),
            (Some(value), None) => {
                if raw.values.is_some() || raw.default.is_some() {
                    return Err(MappingError::TypeConstantWithLookup);
                }
                Ok(EventType::Constant(value))
            }
            (None, Some(field)) => Ok(EventType::FromField {
                field,
                values: raw.values.unwrap_or_default(),
                default: raw.default.ok_or(MappingError::TypeDefaultMissing)?,
            }),
        }
    }
}

impl TryFrom<RawPosition> for Position {
    type Error = MappingError;

    fn try_from(raw: RawPosition) -> Result<Self, MappingError> {
        match (raw.geodetic, raw.ecef) {
            (None, None) => Err(MappingError::PositionMissing),
            (Some(_), Some(_)) => Err(MappingError::PositionAmbiguous),
            (Some([latitude, longitude, altitude]), None) => Ok(Position::Geodetic {
                latitude,
                longitude,
                altitude,
            }),
            (None, Some([x, y, z])) => Ok(Position::Ecef { x, y, z }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> Result<Mapping, String> {
        serde_norway::from_str(yaml).map_err(|e| e.to_string())
    }

    #[test]
    fn constant_type_and_geodetic_position() {
        let mapping = parse(
            "uid: [station_id]\n\
             type:\n  value: \"a-f-G\"\n\
             position:\n  geodetic: [latitude, longitude, altitude]\n\
             stale_minutes: 15\n",
        )
        .unwrap();
        assert_eq!(mapping.uid, ["station_id"]);
        assert_eq!(mapping.event_type, EventType::Constant("a-f-G".into()));
        assert_eq!(
            mapping.position,
            Position::Geodetic {
                latitude: "latitude".into(),
                longitude: "longitude".into(),
                altitude: "altitude".into(),
            }
        );
        assert_eq!(mapping.stale_minutes, 15);
        assert_eq!(mapping.form, Form::Event);
    }

    #[test]
    fn form_is_event_or_flat() {
        let with_form = |form: &str| {
            parse(&format!(
                "uid: [id]\ntype: {{ value: a-f-G }}\nposition: {{ geodetic: [a, b, c] }}\nform: {form}\n"
            ))
        };
        assert_eq!(with_form("event").unwrap().form, Form::Event);
        assert_eq!(with_form("flat").unwrap().form, Form::Flat);
        assert!(with_form("nested").is_err());
    }

    #[test]
    fn looked_up_type_and_ecef_position() {
        let mapping = parse(
            "uid: [sensor, contact]\n\
             type: { field: affiliation, values: { \"Hostile\": \"a-h-A\" }, default: \"a-u-A\" }\n\
             position: { ecef: [ecef_x, ecef_y, ecef_z] }\n",
        )
        .unwrap();
        assert_eq!(
            mapping.event_type,
            EventType::FromField {
                field: "affiliation".into(),
                values: BTreeMap::from([("Hostile".to_string(), "a-h-A".to_string())]),
                default: "a-u-A".into(),
            }
        );
        assert_eq!(
            mapping.position,
            Position::Ecef {
                x: "ecef_x".into(),
                y: "ecef_y".into(),
                z: "ecef_z".into(),
            }
        );
        assert_eq!(mapping.stale_minutes, DEFAULT_STALE_MINUTES);
    }

    #[test]
    fn the_same_mapping_parses_from_json() {
        let mapping: Mapping = serde_json::from_str(
            r#"{"uid": ["a"], "type": {"value": "a-f-G"}, "position": {"ecef": ["x", "y", "z"]}}"#,
        )
        .unwrap();
        assert_eq!(mapping.uid, ["a"]);
    }

    #[test]
    fn unusable_mappings_say_why() {
        let position = "position: { geodetic: [a, b, c] }\n";
        let cases = [
            (
                format!("uid: []\ntype: {{ value: x }}\n{position}"),
                MappingError::EmptyUid,
            ),
            (
                format!("uid: [a]\ntype: {{}}\n{position}"),
                MappingError::TypeMissing,
            ),
            (
                format!("uid: [a]\ntype: {{ value: x, field: f, default: d }}\n{position}"),
                MappingError::TypeAmbiguous,
            ),
            (
                format!("uid: [a]\ntype: {{ value: x, default: d }}\n{position}"),
                MappingError::TypeConstantWithLookup,
            ),
            (
                format!("uid: [a]\ntype: {{ field: f }}\n{position}"),
                MappingError::TypeDefaultMissing,
            ),
            (
                "uid: [a]\ntype: { value: x }\nposition: {}\n".to_string(),
                MappingError::PositionMissing,
            ),
            (
                "uid: [a]\ntype: { value: x }\nposition: { geodetic: [a, b, c], ecef: [x, y, z] }\n"
                    .to_string(),
                MappingError::PositionAmbiguous,
            ),
        ];
        for (yaml, expected) in cases {
            let error = parse(&yaml).unwrap_err();
            assert!(
                error.contains(&expected.to_string()),
                "{yaml}: got {error:?}, expected {expected}"
            );
        }
    }

    #[test]
    fn a_position_needs_exactly_three_fields() {
        assert!(parse("uid: [a]\ntype: { value: x }\nposition: { ecef: [x, y] }\n").is_err());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let error = parse(
            "uid: [a]\ntype: { value: x }\nposition: { ecef: [x, y, z] }\nstale_minuets: 5\n",
        )
        .unwrap_err();
        assert!(error.contains("stale_minuets"), "{error}");
    }
}
