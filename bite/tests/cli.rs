//! The `bite` authoring subcommands, run as a user runs them.
#![cfg(feature = "cli")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn shipped(name: &str) -> String {
    repo().join("dictionaries").join(name).display().to_string()
}

/// A fresh, empty directory under the target directory.
fn scratch(label: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "bite-cli-{}-{label}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_dir(&path, &target);
        } else {
            fs::copy(&path, &target).unwrap();
        }
    }
}

/// Run `bite` and return its exit status and standard output.
fn bite(args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_bite"))
        .args(args)
        .output()
        .unwrap();
    (
        output.status.code().unwrap(),
        String::from_utf8(output.stdout).unwrap() + &String::from_utf8(output.stderr).unwrap(),
    )
}

fn ok(args: &[&str]) -> String {
    let (code, out) = bite(args);
    assert_eq!(code, 0, "bite {args:?}\n{out}");
    out
}

fn fails(args: &[&str]) -> String {
    let (code, out) = bite(args);
    assert_eq!(code, 1, "bite {args:?}\n{out}");
    out
}

#[test]
fn new_scaffolds_a_dictionary_that_passes_the_strict_check() {
    let dir = scratch("new").join("example");
    let path = dir.to_str().unwrap();
    let out = ok(&[
        "new",
        path,
        "--interface",
        "LINK",
        "--port",
        "4000",
        "--interface",
        "AUX",
    ]);
    assert!(
        out.contains("dictionary \"example\" with 2 interfaces"),
        "{out}"
    );
    for file in [
        "dictionary.json",
        "README.md",
        "LINK/metadata.json",
        "AUX/metadata.json",
    ] {
        assert!(dir.join(file).is_file(), "{file}");
    }
    assert!(dir.join("vectors").is_dir() && dir.join("LINK/messages").is_dir());
    assert_eq!(
        ok(&["check", "--strict", path]).trim(),
        "2 interfaces, 0 messages, 0 errors, 0 warnings, 0/0 vectors passed"
    );
    assert!(ok(&["fmt", "--check", path]).contains("3 files, 0 not canonical"));
    // It does not write into a directory that has something in it.
    let (code, out) = bite(&["new", path, "--interface", "LINK"]);
    assert_eq!(code, 2);
    assert!(out.contains("exists and is not empty"), "{out}");
}

#[test]
fn new_repo_instantiates_the_template() {
    let dir = scratch("repo").join("crane");
    let path = dir.to_str().unwrap();
    ok(&["new", path, "--repo", "--interface", "LINK"]);
    for file in [
        "README.md",
        "Makefile",
        ".gitignore",
        "CHANGELOG.md",
        "CODEOWNERS",
        "icd-notes/issues.md",
        "tables/README.md",
        ".github/workflows/check.yml",
        ".github/pull_request_template.md",
        "dictionary/dictionary.json",
        "dictionary/LINK/metadata.json",
    ] {
        assert!(dir.join(file).is_file(), "{file}");
    }
    let readme = fs::read_to_string(dir.join("README.md")).unwrap();
    assert!(readme.starts_with("# crane dictionary\n") && !readme.contains("{{NAME}}"));
    ok(&[
        "check",
        "--strict",
        dir.join("dictionary").to_str().unwrap(),
    ]);
}

