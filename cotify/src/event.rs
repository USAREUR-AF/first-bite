//! Build the Cursor-on-Target event for one decoded message.

use crate::mapping::{EventType, Form, Mapping, Position, UNKNOWN_ERROR};
use chrono::{DateTime, Duration, Utc};
use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::Writer;
use serde::Serialize;
use serde_json::{Map, Value};
use std::io::Cursor;

/// WGS 84 semi-major axis in metres.
const WGS84_A: f64 = 6_378_137.0;
/// WGS 84 inverse flattening.
const WGS84_INVERSE_F: f64 = 298.257_223_563;
/// The latitude iteration stops when a step changes it by less than this
/// many radians, or after `MAX_ITERATIONS` steps.
const LATITUDE_TOLERANCE: f64 = 1e-15;
const MAX_ITERATIONS: usize = 10;

/// Why a record produced no event.
#[derive(Debug, thiserror::Error)]
pub enum CotError {
    #[error("record is not valid JSON: {0}")]
    Record(#[from] serde_json::Error),
    #[error("record carries no decoded message")]
    NoMessage,
    #[error("field `{field}` is missing from the message")]
    MissingField { field: String },
    #[error("field `{field}` is not a single value")]
    NotScalar { field: String },
    #[error("field `{field}` is not a number: {value:?}")]
    NotANumber { field: String, value: String },
    #[error("cannot write the event: {0}")]
    Xml(String),
}

/// A CoT event and the time it was created. Serialises to the JSON object
/// that is published: `{"cot_datetime": ..., "cot_xml_string": ...}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CotEvent {
    pub cot_datetime: String,
    pub cot_xml_string: String,
}

impl Mapping {
    /// Build the event for one decoded message, created at `now`.
    pub fn event(
        &self,
        message: &Map<String, Value>,
        now: DateTime<Utc>,
    ) -> Result<CotEvent, CotError> {
        let uid = self
            .uid
            .iter()
            .map(|field| text(message, field))
            .collect::<Result<Vec<_>, _>>()?
            .join("_");
        let event_type = match &self.event_type {
            EventType::Constant(value) => value.clone(),
            EventType::FromField {
                field,
                values,
                default,
            } => {
                let value = text(message, field)?;
                values.get(&value).unwrap_or(default).clone()
            }
        };
        let (time, start, stale) = event_times(now, self.stale_minutes);
        let (latitude, longitude, altitude) = match &self.position {
            Position::Geodetic {
                latitude,
                longitude,
                altitude,
            } => (
                number(message, latitude)?,
                number(message, longitude)?,
                number(message, altitude)?,
            ),
            Position::Ecef { x, y, z } => ecef_to_geodetic(
                number(message, x)?,
                number(message, y)?,
                number(message, z)?,
            ),
        };
        let cot_xml_string = event_xml(
            self.form,
            &uid,
            &event_type,
            &time,
            &start,
            &stale,
            latitude,
            longitude,
            altitude,
        )?;
        Ok(CotEvent {
            cot_datetime: time,
            cot_xml_string,
        })
    }
}

/// Turn one published record (the JSON of a packet with its decoded `message`)
/// into the JSON of a [`CotEvent`] created now.
pub fn record_to_cot(mapping: &Mapping, record_json: &str) -> Result<String, CotError> {
    record_to_cot_at(mapping, record_json, Utc::now())
}

/// [`record_to_cot`] with the creation time given.
pub fn record_to_cot_at(
    mapping: &Mapping,
    record_json: &str,
    now: DateTime<Utc>,
) -> Result<String, CotError> {
    let record: Value = serde_json::from_str(record_json)?;
    let message = record
        .get("message")
        .and_then(Value::as_object)
        .ok_or(CotError::NoMessage)?;
    let event = mapping.event(message, now)?;
    Ok(serde_json::to_string(&event)?)
}

