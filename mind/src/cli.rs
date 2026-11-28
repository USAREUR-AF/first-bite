use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// Capture packets, decode them against a dictionary, publish the records.
#[derive(Parser, Debug)]
#[command(name = "mind", version, about)]
pub struct Opt {
    /// Without a subcommand `mind` captures; the options below are the
    /// capture run's.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Network interfaces to capture on. Give two for a tap that presents each
    /// direction of the link on its own port.
    pub device_name: Vec<String>,

    /// Read frames from a pcap or pcapng file instead of an interface, then exit.
    #[arg(long, value_name = "FILE", conflicts_with_all = ["device_name", "mock"])]
    pub pcap_file: Option<PathBuf>,

    /// Read Ethernet frames from a UDP socket, one per datagram, instead of an
    /// interface. Needs no capture privileges.
    #[arg(long, conflicts_with = "device_name")]
    pub mock: bool,

    /// Address the --mock socket binds. Every datagram that reaches it is
    /// taken as a frame, so the default accepts them from this machine only.
    #[arg(long, value_name = "ADDR", default_value = "127.0.0.1:12345")]
    pub mock_address: String,

    /// Dictionary directory. Overrides BITE_DICT_PATH.
    #[arg(long, value_name = "DIR")]
    pub dictionary: Option<PathBuf>,

    /// BPF filter applied to the capture, for example "udp port 40100".
    #[arg(long, value_name = "EXPR")]
    pub filter: Option<String>,

    /// UDP destination port that carries VXLAN-wrapped mirrored traffic
    /// (commonly 4789). May be given more than once. GRE and ERSPAN mirrors
    /// need no option.
    #[arg(long, value_name = "PORT")]
    pub vxlan_port: Vec<u16>,

    /// Do not unwrap GRE or ERSPAN; publish only packets that are UDP or TCP
    /// as captured.
    #[arg(long)]
    pub no_gre_decap: bool,

    /// Capture only frames addressed to this host.
    #[arg(long)]
    pub no_promiscuous: bool,

    /// Kernel capture buffer per interface, in MiB. Raise it if the log
    /// reports frames dropped by the kernel.
    #[arg(long, value_name = "MIB", default_value_t = 32)]
    pub buffer_mb: u32,

    /// Seconds between capture-loss reports in the log; 0 turns them off.
    #[arg(long, value_name = "SECONDS", default_value_t = 60)]
    pub stats_interval: u64,

    /// File of KEY=VALUE lines loaded into the environment before anything
    /// else is read from it. The name must end in `.env`.
    #[arg(long, value_name = "FILE")]
    pub env_file: Option<String>,

    /// Also write every record, pretty-printed, to completelog_<time>.log in
    /// the working directory.
    #[arg(long)]
    pub output: bool,

    /// Start the subscriber/publisher pairs from the subpub configuration.
    #[arg(long)]
    pub activate_subpubs: bool,

    /// Also write every captured frame (after --filter) to rotating pcap files
    /// in this directory: `<prefix>-<UTC time>-<n>.pcap`. A file being
    /// written ends in `.pcap.part`.
    #[arg(long, value_name = "DIR")]
    pub pcap_dir: Option<PathBuf>,

    /// Close a pcap slice once it covers this many seconds.
    #[arg(
        long,
        value_name = "SECONDS",
        default_value_t = 60,
        requires = "pcap_dir"
    )]
    pub pcap_slice_seconds: u64,

    /// Also close a pcap slice before it would pass this many MiB.
    #[arg(long, value_name = "MIB", requires = "pcap_dir")]
    pub pcap_slice_mb: Option<u64>,

    /// Finished pcap slices kept on disk; the oldest beyond this are deleted.
    #[arg(long, value_name = "N", default_value_t = 60, requires = "pcap_dir")]
    pub pcap_keep: usize,

    /// File name prefix of the pcap slices.
    #[arg(
        long,
        value_name = "NAME",
        default_value = "capture",
        requires = "pcap_dir"
    )]
    pub pcap_prefix: String,

    /// Publish each finished pcap slice with an HTTP PUT to the REST port of
    /// a Zenoh router, for example the local island: http://127.0.0.1:8000.
    #[arg(long, value_name = "URL", requires_all = ["pcap_dir", "pcap_key"])]
    pub pcap_publish: Option<String>,

    /// Zenoh key a published slice is put on. `{host}` is replaced by the
    /// host name, for example sensor/pcap/{host}.
    #[arg(long, value_name = "KEY", requires = "pcap_publish")]
    pub pcap_key: Option<String>,

    /// Retries of a failed pcap PUT, 2 s apart and doubling.
    #[arg(long, value_name = "N", default_value_t = 5, requires = "pcap_publish")]
    pub pcap_publish_retries: u32,

    /// Only write (and publish) pcap slices: decode nothing, publish no
    /// records, bind no record socket.
    #[arg(long, requires = "pcap_dir")]
    pub tap_only: bool,

    /// Seconds the sinks are given, once input has ended or the process is
    /// told to stop, to deliver the records already published. What is not
    /// delivered by then is counted as abandoned and the exit status is 3.
    #[arg(long, value_name = "SECONDS", default_value_t = 30)]
    pub drain_seconds: u64,
}

/// What `mind` does instead of capturing.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Report or check transport security, without capturing or connecting.
    Security(SecurityOpt),
    /// Survey an unknown sensor's traffic from a capture file or, for a set
    /// time, an interface: talkers, ARP, flows, payload shapes. Sends nothing.
    Discover(DiscoverOpt),
    /// Read-only checks of the link: is it a hub, which hosts are clients of a
    /// server port, and follow a server's stream off the wire. Sends nothing.
    Lan(LanOpt),
}

