use clap::Parser;
use std::path::PathBuf;

/// Capture packets, decode them against a dictionary, publish the records.
#[derive(Parser, Debug)]
#[command(name = "mind", version, about)]
pub struct Opt {
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

    /// Address the --mock socket binds.
    #[arg(long, value_name = "ADDR", default_value = "0.0.0.0:12345")]
    pub mock_address: String,

    /// Dictionary directory. Overrides BITE_DICT_PATH.
    #[arg(long, value_name = "DIR")]
    pub dictionary: Option<PathBuf>,

    /// BPF filter applied to the capture, for example "udp port 40100".
    #[arg(long, value_name = "EXPR")]
    pub filter: Option<String>,

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
}
