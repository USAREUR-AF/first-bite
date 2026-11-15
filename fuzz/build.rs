//! Embed the shipped dictionaries in the fuzz targets, so that a target is one
//! file that runs anywhere. Writes `$OUT_DIR/shipped.rs`: a table of
//! (dictionary name, [(path relative to the dictionary, text)]).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

fn walk(dir: &Path, prefix: &str, files: &mut Vec<(String, PathBuf)>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            walk(&path, &format!("{prefix}{name}/"), files);
        } else if name.ends_with(".json") {
            files.push((format!("{prefix}{name}"), path));
        }
    }
}

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../dictionaries");
    let root = root.canonicalize().expect("the dictionaries directory");
    println!("cargo:rerun-if-changed={}", root.display());

    let mut dictionaries: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    dictionaries.sort();

    let mut out = String::from("pub const SHIPPED: &[(&str, &[(&str, &str)])] = &[\n");
    for dictionary in dictionaries {
        let mut files = Vec::new();
        walk(&dictionary, "", &mut files);
        let name = dictionary.file_name().unwrap().to_string_lossy();
        writeln!(out, "    ({name:?}, &[").unwrap();
        for (relative, path) in files {
            writeln!(
                out,
                "        ({relative:?}, include_str!({:?})),",
                path.display().to_string()
            )
            .unwrap();
        }
        out.push_str("    ]),\n");
    }
    out.push_str("];\n");
    let target = PathBuf::from(env::var("OUT_DIR").unwrap()).join("shipped.rs");
    fs::write(target, out).unwrap();
}