#[test]
fn range_converts_both_ways_and_draws_the_word() {
    let out = ok(&[
        "range",
        "--word-bits",
        "16",
        "--word",
        "3",
        "--bits",
        "12-5",
        "--bit0",
        "lsb",
        "--first-word",
        "1",
    ]);
    assert_eq!(
        out,
        "start 35\nstop  43\nwidth 8 bits, under \"bit_order\": \"msb0\"\n\
         numbering: 16-bit words numbered from 1, sent most significant byte first; bit 0 is the least significant bit of a word\n\
         word 3 (payload bytes 4-5)\n  15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0\n   .  .  .  #  #  #  #  #  #  #  #  .  .  .  .  .\n"
    );
    let out = ok(&[
        "range",
        "--word-bits",
        "16",
        "--bit0",
        "lsb",
        "--first-word",
        "1",
        "--start",
        "35",
        "--stop",
        "43",
    ]);
    assert!(out.starts_with("word 3\nbits 12-5\nwidth 8 bits"), "{out}");
    // From an interface's own numbering, across words.
    let out = ok(&[
        "range",
        "--dictionary",
        &shipped("glasshouse"),
        "--interface",
        "ZONE",
        "--word",
        "7-8",
        "--bits",
        "15-8",
        "--json",
    ]);
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        (value["start"].as_u64(), value["stop"].as_u64()),
        (Some(96), Some(120))
    );
    let (code, out) = bite(&[
        "range",
        "--word-bits",
        "16",
        "--bit0",
        "lsb",
        "--first-word",
        "1",
        "--word",
        "3",
        "--bits",
        "16",
    ]);
    assert_eq!(code, 2);
    assert!(
        out.contains("bit 16: a 16-bit word has bits 0 to 15"),
        "{out}"
    );
}

#[test]
fn fmt_check_fails_until_fmt_has_run() {
    let dir = scratch("fmt");
    copy_dir(Path::new(&shipped("weather-station")), &dir);
    let path = dir.to_str().unwrap();
    let message = dir.join("WX/messages/Heartbeat.json");
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&message).unwrap()).unwrap();
    // serde_json writes keys in alphabetical order and two-space indentation.
    fs::write(&message, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    let out = fails(&["fmt", "--check", path]);
    assert!(
        out.contains("not canonical: ") && out.contains("Heartbeat.json"),
        "{out}"
    );
    assert!(out.contains("10 files, 1 not canonical"), "{out}");
    assert!(ok(&["fmt", path]).contains("10 files, 1 rewritten"));
    assert!(ok(&["fmt", path]).contains("10 files, 0 rewritten"));
    ok(&["fmt", "--check", path]);
    assert_eq!(
        fs::read_to_string(&message).unwrap(),
        fs::read_to_string(
            Path::new(&shipped("weather-station")).join("WX/messages/Heartbeat.json")
        )
        .unwrap()
    );
    // A repeated key is refused, not silently merged.
    fs::write(
        &message,
        r#"{"fields": {"a": {"start": 0, "stop": 8}, "a": {"start": 8, "stop": 16}}}"#,
    )
    .unwrap();
    let out = fails(&["fmt", path]);
    assert!(out.contains("key \"a\" appears twice"), "{out}");
}

#[test]
fn export_and_import_round_trip_through_files() {
    let dir = scratch("csv");
    let tables = dir.join("tables");
    let out = ok(&[
        "export-csv",
        &shipped("glasshouse"),
        "--out",
        tables.to_str().unwrap(),
    ]);
    assert!(out.contains("fields.csv (116 rows)"), "{out}");
    let rebuilt = dir.join("rebuilt");
    ok(&[
        "import-csv",
        tables.to_str().unwrap(),
        "--into",
        rebuilt.to_str().unwrap(),
    ]);
    for file in [
        "ZONE/metadata.json",
        "ZONE/messages/Climate Report.json",
        "SUPV/messages/Vent Command.json",
    ] {
        assert_eq!(
            fs::read(rebuilt.join(file)).unwrap(),
            fs::read(Path::new(&shipped("glasshouse")).join(file)).unwrap(),
            "{file}"
        );
    }
    // An unknown column is refused with the row it is on.
    let fields = tables.join("fields.csv");
    let text = fs::read_to_string(&fields).unwrap();
    fs::write(&fields, text.replacen(",unit,", ",units,", 1)).unwrap();
    let out = fails(&[
        "import-csv",
        tables.to_str().unwrap(),
        "--into",
        rebuilt.to_str().unwrap(),
    ]);
    assert!(
        out.contains("fields.csv: row 1: unknown column \"units\""),
        "{out}"
    );
    assert!(out.contains("nothing was written"), "{out}");
    // A message file the tables do not hold stops the import.
    fs::write(&fields, text).unwrap();
    fs::write(rebuilt.join("ZONE/messages/Stray.json"), "{\"fields\": {}}").unwrap();
    let out = fails(&[
        "import-csv",
        tables.to_str().unwrap(),
        "--into",
        rebuilt.to_str().unwrap(),
    ]);
    assert!(out.contains("Stray.json: not in the tables"), "{out}");
    let out = ok(&[
        "import-csv",
        tables.to_str().unwrap(),
        "--into",
        rebuilt.to_str().unwrap(),
        "--prune",
    ]);
    assert!(
        out.contains("deleted") && !rebuilt.join("ZONE/messages/Stray.json").exists(),
        "{out}"
    );
}

