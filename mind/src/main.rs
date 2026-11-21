use clap::Parser;
use datamodels::packets::{
    get_log_metadata, PacketInfoWithParsedMessage, RecordVersion, RECORD_VERSION_ENV,
};
use flow::{Publishers, SubPubs, Subscribers};
use log::{error, info, warn};
use logger::loggers::setup_logger;
use mind::cli::{Command, Opt};
use mind::slices::{
    expand_key, PublishOptions, SliceOptions, SlicePublisher, SliceWriter, TeeSource,
};
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
    if let Some(Command::Security(security)) = &opt.command {
        return mind::security::run(security, load_environment_file);
    }
    if let Some(Command::Discover(discover)) = &opt.command {
        return mind::discover::run(discover);
    }
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
        Ok(true) => ExitCode::SUCCESS,
        // Every frame was read; a sink did not deliver every record.
        Ok(false) => ExitCode::from(EXIT_NOT_DELIVERED),
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

/// Exit status when the run itself succeeded and a sink abandoned records or
/// delivered none of those it was given.
const EXIT_NOT_DELIVERED: u8 = 3;

/// How long the start waits for the subscribers of this process to subscribe
/// to the record socket before the first record is published.
const SUBSCRIBE_WAIT: Duration = Duration::from_secs(5);

