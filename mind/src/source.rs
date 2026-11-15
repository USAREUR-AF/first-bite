//! Where frames come from: a live interface, a capture file, or a UDP socket.

use std::net::UdpSocket;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How the bytes of a frame start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkType {
    Ethernet,
    /// An IPv4 or IPv6 header with nothing in front of it.
    RawIp,
}

/// One captured frame, borrowed from the source that produced it.
#[derive(Debug)]
pub struct Frame<'a> {
    pub timestamp: SystemTime,
    pub link: LinkType,
    pub data: &'a [u8],
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("capture: {0}")]
    Pcap(#[from] pcap::Error),
    #[error("socket: {0}")]
    Io(#[from] std::io::Error),
    #[error("link type {name} ({number}) is not supported; capture on an Ethernet or raw-IP link")]
    UnsupportedLink { name: String, number: i32 },
    #[error("capture on {device} stopped: {reason}")]
    CaptureStopped { device: String, reason: String },
}

/// What a live capture has seen and lost since it started.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureStats {
    pub device: String,
    /// Frames the kernel matched to this capture, counted before any loss.
    /// On a loopback interface each packet is counted twice.
    pub received: u64,
    /// Frames the kernel dropped because the capture buffer was full.
    pub kernel_dropped: u64,
    /// Frames the interface or its driver dropped.
    pub interface_dropped: u64,
    /// Frames dropped in this process because the pipeline was behind.
    pub queue_dropped: u64,
}

impl CaptureStats {
    pub fn dropped(&self) -> u64 {
        self.kernel_dropped + self.interface_dropped + self.queue_dropped
    }
}

/// A stream of frames.
pub trait FrameSource {
    /// The next frame. `Ok(None)` means no frame arrived within the source's
    /// poll interval, or the source has ended; check [`FrameSource::finished`].
    fn next_frame(&mut self) -> Result<Option<Frame<'_>>, SourceError>;

    /// True once the source can produce no more frames.
    fn finished(&self) -> bool {
        false
    }

    /// One line for the log saying what is being read.
    fn describe(&self) -> String;

    /// Loss counters, one entry per interface. Empty for sources that cannot
    /// lose frames.
    fn stats(&mut self) -> Vec<CaptureStats> {
        Vec::new()
    }
}

/// Settings shared by every live capture.
#[derive(Debug, Clone)]
pub struct CaptureOptions {
    /// See frames addressed to other hosts. A tap or mirror port needs this.
    pub promiscuous: bool,
    /// BPF expression applied in the kernel.
    pub filter: Option<String>,
    /// Kernel capture buffer per interface, in bytes.
    pub buffer_bytes: u32,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        CaptureOptions {
            promiscuous: true,
            filter: None,
            buffer_bytes: 32 * 1024 * 1024,
        }
    }
}

/// How long a live source waits for a frame before returning `Ok(None)`.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

fn link_type(datalink: pcap::Linktype) -> Result<LinkType, SourceError> {
    match datalink {
        pcap::Linktype::ETHERNET => Ok(LinkType::Ethernet),
        pcap::Linktype::RAW | pcap::Linktype::IPV4 | pcap::Linktype::IPV6 => Ok(LinkType::RawIp),
        other => Err(SourceError::UnsupportedLink {
            name: other.get_name().unwrap_or_else(|_| "unknown".to_string()),
            number: other.0,
        }),
    }
}

fn timestamp(header: &pcap::PacketHeader) -> SystemTime {
    let seconds = u64::try_from(header.ts.tv_sec).unwrap_or(0);
    let micros = u32::try_from(header.ts.tv_usec).unwrap_or(0).min(999_999);
    UNIX_EPOCH + Duration::new(seconds, micros * 1_000)
}

/// Frames from a network interface.
pub struct LiveCapture {
    capture: pcap::Capture<pcap::Active>,
    link: LinkType,
    device: String,
}