#[derive(Args, Debug)]
pub struct LanOpt {
    #[command(subcommand)]
    pub action: LanAction,
}

#[derive(Subcommand, Debug)]
pub enum LanAction {
    /// Does this port receive unicast between other hosts (a hub or a mirror
    /// port), so other hosts' traffic can be captured here?
    Hub(LanHubOpt),
    /// The TCP clients of one server endpoint: connects, who ended each
    /// connection, bytes. Shows clients taking turns on a server.
    Clients(LanClientsOpt),
    /// Copy one server's TCP stream to its client off the wire, in order, and
    /// serve it on a local port as a stand-in for the server, never connecting
    /// to the server.
    Follow(LanFollowOpt),
}

#[derive(Args, Debug)]
pub struct LanSource {
    /// A pcap or pcapng file to read.
    #[arg(
        value_name = "FILE",
        required_unless_present = "iface",
        conflicts_with = "iface"
    )]
    pub pcap: Option<PathBuf>,

    /// Capture on this interface instead (needs capture privileges).
    #[arg(long, value_name = "NAME")]
    pub iface: Option<String>,

    /// How long to capture on --iface; 0 runs until stopped.
    #[arg(long, value_name = "SECONDS", default_value_t = 30)]
    pub seconds: u64,

    /// BPF filter applied in the kernel.
    #[arg(long, value_name = "EXPR")]
    pub filter: Option<String>,
}

#[derive(Args, Debug)]
pub struct LanHubOpt {
    #[command(flatten)]
    pub source: LanSource,

    /// The capturing port's MAC address; read from --iface when not given.
    #[arg(long, value_name = "MAC")]
    pub local_mac: Option<String>,

    /// Below this many frames, with no other hosts' unicast seen, the verdict
    /// is undetermined rather than switch.
    #[arg(long, value_name = "N", default_value_t = 50)]
    pub min_frames: u64,

    /// Print the report as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct LanClientsOpt {
    #[command(flatten)]
    pub source: LanSource,

    /// The server endpoint, IPv4:PORT.
    #[arg(long, value_name = "ADDR:PORT")]
    pub server: std::net::SocketAddrV4,

    /// Print the report as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct LanFollowOpt {
    #[command(flatten)]
    pub source: LanSource,

    /// The server endpoint whose stream to follow, IPv4:PORT.
    #[arg(long, value_name = "ADDR:PORT")]
    pub server: std::net::SocketAddrV4,

    /// Serve the stream on this TCP address, for example 127.0.0.1:17003.
    #[arg(long, value_name = "ADDR:PORT")]
    pub listen: Option<String>,

    /// Also write the stream to this file.
    #[arg(long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Client addresses never followed; this interface's own are added.
    #[arg(long, value_name = "IPV4")]
    pub ignore: Vec<std::net::Ipv4Addr>,

    /// Wait this long for a missing segment before skipping the gap.
    #[arg(long, value_name = "MS", default_value_t = 1000)]
    pub gap_wait_ms: u64,

    /// Follow another client's stream once the current one has been quiet
    /// this long.
    #[arg(long, value_name = "MS", default_value_t = 5000)]
    pub switch_idle_ms: u64,

    /// Print the counters as JSON at the end.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct DiscoverOpt {
    /// A pcap or pcapng file to survey.
    #[arg(
        value_name = "FILE",
        required_unless_present = "iface",
        conflicts_with = "iface"
    )]
    pub pcap: Option<PathBuf>,

    /// Capture on this interface instead (needs capture privileges).
    #[arg(long, value_name = "NAME")]
    pub iface: Option<String>,

    /// How long to capture on --iface.
    #[arg(long, value_name = "SECONDS", default_value_t = 30)]
    pub seconds: u64,

    /// BPF filter, for example "not port 22".
    #[arg(long, value_name = "EXPR")]
    pub filter: Option<String>,

    /// Also decode flows on a port a dictionary interface claims, and count
    /// their message types.
    #[arg(long, value_name = "DIR")]
    pub dictionary: Option<PathBuf>,

    /// Print the survey as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct SecurityOpt {
    #[command(subcommand)]
    pub action: SecurityAction,
}

#[derive(Subcommand, Debug)]
pub enum SecurityAction {
    /// Print what every publisher and sink will use: transport, protocol,
    /// minimum TLS version, key-exchange groups, client authentication, the
    /// crypto provider and whether the FIPS module is active.
    Show(SecurityShow),
    /// Exit non-zero, with the messages a capture run would stop with, if
    /// any path does not meet the security block.
    Check(SecurityCommon),
}

#[derive(Args, Debug)]
pub struct SecurityCommon {
    /// The capture run's env file, so the same configuration is read.
    #[arg(long, value_name = "FILE")]
    pub env_file: Option<String>,

    /// Include the publishers of the subpub configuration, as a capture run
    /// with --activate-subpubs would bind them.
    #[arg(long)]
    pub activate_subpubs: bool,
}

#[derive(Args, Debug)]
pub struct SecurityShow {
    #[command(flatten)]
    pub common: SecurityCommon,

    /// Print the report as JSON.
    #[arg(long)]
    pub json: bool,

    /// Perform one TLS handshake with this subscriber's endpoint and report
    /// the negotiated version, cipher suite and key-exchange group. No
    /// record is sent.
    #[arg(long, value_name = "SUBSCRIBER")]
    pub probe: Option<String>,
}
