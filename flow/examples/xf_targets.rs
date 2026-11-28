//! Convert every vector of a directory of packs to every target of the `xf`
//! transform, and print one line per vector and target:
//!
//! ```text
//! <target> TAB <format> TAB <vector> TAB converted | refused: <why>
//! ```
//!
//! For packs kept outside this repository, as `tools/bite2pack` writes them
//! (`model/`, `packs/`, `index.json`). The directory's own check compares the
//! lines with the ones it recorded; nothing of the packs is printed but their
//! format names, the vector file names and the keys a target lacks.
//!
//! ```sh
//! cargo run -p flow --features goat-transformers --example xf_targets -- <packs directory>
//! ```
//!
//! Each target is given the context `tests/xf_targets.rs` gives it. Exits 1
//! when a conversion fails for any reason other than a key the message does
//! not carry.

use base64::Engine as _;
use chrono::{TimeZone, Utc};
use flow::xf::{Xf, XfConfig, XfError};
use std::path::Path;
use std::process::ExitCode;

const TARGETS: [&str; 5] = [
    "cot-xml",
    "geojson",
    "canonical-track-v2",
    "asterix-cat062",
    "sapient",
];

/// As `context` in `tests/xf_targets.rs`.
fn config(to: &str, from: &str, packs: &str) -> XfConfig {
    let (kind, context) = match to {
        "cot-xml" => (None, serde_json::json!({"type": "a-f-G", "how": "m-g"})),
        "canonical-track-v2" => (
            Some("Track"),
            serde_json::json!({"track.source_id": "first-bite", "track.generation": 1}),
        ),
        "asterix-cat062" => (
            Some("track"),
            serde_json::json!({"cat062.sac": 25, "cat062.sic": 4, "track.confirmed": 1}),
        ),
        "sapient" => (Some("detection_report"), serde_json::json!({})),
        _ => (None, serde_json::json!({})),
    };
    serde_json::from_value(serde_json::json!({
        "to": to,
        "from": from,
        "kind": kind,
        "context": context,
        "require": ["uid", "lat", "lon"],
        "packs": packs,
    }))
    .expect("a valid configuration")
}

fn run(packs: &str) -> Result<bool, String> {
    let dir = Path::new(packs);
    let read =
        |path: &Path| std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()));
    let index: Vec<serde_json::Value> =
        serde_json::from_str(&read(&dir.join("index.json"))?).map_err(|e| e.to_string())?;
    let now = Utc.with_ymd_and_hms(2026, 3, 4, 12, 0, 0).unwrap();
    let mut clean = true;
    for to in TARGETS {
        for entry in &index {
            let format = entry["format"].as_str().ok_or("index.json: no format")?;
            let xf = Xf::from_config(&config(to, format, packs)).map_err(|e| e.to_string())?;
            let vectors = dir.join("packs").join(format).join("vectors");
            let mut files: Vec<_> = std::fs::read_dir(&vectors)
                .map_err(|e| format!("{}: {e}", vectors.display()))?
                .flatten()
                .map(|entry| entry.path())
                .collect();
            files.sort();
            for file in files {
                let vector: serde_json::Value =
                    serde_json::from_str(&read(&file)?).map_err(|e| e.to_string())?;
                if vector.get("expect_error").is_some() {
                    continue;
                }
                let input = vector["input"].as_str().unwrap_or_default();
                let bytes = hex::decode(input).map_err(|e| format!("{}: {e}", file.display()))?;
                let payload = base64::engine::general_purpose::STANDARD.encode(bytes);
                let record = serde_json::json!({ "data_encoded_inner": payload }).to_string();
                let outcome = match xf.convert_at(&record, now) {
                    Ok(_) => "converted".to_string(),
                    Err(e @ (XfError::Missing { .. } | XfError::Needs { .. })) => {
                        format!("refused: {e}")
                    }
                    Err(e) => {
                        clean = false;
                        format!("failed: {e}")
                    }
                };
                let name = file.file_name().unwrap_or_default().to_string_lossy();
                println!("{to}\t{format}\t{name}\t{outcome}");
            }
        }
    }
    Ok(clean)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [packs] = args.as_slice() else {
        eprintln!("usage: xf_targets <packs directory>");
        return ExitCode::from(2);
    };
    match run(packs) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("xf_targets: {e}");
            ExitCode::from(2)
        }
    }
}
