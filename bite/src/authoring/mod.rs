//! Tools for writing a dictionary from an interface control document and for
//! keeping it right: canonical formatting, conversion of the document's bit
//! addressing, a spreadsheet form, coverage, convention checks, a semantic
//! diff, payload encoding for test vectors, and statistics over payloads.
//!
//! Nothing here is used to decode. The `bite` command exposes each module as
//! a subcommand; `docs/icd-to-dictionary/` is the method they serve.
//!
//! | Module | Command |
//! |---|---|
//! | [`canon`] | `bite fmt` |
//! | [`range`] | `bite range` |
//! | [`table`], [`csv`] | `bite import-csv`, `bite export-csv` |
//! | [`coverage`] | `bite coverage` |
//! | [`lint`], [`rules`] | `bite lint` |
//! | [`diff`] | `bite diff` |
//! | [`encode`] | `bite vectors encode` |
//! | [`stats`] | `bite stats` |
//! | [`scaffold`] | `bite new`, `bite vectors new` |

pub mod canon;
pub mod coverage;
pub mod csv;
pub mod diff;
pub mod encode;
pub mod json;
pub mod lint;
pub mod model;
pub mod range;
pub mod rules;
pub mod scaffold;
pub mod stats;
pub mod table;

pub use json::Node;
pub use model::{FileSet, Tree};
