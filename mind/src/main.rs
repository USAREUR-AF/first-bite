use clap::Parser;
use datamodels::packets::{get_log_metadata, PacketInfoWithParsedMessage};
use flow::{Publishers, SubPubs, Subscribers};
use log::{error, info};
use logger::loggers::setup_logger;
use mind::cli::Opt;
use mind::splash::print_splash_screen;
use mind::{
    CaptureOptions, Decapsulation, Error, FileCapture, FrameSource, LiveCapture, MultiCapture,
    OutputFile, Pipeline, UdpFrames,
};
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// ZeroMQ PUB drops messages sent before a subscriber has finished connecting.
/// A file is read faster than that takes, so file replay waits this long after
/// the publishers bind and again before exit.
const FILE_REPLAY_SETTLE: Duration = Duration::from_millis(500);

fn main() -> ExitCode {
    if let Some(code) = mind::goat::dispatch() {
        return code;
    }
    let opt = Opt::parse();
    // The env file may set values the logger reads, so it is loaded first and
    // its outcome reported once the logger exists.
    let env_file = opt.env_file.as_deref().map(load_environment_file);
    setup_logger(&get_log_metadata(Some(
        env!("CARGO_PKG_VERSION").to_string(),
    )));
    match env_file {
        Some(Ok(())) => info!("loaded env file {}", opt.env_file.as_deref().unwrap_or("")),
        // A missing env file is reported and tolerated: the process environment
        // may already carry everything.
        Some(Err(e)) => error!("{e}"),
        None => {}
    }
    print_splash_screen();

    match start(opt) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Why the process could not start or had to stop.
#[derive(Debug, thiserror::Error)]
enum StartError {
    #[error(transparent)]
    Capture(#[from] Error),
    #[error(transparent)]
    Egress(#[from] flow::Error),
}

fn start(opt: Opt) -> Result<(), StartError> {
    let dictionary = mind::dictionary::load(opt.dictionary.clone()).map_err(Error::from)?;
    mind::goat::register(dictionary.as_deref());
    let pipeline = Pipeline::new(dictionary).with_decapsulation(Decapsulation {
        keep_gre: opt.no_gre_decap,
        vxlan_ports: opt.vxlan_port.clone(),
    });
    let mut output = if opt.output {
        Some(OutputFile::create()?)
    } else {
        None
    };

    let replaying_file = opt.pcap_file.is_some();
    let mut source = open_source(&opt)?;

    // Publishers, subscribers and subpubs are built in full here, so a
    // configuration mistake in any of them stops the process before capture.
    let publishers = Publishers::load()?;
    let subscribers = Subscribers::load()?;
    if !subscribers.is_empty() {
        thread::spawn(move || {
            if let Err(e) = subscribers.run() {
                error!("subscribers stopped: {e}");
            }
        });
    }
    if opt.activate_subpubs {
        let subpubs = SubPubs::load()?;
        if !subpubs.is_empty() {
            thread::spawn(move || {
                if let Err(e) = subpubs.run() {
                    error!("subpubs stopped: {e}");
                }
            });
        }
    } else {
        info!("subpubs are off; pass --activate-subpubs and set SUBPUB_CONFIG_PATH to run them");
    }

    if replaying_file {
        thread::sleep(FILE_REPLAY_SETTLE);
    }
    info!("reading from {}", source.describe());
    let mut publish = |_: &PacketInfoWithParsedMessage, json: &str| publishers.publish(json);
    let stats_interval = (opt.stats_interval > 0).then(|| Duration::from_secs(opt.stats_interval));
    // SIGINT and SIGTERM end the loop cleanly, so the output file is flushed
    // and the final counts are logged when the service is stopped.
    let stop = Arc::new(AtomicBool::new(false));
    let handler_stop = Arc::clone(&stop);
    if let Err(e) = ctrlc::set_handler(move || handler_stop.store(true, Ordering::Relaxed)) {
        error!(
            "cannot install the signal handler; a stop signal will end the process at once: {e}"
        );
    }
    let started = Instant::now();
    let stats = mind::run(
        source.as_mut(),
        &pipeline,
        &mut publish,
        output.as_mut(),
        stats_interval,
        &stop,
    )?;
    let elapsed = started.elapsed();
    if replaying_file {
        thread::sleep(FILE_REPLAY_SETTLE);
    }
    info!(
        "done: {} frames, {} records, {} decoded, {} not UDP or TCP, {} malformed",
        stats.frames, stats.records, stats.decoded, stats.skipped, stats.malformed
    );
    if pipeline.unwrapped() > 0 {
        info!(
            "{} frames arrived inside a GRE, ERSPAN or VXLAN tunnel and were unwrapped",
            pipeline.unwrapped()
        );
    }
    if replaying_file && elapsed.as_secs_f64() > 0.0 {
        info!(
            "replayed in {:.3} s: {:.0} frames per second",
            elapsed.as_secs_f64(),
            stats.frames as f64 / elapsed.as_secs_f64()
        );
    }
    Ok(())
}

fn open_source(opt: &Opt) -> Result<Box<dyn FrameSource>, Error> {
    let filter = opt.filter.as_deref();
    if let Some(path) = &opt.pcap_file {
        return Ok(Box::new(FileCapture::open(path, filter)?));
    }
    if opt.mock {
        return Ok(Box::new(UdpFrames::bind(&opt.mock_address)?));
    }
    let options = CaptureOptions {
        promiscuous: !opt.no_promiscuous,
        filter: opt.filter.clone(),
        buffer_bytes: opt.buffer_mb.saturating_mul(1024 * 1024),
    };
    match opt.device_name.as_slice() {
        [] => Err(Error::NoSource),
        // One interface is read in place; several meet in a queue.
        [device] => Ok(Box::new(LiveCapture::open(device, &options)?)),
        devices => Ok(Box::new(MultiCapture::open(devices, &options)?)),
    }
}

fn load_environment_file(path: &str) -> Result<(), Error> {
    let fail = |reason: String| Error::EnvFile {
        path: path.to_string(),
        reason,
    };
    if !Path::new(path).exists() {
        return Err(fail("does not exist".to_string()));
    }
    if !path.ends_with(".env") {
        return Err(fail("name must end in .env".to_string()));
    }
    dotenvy::from_path(path).map_err(|e| fail(e.to_string()))
}
