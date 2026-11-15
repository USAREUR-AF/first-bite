# Fuzz targets

first-bite reads three kinds of input it does not control: payloads off the
wire, dictionaries supplied by whoever runs it (in a browser, by whoever opens
the page), and captured frames. Each has a [libFuzzer](https://llvm.org/docs/LibFuzzer.html)
target here, built with [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz).

| Target | Input | What it runs |
|---|---|---|
| `decode_payload` | 8 bytes of frame identifier, then a payload | every interface of every shipped dictionary: message type, `decode`, `decode_frame`, every message's fields, rendering and JSON |
| `load_dictionary` | NUL-separated path and text pairs, then a payload | `Dictionary::from_files`, the issues, the JSON Schemas, the test vectors, then decoding the payload with the dictionary that loaded |
| `frame_pipeline` | one byte of options, then a frame | `mind::Pipeline::process`: Ethernet or raw IP, GRE, ERSPAN types I to III and VXLAN unwrapping, transport location, lookup by port, decoding, the published record |

A finding is a panic, an arithmetic overflow, a memory error reported by
AddressSanitizer, an input that takes more than ten seconds, or one that needs
more than 2 GB.

## Running a target

The targets need a nightly toolchain; the rest of the repository builds with
stable (`rust-toolchain.toml`), and `+nightly` overrides that file for one
command.

```
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
python3 fuzz/seeds.py
cd fuzz
cargo +nightly fuzz run decode_payload -- -max_total_time=300
cargo +nightly fuzz run load_dictionary -- -max_total_time=300 -max_len=16384 -dict=dicts/load_dictionary.dict
cargo +nightly fuzz run frame_pipeline -- -max_total_time=300
```

`seeds.py` writes the seed corpus into `fuzz/corpus/` from the test vectors of
the shipped dictionaries and from `examples/weather-station.pcap`. The corpus
is generated and not committed. An input that crashes a target is written to
`fuzz/artifacts/<target>/`; `cargo +nightly fuzz tmin <target> <file>` reduces
it, and the reduced input belongs in a regression test beside the fix
(`bite/tests/robustness.rs`, `bite/tests/hostile_dictionary.rs`).

`frame_pipeline` links `mind`, so it needs what `mind` needs to build: the
libpcap headers and CMake.

## Layout

This directory is its own Cargo workspace with its own `Cargo.lock`, and the
root workspace excludes it: `cargo build --workspace`, `cargo test --workspace`
and `cargo deny` do not see it. `build.rs` embeds the shipped dictionaries in
the targets, so a target is one file that runs anywhere.

## In CI

[ClusterFuzzLite](https://google.github.io/clusterfuzzlite/) builds the targets
with `.clusterfuzzlite/build.sh` in the image of `.clusterfuzzlite/Dockerfile`.

| Workflow | Runs | Time |
|---|---|---|
| `.github/workflows/cflite_pr.yml` | pull requests that change code, dictionaries or the fuzz set-up | 5 minutes, on the targets the change affects |
| `.github/workflows/cflite_batch.yml` | every night, and pushes to `main` that change code | 30 minutes over all targets; the corpus is kept for the next run |
