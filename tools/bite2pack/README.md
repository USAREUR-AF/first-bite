# bite2pack

Turns a first-bite dictionary into packs for the
[goat-transformers](https://github.com/USAREUR-AF/goat-transformers) engine.
A pack is the data file the engine reads to decode and encode one format; with
a pack for a dictionary's interface, the `xf` transform of `flow`
([flow/README.md](../../flow/README.md#the-xf-transform)) converts a record's
payload to any format that has a pack of its own, such as Cursor-on-Target XML
or GeoJSON.

| | |
|---|---|
| Runs | on your machine: Python 3 standard library, nothing else |
| Reads | dictionary directories, and a bindings file |
| Writes | `model/<dictionary>.def`, `packs/<format>/` (`pack.def`, `pack.md`, `vectors/`) and `index.json` under `--out` |
| Checks | its own output: it re-reads every file it wrote, runs every vector through a codec written from the engine specification, and compares the dictionary's own vectors with that codec. A failed check exits non-zero |
| Status | built; measured on the four dictionaries in `dictionaries/` |

## Use

Name one or more dictionary directories. The output directory is one the `xf`
transform loads with its `packs` key; the run below uses an example
dictionary, and a dictionary of your own is converted the same way:

```console
$ tools/bite2pack/bite2pack.py --out /etc/first-bite/packs --bindings flow/xf/bindings.json dictionaries/weather-station
bite-weather-station-wx      kinds  4  fields  23 mapped  23  vectors   9  dictionary vector fields  26:  25 equal,  1 equal in value only
self-check: passed
```

The format of an interface is `bite-<dictionary>-<interface>`, lower case with
`-` for every other character. `index.json` records which dictionary and
interface each format belongs to; the transform reads it to find the pack of a
record.

| Option | Meaning |
|---|---|
| `--out DIR` | where `model/`, `packs/` and `index.json` are written |
| `--bindings FILE` | which fields carry the shared keys; see below |
| `--core-def FILE` | the engine's `model/core.def`, read for the shared keys and their types; without it the tool uses the keys it was written against |
| `--report DIR` | also write `<dictionary>.map.json`: every field with its key, transform and options |
| `--check` | write nothing; exit 1 when `--out` is not what the dictionaries give |

## Bindings

A dictionary says where a field is and what unit it has. It does not say that
`latitude` is the latitude of the thing the message is about; a format such as
Cursor-on-Target needs to know. The bindings file states it, per message:

```json
{
  "weather-station": {
    "WX/Position": { "uid": "station_id", "lat": "latitude", "lon": "longitude", "hae": "altitude" }
  }
}
```

| Shared key | A field can carry it when it is |
|---|---|
| `uid`, `callsign` | text |
| `lat`, `lon` | a number with unit `deg` |
| `hae` | a number with unit `m` |

A binding the dictionary does not support (no such field, the wrong type, the
wrong unit) stops the run with the reason. A message with no binding still
gets a pack: it converts to its own format and back, and to another format it
carries only what the transform's `context` supplies.

## How a field is mapped

Every field becomes one `field` line, marked `optional`: `bite` decodes a
payload that was cut short up to the cut, and so does the pack.

| Dictionary field | In the pack | Value in the engine |
|---|---|---|
| `uint`, `int` | `uint`, `twos` | the integer |
| with `scale` or `offset` | `uscale`, `sscale` | the number |
| `float` | `f32`, `f64` | the number |
| `ascii` | `ascii trim` | the text, without trailing NULs and spaces |
| `bool`, 8 bits or fewer | `uint via <table>` | `true` or `false` |
| a field with `values`, 8 bits or fewer, no scale | `uint via <table>` | the label, or the decimal text of a value that has none |
| `bytes`, one byte | `uint via <table>` | two hexadecimal digits, as `bite` writes them |
| `bytes`, 2 to 8 bytes | `uint` | the integer the bytes spell |
| `byte_order: little` in an `msb0` interface | the same, with `swap` | |

A value table lists every value the field can hold, because a table that
turns an integer into text has no entry for "any other value". That is why
the rule stops at 8 bits (256 lines); a wider `bool` or labelled field is
carried as its integer. Three kinds of field are not carried in the form
`bite` writes them, and `pack.md` of each pack lists the ones it has:

| Field | Why |
|---|---|
| `bytes` of 2 to 8 bytes | the pack language has no transform to hexadecimal text; the value is the same bytes as an integer |
| `bytes` wider than 64 bits, `float` with a scale | not mapped: no transform reads them |
| a scaled field whose offset is not a whole number of steps | `bite` rounds to the decimal places of `scale` and `offset`; the engine does not, and the last digits of the double can differ |

An interface that identifies messages by `frame_id` gives a pack whose message
is four bytes of frame identifier followed by the payload. A published record
carries no frame identifier, so the transform refuses a record of such an
interface; the pack is for a caller that holds the identifier.

## The packs in this repository

`flow/xf/` holds the packs compiled into `flow` with the cargo feature
`goat-transformers`: one for each interface of the dictionaries in
`dictionaries/`, generated with `flow/xf/bindings.json`, and the target packs
`cot-xml` and `geojson` with the shared keys they use (`model/core.def`,
`model/track.def`), copied
unchanged from goat-transformers at the commit `Cargo.toml` pins
(`flow/xf/UPSTREAM`).

```bash
tools/bite2pack/refresh.sh                        # after a dictionary or the bindings changed
tools/bite2pack/refresh.sh --upstream <checkout>  # after the pinned commit changed
tools/bite2pack/refresh.sh --check                # what `make guards` and CI run
```

`refresh.sh` also rewrites `flow/src/xf_embedded.rs`, the list of the files
that are compiled in. `cargo test -p flow --features goat-transformers` then
runs every generated vector on the engine and compares the engine's decoding
of every fixture with `bite`'s.
