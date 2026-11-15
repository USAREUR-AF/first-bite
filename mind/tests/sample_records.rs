//! `examples/weather-station.records.jsonl` is what `mind` publishes for
//! `examples/weather-station.pcap`: the test replays the capture and compares.
//!
//! To write the file again after the record or the capture changed:
//!
//! ```text
//! UPDATE_SAMPLE_RECORDS=1 cargo test -p mind --test sample_records
//! ```

use bite::Dictionary;
use mind::{FileCapture, FrameSource, Pipeline};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The records of the example capture, one JSON document per line.
fn replayed() -> Vec<String> {
    let dictionary = Dictionary::load(root().join("dictionaries/weather-station")).unwrap();
    let pipeline = Pipeline::new(Some(Arc::new(dictionary)));
    let mut source =
        FileCapture::open(&root().join("examples/weather-station.pcap"), None).unwrap();
    let mut lines = Vec::new();
    while let Some(frame) = source.next_frame().unwrap() {
        let record = pipeline.process(&frame).unwrap();
        lines.push(serde_json::to_string(&record).unwrap());
    }
    lines
}

#[test]
fn the_sample_file_is_what_mind_publishes_for_the_example_capture() {
    // The deployment's settings are copied into records; the sample has none.
    for (name, _) in std::env::vars() {
        assert!(
            !name.starts_with("DATA_SOURCE_") && !name.starts_with("DATA_PACKET_"),
            "{name} is set; unset it to run this test"
        );
    }
    let path = root().join("examples/weather-station.records.jsonl");
    let replayed = replayed();
    if std::env::var_os("UPDATE_SAMPLE_RECORDS").is_some() {
        std::fs::write(&path, replayed.join("\n") + "\n").unwrap();
    }
    let sample = std::fs::read_to_string(&path).unwrap();
    let sample: Vec<&str> = sample.lines().collect();
    assert_eq!(sample.len(), replayed.len(), "number of records");
    assert_eq!(sample.len(), 5);
    for (number, (sample, replayed)) in sample.iter().zip(&replayed).enumerate() {
        let mut sample: Value = serde_json::from_str(sample).unwrap();
        let replayed: Value = serde_json::from_str(replayed).unwrap();
        // The file keeps the version of the `mind` that wrote it.
        assert!(sample["application_version"].is_string());
        sample["application_version"] = replayed["application_version"].clone();
        assert_eq!(sample, replayed, "record {}", number + 1);
    }
    // The current record: typed values and numeric ports.
    let first: Value = serde_json::from_str(sample[0]).unwrap();
    assert_eq!(first["data_model_version"], "2.0.0");
    assert!(first["src_port"].is_u64());
}
