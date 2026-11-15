use crate::source::SourceError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("env file {path}: {reason}")]
    EnvFile { path: String, reason: String },
    #[error("cannot create output file {path}: {source}")]
    OutputFile {
        path: String,
        source: std::io::Error,
    },
    #[error("cannot write output file: {0}")]
    OutputWrite(#[from] std::io::Error),
    #[error("publisher configuration: {0}")]
    PublisherConfig(String),
    #[error("dictionary: {0}")]
    Dictionary(#[from] bite::LoadError),
    #[error("FIRST_BITE_RECORD_VERSION is {0:?}; it must be 2 (typed values, the default) or 1 (every value as text)")]
    RecordVersion(String),
    #[error("give a network interface, --pcap-file or --mock")]
    NoSource,
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error("cannot serialise record: {0}")]
    Serialise(#[from] serde_json::Error),
}
