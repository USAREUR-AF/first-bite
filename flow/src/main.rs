//! Run the configured subscribers without the capture process, against
//! publishers that are already running.

use flow::Subscribers;
use std::process::ExitCode;

fn main() -> ExitCode {
    match Subscribers::load().and_then(Subscribers::run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
