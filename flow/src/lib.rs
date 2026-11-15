//! Egress for first-bite: publish decoded records on local ZeroMQ sockets and
//! forward them from there to configured sinks.
//!
//! * [`Publishers`] binds the sockets and publishes each record.
//! * [`Subscribers`] reads those sockets and hands records to [`sink::Sink`]s:
//!   object storage, HTTP, local files, Zenoh.
//! * [`SubPubs`] reads them, transforms each record and publishes the result
//!   on a second socket.
//! * [`security`] holds every one of those paths to the level the
//!   configuration states.
//!
//! A record is JSON text inside the process; each publisher and sink writes
//! it as JSON or CBOR, as its `encoding` key says (see [`encoding`]).
//!
//! All three are values the caller owns. Each is built from its YAML file
//! (see [`config`]) and checked in full before anything runs, so a
//! configuration mistake stops startup with an error that names the entry.

pub mod config;
pub mod encoding;
pub mod error;
mod feed;
#[cfg(feature = "goat")]
pub mod goat;
pub mod publish;
pub mod secret;
pub mod security;
pub mod sink;
pub mod subpub;
pub mod subscribe;
#[cfg(test)]
mod test_support;

pub use crate::config::{PublisherConfig, SubPubConfig, SubscriberConfig};
pub use crate::encoding::{Encoding, EncodingError};
pub use crate::error::{ConfigError, Error, Problem};
pub use crate::publish::{PublishError, Publisher, Publishers};
pub use crate::secret::Secret;
pub use crate::sink::{build_sink, build_sink_secured, describe_sink, Sink, SinkError};
pub use crate::subpub::{SubPubs, Transform, TransformError};
pub use crate::subscribe::Subscribers;
