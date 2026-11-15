//! A scaled integer decodes to the decimal the document means, not to the
//! product a double makes of it.

use bite::{Dictionary, Value};

const METADATA: &str = r#"{
    "port": 40200,
    "message_type": {"start": 0, "stop": 8, "values": {"1": "Fix"}}
}"#;

const FIX: &str = r#"{
    "fields": {
        "latitude": {"start": 8, "stop": 40, "type": "int", "scale": 1e-7, "unit": "deg"},
        "longitude": {"start": 40, "stop": 72, "type": "int", "scale": 0.0000001, "unit": "deg"},
        "ratio": {"start": 72, "stop": 80, "scale": 0.01},
        "level": {"start": 80, "stop": 96, "scale": 0.05, "offset": -102.4, "unit": "dB"},
        "coarse": {"start": 96, "stop": 104, "scale": 25},
        "binary": {"start": 104, "stop": 120, "scale": 0.0000152587890625},
        "fraction": {"start": 120, "stop": 152, "scale": 2.3283064365386963e-10},
        "third": {"start": 152, "stop": 160, "scale": 0.3333333333333333},
        "wide": {"start": 160, "stop": 224, "scale": 0.001},
        "reading": {"start": 224, "stop": 256, "type": "float", "scale": 0.1}
    }
}"#;

fn dictionary() -> Dictionary {
    Dictionary::from_files([
        ("LINK/metadata.json", METADATA),
        ("LINK/messages/Fix.json", FIX),
    ])
    .unwrap()
}

/// The payload of a Fix message with the given raw values.
#[allow(clippy::too_many_arguments)]
fn payload(
    latitude: i32,
    longitude: i32,
    ratio: u8,
    level: u16,
    coarse: u8,
    binary: u16,
    fraction: u32,
    third: u8,
    wide: u64,
    reading: f32,
) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&latitude.to_be_bytes());
    bytes.extend_from_slice(&longitude.to_be_bytes());
    bytes.push(ratio);
    bytes.extend_from_slice(&level.to_be_bytes());
    bytes.push(coarse);
    bytes.extend_from_slice(&binary.to_be_bytes());
    bytes.extend_from_slice(&fraction.to_be_bytes());
    bytes.push(third);
    bytes.extend_from_slice(&wide.to_be_bytes());
    bytes.extend_from_slice(&reading.to_be_bytes());
    bytes
}

fn float(decoded: &bite::Decoded<'_>, field: &str) -> f64 {
    match decoded.field(field).unwrap().value {
        Some(Value::Float(value)) => value,
        ref other => panic!("{field} is {other:?}"),
    }
}

#[test]
fn decimal_scales_give_the_decimal_the_document_means() {
    let dictionary = dictionary();
    let interface = dictionary.interface("LINK").unwrap();
    let decoded = interface
        .decode(&payload(
            445_012_345,
            -1_477_164_000,
            35,
            2300,
            7,
            0,
            0,
            0,
            0,
            0.0,
        ))
        .unwrap();

    // The plain products are 44.501234499999995, 0.35000000000000003 and
    // 12.599999999999994.
    assert_ne!(445_012_345f64 * 1e-7, 44.5012345);
    assert_ne!(35f64 * 0.01, 0.35);
    assert_eq!(float(&decoded, "latitude"), 44.5012345);
    assert_eq!(float(&decoded, "longitude"), -147.7164);
    assert_eq!(float(&decoded, "ratio"), 0.35);
    assert_eq!(float(&decoded, "level"), 12.6);
    assert_eq!(float(&decoded, "coarse"), 175.0);

    // As text and as JSON the values are written as the document writes them.
    let rendered = decoded.rendered();
    assert_eq!(rendered["latitude"].as_deref(), Some("44.5012345"));
    assert_eq!(rendered["ratio"].as_deref(), Some("0.35"));
    assert_eq!(rendered["level"].as_deref(), Some("12.6"));
    let json = serde_json::to_string(&decoded).unwrap();
    assert!(json.contains(r#""latitude":44.5012345,"#), "{json}");
    assert!(json.contains(r#""longitude":-147.7164,"#), "{json}");
    assert!(json.contains(r#""ratio":0.35,"#), "{json}");
    assert!(json.contains(r#""level":12.6,"#), "{json}");
}

#[test]
fn every_raw_value_of_a_decimal_scale_is_the_nearest_double_to_its_decimal() {
    let dictionary = dictionary();
    let interface = dictionary.interface("LINK").unwrap();
    for raw in 0..=u16::MAX {
        let decoded = interface
            .decode(&payload(
                i32::from(raw) * 6791,
                0,
                raw as u8,
                raw,
                0,
                0,
                0,
                0,
                0,
                0.0,
            ))
            .unwrap();
        // The decimal, written out and parsed, is the reference.
        let latitude = i64::from(raw) * 6791;
        let expected: f64 = format!("{}.{:07}", latitude / 10_000_000, latitude % 10_000_000)
            .parse()
            .unwrap();
        assert_eq!(float(&decoded, "latitude"), expected, "raw {latitude}");
        let ratio = u32::from(raw as u8);
        let expected: f64 = format!("{}.{:02}", ratio / 100, ratio % 100)
            .parse()
            .unwrap();
        assert_eq!(float(&decoded, "ratio"), expected, "raw {ratio}");
        // level = raw * 0.05 - 102.4, in hundredths: raw * 5 - 10240.
        let hundredths = i64::from(raw) * 5 - 10_240;
        let expected: f64 = format!(
            "{}{}.{:02}",
            if hundredths < 0 { "-" } else { "" },
            hundredths.abs() / 100,
            hundredths.abs() % 100
        )
        .parse()
        .unwrap();
        assert_eq!(float(&decoded, "level"), expected, "raw {raw}");
    }
}

#[test]
fn scales_that_are_not_short_decimals_are_left_as_computed() {
    let dictionary = dictionary();
    let interface = dictionary.interface("LINK").unwrap();
    let decoded = interface
        .decode(&payload(
            0,
            0,
            0,
            0,
            0,
            0x4000,
            0x2450_0f3c,
            2,
            u64::MAX,
            2.5,
        ))
        .unwrap();
    // Powers of two: the product is exact and stays exact.
    assert_eq!(float(&decoded, "binary"), 0.25);
    assert_eq!(
        float(&decoded, "fraction"),
        f64::from(0x2450_0f3c_u32) / 4_294_967_296.0
    );
    // 1/3 has no decimal form: the product, as a double makes it.
    assert_eq!(float(&decoded, "third"), 2.0 * 0.3333333333333333);
    // More than 15 significant digits: the product, as a double makes it.
    assert_eq!(float(&decoded, "wide"), u64::MAX as f64 * 0.001);
    // A float field is a binary fraction on the wire; nothing is rounded.
    assert_eq!(float(&decoded, "reading"), 2.5 * 0.1);
}