#[test]
fn coverage_fails_only_on_what_the_flags_require() {
    let can = shipped("vehicle-can");
    let out = ok(&["coverage", &can]);
    assert!(out.contains("POWERTRAIN/Engine Status: 56 bits to the last field, none declared, 55 covered by 6 fields, 1 gap, 1 vector\n  gap [39, 40) 1 bit\n"), "{out}");
    let out = fails(&["coverage", &can, "--require-full-coverage"]);
    assert!(
        out.contains("C001 POWERTRAIN/Engine Status: bits [39, 40) belong to no field (1 bit)"),
        "{out}"
    );
    assert!(out.contains("1 finding failing"), "{out}");
    fails(&["coverage", &can, "--require-source"]);
    ok(&["coverage", &can, "--require-vectors", "--require-defined"]);
    ok(&["coverage", "--strict", &shipped("glasshouse")]);
    let json: serde_json::Value = serde_json::from_str(&ok(&["coverage", &can, "--json"])).unwrap();
    assert_eq!(
        json["interfaces"][0]["messages"][0]["gaps"],
        serde_json::json!([[39, 40]])
    );
    assert_eq!(json["failing"], serde_json::json!([]));
}

#[test]
fn lint_lists_its_rules_and_fails_on_a_finding() {
    let rules = ok(&["lint", "--rules"]);
    assert!(
        rules.starts_with("N001  An interface name is upper-case"),
        "{rules}"
    );
    assert_eq!(rules.lines().count(), bite::authoring::rules::RULES.len());
    assert!(ok(&["lint", "--units"]).contains("degC    degree Celsius"));
    for name in ["glasshouse", "toot", "vehicle-can", "weather-station"] {
        ok(&["lint", &shipped(name)]);
        ok(&["check", "--strict", &shipped(name)]);
        ok(&["fmt", "--check", &shipped(name)]);
        ok(&["coverage", &shipped(name)]);
    }
    // Without its allowances the weather station has findings, and an
    // allowance that matches nothing is itself one.
    let dir = scratch("lint");
    copy_dir(Path::new(&shipped("weather-station")), &dir);
    let file = dir.join("dictionary.json");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, text.replace("\"P001\"", "\"U001\"")).unwrap();
    let out = fails(&["lint", dir.to_str().unwrap()]);
    assert!(
        out.contains("P001 WX: no source document and revision"),
        "{out}"
    );
    assert!(
        out.contains("X002 dictionary.json: lint.allow entry for U001 matches nothing; remove it"),
        "{out}"
    );
    assert!(
        out.contains("2 findings, 9 allowed in dictionary.json"),
        "{out}"
    );
    let json: serde_json::Value =
        serde_json::from_str(&bite(&["lint", "--json", dir.to_str().unwrap()]).1).unwrap();
    assert_eq!(json["findings"][0]["rule"], "P001");
}