impl LiveCapture {
    /// Open `device`.
    pub fn open(device: &str, options: &CaptureOptions) -> Result<Self, SourceError> {
        let mut capture = pcap::Capture::from_device(device)?
            .promisc(options.promiscuous)
            .immediate_mode(true)
            .buffer_size(i32::try_from(options.buffer_bytes).unwrap_or(i32::MAX))
            .timeout(POLL_INTERVAL.as_millis() as i32)
            .open()?;
        if let Some(filter) = &options.filter {
            capture.filter(filter, true)?;
        }
        let link = link_type(capture.get_datalink())?;
        Ok(LiveCapture {
            capture,
            link,
            device: device.to_string(),
        })
    }

    fn capture_stats(&mut self) -> CaptureStats {
        let mut stats = CaptureStats {
            device: self.device.clone(),
            ..CaptureStats::default()
        };
        if let Ok(kernel) = self.capture.stats() {
            stats.received = u64::from(kernel.received);
            stats.kernel_dropped = u64::from(kernel.dropped);
            stats.interface_dropped = u64::from(kernel.if_dropped);
        }
        stats
    }
}

impl FrameSource for LiveCapture {
    fn next_frame(&mut self) -> Result<Option<Frame<'_>>, SourceError> {
        match self.capture.next_packet() {
            Ok(packet) => Ok(Some(Frame {
                timestamp: timestamp(packet.header),
                link: self.link,
                data: packet.data,
            })),
            Err(pcap::Error::TimeoutExpired) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn describe(&self) -> String {
        format!("interface {}", self.device)
    }

    fn stats(&mut self) -> Vec<CaptureStats> {
        vec![self.capture_stats()]
    }
}

/// Frames waiting between the capture threads and the pipeline.
const QUEUE_FRAMES: usize = 65_536;
/// How often a capture thread refreshes its kernel counters.
const STATS_INTERVAL: Duration = Duration::from_secs(1);

struct QueuedFrame {
    timestamp: SystemTime,
    link: LinkType,
    data: Vec<u8>,
}

#[derive(Default)]
struct SharedStats {
    received: AtomicU64,
    kernel_dropped: AtomicU64,
    interface_dropped: AtomicU64,
    queue_dropped: AtomicU64,
}

/// Frames from several interfaces at once, in arrival order.
///
/// A passive tap presents each direction of the tapped link on its own port,
/// so seeing both directions means capturing on two interfaces. Each interface
/// has a capture thread; frames meet in one bounded queue. If the pipeline
/// falls behind, new frames are dropped and counted rather than held, so a
/// stall never grows memory without bound.
pub struct MultiCapture {
    receiver: Receiver<Result<QueuedFrame, SourceError>>,
    current: Option<QueuedFrame>,
    devices: Vec<String>,
    stats: Vec<Arc<SharedStats>>,
}

impl MultiCapture {
    /// Open every device. Fails if any one of them cannot be opened.
    pub fn open(devices: &[String], options: &CaptureOptions) -> Result<Self, SourceError> {
        let (sender, receiver) = sync_channel(QUEUE_FRAMES);
        let mut stats = Vec::new();
        for device in devices {
            let capture = LiveCapture::open(device, options)?;
            let shared = Arc::new(SharedStats::default());
            stats.push(Arc::clone(&shared));
            let sender = sender.clone();
            thread::Builder::new()
                .name(format!("capture-{device}"))
                .spawn(move || capture_thread(capture, sender, shared))?;
        }
        Ok(MultiCapture {
            receiver,
            current: None,
            devices: devices.to_vec(),
            stats,
        })
    }
}

fn capture_thread(
    mut capture: LiveCapture,
    sender: SyncSender<Result<QueuedFrame, SourceError>>,
    shared: Arc<SharedStats>,
) {
    let mut refreshed = Instant::now();
    loop {
        let queued = match capture.next_frame() {
            Ok(Some(frame)) => Some(QueuedFrame {
                timestamp: frame.timestamp,
                link: frame.link,
                data: frame.data.to_vec(),
            }),
            Ok(None) => None,
            Err(e) => {
                // A tap that silently loses one direction is worse than one
                // that stops: report the failure to the pipeline and end.
                let _ = sender.send(Err(SourceError::CaptureStopped {
                    device: capture.device.clone(),
                    reason: e.to_string(),
                }));
                return;
            }
        };
        if let Some(frame) = queued {
            match sender.try_send(Ok(frame)) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    shared.queue_dropped.fetch_add(1, Ordering::Relaxed);
                }
                Err(TrySendError::Disconnected(_)) => return,
            }
        }
        if refreshed.elapsed() >= STATS_INTERVAL {
            refreshed = Instant::now();
            let stats = capture.capture_stats();
            shared.received.store(stats.received, Ordering::Relaxed);
            shared
                .kernel_dropped
                .store(stats.kernel_dropped, Ordering::Relaxed);
            shared
                .interface_dropped
                .store(stats.interface_dropped, Ordering::Relaxed);
        }
    }
}

