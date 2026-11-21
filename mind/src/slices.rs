//! Capture to rotating pcap files ("slices") and publish each finished slice.
//!
//! Every frame `mind` reads, after the BPF filter, is written to the current
//! slice under `--pcap-dir`. A slice is closed when it has covered
//! `--pcap-slice-seconds` or would pass `--pcap-slice-mb`, and the newest
//! `--pcap-keep` finished slices are kept; older ones are deleted. A slice
//! being written ends in `.pcap.part` and is renamed to `.pcap` when it is
//! closed, so nothing reads a half-written file.
//!
//! With `--pcap-publish`, each finished slice is sent with an HTTP `PUT` to
//! `<url>/<key>`, the REST interface of a Zenoh router (a local island), with
//! content type `application/vnd.tcpdump.pcap`. A failed `PUT` is retried with
//! a doubling wait; a slice that still fails stays on disk, in the ring.

use crate::source::{CaptureStats, Frame, FrameSource, LinkType, SourceError};
use chrono::{DateTime, Utc};
use log::{info, warn};
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The content type a published slice carries.
pub const PCAP_CONTENT_TYPE: &str = "application/vnd.tcpdump.pcap";
/// The snapshot length written in each file header: frames are written whole.
const SNAPLEN: u32 = 262_144;
const PART: &str = ".part";

/// How slices are cut and how many are kept.
#[derive(Debug, Clone)]
pub struct SliceOptions {
    pub dir: PathBuf,
    /// File names are `<prefix>-<UTC time>-<sequence>.pcap`.
    pub prefix: String,
    /// Close a slice once it has covered this long. `None`: no time limit.
    pub seconds: Option<u64>,
    /// Close a slice before it would pass this many bytes. `None`: no limit.
    pub max_bytes: Option<u64>,
    /// Finished slices kept on disk; older ones are deleted.
    pub keep: usize,
}

struct OpenSlice {
    file: BufWriter<File>,
    part: PathBuf,
    path: PathBuf,
    started: SystemTime,
    bytes: u64,
    link: LinkType,
    frames: u64,
}

/// Writes frames to the current slice and rotates.
pub struct SliceWriter {
    options: SliceOptions,
    current: Option<OpenSlice>,
    sequence: u64,
    finished: Option<Sender<PathBuf>>,
}

fn network(link: LinkType) -> u32 {
    match link {
        LinkType::Ethernet => 1,
        LinkType::RawIp => 101,
    }
}

impl SliceWriter {
    /// Create the directory if needed. `finished` receives the path of every
    /// slice that is closed, for a publisher.
    pub fn new(options: SliceOptions, finished: Option<Sender<PathBuf>>) -> io::Result<Self> {
        fs::create_dir_all(&options.dir)?;
        Ok(SliceWriter {
            options,
            current: None,
            sequence: 0,
            finished,
        })
    }

    pub fn options(&self) -> &SliceOptions {
        &self.options
    }

    /// Write one frame, opening or rotating the slice first when needed. A
    /// slice is cut by the frames' own timestamps, so a replayed file is cut
    /// the same way every time.
    pub fn write(&mut self, frame: &Frame<'_>) -> io::Result<()> {
        let record = 16 + frame.data.len() as u64;
        if let Some(open) = &self.current {
            let too_old = self.options.seconds.is_some_and(|seconds| {
                frame
                    .timestamp
                    .duration_since(open.started)
                    .is_ok_and(|age| age >= Duration::from_secs(seconds))
            });
            let too_big = self
                .options
                .max_bytes
                .is_some_and(|max| open.frames > 0 && open.bytes + record > max);
            if too_old || too_big || open.link != frame.link {
                self.finish()?;
            }
        }
        if self.current.is_none() {
            self.open(frame.timestamp, frame.link)?;
        }
        let open = self.current.as_mut().expect("opened above");
        let since = frame
            .timestamp
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let length = frame.data.len() as u32;
        open.file
            .write_all(&(since.as_secs() as u32).to_le_bytes())?;
        open.file.write_all(&since.subsec_micros().to_le_bytes())?;
        open.file.write_all(&length.to_le_bytes())?;
        open.file.write_all(&length.to_le_bytes())?;
        open.file.write_all(frame.data)?;
        open.bytes += record;
        open.frames += 1;
        Ok(())
    }

    /// Close the slice if it has covered its time by `now`, so a quiet link
    /// still yields finished slices.
    pub fn tick(&mut self, now: SystemTime) -> io::Result<()> {
        let due = match (&self.current, self.options.seconds) {
            (Some(open), Some(seconds)) => now
                .duration_since(open.started)
                .is_ok_and(|age| age >= Duration::from_secs(seconds)),
            _ => false,
        };
        if due {
            self.finish()?;
        }
        Ok(())
    }