#[test]
fn check_strict_honours_an_allowance_and_nothing_else() {
    let out = ok(&["check", "--strict", &shipped("toot")]);
    assert!(
        out.contains("allowed: FAE/None: named by message_type but has no file"),
        "{out}"
    );
    assert!(
        out.contains("0 warnings (1 allowed in dictionary.json)"),
        "{out}"
    );
    let dir = scratch("strict");
    copy_dir(Path::new(&shipped("toot")), &dir);
    fs::remove_file(dir.join("dictionary.json")).unwrap();
    let out = fails(&["check", "--strict", dir.to_str().unwrap()]);
    assert!(
        out.contains("1 interfaces, 1 messages, 0 errors, 1 warnings, 1/1 vectors passed"),
        "{out}"
    );
    ok(&["check", dir.to_str().unwrap()]);
}

#[test]
fn diff_exit_status_says_whether_consumers_break() {
    let old = shipped("glasshouse");
    assert!(ok(&["diff", &old, &old]).starts_with("no changes\n"));
    let dir = scratch("diff");
    copy_dir(Path::new(&old), &dir);
    let new = dir.to_str().unwrap();
    // A label added: additive.
    let file = dir.join("ZONE/messages/Alarm.json");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(
        &file,
        text.replace(
            "\"7\": \"Power Supply\"",
            "\"7\": \"Power Supply\",\n\"8\": \"Low Battery\"",
        ),
    )
    .unwrap();
    let out = ok(&["diff", &old, new]);
    assert!(out.contains("additive (1)\n  relabelled ZONE/Alarm/alarm_code: value 8: label \"Low Battery\" added"), "{out}");
    assert!(
        out.contains("the changes ask for a new minor version: NOT sufficient"),
        "{out}"
    );
    fails(&["diff", "--check-version", &old, new]);
    // A field removed: breaking.
    let text = fs::read_to_string(&file).unwrap();
    let start = text.find("\"occurrence_count\"").unwrap();
    let end = text.find("\"minutes_since_raised\"").unwrap();
    fs::write(&file, format!("{}{}", &text[..start], &text[end..])).unwrap();
    let out = fails(&["diff", &old, new]);
    assert!(
        out.contains(
            "breaking (1)\n  removed    ZONE/Alarm/occurrence_count: field removed from [80, 96)"
        ),
        "{out}"
    );
    let json: serde_json::Value =
        serde_json::from_str(&bite(&["diff", "--json", &old, new]).1).unwrap();
    assert_eq!(json["breaking"], true);
    assert_eq!(json["required_bump"], "major");
    // With the version raised, --check-version passes though the change breaks.
    let file = dir.join("dictionary.json");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, text.replace("\"1.0.0\"", "\"2.0.0\"")).unwrap();
    ok(&["diff", "--check-version", &old, new]);
    fails(&["diff", &old, new]);
}