/// The `time`, `start` and `stale` attributes for an event created at `now`.
pub fn event_times(now: DateTime<Utc>, stale_minutes: u32) -> (String, String, String) {
    let time = now.to_rfc3339();
    let stale = (now + Duration::minutes(i64::from(stale_minutes))).to_rfc3339();
    (time.clone(), time, stale)
}

/// Convert WGS 84 Earth-centred, Earth-fixed coordinates in metres to
/// latitude and longitude in degrees and height above the ellipsoid in
/// metres.
///
/// Bowring's method: a first latitude from the parametric latitude of the
/// point projected onto the ellipsoid, refined until it stops changing. On
/// the polar axis the longitude is 0.
pub fn ecef_to_geodetic(x: f64, y: f64, z: f64) -> (f64, f64, f64) {
    let f = 1.0 / WGS84_INVERSE_F;
    let b = WGS84_A * (1.0 - f);
    let e2 = f * (2.0 - f);
    let second_e2 = e2 / (1.0 - e2);

    // Distance from the polar axis.
    let p = x.hypot(y);
    let longitude = if p == 0.0 { 0.0 } else { y.atan2(x) };

    let mut parametric = (z * WGS84_A).atan2(p * b);
    let mut latitude = 0.0;
    for _ in 0..MAX_ITERATIONS {
        let (sin, cos) = parametric.sin_cos();
        let next = (z + second_e2 * b * sin.powi(3)).atan2(p - e2 * WGS84_A * cos.powi(3));
        let step = (next - latitude).abs();
        latitude = next;
        if step < LATITUDE_TOLERANCE {
            break;
        }
        parametric = ((1.0 - f) * latitude.sin()).atan2(latitude.cos());
    }

    // This form of the height holds at every latitude, the poles included.
    let (sin, cos) = latitude.sin_cos();
    let altitude = p * cos + z * sin - WGS84_A * (1.0 - e2 * sin * sin).sqrt();
    (latitude.to_degrees(), longitude.to_degrees(), altitude)
}

fn xml_error(error: impl ToString) -> CotError {
    CotError::Xml(error.to_string())
}

/// The event as XML in the given form, with one argument per attribute that
/// varies.
#[allow(clippy::too_many_arguments)]
pub fn event_xml(
    form: Form,
    uid: &str,
    event_type: &str,
    time: &str,
    start: &str,
    stale: &str,
    latitude: f64,
    longitude: f64,
    altitude: f64,
) -> Result<String, CotError> {
    let (latitude, longitude, altitude) = (
        latitude.to_string(),
        longitude.to_string(),
        altitude.to_string(),
    );
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut event = BytesStart::new("event");
    event.push_attribute(("version", "2.0"));
    event.push_attribute(("uid", uid));
    event.push_attribute(("type", event_type));
    event.push_attribute(("time", time));
    event.push_attribute(("start", start));
    event.push_attribute(("stale", stale));
    event.push_attribute(("how", "m-g"));
    match form {
        Form::Event => {
            let mut point = BytesStart::new("point");
            point.push_attribute(("lat", latitude.as_str()));
            point.push_attribute(("lon", longitude.as_str()));
            point.push_attribute(("hae", altitude.as_str()));
            point.push_attribute(("ce", UNKNOWN_ERROR));
            point.push_attribute(("le", UNKNOWN_ERROR));
            writer.write_event(Event::Start(event)).map_err(xml_error)?;
            writer.write_event(Event::Empty(point)).map_err(xml_error)?;
            writer
                .write_event(Event::Empty(BytesStart::new("detail")))
                .map_err(xml_error)?;
            writer
                .write_event(Event::End(BytesEnd::new("event")))
                .map_err(xml_error)?;
        }
        Form::Flat => {
            event.push_attribute(("point", "h-e"));
            event.push_attribute(("lat", latitude.as_str()));
            event.push_attribute(("lon", longitude.as_str()));
            event.push_attribute(("hae", altitude.as_str()));
            event.push_attribute(("detail", "a-f-G-U-C"));
            writer.write_event(Event::Start(event)).map_err(xml_error)?;
        }
    }
    String::from_utf8(writer.into_inner().into_inner()).map_err(xml_error)
}

