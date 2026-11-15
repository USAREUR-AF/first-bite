//! The capture loop: frame in, record out.

use crate::error::Error;
use crate::pipeline::{Pipeline, Skip};
use crate::source::{CaptureStats, FrameSource};
use chrono::Utc;
use datamodels::packets::PacketInfoWithParsedMessage;
use log::{debug, info, log_enabled, warn, Level};
use serde::Serialize;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Kind of a structured log line. Log shippers key on this.
#[derive(Serialize, Debug)]
pub enum MessageType {
    Misc,
    Packet,
    Telemetry,
    Heartbeat,
}

/// A structured log line: `{"message_type": ..., "message": ...}`.
#[derive(Serialize, Debug)]
pub struct LogMessage<'a, T: Serialize> {
    pub message_type: MessageType,
    pub message: &'a T,
}

/// Where records go besides the log.
pub trait RecordSink {
    fn publish(&mut self, record: &PacketInfoWithParsedMessage, json: &str);
}

impl<F: FnMut(&PacketInfoWithParsedMessage, &str)> RecordSink for F {
    fn publish(&mut self, record: &PacketInfoWithParsedMessage, json: &str) {
        self(record, json)
    }
}

/// Counts kept by the loop.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub frames: u64,
    pub records: u64,
    pub decoded: u64,
    pub skipped: u64,
    pub malformed: u64,
}

/// The file `--output` writes: one pretty-printed record after another.
pub struct OutputFile {
    writer: BufWriter<File>,
}

impl OutputFile {
    /// Create `completelog_<UTC time>.log` in the working directory.
    pub fn create() -> Result<Self, Error> {
        let path = format!("completelog_{}.log", Utc::now().format("%Y%m%d%H%M%S"));
        let file = File::create(&path).map_err(|source| Error::OutputFile {
            path: path.clone(),
            source,
        })?;
        info!("writing records to {path}");
        Ok(OutputFile {
            writer: BufWriter::new(file),
        })
    }

    fn write(&mut self, record: &PacketInfoWithParsedMessage) -> Result<(), Error> {
        serde_json::to_writer_pretty(&mut self.writer, record)?;
        self.writer.write_all(b"\n")?;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Error> {
        Ok(self.writer.flush()?)
    }
}

/// Read frames until the source ends or fails, or `stop` is set.
///
/// A live source never ends by itself; setting `stop` (the process does so on
/// SIGINT and SIGTERM) ends the loop within one poll interval, after which the
/// output file is flushed and the loss counters are reported. A file source
/// returns the counts once the file is exhausted.
pub fn run(
    source: &mut dyn FrameSource,
    pipeline: &Pipeline,
    sink: &mut dyn RecordSink,
    mut output: Option<&mut OutputFile>,
    stats_interval: Option<Duration>,
    stop: &AtomicBool,
) -> Result<Stats, Error> {
    let mut stats = Stats::default();
    let mut loss = LossReport::new(stats_interval);
    while !stop.load(Ordering::Relaxed) {
        loss.maybe_report(source);
        let Some(frame) = source.next_frame()? else {
            if source.finished() {
                break;
            }
            // Nothing arrived within the poll interval; keep the output file
            // current while the link is quiet.
            if let Some(output) = output.as_deref_mut() {
                output.flush()?;
            }
            continue;
        };
        stats.frames += 1;
        let record = match pipeline.process(&frame) {
            Ok(record) => record,
            Err(Skip::NotTransport) => {
                stats.skipped += 1;
                continue;
            }
            Err(Skip::Malformed(reason)) => {
                stats.malformed += 1;
                debug!("malformed frame of {} bytes: {reason}", frame.data.len());
                continue;
            }
        };
        stats.records += 1;
        if record.message.is_some() {
            stats.decoded += 1;
        }

        let json = serde_json::to_string(&record)?;
        if log_enabled!(Level::Info) {
            // The record is embedded as raw JSON rather than serialised twice.
            info!(r#"{{"message_type":"Packet","message":{json}}}"#);
        }
        sink.publish(&record, &json);
        if let Some(output) = output.as_deref_mut() {
            output.write(&record)?;
        }
    }
    if let Some(output) = output {
        output.flush()?;
    }
    loss.report(source);
    if stats.malformed > 0 {
        warn!("{} malformed frames were dropped", stats.malformed);
    }
    Ok(stats)
}

/// Logs capture loss: periodically at info, and at warn whenever the number of
/// dropped frames has grown since the last report.
struct LossReport {
    interval: Option<Duration>,
    last: Instant,
    dropped_before: u64,
}

impl LossReport {
    fn new(interval: Option<Duration>) -> Self {
        LossReport {
            interval,
            last: Instant::now(),
            dropped_before: 0,
        }
    }

    fn maybe_report(&mut self, source: &mut dyn FrameSource) {
        if self
            .interval
            .is_some_and(|interval| self.last.elapsed() >= interval)
        {
            self.report(source);
        }
    }

    fn report(&mut self, source: &mut dyn FrameSource) {
        self.last = Instant::now();
        let all = source.stats();
        if all.is_empty() {
            return;
        }
        let dropped: u64 = all.iter().map(CaptureStats::dropped).sum();
        for stats in &all {
            let line = format!(
                "capture {}: {} seen by kernel, {} dropped by kernel, {} by interface, {} in queue",
                stats.device,
                stats.received,
                stats.kernel_dropped,
                stats.interface_dropped,
                stats.queue_dropped
            );
            if dropped > self.dropped_before {
                warn!("{line}");
            } else {
                info!("{line}");
            }
        }
        self.dropped_before = dropped;
    }
}
