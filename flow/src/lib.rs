//! Egress for first-bite: publish decoded records on local ZeroMQ sockets and
//! forward them from there to configured sinks.
//!
//! * [`Publishers`] binds the sockets and publishes each record.
//! * [`Subscribers`] reads those sockets and hands records to [`sink::Sink`]s:
//!   object storage, HTTP, local files, Zenoh.
//! * [`SubPubs`] reads them, transforms each record and publishes the result
//!   on a second socket.
//!
//! All three are values the caller owns. Each is built from its YAML file
//! (see [`config`]) and checked in full before anything runs, so a
//! configuration mistake stops startup with an error that names the entry.

pub mod config;
pub mod error;
mod feed;
pub mod publish;
pub mod sink;
pub mod subpub;
pub mod subscribe;
#[cfg(test)]
mod test_support;

pub use crate::config::{PublisherConfig, SubPubConfig, SubscriberConfig};
pub use crate::error::{ConfigError, Error, Problem};
pub use crate::publish::{PublishError, Publisher, Publishers};
pub use crate::sink::{build_sink, Sink, SinkError};
pub use crate::subpub::{SubPubs, Transform, TransformError};
pub use crate::subscribe::Subscribers;