impl FrameSource for MultiCapture {
    fn next_frame(&mut self) -> Result<Option<Frame<'_>>, SourceError> {
        match self.receiver.recv_timeout(POLL_INTERVAL) {
            Ok(Ok(frame)) => {
                let frame = self.current.insert(frame);
                Ok(Some(Frame {
                    timestamp: frame.timestamp,
                    link: frame.link,
                    data: &frame.data,
                }))
            }
            Ok(Err(e)) => Err(e),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(SourceError::CaptureStopped {
                device: self.devices.join(", "),
                reason: "every capture thread has ended".to_string(),
            }),
        }
    }

    fn describe(&self) -> String {
        format!("interfaces {}", self.devices.join(", "))
    }

    fn stats(&mut self) -> Vec<CaptureStats> {
        self.devices
            .iter()
            .zip(&self.stats)
            .map(|(device, shared)| CaptureStats {
                device: device.clone(),
                received: shared.received.load(Ordering::Relaxed),
                kernel_dropped: shared.kernel_dropped.load(Ordering::Relaxed),
                interface_dropped: shared.interface_dropped.load(Ordering::Relaxed),
                queue_dropped: shared.queue_dropped.load(Ordering::Relaxed),
            })
            .collect()
    }
}

/// Frames from a pcap or pcapng file, read once from start to end.
pub struct FileCapture {
    capture: pcap::Capture<pcap::Offline>,
    link: LinkType,
    path: String,
    finished: bool,
}

impl FileCapture {
    pub fn open(path: &Path, filter: Option<&str>) -> Result<Self, SourceError> {
        let mut capture = pcap::Capture::from_file(path)?;
        if let Some(filter) = filter {
            capture.filter(filter, true)?;
        }
        let link = link_type(capture.get_datalink())?;
        Ok(FileCapture {
            capture,
            link,
            path: path.display().to_string(),
            finished: false,
        })
    }
}

impl FrameSource for FileCapture {
    fn next_frame(&mut self) -> Result<Option<Frame<'_>>, SourceError> {
        match self.capture.next_packet() {
            Ok(packet) => Ok(Some(Frame {
                timestamp: timestamp(packet.header),
                link: self.link,
                data: packet.data,
            })),
            Err(pcap::Error::NoMorePackets) => {
                self.finished = true;
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }

    fn finished(&self) -> bool {
        self.finished
    }

    fn describe(&self) -> String {
        format!("capture file {}", self.path)
    }
}

/// Ethernet frames delivered one per UDP datagram. This is the `--mock` mode:
/// it needs no capture privileges, so it runs in containers and CI.
pub struct UdpFrames {
    socket: UdpSocket,
    buffer: Vec<u8>,
    address: String,
}

impl UdpFrames {
    pub fn bind(address: &str) -> Result<Self, SourceError> {
        let socket = UdpSocket::bind(address)?;
        socket.set_read_timeout(Some(POLL_INTERVAL))?;
        Ok(UdpFrames {
            socket,
            buffer: vec![0u8; 65_535],
            address: address.to_string(),
        })
    }
}

impl FrameSource for UdpFrames {
    fn next_frame(&mut self) -> Result<Option<Frame<'_>>, SourceError> {
        match self.socket.recv_from(&mut self.buffer) {
            Ok((length, _)) => Ok(Some(Frame {
                timestamp: SystemTime::now(),
                link: LinkType::Ethernet,
                data: &self.buffer[..length],
            })),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }

    fn describe(&self) -> String {
        format!("UDP socket {}", self.address)
    }
}