    fn open(&mut self, started: SystemTime, link: LinkType) -> io::Result<()> {
        self.sequence += 1;
        let time: DateTime<Utc> = started.into();
        let name = format!(
            "{}-{}-{:06}.pcap",
            self.options.prefix,
            time.format("%Y%m%dT%H%M%SZ"),
            self.sequence
        );
        let path = self.options.dir.join(&name);
        let part = self.options.dir.join(format!("{name}{PART}"));
        let mut file = BufWriter::new(File::create(&part)?);
        file.write_all(&0xa1b2_c3d4u32.to_le_bytes())?;
        file.write_all(&2u16.to_le_bytes())?;
        file.write_all(&4u16.to_le_bytes())?;
        file.write_all(&0i32.to_le_bytes())?;
        file.write_all(&0u32.to_le_bytes())?;
        file.write_all(&SNAPLEN.to_le_bytes())?;
        file.write_all(&network(link).to_le_bytes())?;
        self.current = Some(OpenSlice {
            file,
            part,
            path,
            started,
            bytes: 24,
            link,
            frames: 0,
        });
        Ok(())
    }

    /// Close the current slice, hand it to the publisher and trim the ring.
    pub fn finish(&mut self) -> io::Result<()> {
        let Some(mut open) = self.current.take() else {
            return Ok(());
        };
        open.file.flush()?;
        drop(open.file);
        fs::rename(&open.part, &open.path)?;
        info!(
            "pcap slice {} closed: {} frames, {} bytes",
            open.path.display(),
            open.frames,
            open.bytes
        );
        if let Some(finished) = &self.finished {
            // A publisher that has stopped leaves the slice on disk.
            let _ = finished.send(open.path.clone());
        }
        self.prune()
    }

    /// Delete the oldest finished slices beyond `keep`.
    fn prune(&self) -> io::Result<()> {
        let mut finished = finished_slices(&self.options.dir, &self.options.prefix)?;
        if finished.len() > self.options.keep {
            let excess = finished.len() - self.options.keep;
            for path in finished.drain(..excess) {
                if let Err(e) = fs::remove_file(&path) {
                    warn!("cannot delete old pcap slice {}: {e}", path.display());
                }
            }
        }
        Ok(())
    }
}

impl Drop for SliceWriter {
    fn drop(&mut self) {
        if let Err(e) = self.finish() {
            warn!("cannot close the last pcap slice: {e}");
        }
    }
}

/// Finished slices of `prefix` in `dir`, oldest first.
pub fn finished_slices(dir: &Path, prefix: &str) -> io::Result<Vec<PathBuf>> {
    let mut found: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(&format!("{prefix}-")) && name.ends_with(".pcap")
                })
        })
        .collect();
    found.sort();
    Ok(found)
}

/// A frame source that also writes every frame it yields to a slice. The
/// last slice is closed when the source is dropped, or by [`TeeSource::finish`].
pub struct TeeSource {
    inner: Box<dyn FrameSource>,
    writer: SliceWriter,
    failed: bool,
}

impl TeeSource {
    pub fn new(inner: Box<dyn FrameSource>, writer: SliceWriter) -> Self {
        TeeSource {
            inner,
            writer,
            failed: false,
        }
    }

    /// Close the last slice.
    pub fn finish(mut self) -> io::Result<()> {
        self.writer.finish()
    }
}

impl FrameSource for TeeSource {
    fn next_frame(&mut self) -> Result<Option<Frame<'_>>, SourceError> {
        // The frame borrows `inner`; it is written through split borrows of
        // the other fields before it is returned.
        let TeeSource {
            inner,
            writer,
            failed,
        } = self;
        let frame = inner.next_frame()?;
        // A full disk must not stop decoding: a write error is reported once
        // and slices are tried again with the next frame.
        let result = match &frame {
            Some(frame) => writer.write(frame),
            None => writer.tick(SystemTime::now()),
        };
        match result {
            Err(e) if !*failed => {
                warn!(
                    "cannot write pcap slice under {}: {e}",
                    writer.options().dir.display()
                );
                *failed = true;
            }
            Err(_) => {}
            Ok(()) => *failed = false,
        }
        Ok(frame)
    }

    fn finished(&self) -> bool {
        self.inner.finished()
    }

    fn describe(&self) -> String {
        format!(
            "{}, writing pcap slices to {}",
            self.inner.describe(),
            self.writer.options().dir.display()
        )
    }

    fn stats(&mut self) -> Vec<CaptureStats> {
        self.inner.stats()
    }
}

