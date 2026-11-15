# bite-wasm

The [`bite`](../bite) decoder as a WebAssembly module, for a web page that
decodes messages with no server behind it. It is the same library `mind` and
the `bite` command use: a dictionary goes in as file paths and text, and
payloads are decoded in the browser.

[`site/decoder/`](../site/decoder) is a page built on it.

## Build

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version <the wasm-bindgen version in Cargo.lock> --locked
make wasm
```

`make wasm` checks that the `wasm-bindgen` command matches the crate version in
`Cargo.lock` and prints the install command when it does not. It writes to
`site/` (none of it is committed):

| File | What |
|---|---|
| `site/pkg/bite_wasm_bg.wasm` | the module, about 270 kB (about 105 kB gzipped) |
| `site/pkg/bite_wasm.js` | an ES module that loads it and exports the interface below |
| `site/pkg/bite_wasm.d.ts` | TypeScript declarations |
| `site/dictionaries.json` | the dictionaries in `dictionaries/`, bundled for the page by `tools/bundle-dictionaries.py` |

To see the page, serve `site/` with any static file server and open
`/decoder/`:

```bash
python3 -m http.server --directory site 8000
```

## JavaScript interface

```js
import init, { loadDictionary } from "./pkg/bite_wasm.js";

await init(); // fetches and compiles bite_wasm_bg.wasm, once

const dictionary = loadDictionary({
  "LINK/metadata.json": JSON.stringify({
    port: 9000,
    message_type: { start: 0, stop: 8, values: { "1": "Ping" } },
  }),
  "LINK/messages/Ping.json": JSON.stringify({
    fields: { sequence: { start: 8, stop: 24 } },
  }),
});

dictionary.decode("LINK", "01 00 2a");
// { interface: "LINK", message_type: "Ping", fields: { sequence: 42 } }
```

`loadDictionary(files)` takes an object of file path to file text, in the
[dictionary layout](../docs/dictionary-format.md#layout) with `/` between path
components, and returns a dictionary. A folder name in front of every path, as
a browser's folder picker gives, is dropped. Files outside the layout are
ignored.

| Method | Returns |
|---|---|
| `info()` | `{name, version, description}` from `dictionary.json`; each is `null` when absent |
| `interfaces()` | one object per interface: `name`, `port`, `description`, `bit_order`, `byte_order`, `message_type` (`source`, `start`, `stop`, `mask`) and `messages`. Each message has `name`, `ids` (the identifiers that select it), `defined` (whether it has a file), `description`, `bit_length` and `fields`; each field has `name`, `start`, `stop`, `type`, `byte_order`, `scale`, `offset`, `unit`, `description` and `labelled` |
| `issues()` | what `bite check` reports: `{severity, location, message, text}` each, `text` being the line `bite check` prints |
| `decode(interface, payloadHex, frameId?)` | `{interface, message_type, fields}`: the JSON `bite decode` prints. A field is the dictionary's label, or the typed value, or `null` beyond the end of the payload |
| `decodeByPort(port, payloadHex, frameId?)` | the same, for the interface that claims the port |
| `schema()` | JSON Schema for the decoded form of every message, keyed `interface/message`: what `bite schema` prints |
| `vectors()` | the test vectors the dictionary carries: `{name, interface, payload_hex, frame_id, expect}`, or `{name, error}` for a file that is not a vector |
| `runVectors()` | `{name, passed, failures}` for each vector |

A mistake is thrown as an `Error` whose message is what the `bite` command
prints for it: `no interface named "X"`, `no interface claims port 1`,
`payload: Odd number of digits`.

Numbers are JavaScript numbers, so an integer field above 2^53 is rounded, and
`frameId` must be an integer no larger than that. A dictionary holds memory in
the module until `dictionary.free()` is called.

## Code

| File | What |
|---|---|
| [`src/api.rs`](src/api.rs) | all of the behaviour, in plain Rust with JSON values in and out, and its tests (`cargo test -p bite-wasm`) |
| [`src/bindings.rs`](src/bindings.rs) | the `wasm-bindgen` exports; compiled only for `wasm32` |