/// The value of `field` as text: a label or number as the decoder wrote it.
fn text(message: &Map<String, Value>, field: &str) -> Result<String, CotError> {
    match message.get(field) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(Value::Number(value)) => Ok(value.to_string()),
        Some(Value::Bool(value)) => Ok(value.to_string()),
        None | Some(Value::Null) => Err(CotError::MissingField {
            field: field.to_string(),
        }),
        Some(_) => Err(CotError::NotScalar {
            field: field.to_string(),
        }),
    }
}

/// The value of `field` as a finite number, whether the decoder wrote it as a
/// decimal string or as a JSON number.
fn number(message: &Map<String, Value>, field: &str) -> Result<f64, CotError> {
    let value = text(message, field)?;
    match value.trim().parse::<f64>() {
        Ok(number) if number.is_finite() => Ok(number),
        _ => Err(CotError::NotANumber {
            field: field.to_string(),
            value,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    /// ECEF coordinates of a point 300 m above the ellipsoid at 48.8584 N,
    /// 2.2945 E.
    const TOWER_ECEF: (f64, f64, f64) = (4201133.037, 168331.004, 4780438.968);

    fn mapping(yaml: &str) -> Mapping {
        serde_norway::from_str(yaml).unwrap()
    }

    fn ecef_mapping() -> Mapping {
        mapping(
            "uid: [sensor, contact]\n\
             type:\n  field: affiliation\n  default: unknown\n  values:\n    \
             \"Amber\": a-p-A\n    \"Blue\": a-f-A\n    \"Green\": a-n-A\n\
             position: { ecef: [ecef_x, ecef_y, ecef_z] }\n",
        )
    }

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    fn noon() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 4, 12, 0, 0).unwrap()
    }

    #[test]
    fn uid_joins_field_values_with_underscores() {
        let mapping = ecef_mapping();
        let event = |sensor: &str, contact: &str| {
            mapping
                .event(
                    &object(json!({
                        "sensor": sensor, "contact": contact, "affiliation": "Blue",
                        "ecef_x": "1", "ecef_y": "2", "ecef_z": "3",
                    })),
                    noon(),
                )
                .unwrap()
                .cot_xml_string
        };
        assert!(event("mast1", "contact1").contains(r#" uid="mast1_contact1" "#));
        assert!(event("", "").contains(r#" uid="_" "#));
    }

    #[test]
    fn type_is_looked_up_with_a_default() {
        let mapping = ecef_mapping();
        let event_type = |affiliation: &str| {
            mapping
                .event(
                    &object(json!({
                        "sensor": "s", "contact": "c", "affiliation": affiliation,
                        "ecef_x": "1", "ecef_y": "2", "ecef_z": "3",
                    })),
                    noon(),
                )
                .unwrap()
                .cot_xml_string
        };
        assert!(event_type("Amber").contains(r#" type="a-p-A" "#));
        assert!(event_type("Green").contains(r#" type="a-n-A" "#));
        assert!(event_type("Violet").contains(r#" type="unknown" "#));
    }

    #[test]
    fn times_start_now_and_go_stale_later() {
        let (time, start, stale) = event_times(noon(), 60);
        assert_eq!(time, "2026-03-04T12:00:00+00:00");
        assert_eq!(start, time);
        assert_eq!(stale, "2026-03-04T13:00:00+00:00");
        assert_eq!(event_times(noon(), 5).2, "2026-03-04T12:05:00+00:00");
    }

    /// The textbook forward conversion, written out here so the function
    /// under test is checked against something it does not share code with.
    fn geodetic_to_ecef(latitude: f64, longitude: f64, altitude: f64) -> (f64, f64, f64) {
        let a = 6_378_137.0_f64;
        let f = 1.0 / 298.257_223_563_f64;
        let e2 = 2.0 * f - f * f;
        let (lat, lon) = (latitude.to_radians(), longitude.to_radians());
        let n = a / (1.0 - e2 * lat.sin().powi(2)).sqrt();
        (
            (n + altitude) * lat.cos() * lon.cos(),
            (n + altitude) * lat.cos() * lon.sin(),
            (n * (1.0 - e2) + altitude) * lat.sin(),
        )
    }

    /// The numeric value of one attribute of an event element.
    fn attribute(xml: &str, name: &str) -> f64 {
        let start = xml.find(&format!(" {name}=\"")).unwrap() + name.len() + 3;
        let end = start + xml[start..].find('"').unwrap();
        xml[start..end].parse().unwrap()
    }

    #[test]
    fn ecef_round_trips_from_known_geodetic_points() {
        let points = [
            // Equator, on and off the prime meridian, on the surface and above.
            (0.0, 0.0, 0.0),
            (0.0, 90.0, 1_000.0),
            (0.0, -179.999, 12.5),
            // Mid-latitudes in all four quadrants.
            (48.8584, 2.2945, 300.0),
            (64.8378, -147.7164, 136.5),
            (-33.8568, 151.2153, 50.0),
            (-54.8019, -68.3030, 30.0),
            // Below the ellipsoid.
            (31.5, 35.5, -430.0),
            // Near the poles and on them.
            (89.9999, 45.0, 10.0),
            (-89.9999, -120.0, 2_800.0),
            (90.0, 0.0, 100.0),
            (-90.0, 0.0, 2_835.0),
            // Aircraft, low orbit and navigation-satellite altitudes.
            (35.0, -120.0, 12_000.0),
            (51.6, 10.0, 400_000.0),
            (-20.0, 77.0, 20_200_000.0),
        ];
        for (latitude, longitude, altitude) in points {
            let (x, y, z) = geodetic_to_ecef(latitude, longitude, altitude);
            let (lat, lon, alt) = ecef_to_geodetic(x, y, z);
            let point = format!("({latitude}, {longitude}, {altitude})");
            assert!((lat - latitude).abs() < 1e-9, "{point}: latitude {lat}");
            assert!((lon - longitude).abs() < 1e-9, "{point}: longitude {lon}");
            assert!((alt - altitude).abs() < 1e-4, "{point}: altitude {alt}");
        }
    }

    #[test]
    fn ecef_converts_to_latitude_longitude_height() {
        let (x, y, z) = TOWER_ECEF;
        let (latitude, longitude, altitude) = ecef_to_geodetic(x, y, z);
        assert!((latitude - 48.858_399_998_376_306).abs() < 1e-9);
        assert!((longitude - 2.294_499_995_781_851).abs() < 1e-9);
        assert!((altitude - 300.000_057_493_336_5).abs() < 1e-4);
    }

    #[test]
    fn the_polar_axis_has_longitude_zero() {
        let (latitude, longitude, altitude) = ecef_to_geodetic(0.0, 0.0, 6_356_852.314_245_179);
        assert!((latitude - 90.0).abs() < 1e-9);
        assert_eq!(longitude, 0.0);
        assert!((altitude - 100.0).abs() < 1e-4);
        let (latitude, _, altitude) = ecef_to_geodetic(0.0, 0.0, -6_356_752.314_245_179);
        assert!((latitude + 90.0).abs() < 1e-9);
        assert!(altitude.abs() < 1e-4);
    }

    #[test]
    fn event_form_is_a_complete_event_with_a_point() {
        let xml = event_xml(
            Form::Event,
            "uid",
            "type",
            "now",
            "start",
            "stale",
            48.8584,
            2.2945,
            300.0,
        )
        .unwrap();
        assert_eq!(
            xml,
            r#"<event version="2.0" uid="uid" type="type" time="now" start="start" stale="stale" how="m-g"><point lat="48.8584" lon="2.2945" hae="300" ce="9999999" le="9999999"/><detail/></event>"#
        );
    }

    #[test]
    fn flat_form_is_one_start_tag_with_the_position_as_attributes() {
        let xml = event_xml(
            Form::Flat,
            "uid",
            "type",
            "now",
            "start",
            "stale",
            48.8584,
            2.2945,
            300.0,
        )
        .unwrap();
        assert_eq!(
            xml,
            r#"<event version="2.0" uid="uid" type="type" time="now" start="start" stale="stale" how="m-g" point="h-e" lat="48.8584" lon="2.2945" hae="300" detail="a-f-G-U-C">"#
        );
    }

    #[test]
    fn an_attribute_value_is_escaped() {
        let xml = event_xml(
            Form::Event,
            r#"a"<b>&c"#,
            "type",
            "now",
            "start",
            "stale",
            0.0,
            0.0,
            0.0,
        )
        .unwrap();
        assert!(xml.contains(r#" uid="a&quot;&lt;b&gt;&amp;c" "#), "{xml}");
    }

    #[test]
    fn ecef_message_becomes_an_event() {
        let (x, y, z) = TOWER_ECEF;
        // Numbers may arrive as JSON numbers as well as decimal strings.
        let message = object(json!({
            "sensor": "1", "contact": "1", "affiliation": "Blue",
            "ecef_x": x, "ecef_y": y, "ecef_z": z,
        }));
        let event = ecef_mapping().event(&message, noon()).unwrap();
        assert_eq!(event.cot_datetime, "2026-03-04T12:00:00+00:00");
        let xml = &event.cot_xml_string;
        assert!(
            xml.starts_with(
                r#"<event version="2.0" uid="1_1" type="a-f-A" time="2026-03-04T12:00:00+00:00" start="2026-03-04T12:00:00+00:00" stale="2026-03-04T13:00:00+00:00" how="m-g"><point lat=""#
            ),
            "{xml}"
        );
        assert!((attribute(xml, "lat") - 48.858_399_998_376_306).abs() < 1e-9);
        assert!((attribute(xml, "lon") - 2.294_499_995_781_851).abs() < 1e-9);
        assert!((attribute(xml, "hae") - 300.000_057_493_336_5).abs() < 1e-4);
        assert!(
            xml.ends_with(r#"" ce="9999999" le="9999999"/><detail/></event>"#),
            "{xml}"
        );
    }

    #[test]
    fn record_with_string_fields_becomes_cot_json() {
        let record = r#"{
            "timestamp": {"secs_since_epoch": 1695769999, "nanos_since_epoch": 999999999},
            "src_port": "11111",
            "dst_port": "22222",
            "src_ip": [192, 0, 2, 10],
            "dst_ip": [192, 0, 2, 20],
            "data_encoded_inner": "AAAA",
            "subsystem": "DEMO",
            "message_type": "Contact Report",
            "message": {
                "message_length": "50",
                "sensor": "12",
                "contact": "3",
                "affiliation": "Green",
                "ecef_x": "123456",
                "ecef_y": "-123456",
                "ecef_z": "12345",
                "speed": "22",
                "simulated": "true"
            },
            "data_encoded_full": "AAAA"
        }"#;
        let json = record_to_cot_at(&ecef_mapping(), record, noon()).unwrap();
        let event: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(event["cot_datetime"], "2026-03-04T12:00:00+00:00");
        let xml = event["cot_xml_string"].as_str().unwrap();
        assert!(
            xml.starts_with(
                r#"<event version="2.0" uid="12_3" type="a-n-A" time="2026-03-04T12:00:00+00:00" start="2026-03-04T12:00:00+00:00" stale="2026-03-04T13:00:00+00:00" how="m-g"><point lat=""#
            ),
            "{xml}"
        );
        assert!(
            xml.ends_with(r#"" ce="9999999" le="9999999"/><detail/></event>"#),
            "{xml}"
        );
        assert!((attribute(xml, "lon") + 45.0).abs() < 1e-9);
        // The published object always lists the time before the XML.
        assert!(json.starts_with(r#"{"cot_datetime":"#), "{json}");
        assert!(record_to_cot(&ecef_mapping(), record).is_ok());
    }

    #[test]
    fn geodetic_message_becomes_an_event() {
        let mapping = mapping(
            "uid: [station_id]\n\
             type: { value: a-f-G }\n\
             position: { geodetic: [latitude, longitude, altitude] }\n\
             stale_minutes: 10\n",
        );
        let message = object(json!({
            "station_id": "KX7", "latitude": "64.8378", "longitude": "-147.7164", "altitude": "136.5",
        }));
        assert_eq!(
            mapping.event(&message, noon()).unwrap().cot_xml_string,
            r#"<event version="2.0" uid="KX7" type="a-f-G" time="2026-03-04T12:00:00+00:00" start="2026-03-04T12:00:00+00:00" stale="2026-03-04T12:10:00+00:00" how="m-g"><point lat="64.8378" lon="-147.7164" hae="136.5" ce="9999999" le="9999999"/><detail/></event>"#
        );
    }

    #[test]
    fn flat_mapping_writes_what_earlier_releases_wrote() {
        let mapping = mapping(
            "uid: [station_id]\n\
             type: { value: a-f-G }\n\
             position: { geodetic: [latitude, longitude, altitude] }\n\
             stale_minutes: 10\n\
             form: flat\n",
        );
        let message = object(json!({
            "station_id": "KX7", "latitude": "64.8378", "longitude": "-147.7164", "altitude": "136.5",
        }));
        assert_eq!(
            mapping.event(&message, noon()).unwrap().cot_xml_string,
            r#"<event version="2.0" uid="KX7" type="a-f-G" time="2026-03-04T12:00:00+00:00" start="2026-03-04T12:00:00+00:00" stale="2026-03-04T12:10:00+00:00" how="m-g" point="h-e" lat="64.8378" lon="-147.7164" hae="136.5" detail="a-f-G-U-C">"#
        );
    }

    #[test]
    fn errors_name_the_field() {
        let mapping = ecef_mapping();
        let complete = json!({
            "sensor": "s", "contact": "c", "affiliation": "Blue",
            "ecef_x": "1", "ecef_y": "2", "ecef_z": "3",
        });

        let mut missing = object(complete.clone());
        missing.remove("contact");
        assert!(matches!(
            mapping.event(&missing, noon()),
            Err(CotError::MissingField { field }) if field == "contact"
        ));

        // A field beyond the end of a short payload decodes to null.
        let mut null = object(complete.clone());
        null.insert("ecef_y".into(), Value::Null);
        assert!(matches!(
            mapping.event(&null, noon()),
            Err(CotError::MissingField { field }) if field == "ecef_y"
        ));

        for bad in ["north", "", "NaN", "inf"] {
            let mut unparsable = object(complete.clone());
            unparsable.insert("ecef_z".into(), json!(bad));
            assert!(
                matches!(
                    mapping.event(&unparsable, noon()),
                    Err(CotError::NotANumber { ref field, ref value }) if field == "ecef_z" && value == bad
                ),
                "{bad:?}"
            );
        }

        let mut nested = object(complete);
        nested.insert("sensor".into(), json!(["a", "b"]));
        assert!(matches!(
            mapping.event(&nested, noon()),
            Err(CotError::NotScalar { field }) if field == "sensor"
        ));
    }

    #[test]
    fn records_without_a_decoded_message_are_refused() {
        let mapping = ecef_mapping();
        assert!(matches!(
            record_to_cot(&mapping, r#"{"subsystem": null, "message": null}"#),
            Err(CotError::NoMessage)
        ));
        assert!(matches!(
            record_to_cot(&mapping, "not json"),
            Err(CotError::Record(_))
        ));
    }
}