/// Returns whether every sink delivered what was published for it.
fn start(opt: Opt) -> Result<bool, StartError> {
    // The security block is read and the crypto provider installed before
    // anything can open a connection.
    flow::security::activate().map_err(flow::Error::from)?;
    if opt.tap_only {
        return Ok(tap(&opt)?);
    }
    let dictionary = mind::dictionary::load(opt.dictionary.clone()).map_err(Error::from)?;
    let version = RecordVersion::from_env().map_err(Error::RecordVersion)?;
    mind::goat::register(dictionary.as_deref(), version);
    if version == RecordVersion::Text {
        info!(
            "publishing version 1 records ({RECORD_VERSION_ENV}=1): field values and ports are text"
        );
    }
    let loaded = dictionary.clone();
    let pipeline = Pipeline::new(dictionary)
        .with_record_version(version)
        .with_decapsulation(Decapsulation {
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
    let (writer, slice_publisher) = slices(&opt)?;
    if let Some(writer) = writer {
        source = Box::new(TeeSource::new(source, writer));
    }

    // Publishers, subscribers and subpubs are built in full here, so a
    // configuration mistake in any of them stops the process before capture.
    let mut publishers = Publishers::load()?;
    let subscribers = Subscribers::load()?;
    if let Some(dictionary) = loaded.as_deref() {
        let unmatched =
            mind::topics::unmatched(dictionary, &publishers.topic_rules(), &subscribers.topics());
        for entry in unmatched {
            warn!("{entry}");
        }
    }
    let running = subscribers.start()?;
    publishers.feed(running.tracker());
    let tracker = running.tracker();
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

    // A record published before a subscriber has subscribed is discarded by
    // the socket, so the subscribers of this process are waited for. A
    // consumer in another process cannot be: file replay gives it a moment.
    if !publishers.wait_for_subscribers(SUBSCRIBE_WAIT) {
        warn!(
            "not every subscriber had subscribed to the record socket after {SUBSCRIBE_WAIT:?}; \
             the first records may not reach it"
        );
    }
    if replaying_file {
        thread::sleep(FILE_REPLAY_SETTLE);
    }
    info!("reading from {}", source.describe());
    let mut publish = |_: &PacketInfoWithParsedMessage, json: &str| {
        publishers.publish(json);
        // A file is read faster than a sink delivers; live capture is never
        // held up.
        if replaying_file {
            tracker.pace();
        }
    };
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
    // A source that fails ends the input like any other end: what was
    // published before it is still delivered, and the failure is reported
    // after.
    let outcome = mind::run(
        source.as_mut(),
        &pipeline,
        &mut publish,
        output.as_mut(),
        stats_interval,
        &stop,
    );
    let elapsed = started.elapsed();
    // Dropping the source closes the last pcap slice and queues it before
    // the publisher drains.
    drop(source);
    finish_publisher(slice_publisher, Duration::from_secs(opt.drain_seconds));

    // Input has ended, or the process was told to stop. The sinks deliver
    // what was already published, then the record socket is closed with time
    // for a consumer in another process to take what is queued for it.
    let drain = Duration::from_secs(opt.drain_seconds);
    if !running.is_empty() {
        info!("waiting up to {drain:?} for the sinks to deliver what was published");
    }
    let reports = running.drain(drain);
    publishers.close(FILE_REPLAY_SETTLE.max(Duration::from_millis(100)));
    let mut complete = true;
    for report in &reports {
        if report.incomplete() {
            complete = false;
            error!("{report}");
        } else {
            info!("{report}");
        }
    }
    let stats = outcome?;
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
    Ok(complete)
}

/// The pcap slice writer and its publisher, when --pcap-dir is given.
fn slices(opt: &Opt) -> Result<(Option<SliceWriter>, Option<SlicePublisher>), Error> {
    let Some(dir) = &opt.pcap_dir else {
        return Ok((None, None));
    };
    if opt.pcap_keep == 0 {
        return Err(Error::Slices("--pcap-keep must be at least 1".to_string()));
    }
    let publisher = match (&opt.pcap_publish, &opt.pcap_key) {
        (Some(url), Some(key)) => {
            let key = expand_key(key);
            let publisher = SlicePublisher::spawn(PublishOptions {
                url: url.clone(),
                key: key.clone(),
                retries: opt.pcap_publish_retries,
                first_wait: Duration::from_secs(2),
            })
            .map_err(Error::Slices)?;
            info!("publishing each finished pcap slice to key {key} through {url}");
            Some(publisher)
        }
        _ => None,
    };
    let writer = SliceWriter::new(
        SliceOptions {
            dir: dir.clone(),
            prefix: opt.pcap_prefix.clone(),
            seconds: (opt.pcap_slice_seconds > 0).then_some(opt.pcap_slice_seconds),
            max_bytes: opt.pcap_slice_mb.map(|mb| mb.saturating_mul(1024 * 1024)),
            keep: opt.pcap_keep,
        },
        publisher.as_ref().map(SlicePublisher::sender),
    )
    .map_err(|e| Error::Slices(format!("{}: {e}", dir.display())))?;
    Ok((Some(writer), publisher))
}

fn finish_publisher(publisher: Option<SlicePublisher>, drain: Duration) {
    let Some(publisher) = publisher else {
        return;
    };
    match publisher.finish(drain) {
        Some(counts) => info!(
            "pcap slices: {} published, {} not published",
            counts.published, counts.failed
        ),
        None => {
            warn!("pcap slices still queued after {drain:?} were not published; they stay on disk")
        }
    }
}

/// `--tap-only`: write and publish pcap slices, nothing else.
fn tap(opt: &Opt) -> Result<bool, Error> {
    let source = open_source(opt)?;
    let (writer, publisher) = slices(opt)?;
    let writer = writer.ok_or_else(|| Error::Slices("--tap-only needs --pcap-dir".to_string()))?;
    let stop = Arc::new(AtomicBool::new(false));
    let handler_stop = Arc::clone(&stop);
    if let Err(e) = ctrlc::set_handler(move || handler_stop.store(true, Ordering::Relaxed)) {
        error!(
            "cannot install the signal handler; a stop signal will end the process at once: {e}"
        );
    }
    let mut frames: u64 = 0;
    {
        let mut tee = TeeSource::new(source, writer);
        info!("tap only: reading from {}", tee.describe());
        while !stop.load(Ordering::Relaxed) {
            if tee.next_frame()?.is_some() {
                frames += 1;
            } else if tee.finished() {
                break;
            }
        }
        if let Err(e) = tee.finish() {
            error!("cannot close the last pcap slice: {e}");
        }
    }
    info!("tap only: {frames} frames written to pcap slices");
    finish_publisher(publisher, Duration::from_secs(opt.drain_seconds));
    Ok(true)
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
