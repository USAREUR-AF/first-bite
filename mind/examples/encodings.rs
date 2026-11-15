//! Size and encode time of one record in each form `mind` can publish:
//! version 1 JSON (every value as text), version 2 JSON (typed values) and
//! version 2 CBOR.
//!
//! ```text
//! cargo run --release -p mind --example encodings -- dictionaries/weather-station
//! ```
//!
//! Each vector of the dictionary is wrapped in a UDP frame and passed through
//! the pipeline, as a captured packet would be. Sizes are of the whole
//! record and, beside it, of its `message` alone. Times are per record, on
//! one core:
//!
//! * record to JSON text, which `mind` does once per packet;
//! * that text to CBOR, which a publisher or sink with `encoding: cbor` does
//!   in addition.

use bite::vectors::load_vectors;
use bite::Dictionary;
use datamodels::packets::{PacketInfoWithParsedMessage, RecordVersion};
use flow::encoding::{cbor_to_json, json_to_cbor};
use mind::{Frame, LinkType, Pipeline};
use std::hint::black_box;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

const ROUNDS: u32 = 200_000;

/// An Ethernet, IPv4 and UDP frame around `payload`, to `port`.
fn udp_frame(port: u16, payload: &[u8]) -> Vec<u8> {
    let udp_length = 8 + payload.len();
    let ip_length = 20 + udp_length;
    let mut frame = vec![2, 0, 0, 0, 0, 2, 2, 0, 0, 0, 0, 1, 0x08, 0x00];
    frame.extend_from_slice(&[0x45, 0]);
    frame.extend_from_slice(&(ip_length as u16).to_be_bytes());
    frame.extend_from_slice(&[0, 0, 0, 0, 64, 17, 0, 0, 192, 0, 2, 1, 192, 0, 2, 2]);
    frame.extend_from_slice(&50000u16.to_be_bytes());
    frame.extend_from_slice(&port.to_be_bytes());
    frame.extend_from_slice(&(udp_length as u16).to_be_bytes());
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(payload);
    frame
}

/// Mean nanoseconds per call of `work` over every record, `ROUNDS` times.
fn nanoseconds<T>(records: usize, mut work: impl FnMut(usize) -> T) -> f64 {
    let started = Instant::now();
    for _ in 0..ROUNDS {
        for index in 0..records {
            black_box(work(index));
        }
    }
    started.elapsed().as_nanos() as f64 / (f64::from(ROUNDS) * records as f64)
}

/// Bytes of a record's `message` as JSON and as CBOR.
fn message_bytes(record: &str) -> (usize, usize) {
    let value: serde_json::Value = serde_json::from_str(record).unwrap_or_default();
    let message = value["message"].to_string();
    let cbor = json_to_cbor(&message).map_or(0, |cbor| cbor.len());
    (message.len(), cbor)
}

fn run(path: &str) -> Result<(), String> {
    let dictionary = Arc::new(Dictionary::load(path).map_err(|e| e.to_string())?);
    let vectors = load_vectors(&dictionary).map_err(|e| e.to_string())?;

    // One frame per vector whose interface is carried on a port.
    let mut frames = Vec::new();
    for (name, vector) in vectors {
        let Ok(vector) = vector else { continue };
        let port = dictionary
            .interface(&vector.interface)
            .and_then(|interface| interface.port);
        let compact: String = vector
            .payload_hex
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if let (Some(port), Ok(payload)) = (port, hex::decode(compact)) {
            frames.push((name, udp_frame(port, &payload)));
        }
    }
    if frames.is_empty() {
        return Err("no vector is carried on a UDP or TCP port".to_string());
    }

    let build = |version: RecordVersion| -> Result<Vec<PacketInfoWithParsedMessage>, String> {
        let pipeline = Pipeline::new(Some(Arc::clone(&dictionary))).with_record_version(version);
        frames
            .iter()
            .map(|(name, data)| {
                pipeline
                    .process(&Frame {
                        timestamp: UNIX_EPOCH + Duration::new(1_700_000_000, 250_000_000),
                        link: LinkType::Ethernet,
                        data,
                        interface: None,
                    })
                    .map_err(|e| format!("{name}: {e}"))
            })
            .collect()
    };
    let text = build(RecordVersion::Text)?;
    let typed = build(RecordVersion::Typed)?;
    let json = |records: &[PacketInfoWithParsedMessage]| -> Result<Vec<String>, String> {
        records
            .iter()
            .map(|record| serde_json::to_string(record).map_err(|e| e.to_string()))
            .collect()
    };
    let (text_json, typed_json) = (json(&text)?, json(&typed)?);
    let typed_cbor = typed_json
        .iter()
        .map(|record| json_to_cbor(record).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;

    println!("{path}: {} records", typed.len());
    println!(
        "{:<28} {:>8} {:>8} {:>8}   {:>8} {:>8} {:>8}",
        "bytes", "v1 json", "v2 json", "v2 cbor", "msg v1", "msg v2", "msg cbor"
    );
    let mut totals = [0usize; 6];
    for (index, (name, _)) in frames.iter().enumerate() {
        let (message_v1, _) = message_bytes(&text_json[index]);
        let (message_v2, message_cbor) = message_bytes(&typed_json[index]);
        let row = [
            text_json[index].len(),
            typed_json[index].len(),
            typed_cbor[index].len(),
            message_v1,
            message_v2,
            message_cbor,
        ];
        println!(
            "{name:<28} {:>8} {:>8} {:>8}   {:>8} {:>8} {:>8}",
            row[0], row[1], row[2], row[3], row[4], row[5]
        );
        for (total, value) in totals.iter_mut().zip(row) {
            *total += value;
        }
    }
    let mean = |total: usize| total as f64 / frames.len() as f64;
    println!(
        "{:<28} {:>8.1} {:>8.1} {:>8.1}   {:>8.1} {:>8.1} {:>8.1}",
        "mean",
        mean(totals[0]),
        mean(totals[1]),
        mean(totals[2]),
        mean(totals[3]),
        mean(totals[4]),
        mean(totals[5])
    );

    let count = typed.len();
    let v1 = nanoseconds(count, |index| serde_json::to_string(&text[index]));
    let v2 = nanoseconds(count, |index| serde_json::to_string(&typed[index]));
    let cbor = nanoseconds(count, |index| json_to_cbor(&typed_json[index]));
    let back = nanoseconds(count, |index| cbor_to_json(&typed_cbor[index]));
    println!("nanoseconds per record:");
    println!("  v1 record to JSON text              {v1:>8.0}");
    println!("  v2 record to JSON text              {v2:>8.0}");
    println!("  v2 JSON text to CBOR (in addition)  {cbor:>8.0}");
    println!("  v2 CBOR back to JSON text           {back:>8.0}");
    Ok(())
}

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: encodings <dictionary directory>");
        return ExitCode::from(2);
    };
    match run(&path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{path}: {e}");
            ExitCode::FAILURE
        }
    }
}
