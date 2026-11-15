//! `mind goat …`: joining a goat data fabric. Optional.
//!
//! The commands and the dictionary hand-off exist only in a build with the
//! cargo feature `goat`. Without it this module is the two entry points
//! `main` calls, one of which says how to get the feature and the other of
//! which does nothing.

#[cfg(feature = "goat")]
mod cli;
#[cfg(feature = "goat")]
pub mod schema;

use std::ffi::OsString;
use std::process::ExitCode;

/// The first argument that selects these commands. A network interface of
/// the same name is still reachable as `mind -- goat`.
pub const COMMAND: &str = "goat";

/// When the process was started as `mind goat …`, run that command and return
/// its exit code. Any other invocation returns `None` and is parsed as before.
pub fn dispatch() -> Option<ExitCode> {
    let mut arguments = std::env::args_os();
    arguments.next()?;
    if arguments.next()? != COMMAND {
        return None;
    }
    Some(run(arguments.collect()))
}

#[cfg(feature = "goat")]
fn run(arguments: Vec<OsString>) -> ExitCode {
    cli::run(arguments)
}

#[cfg(not(feature = "goat"))]
fn run(_: Vec<OsString>) -> ExitCode {
    eprintln!(
        "this mind was built without goat support; build one with it: \
         cargo build --release -p mind --features goat"
    );
    ExitCode::from(2)
}

/// Give the goat sink the messages of the loaded dictionary, so it can
/// declare their schemas. Does nothing in a build without the feature.
pub fn register(dictionary: Option<&bite::Dictionary>) {
    #[cfg(feature = "goat")]
    flow::goat::register(schema::declarations(dictionary));
    #[cfg(not(feature = "goat"))]
    let _ = dictionary;
}