/// Where and how finished slices are published.
#[derive(Debug, Clone)]
pub struct PublishOptions {
    /// Base URL of the Zenoh REST interface, `http://host:port`.
    pub url: String,
    /// Zenoh key expression the slice is put on.
    pub key: String,
    /// Attempts after the first.
    pub retries: u32,
    /// Wait before the first retry; doubled for each one after.
    pub first_wait: Duration,
}

/// `key` with `{host}` replaced by this machine's host name.
pub fn expand_key(key: &str) -> String {
    if !key.contains("{host}") {
        return key.to_string();
    }
    key.replace("{host}", &host_name())
}

fn host_name() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|out| String::from_utf8(out.stdout).ok())
        })
        .or_else(|| std::env::var("HOSTNAME").ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "unknown-host".to_string())
}

/// Check the publish settings before capture starts.
pub fn check_publish(options: &PublishOptions) -> Result<(), String> {
    let url =
        reqwest::Url::parse(&options.url).map_err(|_| "--pcap-publish is not a URL".to_string())?;
    if url.scheme() != "http" {
        return Err(
            "--pcap-publish takes an http:// URL: the REST port of the local Zenoh island"
                .to_string(),
        );
    }
    if options.key.is_empty() || options.key.starts_with('/') || options.key.ends_with('/') {
        return Err("--pcap-key must be a key expression such as sensor/pcap/{host}".to_string());
    }
    Ok(())
}

/// Sends finished slices from a background thread.
pub struct SlicePublisher {
    sender: Option<Sender<PathBuf>>,
    handle: Option<JoinHandle<PublishCounts>>,
}

/// What the publisher did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PublishCounts {
    pub published: u64,
    pub failed: u64,
}

impl SlicePublisher {
    pub fn spawn(options: PublishOptions) -> Result<Self, String> {
        check_publish(&options)?;
        // The HTTP client needs the process's crypto provider even for plain
        // HTTP; `mind` installs it at start, and this call returns that one.
        static ACTIVATED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
        ACTIVATED
            .get_or_init(|| {
                flow::security::activate()
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
            .clone()
            .map_err(|e| format!("pcap publisher: {e}"))?;
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("pcap publisher: {e}"))?;
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || publish_loop(&client, &options, &receiver));
        Ok(SlicePublisher {
            sender: Some(sender),
            handle: Some(handle),
        })
    }

    /// The sender a [`SliceWriter`] hands finished slices to.
    pub fn sender(&self) -> Sender<PathBuf> {
        self.sender.clone().expect("present until finish")
    }

    /// Stop taking slices, wait up to `timeout` for the queue to be sent.
    /// `None` if the thread did not finish in time.
    pub fn finish(mut self, timeout: Duration) -> Option<PublishCounts> {
        self.sender.take();
        let handle = self.handle.take()?;
        let deadline = Instant::now() + timeout;
        while !handle.is_finished() {
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(20));
        }
        handle.join().ok()
    }
}

fn publish_loop(
    client: &reqwest::blocking::Client,
    options: &PublishOptions,
    receiver: &Receiver<PathBuf>,
) -> PublishCounts {
    let url = format!("{}/{}", options.url.trim_end_matches('/'), options.key);
    let mut counts = PublishCounts::default();
    for path in receiver {
        let body = match fs::read(&path) {
            Ok(body) => body,
            Err(e) => {
                warn!("pcap slice {} not published: {e}", path.display());
                counts.failed += 1;
                continue;
            }
        };
        let mut wait = options.first_wait;
        let mut attempt = 0;
        loop {
            let outcome = client
                .put(&url)
                .header(reqwest::header::CONTENT_TYPE, PCAP_CONTENT_TYPE)
                .body(body.clone())
                .send()
                .map_err(|e| e.without_url().to_string())
                .and_then(|response| {
                    let status = response.status();
                    if status.is_success() {
                        Ok(())
                    } else {
                        Err(format!("HTTP {status}"))
                    }
                });
            match outcome {
                Ok(()) => {
                    info!(
                        "pcap slice {} published to key {} ({} bytes)",
                        path.display(),
                        options.key,
                        body.len()
                    );
                    counts.published += 1;
                    break;
                }
                Err(reason) if attempt < options.retries => {
                    warn!(
                        "pcap slice {} not published ({reason}); retrying in {wait:?}",
                        path.display()
                    );
                    thread::sleep(wait);
                    wait = wait.saturating_mul(2);
                    attempt += 1;
                }
                Err(reason) => {
                    warn!(
                        "pcap slice {} not published after {} attempts ({reason}); it stays on disk",
                        path.display(),
                        attempt + 1
                    );
                    counts.failed += 1;
                    break;
                }
            }
        }
    }
    counts
}