#[test]
fn a_vector_is_scaffolded_filled_encoded_and_checked() {
    let dir = scratch("vectors");
    copy_dir(Path::new(&shipped("glasshouse")), &dir);
    let path = dir.to_str().unwrap();
    let out = ok(&[
        "vectors",
        "new",
        path,
        "--interface",
        "SUPV",
        "--message",
        "Vent Command",
        "--name",
        "storm",
    ]);
    assert!(out.contains("7 expected values are empty"), "{out}");
    let file = dir.join("vectors/storm.json");
    let scaffold = fs::read_to_string(&file).unwrap();
    assert!(
        scaffold.contains("\"message_id\": \"3\"") && scaffold.contains("\"spare_84\": \"0\""),
        "{scaffold}"
    );
    // Unfilled, it fails the check and cannot be encoded.
    assert!(fails(&["check", path]).contains("vector storm: "));
    let (code, out) = bite(&["vectors", "encode", path, "--vector", "storm"]);
    assert_eq!(code, 2);
    assert!(out.contains("no value filled in"), "{out}");
    // Fill it in as the document would have it.
    let filled = scaffold
        .replace("\"message_length\": \"\"", "\"message_length\": \"6\"")
        .replace("\"zone_number\": \"\"", "\"zone_number\": \"9\"")
        .replace("\"sequence_number\": \"\"", "\"sequence_number\": \"1\"")
        .replace("\"time_of_day\": \"\"", "\"time_of_day\": \"1000\"")
        .replace(
            "\"windward_vent_target\": \"\"",
            "\"windward_vent_target\": \"0\"",
        )
        .replace(
            "\"leeward_vent_target\": \"\"",
            "\"leeward_vent_target\": \"0\"",
        )
        .replace(
            "\"vent_command_mode\": \"\"",
            "\"vent_command_mode\": \"Storm Close\"",
        )
        .replace("\"note\": \"\"", "\"note\": \"Made up for a test.\"");
    fs::write(&file, filled).unwrap();
    let out = ok(&["vectors", "encode", path, "--vector", "storm", "--write"]);
    assert!(
        out.contains("payload_hex 03062401000003e800003000"),
        "{out}"
    );
    assert!(
        out.contains("vent_command_mode     [80, 84)      0x3                 Storm Close"),
        "{out}"
    );
    assert!(ok(&["check", "--strict", path]).contains("11/11 vectors passed"));
    ok(&["fmt", "--check", path]);
    ok(&["lint", path]);
    // Without --write it compares, and says which byte differs.
    ok(&["vectors", "encode", path, "--vector", "storm"]);
    let text = fs::read_to_string(&file).unwrap();
    fs::write(
        &file,
        text.replace("03062401000003e800003000", "03062401000003e800003001"),
    )
    .unwrap();
    let out = fails(&["vectors", "encode", path, "--vector", "storm"]);
    assert!(
        out.contains("byte 11: built 00, vector 01 (spare_84)"),
        "{out}"
    );
    // From the command line, with a value the field cannot hold.
    let out = ok(&[
        "vectors",
        "encode",
        path,
        "--interface",
        "ZONE",
        "--message",
        "Climate Report",
        "--set",
        "air_temperature=-2.5",
        "--set",
        "outside_temperature=-7.35",
    ]);
    assert!(
        out.contains("payload_hex 0200000000000000ff06000076d0000000000000"),
        "{out}"
    );
    let out = fails(&[
        "vectors",
        "encode",
        path,
        "--interface",
        "ZONE",
        "--message",
        "Climate Report",
        "--set",
        "leaf_wetness=99",
    ]);
    assert!(
        out.contains("leaf_wetness: raw value 99 does not fit an unsigned 6-bit field"),
        "{out}"
    );
}

#[test]
fn stats_reads_payload_lines() {
    let dir = scratch("stats");
    let payloads = dir.join("payloads.txt");
    fs::write(
        &payloads,
        "; three heartbeats and a type nobody names\n01000c4b5837000001518060\n01000c4b5837000001518160\n01000c4b5837000001518220\n2a0003\n",
    )
    .unwrap();
    let out = ok(&[
        "stats",
        &shipped("weather-station"),
        "--port",
        "40100",
        "--payloads",
        payloads.to_str().unwrap(),
    ]);
    assert!(out.starts_with("WX: 4 payloads\n  type 42: 1 payload, not named by the interface\nWX/Heartbeat: 3 payloads (3 of 12 bytes)\n"), "{out}");
    assert!(
        out.ends_with("1 to look at\n  WX/Heartbeat/battery_low: zero in every payload\n"),
        "{out}"
    );
    let json: serde_json::Value = serde_json::from_str(&ok(&[
        "stats",
        &shipped("weather-station"),
        "--interface",
        "WX",
        "--payloads",
        payloads.to_str().unwrap(),
        "--json",
    ]))
    .unwrap();
    assert_eq!(json["messages"][0]["count"], 3);
    assert_eq!(json["unknown_types"]["42"], 1);
}
